use hyper::body::Incoming;
use hyper::{Method, StatusCode};
use redreply_bot::http::{bytes_response, collect_body, json_response};
use redreply_bot::{HttpError, HttpResponse, MenuItem, PluginContext, Result, RouteAccess};
use serde_json::{json, Value};

const PAGE: &str = include_str!("../page.html");

pub const CONFIG_KEY: &str = "chatgpt";

const MIN_CONTEXT_TOKENS: u64 = 1_000;

fn normalize_reasoning_effort(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => Some(String::new()),
        "none" => Some("none".to_string()),
        "minimal" | "low" => Some("low".to_string()),
        "medium" | "high" | "xhigh" => Some("high".to_string()),
        "max" | "ultra" => Some("max".to_string()),
        _ => None,
    }
}

fn normalize_group_whitelist(list: &[Value]) -> Vec<String> {
    let mut result = Vec::new();
    for item in list {
        let text = match item {
            Value::String(text) => text.trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => continue,
        };
        if !text.is_empty() && !result.contains(&text) {
            result.push(text);
        }
    }
    result
}

#[derive(Clone, Debug)]
pub struct ChatConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    pub prefixes: Vec<String>,
    pub group_whitelist: Vec<String>,
    pub private_always: bool,
    pub max_tokens: usize,
    pub temperature: f32,
    pub reasoning_effort: String,
    pub timeout_secs: u64,
    pub send_error_tip: bool,
    pub command_enabled: bool,
    pub command_timeout_secs: u64,
    pub command_max_output: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://opencode.ai/zen/go/v1".to_string(),
            model: "deepseek-v4.1-flash".to_string(),
            system_prompt: "你是一个友善的聊天助手，用简洁的中文回复。".to_string(),
            prefixes: vec!["问 ".to_string(), "gpt ".to_string()],
            group_whitelist: Vec::new(),
            private_always: true,
            max_tokens: 8000,
            temperature: 0.7,
            reasoning_effort: String::new(),
            timeout_secs: 1200,
            send_error_tip: true,
            command_enabled: false,
            command_timeout_secs: 30,
            command_max_output: 8000,
        }
    }
}

impl ChatConfig {
    pub fn is_ready(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn load() -> Self {
        let stored = redlang::config_manager::read_config()
            .ok()
            .and_then(|config| config.get(CONFIG_KEY).cloned())
            .unwrap_or(Value::Null);
        Self::from_value(&stored)
    }

    pub(crate) fn from_value(value: &Value) -> Self {
        let defaults = Self::default();
        let get_str = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let prefixes = value
            .get("prefixes")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .filter(|item| !item.trim().is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|list| !list.is_empty())
            .unwrap_or_else(|| defaults.prefixes.clone());
        Self {
            api_key: get_str("api_key").unwrap_or_default(),
            base_url: get_str("base_url").unwrap_or(defaults.base_url),
            model: get_str("model").unwrap_or(defaults.model),
            system_prompt: value
                .get("system_prompt")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or(defaults.system_prompt),
            prefixes,
            group_whitelist: value
                .get("group_whitelist")
                .and_then(Value::as_array)
                .map(|list| normalize_group_whitelist(list))
                .unwrap_or_default(),
            private_always: value
                .get("private_always")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.private_always),
            max_tokens: value
                .get("max_tokens")
                .and_then(Value::as_u64)
                .map(|tokens| tokens.max(MIN_CONTEXT_TOKENS) as usize)
                .unwrap_or_else(|| {
                    value
                        .get("max_history")
                        .and_then(Value::as_u64)
                        .map(|rounds| (rounds * 1500).max(MIN_CONTEXT_TOKENS) as usize)
                        .unwrap_or(defaults.max_tokens)
                }),
            temperature: value
                .get("temperature")
                .and_then(Value::as_f64)
                .map(|temperature| (temperature as f32).clamp(0.0, 2.0))
                .unwrap_or(defaults.temperature),
            reasoning_effort: value
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .and_then(normalize_reasoning_effort)
                .unwrap_or_default(),
            timeout_secs: value
                .get("timeout_secs")
                .and_then(Value::as_u64)
                .filter(|secs| (5..=3600).contains(secs))
                .unwrap_or(defaults.timeout_secs),
            send_error_tip: value
                .get("send_error_tip")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.send_error_tip),
            command_enabled: value
                .get("command_enabled")
                .or_else(|| value.get("python_enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(defaults.command_enabled),
            command_timeout_secs: value
                .get("command_timeout_secs")
                .or_else(|| value.get("python_timeout_secs"))
                .and_then(Value::as_u64)
                .map(|secs| secs.clamp(1, 600))
                .unwrap_or(defaults.command_timeout_secs),
            command_max_output: value
                .get("command_max_output")
                .or_else(|| value.get("python_max_output"))
                .and_then(Value::as_u64)
                .map(|chars| chars.clamp(500, 200_000) as usize)
                .unwrap_or(defaults.command_max_output),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "api_key": self.api_key,
            "base_url": self.base_url,
            "model": self.model,
            "system_prompt": self.system_prompt,
            "prefixes": self.prefixes,
            "group_whitelist": self.group_whitelist,
            "private_always": self.private_always,
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
            "reasoning_effort": self.reasoning_effort,
            "timeout_secs": self.timeout_secs,
            "send_error_tip": self.send_error_tip,
            "command_enabled": self.command_enabled,
            "command_timeout_secs": self.command_timeout_secs,
            "command_max_output": self.command_max_output,
        })
    }
}

pub fn register(ctx: &PluginContext) -> Result<()> {
    ctx.register_menu(MenuItem::new("config", "配置", "/chatgpt/"))?;
    ctx.route(
        Method::GET,
        "/chatgpt/",
        RouteAccess::Write,
        |_request, _context| async {
            Ok(bytes_response(
                StatusCode::OK,
                "text/html; charset=utf-8",
                PAGE,
                None,
            ))
        },
    );
    ctx.route(
        Method::GET,
        "/chatgpt/api/config",
        RouteAccess::Write,
        |_request, _context| async { Ok(current_config()) },
    );
    ctx.route(
        Method::POST,
        "/chatgpt/api/config",
        RouteAccess::Write,
        |request, _context| async { save_config(request).await },
    );
    ctx.route(
        Method::POST,
        "/chatgpt/api/config/delete",
        RouteAccess::Write,
        |_request, _context| async { clear_config() },
    );
    ctx.route(
        Method::POST,
        "/chatgpt/api/test",
        RouteAccess::Write,
        |request, _context| async { test_connection(request).await },
    );
    ctx.route(
        Method::GET,
        "/chatgpt/api/calls",
        RouteAccess::Write,
        |_request, _context| async { Ok(calls_response()) },
    );
    ctx.route(
        Method::GET,
        "/chatgpt/api/last_trigger",
        RouteAccess::Write,
        |_request, _context| async {
            Ok(json_response(
                StatusCode::OK,
                &json!({"retcode": 0, "data": crate::calls::latest_turn()}),
            ))
        },
    );
    Ok(())
}

fn calls_response() -> HttpResponse {
    let (records, stats) = crate::calls::snapshot();
    json_response(
        StatusCode::OK,
        &json!({
            "retcode": 0,
            "data": {
                "records": records,
                "stats": stats,
                "capacity": crate::calls::CAPACITY,
            }
        }),
    )
}

fn current_config() -> HttpResponse {
    let mut config = ChatConfig::load().to_value();
    let has_key = !config
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .is_empty();
    config["has_api_key"] = json!(has_key);
    json_response(StatusCode::OK, &json!({"retcode": 0, "data": config}))
}

async fn save_config(
    request: hyper::Request<Incoming>,
) -> std::result::Result<HttpResponse, HttpError> {
    let (_, body) = request.into_parts();
    let bytes = collect_body(body, 64 * 1024).await?;
    let input: Value = serde_json::from_slice(&bytes)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))?;
    let config = build_config(&input).map_err(HttpError::bad_request)?;
    redlang::config_manager::set_config_key(CONFIG_KEY, config)
        .map_err(|error| HttpError::internal(format!("保存配置失败：{error}")))?;
    Ok(json_response(
        StatusCode::OK,
        &json!({"retcode": 0, "data": {"message": "已保存，立即生效"}}),
    ))
}

async fn test_connection(
    request: hyper::Request<Incoming>,
) -> std::result::Result<HttpResponse, HttpError> {
    let (_, body) = request.into_parts();
    let bytes = collect_body(body, 64 * 1024).await?;
    let input: Value = serde_json::from_slice(&bytes)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))?;
    let normalized = build_config(&input).map_err(HttpError::bad_request)?;
    let config = ChatConfig::from_value(&normalized);
    if !config.is_ready() {
        return Err(HttpError::bad_request("请先填写 API Key"));
    }
    match crate::chat_once(&config, "config-test", &[], "请只回复四个字：连接正常").await
    {
        Ok(reply) => {
            let message = format!(
                "{}\n（本次实际 prompt_tokens={}，cached_tokens={}，completion_tokens={}）",
                reply.text, reply.prompt_tokens, reply.cached_tokens, reply.completion_tokens
            );
            Ok(json_response(
                StatusCode::OK,
                &json!({"retcode": 0, "data": {"message": message}}),
            ))
        }
        Err(error) => Ok(json_response(
            StatusCode::OK,
            &json!({"retcode": 1, "data": {"message": error}}),
        )),
    }
}

fn clear_config() -> std::result::Result<HttpResponse, HttpError> {
    redlang::config_manager::set_config_key(CONFIG_KEY, Value::Null)
        .map_err(|error| HttpError::internal(format!("清空配置失败：{error}")))?;
    Ok(json_response(
        StatusCode::OK,
        &json!({"retcode": 0, "data": {"message": "已清空，插件不再回复任何消息"}}),
    ))
}

fn build_config(input: &Value) -> std::result::Result<Value, String> {
    let defaults = ChatConfig::default();
    let optional_str = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let base_url = optional_str("base_url")
        .unwrap_or(defaults.base_url)
        .trim_end_matches('/')
        .to_owned();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("接口地址必须以 http:// 或 https:// 开头".to_string());
    }
    let model = optional_str("model").unwrap_or(defaults.model);
    let prefixes: Vec<String> = input
        .get("prefixes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.trim().is_empty())
                .map(str::to_owned)
                .collect()
        })
        .filter(|list: &Vec<String>| !list.is_empty())
        .unwrap_or(defaults.prefixes);
    let group_whitelist = input
        .get("group_whitelist")
        .and_then(Value::as_array)
        .map(|list| normalize_group_whitelist(list))
        .unwrap_or_default();
    let temperature = input
        .get("temperature")
        .and_then(Value::as_f64)
        .unwrap_or(defaults.temperature as f64)
        .clamp(0.0, 2.0);
    let reasoning_effort = match input.get("reasoning_effort").and_then(Value::as_str) {
        Some(raw) => normalize_reasoning_effort(raw)
            .ok_or_else(|| "思考强度只能是默认、none、low、high 或 max".to_string())?,
        None => defaults.reasoning_effort,
    };
    let timeout_secs = input
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(defaults.timeout_secs)
        .clamp(5, 3600);
    let max_tokens = input
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(defaults.max_tokens as u64)
        .max(MIN_CONTEXT_TOKENS);

    Ok(json!({
        "api_key": input.get("api_key").and_then(Value::as_str).unwrap_or("").trim(),
        "base_url": base_url,
        "model": model,
        "system_prompt": input.get("system_prompt").and_then(Value::as_str).unwrap_or(""),
        "prefixes": prefixes,
        "group_whitelist": group_whitelist,
        "private_always": input.get("private_always").and_then(Value::as_bool).unwrap_or(defaults.private_always),
        "temperature": temperature,
        "reasoning_effort": reasoning_effort,
        "timeout_secs": timeout_secs,
        "send_error_tip": input.get("send_error_tip").and_then(Value::as_bool).unwrap_or(defaults.send_error_tip),
        "max_tokens": max_tokens,
        "command_enabled": input
            .get("command_enabled")
            .or_else(|| input.get("python_enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(defaults.command_enabled),
        "command_timeout_secs": input
            .get("command_timeout_secs")
            .or_else(|| input.get("python_timeout_secs"))
            .and_then(Value::as_u64)
            .unwrap_or(defaults.command_timeout_secs)
            .clamp(1, 600),
        "command_max_output": input
            .get("command_max_output")
            .or_else(|| input.get("python_max_output"))
            .and_then(Value::as_u64)
            .unwrap_or(defaults.command_max_output as u64)
            .clamp(500, 200_000),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> Value {
        json!({
            "api_key": "sk-test",
            "base_url": "https://api.openai.com/v1/",
            "model": "gpt-4o-mini",
            "system_prompt": "你是助手",
            "prefixes": ["问 ", " ", "gpt "],
            "private_always": true,
            "max_tokens": 6000,
            "temperature": 0.5,
            "reasoning_effort": "high",
            "timeout_secs": 30,
            "send_error_tip": false,
            "command_enabled": true,
            "command_timeout_secs": 45,
            "command_max_output": 12000
        })
    }

    #[test]
    fn builds_normalized_config() {
        let config = build_config(&valid_input()).unwrap();
        assert_eq!(config["api_key"], "sk-test");
        assert_eq!(config["base_url"], "https://api.openai.com/v1");
        assert_eq!(config["prefixes"], json!(["问 ", "gpt "]));
        assert_eq!(config["max_tokens"], 6000);
        assert_eq!(config["reasoning_effort"], "high");
        assert_eq!(config["command_enabled"], true);
        assert_eq!(config["command_timeout_secs"], 45);
        assert_eq!(config["command_max_output"], 12000);
    }

    #[test]
    fn command_defaults_disabled() {
        let config = ChatConfig::from_value(&json!({}));
        assert!(!config.command_enabled);
        assert_eq!(config.command_timeout_secs, 30);
        assert_eq!(config.command_max_output, 8000);
        assert_eq!(config.reasoning_effort, "");
    }

    #[test]
    fn migrates_legacy_command_fields() {
        let config = ChatConfig::from_value(&json!({
            "python_enabled": true,
            "python_timeout_secs": 45,
            "python_max_output": 12000
        }));
        assert!(config.command_enabled);
        assert_eq!(config.command_timeout_secs, 45);
        assert_eq!(config.command_max_output, 12000);
        let saved = build_config(&json!({"python_enabled": true})).unwrap();
        assert_eq!(saved["command_enabled"], true);
        assert!(saved.get("python_enabled").is_none());
    }

    #[test]
    fn normalizes_reasoning_effort() {
        let mut input = valid_input();
        input["reasoning_effort"] = json!("medium");
        assert_eq!(build_config(&input).unwrap()["reasoning_effort"], "high");
        input["reasoning_effort"] = json!("ultra");
        assert_eq!(build_config(&input).unwrap()["reasoning_effort"], "max");
        input["reasoning_effort"] = json!("none");
        assert_eq!(build_config(&input).unwrap()["reasoning_effort"], "none");
        input["reasoning_effort"] = json!("");
        assert_eq!(build_config(&input).unwrap()["reasoning_effort"], "");
        input["reasoning_effort"] = json!("turbo");
        assert!(build_config(&input).is_err());
    }

    #[test]
    fn rejects_bad_base_url() {
        let mut input = valid_input();
        input["base_url"] = json!("ftp://example.com");
        assert!(build_config(&input).is_err());
    }

    #[test]
    fn clamps_numeric_ranges() {
        let mut input = valid_input();
        input["max_tokens"] = json!(999);
        input["temperature"] = json!(99.0);
        input["timeout_secs"] = json!(1);
        input["command_timeout_secs"] = json!(9999);
        input["command_max_output"] = json!(1);
        let config = build_config(&input).unwrap();
        assert_eq!(config["max_tokens"], 1000);
        assert_eq!(config["temperature"], 2.0);
        assert_eq!(config["timeout_secs"], 5);
        assert_eq!(config["command_timeout_secs"], 600);
        assert_eq!(config["command_max_output"], 500);

        let mut input = valid_input();
        input["max_tokens"] = json!(2_000_000);
        let config = build_config(&input).unwrap();
        assert_eq!(config["max_tokens"], 2_000_000);
    }

    #[test]
    fn defaults_fill_missing_fields() {
        let config = ChatConfig::from_value(&json!({}));
        let defaults = ChatConfig::default();
        assert_eq!(config.base_url, defaults.base_url);
        assert_eq!(config.model, defaults.model);
        assert_eq!(config.prefixes, defaults.prefixes);
        assert!(!config.is_ready());
    }

    #[test]
    fn migrates_legacy_round_fields() {
        let config = ChatConfig::from_value(&json!({"max_history": 10}));
        assert_eq!(config.max_tokens, 15000);
        let config = ChatConfig::from_value(&json!({"max_history": 10, "max_tokens": 6000}));
        assert_eq!(config.max_tokens, 6000);
    }
}
