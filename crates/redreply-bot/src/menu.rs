//! 插件菜单注册表：插件向框架管理台贡献导航入口。
//!
//! 插件在 `register` 阶段调用 [`crate::PluginContext::register_menu`]，
//! 菜单会显示在框架管理台顶栏；插件停用时菜单随 owner 一起回收。

use std::sync::RwLock;

use serde::Serialize;

use crate::error::{FrameworkError, Result};

/// 管理台菜单项。
#[derive(Clone, Debug, Serialize)]
pub struct MenuItem {
    /// 菜单 ID（同一插件内唯一）。
    pub id: String,
    /// 显示文本。
    pub label: String,
    /// 跳转地址（同端口绝对路径）。
    pub url: String,
    /// 排序值，越小越靠前。
    pub order: i32,
    /// 所属插件（由框架填写）。
    pub plugin: String,
}

impl MenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            url: url.into(),
            order: 0,
            plugin: String::new(),
        }
    }

    /// 设置排序值（越小越靠前）。
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

/// 菜单注册表。
#[derive(Default)]
pub struct MenuRegistry {
    items: RwLock<Vec<MenuItem>>,
}

impl MenuRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_with_owner(&self, owner: &str, mut item: MenuItem) -> Result<()> {
        if item.id.is_empty() || item.label.is_empty() || item.url.is_empty() {
            return Err(FrameworkError::Plugin(
                "菜单的 id/label/url 不能为空".to_string(),
            ));
        }
        let mut items = self.items.write().unwrap_or_else(|e| e.into_inner());
        if items
            .iter()
            .any(|existing| existing.plugin == owner && existing.id == item.id)
        {
            return Err(FrameworkError::Plugin(format!(
                "菜单 `{}` 已被插件 `{owner}` 注册",
                item.id
            )));
        }
        item.plugin = owner.to_string();
        items.push(item);
        items.sort_by(|a, b| a.order.cmp(&b.order).then(a.label.cmp(&b.label)));
        Ok(())
    }

    /// 全部菜单（按 order、label 排序）。
    pub fn list(&self) -> Vec<MenuItem> {
        self.items
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn remove_owner(&self, owner: &str) -> usize {
        let mut items = self.items.write().unwrap_or_else(|e| e.into_inner());
        let before = items.len();
        items.retain(|item| item.plugin != owner);
        before - items.len()
    }

    pub fn len(&self) -> usize {
        self.items.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_sorts_and_dedups_per_owner() {
        let registry = MenuRegistry::new();
        registry
            .register_with_owner("b", MenuItem::new("b1", "乙", "/b").order(20))
            .unwrap();
        registry
            .register_with_owner("a", MenuItem::new("a1", "甲", "/a").order(10))
            .unwrap();
        let labels: Vec<_> = registry.list().into_iter().map(|item| item.label).collect();
        assert_eq!(labels, vec!["甲", "乙"]);

        // 同一插件重复 id 报错；不同插件可同名
        assert!(registry
            .register_with_owner("a", MenuItem::new("a1", "甲2", "/a2"))
            .is_err());
        assert!(registry
            .register_with_owner("b", MenuItem::new("a1", "乙2", "/b2"))
            .is_ok());
    }

    #[test]
    fn remove_owner_only_removes_owned() {
        let registry = MenuRegistry::new();
        registry
            .register_with_owner("a", MenuItem::new("a1", "甲", "/a"))
            .unwrap();
        registry
            .register_with_owner("b", MenuItem::new("b1", "乙", "/b"))
            .unwrap();
        assert_eq!(registry.remove_owner("a"), 1);
        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.list()[0].plugin, "b");
    }
}
