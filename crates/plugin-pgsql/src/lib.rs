//! PostgreSQL 插件：事件消息落库。

use std::sync::Arc;

use redreply_bot::{Plugin, PluginContext, PluginMeta, Result};
use redreply_common::plugin_ports::PgServiceHandle;

mod config;
mod pg;

/// PostgreSQL 插件。
pub struct PgsqlPlugin;

impl Plugin for PgsqlPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("pgsql", env!("CARGO_PKG_VERSION"))
            .description("PostgreSQL 事件持久化")
            .provides("pgsql")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        config::register(ctx)
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        if let Err(error) = pg::init_postgresql_db() {
            ctx.logger()
                .warn("pgsql", format!("PostgreSQL 初始化失败：{error}"));
            return Ok(());
        }
        ctx.register_service("pgsql", Arc::new(PgServiceHandle(Arc::new(pg::PgPort))))?;
        ctx.logger().info("pgsql", "PostgreSQL 插件已启动");
        Ok(())
    }
}
