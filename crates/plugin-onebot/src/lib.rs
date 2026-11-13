//! OneBot 反向接口插件：对外提供 `/onebot/{platform}/{self_id}` WebSocket 服务。
//!
//! 外部程序通过该接口可以订阅平台事件、调用已连接平台的 API；
//! 事件/请求转换与 ID 映射（原宿主 `onebot11s`/`onebot_store`）都在插件内部。
//! 事件转发由本插件自行订阅框架事件总线完成，插件停用后路由与订阅一起回收。

use hyper::Method;
use redreply_bot::{
    Event, EventAction, EventFilter, Plugin, PluginContext, PluginMeta, Result, RouteAccess,
};

mod convert;
mod forward;
mod store;
mod ws;

/// 转发订阅优先级：先于脚本管线把事件送给外部 OneBot 客户端，
/// 避免脚本执行耗时拖慢反向接口。
const FORWARD_PRIORITY: i32 = 10;

/// OneBot 反向接口插件。
pub struct OneBotPlugin;

impl Plugin for OneBotPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("onebot", env!("CARGO_PKG_VERSION"))
            .description("OneBot 反向接口：外部程序接入红色问答的平台能力")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.route(
            Method::GET,
            "/onebot/*",
            RouteAccess::Public,
            |request, context| async move {
                crate::ws::handle_request(request, context.is_local).await
            },
        );

        // 转发由插件自己完成：不依赖 redlang 是否启用，
        // 本地适配器事件与 MQTT 远端事件都会进入事件总线。
        let runtime = ctx.runtime();
        ctx.subscribe(
            FORWARD_PRIORITY,
            EventFilter::new(),
            move |event: &Event| {
                forward::forward_event(event, &runtime);
                EventAction::Continue
            },
        );
        Ok(())
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info("onebot", "OneBot 反向接口插件已就绪");
        Ok(())
    }
}
