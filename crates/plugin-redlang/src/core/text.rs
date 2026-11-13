//! 文本工具：简繁转换（命令名归一化用）。

use zhconv::{zhconv, Variant};

/// 转换为简体。
pub fn str_to_jt(s: &str) -> String {
    zhconv(s, Variant::ZhCN)
}

/// 转换为繁体。
pub fn str_to_ft(s: &str) -> String {
    zhconv(s, Variant::ZhHK)
}
