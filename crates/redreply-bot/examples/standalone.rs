//! 独立运行框架的示例（不修改现有主程序）：
//!
//! ```text
//! cargo run -p redreply-bot --example standalone
//! ```
//!
//! 启动后访问 http://127.0.0.1:12071 打开框架管理台。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hyper::{Method, StatusCode};
use serde_json::json;

use redreply_bot::http::json_response;
use redreply_bot::{
    EventAction, EventFilter, Framework, Plugin, PluginContext, PluginMeta, Result, RouteAccess,
    StaticAssets, StaticFile,
};

struct DemoService {
    #[allow(dead_code)]
    greeting: String,
}

struct DemoPlugin {
    events: Arc<AtomicUsize>,
}

impl Plugin for DemoPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("demo", "0.1.0")
            .description("演示插件：注册服务、路由、静态资源与事件订阅")
            .provides("demo")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.register_service(
            "demo",
            Arc::new(DemoService {
                greeting: "你好".to_string(),
            }),
        )?;

        let events = self.events.clone();
        ctx.subscribe(0, EventFilter::new(), move |_event| {
            events.fetch_add(1, Ordering::SeqCst);
            EventAction::Continue
        });

        let events = self.events.clone();
        ctx.route(
            Method::GET,
            "/demo/api/events",
            RouteAccess::Read,
            move |_request, _context| {
                let events = events.clone();
                async move {
                    Ok(json_response(
                        StatusCode::OK,
                        &json!({"retcode": 0, "data": events.load(Ordering::SeqCst)}),
                    ))
                }
            },
        );

        ctx.mount_static(
            "/demo/static",
            RouteAccess::Public,
            StaticAssets::from_fn(|path| {
                if path == "hello.txt" {
                    Some(StaticFile::new(path, "demo static\n"))
                } else {
                    None
                }
            }),
        );
        Ok(())
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info("demo", "演示插件已启动");
        Ok(())
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let framework = Framework::builder()
        .http_host_port("127.0.0.1", 12071)?
        .plugin(DemoPlugin {
            events: Arc::new(AtomicUsize::new(0)),
        })
        .build()?;
    framework.start()?;

    if let Some(addr) = framework.http_addr() {
        println!("框架管理台：http://{addr}");
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
