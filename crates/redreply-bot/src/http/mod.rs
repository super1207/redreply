//! 框架统一 HTTP 服务。
//!
//! 单端口承载：框架管理台（`/`）、框架管理 API（`/api/*`）、
//! 适配器反向接口以及各插件挂载的路由/静态资源。
//! 鉴权在此层统一处理，插件只需声明路由的 [`RouteAccess`]。

pub mod assets;
pub mod auth;
pub mod response;
mod router;

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;

use crate::error::Result;
use crate::logger::Logger;
use router::{CompiledPattern, RouteEntry, Router, StaticEntry};

pub use assets::{mime_for, StaticAssets, StaticFile};
pub use auth::{AccessLevel, AuthConfig, RouteAccess};
pub use response::{
    bytes_response, empty, error_response, full, json_response, redirect_response,
    redirect_with_cookie, text_response, with_body, BoxError, HttpBody, HttpError, HttpResponse,
    RouteFuture,
};

/// 路由处理时的上下文信息。
#[derive(Clone, Debug)]
pub struct RouteContext {
    /// 路径参数（`{name}` 段）。
    pub params: HashMap<String, String>,
    /// 调用者获得的权限级别。
    pub access: AccessLevel,
    /// 是否来自回环地址。
    pub is_local: bool,
    /// 远端地址。
    pub remote_addr: IpAddr,
}

/// 路由处理函数。
pub type RouteHandler =
    Arc<dyn Fn(Request<Incoming>, RouteContext) -> RouteFuture + Send + Sync + 'static>;

/// 框架 HTTP 服务。
pub struct HttpService {
    router: Router,
    fallback: RwLock<Option<(Option<String>, RouteHandler)>>,
    auth: RwLock<AuthConfig>,
    logger: Arc<Logger>,
}

impl HttpService {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self {
            router: Router::default(),
            fallback: RwLock::new(None),
            auth: RwLock::new(AuthConfig::default()),
            logger,
        }
    }

    pub fn set_auth(&self, config: AuthConfig) {
        *self.auth.write().unwrap_or_else(|e| e.into_inner()) = config;
    }

    pub fn auth_snapshot(&self) -> AuthConfig {
        self.auth
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    /// 注册路由（无 owner，框架/主程序使用；插件请通过 `PluginContext::route`）。
    pub fn route<F, Fut>(&self, method: Method, path: &str, access: RouteAccess, handler: F)
    where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        self.route_owned_with(None, method, path, access, handler);
    }

    /// 注册带 owner 的路由（插件使用，便于停用插件时回收）。
    pub(crate) fn route_owned<F, Fut>(
        &self,
        owner: &str,
        method: Method,
        path: &str,
        access: RouteAccess,
        handler: F,
    ) where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        self.route_owned_with(Some(owner.to_string()), method, path, access, handler);
    }

    fn route_owned_with<F, Fut>(
        &self,
        owner: Option<String>,
        method: Method,
        path: &str,
        access: RouteAccess,
        handler: F,
    ) where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        let handler: RouteHandler =
            Arc::new(move |request, context| Box::pin(handler(request, context)));
        self.router.add_route(RouteEntry {
            method: Some(method),
            pattern: CompiledPattern::parse(path),
            access,
            owner,
            handler,
        });
    }

    /// 挂载静态资源前缀。
    pub fn mount_static(&self, prefix: &str, access: RouteAccess, assets: StaticAssets) {
        self.mount_static_owned_with(None, prefix, access, assets);
    }

    pub(crate) fn mount_static_owned(
        &self,
        owner: &str,
        prefix: &str,
        access: RouteAccess,
        assets: StaticAssets,
    ) {
        self.mount_static_owned_with(Some(owner.to_string()), prefix, access, assets);
    }

    fn mount_static_owned_with(
        &self,
        owner: Option<String>,
        prefix: &str,
        access: RouteAccess,
        assets: StaticAssets,
    ) {
        let prefix = normalize_prefix(prefix);
        self.router.add_static(StaticEntry {
            prefix,
            access,
            owner,
            assets,
        });
    }

    /// 设置兜底处理器（未匹配任何路由/静态资源时调用）。
    ///
    /// 旧主程序迁移期间用于接住尚未迁移的路径。
    pub fn fallback<F, Fut>(&self, handler: F)
    where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        self.fallback_owned_with(None, handler);
    }

    #[allow(dead_code)]
    pub(crate) fn fallback_owned<F, Fut>(&self, owner: &str, handler: F)
    where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        self.fallback_owned_with(Some(owner.to_string()), handler);
    }

    fn fallback_owned_with<F, Fut>(&self, owner: Option<String>, handler: F)
    where
        F: Fn(Request<Incoming>, RouteContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static,
    {
        let handler: RouteHandler =
            Arc::new(move |request, context| Box::pin(handler(request, context)));
        *self.fallback.write().unwrap_or_else(|e| e.into_inner()) = Some((owner, handler));
    }

    /// 回收某个 owner 注册的全部路由/静态资源/兜底。
    pub fn remove_owner(&self, owner: &str) -> usize {
        let removed = self.router.remove_owner(owner);
        let mut fallback = self.fallback.write().unwrap_or_else(|e| e.into_inner());
        if fallback
            .as_ref()
            .map(|(fallback_owner, _)| fallback_owner.as_deref() == Some(owner))
            .unwrap_or(false)
        {
            *fallback = None;
            return removed + 1;
        }
        removed
    }

    pub fn route_count(&self) -> usize {
        self.router.route_count()
    }

    pub fn static_count(&self) -> usize {
        self.router.static_count()
    }

    /// 分发一次请求（鉴权 + 路由）。
    pub async fn dispatch(&self, request: Request<Incoming>, remote: IpAddr) -> HttpResponse {
        let access = auth::resolve_access(request.headers(), remote, &self.auth_snapshot());
        let is_local = remote.is_loopback();
        let path = request.uri().path().to_string();
        let method = request.method().clone();

        if let Some((required, handler, params)) = self.router.match_route(&method, &path) {
            if !access.can(required, is_local) {
                return denied_response(&path);
            }
            let context = RouteContext {
                params,
                access,
                is_local,
                remote_addr: remote,
            };
            return match handler(request, context).await {
                Ok(response) => response,
                Err(error) => error.into_response(),
            };
        }

        if self.router.path_exists(&path) {
            return error_response(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
        }

        let static_candidates = self.router.match_static(&path);
        if let Some((_, required, _, _)) = static_candidates.first() {
            if !access.can(*required, is_local) {
                return denied_response(&path);
            }
            for (_, required, assets, relative) in &static_candidates {
                if !access.can(*required, is_local) {
                    continue;
                }
                if let Some(file) = assets.resolve(relative) {
                    return bytes_response(
                        StatusCode::OK,
                        &file.content_type,
                        file.data,
                        assets.cache_control.as_deref(),
                    );
                }
            }
            return text_response(StatusCode::NOT_FOUND, "not found");
        }

        let fallback = self
            .fallback
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some((_, handler)) = fallback {
            let context = RouteContext {
                params: HashMap::new(),
                access,
                is_local,
                remote_addr: remote,
            };
            return match handler(request, context).await {
                Ok(response) => response,
                Err(error) => error.into_response(),
            };
        }

        text_response(StatusCode::NOT_FOUND, "not found")
    }

    /// 在指定地址启动服务；返回可用于停机与读取端口的句柄。
    pub fn serve(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        addr: SocketAddr,
    ) -> Result<ServerHandle> {
        self.serve_one(handle, addr)
    }

    /// 同时监听多个地址（例如 IPv4 与 IPv6 回环）。
    ///
    /// 单个地址绑定失败只记录日志并跳过；全部失败才返回错误。
    pub fn serve_addrs(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        addrs: &[SocketAddr],
    ) -> Result<Vec<ServerHandle>> {
        let mut servers = Vec::new();
        let mut last_error = None;
        for addr in addrs {
            match self.serve_one(handle, *addr) {
                Ok(server) => servers.push(server),
                Err(error) => {
                    self.logger
                        .warn("http", format!("监听 {addr} 失败：{error}"));
                    last_error = Some(error);
                }
            }
        }
        if servers.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                crate::error::FrameworkError::Http("没有可用的监听地址".to_string())
            }));
        }
        Ok(servers)
    }

    fn serve_one(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        addr: SocketAddr,
    ) -> Result<ServerHandle> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let service = self.clone();
        let logger = service.logger.clone();
        let connection_handle = handle.clone();
        let join = handle.spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    logger.error("http", format!("HTTP 监听失败：{error}"));
                    return;
                }
            };
            logger.info("http", format!("HTTP 服务已启动：http://{local_addr}"));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, remote)) => {
                                // 关闭 Nagle，避免小响应被延迟确认拖慢
                                let _ = stream.set_nodelay(true);
                                let connection_service = service.clone();
                                let connection_logger = connection_service.logger.clone();
                                let io = TokioIo::new(stream);
                                connection_handle.spawn(async move {
                                    let connection = http1::Builder::new()
                                        .serve_connection(
                                            io,
                                            service_fn(move |request: Request<Incoming>| {
                                                let service = connection_service.clone();
                                                async move {
                                                    let response =
                                                        service.dispatch(request, remote.ip()).await;
                                                    Ok::<_, std::convert::Infallible>(response)
                                                }
                                            }),
                                        )
                                        .with_upgrades();
                                    if let Err(error) = connection.await {
                                        connection_logger
                                            .debug("http", format!("连接结束：{error}"));
                                    }
                                });
                            }
                            Err(error) => {
                                service.logger.warn("http", format!("accept 失败：{error}"));
                            }
                        }
                    }
                }
            }
            logger.info("http", "HTTP 服务已停止");
        });

        Ok(ServerHandle {
            local_addr,
            shutdown: Some(shutdown_tx),
            join: Some(join),
        })
    }
}

/// HTTP 服务句柄。
pub struct ServerHandle {
    /// 实际绑定地址（端口为 0 时可用于测试）。
    pub local_addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl ServerHandle {
    /// 停止服务。
    pub fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn denied_response(path: &str) -> HttpResponse {
    if path.starts_with("/api") {
        error_response(StatusCode::UNAUTHORIZED, "未登录或无权限")
    } else {
        redirect_response("/login.html")
    }
}

/// 读取请求体并限制大小。
pub async fn collect_body(body: Incoming, limit: usize) -> std::result::Result<Bytes, HttpError> {
    let collected = body
        .collect()
        .await
        .map_err(|error| HttpError::bad_request(format!("读取请求体失败：{error}")))?;
    let bytes = collected.to_bytes();
    if bytes.len() > limit {
        return Err(HttpError::payload_too_large(format!(
            "请求体超过 {limit} 字节"
        )));
    }
    Ok(bytes)
}
