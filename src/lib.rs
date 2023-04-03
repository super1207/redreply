use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use cqapi::cq_get_app_directory2;
use httpserver::init_http_server;

use redlang::RedLang;
use serde_json;
use rust_embed::RustEmbed;

use cqapi::cq_add_log_w;
use cqapi::cq_get_app_directory1;


mod cqapi;
mod cqevent;
mod redlang;
mod mytool;
mod initevent;
mod cronevent;
mod botconn;
mod httpserver;
mod test;

#[macro_use]
extern crate lazy_static; 

#[derive(Clone,Debug)]
pub struct ScriptInfo {
    pkg_name:String,
    script_name:String
}

pub struct InputStream {
    pub group_id:String,
    pub user_id:String,
    pub guild_id:String,
    pub channel_id:String,
    pub echo:String,
    pub stream_type:String,
    pub tx:Option<Arc<Mutex<std::sync::mpsc::Sender<String>>>>
}

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
    pub static ref G_MSG_ID_MAP:RwLock<HashMap<String,VecDeque<String>>> = RwLock::new(HashMap::new());
    // 用于记录自定义的命令
    pub static ref G_CMD_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于记录命令
    pub static ref G_CMD_FUN_MAP:RwLock<HashMap<String, fn(&mut RedLang,&[String]) -> Result<Option<String>, Box<dyn std::error::Error>>>> = RwLock::new(HashMap::new());
    // 异步事件循环
    pub static ref  RT_PTR:Arc<tokio::runtime::Runtime> = Arc::new(tokio::runtime::Runtime::new().unwrap());
    // 退出标记
    pub static ref G_QUIT_FLAG:RwLock<bool> = RwLock::new(false);
    // 记录正在运行的脚本数量（用于退出）
    pub static ref G_RUNNING_SCRIPT_NUM:RwLock<usize> = RwLock::new(0usize);
    // 记录正在运行的脚本名字
    pub static ref G_RUNNING_SCRIPT:RwLock<Vec<ScriptInfo>> = RwLock::new(vec![]);
    // 输入流记录
    pub static ref G_INPUTSTREAM_VEC:RwLock<Vec<InputStream>> = RwLock::new(vec![]);
}



#[derive(RustEmbed)]
#[folder = "res/"]
#[prefix = "res/"]
pub struct Asset;


#[derive(RustEmbed)]
#[folder = "doc/"]
#[prefix = "doc/"]
pub struct AssetDoc;

pub fn wait_for_quit() -> ! {
    (*G_QUIT_FLAG.write().unwrap()) = true;
    let _foo = std::thread::spawn(||{
        std::thread::sleep(core::time::Duration::from_secs(5));
        cq_add_log_w("退出软件超时(5s)，强制退出!").unwrap();
        let running_scripts = get_running_script_info();
        cq_add_log_w(&format!("未退出脚本:{:?}",running_scripts)).unwrap();
        std::process::exit(-1);
    });
    loop {
        {
            if (*G_RUNNING_SCRIPT_NUM.read().unwrap()) == 0 {
                break;
            }
        }
        std::thread::sleep(core::time::Duration::from_millis(1));
    }
    std::process::exit(0);
}

pub fn add_running_script_num(pkg_name:&str,script_name:&str) -> bool {
    if *G_QUIT_FLAG.read().unwrap() == true {
        return false;
    }
    let mut lk = G_RUNNING_SCRIPT_NUM.write().unwrap();
    (*lk) += 1;
    let mut lk = G_RUNNING_SCRIPT.write().unwrap();
    lk.push(ScriptInfo {
        pkg_name: pkg_name.to_owned(),
        script_name: script_name.to_owned()
    });
    return true;
}

pub fn get_running_script_info() -> Vec<ScriptInfo> {
    let lk = G_RUNNING_SCRIPT.read().unwrap();
    let mut ret_vec:Vec<ScriptInfo> = vec![];
    for i in 0..lk.len() {
        let script_info = lk.get(i).unwrap();
        ret_vec.push((*script_info).clone());
    }
    return ret_vec;
}

pub fn dec_running_script_num(pkg_name:&str,script_name:&str) {
    let mut lk = G_RUNNING_SCRIPT_NUM.write().unwrap();
    if (*lk) != 0 {
        (*lk) -= 1;
    }
    let mut lk = G_RUNNING_SCRIPT.write().unwrap();
    let mut pos = 0;
    let mut isfind = false;
    for i in 0..lk.len() {
        let script_info = lk.get(i).unwrap();
        if script_info.script_name == script_name && pkg_name == script_info.pkg_name {
            pos = i;
            isfind = true;
            break;
        }
    }
    if isfind {
        lk.remove(pos);
    }
}


// 这是插件第一个被调用的函数，不要在这里调用任何CQ的API,也不要在此处阻塞
pub fn initialize() -> i32 {
    panic::set_hook(Box::new(|e| {
        cq_add_log_w(e.to_string().as_str()).unwrap();
    }));
    redlang::cqexfun::init_cq_ex_fun_map();
    redlang::exfun::init_ex_fun_map();
    redlang::init_core_fun_map();

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
    if let Err(err) = botconn::do_conn_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    if let Err(err) = cronevent::do_cron_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    return 0;
}

pub fn read_config() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let script_path = cq_get_app_directory1()? + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{
        fs::write(script_path.clone(), "{\"web_port\":1207,\"web_host\":\"127.0.0.1\",\"ws_urls\":[],\"not_open_browser\":false}")?;
    }
    let script = fs::read_to_string(script_path)?;
    Ok(serde_json::from_str(&script)?)
}

pub fn set_ws_urls(ws_urls:serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = read_config()?;
    config["ws_urls"] = ws_urls;
    let script_path = cq_get_app_directory1()? + "config.json";
    fs::write(script_path,config.to_string())?;
    Ok(())
}

pub fn get_all_pkg_name() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let plus_dir_str = cq_get_app_directory1()?;
    let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
    std::fs::create_dir_all(&pkg_dir)?;
    let dirs = fs::read_dir(&pkg_dir)?;
    let mut pkg_names:Vec<String> = vec![];
    for dir in dirs {
        let path = dir?.path();
        if path.is_dir() {
            pkg_names.push(format!("{}",path.file_name().unwrap().to_string_lossy()));
        }
    }
    if pkg_names.contains(&"默认包".to_string()) {
        // 这里强制退出程序
        return Err(RedLang::make_err("附加包的包名不可以为`默认包`!")).unwrap();
    }
    Ok(pkg_names)
}

fn get_all_pkg_code() -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let plus_dir_str = cq_get_app_directory1()?;
    let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
    let pkg_names = get_all_pkg_name()?;
    let mut arr_val:Vec<serde_json::Value> = vec![];
    for it in &pkg_names {
        let script_path = pkg_dir.join(&it).join("script.json");
        {
            // 判断文件是否存在
            let mut is_file_exists = false;
            if fs::metadata(script_path.clone()).is_ok() {
                if fs::metadata(script_path.clone())?.is_file(){
                    is_file_exists = true;
                }
            }
            // 不存在就创建文件
            if !is_file_exists{
                fs::write(script_path.clone(), "[]")?;
            }
        }
        
        let script = fs::read_to_string(script_path)?;
        let mut pkg_script_vec:Vec<serde_json::Value> = serde_json::from_str(&script)?;
        for js in &mut pkg_script_vec {
            if let Some(obj) = js.as_object_mut() {
                obj.insert("pkg_name".to_string(),serde_json::Value::String(it.to_string()));
                arr_val.push(serde_json::Value::Object(obj.clone()));
            }
        }
    }
    Ok(arr_val)
}

pub fn init_code() -> Result<(), Box<dyn std::error::Error>>{
    let script_path = cq_get_app_directory2()? + "script.json";
    // 判断文件是否存在
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    // 不存在就创建文件
    if !is_file_exists{
        fs::write(script_path.clone(), "[]")?;
    }

    // 获取默认包代码
    let script = fs::read_to_string(script_path)?;
    let mut arr_val:Vec<serde_json::Value> = serde_json::from_str(&script)?;

    // 获取所有三方包代码
    let pkg_codes = get_all_pkg_code()?;
    for it in pkg_codes {
        arr_val.push(it);
    }

    // 保存代码到内存
    let mut wk = G_SCRIPT.write()?;
    (*wk) = serde_json::Value::Array(arr_val);
    Ok(())
}

pub fn save_code(contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut code_map:HashMap<String,Vec<serde_json::Value>> = HashMap::new();
    for it in get_all_pkg_name()? {
        code_map.insert(it, vec![]);
    }
    code_map.insert("".to_string(), vec![]);
    let js:Vec<serde_json::Value> = serde_json::from_str(contents)?;
    for it in &js {
        let pkg_name_opt = it.as_object().ok_or("脚本格式错误")?.get("pkg_name");
        let mut pkg_name_str = "";
        if let Some(pkg_name) = pkg_name_opt {
            pkg_name_str = pkg_name.as_str().unwrap_or_default();
        }
        if !code_map.contains_key(pkg_name_str) {
            code_map.insert(pkg_name_str.to_owned(), vec![]);
        }
        let mut it_t = it.to_owned();
        if let Some(k) = it_t.as_object_mut() {
            k.remove("pkg_name");
        }
        code_map.get_mut(pkg_name_str).unwrap().push(it_t);
    }
    {
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let mut wk = G_SCRIPT.write()?;
        for (pkg_name,code) in code_map {
            let cont = serde_json::Value::Array(code).to_string();
            if pkg_name == "" {
                fs::write(cq_get_app_directory2()? + "script.json", cont).unwrap();
            }else {
                let script_path = pkg_dir.join(pkg_name).join("script.json");
                fs::write(script_path, cont).unwrap();
            }
        }
        
        
        (*wk) = serde_json::Value::Array(js);
    }
    if let Err(err) = crate::initevent::do_init_event(){
        cq_add_log_w(&format!("can't call init evt:{}",err)).unwrap();
    }
    Ok(())
}

pub fn read_code() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    Ok((*wk).clone())
}

pub fn release_file() -> Result<(), Box<dyn std::error::Error>> {
    let sep = std::path::MAIN_SEPARATOR;
    let err = "get asset err";
    fs::create_dir_all(cq_get_app_directory1().unwrap() + "webui")?;
    for it in Asset::iter() {
        let file = Asset::get(&it.to_string()).ok_or(err)?;
        fs::write(cq_get_app_directory1().unwrap() + "webui" +  &sep.to_string() +  it.to_string().get(4..).unwrap_or_default(), file.data)?;
    } 
    for it in AssetDoc::iter() {
        let file = AssetDoc::get(&it.to_string()).ok_or(err)?;
        fs::write(cq_get_app_directory1().unwrap() + "webui" + &sep.to_string() + it.to_string().get(4..).unwrap_or_default(), file.data)?;
    } 
    Ok(())
}


pub fn get_version() -> String{
    return "0.0.42".to_string();
}