//! `/onebot/{platform}/{self_id}` WebSocket 服务端。
//!
//! 旧实现位于宿主 `httpserver` 的兜底分支，随 OneBot 能力整体迁入插件。

use std::collections::HashMap;
use std::time::SystemTime;

use futures_util::{SinkExt, StreamExt};
use redlang::config_manager::read_web_password;
use redlang::cqapi::{cq_add_log, cq_add_log_w, cq_call_api};
use redlang::runtime_context;
use redreply_bot::http::full;
use redreply_bot::{HttpError, HttpResponse};

fn get_params_from_uri(uri: &hyper::Uri) -> HashMap<String, String> {
    let mut ret_map = HashMap::new();
    if let Some(query) = uri.query() {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            ret_map.insert(key.into_owned(), value.into_owned());
        }
    }
    ret_map
}

/// 处理 `/onebot/*` 的 WS 升级（框架路由入口）。
pub async fn handle_request(
    request: hyper::Request<hyper::body::Incoming>,
    is_local: bool,
) -> Result<HttpResponse, HttpError> {
    if !hyper_tungstenite::is_upgrade_request(&request) {
        return Ok(hyper::Response::new(full("broken http")));
    }
    let url_path = request.uri().path().to_string();

    let mut access_token = String::new();
    let params = get_params_from_uri(request.uri());
    if let Some(ac) = params.get("access_token") {
        access_token = ac.to_owned();
    } else if let Some(au) = request.headers().get("Authorization") {
        access_token = au
            .to_str()
            .map_err(|error| HttpError::bad_request(error.to_string()))?
            .to_owned();
    }

    let web_pass_raw =
        read_web_password().map_err(|error| HttpError::internal(error.to_string()))?;

    // 需要鉴权
    if !web_pass_raw.is_empty() && !is_local && !access_token.contains(&web_pass_raw) {
        return Ok(hyper::Response::new(full("broken http")));
    }

    let parts = url_path.split('/').collect::<Vec<&str>>();
    let platform = parts.get(2).map(ToString::to_string).unwrap_or_default();
    let self_id = parts.get(3).map(ToString::to_string).unwrap_or_default();

    let (response, websocket) = hyper_tungstenite::upgrade(request, None)
        .map_err(|error| HttpError::internal(error.to_string()))?;
    // 开启一个线程来处理ws
    tokio::spawn(async move {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);
        let ws_uid = uuid::Uuid::new_v4().to_string();
        {
            runtime_context()
                .session
                .add_onebot_session(ws_uid.to_owned(), tx, platform.clone(), self_id.clone())
                .await;
        }
        if let Err(e) =
            serve_onebot_websocket(websocket, rx, platform, self_id, ws_uid.clone()).await
        {
            let _ = cq_add_log_w(&format!("onebots Error in websocket connection: {}", e));
        }
        {
            runtime_context()
                .session
                .remove_onebot_session(&ws_uid)
                .await;
        }
    });

    let headers = response.headers();
    let mut switching = hyper::Response::builder()
        .body(full("switching to websocket protocol"))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    *switching.status_mut() = response.status();
    *switching.headers_mut() = headers.to_owned();
    Ok(switching)
}

async fn serve_onebot_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
    platform: String,
    self_id: String,
    ws_uid: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = cq_add_log("connect to onebot");

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half) = futures_util::StreamExt::split(ws_stream);

    // 发送生命周期事件
    let tm = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let connect_event = serde_json::json!({
        "time":tm,
        "self_id":self_id,
        "post_type":"meta_event",
        "meta_event_type":"lifecycle",
        "sub_type":"connect"
    })
    .to_string();

    write_half
        .send(hyper_tungstenite::tungstenite::Message::Text(
            connect_event.into(),
        ))
        .await?;

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // 当所有tx被释放时,tx.recv会返回None
            let rst = write_half
                .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
                .await;
            if rst.is_err() {
                let _ = cq_add_log_w("serve send1 of onebot err");
                break;
            }
        }
        let _ = cq_add_log_w("serve send2 of onebot err");
    });

    let ret = deal_msg(read_half, platform, self_id, ws_uid).await;
    if ret.is_err() {
        let err = ret.err().unwrap_or_else(|| "unknown".into());
        let _ = cq_add_log_w(&format!("serve recv of onebot err:{err:?}"));
    }

    Ok(())
}

async fn deal_msg(
    mut read_half: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>,
    >,
    platform: String,
    self_id: String,
    ws_uid: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while let Some(msg_t) = read_half.next().await {
        // 转json
        let msg_text = msg_t?.to_text()?.to_owned();
        let req_json_rst = serde_json::from_str(&msg_text);
        let req_json: serde_json::Value = match req_json_rst {
            Ok(value) => value,
            Err(error) => {
                let _ = cq_add_log_w(&format!("req json err:{error:?}"));
                continue;
            }
        };
        let echo = req_json.get("echo").cloned();
        let root_rst = crate::convert::request_to_red(&req_json, &platform, &self_id);
        let (root, passive_id) = match root_rst {
            Ok(value) => value,
            Err(err) => {
                let message = format!("can't convert to red req:{err:?}");
                let _ = cq_add_log_w(&message);
                // 转换失败也必须回包，否则客户端只能等到超时（send_group_msg 尤其明显）
                let response = crate::convert::error_ret(&message, echo);
                runtime_context()
                    .session
                    .send_onebot_api_ret(&ws_uid, response.to_string(), &platform, &self_id)
                    .await;
                continue;
            }
        };

        let self_id_t = self_id.clone();
        let platform_t = platform.clone();
        let ws_uid_t = ws_uid.clone();

        tokio::spawn(async move {
            let self_id = self_id_t.clone();
            let platform = platform_t.clone();

            let root_str = root.to_string();

            let echo_for_call = echo.clone();
            let ret = tokio::task::spawn_blocking(move || {
                let ret = cq_call_api(
                    &platform.clone(),
                    &self_id.clone(),
                    &passive_id,
                    &root_str,
                    "",
                );
                let ret_red_json: serde_json::Value = serde_json::from_str(&ret)
                    .map_err(|err| format!("onebot api返回了无效的JSON:{err}\n原始内容:{ret}"))?;
                Ok::<serde_json::Value, String>(crate::convert::red_ret_to_ob(
                    ret_red_json,
                    echo_for_call,
                ))
            })
            .await;
            let response = match ret {
                Ok(Ok(response)) => response,
                Ok(Err(err)) => {
                    let _ = cq_add_log_w(&format!("err:{err}"));
                    crate::convert::error_ret(&err, echo)
                }
                Err(err) => {
                    let _ = cq_add_log_w(&format!("err:{err:?}"));
                    crate::convert::error_ret(&err.to_string(), echo)
                }
            };
            // 定向回包给发起请求的这个会话，避免多客户端接入时互相干扰
            runtime_context()
                .session
                .send_onebot_api_ret(&ws_uid_t, response.to_string(), &platform_t, &self_id_t)
                .await;
        });
    }
    Ok(())
}
