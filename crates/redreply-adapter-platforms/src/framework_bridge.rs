//! 把 `redreply-adapter` 的协议实现桥接进 `redreply-bot` 框架。
//!
//! 迁移期间适配器仍通过 [`crate::host`] 全局单例上报事件与日志；
//! 本模块在构建适配器时把 host 安装为基于框架 [`AdapterDeps`] 的实现，
//! 让适配器事件进入框架 EventBus、日志进入框架日志。
//! 后续阶段会把适配器改为显式 `AdapterContext` 注入，届时本桥接层收窄。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use redreply_bot::{
    AdapterDeps, AdapterFactory, Bot, BotHandle, FrameworkError, Result as FrameworkResult,
};

use redreply_adapter_core::host::{self, AdapterHost};
use redreply_adapter_core::{AdapterError, AdapterResult, BotConnectTrait};

use crate::onebot11::OneBot11Connect;
use crate::onebot115::OneBot115Connect;
use crate::{
    email::EmailConnect, kook::KookConnect, qqguild_private::QQGuildPrivateConnect,
    qqguild_public::QQGuildPublicConnect, satoriv1::Satoriv1Connect, telegram::TeleTramConnect,
    yunhuv1::Yunhuv1Connect,
};

/// 基于框架 AdapterDeps 的宿主实现。
struct FrameworkHost {
    deps: AdapterDeps,
}

impl AdapterHost for FrameworkHost {
    fn log(&self, msg: &str) {
        self.deps.logger.info("adapter", msg);
    }

    fn warn(&self, msg: &str) {
        self.deps.logger.warn("adapter", msg);
    }

    fn dispatch_event(&self, event_json: &str) -> AdapterResult<()> {
        let event: Value =
            serde_json::from_str(event_json).map_err(|error| -> AdapterError { error.into() })?;
        self.deps
            .dispatch(event)
            .map_err(|error| -> AdapterError { error.into() })?;
        Ok(())
    }

    fn app_dir(&self) -> AdapterResult<String> {
        Ok(format!(
            "{}{}",
            self.deps.app_dir.to_string_lossy(),
            std::path::MAIN_SEPARATOR
        ))
    }

    fn all_to_silk(&self, input: &[u8]) -> AdapterResult<Vec<u8>> {
        match &self.deps.all_to_silk {
            Some(hook) => hook(input).map_err(|error| -> AdapterError { error.into() }),
            None => Err("宿主未提供 all_to_silk 能力".into()),
        }
    }
}

fn ensure_framework_host(deps: &AdapterDeps) {
    let deps = deps.clone();
    host::ensure_host(move || Arc::new(FrameworkHost { deps }));
}

/// 旧适配器句柄到框架 [`Bot`] 的桥。
struct LegacyBot<C: BotConnectTrait> {
    inner: C,
}

#[async_trait]
impl<C: BotConnectTrait + 'static> Bot for LegacyBot<C> {
    fn platform(&self) -> String {
        self.inner
            .get_platform_and_self_id()
            .into_iter()
            .next()
            .map(|(platform, _)| platform)
            .unwrap_or_default()
    }

    fn self_id(&self) -> String {
        self.inner
            .get_platform_and_self_id()
            .into_iter()
            .next()
            .map(|(_, self_id)| self_id)
            .unwrap_or_default()
    }

    fn identities(&self) -> Vec<(String, String)> {
        self.inner.get_platform_and_self_id()
    }

    fn is_alive(&self) -> bool {
        self.inner.get_alive()
    }

    async fn connect(&mut self) -> FrameworkResult<()> {
        self.inner
            .connect()
            .await
            .map_err(|error| FrameworkError::Plugin(error.to_string()))
    }

    async fn disconnect(&mut self) -> FrameworkResult<()> {
        self.inner.disconnect().await;
        Ok(())
    }

    async fn call_api(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        request: Value,
    ) -> FrameworkResult<Value> {
        let mut request = request;
        self.inner
            .call_api(platform, self_id, passive_id, &mut request)
            .await
            .map_err(|error| FrameworkError::Plugin(error.to_string()))
    }
}

/// 为一个旧协议连接类型生成框架工厂。
macro_rules! legacy_factory {
    ($connect:ty, $factory:ident, $func:ident, $name:literal, [$($scheme:literal),*]) => {
        pub struct $factory;

        impl AdapterFactory for $factory {
            fn name(&self) -> &str {
                $name
            }

            fn schemes(&self) -> Vec<&'static str> {
                vec![$($scheme),*]
            }

            fn build(&self, url: &str, deps: AdapterDeps) -> FrameworkResult<BotHandle> {
                ensure_framework_host(&deps);
                let inner = <$connect>::build(url);
                Ok(Arc::new(tokio::sync::RwLock::new(LegacyBot { inner })))
            }
        }

        /// 返回该协议的框架工厂（app 组装用）。
        pub fn $func() -> Arc<dyn AdapterFactory> {
            Arc::new($factory)
        }
    };
}

legacy_factory!(
    OneBot11Connect,
    OneBot11Factory,
    onebot11_factory,
    "onebot11",
    ["ws", "wss"]
);
legacy_factory!(
    OneBot115Connect,
    OneBot115Factory,
    onebot115_factory,
    "onebot115",
    ["ovo"]
);
legacy_factory!(
    Satoriv1Connect,
    Satoriv1Factory,
    satoriv1_factory,
    "satori",
    ["satori"]
);
legacy_factory!(
    QQGuildPrivateConnect,
    QQGuildPrivateFactory,
    qqguild_private_factory,
    "qqguild_private",
    ["qqguild_private"]
);
legacy_factory!(
    QQGuildPublicConnect,
    QQGuildPublicFactory,
    qqguild_public_factory,
    "qqguild_public",
    ["qqguild_public"]
);
legacy_factory!(KookConnect, KookFactory, kook_factory, "kook", ["kook"]);
legacy_factory!(
    EmailConnect,
    EmailFactory,
    email_factory,
    "email",
    ["email"]
);
legacy_factory!(
    TeleTramConnect,
    TelegramFactory,
    telegram_factory,
    "telegram",
    ["telegram"]
);
legacy_factory!(
    Yunhuv1Connect,
    YunhuFactory,
    yunhu_factory,
    "yunhu",
    ["yunhu"]
);
