//! 主程序的框架组装层：创建 `redreply-bot` 框架实例，
//! 注册适配器工厂与插件，并注册框架自有 HTTP 端点。

use std::sync::Arc;
use std::time::Duration;

use hyper::Method;
use plugin_browser::BrowserPlugin;
use plugin_mqtt::MqttPlugin;
use plugin_onebot::OneBotPlugin;
use plugin_pgsql::PgsqlPlugin;
use plugin_redlang::RedLangPlugin;
use plugin_tianwang::ChatGptPlugin;
use plugin_tts::TtsPlugin;
use redreply_adapter::framework_bridge;
use redreply_bot::{AdapterEntry, AuthConfig, Framework, HttpError, RouteAccess, StaticAssets};

/// 构建并启动框架（HTTP + 适配器 + 插件）。
pub fn start() -> Result<(), String> {
    if redlang::framework().is_some() {
        return Ok(());
    }
    let startup = redlang::config_manager::startup_config().map_err(|error| error.to_string())?;
    let app_dir = std::path::PathBuf::from(
        redlang::cqapi::cq_get_app_directory1().map_err(|error| error.to_string())?,
    );
    let webui_dir = app_dir.join("webui");
    let auth = AuthConfig::new(
        redlang::config_manager::read_web_password().map_err(|error| error.to_string())?,
        redlang::config_manager::read_readonly_web_password().map_err(|error| error.to_string())?,
    );

    let mut builder = Framework::builder()
        .runtime(redlang::runtime().handle().clone())
        .app_dir(app_dir)
        .auth(auth)
        .version(redlang::resource_manager::get_version().trim().to_string())
        .all_to_silk(Arc::new(|input: &[u8]| {
            redlang::mytool::all_to_silk::all_to_silk(input).map_err(|error| error.to_string())
        }))
        .http_host_port(&startup.web_host, startup.web_port as u16)
        .map_err(|error| error.to_string())?
        // 适配器（框架特殊层）
        .adapter(framework_bridge::onebot11_factory())
        .adapter(framework_bridge::onebot115_factory())
        .adapter(framework_bridge::satoriv1_factory())
        .adapter(framework_bridge::qqguild_private_factory())
        .adapter(framework_bridge::qqguild_public_factory())
        .adapter(framework_bridge::kook_factory())
        .adapter(framework_bridge::email_factory())
        .adapter(framework_bridge::telegram_factory())
        .adapter(framework_bridge::yunhu_factory())
        // 普通插件（每个插件一个 crate）
        .plugin(RedLangPlugin)
        .plugin(TtsPlugin)
        .plugin(BrowserPlugin)
        .plugin(MqttPlugin)
        .plugin(PgsqlPlugin)
        .plugin(OneBotPlugin)
        .plugin(ChatGptPlugin);
    for name in redlang::config_manager::disabled_plugins() {
        builder = builder.disable_plugin(name);
    }
    let framework = builder.build().map_err(|error| error.to_string())?;
    register_framework_routes(&framework, &webui_dir);

    // 连接配置：启动时同步 config.json；管理台修改会写回 config.json
    framework.adapters().set_entries(read_adapter_entries()?);
    framework
        .adapters()
        .set_entries_persist_hook(Arc::new(|entries: &[AdapterEntry]| {
            let mapped: Vec<(String, bool)> = entries
                .iter()
                .map(|entry| (entry.url.clone(), entry.enabled))
                .collect();
            if let Err(error) = redlang::config_manager::set_adapter_entries(&mapped) {
                let _ = redlang::cq_add_log_w(&format!("保存连接配置失败：{error}"));
            }
        }));
    framework
        .plugins()
        .set_enabled_persist_hook(Arc::new(|name: &str, enabled: bool| {
            if let Err(error) = redlang::config_manager::set_plugin_enabled(name, enabled) {
                let _ = redlang::cq_add_log_w(&format!("保存插件状态失败：{error}"));
            }
        }));

    framework.start().map_err(|error| error.to_string())?;
    spawn_url_sync();

    if !startup.not_open_browser {
        let _ = redlang::resource_manager::show_ctrl_web();
    }

    redlang::set_framework(framework);
    Ok(())
}

/// 框架自有 HTTP 端点：心跳、图标、帮助文档、日志与 Python WebSocket。
///
/// redlang 的页面与 API 由 redlang 插件挂载在 `/redlang/*`，旧根路径不再提供。
fn register_framework_routes(framework: &Framework, webui_dir: &std::path::Path) {
    framework.http().route(
        Method::GET,
        "/heartbeat",
        RouteAccess::Public,
        |_request, _context| async move {
            redlang::note_heartbeat();
            Ok(redreply_bot::http::text_response(
                hyper::StatusCode::OK,
                "ok",
            ))
        },
    );

    framework.http().route(
        Method::GET,
        "/favicon.ico",
        RouteAccess::Public,
        |_request, _context| async move {
            match redlang::Asset::get("res/favicon.ico") {
                Some(file) => Ok(redreply_bot::http::bytes_response(
                    hyper::StatusCode::OK,
                    "image/x-icon",
                    file.data,
                    Some("max-age=300"),
                )),
                None => Err(HttpError::not_found()),
            }
        },
    );

    let docs_assets = StaticAssets::from_dir(webui_dir.join("docs"));
    framework
        .http()
        .mount_static("/docs", RouteAccess::Public, docs_assets);

    framework.http().route(
        Method::GET,
        "/watch_log",
        RouteAccess::Public,
        |request, context| async move {
            redlang::httpserver::legacy_handle(request, context.is_local)
                .await
                .map_err(|error| HttpError::internal(error.to_string()))
        },
    );
}

/// 读取 config.json 的全部连接条目（含停用）。
fn read_adapter_entries() -> Result<Vec<AdapterEntry>, String> {
    Ok(redlang::config_manager::adapter_entries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|(url, enabled)| AdapterEntry::new(url, enabled))
        .collect())
}

/// 每 5 秒跟随 config.json 的连接配置变化（手动编辑配置时无需重启）。
fn spawn_url_sync() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(5));
        let Some(framework) = redlang::framework() else {
            break;
        };
        let Ok(entries) = read_adapter_entries() else {
            continue;
        };
        if framework.adapters().entries() != entries {
            framework.adapters().set_entries(entries);
        }
    });
}
