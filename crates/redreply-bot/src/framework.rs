use std::collections::HashSet;
use std::net::{Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::runtime::{Handle, Runtime};

use crate::adapter::{AdapterDeps, AdapterFactory, AdapterManager, AllToSilkHook};
use crate::console;
use crate::error::{FrameworkError, Result};
use crate::event::EventBus;
use crate::http::{AuthConfig, HttpService, ServerHandle};
use crate::logger::Logger;
use crate::menu::MenuRegistry;
use crate::plugin::{Plugin, PluginContext, PluginManager};
use crate::service::ServiceRegistry;

/// 框架持有的运行时：外部注入或自建。
pub(crate) enum RuntimeProvider {
    Handle(Handle),
    Owned {
        runtime: Option<Runtime>,
        handle: Handle,
    },
}

impl RuntimeProvider {
    pub(crate) fn handle(&self) -> Handle {
        match self {
            RuntimeProvider::Handle(handle) => handle.clone(),
            RuntimeProvider::Owned { handle, .. } => handle.clone(),
        }
    }
}

impl Drop for RuntimeProvider {
    fn drop(&mut self) {
        // 框架可能在异步上下文中被 drop（例如测试或嵌入式使用），
        // 因此自建运行时用 shutdown_background 避免 blocking drop panic。
        if let RuntimeProvider::Owned { runtime, .. } = self {
            if let Some(runtime) = runtime.take() {
                runtime.shutdown_background();
            }
        }
    }
}

/// 框架共享环境（供插件上下文与适配器依赖使用）。
pub(crate) struct FrameworkEnv {
    pub(crate) services: Arc<ServiceRegistry>,
    pub(crate) events: Arc<EventBus>,
    pub(crate) http: Arc<HttpService>,
    pub(crate) adapters: Arc<AdapterManager>,
    pub(crate) logger: Arc<Logger>,
    pub(crate) runtime: Handle,
    pub(crate) app_dir: PathBuf,
    pub(crate) all_to_silk: Option<AllToSilkHook>,
    pub(crate) menus: Arc<MenuRegistry>,
}

impl FrameworkEnv {
    pub(crate) fn plugin_context(&self, name: &str) -> PluginContext {
        PluginContext {
            plugin_name: name.to_string(),
            services: self.services.clone(),
            events: self.events.clone(),
            http: self.http.clone(),
            adapters: self.adapters.clone(),
            logger: self.logger.clone(),
            runtime: self.runtime.clone(),
            app_dir: self.app_dir.clone(),
            menus: self.menus.clone(),
        }
    }

    pub(crate) fn adapter_deps(&self) -> AdapterDeps {
        AdapterDeps {
            app_dir: self.app_dir.clone(),
            logger: self.logger.clone(),
            events: self.events.clone(),
            all_to_silk: self.all_to_silk.clone(),
        }
    }
}

/// 框架运行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameworkState {
    Created,
    Running,
    Stopped,
}

pub(crate) struct FrameworkInner {
    pub(crate) env: Arc<FrameworkEnv>,
    /// 应用版本号（整个项目一个版本，由主程序注入）。
    pub(crate) version: String,
    pub(crate) plugins: Arc<PluginManager>,
    pub(crate) adapters: Arc<AdapterManager>,
    pub(crate) http: Arc<HttpService>,
    pub(crate) services: Arc<ServiceRegistry>,
    pub(crate) events: Arc<EventBus>,
    pub(crate) logger: Arc<Logger>,
    pub(crate) menus: Arc<MenuRegistry>,
    pub(crate) runtime: RuntimeProvider,
    pub(crate) app_dir: PathBuf,
    pub(crate) http_addr: Option<SocketAddr>,
    state: Mutex<FrameworkState>,
    servers: Mutex<Vec<ServerHandle>>,
}

impl FrameworkInner {
    pub(crate) fn state(&self) -> FrameworkState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 管理台展示的地址：运行中优先返回实际绑定地址。
    pub(crate) fn server_addr(&self) -> Option<SocketAddr> {
        let servers = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        servers
            .first()
            .map(|handle| handle.local_addr)
            .or(self.http_addr)
    }
}

/// 框架实例。
pub struct Framework {
    pub(crate) inner: Arc<FrameworkInner>,
}

impl Framework {
    pub fn builder() -> FrameworkBuilder {
        FrameworkBuilder::default()
    }

    pub fn plugins(&self) -> &Arc<PluginManager> {
        &self.inner.plugins
    }

    pub fn services(&self) -> &Arc<ServiceRegistry> {
        &self.inner.services
    }

    pub fn events(&self) -> &Arc<EventBus> {
        &self.inner.events
    }

    pub fn http(&self) -> &Arc<HttpService> {
        &self.inner.http
    }

    pub fn adapters(&self) -> &Arc<AdapterManager> {
        &self.inner.adapters
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.inner.logger
    }

    pub fn menus(&self) -> &Arc<MenuRegistry> {
        &self.inner.menus
    }

    pub fn app_dir(&self) -> &std::path::Path {
        &self.inner.app_dir
    }

    pub fn state(&self) -> FrameworkState {
        self.inner.state()
    }

    /// 实际绑定或配置的 HTTP 地址。
    pub fn http_addr(&self) -> Option<SocketAddr> {
        self.inner.server_addr()
    }

    /// 为插件构造上下文（主程序注册插件外部扩展时使用）。
    pub fn plugin_context(&self, name: &str) -> PluginContext {
        self.inner.env.plugin_context(name)
    }

    /// 为适配器构造依赖。
    pub fn adapter_deps(&self) -> AdapterDeps {
        self.inner.env.adapter_deps()
    }

    /// 启动插件与 HTTP 服务；重复调用安全。
    pub fn start(&self) -> Result<()> {
        {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            match *state {
                FrameworkState::Running => return Ok(()),
                _ => *state = FrameworkState::Running,
            }
        }
        // 先启动插件再连接适配器：插件在 start() 中加载脚本/注册服务，
        // 若适配器先连接，平台事件可能在插件就绪前到达并丢失（本地端尤甚）。
        self.inner.plugins.start_all();
        self.inner.adapters.start();

        if let Some(addr) = self.inner.http_addr {
            let handle = self.inner.runtime.handle();
            let mut addrs = vec![addr];
            // 端口确定时同时监听 IPv6 回环：浏览器访问 localhost 会优先走 ::1，
            // 只监听 IPv4 时会先等待 IPv6 失败再回退（Windows 上可长达 2 秒）。
            if addr.port() != 0 && !matches!(addr, SocketAddr::V6(v6) if v6.ip().is_loopback()) {
                addrs.push(SocketAddr::from((Ipv6Addr::LOCALHOST, addr.port())));
            }
            let servers = self.inner.http.serve_addrs(&handle, &addrs)?;
            for server in &servers {
                self.inner.logger.info(
                    "framework",
                    format!("管理台地址：http://{}", server.local_addr),
                );
            }
            *self.inner.servers.lock().unwrap_or_else(|e| e.into_inner()) = servers;
        }

        self.inner.logger.info(
            "framework",
            format!("红色问答 v{} 已启动", self.inner.version),
        );
        Ok(())
    }

    /// 停止适配器、插件与 HTTP 服务。
    pub fn shutdown(&self) -> Result<()> {
        self.inner.adapters.stop();
        self.inner.plugins.stop_all();
        let servers =
            std::mem::take(&mut *self.inner.servers.lock().unwrap_or_else(|e| e.into_inner()));
        for server in servers {
            server.shutdown();
        }
        *self.inner.state.lock().unwrap_or_else(|e| e.into_inner()) = FrameworkState::Stopped;
        self.inner.logger.info("framework", "框架已停止");
        Ok(())
    }
}

impl Drop for Framework {
    fn drop(&mut self) {
        if self.inner.state() == FrameworkState::Running {
            let _ = self.shutdown();
        }
    }
}

/// 框架构建器。
#[derive(Default)]
pub struct FrameworkBuilder {
    plugins: Vec<Box<dyn Plugin>>,
    disabled_plugins: Vec<String>,
    adapters: Vec<Arc<dyn AdapterFactory>>,
    auth: AuthConfig,
    app_dir: Option<PathBuf>,
    http_addr: Option<SocketAddr>,
    runtime: Option<Handle>,
    all_to_silk: Option<AllToSilkHook>,
    version: Option<String>,
}

impl FrameworkBuilder {
    /// 注册普通插件。
    pub fn plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// 禁用指定插件（仍会注册元信息，但不启动）。
    pub fn disable_plugin(mut self, name: impl Into<String>) -> Self {
        self.disabled_plugins.push(name.into());
        self
    }

    /// 注册适配器工厂（框架特殊层）。
    pub fn adapter(mut self, factory: Arc<dyn AdapterFactory>) -> Self {
        self.adapters.push(factory);
        self
    }

    /// 设置 Web 鉴权密码。
    pub fn auth(mut self, config: AuthConfig) -> Self {
        self.auth = config;
        self
    }

    /// 设置应用目录（插件数据/临时文件根目录）。
    pub fn app_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.app_dir = Some(dir.into());
        self
    }

    /// 设置 HTTP 监听地址（host 为空时使用 127.0.0.1）。
    pub fn http_host_port(mut self, host: &str, port: u16) -> Result<Self> {
        let host = if host.is_empty() { "127.0.0.1" } else { host };
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|error| FrameworkError::Config(format!("无法解析监听地址：{error}")))?
            .next()
            .ok_or_else(|| FrameworkError::Config("监听地址为空".to_string()))?;
        self.http_addr = Some(addr);
        Ok(self)
    }

    /// 设置 HTTP 监听地址。
    pub fn http_addr(mut self, addr: SocketAddr) -> Self {
        self.http_addr = Some(addr);
        self
    }

    /// 注入外部 tokio 运行时（不设置则框架自建）。
    pub fn runtime(mut self, handle: Handle) -> Self {
        self.runtime = Some(handle);
        self
    }

    /// 注入语音转 silk 能力（适配器需要，框架不关心实现）。
    pub fn all_to_silk(mut self, hook: AllToSilkHook) -> Self {
        self.all_to_silk = Some(hook);
        self
    }

    /// 设置应用版本号（整个项目一个版本；管理台与日志展示用）。
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// 组装框架（不启动）。
    pub fn build(self) -> Result<Framework> {
        let logger = Arc::new(Logger::new());
        let services = Arc::new(ServiceRegistry::new());
        let events = Arc::new(EventBus::new());
        let http = Arc::new(HttpService::new(logger.clone()));
        http.set_auth(self.auth.clone());

        let runtime_provider = match self.runtime {
            Some(handle) => RuntimeProvider::Handle(handle),
            None => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("redreply-framework")
                    .build()
                    .map_err(|error| FrameworkError::Runtime(error.to_string()))?;
                let handle = runtime.handle().clone();
                RuntimeProvider::Owned {
                    runtime: Some(runtime),
                    handle,
                }
            }
        };

        let app_dir = match self.app_dir {
            Some(dir) => dir,
            None => default_app_dir()?,
        };

        let all_to_silk = self.all_to_silk;
        let adapters = Arc::new(AdapterManager::new(
            AdapterDeps {
                app_dir: app_dir.clone(),
                logger: logger.clone(),
                events: events.clone(),
                all_to_silk: all_to_silk.clone(),
            },
            runtime_provider.handle(),
        ));

        let menus = Arc::new(MenuRegistry::new());
        let env = Arc::new(FrameworkEnv {
            services: services.clone(),
            events: events.clone(),
            http: http.clone(),
            adapters: adapters.clone(),
            logger: logger.clone(),
            runtime: runtime_provider.handle(),
            app_dir: app_dir.clone(),
            all_to_silk,
            menus: menus.clone(),
        });

        let plugins = Arc::new(PluginManager::new(env.clone()));
        let disabled: HashSet<String> = self.disabled_plugins.into_iter().collect();
        for plugin in self.plugins {
            let enabled = !disabled.contains(&plugin.meta().name);
            plugins.add(plugin, enabled)?;
        }
        for factory in self.adapters {
            adapters.register(factory)?;
        }

        let version = self.version.unwrap_or_else(|| crate::VERSION.to_string());
        let inner = Arc::new(FrameworkInner {
            env,
            version: version.clone(),
            plugins: plugins.clone(),
            adapters: adapters.clone(),
            http: http.clone(),
            services,
            events,
            logger: logger.clone(),
            menus,
            runtime: runtime_provider,
            app_dir,
            http_addr: self.http_addr,
            state: Mutex::new(FrameworkState::Created),
            servers: Mutex::new(Vec::new()),
        });

        console::register(&inner);

        logger.info("framework", format!("红色问答 v{version} 初始化完成"));
        Ok(Framework { inner })
    }
}

fn default_app_dir() -> Result<PathBuf> {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(dir) = executable.parent() {
            return Ok(dir.to_path_buf());
        }
    }
    std::env::current_dir().map_err(FrameworkError::from)
}
