//! 系统代理缓存（60 秒有效期）。

use std::sync::OnceLock;

use tokio::sync::Mutex;

type ProxyCache = Mutex<(Option<String>, u64)>;

static PROXY: OnceLock<ProxyCache> = OnceLock::new();

/// 进程级代理缓存 `(代理地址, 缓存时间秒)`。
pub fn proxy_cache() -> &'static ProxyCache {
    PROXY.get_or_init(|| Mutex::new((None, 0)))
}
