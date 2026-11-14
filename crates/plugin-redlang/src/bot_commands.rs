use std::{collections::HashSet, error::Error, rc::Rc};

use serde_json::{json, Value};

use crate::core::{
    add_fun, astparser::Ast, rv_empty, rv_text, RedCommandResult, RedLang, RedValue,
};
use crate::cqexfun::{apply_telegram_topic, call_platform_api_value, get_platform};
use crate::exfun::json_value_to_red;

#[path = "bot_management.rs"]
pub(super) mod management;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn text_params<const N: usize>(
    rl: &mut RedLang,
    params: &[Ast],
    start: usize,
) -> Result<[String; N]> {
    let mut values = std::array::from_fn(|_| String::new());
    for (index, value) in values.iter_mut().enumerate() {
        *value = rl.get_param_text(params, start + index)?;
    }
    Ok(values)
}

fn array(value: &RedValue) -> Result<&[Rc<RedValue>]> {
    Ok(value.as_array().ok_or("请使用【数组】")?)
}

fn row_text(row: &[Rc<RedValue>], index: usize) -> Result<String> {
    Ok(row
        .get(index)
        .map(|v| v.expect_text_value())
        .transpose()?
        .unwrap_or_default())
}

fn boolean(value: &str, default: bool) -> Result<bool> {
    match value {
        "" => Ok(default),
        "真" => Ok(true),
        "假" => Ok(false),
        _ => Err("逻辑参数请填真或假".into()),
    }
}

fn chat_id(value: &str) -> Result<Value> {
    if let Ok(id) = value.parse::<i64>() {
        if id != 0 {
            return Ok(id.into());
        }
    }
    if value.starts_with('@') && value.len() > 1 {
        return Ok(value.into());
    }
    Err("请填写 Telegram 聊天 ID 或带 @ 的用户名".into())
}

fn positive_id(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| "请填写正整数 ID".into())
}

fn https_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("请填写 HTTPS 链接".into());
    }
    Ok(())
}

fn telegram(rl: &RedLang, method: &str, body: Value) -> Result<Value> {
    call_platform_api_value(
        rl,
        "telegram",
        "telegram_api",
        json!({"method":method,"body":body}),
    )
    .and_then(success)
}

fn success(response: Value) -> Result<Value> {
    if response["retcode"] != 0 {
        let message = response["message"]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap_or("平台未返回错误说明");
        return Err(RedLang::make_err(message));
    }
    Ok(response)
}

fn output(value: &Value) -> RedCommandResult {
    Ok(Some(json_value_to_red(value)?))
}

fn scope_body(scope: &str, language: &str, chat: &str, user: &str) -> Result<Value> {
    let kind = match scope {
        "" | "默认" => "default",
        "所有私聊" => "all_private_chats",
        "所有群聊" => "all_group_chats",
        "所有群管理员" => "all_chat_administrators",
        "指定聊天" => "chat",
        "指定群管理员" => "chat_administrators",
        "指定群成员" => "chat_member",
        _ => return Err("未知的菜单范围".into()),
    };
    let needs_chat = matches!(kind, "chat" | "chat_administrators" | "chat_member");
    if needs_chat != !chat.is_empty() || (kind == "chat_member") != !user.is_empty() {
        return Err("按菜单范围填写聊天 ID 和成员 ID".into());
    }
    let mut scope = json!({"type":kind});
    if needs_chat {
        scope["chat_id"] = chat_id(chat)?;
    }
    if kind == "chat_member" {
        scope["user_id"] = positive_id(user)?.into();
    }
    let mut body = json!({"scope":scope});
    if !language.is_empty() {
        if language.len() != 2 || !language.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err("语言请填两位小写字母，例如 zh".into());
        }
        body["language_code"] = language.into();
    }
    Ok(body)
}

fn menu_items(items: &RedValue) -> Result<Vec<Value>> {
    let items = array(items)?;
    if items.is_empty() || items.len() > 100 {
        return Err("命令菜单需要 1 至 100 项，清空请用删除".into());
    }
    let mut seen = HashSet::new();
    items
        .iter()
        .map(|item| {
            let row = array(item)?;
            if row.len() != 2 {
                return Err("每项用【数组@命令名@说明】填写".into());
            }
            let name = row_text(row, 0)?;
            let command = name.strip_prefix('/').unwrap_or(&name);
            let desc = row_text(row, 1)?;
            if !(1..=32).contains(&command.len())
                || !command
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                || !(1..=256).contains(&desc.chars().count())
                || !seen.insert(command.to_owned())
            {
                return Err("菜单命令名或说明无效，或命令名重复".into());
            }
            Ok(json!({"command":command,"description":desc}))
        })
        .collect()
}

fn keyboard(rows: &RedValue, mode: &str, resize: &str, once: &str) -> Result<Value> {
    if mode == "移除" {
        if !matches!(rows, RedValue::Text(text) if text.is_empty())
            || !resize.is_empty()
            || !once.is_empty()
        {
            return Err("移除回复键盘只填写移除模式".into());
        }
        return Ok(json!({"remove_keyboard":true}));
    }
    let inline = match mode {
        "" | "行内" => true,
        "回复" => false,
        _ => return Err("键盘模式请填行内、回复或移除".into()),
    };
    let mut result = Vec::new();
    for row in array(rows)? {
        let mut buttons = Vec::new();
        for button in array(row)? {
            if inline {
                buttons.push(inline_button(button)?);
            } else {
                let label = button.expect_text_value()?;
                if label.is_empty() {
                    return Err("回复按钮文字不能为空".into());
                }
                buttons.push(json!({"text":label}));
            }
        }
        if buttons.is_empty() {
            return Err("每行至少填写一个按钮".into());
        }
        result.push(buttons);
    }
    if result.is_empty() {
        return Err("键盘至少需要一行".into());
    }
    if inline {
        if !resize.is_empty() || !once.is_empty() {
            return Err("自动缩放和一次性仅用于回复键盘".into());
        }
        Ok(json!({"inline_keyboard":result}))
    } else {
        Ok(
            json!({"keyboard":result,"resize_keyboard":boolean(resize,true)?,"one_time_keyboard":boolean(once,false)?}),
        )
    }
}

fn inline_button(button: &RedValue) -> Result<Value> {
    let button = array(button)?;
    let label = row_text(button, 0)?;
    let value = row_text(button, 1)?;
    let action = row_text(button, 2)?;
    if !(2..=3).contains(&button.len()) || label.is_empty() || value.is_empty() {
        return Err("行内按钮用【数组@按钮文字@值@动作】填写".into());
    }
    match action.as_str() {
        "" | "回调" if value.len() <= 64 => Ok(json!({"text":label,"callback_data":value})),
        "" | "回调" => Err("Telegram 回调值最多 64 字节".into()),
        "链接" => {
            let url = url::Url::parse(&value)?;
            if !matches!(url.scheme(), "http" | "https" | "tg") {
                return Err("Telegram 按钮链接需使用 HTTP(S) 或 tg://".into());
            }
            Ok(json!({"text":label,"url":value}))
        }
        "小程序" => {
            https_url(&value)?;
            Ok(json!({"text":label,"web_app":{"url":value}}))
        }
        _ => Err("行内按钮动作请填回调、链接或小程序".into()),
    }
}

pub(super) fn edit_keyboard(content: &RedValue) -> Result<Value> {
    if matches!(content, RedValue::Text(text) if text.is_empty()) {
        return Ok(json!({"inline_keyboard":[]}));
    }
    let markup = crate::native_args::object(content)?;
    if !markup["inline_keyboard"].is_array() {
        return Err("编辑消息只接受行内键盘".into());
    }
    Ok(markup)
}

pub(super) fn card_args(rl: &mut RedLang, params: &[Ast], input: &RedValue) -> Result<Value> {
    if matches!(input, RedValue::Array(_)) {
        let old = crate::native_args::value(input)?;
        return Ok(old);
    }
    let body = input.expect_text_value()?;
    let rows = rl.get_param(params, 1)?;
    let [theme, size] = text_params::<2>(rl, params, 2)?;
    let theme = match theme.as_str() {
        "" | "蓝色" => "primary",
        "绿色" => "success",
        "红色" => "danger",
        "黄色" => "warning",
        "青色" => "info",
        "灰色" => "secondary",
        "无边框" => "none",
        _ => return Err("未知的卡片主题".into()),
    };
    let size = match size.as_str() {
        "" | "大" => "lg",
        "小" => "sm",
        _ => return Err("卡片大小请填大或小".into()),
    };
    let mut modules = Vec::new();
    if body.chars().count() > 2000 {
        return Err("KOOK 卡片正文最多 2000 字".into());
    }
    if !body.is_empty() {
        modules.push(json!({"type":"section","text":{"type":"plain-text","content":body}}));
    }
    if !matches!(rows.as_ref(), RedValue::Text(text) if text.is_empty()) {
        for row in array(&rows)? {
            let row = array(row)?;
            if !(1..=4).contains(&row.len()) {
                return Err("KOOK 卡片每行需要 1 至 4 个按钮".into());
            }
            let mut buttons = Vec::new();
            for button in row {
                let button = array(button)?;
                let label = row_text(button, 0)?;
                let value = row_text(button, 1)?;
                let action = row_text(button, 2)?;
                if !(2..=3).contains(&button.len()) || label.is_empty() || value.is_empty() {
                    return Err("卡片按钮用【数组@文字@值@动作】填写".into());
                }
                let click = match action.as_str() {
                    "" | "回调" => "return-val",
                    "链接" => {
                        let url = url::Url::parse(&value)?;
                        if !matches!(url.scheme(), "http" | "https") {
                            return Err("KOOK 卡片按钮需使用 HTTP(S) 链接".into());
                        }
                        "link"
                    }
                    _ => return Err("KOOK 卡片按钮动作请填回调或链接".into()),
                };
                buttons.push(json!({"type":"button","text":{"type":"plain-text","content":label},"click":click,"value":value}));
            }
            modules.push(json!({"type":"action-group","elements":buttons}));
        }
    }
    if modules.is_empty() || modules.len() > 50 {
        return Err("KOOK 卡片需要 1 至 50 个模块".into());
    }
    Ok(json!([{"type":"card","theme":theme,"size":size,"modules":modules}]))
}

pub(super) fn card_for_edit(content: &RedValue) -> Result<Value> {
    if matches!(content, RedValue::Array(_)) {
        return crate::native_args::value(content);
    }
    let cq = redlang::mytool::str_msg_to_arr(&json!(content.expect_text_value()?))?;
    let [item] = cq.as_array().map(Vec::as_slice).unwrap_or_default() else {
        return Err("卡片编辑需传一条【KOOK卡片】".into());
    };
    if item["type"] != "kook_card" {
        return Err("卡片编辑需传【KOOK卡片】".into());
    }
    Ok(serde_json::from_str(
        item["data"]["content"].as_str().ok_or("卡片缺少内容")?,
    )?)
}

fn poll_body(
    question: &str,
    options: &RedValue,
    anonymous: &str,
    multiple: &str,
    correct: &RedValue,
) -> Result<Value> {
    let options = array(options)?;
    if !(1..=12).contains(&options.len()) || !(1..=300).contains(&question.chars().count()) {
        return Err("投票需要 1 至 300 字的题目和 1 至 12 个选项".into());
    }
    let count = options.len();
    let options: Vec<Value> = options
        .iter()
        .map(|item| {
            let text = item.expect_text_value()?;
            if !(1..=100).contains(&text.chars().count()) {
                return Err("投票选项需要 1 至 100 字".into());
            }
            Ok(json!({"text":text}))
        })
        .collect::<Result<_>>()?;
    let mut body = json!({
        "question":question,"options":options,
        "is_anonymous":boolean(anonymous,true)?,"allows_multiple_answers":boolean(multiple,false)?
    });
    if !matches!(correct, RedValue::Text(text) if text.is_empty()) {
        let mut indices = match correct {
            RedValue::Array(items) => items
                .iter()
                .map(|item| item.expect_text_value())
                .collect::<Result<Vec<_>>>()?,
            _ => vec![correct.expect_text_value()?],
        }
        .iter()
        .map(|id| {
            id.parse::<usize>()
                .ok()
                .filter(|id| (1..=count).contains(id))
                .map(|id| id - 1)
                .ok_or_else(|| "正确选项从 1 开始，须在选项范围内".into())
        })
        .collect::<Result<Vec<_>>>()?;
        indices.sort_unstable();
        if indices.is_empty() || indices.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("正确选项不能为空或重复".into());
        }
        if indices.len() > 1 && body["allows_multiple_answers"] != true {
            return Err("多个正确选项需要开启多选".into());
        }
        body["type"] = "quiz".into();
        body["correct_option_ids"] = json!(indices);
    }
    Ok(body)
}

fn invite_target(rl: &RedLang, platform: &str, id: &str, kind: &str) -> Result<Value> {
    if platform == "telegram" {
        if !matches!(kind, "" | "群聊" | "频道") {
            return Err("Telegram 邀请目标可填群聊或频道".into());
        }
        let value = rl.get_exmap("群ID");
        return Ok(json!({"chat_id":chat_id(if id.is_empty() { &value } else { id })?}));
    }
    let (field, context) = match kind {
        "" | "频道" => ("channel_id", "群ID"),
        "服务器" => ("guild_id", "群组ID"),
        _ => return Err("KOOK 邀请目标请填频道或服务器".into()),
    };
    let value = rl.get_exmap(context);
    let id = if id.is_empty() { &value } else { id };
    if id.is_empty() {
        return Err("请填写邀请目标 ID".into());
    }
    Ok(json!({field:id}))
}

fn invite_options(
    body: &mut Value,
    platform: &str,
    duration: &str,
    limit: &str,
    name: &str,
    approve: &str,
) -> Result<()> {
    let duration = (!duration.is_empty())
        .then(|| duration.parse::<i64>())
        .transpose()?;
    let limit = (!limit.is_empty())
        .then(|| limit.parse::<i64>())
        .transpose()?;
    if platform == "telegram" {
        let approval = boolean(approve, false)?;
        if name.chars().count() > 32
            || (approval && limit.is_some())
            || limit.is_some_and(|n| !(1..=99999).contains(&n))
        {
            return Err(
                "Telegram 邀请名称最多 32 字，人数上限为 1 至 99999，审核链接请省略人数上限".into(),
            );
        }
        if let Some(seconds) = duration {
            let expiry = if seconds == 0 {
                0
            } else {
                chrono::Utc::now()
                    .timestamp()
                    .checked_add(seconds)
                    .filter(|_| seconds > 0)
                    .ok_or("邀请有效期需要非负秒数")?
            };
            body["expire_date"] = expiry.into();
        }
        if let Some(limit) = limit {
            body["member_limit"] = limit.into();
        }
        if !name.is_empty() {
            body["name"] = name.into();
        }
        if !approve.is_empty() {
            body["creates_join_request"] = approval.into();
        }
    } else {
        if !name.is_empty() || !approve.is_empty() {
            return Err("邀请名称和入群审核适用于 Telegram".into());
        }
        if let Some(seconds) = duration {
            if ![0, 1800, 3600, 21600, 43200, 86400, 604800].contains(&seconds) {
                return Err("KOOK 邀请有效期需填写平台支持的秒数".into());
            }
            body["duration"] = seconds.into();
        }
        if let Some(limit) = limit {
            if ![-1, 1, 5, 10, 25, 50, 100].contains(&limit) {
                return Err("KOOK 邀请次数需填写平台支持的值".into());
            }
            body["setting_times"] = limit.into();
        }
    }
    Ok(())
}

fn kook(rl: &RedLang, method: &str, path: &str, body: Value) -> Result<Value> {
    call_platform_api_value(
        rl,
        "kook",
        "kook_api",
        json!({"http_method":method,"endpoint":path,"body":body}),
    )
    .and_then(success)
}

fn invite_code(input: &str) -> Result<String> {
    if input.contains("://") {
        let url = url::Url::parse(input)?;
        return url
            .path_segments()
            .and_then(|segments| segments.filter(|part| !part.is_empty()).next_back())
            .map(str::to_owned)
            .ok_or_else(|| "邀请链接缺少邀请码".into());
    }
    if input.is_empty() {
        return Err("请填写邀请链接或邀请码".into());
    }
    Ok(input.to_owned())
}

pub(super) fn event_field(event: &Value, field: &str) -> RedCommandResult {
    let telegram = &event["telegram"];
    let value = match field {
        "按钮值" => match (event["notice_type"].as_str(), event["sub_type"].as_str()) {
            (Some("telegram"), Some("callback_query")) => telegram["data"].clone(),
            (Some("kook"), Some("message_btn_click")) => event["kook"]["value"].clone(),
            _ => Value::Null,
        },
        "平台消息ID" => match event["platform"].as_str() {
            Some("kook") => event
                .get("kook_message_id")
                .or_else(|| event["kook"].get("msg_id"))
                .cloned()
                .unwrap_or_default(),
            Some("telegram") => telegram
                .get("message_id")
                .or_else(|| telegram["message"].get("message_id"))
                .cloned()
                .unwrap_or_default(),
            _ => Value::Null,
        },
        "投票选项" if event["sub_type"] == "poll_answer" => {
            let ids = telegram["option_ids"]
                .as_array()
                .ok_or("投票事件缺少选项")?;
            json!(ids
                .iter()
                .filter_map(|id| id.as_u64()?.checked_add(1))
                .collect::<Vec<_>>())
        }
        "投票人数" if event["sub_type"] == "poll" => telegram["total_voter_count"].clone(),
        "查询内容" if event["sub_type"] == "inline_query" => telegram["query"].clone(),
        "查询偏移" if event["sub_type"] == "inline_query" => telegram["offset"].clone(),
        "话题ID" => event["message_thread_id"]
            .as_i64()
            .or_else(|| telegram["message_thread_id"].as_i64())
            .into(),
        "频道私信话题ID" => event["direct_messages_topic_id"]
            .as_i64()
            .or_else(|| telegram["direct_messages_topic"]["topic_id"].as_i64())
            .into(),
        "文件ID" => telegram["document"]["file_id"].clone(),
        "文件名" => telegram["document"]["file_name"]
            .as_str()
            .or_else(|| event["file"]["name"].as_str())
            .into(),
        "文件链接" => event["file"]["url"].clone(),
        "贴纸文件ID" => telegram["sticker"]["file_id"].clone(),
        "贴纸包" => telegram["sticker"]["set_name"].clone(),
        "曲目序号" => event["kook"]["track"].clone(),
        "曲目总数" => event["kook"]["total"].clone(),
        "播放状态" => playback_state(&event["kook"]["state"]),
        "转发来源类型" => match telegram["forward_origin"]["type"].as_str() {
            Some("user") => "用户".into(),
            Some("hidden_user") => "隐藏用户".into(),
            Some("chat") => "群聊".into(),
            Some("channel") => "频道".into(),
            _ => Value::Null,
        },
        "新回应" | "旧回应" => {
            let key = if field == "新回应" {
                "new_reaction"
            } else {
                "old_reaction"
            };
            json!(telegram[key]
                .as_array()
                .into_iter()
                .flatten()
                .map(telegram_reaction)
                .collect::<Vec<_>>())
        }
        "投票选项" | "投票人数" | "查询内容" | "查询偏移" => Value::Null,
        _ => return Err(format!("【事件内容】不支持字段 {field}").into()),
    };
    output(&value)
}

fn telegram_reaction(row: &Value) -> Value {
    match row["type"].as_str() {
        Some("emoji") => json!({"类型":"普通","表情":row["emoji"]}),
        Some("custom_emoji") => json!({"类型":"自定义","表情":row["custom_emoji_id"]}),
        Some("paid") => json!({"类型":"付费"}),
        _ => json!({"类型":"其他"}),
    }
}

pub(super) fn playback_state(state: &Value) -> Value {
    match state.as_str() {
        Some("idle") => "空闲".into(),
        Some("playing") => "播放中".into(),
        Some("stopping") => "停止中".into(),
        Some("finished") => "已播完".into(),
        Some("stopped") => "已停止".into(),
        Some("failed") => "失败".into(),
        _ => Value::Null,
    }
}

pub(super) fn register() {
    management::register();
    add_fun(vec!["Telegram命令菜单"], |rl, params| {
        let operation = rl.get_param_text(params, 0)?;
        let (scope_start, method) = match operation.as_str() {
            "设置" => (2, "setMyCommands"),
            "查询" => (1, "getMyCommands"),
            "删除" => (1, "deleteMyCommands"),
            _ => return Err("命令菜单操作请填设置、查询或删除".into()),
        };
        let [scope, language, chat, user] = text_params::<4>(rl, params, scope_start)?;
        let mut body = scope_body(&scope, &language, &chat, &user)?;
        if operation == "设置" {
            let items = rl.get_param(params, 1)?;
            body["commands"] = json!(menu_items(&items)?);
        }
        let response = telegram(rl, method, body)?;
        if operation == "查询" {
            let commands = response["data"].as_array().ok_or("平台未返回命令列表")?;
            return output(&json!(commands
                .iter()
                .map(|row| json!([row["command"], row["description"]]))
                .collect::<Vec<_>>()));
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["Telegram菜单按钮"], |rl, params| {
        let [kind, target, label, link] = text_params::<4>(rl, params, 0)?;
        let mut body = json!({});
        if !target.is_empty() {
            body["chat_id"] = positive_id(&target)?.into();
        }
        let method = match kind.as_str() {
            "查询" if label.is_empty() && link.is_empty() => "getChatMenuButton",
            "命令" | "默认" if label.is_empty() && link.is_empty() => {
                body["menu_button"] =
                    json!({"type":if kind == "命令" { "commands" } else { "default" }});
                "setChatMenuButton"
            }
            "小程序" if !label.is_empty() => {
                https_url(&link)?;
                body["menu_button"] = json!({"type":"web_app","text":label,"web_app":{"url":link}});
                "setChatMenuButton"
            }
            _ => {
                return Err(
                    "菜单按钮类型请填命令、默认、查询或小程序，小程序需要按钮文字和链接".into(),
                )
            }
        };
        let response = telegram(rl, method, body)?;
        if kind == "查询" {
            let data = &response["data"];
            let label = match data["type"].as_str() {
                Some("commands") => "命令",
                Some("web_app") => "小程序",
                _ => "默认",
            };
            return output(&json!([label, data["text"], data["web_app"]["url"]]));
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["Telegram键盘"], |rl, params| {
        let rows = rl.get_param(params, 0)?;
        let [mode, resize, once] = text_params::<3>(rl, params, 1)?;
        output(&keyboard(&rows, &mode, &resize, &once)?)
    });
    add_fun(vec!["Telegram应答按钮"], |rl, params| {
        let [text, alert, mut id] = text_params::<3>(rl, params, 0)?;
        if id.is_empty() {
            let event: Value = serde_json::from_str(&rl.get_exmap("原始事件"))?;
            if event["sub_type"] != "callback_query" || event["notice_type"] != "telegram" {
                return Err("请在 Telegram 按钮事件中应答或填写回调 ID".into());
            }
            id = event["telegram"]["id"]
                .as_str()
                .ok_or("事件缺少回调 ID")?
                .to_owned();
        }
        if text.chars().count() > 200 {
            return Err("按钮应答提示最多 200 字".into());
        }
        let mut body = json!({"callback_query_id":id,"show_alert":boolean(&alert,false)?});
        if !text.is_empty() {
            body["text"] = text.into();
        }
        telegram(rl, "answerCallbackQuery", body)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["Telegram投票"], |rl, params| {
        let question = rl.get_param_text(params, 0)?;
        let options = rl.get_param(params, 1)?;
        let [anonymous, multiple] = text_params::<2>(rl, params, 2)?;
        let correct = rl.get_param(params, 4)?;
        let [topic] = text_params::<1>(rl, params, 5)?;
        let mut body = poll_body(&question, &options, &anonymous, &multiple, &correct)?;
        let target = rl.get_exmap("群ID");
        let private = rl.get_exmap("发送者ID");
        let target = if target.is_empty() {
            private.as_str()
        } else {
            target.as_str()
        };
        body["chat_id"] = chat_id(target)?;
        if !topic.is_empty() {
            body["message_thread_id"] = positive_id(&topic)?.into();
        } else if let Ok(event) = serde_json::from_str(&rl.get_exmap("原始事件")) {
            apply_telegram_topic(&event, &rl.get_exmap("机器人ID"), target, &mut body);
            if body.get("direct_messages_topic_id").is_some() {
                return Err("Telegram 投票不适用于频道私信".into());
            }
        }
        let response = telegram(rl, "sendPoll", body)?;
        Ok(Some(rv_text(
            response["redreply_message_id"]
                .as_str()
                .ok_or("平台未返回投票消息 ID")?
                .to_owned(),
        )))
    });
    add_fun(vec!["Telegram结束投票"], |rl, params| {
        let mut id = rl.get_param_text(params, 0)?;
        if id.is_empty() {
            id = rl.get_exmap("消息ID").to_string();
        }
        let result = success(call_platform_api_value(
            rl,
            "telegram",
            "telegram_stop_poll",
            json!({"message_id":id}),
        )?)?;
        output(&result["data"]["total_voter_count"])
    });
    add_fun(vec!["邀请链接"], |rl, params| {
        let operation = rl.get_param_text(params, 0)?;
        let platform = get_platform(rl);
        if !matches!(platform.as_str(), "telegram" | "kook") {
            return Err("【邀请链接】需要 KOOK 或 Telegram 机器人来源".into());
        }
        match operation.as_str() {
            "主链接" | "创建付费" | "编辑付费" if platform == "telegram" => {
                let [first, name, target] = text_params::<3>(rl, params, 1)?;
                if name.chars().count() > 32 {
                    return Err("邀请名称最多 32 字".into());
                }
                let mut body = invite_target(
                    rl,
                    &platform,
                    if operation == "主链接" {
                        &first
                    } else {
                        &target
                    },
                    "",
                )?;
                if !name.is_empty() {
                    body["name"] = if name == "清空" && operation == "编辑付费" {
                        ""
                    } else {
                        &name
                    }
                    .into();
                }
                let method = match operation.as_str() {
                    "主链接" => "exportChatInviteLink",
                    "创建付费" => {
                        let price = first
                            .parse::<i64>()
                            .ok()
                            .filter(|v| (1..=10_000).contains(v))
                            .ok_or("订阅价格请填写 1 至 10000")?;
                        body["subscription_price"] = price.into();
                        body["subscription_period"] = 2_592_000.into();
                        "createChatSubscriptionInviteLink"
                    }
                    _ => {
                        https_url(&first)?;
                        body["invite_link"] = first.into();
                        "editChatSubscriptionInviteLink"
                    }
                };
                let data = telegram(rl, method, body)?;
                let link = if operation == "主链接" {
                    &data["data"]
                } else {
                    &data["data"]["invite_link"]
                };
                Ok(Some(rv_text(
                    link.as_str().ok_or("平台未返回邀请链接")?.to_owned(),
                )))
            }
            "创建" | "编辑" => {
                let edit = operation == "编辑";
                if edit && platform != "telegram" {
                    return Err("编辑邀请适用于 Telegram".into());
                }
                let start = if edit { 2 } else { 1 };
                let [id, kind, duration, limit, name, approve] =
                    text_params::<6>(rl, params, start)?;
                let mut body = invite_target(rl, &platform, &id, &kind)?;
                invite_options(&mut body, &platform, &duration, &limit, &name, &approve)?;
                let link = if edit {
                    let link = rl.get_param_text(params, 1)?;
                    https_url(&link)?;
                    body["invite_link"] = link.into();
                    telegram(rl, "editChatInviteLink", body)?["data"]["invite_link"].clone()
                } else if platform == "telegram" {
                    telegram(rl, "createChatInviteLink", body)?["data"]["invite_link"].clone()
                } else {
                    kook(rl, "POST", "invite/create", body)?["data"]["url"].clone()
                };
                Ok(Some(rv_text(
                    link.as_str().ok_or("平台未返回邀请链接")?.to_owned(),
                )))
            }
            "撤销" => {
                let [link, id, kind] = text_params::<3>(rl, params, 1)?;
                let mut body = invite_target(rl, &platform, &id, &kind)?;
                if platform == "telegram" {
                    https_url(&link)?;
                    body["invite_link"] = link.into();
                    telegram(rl, "revokeChatInviteLink", body)?;
                } else {
                    body["url_code"] = invite_code(&link)?.into();
                    kook(rl, "POST", "invite/delete", body)?;
                }
                Ok(Some(rv_empty()))
            }
            "列表" if platform == "kook" => {
                let [id, kind, page] = text_params::<3>(rl, params, 1)?;
                let mut body = invite_target(rl, &platform, &id, &kind)?;
                body["page"] = positive_id(if page.is_empty() { "1" } else { &page })?.into();
                let response = kook(rl, "GET", "invite/list", body)?;
                let items = response["data"]["items"]
                    .as_array()
                    .ok_or("平台未返回邀请列表")?;
                output(&json!(items
                    .iter()
                    .filter_map(|item| item["url"].as_str())
                    .collect::<Vec<_>>()))
            }
            "受邀人数" if platform == "kook" => {
                let [invite, guild] = text_params::<2>(rl, params, 1)?;
                let mut body = json!({"id":invite_code(&invite)?,"page":1,"page_size":20});
                if !guild.is_empty() {
                    body["guild_id"] = guild.into();
                }
                let result = kook(rl, "GET", "invite/invitees", body)?;
                output(&result["data"]["count"])
            }
            _ => Err("邀请操作支持创建、撤销、Telegram 编辑及付费邀请、KOOK 列表和受邀人数".into()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{rv_array, rv_text};

    fn texts(items: &[&str]) -> Rc<RedValue> {
        rv_array(
            items
                .iter()
                .map(|value| rv_text((*value).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn chinese_menu_arguments_build_the_explicit_platform_scope() {
        assert_eq!(
            scope_body("所有私聊", "zh", "", "").unwrap(),
            json!({
                "scope":{"type":"all_private_chats"},"language_code":"zh"
            })
        );
        assert_eq!(
            scope_body("指定群成员", "", "-1001", "42").unwrap(),
            json!({
                "scope":{"type":"chat_member","chat_id":-1001,"user_id":42}
            })
        );
        assert!(scope_body("默认", "", "-1001", "").is_err());
        assert!(scope_body("指定群成员", "", "-1001", "").is_err());
        assert_eq!(
            menu_items(&rv_array(vec![texts(&["/start", "开始使用"])])).unwrap(),
            vec![json!({"command":"start","description":"开始使用"})]
        );
    }

    #[test]
    fn positional_keyboard_and_poll_preserve_values_and_defaults() {
        let inline = rv_array(vec![rv_array(vec![texts(&["下一页", "001"])])]);
        assert_eq!(
            keyboard(&inline, "", "", "").unwrap(),
            json!({
                "inline_keyboard":[[{"text":"下一页","callback_data":"001"}]]
            })
        );
        let reply = keyboard(&rv_array(vec![texts(&["帮助"])]), "回复", "", "").unwrap();
        assert_eq!(reply["resize_keyboard"], true);
        assert_eq!(reply["one_time_keyboard"], false);
        assert!(edit_keyboard(&json_value_to_red(&reply).unwrap()).is_err());
        assert_eq!(
            keyboard(&rv_empty(), "移除", "", "").unwrap(),
            json!({"remove_keyboard":true})
        );

        let ordinary = poll_body(
            "下一次活动选哪天？",
            &texts(&["周六", "周日"]),
            "",
            "",
            &rv_empty(),
        )
        .unwrap();
        assert_eq!(ordinary["is_anonymous"], true);
        assert_eq!(
            ordinary["options"],
            json!([{"text":"周六"},{"text":"周日"}])
        );
        let quiz = poll_body(
            "正确的是？",
            &texts(&["甲", "乙", "丙"]),
            "假",
            "真",
            &texts(&["3", "1"]),
        )
        .unwrap();
        assert_eq!(quiz["correct_option_ids"], json!([0, 2]));
        assert!(poll_body(
            "正确的是？",
            &texts(&["甲", "乙"]),
            "假",
            "",
            &texts(&["0"])
        )
        .is_err());
    }

    #[test]
    fn invitations_follow_provider_specific_defaults() {
        let mut body = json!({});
        invite_options(&mut body, "telegram", "", "", "", "").unwrap();
        assert_eq!(body, json!({}));
        invite_options(&mut body, "telegram", "0", "", "活动", "假").unwrap();
        assert_eq!(
            body,
            json!({"expire_date":0,"name":"活动","creates_join_request":false})
        );
        assert!(invite_options(&mut body, "telegram", "", "5", "", "真").is_err());

        let mut body = json!({});
        invite_options(&mut body, "kook", "3600", "5", "", "").unwrap();
        assert_eq!(body, json!({"duration":3600,"setting_times":5}));
        assert!(invite_options(&mut body, "kook", "3601", "5", "", "").is_err());
        assert_eq!(
            invite_code("https://kook.top/example?x=1").unwrap(),
            "example"
        );
    }
}
