use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::{watch, Notify};

use crate::error::{FrameworkError, Result};
use crate::event::EventBus;
use crate::logger::Logger;

/// 已连接的平台机器人句柄。
pub type BotHandle = Arc<tokio::sync::RwLock<dyn Bot>>;

/// 平台机器人统一接口（适配器特殊层的核心抽象）。
///
/// `request` 为 OneBot 风格的完整请求 JSON（`{"action":..,"params":..}`），
/// 适配器内部负责补 `echo`、ID 转换等协议细节；`passive_id` 为被动回复 ID。
#[async_trait]
pub trait Bot: Send + Sync {
    /// 展示用的平台标识（默认取第一个路由身份）。
    fn platform(&self) -> String;

    /// 展示用的机器人 ID（默认取第一个路由身份）。
    fn self_id(&self) -> String;

    /// 该连接可接受的 `(platform, self_id)` 路由身份。
    ///
    /// 一个协议连接可能有多个身份，例如 QQ 频道连接同时能发频道消息
    /// （`qqguild_public`）和群消息（`qqgroup_public`）。默认单身份。
    fn identities(&self) -> Vec<(String, String)> {
        vec![(self.platform(), self.self_id())]
    }

    /// 连接是否存活。
    fn is_alive(&self) -> bool;

    /// 建立连接（含内部收发循环的启动）。
    async fn connect(&mut self) -> Result<()>;

    /// 断开连接。
    async fn disconnect(&mut self) -> Result<()>;

    /// 调用平台 API；`platform`/`self_id` 为匹配到的路由身份。
    async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        request: Value,
    ) -> Result<Value>;
}

/// 语音转 silk 的回调类型（由 app 注入，框架不关心实现）。
pub type AllToSilkHook = Arc<dyn Fn(&[u8]) -> std::result::Result<Vec<u8>, String> + Send + Sync>;

/// 适配器构建时由框架注入的依赖。
#[derive(Clone)]
pub struct AdapterDeps {
    pub app_dir: PathBuf,
    pub logger: Arc<Logger>,
    pub events: Arc<EventBus>,
    /// 语音转 silk 能力；未注入时适配器相关命令报错。
    pub all_to_silk: Option<AllToSilkHook>,
}

impl AdapterDeps {
    /// 适配器收到平台事件后统一调用此方法发布到事件总线。
    pub fn dispatch(
        &self,
        event: Value,
    ) -> std::result::Result<crate::event::PublishReport, String> {
        let event = crate::event::Event::from_json(event)?;
        Ok(self.events.publish(&event))
    }
}

/// 适配器工厂：按 URL scheme 创建 [`Bot`]。
///
/// 适配器是框架特殊层扩展，不实现 Plugin trait；
/// 由 app 通过 `Framework::builder().adapter(...)` 编译期注册。
pub trait AdapterFactory: Send + Sync + 'static {
    /// 工厂名，例如 `onebot11`。
    fn name(&self) -> &str;

    /// 支持的 URL scheme，例如 `["ws", "wss"]`。
    fn schemes(&self) -> Vec<&'static str>;

    /// 根据配置 URL 构建机器人实例。
    fn build(&self, url: &str, deps: AdapterDeps) -> Result<BotHandle>;
}

/// 适配器工厂信息（管理台展示用）。
#[derive(Clone, Debug, Serialize)]
pub struct AdapterInfo {
    pub name: String,
    pub schemes: Vec<String>,
}

/// 机器人连接状态（管理台展示用）。
#[derive(Clone, Debug, Serialize)]
pub struct BotInfo {
    pub url: String,
    pub platform: String,
    pub self_id: String,
    pub alive: bool,
}

/// 一条连接配置：URL + 是否启用。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AdapterEntry {
    pub url: String,
    pub enabled: bool,
}

impl AdapterEntry {
    pub fn new(url: impl Into<String>, enabled: bool) -> Self {
        Self {
            url: url.into(),
            enabled,
        }
    }
}

/// 连接重试状态。
#[derive(Default)]
struct RetryState {
    attempts: u32,
    next_attempt: Option<Instant>,
}

/// 目标 URL 与连接状态之间的对账间隔。
const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
/// 单次连接超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 断开时的锁等待上限（慢 API 调用不能阻塞管理循环）。
const DISCONNECT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// 失败退避序列（秒）。
const RETRY_STEPS_SECS: [u64; 5] = [5, 10, 20, 40, 60];

/// 适配器注册表 + 连接管理器。
///
/// 职责：
/// - 注册 [`AdapterFactory`]（编译期）；
/// - 维护「目标 URL 列表」（由 app 从配置同步）与实际连接的对账循环；
/// - 断线/被移除的连接自动清理，失败连接按退避重试；
/// - 按 `(platform, self_id)` 路由 [`AdapterManager::call_api`]。
pub struct AdapterManager {
    deps: AdapterDeps,
    runtime: Handle,
    factories: RwLock<Vec<Arc<dyn AdapterFactory>>>,
    bots: RwLock<HashMap<String, BotHandle>>,
    connecting: RwLock<HashSet<String>>,
    retry: RwLock<HashMap<String, RetryState>>,
    entries: RwLock<Vec<AdapterEntry>>,
    persist_hook: Mutex<Option<Arc<dyn Fn(&[AdapterEntry]) + Send + Sync>>>,
    notify: Notify,
    started: AtomicBool,
    shutdown: Mutex<Option<watch::Sender<bool>>>,
}

impl AdapterManager {
    pub(crate) fn new(deps: AdapterDeps, runtime: Handle) -> Self {
        Self {
            deps,
            runtime,
            factories: RwLock::new(Vec::new()),
            bots: RwLock::new(HashMap::new()),
            connecting: RwLock::new(HashSet::new()),
            retry: RwLock::new(HashMap::new()),
            entries: RwLock::new(Vec::new()),
            persist_hook: Mutex::new(None),
            notify: Notify::new(),
            started: AtomicBool::new(false),
            shutdown: Mutex::new(None),
        }
    }

    pub(crate) fn register(&self, factory: Arc<dyn AdapterFactory>) -> Result<()> {
        let name = factory.name().to_string();
        let mut factories = self.factories.write().unwrap_or_else(|e| e.into_inner());
        if factories.iter().any(|existing| existing.name() == name) {
            return Err(FrameworkError::Plugin(format!(
                "适配器 `{name}` 已被注册"
            )));
        }
        factories.push(factory);
        Ok(())
    }

    /// 全部适配器工厂。
    pub fn list(&self) -> Vec<AdapterInfo> {
        let factories = self.factories.read().unwrap_or_else(|e| e.into_inner());
        factories
            .iter()
            .map(|factory| AdapterInfo {
                name: factory.name().to_string(),
                schemes: factory
                    .schemes()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            })
            .collect()
    }

    /// 按 URL 的 scheme 找工厂。
    pub fn factory_for_url(&self, url: &str) -> Option<Arc<dyn AdapterFactory>> {
        let scheme = url_scheme(url)?;
        let factories = self.factories.read().unwrap_or_else(|e| e.into_inner());
        factories
            .iter()
            .find(|factory| factory.schemes().iter().any(|item| *item == scheme))
            .cloned()
    }

    /// 设置全部连接条目（app 从配置同步；立即触发对账）。
    pub fn set_entries(&self, entries: Vec<AdapterEntry>) {
        *self.entries.write().unwrap_or_else(|e| e.into_inner()) = entries;
        self.notify.notify_one();
    }

    /// 设置目标连接 URL 列表（全部启用）。
    pub fn set_urls(&self, urls: Vec<String>) {
        self.set_entries(urls.into_iter().map(|url| AdapterEntry::new(url, true)).collect());
    }

    /// 当前全部连接条目（含停用）。
    pub fn entries(&self) -> Vec<AdapterEntry> {
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 当前启用中的连接 URL。
    pub fn urls(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.url)
            .collect()
    }

    /// 新增一条连接（默认启用；已存在时不重复添加）。
    pub fn add_url(&self, url: &str) {
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            if entries.iter().any(|entry| entry.url == url) {
                return;
            }
            entries.push(AdapterEntry::new(url, true));
        }
        self.notify.notify_one();
    }

    /// 启用/停用一条连接；返回是否找到该条目。
    pub fn set_enabled(&self, url: &str, enabled: bool) -> bool {
        let found = {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            match entries.iter_mut().find(|entry| entry.url == url) {
                Some(entry) => {
                    entry.enabled = enabled;
                    true
                }
                None => false,
            }
        };
        if found {
            self.notify.notify_one();
        }
        found
    }

    /// 修改一条连接的 URL（保留启用状态）；返回是否找到该条目。
    pub fn update_url(&self, old_url: &str, new_url: &str) -> bool {
        let found = {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            match entries.iter_mut().find(|entry| entry.url == old_url) {
                Some(entry) => {
                    entry.url = new_url.to_string();
                    true
                }
                None => false,
            }
        };
        if found {
            self.notify.notify_one();
        }
        found
    }

    /// 删除一条连接（在连接中会同时断开）。
    pub fn remove_url(&self, url: &str) {
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.retain(|entry| entry.url != url);
        }
        self.notify.notify_one();
    }

    /// 设置条目持久化回调（管理台修改连接时调用，app 用于写回 config.json）。
    pub fn set_entries_persist_hook(&self, hook: Arc<dyn Fn(&[AdapterEntry]) + Send + Sync>) {
        *self.persist_hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    /// 触发条目持久化回调。
    pub fn persist_entries(&self, entries: &[AdapterEntry]) {
        let hook = self
            .persist_hook
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(entries);
        }
    }

    /// 当前连接状态快照。
    pub fn bots(&self) -> Vec<BotInfo> {
        let bots = self.bots.read().unwrap_or_else(|e| e.into_inner());
        bots.iter()
            .map(|(url, handle)| match handle.try_read() {
                Ok(bot) => BotInfo {
                    url: url.clone(),
                    platform: bot.platform(),
                    self_id: bot.self_id(),
                    alive: bot.is_alive(),
                },
                Err(_) => BotInfo {
                    url: url.clone(),
                    platform: String::new(),
                    self_id: String::new(),
                    alive: true,
                },
            })
            .collect()
    }

    /// 立即重连某个 URL（停用状态会自动启用；失败会走退避）。
    pub fn reconnect(self: &Arc<Self>, url: &str) {
        self.retry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url);
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            match entries.iter_mut().find(|entry| entry.url == url) {
                Some(entry) => entry.enabled = true,
                None => entries.push(AdapterEntry::new(url, true)),
            }
        }
        // 先断开旧连接：旧轮询实例不断开会导致同一 token 双实例冲突（如 Telegram 409）
        if let Some(previous) = self
            .bots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url)
        {
            self.spawn_disconnect(previous);
        }
        self.spawn_connect(url.to_string());
    }

    /// 断开某个 URL 并从连接条目中移除。
    pub async fn disconnect_url(&self, url: &str) {
        self.remove_url(url);
        self.disconnect_and_remove(url).await;
    }

    /// 启动对账循环；重复调用安全。
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        *self.shutdown.lock().unwrap_or_else(|e| e.into_inner()) = Some(shutdown_tx);
        let this = Arc::clone(self);
        self.runtime.spawn(this.run(shutdown_rx));
    }

    /// 停止对账循环；循环退出时会断开全部连接。
    pub fn stop(&self) {
        if let Some(shutdown) = self
            .shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = shutdown.send(true);
        }
        self.started.store(false, Ordering::SeqCst);
    }

    async fn run(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        loop {
            self.reconcile().await;
            tokio::select! {
                _ = self.notify.notified() => {}
                _ = tokio::time::sleep(RECONCILE_INTERVAL) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
        // 收尾：断开全部连接
        let urls: Vec<String> = {
            let bots = self.bots.read().unwrap_or_else(|e| e.into_inner());
            bots.keys().cloned().collect()
        };
        for url in urls {
            self.disconnect_and_remove(&url).await;
        }
    }

    async fn reconcile(self: &Arc<Self>) {
        let desired = self.urls();
        let stale: Vec<String> = {
            let bots = self.bots.read().unwrap_or_else(|e| e.into_inner());
            bots.iter()
                .filter_map(|(url, handle)| {
                    let alive = handle.try_read().map(|bot| bot.is_alive()).unwrap_or(true);
                    if !desired.contains(url) || !alive {
                        Some(url.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        for url in stale {
            self.disconnect_and_remove(&url).await;
        }

        for url in desired {
            if self.bots.read().unwrap_or_else(|e| e.into_inner()).contains_key(&url)
                || self
                    .connecting
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&url)
                || !self.retry_ready(&url)
            {
                continue;
            }
            self.spawn_connect(url);
        }
    }

    fn spawn_connect(self: &Arc<Self>, url: String) {
        {
            let mut connecting = self.connecting.write().unwrap_or_else(|e| e.into_inner());
            if !connecting.insert(url.clone()) {
                return;
            }
        }
        let this = Arc::clone(self);
        self.runtime.spawn(async move {
            let result = this.connect_url(&url).await;
            this.connecting
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&url);
            match result {
                Ok(handle) => {
                    let previous = this
                        .bots
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(url.clone(), handle);
                    if let Some(previous) = previous {
                        // 替换时保证旧实例被断开，避免双实例轮询
                        this.spawn_disconnect(previous);
                    }
                    this.mark_success(&url);
                    this.deps
                        .logger
                        .info("adapter", format!("适配器已连接：{url}"));
                }
                Err(error) => {
                    this.mark_failure(&url);
                    this.deps
                        .logger
                        .warn("adapter", format!("适配器连接失败：{url} -> {error}"));
                }
            }
        });
    }

    async fn connect_url(&self, url: &str) -> Result<BotHandle> {
        let factory = self.factory_for_url(url).ok_or_else(|| {
            FrameworkError::Plugin(format!("没有匹配 `{url}` 的适配器工厂"))
        })?;
        let bot = factory.build(url, self.deps.clone())?;
        {
            let mut guard = tokio::time::timeout(CONNECT_TIMEOUT, bot.write())
                .await
                .map_err(|_| FrameworkError::Plugin("等待适配器锁超时".to_string()))?;
            tokio::time::timeout(CONNECT_TIMEOUT, guard.connect())
                .await
                .map_err(|_| FrameworkError::Plugin("连接适配器超时".to_string()))??;
        }
        Ok(bot)
    }

    /// 后台断开一个旧连接（限时拿写锁，避免慢调用阻塞管理循环）。
    fn spawn_disconnect(&self, handle: BotHandle) {
        self.runtime.spawn(async move {
            if let Ok(mut guard) =
                tokio::time::timeout(DISCONNECT_LOCK_TIMEOUT, handle.write()).await
            {
                let _ = guard.disconnect().await;
            }
        });
    }

    async fn disconnect_and_remove(&self, url: &str) {
        let handle = self
            .bots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url);
        if let Some(handle) = handle {
            if let Ok(mut guard) =
                tokio::time::timeout(DISCONNECT_LOCK_TIMEOUT, handle.write()).await
            {
                let _ = guard.disconnect().await;
            }
            self.deps
                .logger
                .info("adapter", format!("适配器已断开：{url}"));
        }
        self.retry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url);
    }

    /// 调用平台 API：按 `(platform, self_id)` 匹配连接身份；二者为空且只有一个连接时自动选中。
    ///
    /// 匹配到具体身份后，把该身份回传给适配器，保证多身份连接（如 QQ 群/频道）
    /// 能按正确身份构造请求。返回 `Ok(None)` 表示没有可用连接。
    pub async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        request: &mut Value,
    ) -> Result<Option<Value>> {
        let bots: Vec<(String, BotHandle)> = {
            let bots = self.bots.read().unwrap_or_else(|e| e.into_inner());
            bots.iter()
                .map(|(url, handle)| (url.clone(), Arc::clone(handle)))
                .collect()
        };
        if bots.is_empty() {
            return Ok(None);
        }
        // 未指定目标时只有唯一连接可自动选中（与旧版语义一致）
        let auto_select = platform.is_empty() && self_id.is_empty() && bots.len() == 1;

        let mut last_error: Option<String> = None;
        for (url, handle) in bots {
            let matched = {
                let guard = match handle.try_read() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };
                if platform.is_empty() && self_id.is_empty() {
                    if auto_select {
                        guard.identities().into_iter().next()
                    } else {
                        None
                    }
                } else {
                    guard.identities().into_iter().find(|(bot_platform, bot_self_id)| {
                        (platform.is_empty() || bot_platform == platform)
                            && (self_id.is_empty() || bot_self_id == self_id)
                    })
                }
            };
            let Some((matched_platform, matched_self_id)) = matched else {
                continue;
            };
            let guard = handle.read().await;
            match guard
                .call_api(&matched_platform, &matched_self_id, passive_id, request.clone())
                .await
            {
                Ok(value) => return Ok(Some(value)),
                Err(error) => {
                    let message = format!(
                        "适配器 `{url}`（{matched_platform}/{matched_self_id}）调用失败：{error}"
                    );
                    self.deps.logger.warn("adapter", message.clone());
                    last_error = Some(message);
                }
            }
        }
        // 匹配到连接但调用失败：把真实错误抛给调用方，避免被当成“没有连接”吞掉
        if let Some(error) = last_error {
            return Err(FrameworkError::Plugin(error));
        }
        Ok(None)
    }

    fn retry_ready(&self, url: &str) -> bool {
        let retry = self.retry.read().unwrap_or_else(|e| e.into_inner());
        retry
            .get(url)
            .map(|state| {
                state
                    .next_attempt
                    .map(|next| Instant::now() >= next)
                    .unwrap_or(true)
            })
            .unwrap_or(true)
    }

    fn mark_failure(&self, url: &str) {
        let mut retry = self.retry.write().unwrap_or_else(|e| e.into_inner());
        let state = retry.entry(url.to_string()).or_default();
        state.attempts = state.attempts.saturating_add(1);
        let step = RETRY_STEPS_SECS[std::cmp::min(
            (state.attempts.saturating_sub(1)) as usize,
            RETRY_STEPS_SECS.len() - 1,
        )];
        state.next_attempt = Some(Instant::now() + Duration::from_secs(step));
    }

    fn mark_success(&self, url: &str) {
        self.retry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(url);
    }
}

fn url_scheme(url: &str) -> Option<&str> {
    url.split_once("://").map(|(scheme, _)| scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoBot {
        platform: String,
        self_id: String,
        alive: bool,
    }

    #[async_trait]
    impl Bot for EchoBot {
        fn platform(&self) -> String {
            self.platform.clone()
        }

        fn self_id(&self) -> String {
            self.self_id.clone()
        }

        fn is_alive(&self) -> bool {
            self.alive
        }

        async fn connect(&mut self) -> Result<()> {
            self.alive = true;
            Ok(())
        }

        async fn disconnect(&mut self) -> Result<()> {
            self.alive = false;
            Ok(())
        }

        async fn call_api(
            &self,
            _platform: &str,
            _self_id: &str,
            _passive_id: &str,
            request: Value,
        ) -> Result<Value> {
            Ok(request)
        }
    }

    struct EchoFactory;

    impl AdapterFactory for EchoFactory {
        fn name(&self) -> &str {
            "echo"
        }

        fn schemes(&self) -> Vec<&'static str> {
            vec!["echo"]
        }

        fn build(&self, _url: &str, _deps: AdapterDeps) -> Result<BotHandle> {
            Ok(Arc::new(tokio::sync::RwLock::new(EchoBot {
                platform: "echo".to_string(),
                self_id: "1".to_string(),
                alive: false,
            })))
        }
    }

    fn manager() -> AdapterManager {
        AdapterManager::new(
            AdapterDeps {
                app_dir: PathBuf::from("."),
                logger: Arc::new(Logger::new()),
                events: Arc::new(EventBus::new()),
                all_to_silk: None,
            },
            Handle::current(),
        )
    }

    #[tokio::test]
    async fn factory_lookup_by_scheme() {
        let manager = manager();
        manager.register(Arc::new(EchoFactory)).unwrap();
        assert!(manager.factory_for_url("echo://demo").is_some());
        assert!(manager.factory_for_url("ws://localhost:3000").is_none());
        assert_eq!(manager.list()[0].name, "echo");
    }

    #[tokio::test]
    async fn duplicate_factory_rejected() {
        let manager = manager();
        manager.register(Arc::new(EchoFactory)).unwrap();
        assert!(manager.register(Arc::new(EchoFactory)).is_err());
    }

    #[tokio::test]
    async fn retry_backoff_grows_and_resets() {
        let manager = manager();
        assert!(manager.retry_ready("echo://a"));
        manager.mark_failure("echo://a");
        assert!(!manager.retry_ready("echo://a"));
        manager.mark_success("echo://a");
        assert!(manager.retry_ready("echo://a"));
    }
}
