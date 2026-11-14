//! 进程级生命周期：信号处理、退出编排、子进程清理。
//!
//! 脚本引擎的退出收尾（置退出标志、等待脚本、状态落盘）由插件通过
//! [`set_shutdown_hook`] 注册，宿主退出前统一调用。

static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 进程是否正在退出（供后台线程提前收手）。
pub fn is_quitting() -> bool {
    QUITTING.load(std::sync::atomic::Ordering::SeqCst)
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
    QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(|| {
        std::thread::sleep(core::time::Duration::from_secs(5));
        // 强退看门狗内绝不能 unwrap：日志链路一旦出错，强退失效会导致进程永远退不出
        let _ = crate::cq_add_log_w("退出软件超时(5s)，强制退出!");
        kill_all_sub_process();
        std::process::exit(-1);
    });

    // 插件注册的收尾（等待脚本退出 + 缓存落盘）；失败也不能阻止退出
    crate::run_shutdown_hook();
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
