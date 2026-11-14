use super::*;
use redlang::mytool::cq_params_encode;

const RIGHTS: &[(&str, &str)] = &[
    ("回复和编辑消息", "can_reply"),
    ("标记消息已读", "can_read_messages"),
    ("删除机器人发送的消息", "can_delete_sent_messages"),
    ("删除会话消息", "can_delete_all_messages"),
    ("修改姓名", "can_edit_name"),
    ("修改简介", "can_edit_bio"),
    ("修改头像", "can_edit_profile_photo"),
    ("修改用户名", "can_edit_username"),
    ("修改礼物设置", "can_change_gift_settings"),
    ("查看礼物和星币", "can_view_gifts_and_stars"),
    ("转换礼物", "can_convert_gifts_to_stars"),
    ("转移和升级礼物", "can_transfer_and_upgrade_gifts"),
    ("转移星币", "can_transfer_stars"),
    ("管理动态", "can_manage_stories"),
];

const GIFT_TYPES: &[(&str, &str)] = &[
    ("不限量", "unlimited_gifts"),
    ("限量", "limited_gifts"),
    ("独特礼物", "unique_gifts"),
    ("会员订阅", "premium_subscription"),
    ("频道转入礼物", "gifts_from_channels"),
];

const GIFT_FILTERS: &[(&str, &str)] = &[
    ("未展示", "exclude_unsaved"),
    ("已展示", "exclude_saved"),
    ("不限量", "exclude_unlimited"),
    ("可升级限量", "exclude_limited_upgradable"),
    ("不可升级限量", "exclude_limited_non_upgradable"),
    ("独特礼物", "exclude_unique"),
    ("链上礼物", "exclude_from_blockchain"),
];

pub(crate) fn event_message(event: &Value) -> &Value {
    let native = &event["telegram"];
    native.get("message").unwrap_or(native)
}

pub(crate) fn connection_id(event: &Value) -> Option<&str> {
    let value = if event["sub_type"] == "business_connection" {
        &event["telegram"]["id"]
    } else {
        &event_message(event)["business_connection_id"]
    };
    value.as_str().filter(|id| !id.is_empty())
}

pub(crate) fn event_context(rl: &RedLang) -> Option<Value> {
    let event: Value = serde_json::from_str(&rl.get_exmap("原始事件")).ok()?;
    (event["platform"] == "telegram"
        && !rl.get_exmap("机器人ID").is_empty()
        && event["self_id"].as_str() == Some(rl.get_exmap("机器人ID").as_str()))
    .then_some(event)
}

fn matching_chat(event: &Value, target: &Value) -> bool {
    let chat = &event_message(event)["chat"];
    match target {
        Value::Number(_) => &chat["id"] == target,
        Value::String(username) => chat["username"].as_str().is_some_and(|name| {
            username
                .strip_prefix('@')
                .is_some_and(|target| name.eq_ignore_ascii_case(target))
        }),
        _ => false,
    }
}

pub(crate) fn send_context(rl: &RedLang, target: &Value, explicit: &str) -> Option<String> {
    if explicit == "机器人" {
        return None;
    }
    if !explicit.is_empty() {
        return Some(explicit.to_owned());
    }
    let event = event_context(rl)?;
    if matching_chat(&event, target) {
        connection_id(&event).map(str::to_owned)
    } else {
        None
    }
}

pub(crate) fn default_peer(rl: &RedLang) -> Option<i64> {
    let event = event_context(rl)?;
    connection_id(&event)?;
    if event["user_id"].as_str().unwrap_or_default() != rl.get_exmap("发送者ID").as_str() {
        return None;
    }
    let chat = &event_message(&event)["chat"];
    if chat["type"] != "private" {
        return None;
    }
    chat["id"].as_i64()
}

pub(crate) fn connection(rl: &RedLang, explicit: &str) -> Result<String> {
    if !explicit.is_empty() {
        return Ok(explicit.to_owned());
    }
    event_context(rl)
        .as_ref()
        .and_then(connection_id)
        .map(str::to_owned)
        .ok_or_else(|| "请填写商务连接ID，或在当前机器人的 Telegram 商务事件中调用".into())
}

pub(crate) fn connection_fields(data: &Value) -> Value {
    let user = &data["user"];
    json!({
        "商务连接ID":data["id"],"账户用户ID":user["id"],"账户私聊ID":data["user_chat_id"],
        "姓名":user["first_name"],"姓氏":user["last_name"],"用户名":user["username"],
        "连接时间":data["date"],"已启用":data["is_enabled"],"权限":read_rights(&data["rights"],RIGHTS)
    })
}

fn checked_length(text: &str, min: usize, max: usize, label: &str) -> Result<()> {
    if !(min..=max).contains(&text.chars().count()) {
        return Err(format!("{label}请填写 {min} 至 {max} 字").into());
    }
    Ok(())
}

pub(crate) fn checklist(input: &RedValue) -> Result<Value> {
    let data = array(input)?;
    if !(2..=4).contains(&data.len()) {
        return Err("清单请填【数组@标题@任务数组@允许添加@允许勾选】".into());
    }
    let title = row_text(data, 0)?;
    checked_length(&title, 1, 255, "清单标题")?;
    let tasks = array(&data[1])?;
    if !(1..=30).contains(&tasks.len()) {
        return Err("清单需要 1 至 30 项任务".into());
    }
    let mut ids = HashSet::new();
    let tasks = tasks
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let (id, text) = match item.as_ref() {
                RedValue::Text(text) => ((index + 1) as i64, text.as_str().to_owned()),
                RedValue::Array(row) if row.len() == 2 => {
                    (positive_id(&row_text(row, 0)?)?, row_text(row, 1)?)
                }
                _ => return Err("任务请填文字或【数组@任务ID@文字】".into()),
            };
            checked_length(&text, 1, 100, "任务文字")?;
            if !ids.insert(id) {
                return Err("同一清单上的任务ID必须唯一".into());
            }
            Ok(json!({"id":id,"text":text}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"title":title,"tasks":tasks,
        "others_can_add_tasks":boolean(&row_text(data,2)?,false)?,
        "others_can_mark_tasks_as_done":boolean(&row_text(data,3)?,false)?}))
}

pub(crate) fn checklist_tasks(items: &Value) -> Value {
    json!(items.as_array().into_iter().flatten().map(|task| json!({"任务ID":task["id"],
        "文字":task["text"],"完成时间":task["completion_date"],"完成人ID":task["completed_by_user"]["id"]}))
        .collect::<Vec<_>>())
}

fn business_account(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let operation = rl.get_param_text(params, 0)?;
    if operation == "头像" {
        let source = rl.get_param(params, 1)?;
        let [filename, kind, public, id, cover] = text_params::<5>(rl, params, 2)?;
        let (kind, field) = match kind.as_str() {
            "" | "静态" => ("static", "photo"),
            "动态" => ("animated", "animation"),
            _ => return Err("商务头像类型请填静态或动态".into()),
        };
        let mut photo = json!({"type":kind,field:"attach://profile_file"});
        if !cover.is_empty() {
            if kind != "animated" {
                return Err("封面秒数只用于动态头像".into());
            }
            let seconds: f64 = cover.parse()?;
            if !seconds.is_finite() || seconds < 0.0 {
                return Err("封面秒数需要非负数字".into());
            }
            photo["main_frame_timestamp"] = seconds.into();
        }
        let body = json!({"business_connection_id":connection(rl,&id)?,
            "photo":photo,"is_public":boolean(&public,false)?});
        upload_api(
            rl,
            "telegram",
            "setBusinessAccountProfilePhoto",
            body,
            json!({"profile_file":upload(&source,&filename)?}),
        )?;
        return empty();
    }
    let [first, second, third] = text_params::<3>(rl, params, 1)?;
    let (method, body) = match operation.as_str() {
        "" | "查询" => {
            let data = telegram(
                rl,
                "getBusinessConnection",
                json!({"business_connection_id":connection(rl,&first)?}),
            )?;
            return output(&connection_fields(&data["data"]));
        }
        "姓名" => {
            checked_length(&first, 1, 64, "名字")?;
            checked_length(&second, 0, 64, "姓氏")?;
            (
                "setBusinessAccountName",
                json!({"business_connection_id":connection(rl,&third)?,
                "first_name":first,"last_name":second}),
            )
        }
        "用户名" | "简介" => {
            let (method, field, max) = if operation == "用户名" {
                ("setBusinessAccountUsername", "username", 32)
            } else {
                ("setBusinessAccountBio", "bio", 140)
            };
            checked_length(&first, 0, max, &operation)?;
            (
                method,
                json!({"business_connection_id":connection(rl,&second)?,field:first}),
            )
        }
        "移除头像" => (
            "removeBusinessAccountProfilePhoto",
            json!({"business_connection_id":connection(rl,&second)?,
            "is_public":boolean(&first,false)?}),
        ),
        _ => return Err("商务账户操作请填查询、姓名、用户名、简介、头像或移除头像".into()),
    };
    telegram(rl, method, body)?;
    empty()
}

fn story_period(hours: &str) -> Result<i64> {
    let hours = if hours.is_empty() {
        24
    } else {
        bounded(hours, 6, 48, "动态有效小时")?
    };
    if !matches!(hours, 6 | 12 | 24 | 48) {
        return Err("动态有效期请填 6、12、24 或 48 小时".into());
    }
    Ok(hours * 3600)
}

fn business_story(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let operation = rl.get_param_text(params, 0)?;
    match operation.as_str() {
        "发布" | "编辑" => {
            let edit = operation == "编辑";
            let start = if edit { 2 } else { 1 };
            let source = rl.get_param(params, start)?;
            let [filename, kind, caption, id] = text_params::<4>(rl, params, start + 1)?;
            let kind = match kind.as_str() {
                "" | "图片" => "photo",
                "视频" => "video",
                _ => return Err("动态类型请填图片或视频".into()),
            };
            checked_length(&caption, 0, 2048, "动态说明")?;
            let mut body = json!({"business_connection_id":connection(rl,&id)?,
                "content":{"type":kind,kind:"attach://story_file"}});
            let method = if edit {
                body["story_id"] = positive_id(&rl.get_param_text(params, 1)?)?.into();
                body["caption"] = caption.into();
                "editStory"
            } else {
                let [hours, keep, protect] = text_params::<3>(rl, params, 6)?;
                body["active_period"] = story_period(&hours)?.into();
                body["post_to_chat_page"] = boolean(&keep, false)?.into();
                body["protect_content"] = boolean(&protect, false)?.into();
                insert_text(&mut body, "caption", &caption);
                "postStory"
            };
            let data = upload_api(
                rl,
                "telegram",
                method,
                body,
                json!({"story_file":upload(&source,&filename)?}),
            )?;
            scalar_text(&data["data"]["id"])
        }
        "转发" => {
            let [chat, story, hours, id, keep, protect] = text_params::<6>(rl, params, 1)?;
            let data = telegram(
                rl,
                "repostStory",
                json!({"business_connection_id":connection(rl,&id)?,
                "from_chat_id":positive_id(&chat)?,"from_story_id":positive_id(&story)?,
                "active_period":story_period(&hours)?,"post_to_chat_page":boolean(&keep,false)?,
                "protect_content":boolean(&protect,false)?}),
            )?;
            scalar_text(&data["data"]["id"])
        }
        "删除" => {
            let [story, id] = text_params::<2>(rl, params, 1)?;
            telegram(
                rl,
                "deleteStory",
                json!({"business_connection_id":connection(rl,&id)?,"story_id":positive_id(&story)?}),
            )?;
            empty()
        }
        _ => Err("商务动态操作请填发布、编辑、转发或删除".into()),
    }
}

fn business_gifts(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let operation = rl.get_param_text(params, 0)?;
    let mut body = json!({});
    let (method, id) = match operation.as_str() {
        "列表" => {
            let [id, offset, limit, sort] = text_params::<4>(rl, params, 1)?;
            let filters = rl.get_param(params, 5)?;
            insert_text(&mut body, "offset", &offset);
            if !limit.is_empty() {
                body["limit"] = bounded(&limit, 1, 100, "每页数量")?.into();
            }
            body["sort_by_price"] = match sort.as_str() {
                "" | "日期" => false,
                "价格" => true,
                _ => return Err("礼物排序请填日期或价格".into()),
            }
            .into();
            if !matches!(filters.as_ref(), RedValue::Text(text) if text.is_empty()) {
                let mask = rights(&filters, GIFT_FILTERS)?;
                if let Some(fields) = mask.as_object() {
                    for (key, value) in fields {
                        if value == true {
                            body[key] = true.into();
                        }
                    }
                }
            }
            ("getBusinessAccountGifts", id)
        }
        "接收设置" => {
            let [show] = text_params::<1>(rl, params, 1)?;
            if show.is_empty() {
                return Err("请填写是否显示礼物按钮，填真或假".into());
            }
            body["show_gift_button"] = boolean(&show, false)?.into();
            body["accepted_gift_types"] = rights(rl.get_param(params, 2)?.as_ref(), GIFT_TYPES)?;
            (
                "setBusinessAccountGiftSettings",
                rl.get_param_text(params, 3)?,
            )
        }
        "余额" => (
            "getBusinessAccountStarBalance",
            rl.get_param_text(params, 1)?,
        ),
        "转星币" => {
            let [count, id] = text_params::<2>(rl, params, 1)?;
            body["star_count"] = bounded(&count, 1, 10000, "星币数")?.into();
            ("transferBusinessAccountStars", id)
        }
        "转换" | "删除" => {
            let [gift, id] = text_params::<2>(rl, params, 1)?;
            body["owned_gift_id"] = required(&gift, "持有礼物ID")?.into();
            (
                if operation == "转换" {
                    "convertGiftToStars"
                } else {
                    "deleteBusinessAccountGift"
                },
                id,
            )
        }
        "升级" | "转送" => {
            let [gift, option, amount, id] = text_params::<4>(rl, params, 1)?;
            body["owned_gift_id"] = required(&gift, "持有礼物ID")?.into();
            if !amount.is_empty() {
                body["star_count"] = bounded(&amount, 0, i64::MAX, "星币数")?.into();
            }
            if operation == "升级" {
                body["keep_original_details"] = boolean(&option, false)?.into();
                ("upgradeGift", id)
            } else {
                body["new_owner_chat_id"] = positive_id(&option)?.into();
                ("transferGift", id)
            }
        }
        _ => {
            return Err(
                "商务礼物操作请填列表、接收设置、余额、转星币、转换、升级、转送或删除".into(),
            )
        }
    };
    body["business_connection_id"] = connection(rl, &id)?.into();
    let data = telegram(rl, method, body)?;
    let data = &data["data"];
    match operation.as_str() {
        "列表" => {
            let gifts = data["gifts"].as_array().ok_or("平台未返回礼物列表")?;
            let gifts = gifts.iter().map(|gift| json!({
                "持有礼物ID":gift["owned_gift_id"],"类型":if gift["type"] == "unique" { "独特礼物" } else { "普通礼物" },
                "礼物ID":gift["gift"]["id"],"名称":gift["gift"]["name"],"发送者ID":gift["sender_user"]["id"],
                "说明":gift["text"],"发送时间":gift["send_date"],"已展示":gift["is_saved"] == true,
                "可转换星币":gift["convert_star_count"],"升级所需星币":gift["gift"]["upgrade_star_count"],
                "已预付升级星币":gift["prepaid_upgrade_star_count"],"转送所需星币":gift["transfer_star_count"]
            })).collect::<Vec<_>>();
            output(
                &json!({"礼物":gifts,"总数":data["total_count"],"下一页偏移":data["next_offset"]}),
            )
        }
        "余额" => output(
            &json!({"星币":data["amount"],"十亿分之一星币":data["nanostar_amount"].as_i64().unwrap_or(0)}),
        ),
        _ => empty(),
    }
}

pub(super) fn register() {
    add_fun(vec!["Telegram商务账户"], business_account);
    add_fun(vec!["Telegram商务动态"], business_story);
    add_fun(vec!["Telegram商务礼物"], business_gifts);
    add_fun(vec!["Telegram聊天动作"], |rl, params| {
        let [kind, target, topic, connection] = text_params::<4>(rl, params, 0)?;
        let action = match kind.as_str() {
            "" | "正在输入" => "typing",
            "上传图片" => "upload_photo",
            "录制视频" => "record_video",
            "上传视频" => "upload_video",
            "录制语音" => "record_voice",
            "上传语音" => "upload_voice",
            "上传文件" => "upload_document",
            "选择贴纸" => "choose_sticker",
            "查找位置" => "find_location",
            "录制视频便笺" => "record_video_note",
            "上传视频便笺" => "upload_video_note",
            _ => return Err("未知的 Telegram 聊天动作".into()),
        };
        let chat = chat(rl, &target)?;
        let mut body = json!({"chat_id":chat,"action":action});
        if !topic.is_empty() {
            body["message_thread_id"] = positive_id(&topic)?.into();
        } else if let Some(event) = event_context(rl) {
            if matching_chat(&event, &chat) {
                if event["direct_messages_topic_id"].is_number() {
                    return Err("Telegram 聊天动作适用于群聊或普通私聊".into());
                }
                if let Some(id) = event["message_thread_id"].as_i64() {
                    body["message_thread_id"] = id.into();
                }
            }
        }
        if let Some(id) = send_context(rl, &chat, &connection) {
            body["business_connection_id"] = id.into();
        }
        telegram(rl, "sendChatAction", body)?;
        empty()
    });
    add_fun(vec!["Telegram商务已读"], |rl, params| {
        let [id, peer, business_id] = text_params::<3>(rl, params, 0)?;
        if !peer.is_empty() || !business_id.is_empty() {
            telegram(
                rl,
                "readBusinessMessage",
                json!({
                    "business_connection_id":connection(rl,&business_id)?,
                    "chat_id":positive_id(&peer)?,
                    "message_id":positive_id(&id)?
                }),
            )?;
            return empty();
        }
        let id = context(rl, &id, "消息ID")?;
        let response = call_platform_api_value(
            rl,
            "telegram",
            "telegram_read_business_message",
            json!({"message_id":id}),
        )?;
        success(response)?;
        empty()
    });
    add_fun(vec!["Telegram任务清单"], |rl, params| {
        let title = rl.get_param(params, 0)?;
        let tasks = rl.get_param(params, 1)?;
        let [add, mark] = text_params::<2>(rl, params, 2)?;
        let input = crate::core::rv_array(vec![title, tasks, rv_text(add), rv_text(mark)]);
        let content = cq_params_encode(&checklist(&input)?.to_string());
        Ok(Some(rv_text(format!(
            "[CQ:telegram_checklist,content={content}]"
        ))))
    });
}
