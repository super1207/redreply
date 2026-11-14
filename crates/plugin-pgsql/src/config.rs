//! PostgreSQL 配置页与读写 API：`/pgsql/`、`/pgsql/api/config`。
//!
//! 配置写入 `config.json` 的 `postgresql` 字段，重启后由插件启动逻辑读取。

use hyper::body::Incoming;
use hyper::{Method, StatusCode};
use redreply_bot::http::{bytes_response, collect_body, json_response};
use redreply_bot::{HttpError, HttpResponse, MenuItem, PluginContext, Result, RouteAccess};
use serde_json::{json, Value};

const PAGE: &str = include_str!("../page.html");

/// 注册配置菜单、页面与读写接口。
pub fn register(ctx: &PluginContext) -> Result<()> {
    ctx.register_menu(MenuItem::new("config", "配置", "/pgsql/"))?;
    ctx.route(
        Method::GET,
        "/pgsql/",
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
        "/pgsql/api/config",
        RouteAccess::Write,
        |_request, _context| async { Ok(current_config()) },
    );
    ctx.route(
        Method::POST,
        "/pgsql/api/config",
        RouteAccess::Write,
        |request, _context| async { save_config(request).await },
    );
    ctx.route(
        Method::POST,
        "/pgsql/api/config/delete",
        RouteAccess::Write,
        |_request, _context| async { clear_config() },
    );
    Ok(())
}

fn current_config() -> HttpResponse {
    let value = redlang::config_manager::read_config()
        .ok()
        .and_then(|config| config.get("postgresql").cloned())
        .filter(|value| !value.is_null())
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
    redlang::config_manager::set_config_key("postgresql", config)
        .map_err(|error| HttpError::internal(format!("保存配置失败：{error}")))?;
    Ok(saved_response("已保存，重启软件后生效"))
}

fn clear_config() -> std::result::Result<HttpResponse, HttpError> {
    redlang::config_manager::set_config_key("postgresql", Value::Null)
        .map_err(|error| HttpError::internal(format!("清空配置失败：{error}")))?;
    Ok(saved_response("已清空，重启软件后生效"))
}

fn saved_response(message: &str) -> HttpResponse {
    json_response(
        StatusCode::OK,
        &json!({"retcode": 0, "data": {"message": message}}),
    )
}

/// 由表单 JSON 构造规范化的 `postgresql` 配置。
fn build_config(input: &Value) -> std::result::Result<Value, String> {
    let host = required_str(input, "host", "请填写数据库主机")?;
    let port = input
        .get("port")
        .and_then(Value::as_u64)
        .filter(|port| (1..=65535).contains(port))
        .ok_or("端口必须是 1-65535")?;
    let user = required_str(input, "user", "请填写用户名")?;
    let dbname = required_str(input, "dbname", "请填写数据库名")?;
    let password = input.get("password").and_then(Value::as_str).unwrap_or("");

    Ok(json!({
        "host": host,
        "port": port,
        "user": user,
        "password": password,
        "dbname": dbname,
    }))
}

fn required_str(input: &Value, key: &str, message: &str) -> std::result::Result<String, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::build_config;
    use serde_json::json;

    fn valid_input() -> serde_json::Value {
        json!({
            "host": " localhost ",
            "port": 5432,
            "user": "postgres",
            "password": "p w d",
            "dbname": "redreply"
        })
    }

    #[test]
    fn builds_normalized_config() {
        let config = build_config(&valid_input()).unwrap();
        assert_eq!(config["host"], "localhost");
        assert_eq!(config["port"], 5432);
        assert_eq!(config["user"], "postgres");
        assert_eq!(config["password"], "p w d");
        assert_eq!(config["dbname"], "redreply");
    }

    #[test]
    fn rejects_missing_or_invalid_fields() {
        let mut input = valid_input();
        input["host"] = json!("");
        assert!(build_config(&input).is_err());

        let mut input = valid_input();
        input["port"] = json!(70000);
        assert!(build_config(&input).is_err());

        let mut input = valid_input();
        input["dbname"] = json!(null);
        assert!(build_config(&input).is_err());
    }

    #[test]
    fn password_is_optional() {
        let mut input = valid_input();
        input["password"] = json!(null);
        let config = build_config(&input).unwrap();
        assert_eq!(config["password"], "");
    }
}
