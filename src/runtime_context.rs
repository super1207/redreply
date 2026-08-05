use std::sync::{Arc, OnceLock};

use crate::control_plane::ControlPlane;
use crate::onebot_store::OneBotStore;
use crate::scheduler_store::SchedulerStore;
use crate::script_store::ScriptStore;
use crate::service_hub::ServiceHub;
use crate::session_store::SessionStore;
use crate::status::StatusStore;
use crate::sync_primitives::SyncPrimitives;

/// 进程级运行时上下文。
///
/// RuntimeContext owns process-wide state by responsibility instead of by module.
/// Configuration is intentionally not stored here: `config_manager::read_config`
/// owns the single process-wide configuration cache.
pub(crate) struct RuntimeContext {
    pub(crate) scripts: ScriptStore,
    pub(crate) sync: SyncPrimitives,
    pub(crate) control: ControlPlane,
    pub(crate) session: SessionStore,
    pub(crate) services: ServiceHub,
    pub(crate) status: StatusStore,
    pub(crate) scheduler: SchedulerStore,
    pub(crate) onebot: OneBotStore,
}

impl RuntimeContext {
    pub(crate) fn new() -> Self {
        Self {
            scripts: ScriptStore::new(),
            sync: SyncPrimitives::new(),
            control: ControlPlane::new(),
            session: SessionStore::new(),
            services: ServiceHub::new(),
            status: StatusStore::new(),
            scheduler: SchedulerStore::new(),
            onebot: OneBotStore::new(),
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
pub(crate) fn runtime_context() -> &'static RuntimeContext {
    RUNTIME.get_or_init(|| Arc::new(RuntimeContext::new()))
}
