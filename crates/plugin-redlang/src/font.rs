//! 默认字体缓存（`默认字体` 命令下载后复用，避免重复下载 10MB 字体）。

use std::sync::{OnceLock, RwLock};

static DEFAULT_FONT: OnceLock<RwLock<Option<Vec<u8>>>> = OnceLock::new();

fn store() -> &'static RwLock<Option<Vec<u8>>> {
    DEFAULT_FONT.get_or_init(|| RwLock::new(None))
}

pub fn default_font() -> Option<Vec<u8>> {
    store()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn set_default_font(font: Vec<u8>) {
    *store().write().unwrap_or_else(|error| error.into_inner()) = Some(font);
}
