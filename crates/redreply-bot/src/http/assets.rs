use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// 一个静态文件。
#[derive(Clone, Debug)]
pub struct StaticFile {
    pub data: Cow<'static, [u8]>,
    pub content_type: String,
}

impl StaticFile {
    /// 由路径推断内容类型。
    pub fn new(path: &str, data: impl AsRef<[u8]>) -> Self {
        Self {
            data: Cow::Owned(data.as_ref().to_vec()),
            content_type: mime_for(path).to_string(),
        }
    }
}

type Resolver = Arc<dyn Fn(&str) -> Option<StaticFile> + Send + Sync>;

/// 静态资源集合。
///
/// 支持嵌入式资源（如 rust-embed 生成）与磁盘目录两种来源。
#[derive(Clone)]
pub struct StaticAssets {
    resolver: Resolver,
    /// 可选 Cache-Control 头。
    pub cache_control: Option<String>,
}

impl StaticAssets {
    pub fn from_fn<F>(resolver: F) -> Self
    where
        F: Fn(&str) -> Option<StaticFile> + Send + Sync + 'static,
    {
        Self {
            resolver: Arc::new(resolver),
            cache_control: None,
        }
    }

    /// 以磁盘目录为来源；路径穿越（`..`）会被拒绝。
    pub fn from_dir(root: impl Into<PathBuf>) -> Self {
        let root: PathBuf = root.into();
        Self::from_fn(move |path| {
            let relative = Path::new(path);
            if relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            }) {
                return None;
            }
            let full = root.join(relative);
            if !full.is_file() {
                return None;
            }
            std::fs::read(&full)
                .ok()
                .map(|data| StaticFile::new(path, Cow::Owned(data)))
        })
    }

    /// 什么也不返回的空资源集。
    pub fn empty() -> Self {
        Self::from_fn(|_| None)
    }

    pub fn with_cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    pub(crate) fn resolve(&self, path: &str) -> Option<StaticFile> {
        (self.resolver)(path)
    }
}

impl std::fmt::Debug for StaticAssets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticAssets")
            .field("cache_control", &self.cache_control)
            .finish_non_exhaustive()
    }
}

/// 由扩展名推断内容类型。
pub fn mime_for(path: &str) -> &'static str {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "txt" | "md" => "text/plain; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_by_extension() {
        assert_eq!(mime_for("a/b/index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("app.js"), "application/javascript; charset=utf-8");
        assert_eq!(mime_for("x.unknown"), "application/octet-stream");
    }

    #[test]
    fn dir_source_rejects_traversal() {
        let assets = StaticAssets::from_dir(std::env::temp_dir());
        assert!(assets.resolve("../secret.txt").is_none());
    }
}
