use std::sync::{Arc, OnceLock};

use crate::service_hub::ServiceHub;
use crate::session_store::SessionStore;

/// 进程级运行时上下文。
///
/// RuntimeContext owns process-wide state by responsibility instead of by module.
/// Configuration is intentionally not stored here: `config_manager::read_config`
/// owns the single process-wide configuration cache.
/// 脚本注册表、同步原语与延迟任务队列属于解释器 core（见 `redlang_core::{scripts, sync, scheduler}`）。
pub struct RuntimeContext {
    pub session: SessionStore,
    pub services: ServiceHub,
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self {
            session: SessionStore::new(),
            services: ServiceHub::new(),
        }
    }
}

impl Default for RuntimeContext {
    fn default() -> Self {
        Self::new()
    }
}

static RUNTIME: OnceLock<Arc<RuntimeContext>> = OnceLock::new();

/// 获取进程级运行时上下文。首次访问时创建默认上下文。
pub fn runtime_context() -> &'static RuntimeContext {
    RUNTIME.get_or_init(|| Arc::new(RuntimeContext::new()))
}
