use std::sync::{Arc, OnceLock};

use crate::AdapterResult;

pub trait AdapterHost: Send + Sync + 'static {
    fn log(&self, msg: &str);
    fn warn(&self, msg: &str);
    fn dispatch_event(&self, event_json: &str) -> AdapterResult<()>;
    fn app_dir(&self) -> AdapterResult<String>;
    fn all_to_silk(&self, input: &[u8]) -> AdapterResult<Vec<u8>>;
}

static HOST: OnceLock<Arc<dyn AdapterHost>> = OnceLock::new();

pub fn set_host(host: Arc<dyn AdapterHost>) -> Result<(), Arc<dyn AdapterHost>> {
    HOST.set(host)
}

fn host() -> Option<&'static Arc<dyn AdapterHost>> {
    HOST.get()
}

pub fn log(msg: &str) {
    if let Some(host) = host() {
        host.log(msg);
    }
}

pub fn warn(msg: &str) {
    if let Some(host) = host() {
        host.warn(msg);
    }
}

pub fn dispatch_event(event_json: &str) -> AdapterResult<()> {
    if let Some(host) = host() {
        host.dispatch_event(event_json)
    } else {
        Ok(())
    }
}

pub fn app_dir() -> AdapterResult<String> {
    if let Some(host) = host() {
        host.app_dir()
    } else {
        Ok(String::new())
    }
}

pub fn all_to_silk(input: &[u8]) -> AdapterResult<Vec<u8>> {
    if let Some(host) = host() {
        host.all_to_silk(input)
    } else {
        Err("adapter host does not provide all_to_silk".into())
    }
}

