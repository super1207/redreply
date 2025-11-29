use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fs;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use std::time::SystemTime;
use cqapi::cq_add_log;
use cqapi::cq_get_app_directory2;
use encoding::Encoding;
use httpserver::init_http_server;

use md5::Digest;
use md5::Md5;
use mytool::read_json_str;
use path_clean::PathClean;
use redlang::RedLang;
use serde_json;
use rust_embed::RustEmbed;

pub use cqapi::cq_add_log_w;
use cqapi::cq_get_app_directory1;

use crate::initevent::do_gobal_init_event;


mod cqapi;
mod cqevent;
mod redlang;
mod mytool;
mod initevent;
mod cronevent;
mod botconn;
mod httpserver;
mod httpevent;
mod pyserver;
mod test;
mod pluscenter;
mod onebot11s;
mod status;
mod mqttclient;
mod pgsql;

#[macro_use]
extern crate lazy_static; 

#[derive(Clone,Debug)]
pub struct ScriptInfo {
    pkg_name:String,
    script_name:String
}

pub struct InputStream {
    pub self_id:String,
    pub group_id:String,
    pub user_id:String,
    pub echo:String,
    pub stream_type:String,
    pub tx:Option<Arc<Mutex<std::sync::mpsc::Sender<String>>>>
}

#[derive(Debug)]
pub struct ScriptRelatMsg {
    pub self_id:String,
    pub msg_id_vec:Vec<String>,
    pub create_time:u64
}

lazy_static! {
    // 用于记录加载的脚本(x)
    pub static ref G_SCRIPT:RwLock<serde_json::Value> = RwLock::new(serde_json::json!([]));
    // 用于记录加载的包名(x)
    pub static ref G_PKG_NAME:RwLock<HashSet<String>> = RwLock::new(HashSet::new());
    // 用于类型UUID
    pub static ref REDLANG_UUID:String = "12331549-6D26-68A5-E192-5EBE9A6EB998".to_owned();
    // 用于分页命令
    pub static ref PAGING_UUID:String = "5f4bf0da-1673-4e3f-8d7c-4932cc923504".to_owned();
    // 用于清空命令
    pub static ref CLEAR_UUID:String = "a1e72c64-4d18-4529-bc19-e61c5a836e8c".to_owned();
    // 用于记录常量:包名-常量名-常量值(x)
    pub static ref G_CONST_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于记录信号:uuid-包名-信号名-信号值
    pub static ref G_SINGAL_ARR:RwLock<Vec<(String, String,String,Option<String>)>> = RwLock::new(vec![]);
    // 用于记录临时常量:包名-常量名-常量值-过期时间(x)
    pub static ref G_TEMP_CONST_MAP:RwLock<HashMap<String,HashMap<String, (String, u128)>>> = RwLock::new(HashMap::new());
    // 用于撤回消息 key:self_id+group_id  value:user_id + message_id
    pub static ref G_MSG_ID_MAP:RwLock<HashMap<String,VecDeque<(String,String)>> > = RwLock::new(HashMap::new());
    // 用于记录自定义的命令(x)
    pub static ref G_CMD_MAP:RwLock<HashMap<String,HashMap<String, String>>> = RwLock::new(HashMap::new());
    // 用于记录命令(x)
    pub static ref G_CMD_FUN_MAP:RwLock<HashMap<String, fn(&mut RedLang,&[String]) -> Result<Option<String>, Box<dyn std::error::Error>>>> = RwLock::new(HashMap::new());
    // 异步事件循环
    pub static ref  RT_PTR:Arc<tokio::runtime::Runtime> = Arc::new(tokio::runtime::Runtime::new().unwrap());
    // 退出标记，标记整个pkg都在退出
    pub static ref G_QUIT_FLAG:RwLock<bool> = RwLock::new(false);
    // 退出标记，标记某个pkg_name正在退出
    pub static ref G_PKG_QUIT_FLAG:RwLock<HashSet<String>> = RwLock::new(HashSet::new());
    // 记录正在运行的脚本数量（用于退出）
    pub static ref G_RUNNING_SCRIPT_NUM:RwLock<usize> = RwLock::new(0usize);
    // 记录正在加载的脚本
    pub static ref G_LOADING_SCRIPT_FLAG:RwLock<HashSet<String>> = RwLock::new(HashSet::new());
    // 记录正在运行的脚本名字
    pub static ref G_RUNNING_SCRIPT:RwLock<Vec<ScriptInfo>> = RwLock::new(vec![]);
    // 输入流记录
    pub static ref G_INPUTSTREAM_VEC:RwLock<Vec<InputStream>> = RwLock::new(vec![]);
    // webui的访问密码
    pub static ref G_WEB_PASSWORD:RwLock<Option<String>> = RwLock::new(None);
    // webui的访问密码2
    pub static ref G_READONLY_WEB_PASSWORD:RwLock<Option<String>> = RwLock::new(None);
    // 全局锁(x)
    pub static ref G_LOCK:Mutex<HashMap<String,HashMap<String, i32>>> = Mutex::new(HashMap::new());
    // 记录与某条消息相关的脚本输出(x)
    pub static ref G_SCRIPT_RELATE_MSG:RwLock<HashMap<String,ScriptRelatMsg>> = RwLock::new(HashMap::new());
    // 用于自动关闭进程
    pub static ref G_AUTO_CLOSE:Mutex<bool> = Mutex::new(false);
    // 默认字体
    pub static ref G_DEFAULF_FONT:RwLock<String> = RwLock::new(String::new());
    // 文件锁
    pub static ref G_FILE_MX:std::sync::Mutex<HashMap<String,i32>> = std::sync::Mutex::new(HashMap::new());
    // 历史日志
    static ref G_HISTORY_LOG:std::sync::RwLock<VecDeque<String>> = std::sync::RwLock::new(VecDeque::new());
    // 全局过滤器缓存
    pub static ref G_GOBAL_FILTER:std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);
    // 全局初始化缓存
    pub static ref G_GOBAL_INIT:std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);
    // 跳过多长时间的消息
    pub static ref G_SKIP_MSG_TIME:RwLock<i64> = RwLock::new(600);

    // py解析red变量
    pub static ref G_RED_PY_DECODE:String =  r#"
def __red_py_decode(input:str):
    if input.startswith('12331549-6D26-68A5-E192-5EBE9A6EB998'):
        if input[36] == 'B':
            return bytes.fromhex(input[37:])
        if input[36] == 'A':
            retarr = []
            data = input[37:].encode('utf-8')
            while len(data) != 0:
                pos = data.find(b',')
                l = int(data[0:pos])
                retarr.append(__red_py_decode(data[pos + 1:pos + l + 1].decode('utf-8')))
                data = data[pos + l + 1:]
            return retarr
        if input[36] == 'O':
            retobj = {}
            k = None
            data = input[37:].encode('utf-8')
            while len(data) != 0:
                pos = data.find(b',')
                l = int(data[0:pos])
                d = __red_py_decode(data[pos + 1:pos + l + 1].decode('utf-8'))
                if k == None:
                    k = d
                else:
                    retobj[k] = d
                    k = None
                data = data[pos + l + 1:]
            return retobj
    else:
        return input
def __to_red_type(input):
    if isinstance(input,str):
        return input
    if isinstance(input,bytes):
        print(2)
        return '12331549-6D26-68A5-E192-5EBE9A6EB998B' + input.hex().upper()
    if isinstance(input,bool):
        if input == True:
            return '真'
        else:
            return '假'
    if isinstance(input,int):
        return str(input)
    if isinstance(input,float):
        return str(input)
    if isinstance(input,list):
        retstr = '12331549-6D26-68A5-E192-5EBE9A6EB998A'
        for it in input:
            d = __to_red_type(it)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
        return retstr
    if isinstance(input,dict):
        from collections import OrderedDict
        ordered_dict = OrderedDict()
        for k,v in input.items():
            ordered_dict[k] = v
        retstr = '12331549-6D26-68A5-E192-5EBE9A6EB998O'
        for k,v in ordered_dict.items():
            d = __to_red_type(k)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
            d = __to_red_type(v)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
        return retstr
    return str(input)
"#.to_owned();
}


#[derive(RustEmbed)]
#[folder = "res/"]
#[prefix = "res/"]
pub struct Asset;

#[derive(RustEmbed)]
#[folder = "docs/"]
#[prefix = "docs/"]
pub struct AssetDoc;


pub fn pkg_can_run(pkg_name:&str,script_type:&str) -> bool {
    if *G_QUIT_FLAG.read().unwrap() == true {
        return false;
    }
    if G_PKG_QUIT_FLAG.read().unwrap().contains(pkg_name) {
        return false;
    }
    if script_type != "init" && script_type != "输入流" && script_type != "延时" && script_type != "等待信号" {
        if G_LOADING_SCRIPT_FLAG.read().unwrap().contains(pkg_name) {
            return false;
        }
    }
    return true;
}

pub fn wait_one_pkg_quit(pkg_name:&str,timeoutms:i64) -> bool {
    let start_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    loop {
        let mut can_break = true;
        {
            let lk = G_RUNNING_SCRIPT.read().unwrap();
            for it in &*lk {
                if it.pkg_name == pkg_name {
                    can_break = false;
                    break;
                }
            }
        }
        if can_break == true {
            break;
        }
        let now_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        if now_time - start_time > timeoutms {
            return false;
        }
        std::thread::sleep(core::time::Duration::from_millis(1));
    }
    return true;
}

pub fn wait_all_pkg_quit(timeoutms:i64) -> bool {
    let start_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    loop {
        let mut can_break = true;
        {
            let lk = G_RUNNING_SCRIPT.read().unwrap();
            if lk.len() != 0 {
                can_break = false;
            }
        }
        if can_break == true {
            break;
        }
        let now_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        if now_time - start_time > timeoutms {
            return false;
        }
        std::thread::sleep(core::time::Duration::from_millis(1));
    }
    return true;
}


pub fn get_python_cmd_name() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(windows))]
    return Ok("python3".to_owned());

    #[cfg(windows)]
    return Ok("python".to_owned());
}

pub fn get_local_python_uid() -> Result<String,Box<dyn std::error::Error>> {


    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(get_python_cmd_name().unwrap());

    #[cfg(windows)]
    let output = command.creation_flags(0x08000000).arg("-c").arg("import sys; print(sys.version)").output()?;

    #[cfg(not(windows))]
    let output = command.arg("-c").arg("import sys; print(sys.version)").output()?;

    let version = 
    if cfg!(target_os = "windows") {
        encoding::all::GBK.decode(&output.stdout, encoding::DecoderTrap::Ignore)?
    }else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    
    let mut hasher = Md5::new();
    hasher.update(version.trim().to_string().as_bytes());
    let result = hasher.finalize();
    let mut content = String::new();
    for ch in result {
        content.push_str(&format!("{:02x}",ch));
    }
    return Ok(content);
}


pub fn show_ctrl_web() -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    opener::open(format!("http://localhost:{port}"))?;
    Ok(())
}


pub fn show_help_web() -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    opener::open(format!("http://localhost:{port}/docs/index.html"))?;
    Ok(())
}

pub fn show_log_web() -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    opener::open(format!("http://localhost:{port}/watchlog.html"))?;
    Ok(())
}

pub fn show_dir_web() -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let script_path = cq_get_app_directory1()?;
    opener::open(script_path)?;
    Ok(())
}

pub fn show_debug_web() -> Result<(),Box<dyn std::error::Error + Send + Sync>> {
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    opener::open(format!("http://localhost:{port}/debug.html"))?;
    Ok(())
}

pub fn add_egg_click() -> Result<i64,Box<dyn std::error::Error + Send + Sync>> {
    let app_dir = crate::cqapi::cq_get_app_directory1()?;
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });

    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute("CREATE TABLE IF NOT EXISTS EGG_TABLE (EGG_NAME TEXT,VALUE TEXT DEFAULT 0,PRIMARY KEY(EGG_NAME));", [])?;
    let ret_rst:Result<String,rusqlite::Error> = conn.query_row("SELECT VALUE FROM EGG_TABLE WHERE EGG_NAME = ?", ["CLICK"], |row| row.get(0));
    let mut ret_num:i64;
    if let Ok(ret) =  ret_rst {
        ret_num = ret.parse::<i64>()?;
    }else {
        ret_num = 0;
    }
    ret_num += 1;
    if ret_num < 0 {
        ret_num = 0;
    }
    conn.execute("REPLACE INTO EGG_TABLE (EGG_NAME,VALUE) VALUES (?,?)", ["CLICK",&ret_num.to_string()])?;
    return Ok(ret_num);
}

// 获取绝对路径
fn get_apath(filename:&str) -> Option<String> {
    let fname;
    match PathBuf::from_str(filename) {
        Err(_err) => {
            return None;
        },
        Ok(path) => {
            let apath;
            if path.is_absolute() {
                apath = path.clean();
            }else{
                apath = std::env::current_dir().unwrap().join(path).clean();
            }
            fname = apath.to_string_lossy().to_string();
        }
    }
    // println!("fname:{}",fname);
    return Some(fname);
}


pub fn add_file_lock(filename:&str) {
    let fname;
    if let Some(fname_t) =  get_apath(filename) {
        fname = fname_t;
    } else {
        return;
    }
    loop {
        {
            let mut lk = G_FILE_MX.lock().unwrap();
            if !lk.contains_key(&fname) {
                lk.insert(fname.to_string(), 0);
                return;
            }
        }
        // 如果这个文件正在被读写的话，就等待
        let time_struct = core::time::Duration::from_millis(10);
        std::thread::sleep(time_struct);
    }
}

pub fn del_file_lock(filename:&str) {
    let fname;
    if let Some(fname_t) =  get_apath(filename) {
        fname = fname_t;
    } else {
        return;
    }
    let mut lk = G_FILE_MX.lock().unwrap();
    lk.remove(&fname);
}

// 用于清理一个包使用的内存
pub fn del_pkg_memory(pkg_name:&str) {
    // 删除常量
    G_CONST_MAP.write().unwrap().remove(pkg_name);
    // 删除临时常量
    G_TEMP_CONST_MAP.write().unwrap().remove(pkg_name);
    // 删除自定义命令
    G_CMD_MAP.write().unwrap().remove(pkg_name);
    // 删除全局锁
    G_LOCK.lock().unwrap().remove(pkg_name);
    // 删除脚本输出记录
    {
        let mut lk = G_SCRIPT_RELATE_MSG.write().unwrap();
        let mut to_remove_key = vec![];
        for (key,_val) in &*lk {
            if key.starts_with(&format!("{pkg_name}|")) {
                to_remove_key.push(key.to_string());
            }
        }
        for key in to_remove_key {
            lk.remove(&key);
        }
    }
    // 删除自定义的内置命令
    {
        let mut lk = G_CMD_FUN_MAP.write().unwrap();
        let mut to_remove_key = vec![];
        for key in &*lk {
            if key.0.starts_with(&format!("{pkg_name}eb4d8f3e-1c82-653b-5b26-3be3abb007bc")) {
                to_remove_key.push(key.0.to_owned());
            }
        }
        for key in &to_remove_key {
            lk.remove(key);
        }
    }

}


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
    status::flush_cache_to_db().unwrap() ;
    std::process::exit(0);
}

pub fn add_running_script_num(pkg_name:&str,script_name:&str,script_type:&str) -> bool {
    if pkg_can_run(pkg_name,script_type) == false {
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
    cq_add_log(&format!("欢迎使用`红色问答{}`,正在进行资源初始化...",get_version())).unwrap();
    panic::set_hook(Box::new(|e| {
        let binding = std::env::current_exe().unwrap();
        let parent_path = binding.parent().unwrap();
        let path = parent_path.join("crash.log");
        let mut f;
        if path.exists() {
            f = fs::OpenOptions::new().append(true).open(path).unwrap()
        }else {
            f = fs::File::create(path).unwrap();
        }
        std::io::Write::write_all(&mut f, e.to_string().as_str().as_bytes()).unwrap();
        cq_add_log_w(e.to_string().as_str()).unwrap();
    }));

    // 初始化配置文件
    init_config();

    // 从配置文件初始化全局变量
    init_global_var();

    // 初始化命令
    redlang::webexfun::init_web_ex_fun_map();
    redlang::cqexfun::init_cq_ex_fun_map();
    redlang::exfun::init_ex_fun_map();
    redlang::aifun::init_ai_fun_map();
    redlang::init_core_fun_map();

    // 释放文件
    if let Err(err) = release_file(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 初始化HTTP服务器
    if let Err(err) = init_http_server(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 创建python运行环境
    std::thread::spawn(||{
        if let Err(err) = init_python(){
            cq_add_log_w(&err.to_string()).unwrap();
        }
    });
    
    // 初始化red脚本
    if let Err(err) = init_code(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 初始化bot适配器
    if let Err(err) = botconn::do_conn_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }
    
    // 初始化定时器
    if let Err(err) = cronevent::do_cron_event(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 初始化MQTT客户端
    if let Err(err) = mqttclient::init_mqttclient(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    // 初始化postgresql数据库
    if let Err(err) = pgsql::init_postgresql_db(){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    cq_add_log("资源初始化完成！").unwrap();

    
    // 用于自动退出（嵌入的时候可能需要这个功能）
    let config_json = read_config().unwrap();
    if let Some(auto_close_opt) = config_json.get("auto_close") {
        if let Some(auto_close) = auto_close_opt.as_bool() {
            if auto_close {
                thread::spawn(move || {
                    cq_add_log_w("自动退出已经开启，请每5秒提供一次心跳").unwrap();
                    loop {
                        {
                            let mut lk = G_AUTO_CLOSE.lock().unwrap();
                            (*lk) = true;
                        }
                        std::thread::sleep( std::time::Duration::from_secs(10));
                        {
                            let lk = G_AUTO_CLOSE.lock().unwrap();
                            if *lk == true {
                                cq_add_log_w("未及时提供心跳，程序退出！").unwrap();
                                std::thread::sleep( std::time::Duration::from_secs(1));
                                std::process::exit(-1);
                            }
                        }
                    }
                });
            }
        }
    }
    
    return 0;
}

pub fn read_config() -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let script_path = cq_get_app_directory1()? + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone())?.is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{
        let config_json = serde_json::json!({
            "web_port":1207,
            "web_password":"",
            "readonly_web_password":"",
            "web_host":"127.0.0.1",
            "ws_urls":[],
            "not_open_browser":false
        });
        fs::write(script_path.clone(), config_json.to_string()).unwrap();
    }
    let script = fs::read_to_string(script_path)?;
    Ok(serde_json::from_str(&script)?)
}

fn create_python_env() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_dir = cq_get_app_directory1()?;
    fs::create_dir_all(app_dir.clone() + "pymain")?;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    let foo = std::process::Command::new(get_python_cmd_name()?).creation_flags(0x08000000).current_dir(app_dir).arg("-m").arg("venv").arg("pymain").status();
    
    #[cfg(not(windows))]
    let foo = std::process::Command::new(get_python_cmd_name()?).current_dir(app_dir).arg("-m").arg("venv").arg("pymain").status();

    if foo.is_err() {
        return Err(format!("python环境创建失败:{:?}",foo).into());
    }else {
        cq_add_log(&format!("python服务创建:{:?}",foo.unwrap())).unwrap();
    }
    Ok(())
}

pub fn init_python() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    create_python_env()?;
    let config = read_config()?;
    let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
    let code = r#"
import json
import threading
import time
import os

lk = threading.Lock()
WS_APP = None

def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)

def deal_msg_t(message):
    try:
        js = json.loads(message)
        echo = js["echo"]
        try:
            scope = deal_msg(message,js)
            to_send = {"echo":echo,"data":scope["__red_out_data"]}
        except Exception as e:
            to_send = {"echo":echo,"data":str(e)}
        lk.acquire()
        try:
            WS_APP.send(json.dumps(to_send))
        finally:
            lk.release()
    except Exception as e:
        print(e)

def deal_msg(message,js):
    code = js["code"]
    code = """
__red_out_data = ""
def __red_py_decode(input:str):
    if input.startswith('12331549-6D26-68A5-E192-5EBE9A6EB998'):
        if input[36] == 'B':
            return bytes.fromhex(input[37:])
        if input[36] == 'A':
            retarr = []
            data = input[37:].encode('utf-8')
            while len(data) != 0:
                pos = data.find(b',')
                l = int(data[0:pos])
                retarr.append(__red_py_decode(data[pos + 1:pos + l + 1].decode('utf-8')))
                data = data[pos + l + 1:]
            return retarr
        if input[36] == 'O':
            retobj = {}
            k = None
            data = input[37:].encode('utf-8')
            while len(data) != 0:
                pos = data.find(b',')
                l = int(data[0:pos])
                d = __red_py_decode(data[pos + 1:pos + l + 1].decode('utf-8'))
                if k == None:
                    k = d
                else:
                    retobj[k] = d
                    k = None
                data = data[pos + l + 1:]
            return retobj
    else:
        return input
def __to_red_type(input):
    if isinstance(input,str):
        return input
    if isinstance(input,bytes):
        print(2)
        return '12331549-6D26-68A5-E192-5EBE9A6EB998B' + input.hex().upper()
    if isinstance(input,bool):
        if input == True:
            return '真'
        else:
            return '假'
    if isinstance(input,int):
        return str(input)
    if isinstance(input,float):
        return str(input)
    if isinstance(input,list):
        retstr = '12331549-6D26-68A5-E192-5EBE9A6EB998A'
        for it in input:
            d = __to_red_type(it)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
        return retstr
    if isinstance(input,dict):
        from collections import OrderedDict
        ordered_dict = OrderedDict()
        for k,v in input.items():
            ordered_dict[k] = v
        retstr = '12331549-6D26-68A5-E192-5EBE9A6EB998O'
        for k,v in ordered_dict.items():
            d = __to_red_type(k)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
            d = __to_red_type(v)
            l = str(len(d.encode('utf-8')))
            retstr += l + ',' + d
        return retstr
    return str(input)
def red_install(pkg_name):
    '''安装一个模块'''
    from pip._internal.cli import main
    ret = main.main(['install', pkg_name, '-i',
                    'https://pypi.tuna.tsinghua.edu.cn/simple', "--no-warn-script-location"])

    if ret != 0:
        err = "安装依赖{}失败".format(pkg_name)
        raise Exception(err)
def red_in():
    return __red_py_decode(__red_in_data)
def red_out(s):
    global __red_out_data
    __red_out_data = __to_red_type(s)
""" + code
    input_t = js["input"]
    scope = {"__red_in_data":input_t}
    exec(code,scope)
    return scope
    

def on_message(_, message):
    threading.Thread(target=deal_msg_t,args=(message,)).start()

def on_open(_):
    WS_APP.send("opened")

def conn_fun():
    global WS_APP
    WS_APP = websocket.WebSocketApp(
        "ws://localhost:"+os.environ.get('port', '1207')+"/pyserver",
        on_message=on_message,
        on_open= on_open,
        cookie="password={}".format(os.environ.get('password', ''))
    )
    WS_APP.run_forever()

red_install("websocket-client")
import websocket
conn_fun()
"#;
    let password:String = url::form_urlencoded::byte_serialize(read_web_password()?.as_bytes()).collect();
    let curr_env = std::env::var("PATH").unwrap_or_default();
    
    let new_env = if cfg!(target_os = "windows") {
        format!("{}pymain/Scripts;{}",cq_get_app_directory1()?,curr_env)
    } else {
        format!("{}pymain/bin:{}",cq_get_app_directory1()?,curr_env)
    };

    

    thread::spawn(move ||{
        
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;


        #[cfg(windows)]
        let foo = std::process::Command::new(get_python_cmd_name().unwrap()).creation_flags(0x08000000)
        .env("PATH", new_env)
        .arg("-c")
        .arg(code)
        .env("port", port.to_string())
        .env("password", password)
        .status();


        #[cfg(not(windows))]
        let foo = std::process::Command::new(get_python_cmd_name().unwrap())
        .env("PATH", new_env)
        .arg("-c")
        .arg(code)
        .env("port", port.to_string())
        .env("password", password)
        .status();

        if foo.is_err() {
            cq_add_log_w(&format!("python服务启动失败:{:?}",foo)).unwrap();
        }else {
            cq_add_log_w(&format!("python服务退出:{:?}",foo.unwrap())).unwrap();
        }
    });
    Ok(())
}


pub fn init_global_var(){
    let cfg = read_config().unwrap();
    if let Some(tm) = cfg.get("skip_msg_time") {
        *G_SKIP_MSG_TIME.write().unwrap() = tm.as_i64().unwrap();
    }
}


pub fn init_config() {
    let script_path = cq_get_app_directory1().unwrap() + "config.json";
    let mut is_file_exists = false;
    if fs::metadata(script_path.clone()).is_ok() {
        if fs::metadata(script_path.clone()).unwrap().is_file(){
            is_file_exists = true;
        }
    }
    if !is_file_exists{

        let mut line1 = String::new();
        println!("请输入端口号(默认1207):");
        std::io::stdin().read_line(&mut line1).unwrap();
        let web_port;
        if line1.trim() == "" {
            web_port = 1207;
        }else {
            web_port = line1.trim().parse::<u16>().unwrap();
        }
        
        let mut line2 = String::new();
        let web_host:&str;
        println!("请输入主机地址(默认127.0.0.1):");
        std::io::stdin().read_line(&mut line2).unwrap();
        if line2.trim() == "" {
            web_host = "127.0.0.1";
        } else {
            web_host = &line2.trim();
        }

        let config_json = serde_json::json!({
            "web_port":web_port,
            "web_password":"",
            "readonly_web_password":"",
            "web_host":web_host,
            "ws_urls":[],
            "not_open_browser":false
        });
        fs::write(script_path.clone(), config_json.to_string()).unwrap();
    }
    // 将json格式化之后重新写入文件，保持美观
    let cfg_json = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(script_path.clone()).unwrap()).unwrap();
    fs::write(script_path, serde_json::to_string_pretty(&cfg_json).unwrap()).unwrap();
}

pub fn read_web_password() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    {
        let lk = G_WEB_PASSWORD.read().unwrap();
        if lk.is_some() {
            return Ok(lk.clone().unwrap());
        }
    }
    let mut ret_str = String::new();
    let config = read_config()?;
    if let Some(pass_opt) = config.get("web_password") {
        if let Some(pass) = pass_opt.as_str() {
            ret_str = pass.to_string();
        }
    }
    {
        let mut lk = G_WEB_PASSWORD.write().unwrap();
        *lk = Some(ret_str.clone());
    }
    return Ok(ret_str);
}

pub fn read_readonly_web_password() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    {
        let lk = G_READONLY_WEB_PASSWORD.read().unwrap();
        if lk.is_some() {
            return Ok(lk.clone().unwrap());
        }
    }
    let mut ret_str = String::new();
    let config = read_config()?;
    if let Some(pass_opt) = config.get("readonly_web_password") {
        if let Some(pass) = pass_opt.as_str() {
            ret_str = pass.to_string();
        }
    }
    {
        let mut lk = G_READONLY_WEB_PASSWORD.write().unwrap();
        *lk = Some(ret_str.clone());
    }
    return Ok(ret_str);
}

pub fn set_ws_urls(ws_urls:serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = read_config()?;
    config["ws_urls"] = ws_urls;
    let script_path = cq_get_app_directory1()? + "config.json";
    fs::write(script_path,config.to_string())?;
    Ok(())
}

pub fn release_file() -> Result<(), Box<dyn std::error::Error>> {
    let sep = std::path::MAIN_SEPARATOR;
    let err = "get asset err";
    fs::create_dir_all(cq_get_app_directory1().unwrap() + "webui")?;
    for it in Asset::iter() {
        let file = Asset::get(&it.to_string()).ok_or(err)?;
        let file_path = cq_get_app_directory1().unwrap() + "webui" + &sep.to_string() + it.to_string().get(4..).unwrap_or_default();
        if let Some(path) = PathBuf::from_str(&file_path)?.parent() {
            fs::create_dir_all(path)?;
        }
        fs::write(file_path, file.data)?;
    } 
    for it in AssetDoc::iter() {
        let file = AssetDoc::get(&it.to_string()).ok_or(err)?;
        let file_path = cq_get_app_directory1().unwrap() + "webui" + &sep.to_string() + &it.to_string();
        if let Some(path) = PathBuf::from_str(&file_path)?.parent() {
            fs::create_dir_all(path)?;
        }
        fs::write(file_path, file.data)?;
    } 
    Ok(())
}


pub fn get_version() -> String {
    let file = Asset::get("res/version.txt").unwrap();
    let buf = file.data;
    let version_str = String::from_utf8(buf.to_vec()).unwrap();
    return version_str;
}

pub fn get_all_pkg_name_by_cache() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let wk = G_PKG_NAME.read()?;
    let mut ret: Vec<String> = vec![];
    for it in &*wk {
        ret.push(it.to_owned());
    }
    Ok(ret)
}


fn get_all_pkg_name() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
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

fn get_all_pkg_code() -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
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
                fs::write(&script_path, "[]")?;
            }
        }
        
        let content = fs::read_to_string(&script_path)?;

        // 去除BOM头(如果有的话)
        let script = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
        
        let mut pkg_script_vec:Vec<serde_json::Value>;
        match serde_json::from_str(script) {
            Ok(v) => pkg_script_vec = v,
            Err(err) => {
            let sc = script_path.as_os_str().to_string_lossy();
            return Err(format!("解析脚本文件`{sc}`失败(不是合法的json)：{err:?}").into());
            },
        };
        for js in &mut pkg_script_vec {
            if let Some(obj) = js.as_object_mut() {
                obj.insert("pkg_name".to_string(),serde_json::Value::String(it.to_string()));
                arr_val.push(serde_json::Value::Object(obj.clone()));
            }
        }
    }
    Ok(arr_val)
}

pub fn init_code() -> Result<(), Box<dyn std::error::Error + Send + Sync>>{
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
    {
        let mut wk = G_SCRIPT.write().unwrap();
        (*wk) = serde_json::Value::Array(arr_val);
    }

    {
        // 刷新包名
        let pkg_names = get_all_pkg_name()?;
        let mut lk = G_PKG_NAME.write().unwrap();
        lk.clear();
        for it in &pkg_names {
            lk.insert(it.to_owned());
        }
    }

    // 执行初始化脚本
    if let Err(err) = initevent::do_init_event(None){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    Ok(())
}

fn get_gobal_filter_code_from_sql() -> Result<String, Box<dyn std::error::Error>> {
    let app_dir = crate::cqapi::cq_get_app_directory1().unwrap();
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });

    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute("CREATE TABLE IF NOT EXISTS GOBAL_FILTER_TABLE (GOBAL_FILTER_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_FILTER_NAME));", [])?;
    let ret_rst:Result<String,rusqlite::Error> = conn.query_row("SELECT VALUE FROM GOBAL_FILTER_TABLE WHERE GOBAL_FILTER_NAME = ?", ["CODE"], |row| row.get(0));
    let ret_str:String;
    if let Ok(ret) =  ret_rst {
        ret_str = ret;
    }else {
        ret_str = "".to_owned();
    }
    return Ok(ret_str);
}

fn get_gobal_init_code_from_sql() -> Result<String, Box<dyn std::error::Error>> {
    let app_dir = crate::cqapi::cq_get_app_directory1().unwrap();
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });

    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute("CREATE TABLE IF NOT EXISTS GOBAL_INIT_TABLE (GOBAL_INIT_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_INIT_NAME));", [])?;
    let ret_rst:Result<String,rusqlite::Error> = conn.query_row("SELECT VALUE FROM GOBAL_INIT_TABLE WHERE GOBAL_INIT_NAME = ?", ["CODE"], |row| row.get(0));
    let ret_str:String;
    if let Ok(ret) =  ret_rst {
        ret_str = ret;
    }else {
        ret_str = "".to_owned();
    }
    return Ok(ret_str);
}

pub fn set_gobal_filter_code(code:&str) -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = crate::cqapi::cq_get_app_directory1().unwrap();
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });

    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute("CREATE TABLE IF NOT EXISTS GOBAL_FILTER_TABLE (GOBAL_FILTER_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_FILTER_NAME));", [])?;
    conn.execute("REPLACE INTO GOBAL_FILTER_TABLE (GOBAL_FILTER_NAME,VALUE) VALUES (?,?)", ["CODE",code])?;
    let mut wk = G_GOBAL_FILTER.write().unwrap();
    *wk = Some(code.to_owned());
    return Ok(());
}

pub fn set_gobal_init_code(code:&str) -> Result<(), Box<dyn std::error::Error>> {
    {
        let app_dir = crate::cqapi::cq_get_app_directory1().unwrap();
        let sql_file = app_dir + "reddat.db";
        let sql_file = mytool::path_to_os_str(&sql_file);
        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;
        conn.execute("CREATE TABLE IF NOT EXISTS GOBAL_INIT_TABLE (GOBAL_INIT_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_INIT_NAME));", [])?;
        conn.execute("REPLACE INTO GOBAL_INIT_TABLE (GOBAL_INIT_NAME,VALUE) VALUES (?,?)", ["CODE",code])?;
        let mut wk = G_GOBAL_INIT.write().unwrap();
        *wk = Some(code.to_owned());
    }
    // 设置预初始化脚本的时候先执行一次预初始化脚本
    do_gobal_init_event(None)?;
    return Ok(());
}

pub fn get_gobal_filter_code() -> Result<String, Box<dyn std::error::Error>> {
    {
        let wk = G_GOBAL_FILTER.read().unwrap();
        if wk.is_some() {
            return Ok(wk.as_ref().unwrap().to_owned());
        }
    }
    let mut wk = G_GOBAL_FILTER.write().unwrap();
    let code = get_gobal_filter_code_from_sql()?;
    *wk = Some(code);
    let ret = wk.as_ref().unwrap().to_owned();
    return Ok(ret);
}

pub fn get_gobal_init_code() -> Result<String, Box<dyn std::error::Error>> {
    {
        let wk = G_GOBAL_INIT.read().unwrap();
        if wk.is_some() {
            return Ok(wk.as_ref().unwrap().to_owned());
        }
    }
    let mut wk = G_GOBAL_INIT.write().unwrap();
    let code = get_gobal_init_code_from_sql()?;
    *wk = Some(code);
    let ret = wk.as_ref().unwrap().to_owned();
    return Ok(ret);
}

pub fn save_code(contents: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    // 解析网络数据
    let mut code_map:HashMap<String,Vec<serde_json::Value>> = HashMap::new();
    let mut key_vec:Vec<String> = vec![];
    let mut rename_pkg_process:Vec<Vec<String>> = vec![];
    let js:Vec<serde_json::Value>; 

    {
        let js_t:Vec<serde_json::Value> = serde_json::from_str(contents)?;
        js  = js_t.get(2..).ok_or("save_code err 1")?.to_vec();

        // 获得脚本内容
        { 
            for it in &js {
                let mut it_t = it.to_owned();
                // 得到网络包的包名,如果没有pkg_name，则默认为"",网络包中的默认包是没有pkg_name的
                let pkg_name_str = read_json_str(&it_t, "pkg_name");
                it_t.as_object_mut().ok_or("it_t not obj")?.remove("pkg_name");
                if !code_map.contains_key(&pkg_name_str) {
                    code_map.insert(pkg_name_str.to_owned(), vec![]);
                }
                code_map.get_mut(&pkg_name_str).unwrap().push(it_t);
            }
        }

        // 获得存在的包
        for it in js_t.get(1).unwrap().as_array().ok_or("save_code err 2")? {
            let s = it.as_str().ok_or("save_code err 3")?;
            key_vec.push(s.to_owned());
        }

        // 获得重命名目录的方案
        for it in js_t.get(0).unwrap().as_array().ok_or("save_code err 4")? {
            let k1 = it.as_array().ok_or("save_code err 5")?.get(0).ok_or("save_code err 6")?.as_str().ok_or("save_code err 7")?.to_owned();
            let k2 = it.as_array().ok_or("save_code err 5")?.get(1).ok_or("save_code err 6")?.as_str().ok_or("save_code err 7")?.to_owned();
            rename_pkg_process.push(vec![k1,k2]);
        }
    }

    
    
    // 保存脚本
    {
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");

        // 禁止新脚本加载
        (*G_QUIT_FLAG.write().unwrap()) = true;
        let _guard = scopeguard::guard((),|_| {
            (*G_QUIT_FLAG.write().unwrap()) = false;
        });

        // 等待所有脚本退出
        let mut tm = 0;
        loop {
            tm += 1;
            {
                if (*G_RUNNING_SCRIPT_NUM.read().unwrap()) == 0 {
                    break;
                }
            }
            std::thread::sleep(core::time::Duration::from_millis(1));
            if tm > 10000 {
                let running_scripts = get_running_script_info();
                return Err(format!("有脚本:{:?}不愿意退出,所以无法保存", running_scripts).into());
            }
        }

        // 修改文件名
        for it in rename_pkg_process {
            fs::rename(pkg_dir.join(it[0].to_owned()), pkg_dir.join(it[1].to_owned()))?;
        }

        // 创建文件夹
        for pkg_name in &key_vec {
            let script_path = pkg_dir.join(pkg_name);
            std::fs::create_dir_all(&script_path)?;
        }

        // 保存脚本
        for pkg_name in &key_vec {
            let script_path = pkg_dir.join(pkg_name);
            if code_map.contains_key(pkg_name) {
                let cont = serde_json::Value::Array(code_map[pkg_name].to_vec()).to_string();
                std::fs::create_dir_all(&script_path)?;
                fs::write(script_path.join("script.json"), cont)?;
            } else {
                std::fs::create_dir_all(&script_path)?;
                fs::write(script_path.join("script.json"), "[]")?;
            }
        }

        // 保存默认脚本
        if code_map.contains_key("") {
            let cont = serde_json::Value::Array(code_map[""].to_vec()).to_string();
            fs::write(cq_get_app_directory2()? + "script.json",cont)?;
        } else {
            fs::write(cq_get_app_directory2()? + "script.json", "[]")?;
        }

        // 删除目录下多余的包
        let pkg_names = get_all_pkg_name()?;
        for name in &pkg_names {
            if !key_vec.contains(name) {
                let script_path = pkg_dir.join(name);
                let _ = fs::remove_dir_all(script_path);
                del_pkg_memory(name);
            }
        }

    }
   
    // 重新加载脚本
    if let Err(err) = crate::init_code() {
        cq_add_log_w(&format!("can't call init evt:{}",err)).unwrap();
    }
    Ok(())
}

fn save_one_pkg(contents: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>{
    // 解析网络数据
    let js_t:serde_json::Value = serde_json::from_str(contents)?;
    let pkg_name = read_json_str(&js_t, "pkg_name");
    let scripts = js_t.get("data").ok_or("read script err")?.as_array().ok_or("script is not arr")?;

    // 保存脚本
    if pkg_name == "" {
        let cont = js_t.get("data").unwrap().to_string();
        fs::write(cq_get_app_directory2()? + "script.json",cont)?;
    }else {
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let script_path = pkg_dir.join(pkg_name.to_owned());
        let cont = js_t.get("data").unwrap().to_string();
        std::fs::create_dir_all(&script_path)?;
        fs::write(script_path.join("script.json"), cont)?;
    }

    // 插入初始化标记，可以阻挡其它脚本执行
    G_LOADING_SCRIPT_FLAG.write().unwrap().insert(pkg_name.to_owned());
    let _guard = scopeguard::guard((),|_| {
        G_LOADING_SCRIPT_FLAG.write().unwrap().remove(&pkg_name);
    });
    // 等待正在运行的脚本退出
    if wait_one_pkg_quit(&pkg_name, 15000) == false {
        return Err(format!("有脚本不愿意退出,所以无法保存").into());
    }

    // 更新内存中的脚本
    {
        let mut new_script = vec![];
        let mut wk = G_SCRIPT.write().unwrap();
        for it in wk.as_array().ok_or("read G_SCRIPT err")? {
            let it_name = read_json_str(it, "pkg_name");
            if it_name != pkg_name {
                new_script.push(it.to_owned());
            }
        }
        for it in scripts {
            let mut item = it.to_owned();
            item["pkg_name"] = serde_json::Value::String(pkg_name.to_string());
            new_script.push(item);
        }
        (*wk) = serde_json::Value::Array(new_script);
    }

    if pkg_name != "" {
        G_PKG_NAME.write().unwrap().insert(pkg_name.clone());
    }

    // 执行初始化脚本
    if let Err(err) = initevent::do_init_event(Some(&pkg_name)){
        cq_add_log_w(&err.to_string()).unwrap();
    }

    Ok(())
}

fn rename_one_pkg(old_pkg_name:&str,new_pkg_name:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    // 禁止脚本加载
    G_PKG_QUIT_FLAG.write().unwrap().insert(old_pkg_name.to_owned());
    G_PKG_QUIT_FLAG.write().unwrap().insert(new_pkg_name.to_owned());
    let _guard = scopeguard::guard((),|_| {
        G_PKG_QUIT_FLAG.write().unwrap().remove(old_pkg_name);
        G_PKG_QUIT_FLAG.write().unwrap().remove(new_pkg_name);
    });

    // 等待脚本退出
    if wait_one_pkg_quit(old_pkg_name, 15000) == false {
        return Err(format!("有脚本不愿意退出,所以无法保存").into());
    }

    if old_pkg_name != "" && new_pkg_name != ""{
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        fs::rename(pkg_dir.join(old_pkg_name), pkg_dir.join(new_pkg_name))?;
        // 删除缓存中的包名
        let mut lk = G_PKG_NAME.write().unwrap();
        lk.remove(old_pkg_name);
        // 删除旧脚本占用的内存
        del_pkg_memory(old_pkg_name);
        lk.insert(new_pkg_name.to_owned());

        // 修改内存中的脚本
        {
            let mut new_script = vec![];
            let mut wk = G_SCRIPT.write().unwrap();
            for it in wk.as_array().ok_or("read G_SCRIPT err")? {
                let it_name = read_json_str(it, "pkg_name");
                if it_name == old_pkg_name {
                    let mut it_t = it.to_owned();
                    it_t["pkg_name"] = serde_json::Value::String(new_pkg_name.to_string());
                    new_script.push(it_t);
                }else {
                    new_script.push(it.to_owned());
                }
            }
            (*wk) = serde_json::Value::Array(new_script);
        }
        
    }else{
        cq_add_log_w("改名错误：old_pkg_name 或 new_pkg_name为空").unwrap();
        return Err(None.ok_or("rename err")?);
    }
    Ok(())
}


fn del_one_pkg(pkg_name:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    // 禁止新脚本加载
    G_PKG_QUIT_FLAG.write().unwrap().insert(pkg_name.to_owned());
    let _guard = scopeguard::guard((),|_| {
        G_PKG_QUIT_FLAG.write().unwrap().remove(pkg_name);
    });

    // 等待脚本退出
    if wait_one_pkg_quit(pkg_name, 15000) == false {
        return Err(format!("有脚本不愿意退出,所以无法保存").into());
    }

    // 删除pkg文件
    if pkg_name != "" {
        let plus_dir_str = cq_get_app_directory1()?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let script_path = pkg_dir.join(pkg_name.to_owned());
        let _ = fs::remove_dir_all(script_path);
        // 删除缓存中的包名
        G_PKG_NAME.write().unwrap().remove(pkg_name);
        // 删除脚本占用的内存
        del_pkg_memory(pkg_name);
    }
    else {
        return Err(None.ok_or("default_pkg can't be deleted")?);
    }
    // 删除内存中的脚本
    {
        let mut new_script = vec![];
        let mut wk = G_SCRIPT.write().unwrap();
        for it in wk.as_array().ok_or("read G_SCRIPT err")? {
            let it_name = read_json_str(it, "pkg_name");
            if it_name != pkg_name {
                new_script.push(it.to_owned());
            }
        }
        (*wk) = serde_json::Value::Array(new_script);
    }
    Ok(())
}


pub fn read_code_cache() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    Ok((*wk).clone())
}

pub fn backup_code() -> Result<(),Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    let backup_path = cq_get_app_directory1().map_err(|err|err.to_string())? + "backup";
    std::fs::create_dir_all(&backup_path)?;
    let time_str = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();
    let backup_file = backup_path + &std::path::MAIN_SEPARATOR.to_string() + &time_str + ".json";
    fs::write(backup_file,(*wk).to_string())?;
    Ok(())
}

pub fn read_one_pkg(pkg_name:&str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let wk = G_SCRIPT.read()?;
    let mut ret_vec = vec![];
    for it in wk.as_array().ok_or("read G_SCRIPT err")? {
        let it_name = read_json_str(it, "pkg_name");
            if it_name == pkg_name {
                ret_vec.push(it.to_owned());
            }
    }
    if ret_vec.is_empty() && pkg_name != "" && !G_PKG_NAME.read().unwrap().contains(pkg_name) {
        return Err(None.ok_or("so such pkg")?);
    }
    
    Ok(ret_vec)
}
