use super::*;
use crate::bot_commands::send_options;

pub(super) fn register() {
    add_fun(vec!["禁言"], |self_t, params| {
        let ban_time = self_t.get_param_text(params, 0)?;
        let user_id = self_t.get_exmap("发送者ID");
        let group_id = self_t.get_exmap("群ID");
        let params = serde_json::json!({
            "group_id": group_id.as_str(), "user_id": user_id.as_str(),
            "duration": ban_time.parse::<usize>()?,
        });
        if get_platform(self_t) == "telegram" {
            let result = call_platform_api(self_t, "telegram", "set_group_ban", params)?;
            return check_native_command(result, "Telegram 禁言");
        }
        let send_json = serde_json::json!({"action":"set_group_ban","params":params});
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let _cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["置顶消息"], |rl, params| {
        let platform = get_platform(rl);
        if !matches!(platform.as_str(), "kook" | "telegram") {
            return Err(RedLang::make_err(
                "【置顶消息】需要 KOOK 或 Telegram 机器人来源",
            ));
        }
        let mut id = rl.get_param_text(params, 0)?;
        if id.is_empty() {
            id = rl.get_exmap("消息ID").to_string();
        }
        let remove = match rl.get_param_text(params, 1)?.as_str() {
            "" | "真" => false,
            "假" => true,
            _ => return Err(RedLang::make_err("置顶参数需为真或假")),
        };
        let silent = rl.get_param_text(params, 2)? == "真";
        let result = call_platform_api(
            rl,
            &platform,
            "set_msg_pin",
            serde_json::json!({
                "message_id":id,"remove":remove,"silent":silent,
            }),
        )?;
        check_native_command(result, "置顶消息")
    });
    add_fun(vec!["表情回应"], |self_t, params| {
        let emoji_id = self_t.get_param_text(params, 0)?;
        let mut message_id = self_t.get_param_text(params, 1)?;
        if message_id.is_empty() {
            message_id = self_t.get_exmap("消息ID").to_string();
        }
        let platform = get_platform(self_t);
        if matches!(platform.as_str(), "kook" | "telegram") {
            let remove = self_t.get_param_text(params, 2)? == "假";
            let kind = self_t.get_param_text(params, 3)?;
            if !matches!(kind.as_str(), "" | "普通" | "自定义") {
                return Err(RedLang::make_err("回应类型需填普通或自定义"));
            }
            let request = serde_json::json!({
                "message_id": message_id,
                "group_id": self_t.get_exmap("群ID").as_str(),
                "emoji": emoji_id,
                "remove": remove,
                "custom": kind == "自定义",
                "is_big": self_t.get_param_text(params, 4)? == "真",
                "user_id": self_t.get_param_text(params, 5)?,
                "actor_chat_id": self_t.get_param_text(params, 6)?,
            });
            let result = call_platform_api(self_t, &platform, "set_msg_emoji_like", request)?;
            if let Some(result) = result {
                if let RedValue::Object(fields) = result.as_ref() {
                    if fields
                        .get("retcode")
                        .and_then(|value| value.expect_text_value().ok())
                        .as_deref()
                        != Some("0")
                    {
                        let message = fields
                            .get("message")
                            .and_then(|value| value.expect_text_value().ok())
                            .unwrap_or_default();
                        let _ = cq_add_log_w(&format!("表情回应失败：{message}"));
                    }
                }
            }
            return Ok(Some(rv_empty()));
        }
        let emoji_num;
        if let Ok(emoji_id_t) = emoji_id.parse::<i32>() {
            emoji_num = emoji_id_t;
        } else {
            let emojis = emoji_id.chars().collect::<Vec<char>>();
            if emojis.len() != 1 {
                let _ = cq_add_log_w("表情回应失败，表情ID错误1");
                return Ok(Some(rv_empty()));
            }
            let emoji: char = emojis[0];
            emoji_num = emoji as i32;
        }
        let group_id = self_t.get_exmap("群ID");
        let send_json = serde_json::json!({
            "action":"set_msg_emoji_like",
            "params":{
                "message_id": *message_id,
                "emoji_id": emoji_num.to_string(),
                "group_id": *group_id
            }
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let _cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["输出流"], |self_t, params| {
        let msg = self_t.get_param_text(params, 0)?;
        let options = match get_platform(self_t).as_str() {
            "telegram" => send_options::parse(self_t, params, 6)?,
            "kook" => {
                let input = self_t.get_param(params, 1)?;
                crate::bot_commands::management::kook_message_options(self_t, &input, false)?
            }
            _ => serde_json::Value::Null,
        };
        let markup = if get_platform(self_t) == "telegram" && params.len() > 1 {
            let value = self_t.get_param(params, 1)?;
            match value.as_ref() {
                RedValue::Text(text) if text.is_empty() => None,
                _ => Some(crate::bot_commands::telegram_keyboard_value(&value)?),
            }
        } else {
            None
        };
        let topic = if get_platform(self_t) == "telegram" && params.len() > 2 {
            let id = self_t.get_param_text(params, 2)?;
            let kind = self_t.get_param_text(params, 3)?;
            telegram_output_topic(&id, &kind)?
        } else {
            None
        };
        let (connection, peer) = if get_platform(self_t) == "telegram" {
            (
                self_t.get_param_text(params, 4)?,
                self_t.get_param_text(params, 5)?,
            )
        } else {
            (String::new(), String::new())
        };
        match send_one_msg_with_options(self_t, &msg, markup, topic, &connection, &peer, options) {
            Ok(msg_id) => Ok(Some(rv_text(msg_id))),
            Err(e) => {
                let _ = cq_add_log_w(&format!("输出流失败，{}", e));
                Ok(Some(rv_empty()))
            }
        }
    });
    add_fun(vec!["艾特"], |self_t, params| {
        let mut user_id = self_t.get_param_text(params, 0)?;
        if user_id.is_empty() {
            user_id = self_t.get_exmap("发送者ID").to_string();
        }
        if user_id.is_empty() {
            Ok(Some(rv_empty()))
        } else {
            Ok(Some(rv_text(format!(
                "[CQ:at,qq={}]",
                cq_params_encode(&user_id)
            ))))
        }
    });
    add_fun(vec!["合并转发"], |self_t, params| {
        let param0 = self_t.get_param(params, 0)?;
        let tp = param0.get_type_name();
        let mut ret_str = String::new();
        if tp == "数组" {
            let arr = param0.as_array().ok_or("合并转发:第1个参数不是数组")?;
            for it in arr {
                let content = it.expect_text_value()?;
                if content.is_empty() {
                    continue;
                }
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
        } else {
            let content = param0.expect_text_value()?;
            if !content.is_empty() {
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
            for i in 1..params.len() {
                let content = self_t.get_param_text(params, i)?;
                if content.is_empty() {
                    continue;
                }
                ret_str.push_str(&format!("[CQ:node,content={}]", cq_params_encode(&content)));
            }
        }
        if ret_str.is_empty() {
            return Ok(Some(rv_empty()));
        }
        Ok(Some(rv_text(ret_str)))
    });
    add_fun(vec!["CQ码转义"], |self_t, params| {
        let cq_code = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(cq_params_encode(&cq_code))))
    });
    add_fun(vec!["CQ转义"], |self_t, params| {
        let cq_code = self_t.get_param_text(params, 0)?;
        Ok(Some(rv_text(cq_text_encode(&cq_code))))
    });
    add_fun(vec!["子关键词"], |self_t, _params| {
        return Ok(Some(self_t.get_exmap_value("子关键词")?));
    });
    add_fun(vec!["事件内容"], |self_t, params| {
        let raw_data = self_t.get_exmap("原始事件");
        let raw_json = serde_json::from_str(&raw_data)?;
        let field = self_t.get_param_text(params, 0)?;
        if field.is_empty() {
            Ok(Some(json_value_to_red(&raw_json)?))
        } else {
            crate::bot_commands::event_field(&raw_json, &field)
        }
    });
    add_fun(vec!["OB调用"], |self_t, params| {
        let input = self_t.get_param(params, 0)?;
        let content = match input.as_ref() {
            RedValue::Text(text) => text.as_ref().clone(),
            _ => native_args::object(&input)?.to_string(),
        };
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let call_ret = cq_call_api(&platform, &self_id, &passive_id, &content, &remote_id);
        let js_v = serde_json::from_str(&call_ret)?;
        return Ok(Some(json_value_to_red(&js_v)?));
    });
    add_fun(vec!["接口值"], |rl, params| {
        let kind = rl.get_param_text(params, 0)?;
        let value = rl.get_param_text(params, 1)?;
        Ok(Some(native_args::scalar(&kind, &value)?))
    });
    add_fun(vec!["KOOK卡片"], |rl, params| {
        let value = rl.get_param(params, 0)?;
        let cards = crate::bot_commands::card_args(rl, params, &value)?;
        let content = cq_params_encode(&cards.to_string());
        Ok(Some(rv_text(format!("[CQ:kook_card,content={content}]"))))
    });
    add_fun(vec!["Telegram调用"], |rl, params| {
        let method = rl.get_param_text(params, 0)?;
        let body = native_args::body(rl, params, 1)?;
        let mut request = serde_json::json!({"method": method, "body": body});
        if let Some(files) = native_args::files(rl, params, 2)? {
            request["files"] = files;
        }
        call_platform_api(rl, "telegram", "telegram_api", request)
    });
    add_fun(vec!["Telegram下载文件"], |rl, params| {
        let file_id = rl.get_param_text(params, 0)?;
        let response = call_platform_api(
            rl,
            "telegram",
            "telegram_download",
            serde_json::json!({"file_id": file_id}),
        )?
        .ok_or("Telegram 下载未返回响应")?;
        let RedValue::Object(fields) = response.as_ref() else {
            return Err(RedLang::make_err("Telegram 下载返回格式错误"));
        };
        if fields
            .get("retcode")
            .and_then(|value| value.expect_text_value().ok())
            .as_deref()
            != Some("0")
        {
            let message = fields
                .get("message")
                .and_then(|value| value.expect_text_value().ok())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "Telegram 下载失败".to_owned());
            return Err(RedLang::make_err(&message));
        }
        let Some(RedValue::Object(data)) = fields.get("data").map(Rc::as_ref) else {
            return Err(RedLang::make_err("Telegram 下载未返回文件"));
        };
        let base64 = data
            .get("base64")
            .ok_or("Telegram 下载未返回文件")?
            .expect_text_value()?;
        Ok(Some(rv_bin(BASE64_CUSTOM_ENGINE.decode(base64)?)))
    });
    add_fun(vec!["KOOK调用"], |rl, params| {
        let method = rl.get_param_text(params, 0)?.to_ascii_uppercase();
        let endpoint = rl.get_param_text(params, 1)?;
        let body = native_args::body(rl, params, 2)?;
        let mut request =
            serde_json::json!({"http_method": method, "endpoint": endpoint, "body": body});
        if let Some(files) = native_args::files(rl, params, 3)? {
            request["files"] = files;
        }
        call_platform_api(rl, "kook", "kook_api", request)
    });
    add_fun(vec!["KOOK共享播放"], |rl, params| {
        let channel = rl.get_param_text(params, 0)?;
        let source = rl.get_param(params, 1)?;
        let volume = rl.get_param_text(params, 2)?;
        let password = rl.get_param_text(params, 3)?;
        let volume = if volume.is_empty() {
            100
        } else {
            volume.parse::<u64>()?
        };
        let mut request = serde_json::json!({"channel_id":channel,"volume":volume});
        match source.as_ref() {
            RedValue::Array(items) => {
                let sources = items
                    .iter()
                    .map(|item| item.expect_text_value())
                    .collect::<Result<Vec<_>, _>>()?;
                request["sources"] = serde_json::json!(sources);
            }
            _ => request["source"] = source.expect_text_value()?.into(),
        }
        if !password.is_empty() {
            request["password"] = password.into();
        }
        call_platform_api(rl, "kook", "kook_voice_play", request)
    });
    add_fun(vec!["KOOK停止播放"], |rl, _| {
        call_platform_api(rl, "kook", "kook_voice_stop", serde_json::json!({}))
    });
    add_fun(vec!["KOOK播放状态"], |rl, params| {
        let field = rl.get_param_text(params, 0)?;
        if field.is_empty() {
            return call_platform_api(rl, "kook", "kook_voice_status", serde_json::json!({}));
        }
        let response =
            call_platform_api_value(rl, "kook", "kook_voice_status", serde_json::json!({}))?;
        if response["retcode"] != 0 {
            return Err(RedLang::make_err(
                response["message"].as_str().unwrap_or("无法查询播放状态"),
            ));
        }
        let data = &response["data"];
        let value = match field.as_str() {
            "状态" => crate::bot_commands::playback_state(&data["state"]),
            "当前曲目" => data["track"].clone(),
            "曲目总数" => data["total"].clone(),
            "频道ID" => data["channel_id"].clone(),
            "错误" => data["error"].clone(),
            _ => {
                return Err(RedLang::make_err(
                    "播放状态可查询状态、当前曲目、曲目总数、频道ID或错误",
                ))
            }
        };
        Ok(Some(json_value_to_red(&value)?))
    });
}
