//! MQTT 配置页与读写 API：`/mqtt/`、`/mqtt/api/config`。
//!
//! 配置写入 `config.json` 的 `mqtt_client` 字段，重启后由插件启动逻辑读取。

use hyper::body::Incoming;
use hyper::{Method, StatusCode};
use redreply_bot::http::{bytes_response, collect_body, json_response};
use redreply_bot::{HttpError, HttpResponse, MenuItem, PluginContext, Result, RouteAccess};
use serde_json::{json, Value};

const PAGE: &str = include_str!("../page.html");

/// 注册配置菜单、页面与读写接口。
pub fn register(ctx: &PluginContext) -> Result<()> {
    ctx.register_menu(MenuItem::new("config", "配置", "/mqtt/"))?;
    ctx.route(Method::GET, "/mqtt/", RouteAccess::Write, |_request, _context| async {
        Ok(bytes_response(
            StatusCode::OK,
            "text/html; charset=utf-8",
            PAGE,
            None,
        ))
    });
    ctx.route(
        Method::GET,
        "/mqtt/api/config",
        RouteAccess::Write,
        |_request, _context| async { Ok(current_config()) },
    );
    ctx.route(
        Method::POST,
        "/mqtt/api/config",
        RouteAccess::Write,
        |request, _context| async { save_config(request).await },
    );
    ctx.route(
        Method::POST,
        "/mqtt/api/config/delete",
        RouteAccess::Write,
        |_request, _context| async { clear_config() },
    );
    Ok(())
}

fn current_config() -> HttpResponse {
    let value = redlang::config_manager::read_config()
        .ok()
        .and_then(|config| config.get("mqtt_client").cloned())
        .filter(|value| value.is_object())
        .unwrap_or(Value::Null);
    json_response(StatusCode::OK, &json!({"retcode": 0, "data": value}))
}

async fn save_config(
    request: hyper::Request<Incoming>,
) -> std::result::Result<HttpResponse, HttpError> {
    let (_, body) = request.into_parts();
    let bytes = collect_body(body, 64 * 1024).await?;
    let input: Value = serde_json::from_slice(&bytes)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))?;
    let config = build_config(&input).map_err(HttpError::bad_request)?;
    redlang::config_manager::set_config_key("mqtt_client", config)
        .map_err(|error| HttpError::internal(format!("保存配置失败：{error}")))?;
    Ok(saved_response("已保存，重启软件后生效"))
}

fn clear_config() -> std::result::Result<HttpResponse, HttpError> {
    redlang::config_manager::set_config_key("mqtt_client", Value::Null)
        .map_err(|error| HttpError::internal(format!("清空配置失败：{error}")))?;
    Ok(saved_response("已清空，重启软件后生效"))
}

fn saved_response(message: &str) -> HttpResponse {
    json_response(
        StatusCode::OK,
        &json!({"retcode": 0, "data": {"message": message}}),
    )
}

/// 由表单 JSON 构造规范化的 `mqtt_client` 配置。
fn build_config(input: &Value) -> std::result::Result<Value, String> {
    let broker_host = required_str(input, "broker_host", "请填写 broker 地址")?;
    let broker_port = input
        .get("broker_port")
        .and_then(Value::as_u64)
        .filter(|port| (1..=65535).contains(port))
        .ok_or("broker 端口必须是 1-65535")?;
    let client_id = required_str(input, "client_id", "请填写 client_id")?;
    crate::mqtt::validate_mqtt_client_id(&client_id).map_err(|error| error.to_string())?;

    let mut config = json!({
        "broker_host": broker_host,
        "broker_port": broker_port,
        "client_id": client_id,
    });
    if let Some(username) = optional_str(input, "broker_username") {
        config["broker_username"] = json!(username);
        config["broker_password"] =
            json!(input.get("broker_password").and_then(Value::as_str).unwrap_or(""));
    }
    if let Some(clients) = input.get("remote_clients").and_then(Value::as_array) {
        let list: Vec<String> = clients
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect();
        config["remote_clients"] = json!(list);
    }
    Ok(config)
}

fn required_str(input: &Value, key: &str, message: &str) -> std::result::Result<String, String> {
    optional_str(input, key).ok_or_else(|| message.to_owned())
}

fn optional_str(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::build_config;
    use serde_json::json;

    fn valid_input() -> serde_json::Value {
        json!({
            "broker_host": " 127.0.0.1 ",
            "broker_port": 1883,
            "client_id": "red-1",
            "broker_username": "user",
            "broker_password": "pass",
            "remote_clients": ["a", " ", "b"]
        })
    }

    #[test]
    fn builds_normalized_config() {
        let config = build_config(&valid_input()).unwrap();
        assert_eq!(config["broker_host"], "127.0.0.1");
        assert_eq!(config["broker_port"], 1883);
        assert_eq!(config["client_id"], "red-1");
        assert_eq!(config["broker_username"], "user");
        assert_eq!(config["broker_password"], "pass");
        assert_eq!(config["remote_clients"], json!(["a", "b"]));
    }

    #[test]
    fn rejects_missing_or_invalid_fields() {
        let mut input = valid_input();
        input["client_id"] = json!("bad id!");
        assert!(build_config(&input).is_err());

        let mut input = valid_input();
        input["broker_port"] = json!(0);
        assert!(build_config(&input).is_err());

        let mut input = valid_input();
        input["broker_host"] = json!("  ");
        assert!(build_config(&input).is_err());
    }

    #[test]
    fn username_is_optional() {
        let mut input = valid_input();
        input["broker_username"] = json!("");
        input["broker_password"] = json!("ignored");
        let config = build_config(&input).unwrap();
        assert!(config.get("broker_username").is_none());
        assert!(config.get("broker_password").is_none());
    }
}
