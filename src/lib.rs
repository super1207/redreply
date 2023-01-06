use std::collections::HashMap;
use std::env::current_exe; 
use std::ffi::CStr;
use std::fs;
use std::os::raw::c_char;
use std::panic;
use std::sync::Arc;
use std::sync::RwLock;
use httpserver::init_http_server;

use redlang::RedLang;
use serde_json;
use rust_embed::RustEmbed;

use cqapi::cq_add_log_w;
use cqapi::cq_get_app_directory;


mod cqapi;
mod cqevent;
mod redlang;
mod mytool;
mod initevent;
mod cronevent;
pub mod httpserver;


#[macro_use]
extern crate lazy_static; 

lazy_static! {
    // 用于记录加载的脚本
    pub static ref G_SCRIPT:RwLock<serde_json::Value> = RwLock::new(serde_json::json!([]));
    // 用于类型UUID
    pub static ref REDLANG_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于分页命令
    pub static ref PAGING_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于清空命令
    pub static ref CLEAR_UUID:String = uuid::Uuid::new_v4().to_string();
    // 用于记录常量:包名-常量名-常量值
    pub static ref G_CONST_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于撤回消息
    pub static ref G_MSG_ID_MAP:RwLock<HashMap<String,Vec<String>>> = RwLock::new(HashMap::new());
    // 用于记录自定义的命令
    pub static ref G_CMD_MAP:RwLock<HashMap<String, String>> = RwLock::new(HashMap::new());
    // 用于记录命令
    pub static ref G_CMD_FUN_MAP:RwLock<HashMap<String, fn(&mut RedLang,&[String]) -> Result<Option<String>, Box<dyn std::error::Error>>>> = RwLock::new(HashMap::new());
    // 异步事件循环
    pub static ref  RT_PTR:Arc<tokio::runtime::Runtime> = Arc::new(tokio::runtime::Runtime::new().unwrap());
}



#[derive(RustEmbed)]
#[folder = "res/"]
#[prefix = "res/"]
pub struct Asset;


// 这是插件第一个被调用的函数，不要在这里调用任何CQ的API,也不要在此处阻塞
#[no_mangle]
pub extern "system" fn Initialize(ac: i32) -> i32 {
    cqapi::set_auth_code(ac);
    panic::set_hook(Box::new(|e| {
        cq_add_log_w(e.to_string().as_str()).unwrap();
    }));
    redlang::cqexfun::init_cq_ex_fun_map();
    redlang::exfun::init_ex_fun_map();
    // 要使CQ正常启动，请一定返回0
    return 0;
}

pub fn read_config() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let script_path = cq_get_app_directory()? + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{
        fs::write(script_path.clone(), "{\"web_port\":1207,\"web_host\":\"127.0.0.1\"}")?;
    }
    let script = fs::read_to_string(script_path)?;
    Ok(serde_json::from_str(&script)?)
}

pub fn init_code() -> Result<(), Box<dyn std::error::Error>>{
    let script_path = cq_get_app_directory()? + "script.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{
        fs::write(script_path, "[]")?;
        return Ok(());
    }
    let script = fs::read_to_string(script_path)?;
    let mut wk = G_SCRIPT.write()?;
    (*wk) = serde_json::from_str(&script)?;
    Ok(())
}

pub fn save_code(contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut wk = G_SCRIPT.write()?;
    let js = serde_json::from_str(contents)?;
    fs::write(cq_get_app_directory()? + "script.json", contents).unwrap();
    (*wk) = js;
    Ok(())
}

pub fn read_code() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    Ok((*wk).clone())
}

pub fn release_file() -> Result<(), Box<dyn std::error::Error>> {
    let err = "get asset err";
    fs::create_dir_all(cq_get_app_directory().unwrap() + "toc\\css\\zTreeStyle\\img\\diy")?;
    fs::create_dir_all(cq_get_app_directory().unwrap() + "toc\\js")?;
    fs::create_dir_all(cq_get_app_directory().unwrap() + "toc\\style")?;
    for it in Asset::iter() {
        if it.to_string() == "res/sciter.dll" {
            let pth = current_exe()?.parent().ok_or(err)?.join("bin").join("sciter.dll");
            if !pth.exists() {
                let index_html = Asset::get("res/sciter.dll").ok_or(err)?;
                fs::write(pth, index_html.data)?;
            }
        }else {
            let file = Asset::get(&it.to_string()).ok_or(err)?;
            fs::write(cq_get_app_directory().unwrap() + it.to_string().get(4..).unwrap_or_default(), file.data)?;
        }
    } 
    Ok(())
}

// 插件被MiraiCQ启用后就会调用此函数，这时，已经可以调用不需要和onebot通讯的API了
#[no_mangle]
pub extern "system" fn _eventEnable() -> i32 {
    if let Err(err) = release_file(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = init_http_server(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = init_code(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = initevent::do_init_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = cronevent::do_cron_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    return 0;
}

// 1207号事件，用于接收OneBotv11格式的原始数据，utf8编码
#[no_mangle]
pub extern "system" fn _event1207(msg: *const c_char) -> i32 {
    let onebot_json: String = unsafe {
        CStr::from_ptr(msg)
            .to_str()
            .expect("get error msg ptr from event1207")
            .to_string()
    };

    if let Err(e) = cqevent::do_1207_event(onebot_json.as_str()) {
        cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
    }
    return 0;
}

// menu事件
#[no_mangle]
pub extern "system" fn _menuA() -> i32 {
    if let Err(e) = cqevent::do_menu_event() {
        cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
    }
    return 0;
}

