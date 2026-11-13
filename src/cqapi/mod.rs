use std::collections::VecDeque;

use crate::{httpserver::add_ws_log, runtime, runtime_context};
use redreply_common::plugin_ports::MqttServiceHandle;

fn add_history_log(msg: &str) -> Result<(), Box<dyn std::error::Error>> {
    runtime_context().session.add_history_log(msg);
    Ok(())
}

pub fn get_history_log() -> VecDeque<String> {
    runtime_context().session.history_log()
}

// 获取临时目录，绝对路径，末尾有'\',utf8编码
pub fn get_tmp_dir() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let lib_path = cq_get_app_directory1()? + "tmp";
    std::fs::create_dir_all(&lib_path)?;
    let path_str = format!("{}{}", lib_path, std::path::MAIN_SEPARATOR);
    Ok(path_str)
}

pub async fn get_tmp_dir_async() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    tokio::task::spawn_blocking(get_tmp_dir).await?
}

/// 数据根目录（默认是可执行文件同级的 plus_dir）。
///
/// 自动化测试或无头部署可通过 `REDREPLY_PLUS_DIR` 重定向，避免触碰真实数据。
fn app_plus_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(dir) = std::env::var("REDREPLY_PLUS_DIR") {
        if !dir.is_empty() {
            return Ok(std::path::PathBuf::from(dir));
        }
    }
    let curexedir = std::env::current_exe()?;
    let curdir = curexedir.parent().ok_or("无法获得当前可执行文件的父目录")?;
    Ok(curdir.join("plus_dir"))
}

// 获取插件的目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory1() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let path = app_plus_dir()?;
    std::fs::create_dir_all(&path)?;
    let path_str = format!(
        "{}{}",
        path.to_string_lossy(),
        std::path::MAIN_SEPARATOR
    );
    Ok(crate::mytool::deal_path_str(&path_str).to_string())
}

pub async fn cq_get_app_directory1_async(
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    tokio::task::spawn_blocking(cq_get_app_directory1).await?
}

// 获取应用目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory2() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let path = app_plus_dir()?.join("default_pkg_dir");
    std::fs::create_dir_all(&path)?;
    let path_str = format!(
        "{}{}",
        path.to_string_lossy(),
        std::path::MAIN_SEPARATOR
    );
    Ok(crate::mytool::deal_path_str(&path_str).to_string())
}

// 用于发送Onebot原始数据，返回OneBot原始数据，utf8编码
pub fn cq_call_api(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    json_str: &str,
    remote_id: &str,
) -> String {
    let js_rst = serde_json::from_str(json_str);
    if let Err(err) = js_rst {
        return serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":format!("parse input json err:{:?}",err)
        })
        .to_string();
    }
    let mut js = js_rst.unwrap();

    if !remote_id.is_empty() {
        // MQTT 由插件提供：未安装/未启用 mqtt 插件时按“没有MQTT Client”处理
        let service = crate::framework_services().and_then(|registry| {
            registry.get::<MqttServiceHandle>("mqtt")
        });
        let ret_rsp = match service {
            Some(service) => service.call_remote(platform, self_id, passive_id, js, remote_id),
            None => Err("没有MQTT Client".to_string()),
        };
        if let Ok(ret) = ret_rsp {
            return ret.to_string();
        } else {
            let err = ret_rsp.err().unwrap();
            let _ = cq_add_log_w(&format!("调用mqtt远程失败:{:?}", err));
            return serde_json::json!({
                "retcode":-1,
                "status":"failed",
                "data":format!("call mqtt remote error:{:?}", err)
            })
            .to_string();
        }
    }
    let fut = async {
        let ret = crate::botconn::call_api(platform, self_id, passive_id, &mut js).await;
        if let Ok(ret) = ret {
            return ret.to_string();
        } else {
            let _ = cq_add_log_w(&format!("调用api失败:{:?}", ret));
        }
        serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":format!("call api error:{:?}",ret.err().unwrap())
        })
        .to_string()
    };
    // 已在 runtime worker 上时禁止直接 block_on，否则会死锁
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        runtime().block_on(fut)
    }
}

fn cq_add_log_t(_log_level: i32, log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let now: chrono::DateTime<chrono::Local> = chrono::Local::now();
    let time_str = format!("{}", now.format("%Y-%m-%d %H:%M:%S%.3f"));
    let log_msg_with_level = if _log_level == 0 {
        log::info!("{}", log_msg);
        format!("Info:{}", log_msg)
    } else {
        log::warn!("{}", log_msg);
        format!("Warn:{}", log_msg)
    };
    let web_log = format!("{time_str} {log_msg_with_level}");
    add_history_log(&web_log)?;
    add_ws_log(web_log);
    Ok(0)
}

// 打印日志，utf8编码
pub fn cq_add_log(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    if let Some(out_str) = log_msg.get(0..2000) {
        Ok(cq_add_log_t(0, &format!("{}...", out_str))?)
    } else {
        Ok(cq_add_log_t(0, log_msg)?)
    }
}

// 打印日志，utf8编码
pub fn cq_add_log_w(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    cq_add_log_t(20, log_msg)
}
