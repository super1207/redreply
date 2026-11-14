use std::panic;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use cqapi::cq_add_log;
use rust_embed::RustEmbed;

mod botconn;
pub mod config_manager;
pub mod cqapi;
pub mod httpserver;
pub mod lifecycle;
pub mod resource_manager;
pub mod runtime_context;
pub mod service_hub;
pub mod session_store;

pub use cqapi::{cq_add_log_w, get_tmp_dir};
pub use redreply_common::{mytool, plugin_ports};
pub use runtime_context::runtime_context;

pub fn runtime() -> Arc<tokio::runtime::Runtime> {
    runtime_context().services.runtime()
}

static FRAMEWORK: OnceLock<redreply_bot::Framework> = OnceLock::new();
static FRAMEWORK_STARTER: OnceLock<Box<dyn Fn() -> Result<(), String> + Send + Sync>> =
    OnceLock::new();
static SHUTDOWN_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// 由插件注册退出收尾（置脚本退出标志、等待脚本、状态落盘）。
pub fn set_shutdown_hook(hook: impl Fn() + Send + Sync + 'static) {
    let _ = SHUTDOWN_HOOK.set(Box::new(hook));
}

/// 执行已注册的退出收尾；未注册时直接返回。
pub(crate) fn run_shutdown_hook() {
    if let Some(hook) = SHUTDOWN_HOOK.get() {
        hook();
    }
}

/// 由主程序（bin 包）注入框架启动器：组装适配器与插件并启动框架。
pub fn set_framework_starter<F>(starter: F)
where
    F: Fn() -> Result<(), String> + Send + Sync + 'static,
{
    let _ = FRAMEWORK_STARTER.set(Box::new(starter));
}

/// 由主程序在框架启动后注入实例，供宿主内部调用适配器与服务。
pub fn set_framework(framework: redreply_bot::Framework) {
    let _ = FRAMEWORK.set(framework);
}

/// 全局框架实例（启动后可用）。
pub fn framework() -> Option<&'static redreply_bot::Framework> {
    FRAMEWORK.get()
}

/// 框架服务注册表（TTS/浏览器等能力按服务名取用）。
pub fn framework_services() -> Option<&'static redreply_bot::ServiceRegistry> {
    FRAMEWORK.get().map(|framework| &**framework.services())
}

/// 心跳：关闭 auto_close 看门狗。
pub fn note_heartbeat() {
    runtime_context().services.set_auto_close(false);
}

#[derive(Clone, Debug)]
pub struct ScriptInfo {
    pub pkg_name: String,
    #[allow(dead_code)]
    pub script_name: String,
    /// 每次运行实例唯一 ID，避免同名脚本并发时 dec 删错条目
    pub run_id: u64,
}

pub struct InputStream {
    pub self_id: String,
    pub group_id: String,
    pub user_id: String,
    pub echo: String,
    pub stream_type: String,
    pub tx: Option<Arc<Mutex<std::sync::mpsc::Sender<String>>>>,
}

#[derive(Debug)]
pub struct ScriptRelatMsg {
    pub self_id: String,
    pub msg_id_vec: Vec<String>,
    pub create_time: u64,
}

pub const PAGING_UUID: &str = "5f4bf0da-1673-4e3f-8d7c-4932cc923504";

pub const RED_PY_DECODE: &str = r#"
import base64
import json

def __red_py_decode(input):
    data = json.loads(input) if isinstance(input, str) else input
    tp = data.get("type", "text")
    if tp == "text":
        return data.get("value", "")
    if tp == "bin":
        return base64.b64decode(data.get("value_b64", ""))
    if tp == "array":
        return [__red_py_decode(it) for it in data.get("items", [])]
    if tp == "object":
        return {k: __red_py_decode(v) for k, v in data.get("items", {}).items()}
    raise Exception("未知RedLang值类型: {}".format(tp))

def __to_red_data(input):
    if isinstance(input, str):
        return {"type": "text", "value": input}
    if isinstance(input, bytes):
        return {"type": "bin", "value_b64": base64.b64encode(input).decode("ascii")}
    if isinstance(input, bool):
        return {"type": "text", "value": "真" if input else "假"}
    if isinstance(input, int):
        return {"type": "text", "value": str(input)}
    if isinstance(input, float):
        return {"type": "text", "value": str(input)}
    if isinstance(input, (list, tuple)):
        return {"type": "array", "items": [__to_red_data(it) for it in input]}
    if isinstance(input, dict):
        return {"type": "object", "items": {str(k): __to_red_data(v) for k, v in input.items()}}
    if input is None:
        return {"type": "text", "value": ""}
    return {"type": "text", "value": str(input)}

def __to_red_type(input):
    return json.dumps(__to_red_data(input), ensure_ascii=False)
"#;

#[derive(RustEmbed)]
#[folder = "../../res/"]
#[prefix = "res/"]
pub struct Asset;

#[derive(RustEmbed)]
#[folder = "../../docs/"]
#[prefix = "docs/"]
pub struct AssetDoc;

/// 主 crate 的启动编排。具体配置、脚本、生命周期和资源操作位于对应模块。
pub fn initialize() -> i32 {
    let _ = cq_add_log(&format!(
        "欢迎使用`红色问答{}`,正在进行资源初始化...",
        resource_manager::get_version()
    ));
    // panic hook 内部绝不能再 panic（否则 double-panic 直接 abort，原始崩溃信息丢失）
    panic::set_hook(Box::new(|error| {
        let error_text = error.to_string();
        if let Ok(executable) = std::env::current_exe() {
            if let Some(dir) = executable.parent() {
                let path = dir.join("crash.log");
                let open_result = if path.exists() {
                    std::fs::OpenOptions::new().append(true).open(&path)
                } else {
                    std::fs::File::create(&path)
                };
                if let Ok(mut file) = open_result {
                    use std::io::Write as _;
                    let _ = file.write_all(format!("{error_text}\n").as_bytes());
                }
            }
        }
        let _ = cq_add_log_w(&error_text);
    }));

    config_manager::init_config();
    let _ = runtime_context();
    config_manager::init_global_var();

    if let Err(error) = resource_manager::release_file() {
        let _ = cq_add_log_w(&error.to_string());
    }
    // 框架接管 HTTP 服务与适配器连接；脚本引擎、Python 环境等由插件启动时初始化
    match FRAMEWORK_STARTER.get() {
        Some(starter) => {
            if let Err(error) = starter() {
                let _ = cq_add_log_w(&format!("框架启动失败:{error}"));
            }
        }
        None => {
            let _ = cq_add_log_w("未配置框架启动器，仅运行核心（无适配器与插件）");
        }
    }
    let _ = cq_add_log("资源初始化完成！");

    if let Ok(startup) = config_manager::startup_config() {
        if startup.auto_close {
            thread::spawn(|| {
                let _ = cq_add_log_w("自动退出已经开启，请每5秒提供一次心跳");
                loop {
                    runtime_context().services.set_auto_close(true);
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    if runtime_context().services.auto_close() {
                        let _ = cq_add_log_w("未及时提供心跳，程序退出！");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        std::process::exit(-1);
                    }
                }
            });
        }
    }
    0
}
