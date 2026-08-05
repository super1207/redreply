use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::redlang::RedValueData;

type PackageLocks = HashMap<String, HashMap<String, i32>>;
type SignalEntry = (String, String, String, Option<Arc<RedValueData>>);
type FileLocks = HashMap<String, i32>;

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

    pub(crate) fn acquire_lock(&self, pkg_name: &str, lock_name: &str) {
        let mut locks = self.package_locks.lock().unwrap();
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
                return;
            }

            let (guard, _) = self
                .package_lock_changed
                .wait_timeout(locks, Duration::from_millis(1000))
                .unwrap();
            locks = guard;
        }
    }

    pub(crate) fn try_acquire_lock(&self, pkg_name: &str, lock_name: &str) -> bool {
        let mut locks = self.package_locks.lock().unwrap();
        let package_locks = locks.entry(pkg_name.to_owned()).or_default();
        if package_locks.contains_key(lock_name) {
            false
        } else {
            package_locks.insert(lock_name.to_owned(), 0);
            true
        }
    }

    pub(crate) fn release_lock(&self, pkg_name: &str, lock_name: &str) {
        let mut locks = self.package_locks.lock().unwrap();
        if let Some(package_locks) = locks.get_mut(pkg_name) {
            package_locks.remove(lock_name);
        }
        self.package_lock_changed.notify_all();
    }

    pub(crate) fn release_locks(&self, pkg_name: &str, lock_names: &HashSet<String>) {
        let mut locks = self.package_locks.lock().unwrap();
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
        self.package_locks.lock().unwrap().remove(pkg_name);
        self.package_lock_changed.notify_all();
    }

    pub(crate) fn send_signal(
        &self,
        pkg_name: &str,
        signal_name: &str,
        value: Arc<RedValueData>,
    ) -> bool {
        let mut signals = self.signals.lock().unwrap();
        let mut changed = false;
        for (_, entry_pkg, entry_signal, entry_value) in &mut *signals {
            if entry_pkg == pkg_name && entry_signal == signal_name {
                *entry_value = Some(value.clone());
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
            .unwrap()
            .push((uid, pkg_name, signal_name, None));
    }

    pub(crate) fn unregister_signal(&self, uid: &str) {
        self.signals
            .lock()
            .unwrap()
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
        let mut signals = self.signals.lock().unwrap();
        loop {
            if let Some(value) = signals
                .iter()
                .find(|(entry_uid, _, _, value)| entry_uid == uid && value.is_some())
                .and_then(|(_, _, _, value)| value.clone())
            {
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
                .unwrap();
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
        let mut locks = self.file_locks.lock().unwrap();
        loop {
            if !locks.contains_key(filename) {
                locks.insert(filename.to_owned(), 0);
                return;
            }

            let (guard, timeout_result) = self
                .file_lock_changed
                .wait_timeout(locks, Duration::from_secs(5))
                .unwrap();
            locks = guard;
            if timeout_result.timed_out() {
                on_timeout();
            }
        }
    }

    pub(crate) fn release_file_lock(&self, filename: &str) {
        let mut locks = self.file_locks.lock().unwrap();
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
        sync.acquire_lock("demo", "lock");
        assert!(!sync.try_acquire_lock("demo", "lock"));
        sync.release_lock("demo", "lock");
        assert!(sync.try_acquire_lock("demo", "lock"));
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
    fn file_lock_can_be_released() {
        let sync = SyncPrimitives::new();
        sync.acquire_file_lock("demo.db", || panic!("unexpected timeout"));
        sync.release_file_lock("demo.db");
        sync.acquire_file_lock("demo.db", || panic!("unexpected timeout"));
    }
}
