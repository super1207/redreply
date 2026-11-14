use super::*;

pub(super) fn register() {
    add_fun(vec!["转大写"], |self_t, params| {
        let text1 = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(text1.to_uppercase())))
    });
    add_fun(vec!["转小写"], |self_t, params| {
        let text1 = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(text1.to_lowercase())))
    });
    add_fun(vec!["打印日志"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        let _ = cq_add_log(&text);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["读目录"], |self_t, params| {
        let dir_name = self_t.get_param_text(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec: Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            let file_name = path.to_str().ok_or("获取目录文件异常")?;
            if path.is_dir() {
                ret_vec.push(format!("{}{}", file_name, std::path::MAIN_SEPARATOR));
            } else {
                ret_vec.push(file_name.to_owned());
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["读目录文件"], |self_t, params| {
        let dir_name = self_t.get_param_text(params, 0)?;
        let dirs = fs::read_dir(dir_name)?;
        let mut ret_vec: Vec<String> = vec![];
        for dir in dirs {
            let path = dir?.path();
            if path.is_file() {
                let file_name = path.to_str().ok_or("获取目录文件异常")?.to_owned();
                ret_vec.push(file_name);
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["目录分隔符"], |_self_t, _params| {
        Ok(Some(rv_text(std::path::MAIN_SEPARATOR.to_string())))
    });
    add_fun(vec!["去除开始空白"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(text.trim_start().to_string())))
    });
    add_fun(vec!["去除结尾空白"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(text.trim_end().to_string())))
    });
    add_fun(vec!["去除两边空白"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(text.trim().to_string())))
    });
    add_fun(vec!["数字转字符"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        let num = text.parse::<u8>()?;
        if !(1..=127).contains(&num) {
            return Err(RedLang::make_err("在数字转字符中发生越界"));
        }
        Ok(Some(rv_text((num as char).to_string())))
    });
    add_fun(vec!["创建目录"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        fs::create_dir_all(path)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["读配置"], |self_t, params| {
        let path_rv = self_t.get_param(params, 0)?;
        let section = self_t.get_param_text(params, 1)?;
        let key = self_t.get_param_text(params, 2)?;
        let conf;
        match &*path_rv {
            RedValue::Bin(ini_bin) => {
                conf = Ini::load_from_str(&String::from_utf8(ini_bin.to_vec())?)?;
            }
            RedValue::Text(path_text) => {
                let path = redlang::mytool::path_to_os_str(path_text);
                if Path::new(&path).exists() {
                    add_file_lock(&path);
                    let _guard = scopeguard::guard(path.clone(), |path| {
                        del_file_lock(&path);
                    });
                    conf = Ini::load_from_file(path)?;
                } else {
                    return Ok(Some(rv_empty()));
                }
            }
            _ => return Err(RedLang::make_err("读配置第1参数必须是文本路径或字节集")),
        }
        let section_t = if section.is_empty() {
            conf.section(None::<String>)
        } else {
            conf.section(Some(section))
        };
        let Some(section_t) = section_t else {
            return Ok(Some(rv_empty()));
        };
        let Some(value) = section_t.get(key) else {
            return Ok(Some(rv_empty()));
        };
        Ok(Some(rv_text(value.to_owned())))
    });
    add_fun(vec!["读配置节"], |self_t, params| {
        let path_rv = self_t.get_param(params, 0)?;
        let conf;
        match &*path_rv {
            RedValue::Bin(ini_bin) => {
                conf = Ini::load_from_str(&String::from_utf8(ini_bin.to_vec())?)?;
            }
            RedValue::Text(path_text) => {
                let path = redlang::mytool::path_to_os_str(path_text);
                if Path::new(&path).exists() {
                    add_file_lock(&path);
                    let _guard = scopeguard::guard(path.clone(), |path| {
                        del_file_lock(&path);
                    });
                    conf = Ini::load_from_file(path)?;
                } else {
                    return Ok(Some(rv_array(vec![])));
                }
            }
            _ => return Err(RedLang::make_err("读配置节第1参数必须是文本路径或字节集")),
        }
        let mut to_ret_vec: Vec<String> = vec![];
        for (sec, _prop) in &conf {
            if let Some(sec) = sec {
                to_ret_vec.push(sec.to_string());
            }
        }
        let ret = to_ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });

    add_fun(vec!["读配置键"], |self_t, params| {
        let path_rv = self_t.get_param(params, 0)?;
        let section_name = self_t.get_param_text(params, 1)?;
        let conf;
        match &*path_rv {
            RedValue::Bin(ini_bin) => {
                conf = Ini::load_from_str(&String::from_utf8(ini_bin.to_vec())?)?;
            }
            RedValue::Text(path_text) => {
                let path = redlang::mytool::path_to_os_str(path_text);
                if Path::new(&path).exists() {
                    add_file_lock(&path);
                    let _guard = scopeguard::guard(path.clone(), |path| {
                        del_file_lock(&path);
                    });
                    conf = Ini::load_from_file(path)?;
                } else {
                    return Ok(Some(rv_array(vec![])));
                }
            }
            _ => return Err(RedLang::make_err("读配置键第1参数必须是文本路径或字节集")),
        }
        let mut to_ret_map = BTreeMap::new();
        if let Some(section) = conf.section(Some(section_name)) {
            for (key, val) in section.into_iter() {
                to_ret_map.insert(key.to_owned(), rv_text(val.to_owned()));
            }
        } else {
            return Ok(Some(rv_object(BTreeMap::new())));
        }
        Ok(Some(rv_object(to_ret_map)))
    });

    add_fun(vec!["写配置"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        let path = redlang::mytool::path_to_os_str(&path);
        let section = self_t.get_param_text(params, 1)?;
        let key = self_t.get_param_text(params, 2)?;
        let value = self_t.get_param_text(params, 3)?;
        let parent_path = Path::new(&path)
            .parent()
            .ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        if Path::new(&path).exists() {
            let mut conf = Ini::load_from_file(&path)?;
            if section.is_empty() {
                conf.with_section(None::<String>).set(key, value);
            } else {
                conf.with_section(Some(section)).set(key, value);
            }
            conf.write_to_file(path)?;
        } else {
            let mut conf = Ini::new();
            if section.is_empty() {
                conf.with_section(None::<String>).set(key, value);
            } else {
                conf.with_section(Some(section)).set(key, value);
            }
            conf.write_to_file(path)?;
        }
        Ok(Some(rv_empty()))
    });
    fn text_to_bin_data(text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }
    add_fun(vec!["写文件"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        let path = redlang::mytool::path_to_os_str(&path);
        let data_rv = self_t.get_param(params, 1)?;
        let bin_data = match &*data_rv {
            RedValue::Bin(b) => b.to_vec(),
            RedValue::Text(s) => text_to_bin_data(s),
            _ => return Err(RedLang::make_err("写文件第2参数必须是文本或字节集")),
        };
        let parent_path = Path::new(&path)
            .parent()
            .ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        let mut f = fs::File::create(path)?;
        std::io::Write::write_all(&mut f, &bin_data)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["追加文件"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        let path = redlang::mytool::path_to_os_str(&path);
        let data_rv = self_t.get_param(params, 1)?;
        let bin_data = match &*data_rv {
            RedValue::Bin(b) => b.to_vec(),
            RedValue::Text(s) => text_to_bin_data(s),
            _ => return Err(RedLang::make_err("追加文件第2参数必须是文本或字节集")),
        };
        let parent_path = Path::new(&path)
            .parent()
            .ok_or("写文件：无法创建目录或文件")?;
        fs::create_dir_all(parent_path)?;
        add_file_lock(&path);
        let _guard = scopeguard::guard(path.clone(), |path| {
            del_file_lock(&path);
        });
        let mut f;
        if Path::new(&path).exists() {
            f = fs::OpenOptions::new().append(true).open(path)?
        } else {
            f = fs::File::create(path)?;
        }
        std::io::Write::write_all(&mut f, &bin_data)?;
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["设置浏览器宽高"], |self_t, params| {
        let k1 = self_t.get_param_text(params, 0)?;
        let k2 = self_t.get_param_text(params, 1)?;
        // 校验
        k1.parse::<u32>()?;
        k2.parse::<u32>()?;
        self_t.set_coremap("浏览器宽", &k1)?;
        self_t.set_coremap("浏览器高", &k2)?;
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["网页截图调试"], |self_t, params| {
        let k1: String = self_t.get_param_text(params, 0)?;
        self_t.set_coremap("网页截图调试", &k1)?;
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["网页截图"], |self_t, params| match browser_access(
        self_t,
        params,
        "screenshot",
    ) {
        Ok(data) => Ok(Some(rv_bin(data))),
        Err(err) => {
            let _ = cq_add_log_w(&format!("网页截图失败：`{:?}`", err));
            Ok(Some(rv_bin(vec![])))
        }
    });

    add_fun(
        vec!["网页获取"],
        |self_t, params| match browser_access(self_t, params, "content") {
            Ok(data) => Ok(Some(rv_text(String::from_utf8(data)?))),
            Err(err) => {
                let _ = cq_add_log_w(&format!("网页获取失败：`{:?}`", err));
                Ok(Some(rv_empty()))
            }
        },
    );

    add_fun(vec!["网页获取文本"], |self_t, params| match browser_access(
        self_t,
        params,
        "inner_text",
    ) {
        Ok(data) => Ok(Some(rv_text(String::from_utf8(data)?))),
        Err(err) => {
            let _ = cq_add_log_w(&format!("网页获取文本失败：`{:?}`", err));
            Ok(Some(rv_empty()))
        }
    });

    add_fun(vec!["命令行"], |self_t, params| {
        let cmd_str = self_t.get_param_text(params, 0)?;
        let currdir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;

        let output = if cfg!(target_os = "windows") {
            // cmd 的解析规则too复杂，我不想看了 https://learn.microsoft.com/zh-cn/windows-server/administration/windows-commands/cmd
            let tmp_dir = match get_tmp_dir() {
                Ok(dir) => dir,
                Err(error) => return Err(format!("get_tmp_dir err:{error:?}").into()),
            };
            let tmp_file = uuid::Uuid::new_v4().to_string() + ".cmd";
            let tmp_file_path = PathBuf::from_str(&tmp_dir)?.join(tmp_file);
            let mut gbk_cmd: Vec<u8> = vec![];
            gbk_cmd.append(&mut "@echo off\r\n".as_bytes().to_vec());
            gbk_cmd.append(&mut encoding::Encoding::encode(
                encoding::all::GBK,
                &cmd_str,
                encoding::EncoderTrap::Ignore,
            )?);
            fs::write(&tmp_file_path, gbk_cmd)?;
            let _guard = scopeguard::guard(tmp_file_path.clone(), |tmp_file_path| {
                let _foo = fs::remove_file(tmp_file_path);
            });

            let mut command = std::process::Command::new("cmd");

            #[cfg(windows)]
            let out = command
                .creation_flags(0x08000000)
                .current_dir(currdir)
                .arg("/c")
                .arg(tmp_file_path)
                .output()?;

            #[cfg(not(windows))]
            let out = command
                .current_dir(currdir)
                .arg("/c")
                .arg(tmp_file_path)
                .output()?;

            out
        } else {
            std::process::Command::new("sh")
                .current_dir(currdir)
                .arg("-c")
                .arg(cmd_str)
                .output()?
        };
        let mut output_str = if cfg!(target_os = "windows") {
            let code_t = self_t.get_param_text(params, 1)?;
            let code = code_t.to_lowercase();
            if code == "utf8" || code == "utf-8" {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else if code == "gbk" || code.is_empty() {
                encoding::all::GBK.decode(&output.stdout, encoding::DecoderTrap::Ignore)?
            } else {
                return Err(RedLang::make_err(&("不支持的编码:".to_owned() + &code_t)));
            }
        } else {
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        let out_err = if cfg!(target_os = "windows") {
            encoding::all::GBK.decode(&output.stderr, encoding::DecoderTrap::Ignore)?
        } else {
            String::from_utf8_lossy(&output.stderr).to_string()
        };
        output_str.push_str(&out_err);
        Ok(Some(rv_text(output_str)))
    });
    add_fun(vec!["启动"], |self_t, params| {
        let cmd_str = self_t.get_param_text(params, 0)?;
        opener::open(cmd_str)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["文本查找"], |self_t, params| {
        let text = self_t.get_param_text(params, 0)?;
        let sub = self_t.get_param_text(params, 1)?;
        let pos_str = self_t.get_param_text(params, 2)?;
        let pos = if pos_str.is_empty() {
            0
        } else {
            pos_str.parse::<usize>()?
        };
        let text_chs = text.chars().collect::<Vec<char>>();
        if pos >= text_chs.len() {
            return Ok(Some(rv_text("-1".to_string())));
        }
        let text_str = text_chs
            .get(pos..)
            .ok_or("位置超出范围")?
            .iter()
            .collect::<String>();
        if let Some(pos1) = text_str.find(&sub) {
            let t = text_str.get(0..pos1).ok_or("位置超出范围")?;
            let pos2 = t.chars().collect::<Vec<char>>().len();
            return Ok(Some(rv_text((pos + pos2).to_string())));
        }
        Ok(Some(rv_text("-1".to_string())))
    });

    add_fun(vec!["数组查找"], |self_t, params| {
        let arr = self_t.get_param_array(params, 0)?;
        let sub = self_t.get_param(params, 1)?;
        let sub_text = sub.expect_text_value().ok();
        let pos_str = self_t.get_param_text(params, 2)?;
        let mut pos;
        if pos_str.is_empty() {
            pos = 0;
        } else {
            pos = pos_str.parse::<usize>()?;
        }
        while pos < arr.len() {
            if *arr[pos] == *sub
                || sub_text
                    .as_ref()
                    .is_some_and(|text| arr[pos].as_text() == Some(text.as_str()))
            {
                return Ok(Some(rv_text(pos.to_string())));
            }
            pos += 1;
        }
        Ok(Some(rv_text("-1".to_string())))
    });

    add_fun(vec!["错误信息"], |self_t, _params| {
        Ok(Some(rv_text(self_t.get_coremap("错误信息").to_owned())))
    });

    add_fun(vec!["运行SQL"], |self_t, params| {
        let sql_file = self_t.get_param_text(params, 0)?;
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);
        let sql = self_t.get_param_text(params, 1)?;
        let sql_params_rv = self_t.get_param(params, 2)?;
        let sql_params = if sql_params_rv.as_text() == Some("") {
            vec![]
        } else {
            sql_params_rv
                .expect_array_value()?
                .into_iter()
                .map(|item| item.expect_text_value())
                .collect::<Result<Vec<String>, _>>()?
        };

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        fn add_regexp_function(db: &rusqlite::Connection) -> rusqlite::Result<()> {
            use rusqlite::functions::FunctionFlags;
            use rusqlite::{Error, Result};
            type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
            db.create_scalar_function(
                "regexp",
                2,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |ctx| {
                    let regexp: Arc<fancy_regex::Regex> = ctx
                        .get_or_create_aux(0, |vr| -> Result<_, BoxError> {
                            Ok(fancy_regex::Regex::new(vr.as_str()?)?)
                        })?;
                    let is_match = {
                        let text = ctx
                            .get_raw(1)
                            .as_str()
                            .map_err(|e| Error::UserFunctionError(e.into()))?;
                        regexp
                            .is_match(text)
                            .map_err(|e| Error::UserFunctionError(e.into()))?
                    };
                    Ok(is_match)
                },
            )
        }

        fn add_red_function(
            self_t: &mut RedLang,
            db: &rusqlite::Connection,
        ) -> rusqlite::Result<()> {
            use rusqlite::functions::FunctionFlags;
            use rusqlite::Error;

            let exmap = (*self_t.exmap).borrow().clone();
            let pkg_name = self_t.pkg_name.clone();
            let script_name = self_t.script_name.clone();
            let can_wrong = self_t.can_wrong;

            db.create_scalar_function(
                "red",
                1,
                // 不能标 SQLITE_DETERMINISTIC：red() 会执行脚本代码，有副作用，
                // 标了之后 SQLite 可能缓存/折叠调用，导致脚本少执行或执行顺序被打乱
                FunctionFlags::SQLITE_UTF8,
                move |ctx| {
                    let code = ctx.get::<String>(0)?;

                    let mut rl = RedLang::new();
                    rl.exmap = std::rc::Rc::new(std::cell::RefCell::new(exmap.clone()));
                    rl.pkg_name = pkg_name.clone();
                    rl.script_name = script_name.clone();
                    rl.can_wrong = can_wrong;

                    let red_out_rst = rl.parse_to_string(&code);
                    match red_out_rst {
                        Ok(red_out) => Ok(red_out),
                        Err(e) => Err(Error::UserFunctionError(e.to_string().into())),
                    }
                },
            )
        }

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        add_regexp_function(&conn)?;
        add_red_function(self_t, &conn)?;
        let mut stmt = conn.prepare(&sql)?;
        let count = stmt.column_count();
        let mut vec: Vec<Rc<RedValue>> = vec![];
        let mut rows = stmt.query(rusqlite::params_from_iter(sql_params.iter()))?;
        while let Some(row) = rows.next()? {
            let mut v: Vec<Rc<RedValue>> = vec![];
            for i in 0..count {
                let k = row.get_ref_unwrap(i);
                let dat = match k.data_type() {
                    rusqlite::types::Type::Null => rv_empty(),
                    rusqlite::types::Type::Integer => rv_text(k.as_i64().unwrap_or(0).to_string()),
                    rusqlite::types::Type::Real => rv_text(k.as_f64().unwrap_or(0.0).to_string()),
                    rusqlite::types::Type::Text => rv_text(
                        // TEXT 列可能含非法 UTF-8 字节（SQLite 无类型强制），
                        // as_str 会返回 Err，unwarp 将导致进程崩溃
                        match k.as_str() {
                            Ok(s) => s.to_owned(),
                            Err(_) => {
                                String::from_utf8_lossy(k.as_bytes().unwrap_or(&[])).to_string()
                            }
                        },
                    ),
                    rusqlite::types::Type::Blob => rv_bin(k.as_blob().unwrap_or(&[]).to_vec()),
                };
                v.push(dat);
            }
            vec.push(Rc::new(RedValue::Array(v)));
        }
        let changes = conn.changes().to_string();
        self_t.set_coremap("SQL修改数", &changes)?;
        Ok(Some(Rc::new(RedValue::Array(vec))))
    });
    add_fun(vec!["SQL修改数"], |self_t, _params| {
        let k = self_t.get_coremap("SQL修改数");
        if k.is_empty() {
            return Ok(Some(rv_text("0".to_owned())));
        }
        Ok(Some(rv_text(k)))
    });
    add_fun(vec!["定义持久常量"], |self_t, params| {
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        let key = self_t.get_param_text(params, 0)?;
        let value_rv = self_t.get_param(params, 1)?;
        let value =
            crate::core::persistent_value_codec::to_persistent_legacy_string(value_rv.as_ref())?;

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        conn.execute(
            "CREATE TABLE IF NOT EXISTS CONST_TABLE (KEY TEXT PRIMARY KEY,VALUE TEXT);",
            [],
        )?;
        conn.execute(
            "REPLACE INTO CONST_TABLE (KEY,VALUE) VALUES (?,?)",
            [key, value.as_ref().clone()],
        )?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["持久常量"], |self_t, params| {
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        let key = self_t.get_param_text(params, 0)?;

        let ret_rst: Result<String, rusqlite::Error>;

        {
            add_file_lock(&sql_file);
            let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
                del_file_lock(&sql_file);
            });
            let conn = rusqlite::Connection::open(sql_file)?;
            redlang::resource_manager::configure_conn_busy_timeout(&conn);
            ret_rst = conn.query_row(
                "SELECT VALUE FROM CONST_TABLE WHERE KEY = ?",
                [key],
                |row| row.get(0),
            );
        }

        if let Ok(ret) = ret_rst {
            return Ok(Some(Rc::new(
                crate::core::persistent_value_codec::from_persistent_legacy_string(&ret)?,
            )));
        } else {
            return Ok(Some(self_t.get_param(params, 1)?));
        }
    });

    #[cfg(target_os = "windows")]
    add_fun(vec!["截屏"], |self_t, params| {
        let num_str = self_t.get_param_text(params, 0)?;
        let mut num = 0usize;
        if !num_str.is_empty() {
            num = num_str.parse::<usize>()?;
        }
        let monitors = xcap::Monitor::all()?;
        if num >= monitors.len() {
            return Ok(Some(rv_bin(vec![])));
        }
        let image = monitors[num].capture_image()?;
        let mut bytes: Vec<u8> = Vec::new();
        image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
        Ok(Some(rv_bin(bytes)))
    });

    add_fun(vec!["__重启"], |_self_t, _params| {
        std::thread::spawn(|| {
            use redlang::lifecycle::wait_for_quit;
            use std::env;
            use std::process::Command;

            // 拿不到自身路径属异常情况，按 fail-fast 直接 panic
            let Ok(exe_path) = env::current_exe() else {
                let _ = redlang::cqapi::cq_add_log_w("无法获取当前程序路径");
                return;
            };
            let args: Vec<String> = env::args().skip(1).collect();

            let lock_name = uuid::Uuid::new_v4().to_string();

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const DETACHED_PROCESS: u32 = 0x00000008;
                let _ = Command::new(&exe_path)
                    .args(&args)
                    .env("REDREPLY_LOCK_NAME", lock_name)
                    .creation_flags(DETACHED_PROCESS)
                    .spawn();
                wait_for_quit();
            }

            #[cfg(not(target_os = "windows"))]
            {
                let _ = Command::new(&exe_path)
                    .args(&args)
                    .env("REDREPLY_LOCK_NAME", lock_name)
                    .spawn();
                wait_for_quit();
            }
        });
        Ok(Some(rv_empty()))
    });
}
