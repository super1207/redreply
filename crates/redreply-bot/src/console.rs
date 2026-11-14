//! 框架管理台：内置 WebUI（`/`）与管理 API（`/api/*`）。
//!
//! 管理台属于框架特殊层，负责：
//! - 插件列表 / 启用禁用 / 启停 / 状态；
//! - 适配器工厂与连接配置（后续阶段）；
//! - 服务列表与日志查看；
//! - 登录鉴权（与旧版密码 Cookie 兼容）。

use std::sync::{Arc, Weak};

use hyper::{Method, StatusCode};
use serde_json::json;

use crate::framework::FrameworkInner;
use crate::http::auth::encode_cookie_value;
use crate::http::{
    bytes_response, collect_body, error_response, json_response, redirect_response,
    redirect_with_cookie, HttpError, RouteAccess,
};

#[derive(rust_embed::RustEmbed)]
#[folder = "res/webui"]
struct ConsoleAssets;

pub(crate) fn register(inner: &Arc<FrameworkInner>) {
    let http = inner.http.clone();
    let weak: Weak<FrameworkInner> = Arc::downgrade(inner);

    // 管理台首页：`/` 与 `/index.html` 是同一个页面。
    for path in ["/", "/index.html"] {
        http.route_owned(
            "framework",
            Method::GET,
            path,
            RouteAccess::Write,
            move |_request, _context| async move {
                match ConsoleAssets::get("index.html") {
                    Some(file) => Ok(bytes_response(
                        StatusCode::OK,
                        "text/html; charset=utf-8",
                        file.data,
                        None,
                    )),
                    None => Err(HttpError::not_found()),
                }
            },
        );
    }

    register_login_routes(&http, &weak);
    register_api_routes(&http, &weak);
}

async fn read_json_body(
    request: hyper::Request<hyper::body::Incoming>,
) -> std::result::Result<serde_json::Value, HttpError> {
    let (_, body) = request.into_parts();
    let bytes = collect_body(body, 256 * 1024).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))
}

fn register_login_routes(http: &Arc<crate::http::HttpService>, weak: &Weak<FrameworkInner>) {
    http.route_owned(
        "framework",
        Method::GET,
        "/login.html",
        RouteAccess::Public,
        move |_request, _context| async move {
            match ConsoleAssets::get("login.html") {
                Some(file) => Ok(bytes_response(
                    StatusCode::OK,
                    "text/html; charset=utf-8",
                    file.data,
                    None,
                )),
                None => Err(HttpError::not_found()),
            }
        },
    );

    let login_action = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/login",
        RouteAccess::Public,
        move |request, _context| {
            let weak = login_action.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let auth = inner.http.auth_snapshot();
                let (_, body) = request.into_parts();
                let bytes = collect_body(body, 8192).await?;
                let mut password = String::new();
                for (key, value) in url::form_urlencoded::parse(&bytes) {
                    if key == "password" {
                        password = value.into_owned();
                    }
                }
                let valid = auth.is_open()
                    || (!auth.web_password.is_empty() && password == auth.web_password)
                    || (!auth.readonly_web_password.is_empty()
                        && password == auth.readonly_web_password);
                if valid {
                    let cookie = format!(
                        "password={}; Max-Age=31536000; Path=/",
                        encode_cookie_value(&password)
                    );
                    Ok(redirect_with_cookie("/", &cookie))
                } else {
                    inner.logger.warn("console", "登录失败：密码错误");
                    Ok(redirect_response("/login.html?error=1"))
                }
            }
        },
    );

    http.route_owned(
        "framework",
        Method::POST,
        "/logout",
        RouteAccess::Public,
        move |_request, _context| async move {
            Ok(redirect_with_cookie(
                "/login.html",
                "password=; Max-Age=0; Path=/",
            ))
        },
    );
}

fn register_api_routes(http: &Arc<crate::http::HttpService>, weak: &Weak<FrameworkInner>) {
    let overview = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/overview",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = overview.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let data = json!({
                    "name": "redreply-bot",
                    "version": inner.version,
                    "state": inner.state(),
                    "http_addr": inner.server_addr().map(|addr| addr.to_string()),
                    "app_dir": inner.app_dir.to_string_lossy(),
                    "plugins": inner.plugins.list(),
                    "adapters": {
                        "factories": inner.adapters.list(),
                        "entries": inner.adapters.entries(),
                        "bots": inner.adapters.bots(),
                    },
                    "menus": inner.menus.list(),
                    "services": inner
                        .services
                        .names()
                        .into_iter()
                        .map(|name| {
                            json!({
                                "name": name,
                                "owner": inner.services.owner(&name),
                            })
                        })
                        .collect::<Vec<_>>(),
                    "subscriptions": inner.events.subscriber_count(),
                    "routes": inner.http.route_count(),
                    "static_mounts": inner.http.static_count(),
                });
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": data}),
                ))
            }
        },
    );

    let plugins = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/plugins",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = plugins.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": inner.plugins.list()}),
                ))
            }
        },
    );

    let plugin_action = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/plugins/{name}/{action}",
        RouteAccess::Write,
        move |_request, context| {
            let weak = plugin_action.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let name = context
                    .params
                    .get("name")
                    .cloned()
                    .ok_or_else(|| HttpError::bad_request("缺少插件名"))?;
                let action = context
                    .params
                    .get("action")
                    .cloned()
                    .ok_or_else(|| HttpError::bad_request("缺少操作名"))?;
                let result = match action.as_str() {
                    "enable" => inner.plugins.set_enabled(&name, true),
                    "disable" => inner.plugins.set_enabled(&name, false),
                    "start" => inner.plugins.start(&name),
                    "stop" => inner.plugins.stop(&name),
                    _ => {
                        return Err(HttpError::not_found());
                    }
                };
                if let Err(error) = result {
                    return Ok(error_response(
                        StatusCode::CONFLICT,
                        error.message().to_string(),
                    ));
                }
                match action.as_str() {
                    "enable" => inner.plugins.persist_enabled(&name, true),
                    "disable" => inner.plugins.persist_enabled(&name, false),
                    _ => {}
                }
                let status = inner.plugins.get(&name);
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": status}),
                ))
            }
        },
    );

    let adapters = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/adapters",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = adapters.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let data = json!({
                    "factories": inner.adapters.list(),
                    "entries": inner.adapters.entries(),
                    "bots": inner.adapters.bots(),
                });
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": data}),
                ))
            }
        },
    );

    let add_url = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/adapters/add",
        RouteAccess::Write,
        move |request, _context| {
            let weak = add_url.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let body = read_json_body(request).await?;
                let url = body
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .ok_or_else(|| HttpError::bad_request("缺少 url"))?
                    .to_string();
                inner.adapters.add_url(&url);
                inner.adapters.persist_entries(&inner.adapters.entries());
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": {"url": url}}),
                ))
            }
        },
    );

    let update_url = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/adapters/update",
        RouteAccess::Write,
        move |request, _context| {
            let weak = update_url.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let body = read_json_body(request).await?;
                let old_url = body
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| HttpError::bad_request("缺少 url"))?;
                let new_url = body
                    .get("new_url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .ok_or_else(|| HttpError::bad_request("缺少 new_url"))?;
                if inner
                    .adapters
                    .entries()
                    .iter()
                    .any(|entry| entry.url == new_url && entry.url != old_url)
                {
                    return Err(HttpError::conflict("该连接已存在"));
                }
                if !inner.adapters.update_url(old_url, new_url) {
                    return Err(HttpError::not_found());
                }
                inner.adapters.persist_entries(&inner.adapters.entries());
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": {"url": new_url}}),
                ))
            }
        },
    );

    let toggle = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/adapters/toggle",
        RouteAccess::Write,
        move |request, _context| {
            let weak = toggle.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let body = read_json_body(request).await?;
                let url = body
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| HttpError::bad_request("缺少 url"))?;
                let enabled = body
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| HttpError::bad_request("缺少 enabled"))?;
                if !inner.adapters.set_enabled(url, enabled) {
                    return Err(HttpError::not_found());
                }
                inner.adapters.persist_entries(&inner.adapters.entries());
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": {"url": url, "enabled": enabled}}),
                ))
            }
        },
    );

    let reconnect = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/adapters/reconnect",
        RouteAccess::Write,
        move |request, _context| {
            let weak = reconnect.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let body = read_json_body(request).await?;
                let url = body
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| HttpError::bad_request("缺少 url"))?;
                inner.adapters.reconnect(url);
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": {"url": url}}),
                ))
            }
        },
    );

    let remove = weak.clone();
    http.route_owned(
        "framework",
        Method::POST,
        "/api/adapters/remove",
        RouteAccess::Write,
        move |request, _context| {
            let weak = remove.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let body = read_json_body(request).await?;
                let url = body
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| HttpError::bad_request("缺少 url"))?
                    .to_string();
                inner.adapters.disconnect_url(&url).await;
                inner.adapters.persist_entries(&inner.adapters.entries());
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": {"url": url}}),
                ))
            }
        },
    );

    let menus = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/menus",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = menus.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": inner.menus.list()}),
                ))
            }
        },
    );

    let services = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/services",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = services.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let list: Vec<_> = inner
                    .services
                    .names()
                    .into_iter()
                    .map(|name| {
                        json!({
                            "name": name,
                            "owner": inner.services.owner(&name),
                        })
                    })
                    .collect();
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": list}),
                ))
            }
        },
    );

    let logs = weak.clone();
    http.route_owned(
        "framework",
        Method::GET,
        "/api/logs",
        RouteAccess::Read,
        move |_request, _context| {
            let weak = logs.clone();
            async move {
                let inner = weak
                    .upgrade()
                    .ok_or_else(|| HttpError::internal("框架已停止"))?;
                let mut history = inner.logger.history();
                let limit = 100.min(history.len());
                let records: Vec<_> = history
                    .split_off(history.len() - limit)
                    .into_iter()
                    .map(|mut record| {
                        // 单条日志过长（如协议端原始报文）时截断，避免管理台每 5 秒拉取几百 KB
                        const MAX_CHARS: usize = 500;
                        if record.message.chars().count() > MAX_CHARS {
                            record.message =
                                record.message.chars().take(MAX_CHARS).collect::<String>() + "…";
                        }
                        record
                    })
                    .collect();
                Ok(json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": records}),
                ))
            }
        },
    );
}
