use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, RwLock};

use serde_json::Value;

pub type ConfigResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Settings consumed while services are started. They are snapshotted once;
/// changing `config.json` does not rebind an already running service.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupConfig {
    pub web_host: String,
    pub web_port: u64,
    pub not_open_browser: bool,
    pub auto_close: bool,
    pub mqtt_client: Option<Value>,
    pub postgresql: Option<Value>,
}

fn config_path() -> ConfigResult<PathBuf> {
    Ok(PathBuf::from(crate::cqapi::cq_get_app_directory1()?).join("config.json"))
}

fn default_config() -> Value {
    serde_json::json!({
        "web_port": 1207,
        "web_password": "",
        "readonly_web_password": "",
        "web_host": "127.0.0.1",
        "ws_urls": [],
        "not_open_browser": false
    })
}

static CONFIG_CACHE: OnceLock<RwLock<Option<Value>>> = OnceLock::new();
static CONFIG_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn config_cache() -> &'static RwLock<Option<Value>> {
    CONFIG_CACHE.get_or_init(|| RwLock::new(None))
}

fn config_write_lock() -> &'static Mutex<()> {
    CONFIG_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 原子写入：先写临时文件并 fsync 再重命名，降低断电截断/丢数据风险；
/// 跨文件系统 rename 失败时回退为直接写入。
fn atomic_write_file(path: &std::path::Path, content: &str) -> ConfigResult<()> {
    use std::io::Write;
    let tmp_path = {
        let mut p = path.as_os_str().to_owned();
        p.push(".tmp");
        std::path::PathBuf::from(p)
    };
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        let _ = file.sync_all();
    }
    // 在同目录内重命名是原子操作
    if fs::rename(&tmp_path, path).is_err() {
        let _ = fs::remove_file(&tmp_path);
        fs::write(path, content)?;
    }
    Ok(())
}

fn read_config_from_disk() -> ConfigResult<Value> {
    let path = config_path()?;
    if !path.is_file() {
        atomic_write_file(&path, &default_config().to_string())?;
    }
    let content = fs::read_to_string(&path)?;
    match serde_json::from_str(&content) {
        Ok(config) => Ok(config),
        Err(e) => {
            // 配置损坏（断电半截写入/手工改坏）时备份坏文件并回退默认配置，
            // 避免此后每次 read_config 永久失败导致登录鉴权、服务启动全部瘫痪
            let backup = {
                let mut p = path.clone().into_os_string();
                p.push(format!(
                    ".corrupt-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                ));
                std::path::PathBuf::from(p)
            };
            let renamed = fs::rename(&path, &backup).is_ok();
            let _ =
                crate::cq_add_log_w(&format!("config.json 解析失败:{e}，已{}并回退默认配置",
                    if renamed { format!("备份到 {:?}", backup) } else { "尝试回退".to_owned() }));
            let default = default_config();
            atomic_write_file(&path, &default.to_string())?;
            Ok(default)
        }
    }
}

/// Return the process-wide cached `config.json` value.
///
/// The file is read only on the first call. All field accessors in this
/// module derive their values from this single cached object; they do not
/// maintain additional field-level caches.
pub fn read_config() -> ConfigResult<Value> {
    {
        let cache = match config_cache().read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(config) = cache.as_ref() {
            return Ok(config.clone());
        }
    }

    let mut cache = match config_cache().write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(config) = cache.as_ref() {
        return Ok(config.clone());
    }
    let config = read_config_from_disk()?;
    *cache = Some(config.clone());
    Ok(config)
}

pub(crate) fn parse_startup_config(config: &Value) -> ConfigResult<StartupConfig> {
    let web_host = config
        .get("web_host")
        .ok_or("无法获取web_host")?
        .as_str()
        .ok_or("无法获取web_host")?
        .to_owned();
    let web_port = config
        .get("web_port")
        .ok_or("无法获取web_port")?
        .as_u64()
        .ok_or("无法获取web_port")?;

    Ok(StartupConfig {
        web_host,
        web_port,
        not_open_browser: config
            .get("not_open_browser")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        auto_close: config
            .get("auto_close")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        mqtt_client: config
            .get("mqtt_client")
            .filter(|value| value.is_object())
            .cloned(),
        postgresql: config
            .get("postgresql")
            .filter(|value| !value.is_null())
            .cloned(),
    })
}

/// 更新配置中的顶层字段并落盘（插件配置页保存用；下次启动生效）。
pub fn set_config_key(key: &str, value: Value) -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut config = read_config()?;
    config[key] = value;
    write_config(&config)
}

pub(crate) fn write_config(config: &Value) -> ConfigResult<()> {
    let path = config_path()?;
    let content = serde_json::to_string_pretty(config)?;
    atomic_write_file(&path, &content)?;
    match config_cache().write() {
        Ok(mut g) => *g = Some(config.clone()),
        Err(poison) => *poison.into_inner() = Some(config.clone()),
    }
    Ok(())
}

/// Create the default configuration file used on first startup.
pub fn init_config() {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            let _ = crate::cq_add_log_w(&format!("无法获取配置路径: {e}"));
            return;
        }
    };
    if !path.is_file() {
        let web_port = 1207u64;
        let web_host = "127.0.0.1".to_owned();

        let config = serde_json::json!({
            "web_port": web_port,
            "web_password": "",
            "readonly_web_password": "",
            "web_host": web_host,
            "ws_urls": [],
            "not_open_browser": false,
        });
        if let Err(e) = atomic_write_file(&path, &config.to_string()) {
            let _ = crate::cq_add_log_w(&format!("写入默认配置失败: {e}"));
            return;
        }
    }

    // 读取并解析现有配置；损坏则备份后回退默认（与 read_config_from_disk 同路径）
    let config = match read_config_from_disk() {
        Ok(c) => c,
        Err(e) => {
            let _ = crate::cq_add_log_w(&format!("读取配置失败: {e}"));
            return;
        }
    };
    match config_cache().write() {
        Ok(mut g) => *g = Some(config),
        Err(p) => *p.into_inner() = Some(config),
    }
}

/// Load configuration values whose lifetime is tied to the running process.
///
/// Warm and validate the central configuration cache during startup.
pub fn init_global_var() {
    if let Err(error) = read_config().and_then(|config| parse_startup_config(&config).map(|_| ())) {
        let _ = crate::cq_add_log_w(&format!("配置文件校验失败:{error}"));
    }
}

/// Parse the startup settings from the central configuration cache.
pub fn startup_config() -> ConfigResult<StartupConfig> {
    let config = read_config()?;
    parse_startup_config(&config)
}

/// Explicitly reload the single cached configuration object from disk.
pub fn refresh_cached_config() -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let config = read_config_from_disk()?;
    match config_cache().write() {
        Ok(mut g) => *g = Some(config),
        Err(p) => *p.into_inner() = Some(config),
    }
    Ok(())
}

pub fn read_web_password() -> ConfigResult<String> {
    Ok(read_config()?
        .get("web_password")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

pub fn read_readonly_web_password() -> ConfigResult<String> {
    Ok(read_config()?
        .get("readonly_web_password")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

/// 读取全部平台连接配置（含停用），返回 `(url, enabled)`。
pub fn adapter_entries() -> ConfigResult<Vec<(String, bool)>> {
    parse_adapter_entries(&read_config()?)
}

/// 解析 `ws_urls` + `disabled_ws_urls`（按下标标记停用）。
pub(crate) fn parse_adapter_entries(config: &Value) -> ConfigResult<Vec<(String, bool)>> {
    let urls = config
        .get("ws_urls")
        .ok_or("无法获取ws_urls")?
        .as_array()
        .ok_or("无法获取ws_urls")?;
    let disabled: std::collections::HashSet<usize> = config
        .get("disabled_ws_urls")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_u64)
                .map(|index| index as usize)
                .collect()
        })
        .unwrap_or_default();
    let mut entries = Vec::with_capacity(urls.len());
    for (index, url) in urls.iter().enumerate() {
        entries.push((
            url.as_str().ok_or("ws_url不是字符数组")?.to_owned(),
            !disabled.contains(&index),
        ));
    }
    Ok(entries)
}

/// 编码为 `(ws_urls, disabled_ws_urls)` 两个 JSON 数组。
pub(crate) fn encode_adapter_entries(entries: &[(String, bool)]) -> (Value, Value) {
    let urls = Value::Array(
        entries
            .iter()
            .map(|(url, _)| Value::String(url.clone()))
            .collect(),
    );
    let disabled = Value::Array(
        entries
            .iter()
            .enumerate()
            .filter(|(_, (_, enabled))| !enabled)
            .map(|(index, _)| Value::from(index as u64))
            .collect(),
    );
    (urls, disabled)
}

/// 写入全部平台连接配置。
pub fn set_adapter_entries(entries: &[(String, bool)]) -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut config = read_config()?;
    let (urls, disabled) = encode_adapter_entries(entries);
    config["ws_urls"] = urls;
    config["disabled_ws_urls"] = disabled;
    write_config(&config)
}

pub fn pkg_name_to_display(pkg_name: &str) -> String {
    if pkg_name.is_empty() {
        "默认包".to_owned()
    } else {
        pkg_name.to_owned()
    }
}

pub fn get_ban_pkgs() -> ConfigResult<Vec<String>> {
    Ok(parse_ban_pkgs(&read_config()?))
}

pub fn set_ban_pkg(pkg_name: &str, banned: bool) -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut config = read_config()?;
    let mut ban_pkgs = parse_ban_pkgs(&config);
    let pkg_name = pkg_name_to_display(pkg_name);
    if banned {
        if !ban_pkgs.contains(&pkg_name) {
            ban_pkgs.push(pkg_name);
        }
    } else {
        ban_pkgs.retain(|item| *item != pkg_name);
    }
    config["ban_pkgs"] = Value::Array(ban_pkgs.into_iter().map(Value::String).collect());
    write_config(&config)
}

pub fn rename_ban_pkg(old_pkg_name: &str, new_pkg_name: &str) -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut config = read_config()?;
    let mut ban_pkgs = parse_ban_pkgs(&config);
    let old_pkg_name = pkg_name_to_display(old_pkg_name);
    let new_pkg_name = pkg_name_to_display(new_pkg_name);
    if !ban_pkgs.contains(&old_pkg_name) {
        return Ok(());
    }
    ban_pkgs.retain(|item| *item != old_pkg_name);
    if !ban_pkgs.contains(&new_pkg_name) {
        ban_pkgs.push(new_pkg_name);
    }
    config["ban_pkgs"] = Value::Array(ban_pkgs.into_iter().map(Value::String).collect());
    write_config(&config)
}

/// 被禁用的插件名列表（框架管理台切换后持久化）。
pub fn disabled_plugins() -> Vec<String> {
    read_config()
        .ok()
        .and_then(|config| {
            config
                .get("disabled_plugins")
                .and_then(Value::as_array)
                .cloned()
        })
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// 记录插件的启用/禁用状态。
pub fn set_plugin_enabled(plugin_name: &str, enabled: bool) -> ConfigResult<()> {
    let _write_guard = match config_write_lock().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut config = read_config()?;
    let mut disabled = disabled_plugins();
    if enabled {
        disabled.retain(|name| name != plugin_name);
    } else if !disabled.iter().any(|name| name == plugin_name) {
        disabled.push(plugin_name.to_owned());
    }
    config["disabled_plugins"] = Value::Array(disabled.into_iter().map(Value::String).collect());
    write_config(&config)
}

pub fn skip_msg_time() -> i64 {
    read_config()
        .ok()
        .and_then(|config| config.get("skip_msg_time").and_then(Value::as_i64))
        .unwrap_or(600)
}

pub(crate) fn parse_ban_pkgs(config: &Value) -> Vec<String> {
    config
        .get("ban_pkgs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{encode_adapter_entries, parse_adapter_entries, parse_startup_config};
    use serde_json::json;

    #[test]
    fn parse_startup_config_separates_service_settings() {
        let config = serde_json::json!({
            "web_host": "0.0.0.0",
            "web_port": 8080,
            "not_open_browser": true,
            "auto_close": true,
            "mqtt_client": {"broker_host": "localhost"},
            "postgresql": null
        });
        let startup = parse_startup_config(&config).unwrap();
        assert_eq!(startup.web_host, "0.0.0.0");
        assert_eq!(startup.web_port, 8080);
        assert!(startup.not_open_browser);
        assert!(startup.auto_close);
        assert!(startup.mqtt_client.is_some());
        assert!(startup.postgresql.is_none());
    }

    #[test]
    fn adapter_entries_mark_disabled_indices() {
        let config = json!({
            "ws_urls": ["ws://one", "ws://two", "ws://three"],
            "disabled_ws_urls": [1, 2]
        });
        assert_eq!(
            parse_adapter_entries(&config).unwrap(),
            vec![
                ("ws://one".to_owned(), true),
                ("ws://two".to_owned(), false),
                ("ws://three".to_owned(), false),
            ]
        );
    }

    #[test]
    fn adapter_entries_reject_non_string_entries() {
        let config = json!({"ws_urls": ["ws://one", 2]});
        assert!(parse_adapter_entries(&config).is_err());
    }

    #[test]
    fn adapter_entries_round_trip() {
        let entries = vec![
            ("ws://one".to_owned(), true),
            ("ws://two".to_owned(), false),
        ];
        let (urls, disabled) = encode_adapter_entries(&entries);
        let config = json!({"ws_urls": urls, "disabled_ws_urls": disabled});
        assert_eq!(parse_adapter_entries(&config).unwrap(), entries);
    }
}
