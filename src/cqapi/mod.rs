use crate::{RT_PTR, httpserver::add_ws_log};


// 获取插件的目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory1() -> Result<String, Box<dyn std::error::Error>> {
    let curexedir = std::env::current_exe()?;
    let curdir = curexedir.parent().ok_or("无法获得当前可执行文件的父目录")?; 
    let path = curdir.join("plus_dir");
    std::fs::create_dir_all(&path)?;
    return Ok(format!("{}{}",path.to_str().unwrap(),std::path::MAIN_SEPARATOR.to_string()))
}

// 获取应用目录，绝对路径，末尾有'\',utf8编码
pub fn cq_get_app_directory2() -> Result<String, Box<dyn std::error::Error>> {
    let curexedir = std::env::current_exe()?;
    let curdir = curexedir.parent().ok_or("无法获得当前可执行文件的父目录")?; 
    let path = curdir.join("plus_dir").join("default_pkg_dir");
    std::fs::create_dir_all(&path)?;
    return Ok(format!("{}{}",path.to_str().unwrap(),std::path::MAIN_SEPARATOR.to_string()))
}

// 用于发送Onebot原始数据，返回OneBot原始数据，utf8编码
pub fn cq_call_api(self_id:&str,json_str: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut js:serde_json::Value = serde_json::from_str(json_str)?;
    let out_str = RT_PTR.block_on(async {
        let ret = crate::botconn::call_api(self_id,&mut js).await;
        if let Ok(ret) =  ret {
            return ret.to_string();
        }
        return "".to_string();
    });
    Ok(out_str)
}


fn cq_add_log_t(_log_level:i32,log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    if _log_level == 0 {
        
        log::info!("{}",log_msg);
        add_ws_log(format!("Info:{}",log_msg));
    }else {
        log::warn!("{}",log_msg);
        add_ws_log(format!("Warn:{}",log_msg));
    }
    Ok(0)
}

// 打印日志，utf8编码
pub fn cq_add_log(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(cq_add_log_t(0,log_msg)?)
}

// 打印日志，utf8编码
pub fn cq_add_log_w(log_msg: &str) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(cq_add_log_t(20,log_msg)?)
}