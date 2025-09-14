use std::collections::VecDeque;

use crate::{httpserver::add_ws_log, mqttclient::call_mqtt_remote, G_HISTORY_LOG, RT_PTR};

fn add_history_log(msg:&str) -> Result<(), Box<dyn std::error::Error>> {
    let mut lk = G_HISTORY_LOG.write()?;
    lk.push_back(msg.to_owned());
    if lk.len() > 50 {
        lk.pop_front();
    }
    Ok(())
}

pub fn get_history_log() -> VecDeque<String> {
    let lk_rst = G_HISTORY_LOG.read();
    if let Ok(lk) = lk_rst {
        let ret = &*lk;
        return ret.to_owned();
    }
    return VecDeque::new();
}

// 获取临时目录，绝对路径，末尾有'\',utf8编码
pub fn get_tmp_dir() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let lib_path = cq_get_app_directory1()? + "tmp";
    std::fs::create_dir_all(&lib_path)?;
    let path_str = format!("{}{}",lib_path,std::path::MAIN_SEPARATOR.to_string());
    Ok(path_str)
}

pub async fn get_tmp_dir_async() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(tokio::task::spawn_blocking(||get_tmp_dir()).await??)
}


// 获取插件的目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory1() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let curexedir = std::env::current_exe()?;
    let curdir = curexedir.parent().ok_or("无法获得当前可执行文件的父目录")?; 
    let path = curdir.join("plus_dir");
    std::fs::create_dir_all(&path)?;
    let path_str = format!("{}{}",path.to_str().unwrap(),std::path::MAIN_SEPARATOR.to_string());
    return Ok(crate::mytool::deal_path_str(&path_str).to_string());
}

pub async fn cq_get_app_directory1_async() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(tokio::task::spawn_blocking(||cq_get_app_directory1()).await??)
}

// 获取应用目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory2() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let curexedir = std::env::current_exe()?;
    let curdir = curexedir.parent().ok_or("无法获得当前可执行文件的父目录")?; 
    let path = curdir.join("plus_dir").join("default_pkg_dir");
    std::fs::create_dir_all(&path)?;
    let path_str = format!("{}{}",path.to_str().unwrap(),std::path::MAIN_SEPARATOR.to_string());
    return Ok(crate::mytool::deal_path_str(&path_str).to_string());
}

// 用于发送Onebot原始数据，返回OneBot原始数据，utf8编码
pub fn cq_call_api(platform:&str,self_id:&str,passive_id:&str,json_str: &str,remote_id:&str) -> String {
    let js_rst = serde_json::from_str(json_str);
    if let Err(err) = js_rst {
        return serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":format!("parse input json err:{:?}",err)
        }).to_string();
    }
    let mut js = js_rst.unwrap();

    if remote_id != "" {
        let ret_rsp = call_mqtt_remote(platform,self_id,passive_id,js,remote_id);
        if let Ok(ret) = ret_rsp {
            return ret.to_string();
        } else {
            let err = ret_rsp.err().unwrap();
            cq_add_log_w(&format!("调用mqtt远程失败:{:?}", err)).unwrap();
            return serde_json::json!({
                "retcode":-1,
                "status":"failed",
                "data":format!("call mqtt remote error:{:?}", err)
            }).to_string();
        }
    }
    let out_str = RT_PTR.block_on(async {
        let ret = crate::botconn::call_api(platform,self_id,passive_id,&mut js).await;
        if let Ok(ret) =  ret {
            return ret.to_string();
        } else {
            cq_add_log_w(&format!("调用api失败:{:?}",ret)).unwrap();
        }
        return serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":format!("call api error:{:?}",ret.err().unwrap())
        }).to_string();
    });
    out_str
}


fn cq_add_log_t(_log_level:i32,log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let now: chrono::DateTime<chrono::Local> = chrono::Local::now();
    let time_str = format!("{}",now.format("%Y-%m-%d %H:%M:%S%.3f").to_string());
    let log_msg_with_level;
    if _log_level == 0 {
        log::info!("{}",log_msg);
        log_msg_with_level = format!("Info:{}",log_msg);
    }else {
        log::warn!("{}",log_msg);
        log_msg_with_level = format!("Warn:{}",log_msg);
    }
    let web_log = format!("{time_str} {log_msg_with_level}");
    add_history_log(&web_log)?;
    add_ws_log(web_log);
    Ok(0)
}

// 打印日志，utf8编码
pub fn cq_add_log(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let out_str = log_msg.get(0..2000);
    if out_str.is_some() {
        Ok(cq_add_log_t(0,&format!("{}...",out_str.unwrap()))?)
    }else {
        Ok(cq_add_log_t(0,log_msg)?)
    }
}

// 打印日志，utf8编码
pub fn cq_add_log_w(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(cq_add_log_t(20,log_msg)?)
}