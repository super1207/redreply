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

const LIVE2D_CHAT_GENERATION_DB_KEY: &str = "LIVE2D_CHAT_PENDING_GENERATION";

/// 看板娘日记本最大条数。
pub const LIVE2D_DIARY_LIMIT: usize = 100;

/// 看板娘日记本条目，CREATE_TIME/UPDATE_TIME 为 "YYYY-MM-DD HH:MM:SS" 格式的本地时间。
#[derive(Clone, Debug)]
pub struct Live2dDiaryEntry {
    pub key: String,
    pub title: String,
    pub content: String,
    pub create_time: String,
    pub update_time: String,
}

fn live2d_db_path() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(crate::mytool::path_to_os_str(
        &(cq_get_app_directory1()? + "reddat.db"),
    ))
}

fn open_live2d_db() -> Result<rusqlite::Connection, Box<dyn std::error::Error + Send + Sync>> {
    let sql_file = live2d_db_path()?;
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |path| del_file_lock(&path));
    let conn = rusqlite::Connection::open(sql_file)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS LIVE2D_CHAT_MESSAGES (ID INTEGER PRIMARY KEY AUTOINCREMENT,MSG TEXT NOT NULL);",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS LIVE2D_CHAT_META (KEY TEXT PRIMARY KEY,VALUE TEXT);",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS LIVE2D_DIARY_TABLE (KEY TEXT PRIMARY KEY,TITLE TEXT,CONTENT TEXT,CREATE_TIME TEXT,UPDATE_TIME TEXT);",
        [],
    )?;
    Ok(conn)
}

/// 从 reddat.db 读取全部看板娘对话消息（按写入顺序），没有对话时返回 None。
pub fn load_live2d_chat_messages(
) -> Result<Option<Vec<serde_json::Value>>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    let mut stmt = conn.prepare("SELECT MSG FROM LIVE2D_CHAT_MESSAGES ORDER BY ID")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut messages = Vec::new();
    for row in rows {
        let raw = row?;
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => messages.push(value),
            Err(error) => {
                return Err(format!("看板娘对话消息格式无效: {error}").into());
            }
        }
    }
    if messages.is_empty() {
        Ok(None)
    } else {
        Ok(Some(messages))
    }
}

/// 在对话末尾追加一条消息。
pub fn append_live2d_chat_message(
    message: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    conn.execute(
        "INSERT INTO LIVE2D_CHAT_MESSAGES (MSG) VALUES (?)",
        rusqlite::params![message.to_string()],
    )?;
    Ok(())
}

/// 用新消息替换对话末尾最后一条消息；对话为空时退化为追加。
pub fn replace_live2d_chat_last_message(
    message: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    let changed = conn.execute(
        "UPDATE LIVE2D_CHAT_MESSAGES SET MSG = ? WHERE ID = (SELECT MAX(ID) FROM LIVE2D_CHAT_MESSAGES)",
        rusqlite::params![message.to_string()],
    )?;
    if changed == 0 {
        conn.execute(
            "INSERT INTO LIVE2D_CHAT_MESSAGES (MSG) VALUES (?)",
            rusqlite::params![message.to_string()],
        )?;
    }
    Ok(())
}

/// 用给定消息整体替换对话内容。
pub fn replace_all_live2d_chat_messages(
    messages: &[serde_json::Value],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = open_live2d_db()?;
    let transaction = conn.transaction()?;
    transaction.execute("DELETE FROM LIVE2D_CHAT_MESSAGES", [])?;
    for message in messages {
        transaction.execute(
            "INSERT INTO LIVE2D_CHAT_MESSAGES (MSG) VALUES (?)",
            rusqlite::params![message.to_string()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// 返回当前等待工具结果的对话代次；没有待处理工具调用时返回 None。
pub fn load_live2d_chat_generation(
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    let result = conn.query_row(
        "SELECT VALUE FROM LIVE2D_CHAT_META WHERE KEY = ?",
        [LIVE2D_CHAT_GENERATION_DB_KEY],
        |row| row.get(0),
    );
    match result {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// 设置或清除等待工具结果的代次。
pub fn save_live2d_chat_generation(
    generation: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    match generation {
        Some(generation) => {
            conn.execute(
                "REPLACE INTO LIVE2D_CHAT_META (KEY,VALUE) VALUES (?,?)",
                rusqlite::params![LIVE2D_CHAT_GENERATION_DB_KEY, generation],
            )?;
        }
        None => {
            conn.execute(
                "DELETE FROM LIVE2D_CHAT_META WHERE KEY = ?",
                [LIVE2D_CHAT_GENERATION_DB_KEY],
            )?;
        }
    }
    Ok(())
}

/// 删除 reddat.db 中的全部看板娘对话与代次。
pub fn clear_live2d_chat_conversation() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = open_live2d_db()?;
    let transaction = conn.transaction()?;
    transaction.execute("DELETE FROM LIVE2D_CHAT_MESSAGES", [])?;
    transaction.execute(
        "DELETE FROM LIVE2D_CHAT_META WHERE KEY = ?",
        [LIVE2D_CHAT_GENERATION_DB_KEY],
    )?;
    transaction.commit()?;
    Ok(())
}

fn live2d_diary_key() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("d{nanos:x}{counter:x}")
}

fn trim_live2d_diaries(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM LIVE2D_DIARY_TABLE WHERE KEY IN (SELECT KEY FROM LIVE2D_DIARY_TABLE ORDER BY ROWID DESC LIMIT -1 OFFSET ?)",
        [LIVE2D_DIARY_LIMIT as i64],
    )?;
    Ok(())
}

/// 返回看板娘日记本全部条目，按写入顺序排列。
pub fn list_live2d_diaries(
) -> Result<Vec<Live2dDiaryEntry>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    let mut stmt = conn.prepare(
        "SELECT KEY,TITLE,CONTENT,CREATE_TIME,UPDATE_TIME FROM LIVE2D_DIARY_TABLE ORDER BY ROWID",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Live2dDiaryEntry {
            key: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            create_time: row.get(3)?,
            update_time: row.get(4)?,
        })
    })?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

/// 对 reddat.db 执行只读查询（仅允许 SELECT/PRAGMA 单条语句），
/// 返回 (列名, 行数据)。单条查询最多返回 MAX_ROWS 行，单元格最长 MAX_CELL_CHARS。
pub fn query_live2d_db(
    sql: &str,
) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>), Box<dyn std::error::Error + Send + Sync>> {
    const MAX_ROWS: usize = 100;
    const MAX_CELL_CHARS: usize = 10_000;

    let trimmed = sql.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("SELECT") && !upper.starts_with("PRAGMA") {
        return Err("仅允许 SELECT/PRAGMA 只读查询".into());
    }
    let conn = open_live2d_db()?;
    let mut stmt = conn.prepare(trimmed)?;
    let columns = stmt
        .column_names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let mut rows = stmt.query([])?;
    let mut data = Vec::new();
    while let Some(row) = rows.next()? {
        if data.len() >= MAX_ROWS {
            break;
        }
        let mut cells = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            let cell = match row.get_ref(index)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(value) => serde_json::Value::from(value),
                rusqlite::types::ValueRef::Real(value) => serde_json::Value::from(value),
                rusqlite::types::ValueRef::Text(value) => {
                    serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
                }
                rusqlite::types::ValueRef::Blob(value) => {
                    serde_json::Value::String(format!("<blob {}B>", value.len()))
                }
            };
            if let serde_json::Value::String(text) = &cell {
                if text.chars().count() > MAX_CELL_CHARS {
                    cells.push(serde_json::Value::String(
                        text.chars().take(MAX_CELL_CHARS).collect::<String>() + "...(截断)",
                    ));
                    continue;
                }
            }
            cells.push(cell);
        }
        data.push(cells);
    }
    Ok((columns, data))
}

/// 批量应用压缩助手产生的日记更新：先按 KEY 删除，再新增条目，
/// 最后保证总数不超过 LIVE2D_DIARY_LIMIT（超出时删除最旧的）。
/// 新增条目的创建/更新时间取当前本地时间。
pub fn apply_live2d_diary_updates(
    add: &[(String, String)],
    delete: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = open_live2d_db()?;
    for key in delete {
        conn.execute("DELETE FROM LIVE2D_DIARY_TABLE WHERE KEY = ?", [key])?;
    }
    for (title, content) in add {
        conn.execute(
            "INSERT INTO LIVE2D_DIARY_TABLE (KEY,TITLE,CONTENT,CREATE_TIME,UPDATE_TIME) VALUES (?,?,?,datetime('now','localtime'),datetime('now','localtime'))",
            rusqlite::params![live2d_diary_key(), title, content],
        )?;
    }
    trim_live2d_diaries(&conn)?;
    Ok(())
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
