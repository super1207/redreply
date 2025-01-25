use crate::{add_file_lock, del_file_lock, mytool};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

lazy_static! {
    static ref CACHE: Mutex<HashMap<(String, String, String), i64>> = Mutex::new(HashMap::new());
    static ref LAST_FLUSH: Mutex<Instant> = Mutex::new(Instant::now());
}

pub fn the_500ms_timer() -> Result<(),Box<dyn std::error::Error>> {
    let mut last_flush = LAST_FLUSH.lock().unwrap();
    if last_flush.elapsed() >= Duration::from_secs(10) {
        flush_cache_to_db()?;
        *last_flush = Instant::now();
    }
    Ok(())
}

pub fn flush_cache_to_db() -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = CACHE.lock().unwrap();
    if cache.is_empty() {
        return Ok(());
    }

    let app_dir = crate::cqapi::cq_get_app_directory1().map_err(|err|err.to_string())?;
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });

    let conn = rusqlite::Connection::open(sql_file)?;
    create_table_without_lock(&conn)?;

    for ((platform, bot_id, field), delta) in cache.drain() {
        let ret_rst: Result<i64,rusqlite::Error> = conn.query_row(
            &format!("SELECT {field} FROM STATUS_TABLE WHERE PLATFORM = ? AND BOT_ID = ?"), 
            [&platform, &bot_id], 
            |row| row.get(0)
        );

        let mut value = if let Ok(v) = ret_rst { v } else { 0 };
        value += delta;
        if value < 0 { value = 0; }

        if ret_rst.is_ok() {
            conn.execute(
                &format!("UPDATE STATUS_TABLE SET {field} = ? WHERE PLATFORM = ? AND BOT_ID = ?"),
                [&value.to_string(), &platform, &bot_id]
            )?;
        } else {
            conn.execute(
                &format!("INSERT INTO STATUS_TABLE (PLATFORM,BOT_ID,{field}) VALUES (?,?,?)"),
                [&platform, &bot_id, &value.to_string()]
            )?;
        }
    }
    Ok(())
}

fn create_table_without_lock(conn:&rusqlite::Connection) -> Result<(),Box<dyn std::error::Error>> {
    conn.execute("CREATE TABLE IF NOT EXISTS STATUS_TABLE (\
                        PLATFORM TEXT,\
                        BOT_ID TEXT,\
                        RECV_GROUP_MSG INTEGER DEFAULT 0,\
                        RECV_PRIVATE_MSG INTEGER DEFAULT 0,\
                        SEND_GROUP_MSG INTEGER DEFAULT 0,\
                        SEND_PRIVATE_MSG INTEGER DEFAULT 0,\
                        PRIMARY KEY(PLATFORM,BOT_ID));", [])?;
    Ok(())
}

fn add_something(platform: &str, bot_id: &str, to_add: &str) -> Result<(),Box<dyn std::error::Error>> {
    let mut cache = CACHE.lock().unwrap();
    let key = (platform.to_string(), bot_id.to_string(), to_add.to_string());
    *cache.entry(key).or_insert(0) += 1;
    Ok(())
}

pub fn add_recv_group_msg(platform:&str,bot_id:&str) -> Result<(),Box<dyn std::error::Error>> {
    add_something(platform,bot_id,"RECV_GROUP_MSG")?;
    Ok(())
}


pub fn add_recv_private_msg(platform:&str,bot_id:&str) -> Result<(),Box<dyn std::error::Error>> {
    add_something(platform,bot_id,"RECV_PRIVATE_MSG")?;
    Ok(())
}

pub fn add_send_private_msg(platform:&str,bot_id:&str) -> Result<(),Box<dyn std::error::Error>> {
    add_something(platform,bot_id,"SEND_PRIVATE_MSG")?;
    Ok(())
}

pub fn add_send_group_msg(platform:&str,bot_id:&str) -> Result<(),Box<dyn std::error::Error>> {
    add_something(platform,bot_id,"SEND_GROUP_MSG")?;
    Ok(())
}

pub fn get_status() -> Result<serde_json::Value,Box<dyn std::error::Error>> {
    // 先从数据库读取基础数据
    let app_dir = crate::cqapi::cq_get_app_directory1().map_err(|err|err.to_string())?;
    let sql_file = app_dir + "reddat.db";
    let sql_file = mytool::path_to_os_str(&sql_file);
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
        del_file_lock(&sql_file);
    });
    let conn = rusqlite::Connection::open(sql_file)?;
    create_table_without_lock(&conn)?;

    // 使用HashMap存储合并后的数据
    let mut result_map: HashMap<(String, String), serde_json::Value> = HashMap::new();

    // 读取数据库数据
    let mut stmt = conn.prepare("SELECT * FROM STATUS_TABLE")?;
    let status_iter = stmt.query_map([], |row| {
        Ok((
            (
                row.get::<_, String>(0)?,  // PLATFORM
                row.get::<_, String>(1)?,  // BOT_ID
            ),
            serde_json::json!({
                "PLATFORM": row.get::<_, String>(0)?,
                "BOT_ID": row.get::<_, String>(1)?,
                "RECV_GROUP_MSG": row.get::<_, i64>(2)?,
                "RECV_PRIVATE_MSG": row.get::<_, i64>(3)?,
                "SEND_GROUP_MSG": row.get::<_, i64>(4)?,
                "SEND_PRIVATE_MSG": row.get::<_, i64>(5)?
            })
        ))
    })?;

    for status in status_iter {
        let (key, value) = status?;
        result_map.insert(key, value);
    }

    // 合并内存缓存中的数据
    let cache = CACHE.lock().unwrap();
    for ((platform, bot_id, field), delta) in cache.iter() {
        let key = (platform.clone(), bot_id.clone());
        let entry = result_map.entry(key.clone()).or_insert(serde_json::json!({
            "PLATFORM": platform,
            "BOT_ID": bot_id,
            "RECV_GROUP_MSG": 0,
            "RECV_PRIVATE_MSG": 0,
            "SEND_GROUP_MSG": 0,
            "SEND_PRIVATE_MSG": 0
        }));

        if let Some(current) = entry[field].as_i64() {
            entry[field] = serde_json::json!(current + delta);
        }
    }

    Ok(serde_json::Value::Array(result_map.into_values().collect()))
}