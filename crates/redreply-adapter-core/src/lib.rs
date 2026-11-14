//! Core abstractions for RedReply-style chat platform adapters.
//!
//! This crate intentionally keeps only the adapter boundary here: the async
//! connector trait, shared error/result aliases, connector handles, and a small
//! registry that can dispatch API calls by `(platform, self_id)`.
//!
//! Concrete adapters can implement [`BotConnectTrait`] in any application and
//! register themselves in [`BotRegistry`].

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::Arc,
};

pub use async_trait::async_trait;
use tokio::sync::RwLock;

pub mod cqapi;
pub mod cqevent;
pub mod host;
pub mod httpevent;
pub mod mytool;
pub mod native_api;
pub mod redlang;

pub type AdapterError = Box<dyn Error + Send + Sync>;
pub type AdapterResult<T> = Result<T, AdapterError>;
pub type BotHandle = Arc<RwLock<dyn BotConnectTrait>>;

#[async_trait]
pub trait BotConnectTrait: Send + Sync {
    async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        json: &mut serde_json::Value,
    ) -> AdapterResult<serde_json::Value>;

    fn get_platform_and_self_id(&self) -> Vec<(String, String)>;
    fn get_alive(&self) -> bool;
    async fn connect(&mut self) -> AdapterResult<()>;
    async fn disconnect(&mut self);
}

#[derive(Default)]
pub struct BotRegistry {
    bots: RwLock<HashMap<String, BotHandle>>,
    /// 正在连接中的 URL，与 bots 配合实现 single-flight，避免重复并发连接
    connecting: RwLock<HashSet<String>>,
}

impl BotRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert<T>(&self, url: String, bot: T)
    where
        T: BotConnectTrait + 'static,
    {
        // 锁顺序固定：bots 再 connecting，避免与 try_begin_connect 死锁
        let mut bots = self.bots.write().await;
        let mut connecting = self.connecting.write().await;
        bots.insert(url.clone(), Arc::new(RwLock::new(bot)));
        connecting.remove(&url);
    }

    pub async fn contains_url(&self, url: &str) -> bool {
        self.bots.read().await.contains_key(url)
    }

    /// 原子标记 URL 开始连接；若已在线或已在连接中则返回 false
    pub async fn try_begin_connect(&self, url: &str) -> bool {
        let bots = self.bots.write().await;
        let mut connecting = self.connecting.write().await;
        if bots.contains_key(url) || connecting.contains(url) {
            return false;
        }
        connecting.insert(url.to_string());
        true
    }

    /// 连接失败或取消时清除 connecting 标记
    pub async fn end_connect(&self, url: &str) {
        self.connecting.write().await.remove(url);
    }

    pub async fn remove(&self, url: &str) -> Option<BotHandle> {
        let mut bots = self.bots.write().await;
        let mut connecting = self.connecting.write().await;
        connecting.remove(url);
        bots.remove(url)
    }

    pub async fn removable_urls(&self, configured_urls: &[String]) -> Vec<String> {
        let bots = self.bots.read().await;
        let mut urls = Vec::new();
        for (url, bot) in &*bots {
            if !configured_urls.contains(url) || !bot.read().await.get_alive() {
                urls.push(url.clone());
            }
        }
        urls
    }

    pub async fn disconnect_and_remove(&self, url: &str) -> bool {
        if let Some(bot) = self.remove(url).await {
            // 连接对象可能正被慢 API 调用占用（call_api 持读锁跨网络 await，最长 30s）。
            // 写锁限时等待并放到后台任务：超时则本轮放弃断开（Arc 仍持有对象，
            // 调用结束后自然释放），避免单条慢调用在连接管理循环里阻塞
            // 所有其它 bot 的重连簿记（队头阻塞）。
            let bot = bot.clone();
            tokio::task::spawn(async move {
                match tokio::time::timeout(std::time::Duration::from_secs(3), bot.write()).await {
                    Ok(mut guard) => {
                        guard.disconnect().await;
                    }
                    Err(_) => {
                        // 调用仍在进行中；对象由 Arc 持有，届时随引用释放
                    }
                }
            });
            true
        } else {
            false
        }
    }

    pub async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        json: &mut serde_json::Value,
    ) -> AdapterResult<Option<serde_json::Value>> {
        let mut platform_t = platform.to_owned();
        let mut self_id_t = self_id.to_owned();

        {
            let bots = self.bots.read().await;
            if platform_t.is_empty() && self_id_t.is_empty() && bots.len() == 1 {
                for bot in bots.values() {
                    let platform_and_self_id = bot.read().await.get_platform_and_self_id();
                    if platform_and_self_id.len() == 1 {
                        platform_t = platform_and_self_id[0].0.clone();
                        self_id_t = platform_and_self_id[0].1.clone();
                    }
                }
            }
        }

        let mut bot_select = None;
        {
            let bots = self.bots.read().await;
            for bot in bots.values() {
                let platform_and_self_id = bot.read().await.get_platform_and_self_id();
                for (bot_platform, bot_self_id) in platform_and_self_id {
                    if bot_platform == platform_t && bot_self_id == self_id_t {
                        bot_select = Some(bot.clone());
                        break;
                    }
                }
                if bot_select.is_some() {
                    break;
                }
            }
        }

        if let Some(bot) = bot_select {
            return bot
                .read()
                .await
                .call_api(&platform_t, &self_id_t, passive_id, json)
                .await
                .map(Some);
        }

        Ok(None)
    }
}
