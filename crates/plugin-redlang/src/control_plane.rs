use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Condvar, Mutex, OnceLock, RwLock,
};
use std::time::{Duration, Instant};

use redlang::ScriptInfo;

static CONTROL: OnceLock<ControlPlane> = OnceLock::new();

/// 进程级脚本控制平面。
pub fn control() -> &'static ControlPlane {
    CONTROL.get_or_init(ControlPlane::new)
}

/// Script lifecycle and process/package shutdown state.
pub struct ControlPlane {
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
    pub fn new() -> Self {
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

    pub fn pkg_can_run(&self, pkg_name: &str, script_type: &str) -> bool {
        let quit = match self.quit_flag.read() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        };
        if quit {
            return false;
        }
        let contains_pkg = match self.package_quit_flags.read() {
            Ok(g) => g.contains(pkg_name),
            Err(p) => p.into_inner().contains(pkg_name),
        };
        if contains_pkg {
            return false;
        }
        if script_type != "init"
            && script_type != "输入流"
            && script_type != "延时"
            && script_type != "等待信号"
        {
            let loading = match self.loading_script_flags.read() {
                Ok(g) => g.contains(pkg_name),
                Err(p) => p.into_inner().contains(pkg_name),
            };
            if loading {
                return false;
            }
        }
        true
    }

    pub fn set_quit(&self, quit: bool) {
        match self.quit_flag.write() {
            Ok(mut g) => *g = quit,
            Err(p) => *p.into_inner() = quit,
        }
    }

    pub fn set_package_quit(&self, pkg_name: &str, quitting: bool) {
        let mut flags = match self.package_quit_flags.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if quitting {
            flags.insert(pkg_name.to_owned());
        } else {
            flags.remove(pkg_name);
        }
    }

    pub fn set_loading_script(&self, pkg_name: &str, loading: bool) {
        let mut flags = match self.loading_script_flags.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if loading {
            flags.insert(pkg_name.to_owned());
        } else {
            flags.remove(pkg_name);
        }
    }

    pub fn wait_one_pkg_quit(&self, pkg_name: &str, timeout_ms: i64) -> bool {
        let start = Instant::now();
        let mut scripts = match self.running_scripts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        loop {
            if !scripts.iter().any(|script| script.pkg_name == pkg_name) {
                return true;
            }
            let elapsed_ms = start.elapsed().as_millis() as i64;
            if elapsed_ms > timeout_ms {
                return false;
            }
            let remaining = (timeout_ms - elapsed_ms).max(1) as u64;
            let (guard, _) = self
                .script_quit_changed
                .wait_timeout(scripts, Duration::from_millis(remaining))
                .unwrap_or_else(|e| e.into_inner());
            scripts = guard;
        }
    }

    pub fn wait_all_pkg_quit(&self, timeout_ms: i64) -> bool {
        let start = Instant::now();
        let mut scripts = match self.running_scripts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        loop {
            if scripts.is_empty() {
                return true;
            }
            let elapsed_ms = start.elapsed().as_millis() as i64;
            if elapsed_ms > timeout_ms {
                return false;
            }
            let remaining = (timeout_ms - elapsed_ms).max(1) as u64;
            let (guard, _) = self
                .script_quit_changed
                .wait_timeout(scripts, Duration::from_millis(remaining))
                .unwrap_or_else(|e| e.into_inner());
            scripts = guard;
        }
    }

    pub fn add_running_script_num(
        &self,
        pkg_name: &str,
        script_name: &str,
        script_type: &str,
    ) -> Option<u64> {
        // Keep the original lock order: running scripts, then the count,
        // followed by the process/package state checks.
        let mut scripts = match self.running_scripts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut count = match self.running_script_num.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let quit = match self.quit_flag.read() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        };
        let contains_pkg = match self.package_quit_flags.read() {
            Ok(g) => g.contains(pkg_name),
            Err(p) => p.into_inner().contains(pkg_name),
        };
        let loading_contains = if script_type != "init"
            && script_type != "输入流"
            && script_type != "延时"
            && script_type != "等待信号"
        {
            match self.loading_script_flags.read() {
                Ok(g) => g.contains(pkg_name),
                Err(p) => p.into_inner().contains(pkg_name),
            }
        } else {
            false
        };
        if quit || contains_pkg || loading_contains {
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

    pub fn running_script_info(&self) -> Vec<ScriptInfo> {
        match self.running_scripts.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    pub fn dec_running_script_num(&self, run_id: u64) {
        let mut scripts = match self.running_scripts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut count = match self.running_script_num.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(pos) = scripts.iter().position(|script| script.run_id == run_id) {
            scripts.remove(pos);
            if *count != 0 {
                *count -= 1;
            }
            self.script_quit_changed.notify_all();
        }
    }

    pub fn set_start_time(&self, start_time: String) {
        match self.start_time.lock() {
            Ok(mut g) => *g = start_time,
            Err(p) => *p.into_inner() = start_time,
        }
    }

    pub fn start_time(&self) -> String {
        match self.start_time.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
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
