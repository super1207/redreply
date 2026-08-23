use std::collections::HashMap;
use std::sync::Mutex;

use crate::redlang::RedValueData;

#[derive(Clone, Debug)]
pub(crate) struct OneTimeRunStruct {
    pub(crate) run_time: i64,
    pub(crate) pkg_name: String,
    pub(crate) flag: String,
    pub(crate) sub_data: String,
    pub(crate) data: HashMap<String, RedValueData>,
}

pub(crate) struct SchedulerStore {
    last_run_time: Mutex<Option<i64>>,
    one_time_runs: Mutex<Vec<OneTimeRunStruct>>,
}

impl SchedulerStore {
    pub(crate) fn new() -> Self {
        Self {
            last_run_time: Mutex::new(None),
            one_time_runs: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn cron_window(&self, now_time: i64) -> (i64, i64, Option<(bool, i64)>) {
        let mut last_run_time = self.last_run_time.lock().unwrap_or_else(|e| e.into_inner());
        let Some(previous) = *last_run_time else {
            *last_run_time = Some(now_time);
            return (now_time, now_time, None);
        };

        if now_time < previous {
            let difference = previous - now_time;
            if difference > 30 {
                *last_run_time = Some(now_time);
                (now_time, previous, Some((true, difference)))
            } else {
                *last_run_time = Some(previous);
                (previous, now_time, Some((false, difference)))
            }
        } else {
            *last_run_time = Some(now_time);
            (previous, now_time, None)
        }
    }

    pub(crate) fn add_one_time_run(&self, run: OneTimeRunStruct) {
        self.one_time_runs.lock().unwrap_or_else(|e| e.into_inner()).push(run);
    }

    /// 移除某包的所有延迟触发任务，避免包删除/重载后旧任务残留
    pub(crate) fn remove_pkg_runs(&self, pkg_name: &str) {
        self.one_time_runs
            .lock()
            .unwrap()
            .retain(|run| run.pkg_name != pkg_name);
    }

    pub(crate) fn take_due_runs(&self, now_time: i64) -> Vec<OneTimeRunStruct> {
        let mut runs = self.one_time_runs.lock().unwrap_or_else(|e| e.into_inner());
        let mut due = Vec::new();
        runs.retain(|run| {
            if run.run_time <= now_time {
                due.push(run.clone());
                false
            } else {
                true
            }
        });
        due
    }
}

impl Default for SchedulerStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{OneTimeRunStruct, SchedulerStore};
    use std::collections::HashMap;

    #[test]
    fn due_runs_are_removed_from_the_queue() {
        let store = SchedulerStore::new();
        store.add_one_time_run(OneTimeRunStruct {
            run_time: 10,
            pkg_name: "demo".to_owned(),
            flag: "flag".to_owned(),
            sub_data: String::new(),
            data: HashMap::new(),
        });
        assert_eq!(store.take_due_runs(9).len(), 0);
        assert_eq!(store.take_due_runs(10).len(), 1);
        assert_eq!(store.take_due_runs(10).len(), 0);
    }

    #[test]
    fn remove_pkg_runs_clears_only_matching_package() {
        let store = SchedulerStore::new();
        let run = |pkg: &str| OneTimeRunStruct {
            run_time: 10,
            pkg_name: pkg.to_owned(),
            flag: "flag".to_owned(),
            sub_data: String::new(),
            data: HashMap::new(),
        };
        store.add_one_time_run(run("demo"));
        store.add_one_time_run(run("other"));
        store.remove_pkg_runs("demo");
        let due = store.take_due_runs(10);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].pkg_name, "other");
    }
}
