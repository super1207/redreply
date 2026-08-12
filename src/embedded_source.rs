//! 编译时嵌入的项目源码，可通过 HTTP API 或 RedLang 命令翻阅。
//! 由 build.rs 在编译时扫描项目源码及 res/、docs/ 文本前端资源生成，随二进制分发。

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

/// 返回所有已嵌入文件的相对路径列表（已排序，测试用）。
#[cfg(test)]
pub fn source_list() -> Vec<&'static str> {
    let mut list: Vec<&'static str> = SOURCE_FILES.iter().map(|(k, _)| *k).collect();
    list.sort_unstable();
    list
}

/// 按目录查看已嵌入源码：返回该目录下的子目录名与文件相对路径。
/// `dir` 为空表示根目录。返回 `(子目录, 文件, 上级目录)`，上级目录为空串表示根。
pub fn source_list_dir(dir: &str) -> (Vec<String>, Vec<String>, String) {
    let dir = dir.trim().trim_matches('/');
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let mut subdirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for (key, _) in SOURCE_FILES {
        if !prefix.is_empty() && !key.starts_with(&prefix) {
            continue;
        }
        let rest = if prefix.is_empty() {
            *key
        } else {
            &key[prefix.len()..]
        };
        match rest.find('/') {
            Some(position) => {
                let subdir = rest[..position].to_owned();
                if !subdirs.contains(&subdir) {
                    subdirs.push(subdir);
                }
            }
            None => files.push((*key).to_owned()),
        }
    }
    subdirs.sort();
    files.sort();
    let parent = if dir.is_empty() {
        String::new()
    } else {
        match dir.rfind('/') {
            Some(position) => dir[..position].to_owned(),
            None => String::new(),
        }
    };
    (subdirs, files, parent)
}

#[cfg(test)]
mod tests {
    use super::{get_source, source_list, source_list_dir};

    #[test]
    fn exposes_embedded_frontend_text_resources() {
        let list = source_list();
        assert!(list.contains(&"res/index.html"));
        assert!(list.contains(&"res/live2d/chat.js"));
        assert!(list.contains(&"docs/index.html"));
        assert!(get_source("./res/live2d/chat.js").is_some());
        assert!(get_source("docs/index.html").is_some());
    }

    #[test]
    fn lists_directory_tree_instead_of_flat_list() {
        // 根目录：列出所有一级子目录，根下直接的文件只有 Cargo.toml、build.rs、readme.md 等
        let (subdirs, files, parent) = source_list_dir("");
        assert!(subdirs.contains(&"src".to_owned()));
        assert!(subdirs.contains(&"res".to_owned()));
        assert!(subdirs.contains(&"crates".to_owned()));
        assert!(files.contains(&"Cargo.toml".to_owned()));
        assert!(parent.is_empty());

        // 进入 src 目录：应包含 httpserver、redlang 等子目录，以及 lib.rs、main.rs 等文件
        let (subdirs, files, parent) = source_list_dir("src");
        assert!(subdirs.contains(&"httpserver".to_owned()));
        assert!(subdirs.contains(&"redlang".to_owned()));
        assert!(files.contains(&"src/lib.rs".to_owned()));
        assert!(files.contains(&"src/main.rs".to_owned()));
        assert!(parent.is_empty());

        // 子目录的 parent 返回上级目录
        let (_subdirs, files, parent) = source_list_dir("src/httpserver");
        assert!(files.contains(&"src/httpserver/mod.rs".to_owned()));
        assert!(files.contains(&"src/httpserver/live2d.rs".to_owned()));
        assert_eq!(parent, "src");

        // 不存在/空目录：无子目录无文件
        let (subdirs, files, _parent) = source_list_dir("不存在的目录");
        assert!(subdirs.is_empty());
        assert!(files.is_empty());
    }
}
