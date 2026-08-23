use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::redlang::RedValueData;

type PackageLocks = HashMap<String, HashMap<String, i32>>;
type SignalEntry = (String, String, String, Vec<Arc<RedValueData>>);
type FileLocks = HashMap<String, i32>;

#[derive(Debug)]
pub(crate) enum SignalWaitResult {
    Received(Arc<RedValueData>),
    TimedOut,
    Cancelled,
}

/// Process-wide synchronization state shared by scripts and persistence helpers.
pub(crate) struct SyncPrimitives {
    package_locks: Mutex<PackageLocks>,
    package_lock_changed: Condvar,
    signals: Mutex<Vec<SignalEntry>>,
    signal_changed: Condvar,
    file_locks: Mutex<FileLocks>,
    file_lock_changed: Condvar,
}

impl SyncPrimitives {
    pub(crate) fn new() -> Self {
        Self {
            package_locks: Mutex::new(HashMap::new()),
            package_lock_changed: Condvar::new(),
            signals: Mutex::new(Vec::new()),
            signal_changed: Condvar::new(),
            file_locks: Mutex::new(HashMap::new()),
            file_lock_changed: Condvar::new(),
        }
    }

    /// 获取包内互斥锁。等待期间周期性调用 `can_continue`，返回 false 时放弃并返回 false
    /// （调用方应结束脚本，已持有的锁由 Drop 统一释放），避免脚本间死锁永久占死线程。
    pub(crate) fn acquire_lock<F>(&self, pkg_name: &str, lock_name: &str, mut can_continue: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let mut locks = self.package_locks.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let acquired = {
                let package_locks = locks.entry(pkg_name.to_owned()).or_default();
                if package_locks.contains_key(lock_name) {
                    false
                } else {
                    package_locks.insert(lock_name.to_owned(), 0);
                    true
                }
            };
            if acquired {
                return true;
            }
            if !can_continue() {
                return false;
            }

            let (guard, _) = self
                .package_lock_changed
                .wait_timeout(locks, Duration::from_millis(1000))
                .unwrap_or_else(|e| e.into_inner());
            locks = guard;
        }
    }

    pub(crate) fn try_acquire_lock(&self, pkg_name: &str, lock_name: &str) -> bool {
        let mut locks = self
            .package_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let package_locks = locks.entry(pkg_name.to_owned()).or_default();
        if package_locks.contains_key(lock_name) {
            false
        } else {
            package_locks.insert(lock_name.to_owned(), 0);
            true
        }
    }

    pub(crate) fn release_lock(&self, pkg_name: &str, lock_name: &str) {
        let mut locks = self
            .package_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(package_locks) = locks.get_mut(pkg_name) {
            package_locks.remove(lock_name);
        }
        self.package_lock_changed.notify_all();
    }

    pub(crate) fn release_locks(&self, pkg_name: &str, lock_names: &HashSet<String>) {
        let mut locks = self
            .package_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(package_locks) = locks.get_mut(pkg_name) {
            for lock_name in lock_names {
                package_locks.remove(lock_name);
            }
        }
        if !lock_names.is_empty() {
            self.package_lock_changed.notify_all();
        }
    }

    pub(crate) fn release_package_locks(&self, pkg_name: &str) {
        self.package_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(pkg_name);
        self.package_lock_changed.notify_all();
    }

    /// 向同名信号的所有等待者投递值。采用队列语义：同一等待者收到多条信号时按序消费，
    /// 不再相互覆盖；没有等待者时信号丢弃并返回 false。
    pub(crate) fn send_signal(
        &self,
        pkg_name: &str,
        signal_name: &str,
        value: Arc<RedValueData>,
    ) -> bool {
        let mut signals = self.signals.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;
        for (_, entry_pkg, entry_signal, entry_queue) in &mut *signals {
            if entry_pkg == pkg_name && entry_signal == signal_name {
                entry_queue.push(value.clone());
                changed = true;
            }
        }
        if changed {
            self.signal_changed.notify_all();
        }
        changed
    }

    pub(crate) fn register_signal(&self, uid: String, pkg_name: String, signal_name: String) {
        self.signals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((uid, pkg_name, signal_name, Vec::new()));
    }

    pub(crate) fn unregister_signal(&self, uid: &str) {
        self.signals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(entry_uid, _, _, _)| entry_uid != uid);
    }

    pub(crate) fn wait_for_signal<F>(
        &self,
        uid: &str,
        timeout: Duration,
        mut can_continue: F,
    ) -> SignalWaitResult
    where
        F: FnMut() -> bool,
    {
        let start_time = Instant::now();
        let mut signals = self.signals.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            // 按发送顺序消费队列中的最早一条信号
            if let Some(pos) = signals.iter().position(|(entry_uid, _, _, queue)| {
                entry_uid == uid && !queue.is_empty()
            }) {
                let value = signals[pos].3.remove(0);
                return SignalWaitResult::Received(value);
            }
            if !can_continue() {
                return SignalWaitResult::Cancelled;
            }

            let Some(remaining) = timeout.checked_sub(start_time.elapsed()) else {
                return SignalWaitResult::TimedOut;
            };
            if remaining.is_zero() {
                return SignalWaitResult::TimedOut;
            }

            let (guard, timeout_result) = self
                .signal_changed
                .wait_timeout(signals, remaining)
                .unwrap_or_else(|e| e.into_inner());
            signals = guard;
            if timeout_result.timed_out() {
                return SignalWaitResult::TimedOut;
            }
        }
    }

    pub(crate) fn acquire_file_lock<F>(&self, filename: &str, mut on_timeout: F)
    where
        F: FnMut(),
    {
        let mut locks = match self.file_locks.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        loop {
            if !locks.contains_key(filename) {
                locks.insert(filename.to_owned(), 0);
                return;
            }

            let (guard, timeout_result) = self
                .file_lock_changed
                .wait_timeout(locks, Duration::from_secs(5))
                .unwrap_or_else(|e| e.into_inner());
            locks = guard;
            if timeout_result.timed_out() {
                // 避免持锁回调，防止回调中再尝试获取同锁导致死锁
                drop(locks);
                on_timeout();
                locks = match self.file_locks.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
            }
        }
    }

    pub(crate) fn release_file_lock(&self, filename: &str) {
        let mut locks = self.file_locks.lock().unwrap_or_else(|e| e.into_inner());
        if locks.remove(filename).is_some() {
            self.file_lock_changed.notify_all();
        }
    }
}

impl Default for SyncPrimitives {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{SignalWaitResult, SyncPrimitives};
    use crate::redlang::RedValueData;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn package_lock_can_be_released_and_reacquired() {
        let sync = SyncPrimitives::new();
        assert!(sync.acquire_lock("demo", "lock", || true));
        assert!(!sync.try_acquire_lock("demo", "lock"));
        sync.release_lock("demo", "lock");
        assert!(sync.try_acquire_lock("demo", "lock"));
    }

    #[test]
    fn acquire_lock_can_be_cancelled_while_waiting() {
        let sync = SyncPrimitives::new();
        assert!(sync.acquire_lock("demo", "lock", || true));
        let cancelled = sync.acquire_lock("demo", "lock", || false);
        assert!(!cancelled);
        assert!(sync.try_acquire_lock("demo2", "lock"));
    }

    #[test]
    fn signal_wait_is_woken_by_matching_signal() {
        let sync = Arc::new(SyncPrimitives::new());
        sync.register_signal("id".to_owned(), "demo".to_owned(), "ready".to_owned());

        let waiter = Arc::clone(&sync);
        let handle =
            thread::spawn(move || waiter.wait_for_signal("id", Duration::from_secs(1), || true));
        thread::sleep(Duration::from_millis(20));
        sync.send_signal(
            "demo",
            "ready",
            Arc::new(RedValueData::Text("ok".to_owned())),
        );

        assert!(matches!(
            handle.join().unwrap(),
            SignalWaitResult::Received(value)
                if matches!(&*value, RedValueData::Text(text) if text == "ok")
        ));
        sync.unregister_signal("id");
    }

    #[test]
    fn queued_signals_are_consumed_in_order_without_overwrite() {
        let sync = SyncPrimitives::new();
        sync.register_signal("id".to_owned(), "demo".to_owned(), "ready".to_owned());
        assert!(sync.send_signal(
            "demo",
            "ready",
            Arc::new(RedValueData::Text("first".to_owned()))
        ));
        assert!(sync.send_signal(
            "demo",
            "ready",
            Arc::new(RedValueData::Text("second".to_owned()))
        ));
        for expected in ["first", "second"] {
            match sync.wait_for_signal("id", Duration::from_millis(100), || true) {
                SignalWaitResult::Received(value) => assert!(
                    matches!(&*value, RedValueData::Text(text) if text == expected),
                    "expected `{expected}`"
                ),
                other => panic!("unexpected result: {other:?}"),
            }
        }
        // 队列消费完毕后回到超时路径
        assert!(matches!(
            sync.wait_for_signal("id", Duration::from_millis(50), || true),
            SignalWaitResult::TimedOut
        ));
        sync.unregister_signal("id");
    }

    #[test]
    fn file_lock_can_be_released() {
        let sync = SyncPrimitives::new();
        sync.acquire_file_lock("demo.db", || panic!("unexpected timeout"));
        sync.release_file_lock("demo.db");
        sync.acquire_file_lock("demo.db", || panic!("unexpected timeout"));
    }
}
