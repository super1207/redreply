use std::sync::{Arc, Mutex, OnceLock};

pub struct ServiceHub {
    runtime: OnceLock<Arc<tokio::runtime::Runtime>>,
    auto_close: Mutex<bool>,
}

impl ServiceHub {
    pub fn new() -> Self {
        Self {
            runtime: OnceLock::new(),
            auto_close: Mutex::new(false),
        }
    }

    pub fn runtime(&self) -> Arc<tokio::runtime::Runtime> {
        self.runtime
            .get_or_init(|| Arc::new(tokio::runtime::Runtime::new().unwrap()))
            .clone()
    }

    pub fn set_auto_close(&self, auto_close: bool) {
        *self.auto_close.lock().unwrap_or_else(|e| e.into_inner()) = auto_close;
    }

    pub fn auto_close(&self) -> bool {
        *self.auto_close.lock().unwrap_or_else(|e| e.into_inner())
    }

}

impl Default for ServiceHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceHub;

    #[test]
    fn service_state_round_trips() {
        let services = ServiceHub::new();
        assert!(!services.auto_close());
        services.set_auto_close(true);
        assert!(services.auto_close());
    }
}
