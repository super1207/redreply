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
        // 强退看门狗内绝不能 unwrap：日志链路一旦出错，强退失效会导致进程永远退不出
        let _ = crate::cq_add_log_w("退出软件超时(5s)，强制退出!");
        let _ = crate::cq_add_log_w(&format!("未退出脚本:{:?}", get_running_script_info()));
        kill_all_sub_process();
        std::process::exit(-1);
    });

    while !wait_all_pkg_quit(5000) {}
    // flush 失败也不能阻止退出（缓存丢失可接受，进程卡死不可接受）
    if let Err(e) = crate::status::flush_cache_to_db() {
        let _ = crate::cq_add_log_w(&format!("退出时缓存落盘失败:{e}"));
    }
    std::process::exit(0);
}

/// 注册系统信号处理器：Ctrl+C / SIGTERM（docker stop）也能走优雅退出流程。
pub fn register_signal_handler() {
    std::thread::Builder::new()
        .name("signal-handler".into())
        .spawn(|| {
            let rt_result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let rt = match rt_result {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = crate::cq_add_log_w(&format!("信号监听 runtime 创建失败:{e}"));
                    return;
                }
            };
            rt.block_on(async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = match signal(SignalKind::terminate()) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let mut sigint = match signal(SignalKind::interrupt()) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    tokio::select! {
                        _ = sigterm.recv() => {},
                        _ = sigint.recv() => {},
                    }
                }
                #[cfg(not(unix))]
                {
                    // Windows 下 tokio 内部注册 SetConsoleCtrlHandler，
                    // GUI 子系统无控制台时不会触发，保持安静即可。
                    let _ = tokio::signal::ctrl_c().await;
                }
            });
            let _ = crate::cq_add_log_w("收到系统退出信号，开始优雅退出...");
            wait_for_quit();
        })
        .map(|_| ())
        .ok(); // 线程创建失败只影响信号退出能力，不影响启动
}
