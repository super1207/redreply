use super::*;

pub(super) fn register() {
    add_fun(vec!["BOT权限"], |rl, _params| {
        group_member_role(rl, &rl.get_exmap("机器人ID"))
    });
    add_fun(vec!["伪造OB事件"], |self_t, params| {
        let ob_event = self_t.get_param_text(params, 0)?;
        thread::spawn(move || {
            if let Err(e) = crate::cqevent::do_1207_event(&ob_event) {
                let _ = redlang::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["同意"], |self_t, params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        if get_platform(self_t) == "telegram" && raw_json["notice_type"] == "telegram"
            && matches!(raw_json["sub_type"].as_str(),Some("shipping_query" | "pre_checkout_query"))
        {
            return crate::bot_commands::management::payments::answer(self_t,params,&raw_json,true);
        }
        if get_platform(self_t) == "telegram"
            && raw_json["notice_type"] == "telegram" && raw_json["sub_type"] == "chat_join_request"
        {
            return handle_telegram_join_request(self_t, &raw_json, true);
        }
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let remark = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add" || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":true,
                    "reason":"".to_owned()
                }
            });

            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        } else if request_type == "friend" {
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":true,
                    "remark":remark
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["拒绝"], |self_t, params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        if get_platform(self_t) == "telegram" && raw_json["notice_type"] == "telegram"
            && matches!(raw_json["sub_type"].as_str(),Some("shipping_query" | "pre_checkout_query"))
        {
            return crate::bot_commands::management::payments::answer(self_t,params,&raw_json,false);
        }
        if get_platform(self_t) == "telegram"
            && raw_json["notice_type"] == "telegram" && raw_json["sub_type"] == "chat_join_request"
        {
            return handle_telegram_join_request(self_t, &raw_json, false);
        }
        let request_type = read_json_str(&raw_json, "request_type");
        let sub_type = read_json_str(&raw_json, "sub_type");
        let flag = read_json_str(&raw_json, "flag");
        let reason = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        if request_type == "group" && (sub_type == "add" || sub_type == "invite") {
            let send_json = serde_json::json!({
                "action":"set_group_add_request",
                "params":{
                    "sub_type": sub_type,
                    "flag": flag,
                    "approve":false,
                    "reason":reason
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        } else if request_type == "friend" {
            let send_json = serde_json::json!({
                "action":"set_friend_add_request",
                "params":{
                    "flag": flag,
                    "approve":false,
                    "remark":"".to_owned()
                }
            });
            let platform = get_platform(self_t);
            let passive_id = self_t.get_exmap("消息ID");
            let remote_id = self_t.get_exmap("远程MQTT客户端ID");
            let _cq_ret = cq_call_api(
                &platform,
                &self_id,
                &passive_id,
                &send_json.to_string(),
                &remote_id,
            );
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["脚本输出"], |self_t, params| {
        let src_msg_id = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let key = format!("{}|{}|{}", self_t.pkg_name, self_id, src_msg_id);
        let msg_id_vec = runtime_context().session.related_message_ids(&key);
        if msg_id_vec.is_empty() {
            Ok(Some(rv_array(vec![])))
        } else {
            let ret_vec = msg_id_vec
                .iter()
                .map(|x| rv_text(x.to_string()))
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(ret_vec)))
        }
    });

    add_fun(vec!["脚本输出-增加ID"], |self_t, params| {
        let src_msg_id = self_t.get_exmap("消息ID"); // 来源消息ID
        let to_add_msg_id = self_t.get_param_text(params, 0)?;
        let self_id = self_t.get_exmap("机器人ID");
        let key = format!("{}|{}|{}", self_t.pkg_name, self_id, src_msg_id);
        let tm = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        runtime_context()
            .session
            .add_related_message(key, &self_id, to_add_msg_id, tm);
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分-增加"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let add_score = self_t.get_param_text(params, 0)?.parse::<i64>()?;

        // 创建表
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;

        // 查询积分
        let ret_rst: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?",
            [group_id.clone(), user_id.clone()],
            |row| row.get(0),
        );
        let mut ret_num: i64;
        if let Ok(ret) = ret_rst {
            ret_num = ret;
        } else {
            ret_num = 0;
        }

        // 积分变动
        ret_num = ret_num
            .checked_add(add_score)
            .ok_or_else(|| RedLang::make_err("积分-增加：积分溢出"))?;
        if ret_num < 0 {
            ret_num = 0;
        }

        // 积分设置
        conn.execute(
            "REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)",
            [group_id, user_id, ret_num.to_string()],
        )?;

        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分-设置"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();
        let set_score = self_t.get_param_text(params, 0)?.parse::<u32>()?;

        // 创建表
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        conn.execute("CREATE TABLE IF NOT EXISTS SCORE_TABLE (GROUP_ID TEXT,USER_ID TEXT,VALUE INTEGER DEFAULT 0,PRIMARY KEY(GROUP_ID,USER_ID));", [])?;

        // 积分设置
        conn.execute(
            "REPLACE INTO SCORE_TABLE (GROUP_ID,USER_ID,VALUE) VALUES (?,?,?)",
            [group_id, user_id, set_score.to_string()],
        )?;

        Ok(Some(rv_empty()))
    });

    add_fun(vec!["积分"], |self_t, _params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);
        let user_id = self_t.get_exmap("发送者ID").to_string();

        // 查询积分
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        let ret_rst: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? AND USER_ID = ?",
            [group_id.clone(), user_id.clone()],
            |row| row.get(0),
        );
        let ret_num = ret_rst.unwrap_or(0);

        Ok(Some(rv_text(ret_num.to_string())))
    });

    add_fun(vec!["积分-排行"], |self_t, params| {
        let key1 = self_t.get_exmap("群ID");
        let group_id = format!("{}", key1);

        let limit = self_t.get_param_text(params, 0)?;
        let limit_num = if limit.is_empty() {
            10
        } else {
            limit.parse::<i32>()?
        };

        // 查询积分
        let app_dir = crate::cqexfun::get_app_dir(&self_t.pkg_name)?;
        let sql_file = app_dir + "reddat.db";
        let sql_file = redlang::mytool::path_to_os_str(&sql_file);

        add_file_lock(&sql_file);
        let _guard = scopeguard::guard(sql_file.clone(), |sql_file| {
            del_file_lock(&sql_file);
        });

        let conn = rusqlite::Connection::open(sql_file)?;

        redlang::resource_manager::configure_conn_busy_timeout(&conn);
        let mut stmt = conn.prepare(
            "SELECT USER_ID,VALUE FROM SCORE_TABLE WHERE GROUP_ID = ? ORDER BY VALUE DESC LIMIT ?",
        )?;
        let mut rows = stmt.query(rusqlite::params_from_iter([
            group_id,
            limit_num.to_string(),
        ]))?;
        let mut vec: Vec<Rc<RedValue>> = vec![];
        while let Some(row) = rows.next()? {
            let mut v: Vec<Rc<RedValue>> = vec![];
            for i in 0..2 {
                let k = row.get_ref_unwrap(i);
                let dat = match k.data_type() {
                    rusqlite::types::Type::Null => rv_empty(),
                    rusqlite::types::Type::Integer => rv_text(k.as_i64().unwrap_or(0).to_string()),
                    rusqlite::types::Type::Real => rv_text(k.as_f64().unwrap_or(0.0).to_string()),
                    rusqlite::types::Type::Text => rv_text(match k.as_str() {
                        Ok(s) => s.to_owned(),
                        Err(_) => {
                            String::from_utf8_lossy(k.as_bytes().unwrap_or(&[])).to_string()
                        }
                    }),
                    rusqlite::types::Type::Blob => rv_bin(k.as_blob().unwrap_or(&[]).to_vec()),
                };
                v.push(dat);
            }
            vec.push(Rc::new(RedValue::Array(v)));
        }
        Ok(Some(Rc::new(RedValue::Array(vec))))
    });
    add_fun(vec!["机器人平台"], |self_t, _params| {
        let platform = get_platform(self_t);
        Ok(Some(rv_text(platform)))
    });
    add_fun(vec!["主人"], |self_t, params| {
        let params_len = params.len();
        let master = if params_len == 0 {
            self_t.get_exmap("发送者ID").as_str().to_owned()
        } else {
            self_t.get_param_text(params, 0)?
        };
        let master_arr_value =
            get_const_val(&self_t.pkg_name, "0b863263-484c-4447-9990-0469186b3d97")?;
        if let Some(master_arr_value) = master_arr_value {
            let master_arr_rv = master_arr_value.into_rc_value()?;
            let is_master = match &*master_arr_rv {
                RedValue::Array(arr) => arr
                    .iter()
                    .any(|item| item.as_text() == Some(master.as_str())),
                RedValue::Text(text) => text.as_str() == master.as_str(),
                _ => false,
            };
            if !is_master {
                // 等同于【返回】
                let fun_ret_vec_len = self_t.fun_ret_vec.len();
                self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
            }
        } else {
            // 等同于【返回】
            let fun_ret_vec_len = self_t.fun_ret_vec.len();
            self_t.fun_ret_vec[fun_ret_vec_len - 1].0 = true;
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["主人数组"], |self_t, _params| {
        if let Some(master_arr_value) =
            get_const_val(&self_t.pkg_name, "0b863263-484c-4447-9990-0469186b3d97")?
        {
            return Ok(Some(master_arr_value.into_rc_value()?));
        }
        Ok(Some(rv_array(vec![])))
    });
    add_fun(vec!["设置主人"], |self_t, params| {
        let master_arr = self_t.get_param_array(params, 0)?;
        // 这里仅仅为了验证是否规范
        for it in &master_arr {
            if it.get_type_name() != "文本" {
                return Err("主人数组的元素必须为全为文本类型".into());
            }
        }
        let master_arr_value = RedValueData::from_red_value(&RedValue::Array(master_arr))?;
        crate::core::set_const_val(
            &self_t.pkg_name,
            "0b863263-484c-4447-9990-0469186b3d97",
            master_arr_value,
        )?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["重定向"], |self_t, params| {
        let plus_name = self_t.get_param_text(params, 0)?;
        let script_json = crate::core::scripts().read_scripts();
        for i in 0..script_json
            .as_array()
            .ok_or("script.json文件不是数组格式")?
            .len()
        {
            // 默认包可能没有包名，这个时候要认为是`""`
            let pkg_name = script_json[i]["pkg_name"].as_str().unwrap_or_default();
            let name = script_json[i]["name"].as_str().ok_or("name not str")?;
            if self_t.pkg_name != pkg_name || plus_name != name {
                continue;
            }
            let code = script_json[i]["content"]["code"]
                .as_str()
                .ok_or("脚本中code不是str")?
                .to_owned();
            let exmap = (*self_t.exmap).borrow().clone();
            let pkg_name = self_t.pkg_name.to_owned();
            let name = plus_name.to_owned();
            runtime().spawn_blocking(move || {
                let mut rl = RedLang::new();
                rl.exmap = Rc::new(RefCell::new(exmap));
                rl.pkg_name = pkg_name;
                rl.script_name = name;
                if let Err(e) = crate::cqevent::do_script(&mut rl, &code, "normal", false) {
                    let _ = cq_add_log_w(format!("err in do_group_msg:do_redlang:{}", e).as_str());
                }
            });
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["邮件主题"], |self_t, _params| {
        // 非 email 平台直接返回空
        let platform = get_platform(self_t);
        if platform != "email" {
            return Ok(Some(rv_empty()));
        }
        // 尝试从缓存获取
        let cached = self_t.get_exmap("邮件主题");
        if !(*cached).is_empty() {
            return Ok(Some(rv_text(cached.to_string())));
        }
        use base64::Engine;
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
        let raw_message_b64 = raw_json
            .get("raw_message")
            .and_then(|v| v.as_str())
            .ok_or("raw_message 不存在或不是字符串")?;
        let raw_bytes = base64::engine::general_purpose::STANDARD.decode(raw_message_b64)?;
        let parsed = mail_parser::MessageParser::default()
            .parse(&raw_bytes)
            .ok_or("邮件解析失败")?;
        let subject = parsed.subject().unwrap_or("").to_string();
        // 缓存结果
        self_t.set_exmap("邮件主题", &subject);
        Ok(Some(rv_text(subject)))
    });
}
