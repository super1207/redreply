//! 通用 HTTP 反向代理：/proxy?url=<目标URL>
//! 浏览器跨域请求经本地服务转发到目标服务器，响应原样回传并附加 CORS 头。
//! 请求的 method/headers/body 原样透传；响应 status/headers/body 原样回传。

use std::sync::OnceLock;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use http_body_util::StreamBody;
use hyper::body::{Frame, Incoming};
use hyper::http::HeaderValue;
use hyper::{Method, Request, Response, StatusCode};

use super::{BoxBody, GenericError, Result};

/// 逐跳头（不得透传）
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

/// 请求体上限（32MB）
const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
/// 响应体上限（64MB，超出截断）
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

/// 全局复用客户端（连接池 + 超时 + 重定向）
static PROXY_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn proxy_client() -> &'static reqwest::Client {
    PROXY_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::limited(10))
            .pool_max_idle_per_host(8)
            .build()
            .expect("proxy client build failed")
    })
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    http_body_util::Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

/// 统一错误响应（JSON）
fn json_error(message: &str) -> Response<BoxBody> {
    let body = serde_json::json!({ "retcode": -1, "data": { "message": message } });
    let mut resp = Response::new(full(body.to_string()));
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp
}

/// CORS 预检响应
fn preflight_response() -> Response<BoxBody> {
    let mut resp = Response::new(full(""));
    *resp.status_mut() = StatusCode::NO_CONTENT;
    resp.headers_mut()
        .insert("access-control-allow-origin", HeaderValue::from_static("*"));
    resp.headers_mut()
        .insert("access-control-allow-headers", HeaderValue::from_static("*"));
    resp.headers_mut().insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS"),
    );
    resp.headers_mut()
        .insert("access-control-max-age", HeaderValue::from_static("86400"));
    resp
}

/// 从 query 解析目标 URL
fn target_url(request: &Request<Incoming>) -> Option<String> {
    let query = request.uri().query()?;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        if k == "url" {
            let url = v.into_owned();
            if url.starts_with("http://") || url.starts_with("https://") {
                return Some(url);
            }
            return None;
        }
    }
    None
}

/// 读取请求体（带上限）
async fn read_body(mut body: Incoming, limit: usize) -> Result<Bytes> {
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame?;
        if let Some(chunk) = frame.data_ref() {
            if data.len() + chunk.len() > limit {
                return Err("请求体过大".into());
            }
            data.extend_from_slice(chunk);
        }
    }
    Ok(Bytes::from(data))
}

/// /proxy 主处理：透传转发并回传
pub async fn handle(request: Request<Incoming>) -> Result<Response<BoxBody>> {
    // CORS 预检
    if request.method() == Method::OPTIONS {
        return Ok(preflight_response());
    }

    // 解析目标 URL
    let url = match target_url(&request) {
        Some(url) => url,
        None => return Ok(json_error("缺少 url 参数或格式不正确")),
    };

    // 构造转发请求：method + headers（过滤逐跳头）+ body
    // User-Agent 特殊处理：浏览器 JS 无法设置 UA（forbidden header），
    // 前端通过 X-Redreply-UA 头传递期望的 UA，由本服务覆盖转发请求的 User-Agent。
    let mut builder = proxy_client().request(request.method().clone(), url);
    let mut custom_ua: Option<String> = None;
    let mut original_ua: Option<String> = None;
    for (name, value) in request.headers() {
        let lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) {
            continue;
        }
        if lower == "x-redreply-ua" {
            custom_ua = value.to_str().ok().map(|s| s.to_owned());
            continue; // 代理指令，不透传给上游
        }
        if lower == "user-agent" {
            original_ua = value.to_str().ok().map(|s| s.to_owned());
            continue; // 稍后统一设置（custom_ua 优先）
        }
        builder = builder.header(name.as_str(), value.clone());
    }
    if let Some(ua) = custom_ua.or(original_ua) {
        builder = builder.header("user-agent", ua);
    }
    let body = read_body(request.into_body(), MAX_REQUEST_BYTES).await?;
    if !body.is_empty() {
        builder = builder.body(body);
    }

    let built = builder.build().map_err(|e| -> GenericError { Box::new(e) })?;
    let resp = match proxy_client().execute(built).await {
        Ok(resp) => resp,
        Err(e) => return Ok(json_error(&format!("转发失败: {e}"))),
    };

    // 回传：status + headers（过滤逐跳头与原有 CORS 头）+ 附加 CORS 头
    let mut resp_builder = Response::builder().status(resp.status());
    for (name, value) in resp.headers() {
        let lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) || lower.starts_with("access-control-") {
            continue;
        }
        resp_builder = resp_builder.header(name.as_str(), value.clone());
    }
    let resp_builder = resp_builder
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-headers", "*")
        .header(
            "access-control-allow-methods",
            "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
        );

    // 流式回传（截断上限）
    let stream = resp
        .bytes_stream()
        .take(MAX_RESPONSE_BYTES as usize)
        .map(|item| {
            item.map(Frame::data)
                .map_err(|e| -> GenericError { Box::new(e) })
        });
    let body = StreamBody::new(stream);
    Ok(resp_builder.body(BoxBody::new(body))?)
}
