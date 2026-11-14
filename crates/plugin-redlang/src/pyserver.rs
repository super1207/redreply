//! Python 桥：sidecar 客户端调用（call_py）与 `/pyserver` WebSocket 服务端。

use futures_util::{SinkExt, StreamExt};
use redlang::cqapi::{cq_add_log, cq_add_log_w};
use redlang::mytool::read_json_str;
use redlang::runtime;
use redlang::runtime_context;
use redreply_bot::{HttpError, HttpResponse};

async fn send_to_ser(code: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let hand = runtime_context()
        .session
        .python_handler()
        .await
        .ok_or("not have python env")?;
    hand.send(code).await?;
    Ok(())
}

pub fn call_py_block(code: &str, input: &str) -> String {
    runtime().block_on(async {
        let rst = call_py(code, input).await;
        match rst {
            Ok(s) => s.to_string(),
            Err(err) => {
                let _ = cq_add_log_w(&err.to_string());
                "".to_string()
            }
        }
    })
}

async fn call_py(
    code: &str,
    input: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let uid = uuid::Uuid::new_v4().to_string();
    let send_json = serde_json::json!({
        "echo":uid,
        "code":code,
        "input":input
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel::<serde_json::Value>(1);
    {
        runtime_context()
            .session
            .add_python_echo(uid.clone(), tx)
            .await;
    }
    let _guard = scopeguard::guard(uid, |uid| {
        runtime().spawn(async move {
            runtime_context().session.remove_python_echo(&uid).await;
        });
    });
    let ret = send_to_ser(send_json.to_string()).await;
    if ret.is_err() {
        let _ = cq_add_log_w(&format!("call_py err:{:?}", ret.err()));
        return Ok(serde_json::Value::String(String::new()));
    }
    tokio::select! {
        std::option::Option::Some(val) = rx.recv() => {
            Ok(val)
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(90)) => {
            let _ = cq_add_log_w("接收python返回超时");
            Ok(serde_json::Value::String(String::new()))
        }
    }
}

/// `/pyserver` 的 WS 升级处理（框架路由入口）。
pub async fn handle_ws_upgrade(
    request: hyper::Request<hyper::body::Incoming>,
) -> Result<HttpResponse, HttpError> {
    if !hyper_tungstenite::is_upgrade_request(&request) {
        return Err(HttpError::bad_request("broken http"));
    }
    let mut conf = tungstenite::protocol::WebSocketConfig::default();
    // 保持 100MB：pyserver 需要传输文件等大块数据
    conf.max_frame_size = Some(1024 * 1024 * 100);
    let (response, websocket) = hyper_tungstenite::upgrade(request, Some(conf))
        .map_err(|error| HttpError::internal(error.to_string()))?;

    tokio::spawn(async move {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);
        // 注册处理器并拿到代次号：第二个 python 客户端接入时，
        // 旧连接凭代次号变化感知被替换并主动退出，避免孤儿连接残留
        let generation = runtime_context().session.set_python_handler(tx).await;
        if let Err(e) = serve_py_websocket(websocket, rx, generation).await {
            let _ = cq_add_log_w(&format!("pyserver Error in websocket connection: {}", e));
        }
    });

    let headers = response.headers();
    let mut switching = hyper::Response::builder()
        .body(redreply_bot::http::empty())
        .map_err(|error| HttpError::internal(error.to_string()))?;
    *switching.status_mut() = response.status();
    *switching.headers_mut() = headers.to_owned();
    Ok(switching)
}

async fn serve_py_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
    generation: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = cq_add_log("connect to pyserver");

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half) = futures_util::StreamExt::split(ws_stream);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // 当所有tx被释放时,tx.recv会返回None
            let rst = write_half
                .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
                .await;
            if rst.is_err() {
                runtime_context()
                    .session
                    .clear_python_handler_if_current(generation)
                    .await;
                let _ = cq_add_log_w(&format!("serve send1 of python err:{:?}", rst.err()));
                break;
            }
        }
        let _ = cq_add_log_w("serve send2 of python err");
    });

    let ret = deal_py_msg(read_half, generation).await;
    if ret.is_err() {
        let err = ret.err().unwrap_or_else(|| "unknown".into());
        let _ = cq_add_log_w(&format!("serve recv of python err:{err:?}"));
    }
    // 只清理本代次的处理器，避免旧连接退出时抹掉新连接刚注册的处理器
    runtime_context()
        .session
        .clear_python_handler_if_current(generation)
        .await;

    Ok(())
}

async fn deal_py_msg(
    mut read_half: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>,
    >,
    generation: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut consecutive_errors = 0u32;
    while let Some(msg_t) = read_half.next().await {
        {
            if !runtime_context().session.has_python_handler().await {
                break;
            }
            // 被新连接替换时主动退出，避免旧连接成为孤儿会话
            if runtime_context().session.python_handler_generation().await != Some(generation) {
                break;
            }
        }
        let msg_text = match msg_t {
            Ok(msg) => match msg.to_text() {
                Ok(text) => text.to_owned(),
                Err(e) => {
                    // 单个坏帧只记录并跳过，连续出错才断开，
                    // 不让一条畸形消息杀死整条 python 桥接
                    consecutive_errors += 1;
                    let _ = cq_add_log_w(&format!("pyserver 坏帧({consecutive_errors}):{e}"));
                    if consecutive_errors >= 10 {
                        return Err("连续收到过多坏帧".into());
                    }
                    continue;
                }
            },
            Err(e) => return Err(Box::new(e)),
        };
        consecutive_errors = 0;

        if msg_text == "opened" {
            let _ = cq_add_log("python环境已经连接！");
            continue;
        }

        let js: serde_json::Value = match serde_json::from_str(&msg_text) {
            Ok(js) => js,
            Err(e) => {
                consecutive_errors += 1;
                let _ = cq_add_log_w(&format!("pyserver 消息解析失败({consecutive_errors}):{e}"));
                if consecutive_errors >= 10 {
                    return Err("连续收到过多无法解析的消息".into());
                }
                continue;
            }
        };
        consecutive_errors = 0;
        let echo = read_json_str(&js, "echo");
        let data = js
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::String(String::new()));
        let _ = runtime_context()
            .session
            .send_python_echo(&echo, data)
            .await;
    }
    Ok(())
}
