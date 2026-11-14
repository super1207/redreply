use std::collections::HashMap;
use std::sync::RwLock;

use hyper::Method;

use super::assets::StaticAssets;
use super::auth::RouteAccess;
use super::RouteHandler;

#[derive(Clone, Debug)]
enum Segment {
    Literal(String),
    Param(String),
    Wildcard,
}

/// 编译后的路由匹配模式。
///
/// 支持三种段：
/// - 普通字面量：`/redlang/api/get_code`
/// - 参数：`/api/plugins/{name}/{action}`
/// - 通配：`/redlang/*`（匹配剩余全部路径）
#[derive(Clone, Debug)]
pub(crate) struct CompiledPattern {
    segments: Vec<Segment>,
}

impl CompiledPattern {
    pub(crate) fn parse(pattern: &str) -> Self {
        let segments = pattern
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                if segment == "*" {
                    Segment::Wildcard
                } else if segment.len() > 2 && segment.starts_with('{') && segment.ends_with('}') {
                    Segment::Param(segment[1..segment.len() - 1].to_string())
                } else {
                    Segment::Literal(segment.to_string())
                }
            })
            .collect();
        Self { segments }
    }

    /// 匹配路径并返回路径参数；失败返回 None。
    pub(crate) fn captures(&self, path: &str) -> Option<HashMap<String, String>> {
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let mut params = HashMap::new();
        let mut index = 0;
        for segment in &self.segments {
            match segment {
                Segment::Wildcard => {
                    // 通配段捕获剩余路径（可能为空），键名固定为 `*`。
                    params.insert("*".to_string(), parts[index..].join("/"));
                    return Some(params);
                }
                Segment::Literal(literal) => {
                    if index >= parts.len() || literal != parts[index] {
                        return None;
                    }
                    index += 1;
                }
                Segment::Param(name) => {
                    if index >= parts.len() {
                        return None;
                    }
                    params.insert(name.clone(), parts[index].to_string());
                    index += 1;
                }
            }
        }
        if index == parts.len() {
            Some(params)
        } else {
            None
        }
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        self.captures(path).is_some()
    }
}

pub(crate) struct RouteEntry {
    pub method: Option<Method>,
    pub pattern: CompiledPattern,
    pub access: RouteAccess,
    pub owner: Option<String>,
    pub handler: RouteHandler,
}

pub(crate) struct StaticEntry {
    /// 规范化前缀：`/` 或 `/xxx`（无尾斜杠）。
    pub prefix: String,
    pub access: RouteAccess,
    pub owner: Option<String>,
    pub assets: StaticAssets,
}

/// 路由表。
#[derive(Default)]
pub(crate) struct Router {
    routes: RwLock<Vec<RouteEntry>>,
    statics: RwLock<Vec<StaticEntry>>,
}

impl Router {
    pub(crate) fn add_route(&self, entry: RouteEntry) {
        let mut routes = self.routes.write().unwrap_or_else(|e| e.into_inner());
        routes.push(entry);
    }

    pub(crate) fn add_static(&self, entry: StaticEntry) {
        let mut statics = self.statics.write().unwrap_or_else(|e| e.into_inner());
        statics.push(entry);
    }

    /// 查找匹配的路由（需要方法一致）。
    pub(crate) fn match_route(
        &self,
        method: &Method,
        path: &str,
    ) -> Option<(RouteAccess, RouteHandler, HashMap<String, String>)> {
        let routes = self.routes.read().unwrap_or_else(|e| e.into_inner());
        for entry in routes.iter() {
            if let Some(entry_method) = &entry.method {
                if entry_method != method {
                    continue;
                }
            }
            if let Some(params) = entry.pattern.captures(path) {
                return Some((entry.access, entry.handler.clone(), params));
            }
        }
        None
    }

    /// 是否存在与方法无关的路径匹配（用于 405 判断）。
    pub(crate) fn path_exists(&self, path: &str) -> bool {
        let routes = self.routes.read().unwrap_or_else(|e| e.into_inner());
        routes.iter().any(|entry| entry.pattern.matches(path))
    }

    /// 查找匹配的静态资源挂载点，返回（前缀长度、访问级别、资源集、相对路径）。
    ///
    /// 结果按前缀从长到短排序：更具体的前缀优先；调用方应依次尝试
    /// [`StaticAssets::resolve`]，未命中时继续尝试下一个挂载点。
    pub(crate) fn match_static(
        &self,
        path: &str,
    ) -> Vec<(usize, RouteAccess, StaticAssets, String)> {
        let statics = self.statics.read().unwrap_or_else(|e| e.into_inner());
        let mut matches: Vec<(usize, RouteAccess, StaticAssets, String)> = statics
            .iter()
            .filter_map(|entry| {
                let relative = if entry.prefix == "/" {
                    Some(path.trim_start_matches('/').to_string())
                } else if path == entry.prefix {
                    Some(String::new())
                } else {
                    path.strip_prefix(&format!("{}/", entry.prefix))
                        .map(|rest| rest.to_string())
                }?;
                let mut relative = relative;
                if relative.is_empty() || relative.ends_with('/') {
                    relative.push_str("index.html");
                }
                Some((
                    entry.prefix.len(),
                    entry.access,
                    entry.assets.clone(),
                    relative,
                ))
            })
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0));
        matches
    }

    /// 移除某个 owner（插件）注册的路由与静态挂载。
    pub(crate) fn remove_owner(&self, owner: &str) -> usize {
        let mut removed = 0;
        {
            let mut routes = self.routes.write().unwrap_or_else(|e| e.into_inner());
            let before = routes.len();
            routes.retain(|entry| entry.owner.as_deref() != Some(owner));
            removed += before - routes.len();
        }
        {
            let mut statics = self.statics.write().unwrap_or_else(|e| e.into_inner());
            let before = statics.len();
            statics.retain(|entry| entry.owner.as_deref() != Some(owner));
            removed += before - statics.len();
        }
        removed
    }

    pub(crate) fn route_count(&self) -> usize {
        self.routes.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub(crate) fn static_count(&self) -> usize {
        self.statics.read().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_param_wildcard() {
        let pattern = CompiledPattern::parse("/api/plugins/{name}/{action}");
        let params = pattern.captures("/api/plugins/redlang/enable").unwrap();
        assert_eq!(params.get("name").map(String::as_str), Some("redlang"));
        assert_eq!(params.get("action").map(String::as_str), Some("enable"));
        assert!(pattern.captures("/api/plugins/redlang").is_none());
        assert!(pattern
            .captures("/api/plugins/redlang/enable/extra")
            .is_none());

        let wildcard = CompiledPattern::parse("/redlang/*");
        let params = wildcard.captures("/redlang/a/b/c").unwrap();
        assert_eq!(params.get("*").map(String::as_str), Some("a/b/c"));
        let params = wildcard.captures("/redlang/").unwrap();
        assert_eq!(params.get("*").map(String::as_str), Some(""));
        assert!(wildcard.captures("/other").is_none());

        let root = CompiledPattern::parse("/");
        assert!(root.captures("/").is_some());
        assert!(root.captures("/other").is_none());
    }

    #[test]
    fn static_prefix_matching() {
        let router = Router::default();
        router.add_static(StaticEntry {
            prefix: "/redlang".to_string(),
            access: RouteAccess::Read,
            owner: Some("redlang".to_string()),
            assets: StaticAssets::empty(),
        });
        assert!(!router.match_static("/redlang/index.html").is_empty());
        assert!(!router.match_static("/redlang").is_empty());
        let matches = router.match_static("/redlang/app.js");
        assert_eq!(matches[0].3, "app.js");
        assert!(router.match_static("/redlanging/x").is_empty());
        let matches = router.match_static("/redlang/");
        assert_eq!(matches[0].3, "index.html");
    }

    #[test]
    fn static_specific_prefix_wins_over_root() {
        let router = Router::default();
        router.add_static(StaticEntry {
            prefix: "/".to_string(),
            access: RouteAccess::Write,
            owner: Some("framework".to_string()),
            assets: StaticAssets::empty(),
        });
        router.add_static(StaticEntry {
            prefix: "/redlang".to_string(),
            access: RouteAccess::Read,
            owner: Some("redlang".to_string()),
            assets: StaticAssets::empty(),
        });
        let matches = router.match_static("/redlang/app.js");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, "/redlang".len());
        assert_eq!(matches[0].3, "app.js");
        assert_eq!(matches[1].0, 1);
    }
}
