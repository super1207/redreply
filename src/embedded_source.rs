//! 编译时嵌入的项目源码，可通过 HTTP API 或 RedLang 命令翻阅。
//! 由 build.rs 在编译时扫描 src/、crates/ 等目录生成，随二进制分发。

include!(concat!(env!("OUT_DIR"), "/embedded_source.rs"));

use std::collections::HashMap;
use std::sync::OnceLock;

fn source_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| SOURCE_FILES.iter().map(|(k, v)| (*k, *v)).collect())
}

/// 按相对路径（如 `src/lib.rs`）获取嵌入的源码文本。
pub fn get_source(path: &str) -> Option<&'static str> {
    let key = path.trim_start_matches('/').trim_start_matches("./");
    source_map().get(key).copied()
}

/// 返回所有已嵌入文件的相对路径列表（已排序）。
pub fn source_list() -> Vec<&'static str> {
    let mut list: Vec<&'static str> = SOURCE_FILES.iter().map(|(k, _)| *k).collect();
    list.sort_unstable();
    list
}