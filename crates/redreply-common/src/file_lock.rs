//! 文件锁与 SQLite 并发等待：脚本读写同一文件/数据库时串行化，避免竞争。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::ThreadId;

#[derive(Clone, Copy)]
struct FileLockEntry {
    owner: ThreadId,
    count: u32,
}

fn file_lock_state() -> &'static (Mutex<HashMap<String, FileLockEntry>>, Condvar) {
    static STATE: OnceLock<(Mutex<HashMap<String, FileLockEntry>>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(HashMap::new()), Condvar::new()))
}

fn absolute_path(filename: &str) -> Option<String> {
    let path = PathBuf::from(filename);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    Some(absolute.to_string_lossy().into_owned())
}

/// 获取文件锁（同线程可重入；被占用时每 5 秒记录一次等待日志）。
pub fn add_file_lock(filename: &str) {
    let Some(filename) = absolute_path(filename) else {
        return;
    };
    let (lock, changed) = file_lock_state();
    let current = std::thread::current().id();
    let mut locks = lock.lock().unwrap_or_else(|error| error.into_inner());
    loop {
        if let Some(entry) = locks.get_mut(&filename) {
            if entry.owner == current {
                entry.count = entry.count.saturating_add(1);
                return;
            }
        } else {
            locks.insert(
                filename.clone(),
                FileLockEntry {
                    owner: current,
                    count: 1,
                },
            );
            return;
        }
        let (guard, timeout) = changed
            .wait_timeout(locks, std::time::Duration::from_secs(5))
            .unwrap_or_else(|error| error.into_inner());
        locks = guard;
        if timeout.timed_out() {
            log::warn!("等待文件锁超时(5s)，仍在等待:{filename}");
        }
    }
}

/// 释放文件锁（只对当前线程持有的锁生效）。
pub fn del_file_lock(filename: &str) {
    let Some(filename) = absolute_path(filename) else {
        return;
    };
    let (lock, changed) = file_lock_state();
    let current = std::thread::current().id();
    let mut locks = lock.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = locks.get_mut(&filename) {
        if entry.owner == current {
            entry.count = entry.count.saturating_sub(1);
            if entry.count == 0 {
                locks.remove(&filename);
            }
            changed.notify_all();
        }
    }
}

/// 为 SQLite 连接设置并发等待：多连接并发访问同一 DB 文件时，
/// 不设置 busy_timeout 会默认立即返回 SQLITE_BUSY 导致操作失败。
pub fn configure_conn_busy_timeout(conn: &rusqlite::Connection) {
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
}
