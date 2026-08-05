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

fn read_config_from_disk() -> ConfigResult<Value> {
    let path = config_path()?;
    if !path.is_file() {
        fs::write(&path, default_config().to_string())?;
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// Return the process-wide cached `config.json` value.
///
/// The file is read only on the first call. All field accessors in this
/// module derive their values from this single cached object; they do not
/// maintain additional field-level caches.
pub fn read_config() -> ConfigResult<Value> {
    {
        let cache = config_cache().read().unwrap();
        if let Some(config) = cache.as_ref() {
            return Ok(config.clone());
        }
    }

    let mut cache = config_cache().write().unwrap();
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

pub(crate) fn write_config(config: &Value) -> ConfigResult<()> {
    fs::write(config_path()?, serde_json::to_string_pretty(config)?)?;
    *config_cache().write().unwrap() = Some(config.clone());
    Ok(())
}

/// Create the interactive configuration file used on first startup.
pub fn init_config() {
    let _write_guard = config_write_lock().lock().unwrap();
    let path = config_path().unwrap();
    if !path.is_file() {
        let mut line = String::new();
        println!("请输入端口号(默认1207):");
        std::io::stdin().read_line(&mut line).unwrap();
        let web_port = if line.trim().is_empty() {
            1207
        } else {
            line.trim().parse::<u16>().unwrap()
        };

        line.clear();
        println!("请输入主机地址(默认127.0.0.1):");
        std::io::stdin().read_line(&mut line).unwrap();
        let web_host = if line.trim().is_empty() {
            "127.0.0.1".to_owned()
        } else {
            line.trim().to_owned()
        };

        let config = serde_json::json!({
            "web_port": web_port,
            "web_password": "",
            "readonly_web_password": "",
            "web_host": web_host,
            "ws_urls": [],
            "not_open_browser": false
        });
        fs::write(&path, config.to_string()).unwrap();
    }

    let config = serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    fs::write(path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
    *config_cache().write().unwrap() = Some(config);
}

/// Load configuration values whose lifetime is tied to the running process.
///
/// Warm and validate the central configuration cache during startup.
pub fn init_global_var() {
    if let Err(error) = read_config().and_then(|config| parse_startup_config(&config).map(|_| ())) {
        crate::cq_add_log_w(&format!("配置文件校验失败:{error}")).unwrap();
    }
}

/// Parse the startup settings from the central configuration cache.
pub fn startup_config() -> ConfigResult<StartupConfig> {
    let config = read_config()?;
    parse_startup_config(&config)
}

/// Explicitly reload the single cached configuration object from disk.
pub fn refresh_cached_config() -> ConfigResult<()> {
    let _write_guard = config_write_lock().lock().unwrap();
    let config = read_config_from_disk()?;
    *config_cache().write().unwrap() = Some(config);
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

pub fn set_ws_urls(ws_urls: Value) -> ConfigResult<()> {
    set_config_value("ws_urls", ws_urls)
}

pub fn set_disabled_ws_urls(disabled: Value) -> ConfigResult<()> {
    set_config_value("disabled_ws_urls", disabled)
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
    let _write_guard = config_write_lock().lock().unwrap();
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
    let _write_guard = config_write_lock().lock().unwrap();
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

pub fn skip_msg_time() -> i64 {
    read_config()
        .ok()
        .and_then(|config| config.get("skip_msg_time").and_then(Value::as_i64))
        .unwrap_or(600)
}

pub fn ws_urls() -> ConfigResult<Vec<String>> {
    parse_ws_urls(&read_config()?)
}

fn set_config_value(key: &str, value: Value) -> ConfigResult<()> {
    let _write_guard = config_write_lock().lock().unwrap();
    let mut config = read_config()?;
    config[key] = value;
    write_config(&config)
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

pub(crate) fn parse_ws_urls(config: &Value) -> ConfigResult<Vec<String>> {
    let urls = config
        .get("ws_urls")
        .ok_or("无法获取ws_urls")?
        .as_array()
        .ok_or("无法获取ws_urls")?;
    let mut ws_urls = Vec::with_capacity(urls.len());
    for url in urls {
        ws_urls.push(url.as_str().ok_or("ws_url不是字符数组")?.to_owned());
    }

    if let Some(disabled) = config.get("disabled_ws_urls").and_then(Value::as_array) {
        let disabled_indices: std::collections::HashSet<usize> = disabled
            .iter()
            .filter_map(Value::as_u64)
            .map(|index| index as usize)
            .collect();
        ws_urls = ws_urls
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !disabled_indices.contains(index))
            .map(|(_, url)| url)
            .collect();
    }

    Ok(ws_urls)
}

#[cfg(test)]
mod tests {
    use super::{parse_startup_config, parse_ws_urls};

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
    fn parse_ws_urls_filters_disabled_indices() {
        let config = serde_json::json!({
            "ws_urls": ["ws://one", "ws://two", "ws://three"],
            "disabled_ws_urls": [1]
        });
        assert_eq!(
            parse_ws_urls(&config).unwrap(),
            vec!["ws://one".to_owned(), "ws://three".to_owned()]
        );
    }

    #[test]
    fn parse_ws_urls_rejects_non_string_entries() {
        let config = serde_json::json!({"ws_urls": ["ws://one", 2]});
        assert!(parse_ws_urls(&config).is_err());
    }
}
