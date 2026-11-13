use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::error::{FrameworkError, Result};

#[derive(Clone)]
struct ServiceEntry {
    value: Arc<dyn Any + Send + Sync>,
    owner: Option<String>,
}

/// 通用服务注册表。
///
/// 插件通过 [`crate::PluginContext::register_service`] 注册服务，
/// 其他插件/框架通过名字与类型取用（`get::<T>`）。
/// 名字全局唯一，重复注册会返回错误；插件停止时框架会按 owner 回收服务。
#[derive(Default)]
pub struct ServiceRegistry {
    entries: RwLock<HashMap<String, ServiceEntry>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个服务；名字已存在时返回错误。
    pub fn register<T: Any + Send + Sync>(&self, name: &str, service: Arc<T>) -> Result<()> {
        self.register_with_owner(name, service, None)
    }

    pub(crate) fn register_with_owner<T: Any + Send + Sync>(
        &self,
        name: &str,
        service: Arc<T>,
        owner: Option<&str>,
    ) -> Result<()> {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if entries.contains_key(name) {
            return Err(FrameworkError::Service(format!("服务 `{name}` 已被注册")));
        }
        entries.insert(
            name.to_string(),
            ServiceEntry {
                value: service,
                owner: owner.map(str::to_string),
            },
        );
        Ok(())
    }

    /// 按名字与类型取服务；不存在或类型不匹配返回 None。
    pub fn get<T: Any + Send + Sync>(&self, name: &str) -> Option<Arc<T>> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries
            .get(name)
            .and_then(|entry| entry.value.clone().downcast::<T>().ok())
    }

    pub fn contains(&self, name: &str) -> bool {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries.contains_key(name)
    }

    pub fn remove(&self, name: &str) -> bool {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.remove(name).is_some()
    }

    /// 移除某个 owner（插件）注册的全部服务，返回被移除的服务名。
    pub(crate) fn remove_owner(&self, owner: &str) -> Vec<String> {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let names: Vec<String> = entries
            .iter()
            .filter(|(_, entry)| entry.owner.as_deref() == Some(owner))
            .map(|(name, _)| name.clone())
            .collect();
        for name in &names {
            entries.remove(name);
        }
        names
    }

    /// 全部服务名（排序后）。
    pub fn names(&self) -> Vec<String> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let mut names: Vec<String> = entries.keys().cloned().collect();
        names.sort();
        names
    }

    /// 服务名的注册者（插件名）。
    pub fn owner(&self, name: &str) -> Option<String> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries.get(name).and_then(|entry| entry.owner.clone())
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Greeting {
        text: String,
    }

    struct Other;

    #[test]
    fn register_and_get() {
        let registry = ServiceRegistry::new();
        registry
            .register(
                "greet",
                Arc::new(Greeting {
                    text: "你好".into(),
                }),
            )
            .unwrap();
        assert!(registry.contains("greet"));
        let service = registry.get::<Greeting>("greet").unwrap();
        assert_eq!(service.text, "你好");
        assert!(registry.get::<Other>("greet").is_none());
        assert!(registry.get::<Greeting>("missing").is_none());
    }

    #[test]
    fn duplicate_is_rejected() {
        let registry = ServiceRegistry::new();
        registry.register("greet", Arc::new(Greeting { text: "a".into() })).unwrap();
        let error = registry
            .register("greet", Arc::new(Greeting { text: "b".into() }))
            .unwrap_err();
        assert!(matches!(error, FrameworkError::Service(_)));
    }

    #[test]
    fn remove_owner_only_removes_owned() {
        let registry = ServiceRegistry::new();
        registry
            .register_with_owner("a", Arc::new(Other), Some("p1"))
            .unwrap();
        registry
            .register_with_owner("b", Arc::new(Other), Some("p2"))
            .unwrap();
        let removed = registry.remove_owner("p1");
        assert_eq!(removed, vec!["a".to_string()]);
        assert!(registry.contains("b"));
    }
}
