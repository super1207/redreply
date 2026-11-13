use std::fs;
use std::path::PathBuf;

use crate::cqapi::cq_get_app_directory1;

pub fn show_ctrl_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://127.0.0.1:{port}"))?;
    Ok(())
}

pub fn show_help_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://127.0.0.1:{port}/docs/index.html"))?;
    Ok(())
}

pub fn show_log_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://127.0.0.1:{port}/redlang/watchlog.html"))?;
    Ok(())
}

pub fn show_dir_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    opener::open(cq_get_app_directory1()?)?;
    Ok(())
}

pub fn show_debug_web() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = crate::config_manager::startup_config()?.web_port;
    opener::open(format!("http://127.0.0.1:{port}/redlang/debug.html"))?;
    Ok(())
}

pub use redreply_common::file_lock::{
    add_file_lock, configure_conn_busy_timeout, del_file_lock,
};

pub fn add_egg_click() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let sql_file = crate::mytool::path_to_os_str(&(cq_get_app_directory1()? + "reddat.db"));
    add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |path| del_file_lock(&path));

    let conn = rusqlite::Connection::open(sql_file)?;
    configure_conn_busy_timeout(&conn);
    conn.execute(
        "CREATE TABLE IF NOT EXISTS EGG_TABLE (EGG_NAME TEXT,VALUE TEXT DEFAULT 0,PRIMARY KEY(EGG_NAME));",
        [],
    )?;
    let current: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT VALUE FROM EGG_TABLE WHERE EGG_NAME = ?",
        ["CLICK"],
        |row| row.get(0),
    );
    // VALUE 损坏时从 0 重新计数，而不是让整个功能卡死
    let mut value = current
        .unwrap_or_else(|_| "0".to_owned())
        .parse::<i64>()
        .unwrap_or(0);
    value = value.saturating_add(1).max(0);
    conn.execute(
        "REPLACE INTO EGG_TABLE (EGG_NAME,VALUE) VALUES (?,?)",
        ["CLICK", &value.to_string()],
    )?;
    Ok(value)
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
    crate::Asset::get("res/version.txt")
        .map(|file| String::from_utf8_lossy(&file.data).trim().to_string())
        .unwrap_or_default()
}
