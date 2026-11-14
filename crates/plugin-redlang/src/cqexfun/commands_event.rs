use super::*;
use crate::bot_commands::send_options;

pub(super) fn register() {
    add_fun(vec!["分页"], |_self_t, _params| {
        Ok(Some(rv_text(PAGING_UUID.to_string())))
    });
    add_fun(vec!["设置来源"], |self_t, params| {
        let key_t = self_t.get_param_text(params, 0)?;
        let val = self_t.get_param_text(params, 1)?;
        let key = redlang::mytool::str_to_jt(&key_t);
        self_t.set_exmap(&key, &val);
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["发送私聊"], |self_t, params| {
        let msg_type: &'static str = crate::cqevent::get_msg_type(self_t);
        if msg_type != "group" {
            return Ok(Some(rv_empty()));
        }
        let msg = self_t.get_param_text(params, 0)?;
        let user_id = self_t.get_param_text(params, 1)?;
        let options = match get_platform(self_t).as_str() {
            "telegram" => send_options::parse(self_t, params, 2)?,
            "kook" => {
                let input = self_t.get_param(params, 2)?;
                crate::bot_commands::management::kook_message_options(self_t, &input, false)?
            }
            _ => serde_json::Value::Null,
        };
        let old_group_id = self_t.get_exmap("群ID"); // 备份群ID
        let old_user_id = self_t.get_exmap("发送者ID"); // 备份用户ID
        self_t.set_exmap("群ID", "");
        if !user_id.is_empty() {
            self_t.set_exmap("发送者ID", &user_id);
        }
        match send_one_msg_with_options(self_t, &msg, None, None, "", "", options) {
            Ok(msg_id) => {
                self_t.set_exmap("群ID", &old_group_id); // 还原群ID
                self_t.set_exmap("发送者ID", &old_user_id); // 还原用户ID
                Ok(Some(rv_text(msg_id)))
            }
            Err(e) => {
                self_t.set_exmap("群ID", &old_group_id); // 还原群ID
                self_t.set_exmap("发送者ID", &old_user_id); // 还原用户ID
                let _ = cq_add_log_w(&format!("输出流失败，{}", e));
                Ok(Some(rv_empty()))
            }
        }
    });
    add_fun(vec!["流式开始"], |self_t, params| {
        let content = self_t.get_param_text(params, 0)?;
        if !is_qq_c2c(self_t) {
            // 不支持流式的平台：返回随机会话ID，内容先缓冲，结束时整条发出
            let platform = get_platform(self_t);
            let message_type = self_t.get_exmap("消息类型").to_string();
            let _ = cq_add_log_w(&format!(
                "流式开始：当前会话不支持流式（platform=`{platform}`,消息类型=`{message_type}`），已降级为结束后整条发送"
            ));
            let session_id = uuid::Uuid::new_v4().to_string();
            self_t.set_exmap(&stream_buffer_key(&session_id), &content);
            return Ok(Some(rv_text(session_id)));
        }
        let user_id = self_t.get_exmap("发送者ID");
        let action_json = serde_json::json!({
            "action":"send_stream_msg",
            "params":{
                "mode":"start",
                "user_id":(*user_id).clone(),
                "content":content
            }
        });
        let (retcode, cq_ret) = call_stream_action(self_t, action_json)?;
        if retcode != 0 {
            return Ok(Some(rv_text(String::new())));
        }
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let session_id = ret_json
            .get("data")
            .and_then(|data| data.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        // 分片序号存入 exmap 的内部键（脚本不可见）
        let key = stream_index_key(session_id);
        self_t.set_exmap(&key, "1");
        Ok(Some(rv_text(session_id.to_owned())))
    });
    add_fun(vec!["流式发送"], |self_t, params| {
        let session_id = self_t.get_param_text(params, 0)?;
        let content = self_t.get_param_text(params, 1)?;
        if !is_qq_c2c(self_t) {
            if !session_id.is_empty() && !content.is_empty() {
                let key = stream_buffer_key(&session_id);
                let mut text = self_t.get_exmap(&key).to_string();
                text.push_str(&content);
                self_t.set_exmap(&key, &text);
            }
            return Ok(Some(rv_empty()));
        }
        if session_id.is_empty() {
            return Ok(Some(rv_empty()));
        }
        let key = stream_index_key(&session_id);
        let index: u64 = self_t.get_exmap(&key).parse().unwrap_or(1);
        self_t.set_exmap(&key, &(index + 1).to_string());
        let user_id = self_t.get_exmap("发送者ID");
        let action_json = serde_json::json!({
            "action":"send_stream_msg",
            "params":{
                "mode":"append",
                "session_id":session_id,
                "user_id":(*user_id).clone(),
                "content":content,
                "index":index
            }
        });
        let _ = call_stream_action(self_t, action_json)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["流式结束"], |self_t, params| {
        let session_id = self_t.get_param_text(params, 0)?;
        let content = self_t.get_param_text(params, 1)?;
        if !is_qq_c2c(self_t) {
            // 不支持流式的平台：把缓冲内容与最后一片拼接后整条发出
            let key = stream_buffer_key(&session_id);
            let mut text = self_t.get_exmap(&key).to_string();
            (*self_t.exmap).borrow_mut().remove(&key);
            text.push_str(&content);
            if !text.is_empty() {
                match send_one_msg(self_t, &text) {
                    Ok(msg_id) => return Ok(Some(rv_text(msg_id))),
                    Err(e) => {
                        let _ = cq_add_log_w(&format!("输出流失败，{}", e));
                    }
                }
            }
            return Ok(Some(rv_empty()));
        }
        let key = stream_index_key(&session_id);
        let index: u64 = self_t.get_exmap(&key).parse().unwrap_or(1);
        (*self_t.exmap).borrow_mut().remove(&key);
        let user_id = self_t.get_exmap("发送者ID");
        let action_json = serde_json::json!({
            "action":"send_stream_msg",
            "params":{
                "mode":"end",
                "session_id":session_id,
                "user_id":(*user_id).clone(),
                "content":content,
                "index":index
            }
        });
        let (_, cq_ret) = call_stream_action(self_t, action_json)?;
        let message_id = serde_json::from_str::<serde_json::Value>(&cq_ret)
            .ok()
            .and_then(|value| {
                value
                    .get("data")
                    .and_then(|data| data.get("message_id"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        Ok(Some(rv_text(message_id)))
    });
    add_fun(vec!["清空"], |_self_t, _params| {
        Ok(Some(rv_text(CLEAR_UUID.to_string())))
    });
    add_fun(vec!["获取消息"], |self_t, params| {
        let msg_id = self_t.get_param_text(params, 0)?;
        let send_json = serde_json::json!({
            "action":"get_msg",
            "params":{
                "message_id":msg_id
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            &send_json.to_string(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取消息失败:{ret_json}");
        let raw_message = redlang::mytool::json_to_cq_str(ret_json.get("data").ok_or(err)?)?;
        Ok(Some(rv_text(raw_message)))
    });
    add_fun(vec!["当前消息"], |self_t, _params| {
        let msg = self_t.get_exmap("当前消息");
        if (*msg).is_empty() {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            if let Ok(message) = json_to_cq_str(&raw_json) {
                return Ok(Some(rv_text(message)));
            }
        }
        Ok(Some(rv_text(msg.to_string())))
    });
    add_fun(vec!["输入流"], |self_t, params| {
        let tm = self_t.get_param_text(params, 0)?;
        let tm = tm.parse::<u64>().unwrap_or(15000);
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (
            std::sync::mpsc::Sender<String>,
            std::sync::mpsc::Receiver<String>,
        ) = std::sync::mpsc::channel();
        let ip = redlang::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            echo: echo.clone(),
            stream_type: "输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        runtime_context().session.add_input_stream(ip);
        let _guard = scopeguard::guard(echo, |echo| {
            runtime_context().session.remove_input_stream(&echo);
        });
        let mut tm = tm;
        while tm > 1000 {
            if !pkg_can_run(&self_t.pkg_name, "输入流") {
                return Err("输入流终止，因用户要求退出".into());
            }
            let rv = rx.recv_timeout(std::time::Duration::from_secs(1));
            if let Ok(msg) = rv {
                return Ok(Some(rv_text(msg)));
            }
            tm -= 1000;
        }
        if !pkg_can_run(&self_t.pkg_name, "输入流") {
            return Err("输入流终止，因用户要求退出".into());
        }
        let rv = rx.recv_timeout(std::time::Duration::from_millis(tm));
        if let Ok(msg) = rv {
            return Ok(Some(rv_text(msg)));
        }
        let ret_str = String::new();
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["群输入流"], |self_t, params| {
        let tm = self_t.get_param_text(params, 0)?;
        let tm = tm.parse::<u64>().unwrap_or(15000);
        let self_id = self_t.get_exmap("机器人ID");
        let group_id = self_t.get_exmap("群ID");
        let user_id = self_t.get_exmap("发送者ID");
        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx): (
            std::sync::mpsc::Sender<String>,
            std::sync::mpsc::Receiver<String>,
        ) = std::sync::mpsc::channel();
        let ip = redlang::InputStream {
            self_id: self_id.to_string(),
            group_id: group_id.to_string(),
            user_id: user_id.to_string(),
            echo: echo.clone(),
            stream_type: "群输入流".to_owned(),
            tx: Some(Arc::new(std::sync::Mutex::new(tx))),
        };
        runtime_context().session.add_input_stream(ip);
        let _guard = scopeguard::guard(echo, |echo| {
            runtime_context().session.remove_input_stream(&echo);
        });
        let mut ret_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        let mut tm = tm;
        while tm > 1000 {
            if !pkg_can_run(&self_t.pkg_name, "输入流") {
                return Err("输入流终止，因用户要求退出".into());
            }
            let rv = rx.recv_timeout(std::time::Duration::from_secs(1));
            if let Ok(msg) = rv {
                let js: serde_json::Value = serde_json::from_str(&msg)?;
                let js_obj = js.as_object().ok_or("解析JSON失败")?;
                let mut mp: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
                mp.insert(
                    "发送者ID".to_string(),
                    rv_text(
                        js_obj["发送者ID"]
                            .as_str()
                            .ok_or("解析JSON失败")?
                            .to_owned(),
                    ),
                );
                mp.insert(
                    "消息".to_string(),
                    rv_text(js_obj["消息"].as_str().ok_or("解析JSON失败")?.to_owned()),
                );
                mp.insert(
                    "消息ID".to_string(),
                    rv_text(js_obj["消息ID"].as_str().ok_or("解析JSON失败")?.to_owned()),
                );
                return Ok(Some(rv_object(mp)));
            }
            tm -= 1000;
        }
        if !pkg_can_run(&self_t.pkg_name, "输入流") {
            return Err("输入流终止，因用户要求退出".into());
        }
        let rv = rx.recv_timeout(std::time::Duration::from_millis(tm));
        if let Ok(msg) = rv {
            let js: serde_json::Value = serde_json::from_str(&msg)?;
            let js_obj = js.as_object().ok_or("解析JSON失败")?;
            ret_obj.insert(
                "发送者ID".to_string(),
                rv_text(
                    js_obj["发送者ID"]
                        .as_str()
                        .ok_or("解析JSON失败")?
                        .to_owned(),
                ),
            );
            ret_obj.insert(
                "消息".to_string(),
                rv_text(js_obj["消息"].as_str().ok_or("解析JSON失败")?.to_owned()),
            );
            ret_obj.insert(
                "消息ID".to_string(),
                rv_text(js_obj["消息ID"].as_str().ok_or("解析JSON失败")?.to_owned()),
            );
        }
        Ok(Some(rv_object(ret_obj)))
    });
}
