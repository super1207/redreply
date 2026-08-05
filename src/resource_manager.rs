use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use encoding::Encoding;
use md5::{Digest, Md5};
use path_clean::PathClean;

use crate::{cqapi::cq_get_app_directory1, runtime_context};

pub fn get_python_cmd_name() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(windows))]
    return Ok("python3".to_owned());

    #[cfg(windows)]
    Ok("python".to_owned())
}

pub fn get_local_python_uid() -> Result<String, Box<dyn std::error::Error>> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(get_python_cmd_name().unwrap());
    #[cfg(windows)]
    let output = command
        .creation_flags(0x08000000)
        .arg("-c")
        .arg("import sys; print(sys.version)")
        .output()?;
    #[cfg(not(windows))]
    let output = command
        .arg("-c")
        .arg("import sys; print(sys.version)")
        .output()?;

    let version = if cfg!(target_os = "windows") {
        encoding::all::GBK.decode(&output.stdout, encoding::DecoderTrap::Ignore)?
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };

    let mut hasher = Md5::new();
    hasher.update(version.trim().as_bytes());
    let result = hasher.finalize();
    Ok(result.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn show_ctrl_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://localhost:{port}"))?;
    Ok(())
}

pub fn show_help_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://localhost:{port}/docs/index.html"))?;
    Ok(())
}

pub fn show_log_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://localhost:{port}/watchlog.html"))?;
    Ok(())
}

pub fn show_dir_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    opener::open(cq_get_app_directory1()?)?;
    Ok(())
}

pub fn show_debug_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://localhost:{port}/debug.html"))?;
    Ok(())
}

pub fn add_egg_click() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let sql_file = crate::mytool::path_to_os_str(&(cq_get_app_directory1()? + "reddat.db"));
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |path| del_file_lock(&path));

    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS EGG_TABLE (EGG_NAME TEXT,VALUE TEXT DEFAULT 0,PRIMARY KEY(EGG_NAME));",
        [],
    )?;
    let current: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT VALUE FROM EGG_TABLE WHERE EGG_NAME = ?",
        ["CLICK"],
        |row| row.get(0),
    );
    let mut value = current.unwrap_or_else(|_| "0".to_owned()).parse::<i64>()?;
    value = (value + 1).max(0);
    conn.execute(
        "REPLACE INTO EGG_TABLE (EGG_NAME,VALUE) VALUES (?,?)",
        ["CLICK", &value.to_string()],
    )?;
    Ok(value)
}

fn get_apath(filename: &str) -> Option<String> {
    let path = PathBuf::from_str(filename).ok()?;
    let absolute = if path.is_absolute() {
        path.clean()
    } else {
        std::env::current_dir().ok()?.join(path).clean()
    };
    Some(absolute.to_string_lossy().into_owned())
}

pub fn add_file_lock(filename: &str) {
    let Some(filename) = get_apath(filename) else {
        return;
    };
    runtime_context().sync.acquire_file_lock(&filename, || {
        crate::cq_add_log_w(&format!("等待文件锁超时(5s)，仍在等待:{filename}")).unwrap();
    });
}

pub fn del_file_lock(filename: &str) {
    let Some(filename) = get_apath(filename) else {
        return;
    };
    runtime_context().sync.release_file_lock(&filename);
}

pub fn release_file() -> Result<(), Box<dyn std::error::Error>> {
    let separator = std::path::MAIN_SEPARATOR.to_string();
    let app_dir =
        cq_get_app_directory1().map_err(|error| std::io::Error::other(error.to_string()))?;
    let webui = PathBuf::from(&app_dir).join("webui");
    fs::create_dir_all(&webui)?;

    for entry in crate::Asset::iter() {
        let entry = entry.to_string();
        let file = crate::Asset::get(&entry).ok_or("get asset err")?;
        let relative = entry.get(4..).unwrap_or_default();
        let file_path = webui.join(relative.replace('/', &separator));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file_path, file.data)?;
    }
    for entry in crate::AssetDoc::iter() {
        let entry = entry.to_string();
        let file = crate::AssetDoc::get(&entry).ok_or("get asset err")?;
        let file_path = webui.join(entry.replace('/', &separator));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file_path, file.data)?;
    }
    Ok(())
}

pub fn get_version() -> String {
    let file = crate::Asset::get("res/version.txt").unwrap();
    String::from_utf8(file.data.to_vec()).unwrap()
}

fn create_python_env() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_dir = cq_get_app_directory1()?;
    fs::create_dir_all(app_dir.clone() + "pymain")?;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    let status = std::process::Command::new(get_python_cmd_name()?)
        .creation_flags(0x08000000)
        .current_dir(app_dir)
        .arg("-m")
        .arg("venv")
        .arg("pymain")
        .status();
    #[cfg(not(windows))]
    let status = std::process::Command::new(get_python_cmd_name()?)
        .current_dir(app_dir)
        .arg("-m")
        .arg("venv")
        .arg("pymain")
        .status();

    match status {
        Ok(status) => {
            crate::cq_add_log(&format!("python服务创建:{status}")).unwrap();
            Ok(())
        }
        Err(err) => Err(format!("python环境创建失败:{err}").into()),
    }
}

pub fn init_python() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    create_python_env()?;
    let port = crate::config_manager::startup_config()?.web_port;
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
import base64

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
    __red_out_data = __to_red_data(s)
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
    let password: String = url::form_urlencoded::byte_serialize(
        crate::config_manager::read_web_password()?.as_bytes(),
    )
    .collect();
    let curr_env = std::env::var("PATH").unwrap_or_default();

    let new_env = if cfg!(target_os = "windows") {
        format!("{}pymain/Scripts;{}", cq_get_app_directory1()?, curr_env)
    } else {
        format!("{}pymain/bin:{}", cq_get_app_directory1()?, curr_env)
    };

    std::thread::spawn(move || {
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        #[cfg(windows)]
        let status = std::process::Command::new(get_python_cmd_name().unwrap())
            .creation_flags(0x08000000)
            .env("PATH", new_env)
            .arg("-c")
            .arg(code)
            .env("port", port.to_string())
            .env("password", password)
            .status();

        #[cfg(not(windows))]
        let status = std::process::Command::new(get_python_cmd_name().unwrap())
            .env("PATH", new_env)
            .arg("-c")
            .arg(code)
            .env("port", port.to_string())
            .env("password", password)
            .status();

        match status {
            Err(err) => {
                crate::cq_add_log_w(&format!("python服务启动失败:{err:?}")).unwrap();
            }
            Ok(status) => {
                crate::cq_add_log_w(&format!("python服务退出:{status:?}")).unwrap();
            }
        }
    });
    Ok(())
}
