//! Core abstractions for RedReply-style chat platform adapters.
//!
//! This crate intentionally keeps only the adapter boundary here: the async
//! connector trait, shared error/result aliases, connector handles, and a small
//! registry that can dispatch API calls by `(platform, self_id)`.
//!
//! Concrete adapters can implement [`BotConnectTrait`] in any application and
//! register themselves in [`BotRegistry`].

#[macro_use]
extern crate lazy_static;

use std::{collections::HashMap, error::Error, sync::Arc};

pub use async_trait::async_trait;
use tokio::sync::RwLock;

pub mod cqapi;
pub mod cqevent;
pub mod email;
pub mod host;
pub mod httpevent;
pub mod kook;
pub mod mytool;
pub mod onebot11;
pub mod onebot115;
pub mod qq_guild_all;
pub mod qqguild_private;
pub mod qqguild_public;
pub mod redlang;
pub mod satoriv1;
pub mod telegram;
pub mod yunhuv1;

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
}

impl BotRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert<T>(&self, url: String, bot: T)
    where
        T: BotConnectTrait + 'static,
    {
        self.bots
            .write()
            .await
            .insert(url, Arc::new(RwLock::new(bot)));
    }

    pub async fn contains_url(&self, url: &str) -> bool {
        self.bots.read().await.contains_key(url)
    }

    pub async fn remove(&self, url: &str) -> Option<BotHandle> {
        self.bots.write().await.remove(url)
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
            bot.write().await.disconnect().await;
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
