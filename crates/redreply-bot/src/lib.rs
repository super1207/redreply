//! 红色问答 bot 框架核心。
//!
//! 框架负责两件特殊层的事情：
//! - 连接并管理平台适配器（[`AdapterManager`]）；
//! - 管理插件生命周期（[`PluginManager`]）。
//!
//! 其余能力（脚本引擎、TTS、浏览器等）都以普通插件形式接入。
//! 框架提供统一的事件总线、服务注册表、日志以及单端口 HTTP 服务，
//! 插件通过 [`PluginContext`] 注册路由/服务/事件订阅。

mod adapter;
mod console;
mod error;
mod event;
mod framework;
mod logger;
mod menu;
mod plugin;
mod service;

pub mod http;

pub use adapter::{
    AdapterDeps, AdapterEntry, AdapterFactory, AdapterInfo, AdapterManager, Bot, BotHandle, BotInfo,
};
pub use error::{FrameworkError, Result};
pub use event::{Event, EventAction, EventBus, EventFilter, PublishReport, SubscriptionId};
pub use framework::{Framework, FrameworkBuilder};
pub use http::{
    AccessLevel, AuthConfig, HttpError, HttpResponse, HttpService, RouteAccess, RouteContext,
    ServerHandle, StaticAssets, StaticFile,
};
pub use logger::{LogLevel, LogRecord, Logger};
pub use menu::{MenuItem, MenuRegistry};
pub use plugin::{Plugin, PluginContext, PluginManager, PluginMeta, PluginStatus};
pub use service::ServiceRegistry;

/// 框架版本号。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
