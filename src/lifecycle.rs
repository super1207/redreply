use crate::ScriptInfo;

pub fn pkg_can_run(pkg_name: &str, script_type: &str) -> bool {
    crate::runtime_context()
        .control
        .pkg_can_run(pkg_name, script_type)
}

pub fn wait_one_pkg_quit(pkg_name: &str, timeout_ms: i64) -> bool {
    crate::runtime_context()
        .control
        .wait_one_pkg_quit(pkg_name, timeout_ms)
}

pub fn wait_all_pkg_quit(timeout_ms: i64) -> bool {
    crate::runtime_context()
        .control
        .wait_all_pkg_quit(timeout_ms)
}

pub fn add_running_script_num(pkg_name: &str, script_name: &str, script_type: &str) -> Option<u64> {
    crate::runtime_context()
        .control
        .add_running_script_num(pkg_name, script_name, script_type)
}

pub fn get_running_script_info() -> Vec<ScriptInfo> {
    crate::runtime_context().control.running_script_info()
}

pub fn dec_running_script_num(run_id: u64) {
    crate::runtime_context()
        .control
        .dec_running_script_num(run_id);
}

pub fn set_start_time(start_time: String) {
    crate::runtime_context().control.set_start_time(start_time);
}

fn kill_all_sub_process() {
    let current_pid = std::process::id();
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::everything(),
    );
    // 直接子进程一律清理：脚本启动的辅助进程既可能是本程序自身
    // (浏览器/工具模式，见 browserbridge/lib.rs 以 current_exe 启动)，
    // 也可能是 python/ffmpeg 等外部程序，退出时都不应残留。
    // 原实现跳过同 exe 的子进程，导致这类辅助进程永远不被清理。
    for process in system.processes().values() {
        if let Some(parent_pid) = process.parent() {
            if parent_pid.as_u32() == current_pid {
                process.kill();
            }
        }
    }
}

pub fn wait_for_quit() -> ! {
    crate::runtime_context().control.set_quit(true);
    std::thread::spawn(|| {
        std::thread::sleep(core::time::Duration::from_secs(5));
        crate::cq_add_log_w("退出软件超时(5s)，强制退出!").unwrap();
        crate::cq_add_log_w(&format!("未退出脚本:{:?}", get_running_script_info())).unwrap();
        kill_all_sub_process();
        std::process::exit(-1);
    });

    while !wait_all_pkg_quit(5000) {}
    crate::status::flush_cache_to_db().unwrap();
    std::process::exit(0);
}
