use std::any::Any;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Method, Request};
use serde::Serialize;
use tokio::runtime::Handle;

use crate::adapter::AdapterManager;
use crate::error::{FrameworkError, Result};
use crate::event::{Event, EventAction, EventBus, EventFilter, SubscriptionId};
use crate::framework::FrameworkEnv;
use crate::http::{
    HttpError, HttpResponse, HttpService, RouteAccess, RouteContext, StaticAssets,
};
use crate::logger::Logger;
use crate::menu::{MenuItem, MenuRegistry};
use crate::service::ServiceRegistry;

/// 插件元信息。
#[derive(Clone, Debug)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    /// 本插件提供的服务名。
    pub provides: Vec<String>,
    /// 本插件依赖的服务名（在 start 前校验）。
    pub requires: Vec<String>,
}

impl PluginMeta {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: String::new(),
            provides: Vec::new(),
            requires: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn provides(mut self, service: impl Into<String>) -> Self {
        self.provides.push(service.into());
        self
    }

    pub fn requires(mut self, service: impl Into<String>) -> Self {
        self.requires.push(service.into());
        self
    }
}

/// 普通插件接口。
///
/// 生命周期：`register`（注册服务/路由/事件订阅）→ `start`（启动后台任务）
/// → `stop`（停止后台任务）；禁用插件时框架会按 owner 回收其注册的资源。
pub trait Plugin: Send + Sync + 'static {
    fn meta(&self) -> PluginMeta;

    /// 注册扩展点；只在首次启用时调用一次。
    fn register(&self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }

    /// 启动插件后台任务。
    fn start(&self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }

    /// 停止插件后台任务。
    fn stop(&self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }
}

/// 插件上下文：插件访问框架能力的唯一入口。
#[derive(Clone)]
pub struct PluginContext {
    pub(crate) plugin_name: String,
    pub(crate) services: Arc<ServiceRegistry>,
    pub(crate) events: Arc<EventBus>,
    pub(crate) http: Arc<HttpService>,
    pub(crate) adapters: Arc<AdapterManager>,
    pub(crate) logger: Arc<Logger>,
    pub(crate) runtime: Handle,
    pub(crate) app_dir: PathBuf,
    pub(crate) menus: Arc<MenuRegistry>,
}

impl PluginContext {
    pub fn name(&self) -> &str {
        &self.plugin_name
    }

    pub fn services(&self) -> &ServiceRegistry {
        &self.services
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// 事件总线的共享句柄。
    ///
    /// 需要在 `PluginContext` 之外长期持有的后台任务（如 MQTT 收包线程）
    /// 用它在任意线程发布事件。
    pub fn events_handle(&self) -> Arc<EventBus> {
        self.events.clone()
    }

    pub fn http(&self) -> &HttpService {
        &self.http
    }

    pub fn adapters(&self) -> &AdapterManager {
        &self.adapters
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn runtime(&self) -> Handle {
        self.runtime.clone()
    }

    pub fn app_dir(&self) -> &std::path::Path {
        &self.app_dir
    }

    pub fn menus(&self) -> &MenuRegistry {
        &self.menus
    }

    /// 注册框架管理台菜单入口（插件停用时自动回收）。
    pub fn register_menu(&self, item: MenuItem) -> Result<()> {
        self.menus.register_with_owner(&self.plugin_name, item)
    }

    /// 注册本插件提供的服务。
    pub fn register_service<T: Any + Send + Sync>(
        &self,
        name: &str,
        service: Arc<T>,
    ) -> Result<()> {
        self.services
            .register_with_owner(name, service, Some(&self.plugin_name))
    }

    /// 订阅事件；插件停用时订阅会自动回收。
    pub fn subscribe<F>(&self, priority: i32, filter: EventFilter, handler: F) -> SubscriptionId
    where
        F: Fn(&Event) -> EventAction + Send + Sync + 'static,
    {
        self.events
            .subscribe(&self.plugin_name, priority, filter, handler)
    }

    /// 注册插件路由（同端口，插件停用时自动回收）。
    pub fn route<F, Fut>(&self, method: Method, path: &str, access: RouteAccess, handler: F)
    where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        self.http
            .route_owned(&self.plugin_name, method, path, access, handler);
    }

    /// 挂载插件静态资源（同端口，插件停用时自动回收）。
    pub fn mount_static(&self, prefix: &str, access: RouteAccess, assets: StaticAssets) {
        self.http
            .mount_static_owned(&self.plugin_name, prefix, access, assets);
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PluginState {
    Registered,
    Started,
    Stopped,
    Failed(String),
}

impl PluginState {
    fn name(&self) -> &'static str {
        match self {
            PluginState::Registered => "registered",
            PluginState::Started => "started",
            PluginState::Stopped => "stopped",
            PluginState::Failed(_) => "failed",
        }
    }

    fn error(&self) -> Option<String> {
        match self {
            PluginState::Failed(message) => Some(message.clone()),
            _ => None,
        }
    }
}

/// 插件状态快照（管理台展示/JSON 序列化）。
#[derive(Clone, Debug, Serialize)]
pub struct PluginStatus {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub registered: bool,
    pub running: bool,
    pub state: String,
    pub error: Option<String>,
    pub provides: Vec<String>,
    pub requires: Vec<String>,
}

struct PluginEntry {
    plugin: Box<dyn Plugin>,
    meta: PluginMeta,
    enabled: bool,
    registered: bool,
    state: PluginState,
}

/// 插件管理器。
pub struct PluginManager {
    env: Arc<FrameworkEnv>,
    entries: std::sync::RwLock<Vec<PluginEntry>>,
    persist_hook: std::sync::Mutex<Option<PluginPersistHook>>,
}

type PluginPersistHook = Arc<dyn Fn(&str, bool) + Send + Sync>;

impl PluginManager {
    pub(crate) fn new(env: Arc<FrameworkEnv>) -> Self {
        Self {
            env,
            entries: std::sync::RwLock::new(Vec::new()),
            persist_hook: std::sync::Mutex::new(None),
        }
    }

    /// 设置启用状态持久化回调（管理台切换时调用，app 用于写回 config.json）。
    pub fn set_enabled_persist_hook(&self, hook: PluginPersistHook) {
        *self.persist_hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    /// 触发启用状态持久化回调。
    pub fn persist_enabled(&self, name: &str, enabled: bool) {
        let hook = self
            .persist_hook
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(name, enabled);
        }
    }

    pub(crate) fn add(&self, plugin: Box<dyn Plugin>, enabled: bool) -> Result<()> {
        let meta = plugin.meta();
        if meta.name.is_empty() {
            return Err(FrameworkError::Plugin("插件名不能为空".to_string()));
        }
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if entries.iter().any(|entry| entry.meta.name == meta.name) {
            return Err(FrameworkError::Plugin(format!(
                "插件 `{}` 已被注册",
                meta.name
            )));
        }
        entries.push(PluginEntry {
            plugin,
            meta: meta.clone(),
            enabled,
            registered: false,
            state: PluginState::Stopped,
        });
        drop(entries);
        self.env.logger.info(
            "plugin",
            format!(
                "注册插件 `{}` v{}（{}）",
                meta.name,
                meta.version,
                if enabled { "启用" } else { "禁用" }
            ),
        );
        Ok(())
    }

    /// 启动全部已启用插件（框架启动时调用）。
    pub(crate) fn start_all(&self) {
        let names: Vec<String> = {
            let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
            entries
                .iter()
                .filter(|entry| entry.enabled)
                .map(|entry| entry.meta.name.clone())
                .collect()
        };
        for name in names {
            if let Err(error) = self.start(&name) {
                self.env
                    .logger
                    .warn("plugin", format!("插件 `{name}` 启动失败：{error}"));
            }
        }
    }

    /// 停止全部已启动插件（框架停止时调用，逆序）。
    pub(crate) fn stop_all(&self) {
        let names: Vec<String> = {
            let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
            entries
                .iter()
                .rev()
                .filter(|entry| matches!(entry.state, PluginState::Started))
                .map(|entry| entry.meta.name.clone())
                .collect()
        };
        for name in names {
            let _ = self.stop(&name);
        }
    }

    /// 插件状态列表。
    pub fn list(&self) -> Vec<PluginStatus> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries.iter().map(status_of).collect()
    }

    pub fn get(&self, name: &str) -> Option<PluginStatus> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries
            .iter()
            .find(|entry| entry.meta.name == name)
            .map(status_of)
    }

    /// 启用/禁用插件（禁用会停止并回收其注册的资源）。
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            let entry = entries
                .iter_mut()
                .find(|entry| entry.meta.name == name)
                .ok_or_else(|| FrameworkError::Plugin(format!("插件 `{name}` 不存在")))?;
            if entry.enabled == enabled {
                return Ok(());
            }
            entry.enabled = enabled;
        }
        if enabled {
            self.start(name)
        } else {
            self.stop(name)
        }
    }

    /// 启动插件（必要时先注册）。
    pub fn start(&self, name: &str) -> Result<()> {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let index = find_index(&entries, name)?;
        self.start_locked(&mut entries, index)
    }

    /// 停止插件并回收资源。
    pub fn stop(&self, name: &str) -> Result<()> {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let index = find_index(&entries, name)?;
        self.stop_locked(&mut entries, index);
        Ok(())
    }

    fn start_locked(&self, entries: &mut [PluginEntry], index: usize) -> Result<()> {
        let name = entries[index].meta.name.clone();
        if !entries[index].enabled {
            return Err(FrameworkError::Plugin(format!("插件 `{name}` 已禁用")));
        }
        if matches!(entries[index].state, PluginState::Started) {
            return Ok(());
        }

        for required in &entries[index].meta.requires {
            if !self.env.services.contains(required) {
                let message = format!("缺少依赖服务 `{required}`");
                entries[index].state = PluginState::Failed(message.clone());
                return Err(FrameworkError::Plugin(format!("插件 `{name}` {message}")));
            }
        }

        let context = self.env.plugin_context(&name);
        if !entries[index].registered {
            if let Err(error) = entries[index].plugin.register(&context) {
                entries[index].state = PluginState::Failed(error.to_string());
                return Err(error);
            }
            entries[index].registered = true;
        }
        entries[index].state = PluginState::Registered;
        if let Err(error) = entries[index].plugin.start(&context) {
            entries[index].state = PluginState::Failed(error.to_string());
            return Err(error);
        }
        entries[index].state = PluginState::Started;
        self.env
            .logger
            .info("plugin", format!("插件 `{name}` 已启动"));
        Ok(())
    }

    fn stop_locked(&self, entries: &mut [PluginEntry], index: usize) {
        let name = entries[index].meta.name.clone();
        let context = self.env.plugin_context(&name);
        if matches!(entries[index].state, PluginState::Started) {
            if let Err(error) = entries[index].plugin.stop(&context) {
                self.env
                    .logger
                    .warn("plugin", format!("插件 `{name}` 停止失败：{error}"));
            }
        }
        self.cleanup(&name);
        entries[index].registered = false;
        entries[index].state = PluginState::Stopped;
        self.env
            .logger
            .info("plugin", format!("插件 `{name}` 已停止"));
    }

    fn cleanup(&self, name: &str) {
        let services = self.env.services.remove_owner(name);
        let subscriptions = self.env.events.unsubscribe_owner(name);
        let routes = self.env.http.remove_owner(name);
        let menus = self.env.menus.remove_owner(name);
        if !services.is_empty() || subscriptions > 0 || routes > 0 || menus > 0 {
            self.env.logger.debug(
                "plugin",
                format!(
                    "插件 `{name}` 资源回收：服务 {}、订阅 {}、路由 {}、菜单 {}",
                    services.len(),
                    subscriptions,
                    routes,
                    menus
                ),
            );
        }
    }
}

fn find_index(entries: &[PluginEntry], name: &str) -> Result<usize> {
    entries
        .iter()
        .position(|entry| entry.meta.name == name)
        .ok_or_else(|| FrameworkError::Plugin(format!("插件 `{name}` 不存在")))
}

fn status_of(entry: &PluginEntry) -> PluginStatus {
    PluginStatus {
        name: entry.meta.name.clone(),
        version: entry.meta.version.clone(),
        description: entry.meta.description.clone(),
        enabled: entry.enabled,
        registered: entry.registered,
        running: matches!(entry.state, PluginState::Started),
        state: entry.state.name().to_string(),
        error: entry.state.error(),
        provides: entry.meta.provides.clone(),
        requires: entry.meta.requires.clone(),
    }
}
