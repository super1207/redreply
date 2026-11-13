//! 宿主 HTTP 兜底处理器：日志 WebSocket。
//!
//! 脚本/编辑台相关 API 在 `plugin-redlang`；OneBot 反向接口在 `plugin-onebot`。

use crate::config_manager::{read_readonly_web_password, read_web_password};
use crate::cqapi::{cq_add_log_w, get_history_log};
use crate::runtime;
use crate::runtime_context;
use bytes::Bytes;
use futures_util::SinkExt;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::Response;

pub type GenericError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, GenericError>;
pub type Result<T> = std::result::Result<T, GenericError>;

pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    http_body_util::Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

pub fn add_ws_log(log_msg: String) {
    runtime().spawn(async move {
        runtime_context().session.broadcast_log(log_msg).await;
    });
}
async fn serve_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<()> {
    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, mut _read_half) = futures_util::StreamExt::split(ws_stream);

    while let Some(msg) = rx.recv().await {
        // 当所有tx被释放时,tx.recv会返回None
        // log::info!("recive:{}",msg);
        write_half
            .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
            .await?;
    }
    Ok(())
}

async fn http_auth(request: &hyper::Request<Incoming>) -> Result<i32> {
    let web_pass_raw =
        tokio::task::spawn_blocking(move || -> Result<String> { read_web_password() }).await??;

    if web_pass_raw.is_empty() {
        return Ok(2);
    }
    let headers = request.headers();
    let cookie_str = headers
        .get("Cookie")
        .ok_or("can not found Cookie")?
        .to_str()?;
    {
        let web_pass: String =
            url::form_urlencoded::byte_serialize(web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}", web_pass)) {
            return Ok(2);
        }
    }
    let read_only_web_pass_raw =
        tokio::task::spawn_blocking(move || -> Result<String> { read_readonly_web_password() })
            .await??;
    if read_only_web_pass_raw.is_empty() {
        return Ok(1);
    }
    {
        let web_pass: String =
            url::form_urlencoded::byte_serialize(read_only_web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}", web_pass)) {
            return Ok(1);
        }
    }
    Err("password invaild".into())
}


/// 框架接管 HTTP 服务后，平台侧未迁移路径的统一入口。
pub async fn legacy_handle(
    request: hyper::Request<hyper::body::Incoming>,
    is_local: bool,
) -> Result<Response<BoxBody>> {
    let url_path = request.uri().path().to_string();

    // 日志与 Python sidecar 的 WebSocket：远程需要写权限
    let can_write = if is_local {
        true
    } else {
        matches!(http_auth(&request).await, Ok(2))
    };
    if hyper_tungstenite::is_upgrade_request(&request) {
        if url_path == "/watch_log" {
            // 没有写权限不允许访问log
            if !can_write {
                let res = hyper::Response::new(full("api not found"));
                return Ok(res);
            }

            // ws协议升级返回
            let (response, websocket) = hyper_tungstenite::upgrade(request, None)?;
            let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);

            let history_log = get_history_log();
            for it in history_log {
                tx.send(it).await?;
            }

            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let uid = uuid::Uuid::new_v4().to_string();
                runtime_context()
                    .session
                    .add_log_session(uid.clone(), tx)
                    .await;
                if let Err(e) = serve_websocket(websocket, rx).await {
                    let _ = cq_add_log_w(&format!("watchlog Error in websocket connection: {}", e));
                }
                // 线程结束，删除对应的entry
                runtime_context().session.remove_log_session(&uid).await;
            });
            let headers = response.headers();
            let mut rrr = Response::builder().body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            Ok(rrr)
        } else {
            Ok(hyper::Response::new(full("broken http")))
        }
    } else {
        Ok(hyper::Response::new(full("not found")))
    }
}
