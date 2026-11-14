//! 框架 + 真实 OneBot11 适配器端到端测试：
//! 假 OneBot 服务端 ← 适配器桥 ← framework AdapterManager，验证
//! 事件进入框架 EventBus、call_api 能到达协议端。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use redreply_adapter::framework_bridge::onebot11_factory;
use redreply_bot::{Event, EventAction, EventFilter, Framework};

async fn wait_for<F: FnMut() -> bool>(mut condition: F, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn framework_manages_onebot11_adapter_end_to_end() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_addr = listener.local_addr().unwrap();

    let events = Arc::new(AtomicUsize::new(0));
    let last_event: Arc<Mutex<Option<Event>>> = Arc::new(Mutex::new(None));

    let framework = Framework::builder()
        .adapter(onebot11_factory())
        .http_addr("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();

    {
        let events = events.clone();
        let last_event = last_event.clone();
        framework
            .events()
            .subscribe("test", 0, EventFilter::new(), move |event| {
                events.fetch_add(1, Ordering::SeqCst);
                *last_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(event.clone());
                EventAction::Continue
            });
    }

    framework
        .adapters()
        .set_urls(vec![format!("ws://{ws_addr}")]);
    framework.start().unwrap();

    // 假 OneBot 服务端：连接后发群消息，并对所有 API 请求回包
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    let scenario = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("等待适配器连接失败");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("WebSocket 握手失败");
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
            "sender": {"nickname": "适配器测试", "user_id": 30003},
            "message": [{"type": "text", "data": {"text": "ping"}}],
            "raw_message": "ping"
        });
        socket
            .send(Message::Text(event.to_string().into()))
            .await
            .expect("发送事件失败");
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                message = socket.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                            if let Some(echo) = value.get("echo") {
                                let reply = json!({
                                    "status": "ok",
                                    "retcode": 0,
                                    "data": {"user_id": 10001},
                                    "echo": echo
                                });
                                let _ = socket.send(Message::Text(reply.to_string().into())).await;
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            let _ = socket.send(Message::Pong(payload)).await;
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
        }
    });

    assert!(
        wait_for(
            || framework.adapters().bots().iter().any(|bot| bot.alive),
            Duration::from_secs(5)
        )
        .await,
        "OneBot11 适配器应连接成功"
    );
    assert!(
        wait_for(
            || events.load(Ordering::SeqCst) >= 1,
            Duration::from_secs(5)
        )
        .await,
        "适配器事件应进入框架 EventBus"
    );
    {
        let event = last_event
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap();
        assert_eq!(event.platform, "onebot11");
        assert_eq!(event.self_id, "10001");
        assert_eq!(event.post_type, "message");
    }

    // 框架 -> 适配器 call_api（假服务端负责回包）
    let mut request = json!({"action": "get_login_info", "params": {}});
    let result = framework
        .adapters()
        .call_api("onebot11", "10001", "", &mut request)
        .await
        .expect("call_api 不应出错")
        .expect("应有可用连接");
    assert_eq!(result["retcode"], 0);

    let _ = stop_tx.send(());
    let _ = scenario.await;
    framework.shutdown().unwrap();
}
