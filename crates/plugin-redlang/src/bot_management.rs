use super::*;
#[path = "bot_inline_results.rs"]
mod inline;

#[path = "bot_assets.rs"]
mod assets;
#[path = "bot_kook_activity.rs"]
mod activity;
#[path = "bot_business.rs"]
pub(crate) mod business;
#[path = "bot_location.rs"]
pub(crate) mod location;
#[path = "bot_payments.rs"]
pub(crate) mod payments;
#[path = "bot_templates.rs"]
pub(crate) mod templates;

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    if value.is_empty() {
        return Err(format!("请填写{label}").into());
    }
    Ok(value)
}

fn context(rl: &RedLang, value: &str, key: &str) -> Result<String> {
    let value = if value.is_empty() {
        rl.get_exmap(key).to_string()
    } else {
        value.to_owned()
    };
    required(&value, key)?;
    Ok(value)
}

fn chat(rl: &RedLang, value: &str) -> Result<Value> {
    let target = if value.is_empty() && rl.get_exmap("群ID").is_empty() {
        if let Some(peer) = business::default_peer(rl) {
            return Ok(peer.into());
        }
        context(rl, value, "发送者ID")?
    } else {
        context(rl, value, "群ID")?
    };
    chat_id(&target)
}

fn guild(rl: &RedLang, value: &str) -> Result<String> {
    if !value.is_empty() || !rl.get_exmap("群组ID").is_empty() {
        return context(rl, value, "群组ID");
    }
    let channel = context(rl, "", "群ID")?;
    let response = kook(rl, "GET", "channel/view", json!({"target_id":channel}))?;
    Ok(response["data"]["guild_id"]
        .as_str()
        .ok_or("未找到频道所属服务器")?
        .to_owned())
}

pub(crate) fn kook_chat_code(event: &Value) -> Option<&str> {
    let kook = &event["kook"];
    [
        &kook["chat_code"],
        &kook["extra"]["chat_code"],
        &kook["extra"]["code"],
    ]
    .into_iter()
    .find_map(|value| value.as_str().filter(|code| !code.is_empty()))
}

fn insert_text(body: &mut Value, field: &str, value: &str) {
    if !value.is_empty() {
        body[field] = value.into();
    }
}

pub(crate) fn kook_message_options(rl: &RedLang, input: &RedValue, edit: bool) -> Result<Value> {
    if matches!(input, RedValue::Text(text) if text.is_empty()) {
        return Ok(json!({}));
    }
    let options = array(input)?;
    let fields = if edit {
        ["temp_target_id", "quote", "reply_msg_id", ""]
    } else {
        ["temp_target_id", "nonce", "reply_msg_id", "chat_code"]
    };
    if options.len() > if edit { 3 } else { 4 } {
        return Err("KOOK 消息选项参数过多".into());
    }
    let mut body = json!({});
    for (index, field) in fields.into_iter().enumerate().take(options.len()) {
        let value = row_text(options, index)?;
        match (field, value.as_str()) {
            ("quote", "清空") => body[field] = "".into(),
            ("temp_target_id", "当前") => body[field] = context(rl, "", "发送者ID")?.into(),
            ("quote" | "reply_msg_id", "当前") => {
                let event: Value = serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
                let id = event["kook_message_id"].as_str().or_else(|| event["kook"]["msg_id"].as_str())
                    .filter(|id| !id.is_empty()).ok_or("当前事件缺少 KOOK 平台消息ID")?;
                body[field] = id.into();
            }
            _ => insert_text(&mut body, field, &value),
        }
    }
    Ok(body)
}

fn voice_quality(value: &str) -> Result<&'static str> {
    match value {
        "流畅" | "1" => Ok("1"),
        "正常" | "2" => Ok("2"),
        "高质量" | "3" => Ok("3"),
        _ => Err("语音音质请填流畅、正常或高质量".into()),
    }
}

fn bounded(value: &str, min: i64, max: i64, label: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|number| (min..=max).contains(number))
        .ok_or_else(|| format!("{label}请填写 {min} 至 {max} 的整数").into())
}

fn mapped_list(data: &Value, map: fn(&Value) -> Value) -> RedCommandResult {
    let list = data.as_array().ok_or("平台未返回列表")?;
    output(&Value::Array(list.iter().map(map).collect()))
}

fn scalar_text(value: &Value) -> RedCommandResult {
    if !value.is_string() && !value.is_number() {
        return Err("平台缺少预期的 ID、文本或数字".into());
    }
    output(value)
}

fn empty() -> RedCommandResult {
    Ok(Some(rv_empty()))
}

fn inline_results(items: &RedValue) -> Result<Vec<Value>> {
    let rows = array(items)?;
    if rows.len() > 50 {
        return Err("行内查询一次最多返回 50 个结果".into());
    }
    let mut ids = HashSet::new();
    rows.iter()
        .map(|item| {
            let row = array(item)?;
            let id = row_text(row, 0)?;
            let title = row_text(row, 1)?;
            let description = row_text(row, 3)?;
            let link = row_text(row, 4)?;
            let kind = row_text(row, 5)?;
            if row.len() > 10 || id.is_empty() || id.len() > 64 || !ids.insert(id.clone()) {
                return Err("结果 ID 须为不重复的 1 至 64 字节文本，每行最多 10 项".into());
            }
            let options = row.get(7).filter(|v| !matches!(v.as_ref(),RedValue::Text(text) if text.is_empty()))
                .map(|v| array(v)).transpose()?.unwrap_or_default();
            let mut result = match kind.as_str() {
                "" | "文章" => {
                    required(&title, "结果标题")?;
                    let content = row_text(row, 2)?;
                    required(&content, "发送文字")?;
                    let mut article =
                        json!({"type":"article","input_message_content":{"message_text":content}});
                    if !link.is_empty() {
                        https_url(&link)?;
                        article["url"] = link.into();
                    }
                    article
                }
                "富文本" => {
                    required(&title, "结果标题")?;
                    let content = row.get(2).ok_or("请填写【Telegram富文本】")?;
                    let mut article = json!({"type":"article","input_message_content":super::rich::inline(content)?});
                    if !link.is_empty() {
                        https_url(&link)?;
                        article["url"] = link.into();
                    }
                    article
                }
                _ => inline::variant(&kind,&title,&link,&description,row.get(2),options)?,
            };
            result["id"] = id.into();
            if result["type"] == "article" {
                result["title"] = title.into();
                insert_text(&mut result, "description", &description);
                inline::article_options(&mut result,options)?;
            }
            if let Some(keyboard) = row.get(6).filter(|value| !matches!(value.as_ref(),RedValue::Text(text) if text.is_empty())) {
                result["reply_markup"] = super::edit_keyboard(keyboard)?;
            }
            if let Some(alternate) = row.get(8) { inline::apply_alternate(&mut result,alternate)?; }
            if result["mime_type"] == "text/html" && result.get("input_message_content").is_none() {
                return Err("网页视频结果需要替代发送内容".into());
            }
            if let Some(styles) = row.get(9) {
                super::send_options::apply_styles(&mut result,"caption","caption_entities",styles)?;
            }
            Ok(result)
        })
        .collect()
}

fn telegram_member(data: &Value) -> Value {
    let user = &data["user"];
    let name = [
        user["first_name"].as_str().unwrap_or_default(),
        user["last_name"].as_str().unwrap_or_default(),
    ]
    .join(" ");
    let role = match data["status"].as_str() {
        Some("creator") => "群主",
        Some("administrator") => "管理员",
        Some("restricted") => "受限",
        Some("left") => "已离开",
        Some("kicked") => "已封禁",
        _ => "成员",
    };
    json!({"用户ID":user["id"],"昵称":name.trim(),"用户名":user["username"],"身份":role,"机器人":user["is_bot"]})
}

fn kook_user(data: &Value) -> Value {
    let name = data["nickname"]
        .as_str()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| data["username"].as_str().unwrap_or_default());
    json!({"用户ID":data["id"],"昵称":name,"用户名":data["username"],
        "头像":data["avatar"],"在线":data["online"],"机器人":data["bot"],"身份组":data["roles"]})
}

fn telegram_chat(data: &Value) -> Value {
    let kind = match data["type"].as_str() {
        Some("private") => "私聊",
        Some("group") => "群聊",
        Some("supergroup") => "超级群",
        Some("channel") => "频道",
        _ => "",
    };
    let reactions = data["available_reactions"].as_array().map(|items| {
        items
            .iter()
            .map(super::telegram_reaction)
            .collect::<Vec<_>>()
    });
    json!({"群ID":data["id"],"群名":data["title"],"简介":data["description"],"类型":kind,
        "用户名":data["username"],"论坛":data["is_forum"].as_bool().unwrap_or(false),
        "头像文件ID":data["photo"]["big_file_id"],"可用回应":reactions})
}

pub(super) fn kook_channel(data: &Value) -> Value {
    let kind = if data["is_category"] == true || data["is_category"] == 1 {
        "分组"
    } else {
        match data["type"].as_i64() {
            Some(1) => "文字",
            Some(2) => "语音",
            Some(3) => "帖子",
            _ => "其他",
        }
    };
    let quality = match data["voice_quality"].as_str() {
        Some("1") => "流畅",
        Some("2") => "正常",
        Some("3") => "高质量",
        _ => "",
    };
    json!({"群ID":data["id"],"群名":data["name"],"简介":data["topic"],"类型":kind,
        "服务器ID":data["guild_id"],"分组ID":data["parent_id"],"慢速秒数":data["slow_mode"],
        "创建者ID":data["user_id"],"排序":data["level"],"人数上限":data["limit_amount"],
        "已设密码":data["has_password"],"权限同步":data["permission_sync"],
        "语音音质":quality,"语音服务器":data["server_url"],"子频道ID列表":data["children"]})
}

fn members(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let operation = rl.get_param_text(params, 0)?;
    let [first, second] = text_params::<2>(rl, params, 1)?;
    match (get_platform(rl).as_str(), operation.as_str()) {
        ("telegram", "" | "详情") => {
            let user = positive_id(&context(rl, &first, "发送者ID")?)?;
            let data = telegram(
                rl,
                "getChatMember",
                json!({"chat_id":chat(rl,&second)?,"user_id":user}),
            )?;
            output(&telegram_member(&data["data"]))
        }
        ("telegram", "管理员") => {
            let mut body = json!({"chat_id":chat(rl,&first)?});
            if !second.is_empty() {
                body["return_bots"] = boolean(&second, false)?.into();
            }
            let data = telegram(rl, "getChatAdministrators", body)?;
            mapped_list(&data["data"], telegram_member)
        }
        ("telegram", "人数") => {
            let data = telegram(
                rl,
                "getChatMemberCount",
                json!({"chat_id":chat(rl,&first)?}),
            )?;
            scalar_text(&data["data"])
        }
        ("kook", "" | "详情" | "人数" | "列表" | "语音") => {
            if operation == "语音" {
                let channel = context(rl, &first, "群ID")?;
                let data = kook(
                    rl,
                    "GET",
                    "channel/user-list",
                    json!({"channel_id":channel}),
                )?;
                return mapped_list(&data["data"], kook_user);
            }
            let details = matches!(operation.as_str(), "" | "详情");
            let mut body = json!({"guild_id":guild(rl, if details { &second } else { &first })?});
            if details {
                body["filter_user_id"] = context(rl, &first, "发送者ID")?.into();
            } else if operation == "列表" {
                let [search, role, size] = text_params::<3>(rl, params, 3)?;
                let [verified, activity, joined, channel] = text_params::<4>(rl, params, 6)?;
                body["page"] = if second.is_empty() {
                    1
                } else {
                    positive_id(&second)?
                }
                .into();
                insert_text(&mut body, "search", &search);
                if !role.is_empty() {
                    body["role_id"] = bounded(&role, 0, i64::MAX, "身份组ID")?.into();
                }
                if !size.is_empty() {
                    body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
                }
                if !verified.is_empty() {
                    body["mobile_verified"] = match verified.as_str() {
                        "已认证" | "真" => 1,
                        "未认证" | "假" => 0,
                        _ => return Err("手机认证请填已认证或未认证".into()),
                    }.into();
                }
                for (field, value) in [("active_time", activity), ("joined_at", joined)] {
                    if !value.is_empty() {
                        body[field] = match value.as_str() {
                            "升序" => 0,
                            "降序" => 1,
                            _ => return Err("时间排序请填升序或降序".into()),
                        }.into();
                    }
                }
                insert_text(&mut body, "channel_id", &channel);
            } else {
                body["page_size"] = 1.into();
            }
            let data = kook(rl, "GET", "guild/user-list", body)?;
            let data = &data["data"];
            if details {
                let member = data["items"].get(0).ok_or("未找到该服务器成员")?;
                output(&kook_user(member))
            } else if operation == "人数" {
                scalar_text(&data["user_count"])
            } else {
                mapped_list(&data["items"], kook_user)
            }
        }
        _ => Err("群成员支持详情、人数、Telegram 管理员、KOOK 列表和语音".into()),
    }
}

const MEMBER_RIGHTS: &[(&str, &str)] = &[
    ("文本", "can_send_messages"),
    ("音频", "can_send_audios"),
    ("文件", "can_send_documents"),
    ("图片", "can_send_photos"),
    ("视频", "can_send_videos"),
    ("视频便笺", "can_send_video_notes"),
    ("语音", "can_send_voice_notes"),
    ("投票", "can_send_polls"),
    ("其他消息", "can_send_other_messages"),
    ("链接预览", "can_add_web_page_previews"),
    ("表情回应", "can_react_to_messages"),
    ("修改群资料", "can_change_info"),
    ("邀请成员", "can_invite_users"),
    ("置顶", "can_pin_messages"),
    ("管理话题", "can_manage_topics"),
    ("编辑标签", "can_edit_tag"),
];

const ADMIN_RIGHTS: &[(&str, &str)] = &[
    ("匿名", "is_anonymous"),
    ("管理聊天", "can_manage_chat"),
    ("删除消息", "can_delete_messages"),
    ("管理视频聊天", "can_manage_video_chats"),
    ("管理成员", "can_restrict_members"),
    ("委任管理员", "can_promote_members"),
    ("修改群资料", "can_change_info"),
    ("邀请成员", "can_invite_users"),
    ("发布动态", "can_post_stories"),
    ("编辑动态", "can_edit_stories"),
    ("删除动态", "can_delete_stories"),
    ("发布频道消息", "can_post_messages"),
    ("编辑频道消息", "can_edit_messages"),
    ("置顶", "can_pin_messages"),
    ("管理话题", "can_manage_topics"),
    ("管理频道私信", "can_manage_direct_messages"),
    ("管理标签", "can_manage_tags"),
    ("欢迎消息", "can_send_welcome_messages"),
];

fn rights(selected: &RedValue, fields: &[(&str, &str)]) -> Result<Value> {
    let mut body = json!({});
    for (_, field) in fields {
        body[*field] = false.into();
    }
    for permission in array(selected)? {
        let name = permission.expect_text_value()?;
        let field = fields
            .iter()
            .find(|(label, _)| *label == name)
            .ok_or_else(|| format!("未知权限：{name}"))?
            .1;
        body[field] = true.into();
    }
    Ok(body)
}

fn read_rights(data: &Value, fields: &[(&str, &str)]) -> Value {
    json!(fields
        .iter()
        .filter_map(|(label, field)| (data[*field] == true).then_some(*label))
        .collect::<Vec<_>>())
}

const KOOK_RIGHTS: &[&str] = &[
    "管理员",
    "管理服务器",
    "查看管理日志",
    "创建服务器邀请",
    "管理邀请",
    "频道管理",
    "踢出用户",
    "封禁用户",
    "管理自定义表情",
    "修改服务器昵称",
    "管理角色权限",
    "查看频道",
    "发布消息",
    "管理消息",
    "上传文件",
    "语音连接",
    "语音管理",
    "提及全体成员",
    "添加回应",
    "跟随添加回应",
    "被动连接语音频道",
    "仅使用按键说话",
    "使用自由麦",
    "说话",
    "服务器静音",
    "服务器闭麦",
    "修改他人昵称",
    "播放伴奏",
    "屏幕分享",
    "回复帖子",
    "开启录音",
];

fn kook_mask(input: &RedValue) -> Result<u32> {
    if let RedValue::Text(raw) = input {
        return Ok(bounded(raw, 0, u32::MAX.into(), "权限掩码")? as u32);
    }
    array(input)?.iter().try_fold(0, |mask, item| {
        let name = item.expect_text_value()?;
        let bit = KOOK_RIGHTS
            .iter()
            .position(|label| *label == name)
            .ok_or_else(|| format!("未知 KOOK 权限：{name}"))?;
        Ok(mask | (1 << bit))
    })
}

pub(super) fn kook_right_names(mask: &Value) -> Value {
    let mask = mask.as_u64().unwrap_or_default();
    json!(KOOK_RIGHTS
        .iter()
        .enumerate()
        .filter_map(|(bit, label)| (mask & (1 << bit) != 0).then_some(*label))
        .collect::<Vec<_>>())
}

fn upload(input: &RedValue, filename: &str) -> Result<Value> {
    required(filename, "文件名")?;
    let RedValue::Bin(bytes) = input else {
        return Err("请使用【读文件】等命令提供文件字节集".into());
    };
    Ok(crate::native_args::file_part(filename, bytes, None))
}

fn upload_api(
    rl: &RedLang,
    platform: &str,
    method: &str,
    body: Value,
    files: Value,
) -> Result<Value> {
    let (action, args) = match platform {
        "telegram" => (
            "telegram_api",
            json!({"method":method,"body":body,"files":files}),
        ),
        _ => (
            "kook_api",
            json!({"http_method":"POST","endpoint":method,"body":body,"files":files}),
        ),
    };
    call_platform_api_value(rl, platform, action, args).and_then(success)
}

fn telegram_topics(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let op = rl.get_param_text(params, 0)?;
    let [first, second, third, fourth] = text_params::<4>(rl, params, 1)?;
    if op == "图标" {
        let data = telegram(rl, "getForumTopicIconStickers", json!({}))?;
        return mapped_list(
            &data["data"],
            |icon| json!({"图标ID":icon["custom_emoji_id"],"表情":icon["emoji"],"文件ID":icon["file_id"]}),
        );
    }
    let (method, mut body) = match op.as_str() {
        "创建" => {
            let mut body = json!({"chat_id":chat(rl,&second)?,"name":required(&first,"话题名称")?});
            if !third.is_empty() {
                body["icon_color"] = match third.as_str() {
                    "蓝色" => 0x6FB9F0,
                    "黄色" => 0xFFD67E,
                    "紫色" => 0xCB86DB,
                    "绿色" => 0x8EEE98,
                    "粉色" => 0xFF93B2,
                    "红色" => 0xFB6F5F,
                    _ => return Err("话题颜色请填蓝色、黄色、紫色、绿色、粉色或红色".into()),
                }
                .into();
            }
            insert_text(&mut body, "icon_custom_emoji_id", &fourth);
            let data = telegram(rl, "createForumTopic", body)?;
            return scalar_text(&data["data"]["message_thread_id"]);
        }
        "改名默认" => (
            "editGeneralForumTopic",
            json!({"chat_id":chat(rl,&second)?,"name":required(&first,"话题名称")?}),
        ),
        "关闭默认" | "重开默认" | "隐藏默认" | "显示默认" | "清空默认置顶" => {
            let method = match op.as_str() {
                "关闭默认" => "closeGeneralForumTopic",
                "重开默认" => "reopenGeneralForumTopic",
                "隐藏默认" => "hideGeneralForumTopic",
                "显示默认" => "unhideGeneralForumTopic",
                _ => "unpinAllGeneralForumTopicMessages",
            };
            (method, json!({"chat_id":chat(rl,&first)?}))
        }
        "编辑" | "关闭" | "重开" | "删除" | "清空置顶" => {
            let edit = op == "编辑";
            let target = if edit { &third } else { &second };
            if first.is_empty() && !target.is_empty() {
                return Err("指定目标群时请填写话题ID".into());
            }
            let topic = if first.is_empty() {
                let event: Value =
                    serde_json::from_str(&rl.get_exmap("原始事件")).map_err(|_| "请填写话题ID")?;
                event["message_thread_id"]
                    .as_i64()
                    .or_else(|| event["telegram"]["message_thread_id"].as_i64())
                    .ok_or("请填写话题ID")?
            } else {
                positive_id(&first)?
            };
            let method = match op.as_str() {
                "编辑" => "editForumTopic",
                "关闭" => "closeForumTopic",
                "重开" => "reopenForumTopic",
                "删除" => "deleteForumTopic",
                _ => "unpinAllForumTopicMessages",
            };
            (
                method,
                json!({"chat_id":chat(rl,target)?,"message_thread_id":topic}),
            )
        }
        _ => {
            return Err(
                "话题操作请填创建、编辑、关闭、重开、删除、清空置顶、图标或默认话题操作".into(),
            )
        }
    };
    if op == "编辑" {
        if second.is_empty() && fourth.is_empty() {
            return Err("请填写新名称或图标ID".into());
        }
        insert_text(&mut body, "name", &second);
        if fourth == "移除" {
            body["icon_custom_emoji_id"] = "".into();
        } else {
            insert_text(&mut body, "icon_custom_emoji_id", &fourth);
        }
    }
    telegram(rl, method, body)?;
    empty()
}

fn register_admin() {
    add_fun(vec!["Telegram话题"], telegram_topics);
    add_fun(vec!["Telegram机器人资料"], |rl, params| {
        let [operation, field, first, second] = text_params::<4>(rl, params, 0)?;
        let (get, set, key, max) = match field.as_str() {
            "名称" => ("getMyName", "setMyName", "name", 64),
            "简介" => ("getMyDescription", "setMyDescription", "description", 512),
            "短简介" => (
                "getMyShortDescription",
                "setMyShortDescription",
                "short_description",
                120,
            ),
            _ => return Err("机器人资料请填名称、简介或短简介".into()),
        };
        let mut body = json!({});
        match operation.as_str() {
            "查询" => {
                insert_text(&mut body, "language_code", &first);
                let data = telegram(rl, get, body)?;
                scalar_text(&data["data"][key])
            }
            "设置" => {
                if first.chars().count() > max {
                    return Err(format!("{field}最多 {max} 字").into());
                }
                body[key] = first.into();
                insert_text(&mut body, "language_code", &second);
                telegram(rl, set, body)?;
                empty()
            }
            _ => Err("机器人资料操作请填查询或设置".into()),
        }
    });
    add_fun(vec!["Telegram群权限"], |rl, params| {
        let op = rl.get_param_text(params, 0)?;
        match op.as_str() {
            "查询" => {
                let target = rl.get_param_text(params, 1)?;
                let data = telegram(rl, "getChat", json!({"chat_id":chat(rl,&target)?}))?;
                let permissions = &data["data"]["permissions"];
                if !permissions.is_object() {
                    return Err("当前聊天没有普通成员权限配置".into());
                }
                output(&read_rights(permissions, MEMBER_RIGHTS))
            }
            "设置" | "限制" => {
                let limited = op == "限制";
                let selected = rl.get_param(params, if limited { 2 } else { 1 })?;
                let target = rl.get_param_text(params, if limited { 4 } else { 2 })?;
                let mut body = json!({"chat_id":chat(rl,&target)?,"permissions":rights(&selected,MEMBER_RIGHTS)?,
                    "use_independent_chat_permissions":true});
                let mut method = "setChatPermissions";
                if limited {
                    let user = rl.get_param_text(params, 1)?;
                    body["user_id"] = positive_id(&context(rl, &user, "发送者ID")?)?.into();
                    let seconds = rl.get_param_text(params, 3)?;
                    if !seconds.is_empty() && seconds != "0" {
                        let seconds = bounded(&seconds, 30, 366 * 86400, "有效秒数")?;
                        body["until_date"] = (chrono::Utc::now().timestamp() + seconds).into();
                    }
                    method = "restrictChatMember";
                }
                telegram(rl, method, body)?;
                empty()
            }
            "管理员权限" | "设管理员" | "撤管理员" => {
                let user = rl.get_param_text(params, 1)?;
                let user = positive_id(&context(rl, &user, "发送者ID")?)?;
                let target = rl.get_param_text(params, if op == "设管理员" { 3 } else { 2 })?;
                let mut body = json!({"chat_id":chat(rl,&target)?,"user_id":user});
                if op == "管理员权限" {
                    let data = telegram(rl, "getChatMember", body)?;
                    return output(&read_rights(&data["data"], ADMIN_RIGHTS));
                }
                let selected = if op == "设管理员" {
                    rl.get_param(params, 2)?
                } else {
                    crate::core::rv_array(vec![])
                };
                let permissions = rights(&selected, ADMIN_RIGHTS)?;
                if op == "设管理员"
                    && !permissions.as_object().is_some_and(|map| {
                        map.iter()
                            .any(|(key, val)| key != "is_anonymous" && val == true)
                    })
                {
                    return Err("设管理员需要至少一项管理权限".into());
                }
                for (key, value) in permissions.as_object().ok_or("权限格式错误")? {
                    body[key] = value.clone();
                }
                telegram(rl, "promoteChatMember", body)?;
                empty()
            }
            _ => Err("群权限操作请填查询、设置、限制、管理员权限、设管理员或撤管理员".into()),
        }
    });
    add_fun(vec!["群管理"], |rl, params| {
        let [op, value, target] = text_params::<3>(rl, params, 0)?;
        match get_platform(rl).as_str() {
            "telegram" => {
                let mut body = json!({"chat_id":chat(rl,if op == "退出" || op == "移除头像" { &value } else { &target })?});
                let method = match op.as_str() {
                    "改名" => {
                        body["title"] = required(&value, "群名")?.into();
                        "setChatTitle"
                    }
                    "简介" => {
                        body["description"] = value.into();
                        "setChatDescription"
                    }
                    "移除头像" => "deleteChatPhoto",
                    "封禁" | "解封" => {
                        body["user_id"] = positive_id(&context(rl, &value, "发送者ID")?)?.into();
                        if op == "封禁" {
                            "banChatMember"
                        } else {
                            body["only_if_banned"] = true.into();
                            "unbanChatMember"
                        }
                    }
                    "退出" => "leaveChat",
                    _ => {
                        return Err(
                            "Telegram 群管理支持改名、简介、封禁、解封、移除头像或退出".into()
                        )
                    }
                };
                telegram(rl, method, body)?;
            }
            "kook" => {
                if op == "黑名单" {
                    let [size] = text_params::<1>(rl, params, 3)?;
                    let page = if target.is_empty() {
                        1
                    } else {
                        positive_id(&target)?
                    };
                    let mut body = json!({"guild_id":guild(rl,&value)?,"page":page});
                    if !size.is_empty() {
                        body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
                    }
                    let data = kook(rl, "GET", "blacklist/list", body)?;
                    return mapped_list(&data["data"]["items"], |row| {
                        let mut member = kook_user(&row["user"]);
                        member["用户ID"] = row["user_id"].clone();
                        member["封禁时间"] = row["created_time"].clone();
                        member["原因"] = row["remark"].clone();
                        member
                    });
                }
                let (path, body) = match op.as_str() {
                    "改名" | "简介" => {
                        let channel = context(rl, &target, "群ID")?;
                        let mut body = json!({"channel_id":channel});
                        if op == "改名" {
                            body["name"] = required(&value, "频道名")?.into();
                        } else {
                            body["topic"] = value.into();
                        }
                        ("channel/update", body)
                    }
                    "封禁" | "解封" | "踢出" => {
                        let path = match op.as_str() {
                            "封禁" => "blacklist/create",
                            "解封" => "blacklist/delete",
                            _ => "guild/kickout",
                        };
                        let mut body = json!({"guild_id":guild(rl,&target)?,"target_id":context(rl,&value,"发送者ID")?});
                        if op == "封禁" {
                            let [reason, days] = text_params::<2>(rl, params, 3)?;
                            insert_text(&mut body, "remark", &reason);
                            if !days.is_empty() {
                                body["del_msg_days"] = bounded(&days, 0, 7, "清理天数")?.into();
                            }
                        }
                        (path, body)
                    }
                    "昵称" => {
                        let server = rl.get_param_text(params, 3)?;
                        if !target.is_empty() && !(2..=64).contains(&target.chars().count()) {
                            return Err("服务器昵称请填写 2 至 64 字，留空清除".into());
                        }
                        let mut body = json!({"guild_id":guild(rl,&server)?,"user_id":context(rl,&value,"发送者ID")?});
                        insert_text(&mut body, "nickname", &target);
                        ("guild/nickname", body)
                    }
                    "退出" => ("guild/leave", json!({"guild_id":guild(rl,&value)?})),
                    _ => {
                        return Err(
                            "KOOK 群管理支持改名、简介、昵称、封禁、解封、黑名单、踢出或退出"
                                .into(),
                        )
                    }
                };
                kook(rl, "POST", path, body)?;
            }
            _ => return Err("群管理需要 KOOK 或 Telegram 机器人来源".into()),
        }
        empty()
    });
    add_fun(vec!["Telegram群头像"], |rl, params| {
        let file = rl.get_param(params, 0)?;
        let [name, target] = text_params::<2>(rl, params, 1)?;
        upload_api(
            rl,
            "telegram",
            "setChatPhoto",
            json!({"chat_id":chat(rl,&target)?}),
            json!({"photo":upload(&file,&name)?}),
        )?;
        empty()
    });
    add_fun(vec!["Telegram文件"], |rl, params| {
        let file = rl.get_param(params, 0)?;
        let [name, caption, target, topic, topic_kind, connection] =
            text_params::<6>(rl, params, 1)?;
        let mut body = json!({"chat_id":chat(rl,&target)?});
        if let Some(id) = business::send_context(rl, &body["chat_id"], &connection) {
            body["business_connection_id"] = id.into();
        }
        insert_text(&mut body, "caption", &caption);
        let styles = rl.get_param(params,8)?;
        super::send_options::apply_styles(&mut body,"caption","caption_entities",&styles)?;
        if let Some((field, id)) = crate::cqexfun::telegram_output_topic(&topic, &topic_kind)? {
            body[field] = id.into();
        } else if let Ok(event) = serde_json::from_str(&rl.get_exmap("原始事件")) {
            let target_id = body["chat_id"]
                .as_i64()
                .map(|id| id.to_string())
                .unwrap_or_else(|| target.clone());
            apply_telegram_topic(&event, &rl.get_exmap("机器人ID"), &target_id, &mut body);
        }
        let options = super::send_options::parse(rl, params, 7)?;
        let private = body["chat_id"].as_i64().is_some_and(|id| id > 0);
        super::send_options::simple_options(&options, &mut body, private, true)?;
        let data = if matches!(file.as_ref(), RedValue::Bin(_)) {
            upload_api(
                rl,
                "telegram",
                "sendDocument",
                body,
                json!({"document":upload(&file,&name)?}),
            )?
        } else {
            if !name.is_empty() {
                return Err("只有上传字节集时可以填写文件名".into());
            }
            let id = file.expect_text_value()?;
            body["document"] = required(&id, "文件ID或直链")?.into();
            telegram(rl, "sendDocument", body)?
        };
        scalar_text(&data["data"]["document"]["file_id"])
    });
    add_fun(vec!["Telegram小程序应答"], |rl, params| {
        let [query, id, title] = text_params::<3>(rl, params, 0)?;
        let message = rl.get_param(params,3)?;
        let kind = rl.get_param_text(params,4)?;
        let content = match kind.as_str() {
            "" | "文章" => json!({"message_text":required(&message.expect_text_value()?,"发送文字")?}),
            "富文本" => super::rich::inline(&message)?,
            _ => return Err("小程序应答类型请填文章或富文本".into()),
        };
        let result = json!({"type":"article","id":required(&id,"结果ID")?,"title":required(&title,"标题")?,
            "input_message_content":content});
        let data = telegram(
            rl,
            "answerWebAppQuery",
            json!({"web_app_query_id":required(&query,"小程序查询ID")?,"result":result}),
        )?;
        output(&data["data"]["inline_message_id"])
    });
    add_fun(vec!["Telegram准备行内消息"], |rl, params| {
        let row = rl.get_param(params,0)?;
        let result = inline_results(&RedValue::Array(vec![row]))?.into_iter().next().ok_or("请填写行内结果")?;
        let user = rl.get_param_text(params,1)?;
        let user = positive_id(&context(rl,&user,"发送者ID")?)?;
        let [users,bots,groups,channels] = text_params::<4>(rl,params,2)?;
        let data = telegram(rl,"savePreparedInlineMessage",json!({"user_id":user,"result":result,
            "allow_user_chats":boolean(&users,false)?,"allow_bot_chats":boolean(&bots,false)?,
            "allow_group_chats":boolean(&groups,false)?,"allow_channel_chats":boolean(&channels,false)?}))?;
        output(&json!({"准备ID":data["data"]["id"],"到期时间":data["data"]["expiration_date"]}))
    });
}

fn thread_info(data: &Value) -> Value {
    json!({"帖子ID":data["id"],"标题":data["title"],"封面":data["cover"],"主楼ID":data["post_id"],
        "分区ID":data["category"]["id"],"摘要":data["preview_content"],"内容":data["content"],
        "回复数":data["post_count"],"创建时间":data["create_time"],"活跃时间":data["latest_active_time"]})
}

fn thread_post(data: &Value) -> Value {
    json!({"回复ID":data["id"],"帖子ID":data["thread_id"],"上级回复ID":data["reply_id"],
        "内容":data["content"],"用户ID":data["user"]["id"],"创建时间":data["create_time"]})
}

fn kook_threads(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let op = rl.get_param_text(params, 0)?;
    if op == "发布" {
        let title = rl.get_param_text(params, 1)?;
        let content = rl.get_param(params, 2)?;
        let [channel, server, section, cover] = text_params::<4>(rl, params, 3)?;
        let channel = context(rl, &channel, "群ID")?;
        let server = if !server.is_empty() || channel.as_str() == &*rl.get_exmap("群ID") {
            guild(rl, &server)?
        } else {
            let data = kook(rl, "GET", "channel/view", json!({"target_id":channel}))?;
            data["data"]["guild_id"]
                .as_str()
                .ok_or("未找到帖子频道所属服务器")?
                .to_owned()
        };
        let mut body = json!({"channel_id":channel,"guild_id":server,"title":required(&title,"帖子标题")?,
            "content":card_for_edit(&content)?.to_string()});
        insert_text(&mut body, "category_id", &section);
        insert_text(&mut body, "cover", &cover);
        let data = kook(rl, "POST", "thread/create", body)?;
        return scalar_text(&data["data"]["id"]);
    }
    let [first, second, third, fourth, fifth] = text_params::<5>(rl, params, 1)?;
    match op.as_str() {
        "分区" => {
            let channel = context(rl, &first, "群ID")?;
            let data = kook(rl, "GET", "category/list", json!({"channel_id":channel}))?;
            mapped_list(
                &data["data"]["list"],
                |item| json!({"分区ID":item["id"],"名称":item["name"]}),
            )
        }
        "列表" => {
            let channel = context(rl, &first, "群ID")?;
            let mut body = json!({"channel_id":channel});
            insert_text(&mut body, "category_id", &second);
            if !third.is_empty() {
                body["sort"] = match third.as_str() {
                    "最近回复" => 1,
                    "最新创建" => 2,
                    _ => return Err("帖子排序请填最近回复或最新创建".into()),
                }
                .into();
            }
            if !fourth.is_empty() {
                if third.is_empty() {
                    return Err("按时间翻页时请同时指定排序".into());
                }
                body["time"] = positive_id(&fourth)?.into();
            }
            if !fifth.is_empty() {
                body["page_size"] = bounded(&fifth, 1, 50, "每页数量")?.into();
            }
            let data = kook(rl, "GET", "thread/list", body)?;
            mapped_list(&data["data"]["items"], thread_info)
        }
        "详情" => {
            let channel = context(rl, &second, "群ID")?;
            let data = kook(
                rl,
                "GET",
                "thread/view",
                json!({"channel_id":channel,"thread_id":required(&first,"帖子ID")?}),
            )?;
            output(&thread_info(&data["data"]))
        }
        "回复" => {
            let channel = context(rl, &third, "群ID")?;
            let mut body = json!({"channel_id":channel,"thread_id":required(&first,"帖子ID")?,"content":required(&second,"回复文字")?});
            insert_text(&mut body, "reply_id", &fourth);
            let data = kook(rl, "POST", "thread/reply", body)?;
            scalar_text(&data["data"]["id"])
        }
        "回复列表" => {
            let channel = context(rl, &second, "群ID")?;
            let page = if third.is_empty() {
                1
            } else {
                positive_id(&third)?
            };
            let order = match fourth.as_str() {
                "" | "由早到晚" => "asc",
                "由晚到早" => "desc",
                _ => return Err("回复顺序请填由早到晚或由晚到早".into()),
            };
            let mut body = json!({"channel_id":channel,"thread_id":required(&first,"帖子ID")?,
                "page":page.to_string(),"order":order});
            insert_text(&mut body, "post_id", &fifth);
            let data = kook(rl, "GET", "thread/post", body)?;
            mapped_list(&data["data"]["items"], thread_post)
        }
        "删除" | "删除回复" => {
            let channel = context(rl, &second, "群ID")?;
            let field = if op == "删除" {
                "thread_id"
            } else {
                "post_id"
            };
            kook(
                rl,
                "POST",
                "thread/delete",
                json!({"channel_id":channel,field:required(&first,"帖子或回复ID")?}),
            )?;
            empty()
        }
        _ => Err("帖子操作请填分区、列表、详情、发布、回复、回复列表、删除或删除回复".into()),
    }
}

pub(super) fn kook_permissions(data: &Value) -> Value {
    fn entries(data: &Value, users: bool) -> Vec<Value> {
        data.as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                let id = if users {
                    &row["user"]["id"]
                } else {
                    &row["role_id"]
                };
                json!({"目标ID":id,"允许":row["allow"],"禁止":row["deny"],
                    "允许权限":kook_right_names(&row["allow"]),"禁止权限":kook_right_names(&row["deny"])})
            })
            .collect()
    }
    json!({"身份组":entries(&data["permission_overwrites"],false),
        "成员":entries(&data["permission_users"],true),"已同步":data["permission_sync"]})
}

fn register_kook() {
    add_fun(vec!["KOOK帖子"], kook_threads);
    add_fun(vec!["KOOK私聊会话"], |rl, params| {
        let [operation, target, size] = text_params::<3>(rl, params, 0)?;
        let record = |row: &Value| {
            json!({"会话码":row["code"],"上次阅读时间":row["last_read_time"],
            "最后消息时间":row["latest_msg_time"],"未读数":row["unread_count"],"用户":kook_user(&row["target_info"]),
            "好友":row["is_friend"],"已屏蔽对方":row["is_blocked"],"被对方屏蔽":row["is_target_blocked"]})
        };
        match operation.as_str() {
            "列表" | "" => {
                let mut body = json!({});
                if !target.is_empty() {
                    body["page"] = positive_id(&target)?.into();
                }
                if !size.is_empty() {
                    body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
                }
                let data = kook(rl, "GET", "user-chat/list", body)?;
                mapped_list(&data["data"]["items"], record)
            }
            "详情" | "删除" => {
                let event: Value =
                    serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
                let code = if target.is_empty() {
                    kook_chat_code(&event).unwrap_or_default()
                } else {
                    &target
                };
                let body = json!({"chat_code":required(code,"私聊会话码")?});
                if operation == "详情" {
                    output(&record(&kook(rl, "GET", "user-chat/view", body)?["data"]))
                } else {
                    kook(rl, "POST", "user-chat/delete", body)?;
                    empty()
                }
            }
            "创建" => {
                let data = kook(
                    rl,
                    "POST",
                    "user-chat/create",
                    json!({"target_id":context(rl,&target,"发送者ID")?}),
                )?;
                output(&record(&data["data"]))
            }
            _ => Err("私聊会话操作请填列表、详情、创建或删除".into()),
        }
    });
    add_fun(vec!["KOOK服务器"], |rl, params| {
        let [operation, target] = text_params::<2>(rl, params, 0)?;
        let record = |row: &Value| {
            json!({"服务器ID":row["id"],"名称":row["name"],"简介":row["topic"],
            "所有者ID":row["user_id"],"图标":row["icon"],"默认频道ID":row["default_channel_id"],"欢迎频道ID":row["welcome_channel_id"],
            "通知类型":row["notify_type"],"地区":row["region"],"公开":row["enable_open"],"公开ID":row["open_id"],
            "助力数":row["boost_num"],"等级":row["level"]})
        };
        match operation.as_str() {
            "徽章" => activity::badge(rl, params, &target),
            "助力历史" => {
                let [page, size, from, to] = text_params::<4>(rl, params, 2)?;
                let mut body = json!({"guild_id":guild(rl,&target)?});
                for (name, value, min, max, label) in [
                    ("page", page, 1, i64::MAX, "页码"),
                    ("page_size", size, 1, 50, "每页数量"),
                    ("start_time", from, 0, i64::MAX, "开始时间"),
                    ("end_time", to, 0, i64::MAX, "结束时间"),
                ] {
                    if !value.is_empty() {
                        body[name] = bounded(&value, min, max, label)?.into();
                    }
                }
                if let (Some(from), Some(to)) =
                    (body["start_time"].as_i64(), body["end_time"].as_i64())
                {
                    if from > to {
                        return Err("开始时间不得晚于结束时间".into());
                    }
                }
                let data = kook(rl, "GET", "guild-boost/history", body)?;
                mapped_list(&data["data"]["items"], |row| {
                    json!({"用户ID":row["user_id"],
                    "服务器ID":row["guild_id"],"开始时间":row["start_time"],"结束时间":row["end_time"],
                    "成员":kook_user(&row["user"])})
                })
            }
            "" | "详情" => {
                let data = kook(
                    rl,
                    "GET",
                    "guild/view",
                    json!({"guild_id":guild(rl,&target)?}),
                )?;
                let row = &data["data"];
                let mut result = record(row);
                let roles: Vec<_> = row["roles"].as_array().into_iter().flatten().map(|role| {
                    json!({"身份组ID":role["role_id"],"名称":role["name"],"颜色":role["color"],
                           "排序":role["position"],"独立显示":role["hoist"],"可提及":role["mentionable"],
                           "权限":kook_right_names(&role["permissions"])})
                }).collect();
                let channels: Vec<_> = row["channels"].as_array().into_iter().flatten().map(kook_channel).collect();
                result["身份组列表"] = roles.into();
                result["频道列表"] = channels.into();
                output(&result)
            }
            "列表" => {
                let page = if target.is_empty() {
                    1
                } else {
                    positive_id(&target)?
                };
                let size = rl.get_param_text(params, 2)?;
                let mut body = json!({"page":page});
                if !size.is_empty() {
                    body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
                }
                let data = kook(rl, "GET", "guild/list", body)?;
                mapped_list(&data["data"]["items"], record)
            }
            _ => Err("服务器查询请填列表、详情、徽章或助力历史".into()),
        }
    });
    add_fun(vec!["KOOK上传文件"], |rl, params| {
        let file = rl.get_param(params, 0)?;
        let name = rl.get_param_text(params, 1)?;
        let data = upload_api(
            rl,
            "kook",
            "asset/create",
            json!({}),
            json!({"file":upload(&file,&name)?}),
        )?;
        scalar_text(&data["data"]["url"])
    });
    add_fun(vec!["KOOK频道"], |rl, params| {
        let [op, first, second, third, fourth, fifth] = text_params::<6>(rl, params, 0)?;
        match op.as_str() {
            "创建" => {
                let quality = rl.get_param_text(params, 6)?;
                let mut body =
                    json!({"guild_id":guild(rl,&third)?,"name":required(&first,"频道名")?});
                match second.as_str() {
                    "分组" if fourth.is_empty() && fifth.is_empty() && quality.is_empty() => {
                        body["is_category"] = 1.into()
                    }
                    "" | "文字" | "语音" => {
                        body["type"] = (if second == "语音" { 2 } else { 1 }).into();
                        insert_text(&mut body, "parent_id", &fourth);
                        if !fifth.is_empty() {
                            if second != "语音" {
                                return Err("人数上限只适用于语音频道".into());
                            }
                            body["limit_amount"] = bounded(&fifth, 0, 99, "人数上限")?.into();
                        }
                        if !quality.is_empty() {
                            if second != "语音" {
                                return Err("音质只适用于语音频道".into());
                            }
                            body["voice_quality"] = voice_quality(&quality)?.into();
                        }
                    }
                    _ => return Err("频道类型请填文字、语音或分组，分组不填上级分组和人数".into()),
                }
                let data = kook(rl, "POST", "channel/create", body)?;
                scalar_text(&data["data"]["id"])
            }
            "编辑" => {
                let [level, capacity, quality, password] = text_params::<4>(rl, params, 6)?;
                let channel = context(rl, &first, "群ID")?;
                let mut body = json!({"channel_id":channel});
                insert_text(&mut body, "name", &second);
                if third == "清空" {
                    body["topic"] = "".into();
                } else {
                    insert_text(&mut body, "topic", &third);
                }
                if !fourth.is_empty() {
                    let seconds = bounded(&fourth, 0, 21600, "慢速秒数")?;
                    if ![
                        0, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 21600,
                    ]
                    .contains(&seconds)
                    {
                        return Err("慢速秒数须使用官方档位".into());
                    }
                    body["slow_mode"] = (seconds * 1000).into();
                }
                insert_text(&mut body, "parent_id", &fifth);
                if !level.is_empty() {
                    body["level"] = level.parse::<i64>()?.into();
                }
                if !capacity.is_empty() {
                    body["limit_amount"] = bounded(&capacity, 0, 99, "人数上限")?.into();
                }
                if !quality.is_empty() {
                    body["voice_quality"] = voice_quality(&quality)?.into();
                }
                if password == "清空" {
                    body["password"] = "".into();
                } else {
                    insert_text(&mut body, "password", &password);
                }
                if body.as_object().is_some_and(|body| body.len() == 1) {
                    return Err("请填写要修改的频道资料".into());
                }
                kook(rl, "POST", "channel/update", body)?;
                empty()
            }
            "删除" => {
                kook(
                    rl,
                    "POST",
                    "channel/delete",
                    json!({"channel_id":context(rl,&first,"群ID")?}),
                )?;
                empty()
            }
            "列表" => {
                let page = if second.is_empty() {
                    1
                } else {
                    positive_id(&second)?
                };
                let mut body = json!({"guild_id":guild(rl,&first)?,"page":page});
                body["type"] = match third.as_str() {
                    "" | "文字" => 1,
                    "语音" => 2,
                    _ => return Err("列表类型请填文字或语音".into()),
                }
                .into();
                insert_text(&mut body, "parent_id", &fourth);
                if !fifth.is_empty() {
                    body["page_size"] = bounded(&fifth, 1, 50, "每页数量")?.into();
                }
                let data = kook(rl, "GET", "channel/list", body)?;
                mapped_list(&data["data"]["items"], kook_channel)
            }
            _ => Err("频道操作请填创建、编辑、删除或列表，详情用【群信息】".into()),
        }
    });
    add_fun(vec!["KOOK频道权限"], |rl, params| {
        let [op, first, second] = text_params::<3>(rl, params, 0)?;
        if matches!(op.as_str(), "查询" | "同步") {
            let channel = context(rl, &first, "群ID")?;
            let (method, path) = if op == "查询" {
                ("GET", "channel-role/index")
            } else {
                ("POST", "channel-role/sync")
            };
            let data = kook(rl, method, path, json!({"channel_id":channel}))?;
            return output(&kook_permissions(&data["data"]));
        }
        let write = op == "设置";
        let kind = match first.as_str() {
            "身份组" => "role_id",
            "成员" => "user_id",
            _ => return Err("权限目标请填身份组或成员".into()),
        };
        let target = rl.get_param_text(params, if write { 5 } else { 3 })?;
        let channel = context(rl, &target, "群ID")?;
        let mut body =
            json!({"channel_id":channel,"type":kind,"value":required(&second,"目标ID")?});
        let path = match op.as_str() {
            "创建" => "channel-role/create",
            "设置" => {
                let allow = rl.get_param(params, 3)?;
                let deny = rl.get_param(params, 4)?;
                let allow = kook_mask(&allow)?;
                let deny = kook_mask(&deny)?;
                if allow & deny != 0 {
                    return Err("同一权限只能出现在允许或禁止的一侧".into());
                }
                body["allow"] = allow.into();
                body["deny"] = deny.into();
                "channel-role/update"
            }
            "删除" => "channel-role/delete",
            _ => return Err("频道权限操作请填查询、同步、创建、设置或删除".into()),
        };
        kook(rl, "POST", path, body)?;
        empty()
    });
    add_fun(vec!["KOOK频道消息"], |rl, params| {
        let [op, first, second, third, scope] = text_params::<5>(rl, params, 0)?;
        let [code_override, page] = text_params::<2>(rl, params, 5)?;
        let private = match scope.as_str() {
            "" | "频道" => false,
            "私聊" => true,
            _ => return Err("消息范围请填频道或私聊".into()),
        };
        let record = |data: &Value| {
            let author = data.get("author_id").unwrap_or(&data["author"]["id"]);
            let timestamp = data.get("create_at").unwrap_or(&data["msg_timestamp"]);
            let kind = match data["type"].as_i64() {
                Some(1) => "文本", Some(2) => "图片", Some(3) => "视频", Some(4) => "文件",
                Some(8) => "音频", Some(9) => "富文本", Some(10) => "卡片", Some(12) => "道具", _ => "",
            };
            let quote = &data["quote"];
            let reactions: Vec<_> = data["reactions"].as_array().into_iter().flatten().map(|reaction|
                json!({"表情ID":reaction["emoji"]["id"],"表情名":reaction["emoji"]["name"],
                    "数量":reaction["count"],"自己已回应":reaction["me"]})).collect();
            json!({"消息ID":data["id"],"正文":data["content"],"发送者ID":author,
                "发送时间":timestamp,"附件链接":data["attachments"]["url"],"文件名":data["attachments"]["name"],
                "消息类型":kind,"修改时间":data["updated_at"],"附件大小":data["attachments"]["size"],
                "附件类型":data["attachments"]["type"],"附件时长":data["attachments"]["duration"],
                "附件宽度":data["attachments"]["width"],"附件高度":data["attachments"]["height"],
                "已读":data["read_status"],"已置顶":data["is_pinned"],"图片文件名":data["image_name"],
                "提及用户ID列表":data["mention"],"提及身份组ID列表":data["mention_roles"],
                "提及所有人":data["mention_all"],"提及在线成员":data["mention_here"],"回应":reactions,
                "引用消息ID":quote["id"],"引用正文":quote["content"],"引用发送者ID":quote["author"]["id"],
                "消息图标":data["msg_icon"],"链接预览":super::kook_fields::previews(&data["embeds"]),
                "视频":super::kook_fields::media(data,Some("视频")),"音频":super::kook_fields::media(data,Some("音频")),
                "媒体列表":super::kook_fields::media_list(data),"道具":super::kook_fields::items(&data["content"])})
        };
        if op == "详情" {
            let mut body = json!({"msg_id":required(&first,"平台消息ID")?});
            let path = if private {
                let event: Value =
                    serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
                let code = if second.is_empty() {
                    kook_chat_code(&event).unwrap_or_default()
                } else {
                    &second
                };
                body["chat_code"] = required(code, "私聊会话码")?.into();
                "direct-message/view"
            } else {
                "message/view"
            };
            let data = kook(rl, "GET", path, body)?;
            return output(&record(&data["data"]));
        }
        let mut body = if !code_override.is_empty() {
            if !private || !second.is_empty() {
                return Err("会话码只用于私聊消息列表，使用时留空目标用户ID".into());
            }
            json!({"chat_code":code_override})
        } else {
            json!({"target_id":context(rl,&second,if private { "发送者ID" } else { "群ID" })?})
        };
        if !first.is_empty() {
            body["page_size"] = bounded(&first, 1, 50, "消息条数")?.into();
        }
        if !page.is_empty() {
            if !private {
                return Err("页码仅适用于 KOOK 私聊历史，频道历史使用参考消息ID".into());
            }
            body["page"] = positive_id(&page)?.into();
        }
        match op.as_str() {
            "" | "最新" => (),
            "置顶" if !private => body["pin"] = 1.into(),
            "之前" | "之后" | "附近" => {
                body["flag"] = match op.as_str() {
                    "之前" => "before",
                    "之后" => "after",
                    _ => "around",
                }
                .into();
                body["msg_id"] = required(&third, "参考消息ID")?.into();
            }
            _ => return Err("频道消息操作请填最新、置顶、之前、之后、附近或详情".into()),
        }
        let path = if private {
            "direct-message/list"
        } else {
            "message/list"
        };
        let data = kook(rl, "GET", path, body)?;
        mapped_list(&data["data"]["items"], record)
    });
    add_fun(vec!["KOOK语音管理"], |rl, params| {
        let op = rl.get_param_text(params, 0)?;
        match op.as_str() {
            "用户所在频道" => {
                let [user, server, page, size] = text_params::<4>(rl, params, 1)?;
                let mut body =
                    json!({"user_id":context(rl,&user,"发送者ID")?,"guild_id":guild(rl,&server)?});
                if !page.is_empty() {
                    body["page"] = positive_id(&page)?.into();
                }
                if !size.is_empty() {
                    body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
                }
                let data = kook(rl, "GET", "channel-user/get-joined-channel", body)?;
                mapped_list(&data["data"]["items"], kook_channel)
            }
            "限制列表" => {
                let server = rl.get_param_text(params, 1)?;
                let data = kook(
                    rl,
                    "GET",
                    "guild-mute/list",
                    json!({"guild_id":guild(rl,&server)?,"return_type":"detail"}),
                )?;
                let data = &data["data"];
                if !data.is_object() {
                    return Err("平台未返回服务器语音限制列表".into());
                }
                let ids = |key| {
                    data[key]["user_ids"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                };
                output(&json!({"闭麦":ids("mic"),"静音":ids("headset")}))
            }
            "闭麦" | "解除闭麦" | "静音" | "解除静音" => {
                let [user, server] = text_params::<2>(rl, params, 1)?;
                let kind = if op.ends_with("闭麦") { 1 } else { 2 };
                let path = if op.starts_with("解除") {
                    "guild-mute/delete"
                } else {
                    "guild-mute/create"
                };
                kook(
                    rl,
                    "POST",
                    path,
                    json!({"guild_id":guild(rl,&server)?,
                    "user_id":context(rl,&user,"发送者ID")?,"type":kind}),
                )?;
                empty()
            }
            "已加入" => {
                let data = kook(rl, "GET", "voice/list", json!({}))?;
                mapped_list(&data["data"]["items"], |row| {
                    let mut channel = kook_channel(row);
                    channel["类型"] = "语音".into();
                    channel
                })
            }
            "移动" => {
                let users = rl.get_param(params, 1)?;
                let target = rl.get_param_text(params, 2)?;
                let ids = array(&users)?
                    .iter()
                    .map(|user| {
                        let value = user.expect_text_value()?;
                        required(&value, "用户ID")?;
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if ids.is_empty() {
                    return Err("请至少填写一个用户ID".into());
                }
                kook(
                    rl,
                    "POST",
                    "channel/move-user",
                    json!({"target_id":required(&target,"语音频道ID")?,"user_ids":ids}),
                )?;
                empty()
            }
            "移出" => {
                let [user, channel] = text_params::<2>(rl, params, 1)?;
                kook(
                    rl,
                    "POST",
                    "channel/kickout",
                    json!({"channel_id":context(rl,&channel,"群ID")?,
                    "user_id":context(rl,&user,"发送者ID")?}),
                )?;
                empty()
            }
            _ => Err(
                "语音管理支持已加入、移动、移出、限制列表、闭麦、静音、解除闭麦或解除静音".into(),
            ),
        }
    });
}

pub(super) fn register() {
    assets::register();
    activity::register();
    templates::register();
    payments::register();
    location::register();
    business::register();
    add_fun(vec!["Telegram应答查询"], |rl, params| {
        let rows = rl.get_param(params, 0)?;
        let [cache, offset, personal, mut id] = text_params::<4>(rl, params, 1)?;
        if id.is_empty() {
            let event: Value = serde_json::from_str(&rl.get_exmap("原始事件"))
                .map_err(|_| "请在 Telegram 行内查询事件中应答或填写查询ID")?;
            if event["notice_type"] != "telegram" || event["sub_type"] != "inline_query" {
                return Err("请在 Telegram 行内查询事件中应答或填写查询ID".into());
            }
            id = event["telegram"]["id"]
                .as_str()
                .ok_or("事件缺少查询ID")?
                .to_owned();
        }
        if offset.len() > 64 {
            return Err("下一页偏移最多 64 字节".into());
        }
        let cache = if cache.is_empty() {
            0
        } else {
            bounded(&cache, 0, i32::MAX.into(), "缓存秒数")?
        };
        let mut body = json!({"inline_query_id":id,"cache_time":cache,
            "results":inline_results(&rows)?,"next_offset":offset,"is_personal":boolean(&personal,false)?});
        let button = rl.get_param(params,5)?;
        if !matches!(button.as_ref(),RedValue::Text(text) if text.is_empty()) {
            let row = array(&button)?;
            if row.len() != 3 { return Err("结果上方按钮用【数组@文字@类型@值】".into()); }
            let text = row_text(row,0)?;
            let kind = row_text(row,1)?;
            let value = row_text(row,2)?;
            let mut data = json!({"text":required(&text,"按钮文字")?});
            match kind.as_str() {
                "小程序" => { https_url(&value)?; data["web_app"] = json!({"url":value}); }
                "开始" if (1..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'_' | b'-')) => { data["start_parameter"] = value.into(); }
                _ => return Err("结果按钮类型请填小程序或开始，开始值用 1 至 64 位字母数字、下划线或连字符".into()),
            }
            body["button"] = data;
        }
        telegram(rl,"answerInlineQuery",body)?;
        empty()
    });
    add_fun(vec!["群信息"], |rl, params| {
        let target = rl.get_param_text(params, 0)?;
        match get_platform(rl).as_str() {
            "telegram" => {
                let data = telegram(rl, "getChat", json!({"chat_id":chat(rl,&target)?}))?;
                output(&telegram_chat(&data["data"]))
            }
            "kook" => {
                let target = context(rl, &target, "群ID")?;
                let include_children = rl.get_param_text(params, 1)?;
                let mut body = json!({"target_id":target});
                if !include_children.is_empty() {
                    body["need_children"] = boolean(&include_children, false)?.into();
                }
                let data = kook(rl, "GET", "channel/view", body)?;
                output(&kook_channel(&data["data"]))
            }
            _ => Err("群信息需要 KOOK 或 Telegram 机器人来源".into()),
        }
    });
    add_fun(vec!["群成员"], members);
    register_admin();
    register_kook();
}

pub(crate) fn telegram_avatars(
    rl: &RedLang,
    user: &str,
    offset: &str,
    limit: &str,
) -> RedCommandResult {
    let mut body = json!({"user_id":positive_id(&context(rl,user,"发送者ID")?)?});
    if !offset.is_empty() {
        body["offset"] = bounded(offset, 0, i32::MAX.into(), "跳过数量")?.into();
    }
    if !limit.is_empty() {
        body["limit"] = bounded(limit, 1, 100, "返回数量")?.into();
    }
    let data = telegram(rl, "getUserProfilePhotos", body)?;
    mapped_list(&data["data"]["photos"], |sizes| {
        sizes
            .as_array()
            .and_then(|sizes| {
                sizes.iter().max_by_key(|photo| {
                    photo["width"]
                        .as_u64()
                        .unwrap_or_default()
                        .saturating_mul(photo["height"].as_u64().unwrap_or_default())
                })
            })
            .map(|size| size["file_id"].clone())
            .unwrap_or_default()
    })
}
