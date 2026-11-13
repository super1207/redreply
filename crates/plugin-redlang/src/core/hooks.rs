//! 宿主钩子：解释器核心向宿主暴露的最小回调集合。
//!
//! 核心 crate 不依赖宿主；宿主在启动时注入日志、取消检查与脚本运行器。

use std::sync::OnceLock;

use super::RedLang;

type LogFn = Box<dyn Fn(&str) + Send + Sync>;
type CancelFn = Box<dyn Fn(&str, &str) -> bool + Send + Sync>;
type ScriptRunnerFn = Box<dyn Fn(&mut RedLang, &str) + Send + Sync>;

static LOGGER: OnceLock<LogFn> = OnceLock::new();
static CANCELLATION: OnceLock<CancelFn> = OnceLock::new();
static SCRIPT_RUNNER: OnceLock<ScriptRunnerFn> = OnceLock::new();

/// 注入日志回调（宿主通常接到 cq_add_log_w）。
pub fn set_logger(logger: impl Fn(&str) + Send + Sync + 'static) {
    let _ = LOGGER.set(Box::new(logger));
}

/// 注入取消检查回调（宿主通常接到 lifecycle::pkg_can_run）。
pub fn set_cancellation(check: impl Fn(&str, &str) -> bool + Send + Sync + 'static) {
    let _ = CANCELLATION.set(Box::new(check));
}

/// 注入后台脚本运行器（宿主通常接到 cqevent::do_script）。
pub fn set_script_runner(runner: impl Fn(&mut RedLang, &str) + Send + Sync + 'static) {
    let _ = SCRIPT_RUNNER.set(Box::new(runner));
}

pub(crate) fn log_warning(message: &str) {
    if let Some(logger) = LOGGER.get() {
        logger(message);
    }
}

/// 脚本是否仍被允许运行（进程/包退出时会被取消）。
pub(crate) fn pkg_can_run(pkg_name: &str, script_type: &str) -> bool {
    match CANCELLATION.get() {
        Some(check) => check(pkg_name, script_type),
        None => true,
    }
}

/// 在后台线程中执行一段脚本（由宿主提供的运行器实现）。
pub(crate) fn run_script(rl: &mut RedLang, code: &str) {
    match SCRIPT_RUNNER.get() {
        Some(runner) => runner(rl, code),
        None => log_warning("后台运行脚本失败：宿主未配置脚本运行器"),
    }
}
