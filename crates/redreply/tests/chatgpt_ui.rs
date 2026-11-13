//! ChatGPT 配置页「调用明细」选项卡与 API 回归测试。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

async fn http_get(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("连接失败");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("写入请求失败");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .expect("读取响应失败");
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

#[tokio::test(flavor = "multi_thread")]
async fn chatgpt_config_page_has_call_detail_tab() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "redreply-chatgpt-ui-{}-{unique}",
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
            "ws_urls": [],
            "not_open_browser": true
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
        temp_dir,
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut ready = false;
    while tokio::time::Instant::now() < deadline {
        let (status, _) = http_get(web_port, "/api/overview").await;
        if status == 200 {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(ready, "框架管理 API 应在 30 秒内就绪");

    let (status, body) = http_get(web_port, "/chatgpt/").await;
    assert_eq!(status, 200);
    assert!(body.contains("调用明细"), "配置页应包含调用明细选项卡");
    assert!(body.contains("最近触发"), "配置页应包含最近触发选项卡");
    assert!(body.contains("ChatGPT 配置"), "配置页应包含配置选项卡");

    let (status, body) = http_get(web_port, "/chatgpt/api/calls").await;
    assert_eq!(status, 200);
    let value: Value = serde_json::from_str(&body).expect("调用明细 API 应返回 JSON");
    assert_eq!(value["retcode"], 0);
    assert_eq!(value["data"]["capacity"], 500);
    assert!(value["data"]["records"].is_array());
    assert!(value["data"]["stats"].is_object());

    let (status, body) = http_get(web_port, "/chatgpt/api/last_trigger").await;
    assert_eq!(status, 200);
    let value: Value = serde_json::from_str(&body).expect("最近触发 API 应返回 JSON");
    assert_eq!(value["retcode"], 0);
    assert!(value["data"].is_null());

    let _ = app.child.kill();
    let _ = app.child.wait();
}
