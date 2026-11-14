//! RedLang 插件：解释器、宿主命令库、脚本管理、事件/定时/网络触发与脚本 API。
//!
//! - `core`：解释器本体（值/AST/求值器/命令注册表/同步原语/调度队列），不依赖宿主；
//! - `exfun` / `cqexfun` / `webexfun`：宿主标准命令库；
//! - `script_manager`：脚本包持久化与热重载；
//! - `cqevent` / `cronevent` / `initevent` / `httpevent`：脚本触发链路；
//! - `api`：脚本 HTTP API（`/redlang/api/*` 与 `/redlang/user/*`）。

pub mod core;

mod api;
mod bot_commands;
mod control_plane;
mod cqevent;
mod cqexfun;
mod cronevent;
mod exfun;
mod font;
mod httpevent;
mod initevent;
pub mod lifecycle;
mod native_args;
mod pyserver;
mod python_env;
mod script_manager;
mod status;
mod tts_commands;
mod webexfun;

#[cfg(test)]
mod tests;

use hyper::Method;
use redreply_bot::{
    AccessLevel, Event, EventAction, EventFilter, HttpError, MenuItem, Plugin, PluginContext,
    PluginMeta, Result, RouteAccess, StaticAssets,
};

/// 脚本引擎插件。
///
/// 这是普通插件层的能力：禁用后框架仍可收发消息，
/// 只是不再执行脚本、不再提供编辑台与脚本 API。
pub struct RedLangPlugin;

impl Plugin for RedLangPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("redlang", env!("CARGO_PKG_VERSION"))
            .description("红色问答脚本引擎（RedLang）：事件分发、脚本执行、定时任务")
            .provides("redlang")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        // 管理台菜单：进入脚本编辑器与日志
        ctx.register_menu(MenuItem::new("editor", "脚本管理", "/redlang/").order(10))?;
        ctx.register_menu(
            MenuItem::new("logs", "运行日志", "/redlang/watchlog.html").order(20),
        )?;

        // 编辑台静态页面：/redlang/ → <数据目录>/webui/
        ctx.mount_static(
            "/redlang",
            RouteAccess::Read,
            StaticAssets::from_dir(ctx.app_dir().join("webui")),
        );

        // 脚本 API：/redlang/api/<名> → 脚本处理器
        for method in [Method::GET, Method::POST] {
            ctx.route(
                method,
                "/redlang/api/*",
                RouteAccess::Read,
                |mut request, context| async move {
                    let rest = context.params.get("*").cloned().unwrap_or_default();
                    if rest.is_empty() {
                        return Err(HttpError::not_found());
                    }
                    // 改写路径时必须保留原始查询串（pkg_name 等参数都在查询里）
                    let query = request
                        .uri()
                        .query()
                        .map(|query| format!("?{query}"))
                        .unwrap_or_default();
                    if let Ok(uri) = format!("/{rest}{query}").parse::<hyper::Uri>() {
                        *request.uri_mut() = uri;
                    }
                    let (can_read, can_write) = access_flags(context.access);
                    crate::api::deal_api(request, can_write, can_read)
                        .await
                        .map_err(|error| HttpError::internal(error.to_string()))
                },
            );
        }

        // 网络触发：/redlang/user/<包名>/<自定义路径> → /user/...
        for method in [Method::GET, Method::POST] {
            ctx.route(
                method,
                "/redlang/user/*",
                RouteAccess::Read,
                |mut request, context| async move {
                    let rest = context.params.get("*").cloned().unwrap_or_default();
                    if rest.is_empty() {
                        return Err(HttpError::not_found());
                    }
                    // 网络触发的自定义路径同样可能带查询串，改写时一并保留
                    let query = request
                        .uri()
                        .query()
                        .map(|query| format!("?{query}"))
                        .unwrap_or_default();
                    if let Ok(uri) = format!("/user/{rest}{query}").parse::<hyper::Uri>() {
                        *request.uri_mut() = uri;
                    }
                    let (can_read, can_write) = access_flags(context.access);
                    crate::api::deal_api(request, can_write, can_read)
                        .await
                        .map_err(|error| HttpError::internal(error.to_string()))
                },
            );
        }

        // Python 常驻桥：/pyserver（python sidecar 反向连接）
        ctx.route(
            Method::GET,
            "/pyserver",
            RouteAccess::Write,
            |request, _context| async move { crate::pyserver::handle_ws_upgrade(request).await },
        );

        // 脚本管线：框架事件 → 脚本分发
        ctx.subscribe(0, EventFilter::new(), |event: &Event| {
            if let Err(error) = crate::cqevent::do_1207_event(&event.raw.to_string()) {
                let _ = redlang::cq_add_log_w(&format!("脚本事件处理失败：{error:?}"));
            }
            EventAction::Continue
        });
        Ok(())
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        // 向解释器核心注入宿主能力
        core::set_logger(|message| {
            let _ = redlang::cq_add_log_w(message);
        });
        core::set_cancellation(|pkg_name, script_type| {
            crate::lifecycle::pkg_can_run(pkg_name, script_type)
        });
        core::set_script_runner(|rl, code| {
            if let Err(error) = crate::cqevent::do_script(rl, code, "normal", false) {
                let _ = redlang::cq_add_log_w(&format!("{error}"));
            }
        });
        // MQTT 远端事件由 mqtt 插件直接发布到框架事件总线，本插件通过
        // register() 中的订阅统一接收，不再需要宿主转发钩子。
        // 宿主退出前收尾：置退出标志、等待脚本、状态落盘
        redlang::set_shutdown_hook(|| crate::lifecycle::shutdown_engine());

        // 命令表与脚本加载
        exfun::init_ex_fun_map();
        cqexfun::init_cq_ex_fun_map();
        webexfun::init_web_ex_fun_map();
        core::init_core_fun_map();
        if let Err(error) = script_manager::init_code() {
            let _ = redlang::cq_add_log_w(&format!("脚本加载失败：{error}"));
        }

        // 定时/延迟触发线程
        if let Err(error) = cronevent::do_cron_event() {
            let _ = redlang::cq_add_log_w(&format!("定时器启动失败：{error}"));
        }

        // Python 环境（venv + sidecar 监督重启），失败不阻塞插件启动
        std::thread::spawn(|| {
            if let Err(error) = crate::python_env::init_python() {
                let _ = redlang::cq_add_log_w(&error.to_string());
            }
        });

        ctx.logger().info("redlang", "RedLang 脚本引擎已接入事件总线");
        Ok(())
    }
}

fn access_flags(access: AccessLevel) -> (bool, bool) {
    let can_write = access == AccessLevel::Write;
    let can_read = matches!(access, AccessLevel::Read | AccessLevel::Write);
    (can_read, can_write)
}
