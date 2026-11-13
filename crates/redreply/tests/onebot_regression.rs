//! 旧版链路回归基线（迁移保护网）：
//! 启动真实 redlang 进程，连接假 OneBot11 服务端，验证
//! 「平台事件 → 脚本匹配执行 → send_group_msg 回复」全链路。
//!
//! 依赖两个测试钩子：
//! - `REDREPLY_PLUS_DIR`：把数据目录重定向到临时目录；
//! - `REDREPLY_DISABLE_GUI`：Windows 下跳过托盘 / WebView 窗口。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
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

/// 极简 HTTP 客户端（测试用）。
async fn http_request(port: u16, method: &str, path: &str) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("连接失败");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("写入失败");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("读取失败");
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn message_to_text(message: &Value) -> String {
    match message {
        Value::String(text) => text.clone(),
        Value::Array(segments) => segments
            .iter()
            .map(|segment| {
                segment
                    .get("data")
                    .and_then(|data| data.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

/// 假 OneBot11 服务端：发一条群消息，等待脚本回复。
async fn run_scenario(listener: TcpListener) -> Result<(), String> {
    let (stream, _) = listener
        .accept()
        .await
        .map_err(|error| format!("等待适配器连接失败：{error}"))?;
    let mut socket = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|error| format!("WebSocket 握手失败：{error}"))?;

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
        "sender": {"nickname": "回归测试", "user_id": 30003},
        "message": [{"type": "text", "data": {"text": "ping"}}],
        "raw_message": "ping"
    });
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .map_err(|error| format!("发送事件失败：{error}"))?;

    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                let value: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if value.get("action").and_then(Value::as_str) == Some("send_group_msg") {
                    let params = &value["params"];
                    let group_id = params
                        .get("group_id")
                        .map(value_to_string)
                        .unwrap_or_default();
                    if group_id != "20002" {
                        return Err(format!("回复群号不匹配：{group_id}"));
                    }
                    let message = params.get("message").cloned().unwrap_or(Value::Null);
                    let message_text = message_to_text(&message);
                    if !message_text.contains("pong") {
                        return Err(format!("回复内容不匹配：{message_text}"));
                    }
                    return Ok(());
                }
                // 对适配器心跳等 API 请求回包，维持连接与识别
                if let Some(echo) = value.get("echo") {
                    let reply = json!({
                        "status": "ok",
                        "retcode": 0,
                        "data": {"user_id": 10001, "nickname": "fake-onebot"},
                        "echo": echo
                    });
                    let _ = socket.send(Message::Text(reply.to_string().into())).await;
                }
            }
            Some(Ok(Message::Ping(payload))) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err("连接在收到回复前关闭".to_string());
            }
            Some(Err(error)) => return Err(format!("WebSocket 错误：{error}")),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn group_message_triggers_script_and_sends_reply() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "redreply-regression-{}-{unique}",
        std::process::id()
    ));
    let plus_dir = temp_dir.join("plus_dir");
    let pkg_dir = plus_dir.join("default_pkg_dir");
    std::fs::create_dir_all(&pkg_dir).expect("创建临时数据目录失败");

    let ws_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("监听假 OneBot 失败");
    let ws_addr = ws_listener.local_addr().unwrap();
    let web_port = free_port().await;

    std::fs::write(
        plus_dir.join("config.json"),
        json!({
            "web_host": "127.0.0.1",
            "web_port": web_port,
            "web_password": "",
            "readonly_web_password": "",
            "ws_urls": [format!("ws://{ws_addr}")],
            "not_open_browser": true
        })
        .to_string(),
    )
    .expect("写入 config.json 失败");

    std::fs::write(
        pkg_dir.join("script.json"),
        json!([{
            "name": "regression_ping",
            "description": "回归测试脚本",
            "content": {
                "关键词": "ping",
                "匹配方式": "完全匹配",
                "触发方式": "群、私聊触发",
                "code": "pong"
            }
        }])
        .to_string(),
    )
    .expect("写入 script.json 失败");

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

    let scenario = tokio::time::timeout(Duration::from_secs(90), run_scenario(ws_listener)).await;
    // 先结束子进程再断言，避免失败时残留进程
    let _ = app.child.kill();
    let _ = app.child.wait();

    match scenario {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("回归场景失败：{error}"),
        Err(_) => panic!("回归场景超时（90s）"),
    }
}

/// 应用级验收：框架管理台、插件状态/启停持久化、redlang 路径别名。
#[tokio::test(flavor = "multi_thread")]
async fn app_runs_on_framework_with_plugins_and_aliases() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "redreply-framework-{}-{unique}",
        std::process::id()
    ));
    let plus_dir = temp_dir.join("plus_dir");
    std::fs::create_dir_all(plus_dir.join("default_pkg_dir")).expect("创建临时数据目录失败");
    let web_port = free_port().await;
    std::fs::write(
        plus_dir.join("config.json"),
        format!(
            r#"{{"web_host":"127.0.0.1","web_port":{web_port},"web_password":"","readonly_web_password":"","ws_urls":[],"not_open_browser":true}}"#
        ),
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

    // 等待框架 HTTP 就绪
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut overview_body = String::new();
    let mut ready = false;
    while tokio::time::Instant::now() < deadline {
        let (status, body) = http_request(web_port, "GET", "/api/overview").await;
        if status == 200 {
            overview_body = body;
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(ready, "框架管理 API 应在 30 秒内就绪");

    let overview: Value = serde_json::from_str(&overview_body).expect("overview 应为 JSON");
    let plugins = overview["data"]["plugins"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for name in ["redlang", "tts", "browser", "mqtt", "pgsql", "onebot"] {
        assert!(
            plugins
                .iter()
                .any(|plugin| plugin["name"] == name && plugin["running"] == true),
            "插件 {name} 应处于运行状态"
        );
    }

    // redlang 插件应在框架管理台注册菜单入口
    let menus = overview["data"]["menus"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        menus
            .iter()
            .any(|menu| menu["label"] == "脚本管理" && menu["url"] == "/redlang/"),
        "管理台应包含 redlang 的脚本管理菜单：{menus:?}"
    );

    // 管理台首页应为框架控制台
    let (status, body) = http_request(web_port, "GET", "/").await;
    assert_eq!(status, 200);
    assert!(body.contains("框架管理台"), "根路径应返回框架管理台");

    // redlang 插件路径：页面、日志页与 API
    let (status, body) = http_request(web_port, "GET", "/redlang/").await;
    assert_eq!(status, 200);
    assert!(!body.is_empty(), "redlang 页面不应为空");
    let (status, _) = http_request(web_port, "GET", "/redlang/watchlog.html").await;
    assert_eq!(status, 200);
    let (status, body) = http_request(web_port, "GET", "/redlang/api/get_version").await;
    assert_eq!(status, 200);
    assert!(body.contains("1.0.86"), "版本接口应返回版本号：{body}");

    // 框架自有端点：文档与图标
    let (status, _) = http_request(web_port, "GET", "/docs/index.html").await;
    assert_eq!(status, 200);
    let (status, _) = http_request(web_port, "GET", "/favicon.ico").await;
    assert_eq!(status, 200);

    // 停用 tts 插件：服务被回收并持久化到 config.json
    let (status, _) = http_request(web_port, "POST", "/api/plugins/tts/disable").await;
    assert_eq!(status, 200);
    let (_, services_body) = http_request(web_port, "GET", "/api/services").await;
    assert!(
        !services_body.contains("\"tts\""),
        "停用后 tts 服务应被回收：{services_body}"
    );
    let config = std::fs::read_to_string(plus_dir.join("config.json")).expect("读取配置失败");
    assert!(
        config.contains("disabled_plugins") && config.contains("tts"),
        "禁用状态应写入配置：{config}"
    );

    let _ = app.child.kill();
    let _ = app.child.wait();
}
