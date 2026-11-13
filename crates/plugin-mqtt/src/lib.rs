//! MQTT 分布式插件：多实例事件共享与远程 API 调用。

use std::sync::Arc;

use redreply_bot::{Plugin, PluginContext, PluginMeta, Result};
use redreply_common::plugin_ports::MqttServiceHandle;

mod config;
mod mqtt;

/// MQTT 插件。
pub struct MqttPlugin;

impl Plugin for MqttPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("mqtt", env!("CARGO_PKG_VERSION"))
            .description("MQTT 5.0 分布式：跨实例事件/API 互通")
            .provides("mqtt")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        // 远端事件直接注入框架事件总线，由各插件按自己的订阅处理；
        // 这样 OneBot 转发、脚本管线都不依赖 redlang 的远端事件钩子。
        mqtt::set_event_bus(ctx.events_handle());
        config::register(ctx)
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        if let Err(error) = mqtt::init_mqttclient() {
            ctx.logger()
                .warn("mqtt", format!("MQTT 初始化失败：{error}"));
            return Ok(());
        }
        ctx.register_service(
            "mqtt",
            Arc::new(MqttServiceHandle(Arc::new(mqtt::MqttPort))),
        )?;
        ctx.logger().info("mqtt", "MQTT 插件已启动");
        Ok(())
    }
}
