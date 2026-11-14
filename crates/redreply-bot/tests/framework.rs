//! 框架 P0 集成测试：真实 TCP 请求覆盖管理台、鉴权（回环）、
//! 插件路由/静态资源与插件生命周期（管理 API 启停）。

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hyper::{Method, StatusCode};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use redreply_bot::{
    EventAction, EventFilter, Framework, MenuItem, Plugin, PluginContext, PluginMeta, Result,
    RouteAccess, StaticAssets, StaticFile,
};

struct TestService {
    #[allow(dead_code)]
    greeting: String,
}

struct TestPlugin {
    events: Arc<AtomicUsize>,
}

impl Plugin for TestPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("test", "0.1.0")
            .description("集成测试插件")
            .provides("test")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.register_service(
            "test",
            Arc::new(TestService {
                greeting: "你好".to_string(),
            }),
        )?;

        let events = self.events.clone();
        ctx.subscribe(5, EventFilter::new(), move |_event| {
            events.fetch_add(1, Ordering::SeqCst);
            EventAction::Continue
        });

        ctx.register_menu(MenuItem::new("panel", "测试面板", "/test/ping").order(5))?;

        ctx.route(
            Method::GET,
            "/test/ping",
            RouteAccess::Read,
            move |_request, _context| async move {
                Ok(redreply_bot::http::json_response(
                    StatusCode::OK,
                    &json!({"retcode": 0, "data": "pong"}),
                ))
            },
        );

        ctx.mount_static(
            "/test/static",
            RouteAccess::Public,
            StaticAssets::from_fn(|path| {
                if path == "hello.txt" {
                    Some(StaticFile::new(path, "static ok\n"))
                } else {
                    None
                }
            }),
        );
        Ok(())
    }
}

struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl RawResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|error| panic!("响应不是 JSON：{error}\n{}", self.body))
    }
}

async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> RawResponse {
    let mut stream = TcpStream::connect(addr).await.expect("连接失败");
    let mut raw_request =
        format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        raw_request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        raw_request.push_str(&format!(
            "Content-Length: {}\r\nContent-Type: application/x-www-form-urlencoded\r\n",
            body.len()
        ));
    }
    raw_request.push_str("\r\n");
    if let Some(body) = body {
        raw_request.push_str(body);
    }
    stream
        .write_all(raw_request.as_bytes())
        .await
        .expect("写入失败");

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("读取失败");
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    RawResponse {
        status,
        headers,
        body: body.to_string(),
    }
}

async fn start_framework(
    plugin: Option<TestPlugin>,
    auth: Option<(&str, &str)>,
) -> (Framework, SocketAddr) {
    let mut builder = Framework::builder().http_addr("127.0.0.1:0".parse().unwrap());
    if let Some(plugin) = plugin {
        builder = builder.plugin(plugin);
    }
    if let Some((write, read)) = auth {
        builder = builder.auth(redreply_bot::AuthConfig::new(write, read));
    }
    let framework = builder.build().expect("构建框架失败");
    framework.start().expect("启动框架失败");
    let addr = framework.http_addr().expect("没有监听地址");
    (framework, addr)
}

#[tokio::test(flavor = "multi_thread")]
async fn console_and_overview_are_served() {
    let (framework, addr) = start_framework(None, None).await;

    let index = request(addr, "GET", "/", &[], None).await;
    assert_eq!(index.status, StatusCode::OK.as_u16());
    assert!(index.body.contains("框架管理台"));

    let overview = request(addr, "GET", "/api/overview", &[], None).await;
    assert_eq!(overview.status, StatusCode::OK.as_u16());
    let payload = overview.json();
    assert_eq!(payload["retcode"], 0);
    assert_eq!(payload["data"]["name"], "redreply-bot");
    assert!(payload["data"]["plugins"].as_array().unwrap().is_empty());

    let missing = request(addr, "GET", "/api/not-exist", &[], None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND.as_u16());

    framework.shutdown().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn login_sets_cookie_and_logout_clears_it() {
    let (framework, addr) = start_framework(None, Some(("secret", "readonly"))).await;

    // 回环地址虽然免鉴权，但登录页与登录接口本身仍然可访问。
    let login_page = request(addr, "GET", "/login.html", &[], None).await;
    assert_eq!(login_page.status, StatusCode::OK.as_u16());
    assert!(login_page.body.contains("请输入管理密码"));

    let login = request(addr, "POST", "/login", &[], Some("password=secret")).await;
    assert_eq!(login.status, StatusCode::FOUND.as_u16());
    let cookie = login
        .header("set-cookie")
        .expect("缺少 Set-Cookie")
        .to_string();
    assert!(cookie.contains("password=secret"));
    assert_eq!(login.header("location"), Some("/"));

    let bad_login = request(addr, "POST", "/login", &[], Some("password=wrong")).await;
    assert_eq!(bad_login.header("location"), Some("/login.html?error=1"));

    let logout = request(addr, "POST", "/logout", &[], None).await;
    assert!(logout.header("set-cookie").is_some());

    framework.shutdown().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_route_static_and_lifecycle() {
    let events = Arc::new(AtomicUsize::new(0));
    let (framework, addr) = start_framework(
        Some(TestPlugin {
            events: events.clone(),
        }),
        None,
    )
    .await;

    // 插件路由与静态资源可用。
    let ping = request(addr, "GET", "/test/ping", &[], None).await;
    assert_eq!(ping.status, StatusCode::OK.as_u16());
    assert_eq!(ping.json()["data"], "pong");

    let static_file = request(addr, "GET", "/test/static/hello.txt", &[], None).await;
    assert_eq!(static_file.status, StatusCode::OK.as_u16());
    assert_eq!(static_file.body, "static ok\n");

    // 服务、菜单与事件订阅生效。
    let overview = request(addr, "GET", "/api/overview", &[], None).await;
    let services = overview.json()["data"]["services"].clone();
    assert_eq!(services, json!([{"name": "test", "owner": "test"}]));
    let menus = overview.json()["data"]["menus"].clone();
    assert_eq!(menus.as_array().unwrap().len(), 1);
    assert_eq!(menus[0]["label"], "测试面板");
    assert_eq!(menus[0]["url"], "/test/ping");
    assert_eq!(menus[0]["plugin"], "test");

    let event = redreply_bot::Event::from_json(json!({
        "platform": "onebot11",
        "self_id": "1",
        "post_type": "message"
    }))
    .unwrap();
    framework.events().publish(&event);
    assert_eq!(events.load(Ordering::SeqCst), 1);

    // 通过管理 API 禁用插件：路由/服务/订阅都被回收。
    let disable = request(addr, "POST", "/api/plugins/test/disable", &[], None).await;
    assert_eq!(disable.status, StatusCode::OK.as_u16());
    let ping_after = request(addr, "GET", "/test/ping", &[], None).await;
    assert_eq!(ping_after.status, StatusCode::NOT_FOUND.as_u16());
    let overview_after = request(addr, "GET", "/api/overview", &[], None).await;
    assert!(overview_after.json()["data"]["services"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(overview_after.json()["data"]["menus"]
        .as_array()
        .unwrap()
        .is_empty());
    framework.events().publish(&event);
    assert_eq!(events.load(Ordering::SeqCst), 1);

    // 重新启用后恢复。
    let enable = request(addr, "POST", "/api/plugins/test/enable", &[], None).await;
    assert_eq!(enable.status, StatusCode::OK.as_u16());
    let ping_again = request(addr, "GET", "/test/ping", &[], None).await;
    assert_eq!(ping_again.status, StatusCode::OK.as_u16());
    framework.events().publish(&event);
    assert_eq!(events.load(Ordering::SeqCst), 2);

    framework.shutdown().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_plugin_is_not_started() {
    let events = Arc::new(AtomicUsize::new(0));
    let framework = Framework::builder()
        .http_addr("127.0.0.1:0".parse().unwrap())
        .plugin(TestPlugin {
            events: events.clone(),
        })
        .disable_plugin("test")
        .build()
        .unwrap();
    framework.start().unwrap();
    let addr = framework.http_addr().unwrap();

    let ping = request(addr, "GET", "/test/ping", &[], None).await;
    assert_eq!(ping.status, StatusCode::NOT_FOUND.as_u16());
    let overview = request(addr, "GET", "/api/overview", &[], None).await;
    assert_eq!(overview.json()["data"]["plugins"][0]["enabled"], false);

    framework.shutdown().unwrap();
}
