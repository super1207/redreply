use std::{collections::HashMap, error::Error, fs, path::PathBuf, str::FromStr};

use redlang::cqapi::{cq_add_log_w, cq_get_app_directory1, cq_get_app_directory2};
use crate::initevent;
use redlang::mytool::{path_to_os_str, read_json_str};
use redlang::runtime_context;

/// 串行化所有“保存/删除/改名/上传”类写操作，避免并发保存交错写损坏 script.json
static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_save(
) -> Result<std::sync::MutexGuard<'static, ()>, Box<dyn Error + Send + Sync>> {
    SAVE_LOCK.lock().map_err(|_| "保存锁已毒化".into())
}

/// 原子写入：先写临时文件、fsync 落盘后再重命名，
/// 避免断电时 rename 已生效而数据块未落盘，留下半截/空的 script.json
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::io::Write;
    // 使用 uuid 后缀避免同名 tmp 竞争
    let tmp_path = {
        let mut p = path.as_os_str().to_owned();
        p.push(format!(".tmp.{}", uuid::Uuid::new_v4()));
        std::path::PathBuf::from(p)
    };
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        let _ = file.sync_all();
    }
    // 同目录重命名是原子的；跨文件系统则回退为拷贝
    if let Err(e) = fs::rename(&tmp_path, path) {
        // EXDEV 等跨文件系统错误时回退
        if e.kind() == std::io::ErrorKind::CrossesDevices {
            let _ = fs::remove_file(path);
            fs::copy(&tmp_path, path)?;
            let _ = fs::remove_file(&tmp_path);
            return Ok(());
        }
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

fn get_all_pkg_name() -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
    let pkg_dir = PathBuf::from_str(&cq_get_app_directory1()?)?.join("pkg_dir");
    fs::create_dir_all(&pkg_dir)?;
    let mut pkg_names = Vec::new();
    for entry in fs::read_dir(pkg_dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if let Some(name) = path.file_name() {
                pkg_names.push(name.to_string_lossy().into_owned());
            }
        }
    }
    if pkg_names.iter().any(|name| name == "默认包") {
        // 返回错误而不是 panic：release 下 panic=abort 会让整个进程退出
        return Err("附加包的包名不可以为`默认包`!".into());
    }
    Ok(pkg_names)
}

fn get_all_pkg_code() -> Result<Vec<serde_json::Value>, Box<dyn Error + Send + Sync>> {
    let pkg_dir = PathBuf::from_str(&cq_get_app_directory1()?)?.join("pkg_dir");
    let pkg_names = get_all_pkg_name()?;
    let mut scripts = Vec::new();
    for pkg_name in pkg_names {
        let script_path = pkg_dir.join(&pkg_name).join("script.json");
        if !script_path.is_file() {
            fs::write(&script_path, "[]")?;
        }

        let content = fs::read_to_string(&script_path)?;
        let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
        let mut package_scripts: Vec<serde_json::Value> =
            serde_json::from_str(content).map_err(|err| {
                let path = script_path.as_os_str().to_string_lossy();
                format!("解析脚本文件`{path}`失败(不是合法的json)：{err:?}")
            })?;
        for script in &mut package_scripts {
            if let Some(object) = script.as_object_mut() {
                object.insert(
                    "pkg_name".to_owned(),
                    serde_json::Value::String(pkg_name.clone()),
                );
                scripts.push(serde_json::Value::Object(object.clone()));
            }
        }
    }
    Ok(scripts)
}

pub fn init_code() -> Result<(), Box<dyn Error + Send + Sync>> {
    let script_path = cq_get_app_directory2()? + "script.json";
    if !std::path::Path::new(&script_path).is_file() {
        fs::write(&script_path, "[]")?;
    }

    let content = fs::read_to_string(&script_path)?;
    // 与附加包一致：剥离 BOM，避免 Windows 编辑器写入 BOM 后 JSON 解析失败
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let mut scripts: Vec<serde_json::Value> =
        serde_json::from_str(content).map_err(|err| {
            format!("解析脚本文件`{script_path}`失败(不是合法的json)：{err:?}")
        })?;
    scripts.extend(get_all_pkg_code()?);
    crate::core::scripts()
        .replace_scripts(serde_json::Value::Array(scripts));
    crate::core::scripts()
        .replace_package_names(get_all_pkg_name()?);

    if let Err(err) = initevent::do_init_event(None) {
        let _ = cq_add_log_w(&err.to_string());
    }
    Ok(())
}

fn get_global_code_from_sql(table: &str, key: &str) -> Result<String, Box<dyn Error>> {
    let app_dir =
        cq_get_app_directory1().map_err(|error| std::io::Error::other(error.to_string()))?;
    let sql_file = path_to_os_str(&(app_dir + "reddat.db"));
    redlang::resource_manager::add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |path| {
        redlang::resource_manager::del_file_lock(&path);
    });

    let conn = rusqlite::Connection::open(sql_file)?;

    redlang::resource_manager::configure_conn_busy_timeout(&conn);
    let create_sql = match table {
        "GOBAL_FILTER_TABLE" => {
            "CREATE TABLE IF NOT EXISTS GOBAL_FILTER_TABLE (GOBAL_FILTER_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_FILTER_NAME));"
        }
        "GOBAL_INIT_TABLE" => {
            "CREATE TABLE IF NOT EXISTS GOBAL_INIT_TABLE (GOBAL_INIT_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_INIT_NAME));"
        }
        _ => return Err("unsupported global code table".into()),
    };
    conn.execute(create_sql, [])?;
    let value: Result<String, rusqlite::Error> = conn.query_row(
        &format!("SELECT VALUE FROM {table} WHERE {key} = ?"),
        ["CODE"],
        |row| row.get(0),
    );
    Ok(value.unwrap_or_default())
}

pub fn set_gobal_filter_code(code: &str) -> Result<(), Box<dyn Error>> {
    let code_owned = code.to_owned();
    set_global_code("GOBAL_FILTER_TABLE", "GOBAL_FILTER_NAME", code, move || {
        // 在文件锁范围内同步更新缓存，与父版本行为一致，避免并发设置时缓存被旧值覆盖
        crate::core::scripts().set_global_filter_code(code_owned);
    })?;
    Ok(())
}

pub fn set_gobal_init_code(code: &str) -> Result<(), Box<dyn Error>> {
    let code_owned = code.to_owned();
    set_global_code("GOBAL_INIT_TABLE", "GOBAL_INIT_NAME", code, move || {
        crate::core::scripts().set_global_init_code(code_owned);
    })?;
    crate::initevent::do_gobal_init_event(None)?;
    Ok(())
}

fn set_global_code(
    table: &str,
    key: &str,
    code: &str,
    update_cache: impl FnOnce(),
) -> Result<(), Box<dyn Error>> {
    let app_dir =
        cq_get_app_directory1().map_err(|error| std::io::Error::other(error.to_string()))?;
    let sql_file = path_to_os_str(&(app_dir + "reddat.db"));
    redlang::resource_manager::add_file_lock(&sql_file);
    let _guard = scopeguard::guard(sql_file.clone(), |path| {
        redlang::resource_manager::del_file_lock(&path);
    });

    let conn = rusqlite::Connection::open(sql_file)?;

    redlang::resource_manager::configure_conn_busy_timeout(&conn);
    let create_sql = match table {
        "GOBAL_FILTER_TABLE" => {
            "CREATE TABLE IF NOT EXISTS GOBAL_FILTER_TABLE (GOBAL_FILTER_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_FILTER_NAME));"
        }
        "GOBAL_INIT_TABLE" => {
            "CREATE TABLE IF NOT EXISTS GOBAL_INIT_TABLE (GOBAL_INIT_NAME TEXT,VALUE TEXT DEFAULT '',PRIMARY KEY(GOBAL_INIT_NAME));"
        }
        _ => return Err("unsupported global code table".into()),
    };
    conn.execute(create_sql, [])?;
    conn.execute(
        &format!("REPLACE INTO {table} ({key},VALUE) VALUES (?,?)"),
        ["CODE", code],
    )?;
    // 在文件锁仍持有的情况下更新内存缓存，与 DB 写入原子化，防止并发设置时缓存被旧值覆盖
    update_cache();
    Ok(())
}

pub fn get_gobal_filter_code() -> Result<String, Box<dyn Error>> {
    if let Some(code) = crate::core::scripts().global_filter_code() {
        return Ok(code);
    }
    let code = get_global_code_from_sql("GOBAL_FILTER_TABLE", "GOBAL_FILTER_NAME")?;
    crate::core::scripts()
        .set_global_filter_code(code.clone());
    Ok(code)
}

pub fn get_gobal_init_code() -> Result<String, Box<dyn Error>> {
    if let Some(code) = crate::core::scripts().global_init_code() {
        return Ok(code);
    }
    let code = get_global_code_from_sql("GOBAL_INIT_TABLE", "GOBAL_INIT_NAME")?;
    crate::core::scripts().set_global_init_code(code.clone());
    Ok(code)
}

pub fn save_code(contents: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    // 先解析校验，再进入临界区，减少持锁时间
    let parsed: Vec<serde_json::Value> = serde_json::from_str(contents)?;
    let scripts = parsed.get(2..).ok_or("save_code err 1")?;
    let package_names = parsed
        .get(1)
        .ok_or("save_code err 2")?
        .as_array()
        .ok_or("save_code err 2")?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or("save_code err 3"))
        .collect::<Result<Vec<_>, _>>()?;
    let rename_process = parsed
        .first()
        .ok_or("save_code err 4")?
        .as_array()
        .ok_or("save_code err 4")?
        .iter()
        .map(|value| {
            let pair = value.as_array().ok_or("save_code err 5")?;
            Ok::<_, Box<dyn Error + Send + Sync>>((
                pair.first()
                    .and_then(|v| v.as_str())
                    .ok_or("save_code err 6")?
                    .to_owned(),
                pair.get(1)
                    .and_then(|v| v.as_str())
                    .ok_or("save_code err 7")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut code_map: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for script in scripts {
        let mut script = script.clone();
        let pkg_name = read_json_str(&script, "pkg_name");
        script
            .as_object_mut()
            .ok_or("it_t not obj")?
            .remove("pkg_name");
        code_map.entry(pkg_name).or_default().push(script);
    }

    let pkg_dir = PathBuf::from_str(&cq_get_app_directory1()?).map(|path| path.join("pkg_dir"))?;
    // 先等脚本退出，再持锁写盘，避免持锁空等 10s 阻塞其它包操作
    crate::control_plane::control().set_quit(true);
    let quit_guard = scopeguard::guard((), |_| crate::control_plane::control().set_quit(false));
    if !crate::lifecycle::wait_all_pkg_quit(10000) {
        return Err(format!(
            "有脚本:{:?}不愿意退出,所以无法保存",
            crate::lifecycle::get_running_script_info()
        )
        .into());
    }
    // 等待成功后再进入临界区，缩短持锁时间
    let _save_guard = lock_save()?;

    // 重命名带回滚：若中途失败，尝试逆向恢复已成功的重命名
    let mut succeeded_renames: Vec<(String, String)> = Vec::new();
    for (old_name, new_name) in &rename_process {
        if let Err(e) = fs::rename(pkg_dir.join(old_name), pkg_dir.join(new_name)) {
            for (s_old, s_new) in succeeded_renames.iter().rev() {
                let _ = fs::rename(pkg_dir.join(s_new), pkg_dir.join(s_old));
            }
            return Err(e.into());
        }
        succeeded_renames.push((old_name.clone(), new_name.clone()));
    }
    for pkg_name in &package_names {
        let path = pkg_dir.join(pkg_name);
        fs::create_dir_all(&path)?;
        let content = code_map
            .get(pkg_name)
            .cloned()
            .map(serde_json::Value::Array)
            .unwrap_or_else(|| serde_json::json!([]));
        atomic_write(&path.join("script.json"), &content.to_string())?;
    }

    let default_path = cq_get_app_directory2()? + "script.json";
    let default_content = code_map
        .get("")
        .cloned()
        .map(serde_json::Value::Array)
        .unwrap_or_else(|| serde_json::json!([]));
    atomic_write(std::path::Path::new(&default_path), &default_content.to_string())?;

    // 删除集合 = 磁盘上存在但不在本次提交列表中的包目录；
    // 本次重命名产生的新名必须保留（客户端列表可能仍用旧名描述改名过程）
    let mut keep_names: std::collections::HashSet<String> =
        package_names.iter().cloned().collect();
    for (_, new_name) in &succeeded_renames {
        keep_names.insert(new_name.clone());
    }
    for pkg_name in get_all_pkg_name()? {
        if !keep_names.contains(&pkg_name) {
            let _ = cq_add_log_w(&format!("保存脚本：删除不在提交列表中的包目录`{pkg_name}`"));
            let _ = fs::remove_dir_all(pkg_dir.join(&pkg_name));
            del_pkg_memory(&pkg_name);
            if let Err(err) = redlang::config_manager::set_ban_pkg(&pkg_name, false) {
                let _ = cq_add_log_w(&format!("从禁用列表删除包`{pkg_name}`失败:{err}"));
            }
        }
    }

    // 文件操作全部完成，在此释放 quit_guard，使 quit_flag 恢复 false
    // 再调用 init_code()，否则 add_running_script_num 会因 quit_flag==true 跳过初始化脚本
    drop(quit_guard);
    if let Err(err) = init_code() {
        let _ = cq_add_log_w(&format!("can't call init evt:{err}"));
    }
    Ok(())
}

pub fn save_one_pkg(contents: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let json: serde_json::Value = serde_json::from_str(contents)?;
    let pkg_name = read_json_str(&json, "pkg_name");
    let scripts = json
        .get("data")
        .ok_or("read script err")?
        .as_array()
        .ok_or("script is not arr")?;

    // 先等该包脚本退出，再持锁写盘，避免持锁空等 15s
    crate::control_plane::control()
        .set_loading_script(&pkg_name, true);
    let _guard = scopeguard::guard((), |_| {
        crate::control_plane::control()
            .set_loading_script(&pkg_name, false);
    });
    if !crate::lifecycle::wait_one_pkg_quit(&pkg_name, 15000) {
        return Err("有脚本不愿意退出,所以无法保存".into());
    }
    let _save_guard = lock_save()?;

    if pkg_name.is_empty() {
        let default_path = cq_get_app_directory2()? + "script.json";
        atomic_write(std::path::Path::new(&default_path), &json["data"].to_string())?;
    } else {
        let path = PathBuf::from_str(&cq_get_app_directory1()?)?
            .join("pkg_dir")
            .join(&pkg_name);
        fs::create_dir_all(&path)?;
        atomic_write(&path.join("script.json"), &json["data"].to_string())?;
    }

    let current = crate::core::scripts().read_scripts();
    let mut merged = current
        .as_array()
        .ok_or("read script cache err")?
        .iter()
        .filter(|script| read_json_str(script, "pkg_name") != pkg_name)
        .cloned()
        .collect::<Vec<_>>();
    for script in scripts {
        let mut script = script.clone();
        script["pkg_name"] = serde_json::Value::String(pkg_name.clone());
        merged.push(script);
    }
    crate::core::scripts()
        .replace_scripts(serde_json::Value::Array(merged));
    if !pkg_name.is_empty() {
        crate::core::scripts().add_package_name(pkg_name.clone());
    }
    if let Err(err) = initevent::do_init_event(Some(&pkg_name)) {
        let _ = cq_add_log_w(&err.to_string());
    }
    Ok(())
}

pub fn rename_one_pkg(
    old_pkg_name: &str,
    new_pkg_name: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    crate::control_plane::control()
        .set_package_quit(old_pkg_name, true);
    crate::control_plane::control()
        .set_package_quit(new_pkg_name, true);
    let _guard = scopeguard::guard((), |_| {
        crate::control_plane::control()
            .set_package_quit(old_pkg_name, false);
        crate::control_plane::control()
            .set_package_quit(new_pkg_name, false);
    });
    if !crate::lifecycle::wait_one_pkg_quit(old_pkg_name, 15000) {
        return Err("有脚本不愿意退出,所以无法保存".into());
    }
    // 等待成功后再持锁，避免长时间占用
    let _save_guard = lock_save()?;
    if old_pkg_name.is_empty() || new_pkg_name.is_empty() {
        let _ = cq_add_log_w("改名错误：old_pkg_name 或 new_pkg_name为空");
        return Err("rename err".into());
    }

    let pkg_dir = PathBuf::from_str(&cq_get_app_directory1()?).map(|path| path.join("pkg_dir"))?;
    fs::rename(pkg_dir.join(old_pkg_name), pkg_dir.join(new_pkg_name))?;
    crate::core::scripts().remove_package_name(old_pkg_name);
    del_pkg_memory(old_pkg_name);
    crate::core::scripts()
        .add_package_name(new_pkg_name.to_owned());
    if let Err(err) = redlang::config_manager::rename_ban_pkg(old_pkg_name, new_pkg_name) {
        let _ = cq_add_log_w(&format!("同步禁用列表失败:{err}"));
    }

    let scripts = crate::core::scripts().read_scripts();
    let scripts: Vec<serde_json::Value> = scripts
        .as_array()
        .ok_or("read script cache err")?
        .iter()
        .map(|script| {
            let mut script = script.clone();
            if read_json_str(&script, "pkg_name") == old_pkg_name {
                script["pkg_name"] = serde_json::Value::String(new_pkg_name.to_owned());
            }
            script
        })
        .collect();
    crate::core::scripts()
        .replace_scripts(serde_json::Value::Array(scripts));
    if let Err(err) = initevent::do_init_event(Some(new_pkg_name)) {
        let _ = cq_add_log_w(&err.to_string());
    }
    Ok(())
}

pub fn del_one_pkg(pkg_name: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    crate::control_plane::control().set_package_quit(pkg_name, true);
    let _guard = scopeguard::guard((), |_| {
        crate::control_plane::control().set_package_quit(pkg_name, false);
    });
    if !crate::lifecycle::wait_one_pkg_quit(pkg_name, 15000) {
        return Err("有脚本不愿意退出,所以无法保存".into());
    }
    let _save_guard = lock_save()?;
    if pkg_name.is_empty() {
        return Err("default_pkg can't be deleted".into());
    }

    let pkg_dir = PathBuf::from_str(&cq_get_app_directory1()?).map(|path| path.join("pkg_dir"))?;
    let _ = fs::remove_dir_all(pkg_dir.join(pkg_name));
    crate::core::scripts().remove_package_name(pkg_name);
    del_pkg_memory(pkg_name);
    if let Err(err) = redlang::config_manager::set_ban_pkg(pkg_name, false) {
        let _ = cq_add_log_w(&format!("从禁用列表删除包`{pkg_name}`失败:{err}"));
    }

    let scripts = crate::core::scripts().read_scripts();
    let scripts = scripts
        .as_array()
        .ok_or("read script cache err")?
        .iter()
        .filter(|script| read_json_str(script, "pkg_name") != pkg_name)
        .cloned()
        .collect();
    crate::core::scripts()
        .replace_scripts(serde_json::Value::Array(scripts));
    Ok(())
}

pub fn backup_code() -> Result<(), Box<dyn Error>> {
    let backup_path =
        PathBuf::from(cq_get_app_directory1().map_err(|err| err.to_string())?).join("backup");
    fs::create_dir_all(&backup_path)?;
    let time_str = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S");
    fs::write(
        backup_path.join(format!("{time_str}.json")),
        crate::core::scripts().read_scripts().to_string(),
    )?;
    prune_old_backups(&backup_path, MAX_BACKUP_FILES)?;
    Ok(())
}

/// 最多保留的脚本备份文件数量，防止 backup 目录无限增长
const MAX_BACKUP_FILES: usize = 50;

/// 按文件名（时间戳格式）排序删除最旧的备份
fn prune_old_backups(backup_path: &std::path::Path, max_keep: usize) -> Result<(), Box<dyn Error>> {
    let mut backups: Vec<PathBuf> = fs::read_dir(backup_path)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().map(|ext| ext == "json").unwrap_or(false))
        .collect();
    if backups.len() <= max_keep {
        return Ok(());
    }
    // 文件名为 %Y-%m-%d-%H-%M-%S.json，字典序即时间序
    backups.sort();
    let delete_count = backups.len() - max_keep;
    for path in backups.into_iter().take(delete_count) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub fn read_one_pkg(pkg_name: &str) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    let scripts = crate::core::scripts().read_scripts();
    let scripts = scripts.as_array().ok_or("read script cache err")?;
    let result = scripts
        .iter()
        .filter(|script| read_json_str(script, "pkg_name") == pkg_name)
        .cloned()
        .collect::<Vec<_>>();
    if result.is_empty()
        && !pkg_name.is_empty()
        && !crate::core::scripts().contains_package_name(pkg_name)
    {
        return Err("so such pkg".into());
    }
    Ok(result)
}

pub fn del_pkg_memory(pkg_name: &str) {
    crate::core::scripts().remove_package_memory(pkg_name);
    crate::core::sync().release_package_locks(pkg_name);
    runtime_context()
        .session
        .remove_package_related_messages(pkg_name);
    // 清理该包的延迟触发任务，避免重建同名包后被旧任务误触发
    crate::core::scheduler().remove_pkg_runs(pkg_name);
}
