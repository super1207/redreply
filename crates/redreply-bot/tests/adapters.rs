//! AdapterManager 集成测试：mock 工厂 + 连接对账 + call_api 路由。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use redreply_bot::{
    AdapterDeps, AdapterFactory, Bot, BotHandle, Framework, FrameworkError, Result,
};

struct MockBot {
    platform: String,
    self_id: String,
    identities: Vec<(String, String)>,
    alive: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    last_route: Arc<Mutex<(String, String)>>,
}

#[async_trait]
impl Bot for MockBot {
    fn platform(&self) -> String {
        self.platform.clone()
    }

    fn self_id(&self) -> String {
        self.self_id.clone()
    }

    fn identities(&self) -> Vec<(String, String)> {
        self.identities.clone()
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn connect(&mut self) -> Result<()> {
        self.alive.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        _passive_id: &str,
        request: Value,
    ) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_route.lock().unwrap_or_else(|e| e.into_inner()) =
            (platform.to_string(), self_id.to_string());
        Ok(json!({"retcode": 0, "data": request}))
    }
}

struct MockFactory {
    platform: String,
    alive: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    last_route: Arc<Mutex<(String, String)>>,
}

impl AdapterFactory for MockFactory {
    fn name(&self) -> &str {
        "mock"
    }

    fn schemes(&self) -> Vec<&'static str> {
        vec!["mock"]
    }

    fn build(&self, _url: &str, _deps: AdapterDeps) -> Result<BotHandle> {
        Ok(Arc::new(tokio::sync::RwLock::new(MockBot {
            platform: self.platform.clone(),
            self_id: "10001".to_string(),
            identities: vec![
                (self.platform.clone(), "10001".to_string()),
                ("mock_sub".to_string(), "10002".to_string()),
            ],
            alive: self.alive.clone(),
            calls: self.calls.clone(),
            last_route: self.last_route.clone(),
        })))
    }
}

struct FailingFactory;

impl AdapterFactory for FailingFactory {
    fn name(&self) -> &str {
        "failing"
    }

    fn schemes(&self) -> Vec<&'static str> {
        vec!["fail"]
    }

    fn build(&self, _url: &str, _deps: AdapterDeps) -> Result<BotHandle> {
        Err(FrameworkError::Plugin("mock 构建失败".to_string()))
    }
}

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
async fn adapter_connects_calls_api_and_disconnects() {
    let alive = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let last_route = Arc::new(Mutex::new((String::new(), String::new())));
    let framework = Framework::builder()
        .adapter(Arc::new(MockFactory {
            platform: "mock".to_string(),
            alive: alive.clone(),
            calls: calls.clone(),
            last_route: last_route.clone(),
        }))
        .http_addr("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    framework
        .adapters()
        .set_urls(vec!["mock://one".to_string()]);
    framework.start().unwrap();

    assert!(
        wait_for(
            || framework.adapters().bots().iter().any(|bot| bot.alive),
            Duration::from_secs(3)
        )
        .await,
        "适配器应连接成功"
    );
    let bots = framework.adapters().bots();
    assert_eq!(bots[0].platform, "mock");
    assert_eq!(bots[0].self_id, "10001");

    let mut request = json!({
        "action": "send_group_msg",
        "params": {"group_id": "1", "message": []}
    });
    let result = framework
        .adapters()
        .call_api("mock", "10001", "passive-1", &mut request)
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 多身份连接：第二个身份也能路由，且匹配到的身份会回传给适配器
    let result = framework
        .adapters()
        .call_api("mock_sub", "10002", "passive-2", &mut request)
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *last_route.lock().unwrap_or_else(|e| e.into_inner()),
        ("mock_sub".to_string(), "10002".to_string())
    );

    // 平台不匹配时返回 None，且不会调用机器人。
    let result = framework
        .adapters()
        .call_api("other", "", "", &mut request)
        .await
        .unwrap();
    assert!(result.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    framework.adapters().disconnect_url("mock://one").await;
    assert!(framework.adapters().bots().is_empty());
    assert!(!alive.load(Ordering::SeqCst));

    framework.shutdown().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn failing_adapter_does_not_block_others() {
    let alive = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let framework = Framework::builder()
        .adapter(Arc::new(FailingFactory))
        .adapter(Arc::new(MockFactory {
            platform: "mock".to_string(),
            alive: alive.clone(),
            calls: calls.clone(),
            last_route: Arc::new(Mutex::new((String::new(), String::new()))),
        }))
        .http_addr("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    framework
        .adapters()
        .set_urls(vec!["fail://broken".to_string(), "mock://good".to_string()]);
    framework.start().unwrap();

    assert!(
        wait_for(
            || framework
                .adapters()
                .bots()
                .iter()
                .any(|bot| bot.url == "mock://good" && bot.alive),
            Duration::from_secs(3)
        )
        .await,
        "失败的适配器不应阻塞正常适配器连接"
    );
    let bots = framework.adapters().bots();
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0].url, "mock://good");

    framework.shutdown().unwrap();
}
