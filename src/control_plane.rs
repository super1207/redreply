use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Condvar, Mutex, RwLock,
};
use std::time::SystemTime;

use crate::ScriptInfo;

/// Script lifecycle and process/package shutdown state.
pub(crate) struct ControlPlane {
    quit_flag: RwLock<bool>,
    package_quit_flags: RwLock<HashSet<String>>,
    running_script_num: RwLock<usize>,
    loading_script_flags: RwLock<HashSet<String>>,
    running_scripts: Mutex<Vec<ScriptInfo>>,
    script_quit_changed: Condvar,
    script_run_id: AtomicU64,
    start_time: Mutex<String>,
}

impl ControlPlane {
    pub(crate) fn new() -> Self {
        Self {
            quit_flag: RwLock::new(false),
            package_quit_flags: RwLock::new(HashSet::new()),
            running_script_num: RwLock::new(0),
            loading_script_flags: RwLock::new(HashSet::new()),
            running_scripts: Mutex::new(Vec::new()),
            script_quit_changed: Condvar::new(),
            script_run_id: AtomicU64::new(1),
            start_time: Mutex::new("0".to_owned()),
        }
    }

    pub(crate) fn pkg_can_run(&self, pkg_name: &str, script_type: &str) -> bool {
        if *self.quit_flag.read().unwrap() {
            return false;
        }
        if self.package_quit_flags.read().unwrap().contains(pkg_name) {
            return false;
        }
        if script_type != "init"
            && script_type != "输入流"
            && script_type != "延时"
            && script_type != "等待信号"
            && self.loading_script_flags.read().unwrap().contains(pkg_name)
        {
            return false;
        }
        true
    }

    pub(crate) fn set_quit(&self, quit: bool) {
        *self.quit_flag.write().unwrap() = quit;
    }

    pub(crate) fn set_package_quit(&self, pkg_name: &str, quitting: bool) {
        let mut flags = self.package_quit_flags.write().unwrap();
        if quitting {
            flags.insert(pkg_name.to_owned());
        } else {
            flags.remove(pkg_name);
        }
    }

    pub(crate) fn set_loading_script(&self, pkg_name: &str, loading: bool) {
        let mut flags = self.loading_script_flags.write().unwrap();
        if loading {
            flags.insert(pkg_name.to_owned());
        } else {
            flags.remove(pkg_name);
        }
    }

    pub(crate) fn wait_one_pkg_quit(&self, pkg_name: &str, timeout_ms: i64) -> bool {
        let start_time = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mut scripts = self.running_scripts.lock().unwrap();
        loop {
            if !scripts.iter().any(|script| script.pkg_name == pkg_name) {
                return true;
            }
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let elapsed = now - start_time;
            if elapsed > timeout_ms {
                return false;
            }
            let remaining = (timeout_ms - elapsed).max(1) as u64;
            let (guard, _) = self
                .script_quit_changed
                .wait_timeout(scripts, core::time::Duration::from_millis(remaining))
                .unwrap();
            scripts = guard;
        }
    }

    pub(crate) fn wait_all_pkg_quit(&self, timeout_ms: i64) -> bool {
        let start_time = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mut scripts = self.running_scripts.lock().unwrap();
        loop {
            if scripts.is_empty() {
                return true;
            }
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let elapsed = now - start_time;
            if elapsed > timeout_ms {
                return false;
            }
            let remaining = (timeout_ms - elapsed).max(1) as u64;
            let (guard, _) = self
                .script_quit_changed
                .wait_timeout(scripts, core::time::Duration::from_millis(remaining))
                .unwrap();
            scripts = guard;
        }
    }

    pub(crate) fn add_running_script_num(
        &self,
        pkg_name: &str,
        script_name: &str,
        script_type: &str,
    ) -> Option<u64> {
        // Keep the original lock order: running scripts, then the count,
        // followed by the process/package state checks.
        let mut scripts = self.running_scripts.lock().unwrap();
        let mut count = self.running_script_num.write().unwrap();
        if *self.quit_flag.read().unwrap()
            || self.package_quit_flags.read().unwrap().contains(pkg_name)
            || (script_type != "init"
                && script_type != "输入流"
                && script_type != "延时"
                && script_type != "等待信号"
                && self.loading_script_flags.read().unwrap().contains(pkg_name))
        {
            return None;
        }

        let run_id = self.script_run_id.fetch_add(1, Ordering::Relaxed);
        *count += 1;
        scripts.push(ScriptInfo {
            pkg_name: pkg_name.to_owned(),
            script_name: script_name.to_owned(),
            run_id,
        });
        Some(run_id)
    }

    pub(crate) fn running_script_info(&self) -> Vec<ScriptInfo> {
        self.running_scripts.lock().unwrap().clone()
    }

    pub(crate) fn dec_running_script_num(&self, run_id: u64) {
        let mut scripts = self.running_scripts.lock().unwrap();
        let mut count = self.running_script_num.write().unwrap();
        if let Some(pos) = scripts.iter().position(|script| script.run_id == run_id) {
            scripts.remove(pos);
            if *count != 0 {
                *count -= 1;
            }
            self.script_quit_changed.notify_all();
        }
    }

    pub(crate) fn set_start_time(&self, start_time: String) {
        *self.start_time.lock().unwrap() = start_time;
    }

    pub(crate) fn start_time(&self) -> String {
        self.start_time.lock().unwrap().clone()
    }
}

impl Default for ControlPlane {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ControlPlane;

    #[test]
    fn running_script_lifecycle_updates_wait_state() {
        let control = ControlPlane::new();
        assert!(control.wait_all_pkg_quit(0));
        let run_id = control
            .add_running_script_num("demo", "script", "normal")
            .unwrap();
        assert_eq!(control.running_script_info().len(), 1);
        assert!(!control.wait_one_pkg_quit("demo", 0));
        control.dec_running_script_num(run_id);
        assert!(control.wait_one_pkg_quit("demo", 0));
        assert!(control.wait_all_pkg_quit(0));
    }

    #[test]
    fn package_and_process_flags_block_scripts() {
        let control = ControlPlane::new();
        control.set_package_quit("demo", true);
        assert!(!control.pkg_can_run("demo", "normal"));
        control.set_package_quit("demo", false);
        control.set_loading_script("demo", true);
        assert!(!control.pkg_can_run("demo", "normal"));
        assert!(control.pkg_can_run("demo", "输入流"));
        control.set_quit(true);
        assert!(!control.pkg_can_run("other", "init"));
    }
}
