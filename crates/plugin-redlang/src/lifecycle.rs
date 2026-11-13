//! 脚本生命周期：包/进程退出标志、运行中脚本计数、退出收尾。
//!
//! 进程级信号与强退看门狗在宿主 `redlang::lifecycle`；
//! 这里只处理脚本引擎自身的启停语义。

use crate::control_plane::control;
use redlang::ScriptInfo;

pub fn pkg_can_run(pkg_name: &str, script_type: &str) -> bool {
    control().pkg_can_run(pkg_name, script_type)
}

pub fn wait_one_pkg_quit(pkg_name: &str, timeout_ms: i64) -> bool {
    control().wait_one_pkg_quit(pkg_name, timeout_ms)
}

pub fn wait_all_pkg_quit(timeout_ms: i64) -> bool {
    control().wait_all_pkg_quit(timeout_ms)
}

pub fn add_running_script_num(pkg_name: &str, script_name: &str, script_type: &str) -> Option<u64> {
    control().add_running_script_num(pkg_name, script_name, script_type)
}

pub fn get_running_script_info() -> Vec<ScriptInfo> {
    control().running_script_info()
}

pub fn dec_running_script_num(run_id: u64) {
    control().dec_running_script_num(run_id);
}

pub fn set_start_time(start_time: String) {
    control().set_start_time(start_time);
}

/// 宿主退出前调用：置退出标志 → 等待脚本退出 → 状态计数落盘。
pub(crate) fn shutdown_engine() {
    control().set_quit(true);
    while !wait_all_pkg_quit(5000) {}
    if let Err(error) = crate::status::flush_cache_to_db() {
        let _ = redlang::cq_add_log_w(&format!("退出时缓存落盘失败:{error}"));
    }
}
