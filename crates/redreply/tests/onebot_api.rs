//! OneBot 反向接口 API 回归测试：
//! - 普通群（无 `groups_id`）的 `group_id` 请求必须能到达上游适配器；
//! - 请求转换失败时必须返回错误回包，而不是让客户端等到超时。
//!
//! 这两个场景对应真实对接中「send_group_msg 未收到响应」和
//! 「get_group_member_list 全部 10s 超时」的根因。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

struct AppProcess {
    child: Child,
    temp_dir: std::path::PathBuf,
}

impl Drop for AppProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("预留端口失败");
    listener.local_addr().unwrap().port()
}

async fn wait_web_ready(port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await {
            let request = format!(
                "GET /api/overview HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            );
            if stream.write_all(request.as_bytes()).await.is_ok() {
                let mut raw = Vec::new();
                if stream.read_to_end(&mut raw).await.is_ok()
                    && String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 200") {
                        return;
                    }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("框架管理 API 应在 30 秒内就绪");
}

/// 启动一份无头 redlang，返回进程与数据目录。
async fn start_app(ws_urls: Vec<String>) -> (AppProcess, u16) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "redreply-onebot-api-{}-{unique}",
        std::process::id()
    ));
    let plus_dir = temp_dir.join("plus_dir");
    std::fs::create_dir_all(plus_dir.join("default_pkg_dir")).expect("创建临时数据目录失败");

    let web_port = free_port().await;
    std::fs::write(
        plus_dir.join("config.json"),
        json!({
            "web_host": "127.0.0.1",
            "web_port": web_port,
            "web_password": "",
            "readonly_web_password": "",
            "ws_urls": ws_urls,
            "not_open_browser": true
        })
        .to_string(),
    )
    .expect("写入 config.json 失败");

    let app = AppProcess {
        child: Command::new(env!("CARGO_BIN_EXE_redlang"))
            .env("REDREPLY_PLUS_DIR", &plus_dir)
            .env("REDREPLY_DISABLE_GUI", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 redlang 失败"),
        temp_dir,
    };
    wait_web_ready(web_port).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    (app, web_port)
}

async fn connect_reverse(web_port: u16) -> tokio_tungstenite::WebSocketStream<TcpStream> {
    let stream = TcpStream::connect(("127.0.0.1", web_port))
        .await
        .expect("连接反向接口失败");
    let (mut socket, _) = tokio_tungstenite::client_async(
        format!("ws://127.0.0.1:{web_port}/onebot/onebot11/10001"),
        stream,
    )
    .await
    .expect("反向接口 WebSocket 握手失败");
    let lifecycle = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .expect("等待生命周期事件超时");
    let text = lifecycle
        .expect("反向接口关闭")
        .expect("读取生命周期事件失败")
        .into_text()
        .expect("生命周期事件非文本");
    assert!(text.contains("lifecycle"), "首条应为生命周期事件：{text}");
    socket
}

async fn next_json(socket: &mut tokio_tungstenite::WebSocketStream<TcpStream>) -> Value {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(value) = serde_json::from_str(&text) {
                    return value;
                }
            }
            Some(Ok(Message::Ping(payload))) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Some(Ok(Message::Close(_))) | None => panic!("反向接口提前关闭"),
            Some(Err(error)) => panic!("反向接口错误：{error}"),
            _ => {}
        }
    }
}

/// 假上游：收到目标 action 时返回请求；其它带 echo 的探测请求自动回包。
async fn accept_action(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    action: &str,
) -> Value {
    loop {
        let value = next_json(socket).await;
        if value.get("action").and_then(Value::as_str) == Some(action) {
            return value;
        }
        if let Some(echo) = value.get("echo").cloned() {
            let reply = json!({
                "status": "ok",
                "retcode": 0,
                "data": {"app_name": "SnowLuma", "app_version": "test"},
                "echo": echo
            });
            let _ = socket.send(Message::Text(reply.to_string().into())).await;
        }
    }
}

async fn reply_ok(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    echo: Value,
) {
    socket
        .send(Message::Text(
            json!({
                "status": "ok",
                "retcode": 0,
                "data": {"message_id": 12345},
                "echo": echo
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("上游回包失败");
}

#[tokio::test(flavor = "multi_thread")]
async fn group_api_requests_reach_upstream_and_reply() {
    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("监听假 OneBot 失败");
    let upstream_addr = upstream.local_addr().unwrap();
    let (_app, web_port) = start_app(vec![format!("ws://{upstream_addr}")]).await;

    let (stream, _) = upstream.accept().await.expect("等待适配器连接失败");
    let mut upstream_socket = tokio_tungstenite::accept_async(stream)
        .await
        .expect("适配器 WebSocket 握手失败");
    // 心跳让适配器获知 self_id；心跳不会进入事件管线
    upstream_socket
        .send(Message::Text(
            json!({
                "post_type": "meta_event",
                "meta_event_type": "heartbeat",
                "self_id": 10001,
                "time": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送适配器心跳失败");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut reverse = connect_reverse(web_port).await;

    // send_group_msg：普通群 group_id 应原样透传到上游
    reverse
        .send(Message::Text(
            json!({
                "action": "send_group_msg",
                "params": {
                    "group_id": 20002,
                    "message": [{"type": "text", "data": {"text": "hello"}}]
                },
                "echo": 1
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 send_group_msg 失败");

    let forwarded = tokio::time::timeout(
        Duration::from_secs(5),
        accept_action(&mut upstream_socket, "send_group_msg"),
    )
    .await
    .expect("上游未收到 send_group_msg（转换层丢弃了请求）");
    assert_eq!(forwarded["params"]["group_id"], json!(20002));
    reply_ok(
        &mut upstream_socket,
        forwarded.get("echo").cloned().unwrap_or(Value::Null),
    )
    .await;

    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = next_json(&mut reverse).await;
            if value.get("echo") == Some(&json!(1)) {
                return value;
            }
        }
    })
    .await
    .expect("反向客户端未收到 send_group_msg 响应");
    assert_eq!(response["status"], "ok");

    // get_group_member_list：同一套 group_id 转换路径
    reverse
        .send(Message::Text(
            json!({
                "action": "get_group_member_list",
                "params": {"group_id": 20002, "no_cache": false},
                "echo": 2
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 get_group_member_list 失败");

    let forwarded = tokio::time::timeout(
        Duration::from_secs(5),
        accept_action(&mut upstream_socket, "get_group_member_list"),
    )
    .await
    .expect("上游未收到 get_group_member_list");
    // get_group_member_list 没有额外的能力探测，直接回包即可
    upstream_socket
        .send(Message::Text(
            json!({
                "status": "ok",
                "retcode": 0,
                "data": [],
                "echo": forwarded.get("echo").cloned().unwrap_or(Value::Null)
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("上游回包失败");

    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = next_json(&mut reverse).await;
            if value.get("echo") == Some(&json!(2)) {
                return value;
            }
        }
    })
    .await
    .expect("反向客户端未收到 get_group_member_list 响应");
    assert_eq!(response["status"], "ok");
    assert_eq!(response["data"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn conversion_failure_returns_error_response() {
    let (_app, web_port) = start_app(Vec::new()).await;
    let mut reverse = connect_reverse(web_port).await;

    // 未知 message_id 无法映射回内部消息，转换应失败
    reverse
        .send(Message::Text(
            json!({
                "action": "get_msg",
                "params": {"message_id": 123456789},
                "echo": 7
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 get_msg 失败");

    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = next_json(&mut reverse).await;
            if value.get("echo") == Some(&json!(7)) {
                return value;
            }
        }
    })
    .await
    .expect("转换失败时客户端应在超时前收到错误回包");

    assert_eq!(response["status"], "failed");
    assert_eq!(response["retcode"], -1);
    assert!(response["wording"]
        .as_str()
        .unwrap_or_default()
        .contains("message_id"));
}
