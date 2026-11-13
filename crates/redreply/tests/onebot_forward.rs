//! OneBot 反向接口转发回归测试：
//! 平台事件经框架事件总线转发到 `/onebot/{platform}/{self_id}` 订阅者。
//!
//! 转发必须由 onebot 插件独立完成，因此额外覆盖「redlang 插件禁用」场景，
//! 确保外部 OneBot 客户端不依赖脚本引擎也能收到平台事件。

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
                if stream.read_to_end(&mut raw).await.is_ok() {
                    let text = String::from_utf8_lossy(&raw);
                    if text.starts_with("HTTP/1.1 200") {
                        return;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("框架管理 API 应在 30 秒内就绪");
}

async fn run_forward_scenario(disabled_plugins: &[&str]) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "redreply-onebot-forward-{}-{unique}",
        std::process::id()
    ));
    let plus_dir = temp_dir.join("plus_dir");
    std::fs::create_dir_all(plus_dir.join("default_pkg_dir")).expect("创建临时数据目录失败");

    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("监听假 OneBot 失败");
    let upstream_addr = upstream.local_addr().unwrap();
    let web_port = free_port().await;

    std::fs::write(
        plus_dir.join("config.json"),
        json!({
            "web_host": "127.0.0.1",
            "web_port": web_port,
            "web_password": "",
            "readonly_web_password": "",
            "ws_urls": [format!("ws://{upstream_addr}")],
            "not_open_browser": true,
            "disabled_plugins": disabled_plugins,
        })
        .to_string(),
    )
    .expect("写入 config.json 失败");

    let mut app = AppProcess {
        child: Command::new(env!("CARGO_BIN_EXE_redlang"))
            .env("REDREPLY_PLUS_DIR", &plus_dir)
            .env("REDREPLY_DISABLE_GUI", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 redlang 失败"),
        temp_dir: temp_dir.clone(),
    };

    wait_web_ready(web_port).await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let (stream, _) = upstream.accept().await.expect("等待适配器连接失败");
    let mut adapter_socket = tokio_tungstenite::accept_async(stream)
        .await
        .expect("适配器 WebSocket 握手失败");

    let reverse_stream = TcpStream::connect(("127.0.0.1", web_port))
        .await
        .expect("连接反向接口失败");
    let (mut reverse_socket, _) = tokio_tungstenite::client_async(
        format!("ws://127.0.0.1:{web_port}/onebot/onebot11/10001"),
        reverse_stream,
    )
    .await
    .expect("反向接口 WebSocket 握手失败");

    // 等反向接口的生命周期事件，确保会话已注册
    let lifecycle = tokio::time::timeout(Duration::from_secs(10), reverse_socket.next())
        .await
        .expect("等待生命周期事件超时");
    let lifecycle_text = lifecycle
        .expect("反向接口关闭")
        .expect("读取生命周期事件失败")
        .into_text()
        .expect("生命周期事件非文本");
    assert!(
        lifecycle_text.contains("lifecycle"),
        "首条应为生命周期事件：{lifecycle_text}"
    );

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let event = json!({
        "post_type": "message",
        "message_type": "group",
        "sub_type": "normal",
        "self_id": 10001,
        "group_id": 20002,
        "user_id": 30003,
        "message_id": 40004,
        "time": now,
        "sender": {"nickname": "转发测试", "user_id": 30003},
        "message": [{"type": "text", "data": {"text": "hello-forward"}}],
        "raw_message": "hello-forward"
    });
    adapter_socket
        .send(Message::Text(event.to_string().into()))
        .await
        .expect("发送上游事件失败");

    loop {
        let next = tokio::time::timeout(Duration::from_secs(15), reverse_socket.next())
            .await
            .expect("等待转发事件超时");
        match next {
            Some(Ok(Message::Text(text))) => {
                let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if value.get("post_type").and_then(Value::as_str) == Some("message") {
                    let message = value.get("raw_message").cloned().unwrap_or(Value::Null);
                    assert!(
                        message.to_string().contains("hello-forward"),
                        "转发事件内容不匹配：{text}"
                    );
                    let _ = app.child.kill();
                    let _ = app.child.wait();
                    return;
                }
            }
            Some(Ok(Message::Ping(payload))) => {
                let _ = reverse_socket.send(Message::Pong(payload)).await;
            }
            Some(Ok(Message::Close(_))) | None => panic!("反向接口在收到转发事件前关闭"),
            Some(Err(error)) => panic!("反向接口错误：{error}"),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn platform_event_is_forwarded_to_onebot_reverse_ws() {
    run_forward_scenario(&[]).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn forwarding_works_without_redlang_plugin() {
    run_forward_scenario(&["redlang"]).await;
}
