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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Live2dProviderConfig {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub context_length: u64,
    #[serde(default)]
    pub thinking_level: String,
}

impl Default for Live2dProviderConfig {
    fn default() -> Self {
        Self {
            name: "OpenAI 官方".to_owned(),
            base_url: "https://api.openai.com/v1/chat/completions".to_owned(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_owned(),
            context_length: 131_072,
            thinking_level: "high".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Live2dChatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub config: String,
    pub configs: Vec<Live2dProviderConfig>,
    pub system_prompt: String,
    pub thinking_level: String,
    pub context_length: u64,
}

impl Default for Live2dChatConfig {
    fn default() -> Self {
        let provider = Live2dProviderConfig::default();
        Self {
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            model: provider.model.clone(),
            config: provider.name.clone(),
            configs: vec![provider.clone()],
            system_prompt: "你是红色问答的看板娘。请使用简洁、自然、友好的中文回答。".to_owned(),
            thinking_level: provider.thinking_level.clone(),
            context_length: provider.context_length,
        }
    }
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
        "not_open_browser": false,
        "live2d_chat": {
            "config": "OpenAI 官方",
            "configs": [
                {
                    "name": "OpenAI 官方",
                    "base_url": "https://api.openai.com/v1/chat/completions",
                    "api_key": "",
                    "model": "gpt-4o-mini",
                    "context_length": 131072,
                    "thinking_level": "high"
                }
            ],
            "system_prompt": "你是红色问答的看板娘。请使用简洁、自然、友好的中文回答。"
        }
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
            "not_open_browser": false,
            "live2d_chat": {
                "config": "OpenAI 官方",
                "configs": [
                    {
                        "name": "OpenAI 官方",
                        "base_url": "https://api.openai.com/v1/chat/completions",
                        "api_key": "",
                        "model": "gpt-4o-mini",
                        "context_length": 131072,
                        "thinking_level": "high"
                    }
                ],
                "system_prompt": "你是红色问答的看板娘。请使用简洁、自然、友好的中文回答。"
            }
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

pub fn live2d_chat_config() -> ConfigResult<Live2dChatConfig> {
    Ok(parse_live2d_chat_config(&read_config()?))
}

pub fn save_live2d_chat_config(chat: &Live2dChatConfig) -> ConfigResult<()> {
    validate_live2d_chat_config(chat)?;
    let _write_guard = config_write_lock().lock().unwrap();
    let mut config = read_config()?;
    config["live2d_chat"] = serde_json::json!({
        "config": chat.config,
        "configs": chat.configs,
        "system_prompt": chat.system_prompt,
    });
    write_config(&config)
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

fn parse_provider_config(value: &Value) -> Option<Live2dProviderConfig> {
    let obj = value.as_object()?;
    let defaults = Live2dProviderConfig::default();
    let name = obj.get("name").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())?.to_owned();
    let model = obj.get("model").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())?.to_owned();
    Some(Live2dProviderConfig {
        name,
        base_url: obj.get("base_url").and_then(Value::as_str).unwrap_or(&defaults.base_url).to_owned(),
        api_key: obj.get("api_key").and_then(Value::as_str).unwrap_or_default().to_owned(),
        model,
        context_length: obj.get("context_length").and_then(Value::as_u64).unwrap_or(defaults.context_length),
        thinking_level: obj.get("thinking_level").and_then(Value::as_str).unwrap_or(&defaults.thinking_level).to_owned(),
    })
}

pub(crate) fn parse_live2d_chat_config(config: &Value) -> Live2dChatConfig {
    let defaults = Live2dChatConfig::default();
    let Some(chat) = config.get("live2d_chat").and_then(Value::as_object) else {
        return defaults;
    };
    let mut configs: Vec<Live2dProviderConfig> = chat.get("configs").and_then(Value::as_array).map(|items| items.iter().filter_map(parse_provider_config).collect()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    configs.retain(|item| seen.insert(item.name.clone()));
    if configs.is_empty() {
        configs.push(Live2dProviderConfig::default());
    }
    let selected_name = chat.get("config").and_then(Value::as_str).unwrap_or(&defaults.config).trim();
    let current = configs.iter().find(|item| item.name == selected_name).cloned().unwrap_or_else(|| configs[0].clone());
    Live2dChatConfig {
        base_url: current.base_url.clone(),
        api_key: current.api_key.clone(),
        model: current.model.clone(),
        config: current.name.clone(),
        configs,
        system_prompt: chat.get("system_prompt").and_then(Value::as_str).unwrap_or(&defaults.system_prompt).to_owned(),
        thinking_level: current.thinking_level.clone(),
        context_length: current.context_length,
    }
}

fn validate_live2d_chat_config(chat: &Live2dChatConfig) -> ConfigResult<()> {
    if chat.configs.is_empty() || chat.configs.len() > 50 {
        return Err("配置列表不能为空且最多50个配置".into());
    }
    for item in &chat.configs {
        if item.name.trim().is_empty() || item.name.chars().count() > 200 {
            return Err("配置名称不能为空且不能超过200个字符".into());
        }
        if item.model.trim().is_empty() || item.model.chars().count() > 200 {
            return Err("模型名称不能为空且不能超过200个字符".into());
        }
        if item.base_url.chars().count() > 2048 {
            return Err("对话接口地址不能超过2048个字符".into());
        }
        let url = reqwest::Url::parse(item.base_url.trim())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("对话接口地址只支持 http 或 https".into());
        }
        if item.api_key.chars().count() > 8192 {
            return Err("API Key过长".into());
        }
        if !matches!(
            item.thinking_level.as_str(),
            "none" | "low" | "high" | "xhigh" | "max"
        ) {
            return Err("思考等级无效".into());
        }
        if !matches!(
            item.context_length,
            32_768 | 65_536 | 131_072 | 262_144 | 524_288 | 1_048_576
        ) {
            return Err("上下文长度必须是 32K、64K、128K、256K、512K 或 1M 之一".into());
        }
    }
    if chat.config.trim().is_empty()
        || !chat.configs.iter().any(|item| item.name == chat.config)
    {
        return Err("当前选中的配置必须存在于配置列表中".into());
    }
    if chat.system_prompt.chars().count() > 8000 {
        return Err("角色设定不能超过8000个字符".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_live2d_chat_config, parse_startup_config, parse_ws_urls};

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

    #[test]
    fn parse_live2d_chat_config_uses_defaults_when_missing() {
        let chat = parse_live2d_chat_config(&serde_json::json!({}));
        assert_eq!(chat.config, "OpenAI 官方");
        assert_eq!(chat.configs.len(), 1);
        assert_eq!(chat.configs[0].name, "OpenAI 官方");
        assert_eq!(chat.model, "gpt-4o-mini");
    }

    #[test]
    fn parse_live2d_chat_config_reads_named_configs() {
        let chat = parse_live2d_chat_config(&serde_json::json!({
            "live2d_chat": {
                "config": "DeepSeek 官方",
                "configs": [
                    {
                        "name": "DeepSeek 官方",
                        "base_url": "https://api.deepseek.com/v1/chat/completions",
                        "api_key": "key",
                        "model": "deepseek-v4-flash",
                        "context_length": 524288,
                        "thinking_level": "max"
                    },
                    {
                        "name": "本地模型",
                        "base_url": "http://127.0.0.1:11434/v1/chat/completions",
                        "api_key": "",
                        "model": "qwen2.5",
                        "context_length": 131072,
                        "thinking_level": "high"
                    }
                ]
            }
        }));
        assert_eq!(chat.config, "DeepSeek 官方");
        assert_eq!(chat.model, "deepseek-v4-flash");
        assert_eq!(chat.base_url, "https://api.deepseek.com/v1/chat/completions");
        assert_eq!(chat.configs.len(), 2);
        assert_eq!(chat.configs[1].name, "本地模型");
        assert_eq!(chat.configs[1].model, "qwen2.5");
    }

    #[test]
    fn parse_live2d_chat_config_falls_back_to_first_config() {
        let chat = parse_live2d_chat_config(&serde_json::json!({
            "live2d_chat": {
                "config": "不存在",
                "configs": [
                    {
                        "name": "配置 A",
                        "base_url": "http://a/v1/chat/completions",
                        "api_key": "",
                        "model": "a",
                        "context_length": 32768,
                        "thinking_level": "low"
                    }
                ]
            }
        }));
        assert_eq!(chat.config, "配置 A");
        assert_eq!(chat.model, "a");
    }
}
