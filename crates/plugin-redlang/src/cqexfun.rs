use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
    sync::Arc,
    thread,
    time::SystemTime,
    vec,
};

use crate::bot_commands::management::business;
use crate::bot_commands::telegram_media;
use crate::core::{
    add_fun, get_const_val, get_temp_const_val, rv_array, rv_bin, rv_empty, rv_object, rv_text,
    set_const_val, set_temp_const_val, RedCommandResult, RedLang, RedValue, RedValueData,
    CLEAR_UUID,
};
use crate::exfun::json_value_to_red;
use crate::lifecycle::pkg_can_run;
use crate::native_args;
use crate::status::{add_send_group_msg, add_send_private_msg};
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use redlang::cqapi::{cq_add_log_w, cq_call_api, cq_get_app_directory1, cq_get_app_directory2};
use redlang::mytool::{cq_params_encode, cq_text_encode, json_to_cq_str, read_json_str};
use redlang::resource_manager::{add_file_lock, del_file_lock};
use redlang::runtime;
use redlang::runtime_context;
use redlang::PAGING_UUID;
use url::Url;
const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose =
    engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

pub fn get_app_dir(pkg_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let app_dir = if pkg_name.is_empty() {
        cq_get_app_directory2().map_err(|error| error.to_string())?
    } else {
        let plus_dir_str = cq_get_app_directory1().map_err(|error| error.to_string())?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        pkg_dir
            .join(pkg_name)
            .to_str()
            .ok_or("获得应用目录失败")?
            .to_owned()
            + std::path::MAIN_SEPARATOR_STR
    };
    Ok(app_dir)
}

pub(super) fn get_platform(self_t: &RedLang) -> String {
    let platform = self_t.get_exmap("机器人平台");
    (*platform).to_owned()
}

fn call_platform_api(
    rl: &RedLang,
    platform: &str,
    action: &str,
    params: serde_json::Value,
) -> RedCommandResult {
    let response = call_platform_api_value(rl, platform, action, params)?;
    Ok(Some(json_value_to_red(&response)?))
}

#[cfg(test)]
type TestPlatformCall = Box<dyn Fn(&str, &str, serde_json::Value) -> serde_json::Value>;

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_PLATFORM_CALL: RefCell<Option<TestPlatformCall>> = RefCell::new(None);
}

pub(super) fn call_platform_api_value(
    rl: &RedLang,
    platform: &str,
    action: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if get_platform(rl) != platform {
        return Err(RedLang::make_err(&format!(
            "此命令需要 {platform} 机器人来源"
        )));
    }
    #[cfg(test)]
    if let Some(response) = TEST_PLATFORM_CALL.with_borrow(|handler| {
        handler
            .as_ref()
            .map(|handler| handler(platform, action, params.clone()))
    }) {
        return Ok(response);
    }
    let self_id = rl.get_exmap("机器人ID");
    if self_id.is_empty() {
        return Err(RedLang::make_err("请使用【设置来源】指定机器人ID"));
    }
    let request = serde_json::json!({"action": action, "params": params});
    let result = cq_call_api(
        platform,
        &self_id,
        &rl.get_exmap("消息ID"),
        &request.to_string(),
        &rl.get_exmap("远程MQTT客户端ID"),
    );
    let response: serde_json::Value = serde_json::from_str(&result)?;
    if !response.is_object() {
        return Err(RedLang::make_err(
            "原生 API 未收到响应，请检查机器人是否已连接",
        ));
    }
    Ok(response)
}

pub(crate) fn telegram_join_request(
    event: &serde_json::Value,
    approve: bool,
) -> Option<(&'static str, serde_json::Value)> {
    if event["notice_type"] != "telegram" || event["sub_type"] != "chat_join_request" {
        return None;
    }
    let request = &event["telegram"];
    if let Some(query_id) = request["query_id"].as_str().filter(|id| !id.is_empty()) {
        return Some((
            "answerChatJoinRequestQuery",
            serde_json::json!({
                "chat_join_request_query_id": query_id,
                "result": if approve { "approve" } else { "decline" },
            }),
        ));
    }
    let chat_id = request["chat"]["id"].as_i64()?;
    let user_id = request["from"]["id"].as_i64()?;
    let method = if approve {
        "approveChatJoinRequest"
    } else {
        "declineChatJoinRequest"
    };
    Some((
        method,
        serde_json::json!({"chat_id": chat_id, "user_id": user_id}),
    ))
}

fn handle_telegram_join_request(
    rl: &RedLang,
    event: &serde_json::Value,
    approve: bool,
) -> RedCommandResult {
    let Some((method, body)) = telegram_join_request(event, approve) else {
        return Err(RedLang::make_err("Telegram 入群申请缺少聊天或用户 ID"));
    };
    let response = call_platform_api(
        rl,
        "telegram",
        "telegram_api",
        serde_json::json!({"method": method, "body": body}),
    )?;
    check_native_command(response, "Telegram 入群审核")
}

fn check_native_command(result: Option<Rc<RedValue>>, action: &str) -> RedCommandResult {
    let response = result.ok_or_else(|| RedLang::make_err(&format!("{action}未返回响应")))?;
    let RedValue::Object(fields) = response.as_ref() else {
        return Err(RedLang::make_err(&format!("{action}返回格式错误")));
    };
    let code = fields
        .get("retcode")
        .ok_or("缺少 retcode")?
        .expect_text_value()?;
    if code != "0" {
        let message = fields
            .get("message")
            .ok_or("缺少 message")?
            .expect_text_value()?;
        return Err(RedLang::make_err(&format!(
            "{action}失败（{code}）{message}"
        )));
    }
    Ok(Some(rv_empty()))
}

fn get_sub_id(rl: &RedLang, msg_type: &str) -> String {
    if msg_type == "group" || msg_type == "private_temp" {
        rl.get_exmap("群ID").to_string()
    } else {
        "".to_owned()
    }
}

/// 是否 QQ 官方机器人 C2C 单聊（仅该场景支持流式消息）。
fn is_qq_c2c(self_t: &RedLang) -> bool {
    get_platform(self_t) == "qqgroup_public" && *self_t.get_exmap("消息类型") == "private"
}

/// 流式分片序号存放在 exmap 的内部键中，脚本不可见。
fn stream_index_key(session_id: &str) -> String {
    format!("__流式序号@{session_id}")
}

/// 不支持流式的平台：缓冲后续分片内容，结束时整条发出（键同样脚本不可见）。
fn stream_buffer_key(session_id: &str) -> String {
    format!("__流式缓冲@{session_id}")
}

/// 调用流式消息 action；失败记日志并返回 retcode 与原始响应。
fn call_stream_action(
    self_t: &RedLang,
    action_json: serde_json::Value,
) -> Result<(i64, String), Box<dyn std::error::Error>> {
    let self_id = self_t.get_exmap("机器人ID");
    let platform = get_platform(self_t);
    let passive_id = self_t.get_exmap("消息ID");
    let remote_id = self_t.get_exmap("远程MQTT客户端ID");
    let cq_ret = cq_call_api(
        &platform,
        &self_id,
        &passive_id,
        &action_json.to_string(),
        &remote_id,
    );
    let ret_json: serde_json::Value =
        serde_json::from_str(&cq_ret).unwrap_or(serde_json::Value::Null);
    let retcode = ret_json
        .get("retcode")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    if retcode != 0 {
        let detail = if ret_json.is_string() {
            format!("{cq_ret}（未匹配到机器人连接，详见适配器日志）")
        } else {
            cq_ret.clone()
        };
        let _ = cq_add_log_w(&format!("流式消息调用失败:{detail}"));
    }
    Ok((retcode, cq_ret))
}

fn hex_to_num(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_path_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut ret = Vec::with_capacity(bytes.len());
    let mut changed = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_to_num(bytes[i + 1]), hex_to_num(bytes[i + 2])) {
                ret.push(high * 16 + low);
                i += 3;
                changed = true;
                continue;
            }
        }
        ret.push(bytes[i]);
        i += 1;
    }
    if changed {
        String::from_utf8_lossy(&ret).to_string()
    } else {
        segment.to_string()
    }
}

fn infer_file_name_from_resource(resource: &str) -> Option<String> {
    if resource.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(resource) {
        if url.scheme() == "http" || url.scheme() == "https" {
            return url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(percent_decode_path_segment)
                .filter(|name| !name.is_empty());
        }
    }
    if resource.ends_with('/') || resource.ends_with('\\') {
        return None;
    }
    resource
        .rsplit(['/', '\\'])
        .next()
        .map(str::to_string)
        .filter(|name| !name.is_empty())
}

fn resolve_file_name(name: &str, resource: Option<&str>) -> String {
    if !name.is_empty() {
        return name.to_string();
    }
    resource
        .and_then(infer_file_name_from_resource)
        .unwrap_or_else(|| "未命名".to_string())
}

pub(crate) fn apply_telegram_topic(
    event: &serde_json::Value,
    bot_id: &str,
    chat_id: &str,
    params: &mut serde_json::Value,
) {
    let native = &event["telegram"];
    let original_chat = native["chat"]["id"]
        .as_i64()
        .or_else(|| native["message"]["chat"]["id"].as_i64());
    if original_chat.is_none()
        || chat_id.parse::<i64>().ok() != original_chat
        || event["self_id"].as_str() != Some(bot_id)
    {
        return;
    }
    for field in ["message_thread_id", "direct_messages_topic_id"] {
        if let Some(value) = event.get(field) {
            params[field] = value.clone();
        }
    }
}

pub(crate) fn telegram_output_topic(
    id: &str,
    kind: &str,
) -> Result<Option<(&'static str, i64)>, Box<dyn std::error::Error>> {
    if id.is_empty() && kind.is_empty() {
        return Ok(None);
    }
    let field = match kind {
        "" | "话题" => "message_thread_id",
        "频道私信" => "direct_messages_topic_id",
        _ => return Err(RedLang::make_err("Telegram 话题类型需为话题或频道私信")),
    };
    let id = id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| RedLang::make_err("Telegram 话题 ID 需要正整数"))?;
    Ok(Some((field, id)))
}

pub fn send_one_msg(rl: &RedLang, msg: &str) -> Result<String, Box<dyn std::error::Error>> {
    send_one_msg_with_options(rl, msg, None, None, "", "", serde_json::Value::Null)
}

fn send_one_msg_with_options(
    rl: &RedLang,
    msg: &str,
    reply_markup: Option<serde_json::Value>,
    topic: Option<(&'static str, i64)>,
    business_id: &str,
    business_peer: &str,
    send_options: serde_json::Value,
) -> Result<String, Box<dyn std::error::Error>> {
    if msg.is_empty() {
        return Ok("".to_string());
    }

    let arr_msg = redlang::mytool::str_msg_to_arr(&serde_json::json!(msg))?;
    let platform = get_platform(rl);
    let mut msg_type: &'static str = crate::cqevent::get_msg_type(rl);
    let mut peer = rl.get_exmap("发送者ID").to_string();
    if platform == "telegram" {
        if !business_peer.is_empty() {
            if business_id.is_empty() || business_id == "机器人" {
                return Err("指定商务对方ID时请同时填写商务连接ID".into());
            }
            peer = business_peer
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or("商务对方ID需要正整数")?
                .to_string();
            msg_type = "private";
        } else if matches!(msg_type, "" | "private") {
            if let Some(chat_id) = business::default_peer(rl) {
                peer = chat_id.to_string();
                msg_type = "private";
            }
        }
        if !business_id.is_empty() && business_id != "机器人" && msg_type != "private" {
            return Err("以商务账户发送时需要目标私聊或商务对方ID".into());
        }
    }
    if msg_type.is_empty()
        && platform == "telegram"
        && send_options["ephemeral_message_parameters"]["callback_query_id"].is_string()
    {
        msg_type = "private";
    }
    let sub_id = get_sub_id(rl, msg_type);

    // 没有设置输出流类型，所以不输出
    if msg_type.is_empty() {
        return Ok("".to_string());
    }
    let mut send_json: serde_json::Value;
    if msg_type == "group" {
        send_json = serde_json::json!({
            "action":"send_group_msg",
            "params":{
                "group_id":sub_id,
                "message":arr_msg
            }
        });
    } else if msg_type == "private" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":peer,
                "message":arr_msg
            }
        });
    } else if msg_type == "private_temp" {
        send_json = serde_json::json!( {
            "action":"send_private_msg",
            "params":{
                "user_id":(*rl.get_exmap("发送者ID")),
                "group_id":sub_id,
                "message":arr_msg
            }
        });
    } else {
        return Err(RedLang::make_err(
            &("不支持的输出流:".to_string() + msg_type),
        ));
    }
    let self_id = rl.get_exmap("机器人ID");
    if platform == "telegram" {
        if let Some(reply_markup) = reply_markup {
            send_json["params"]["reply_markup"] = reply_markup;
        }
        let source = rl.get_exmap("原始事件");
        let target = if msg_type == "group" {
            sub_id.clone()
        } else {
            peer
        };
        let event: serde_json::Value = serde_json::from_str(&source).unwrap_or_default();
        if msg_type == "group"
            && event["self_id"].as_str() == Some(self_id.as_str())
            && event["chat_id"].as_str() == Some(target.as_str())
            && matches!(
                event["sub_type"].as_str(),
                Some("ephemeral_message" | "edited_ephemeral_message")
            )
            && send_options["ephemeral_message_parameters"].is_null()
        {
            return Err("回复 Telegram 临时消息时，请填写临时消息选项以限定接收者".into());
        }
        if msg_type == "private" && !target.is_empty() {
            let chat_id = target.parse::<i64>()?;
            if let Some(id) = business::send_context(rl, &serde_json::json!(chat_id), business_id) {
                if topic.is_some() {
                    return Err("商务私聊的输出流请省略话题ID和话题类型".into());
                }
                send_json["params"]["business_connection_id"] = id.into();
            }
        }
        if let Some((field, id)) = topic {
            send_json["params"][field] = id.into();
        } else {
            apply_telegram_topic(&event, &self_id, &target, &mut send_json["params"]);
        }
    }
    if send_options
        .as_object()
        .is_some_and(|fields| !fields.is_empty())
    {
        send_json["params"]["send_options"] = send_options;
    }
    let passive_id = rl.get_exmap("消息ID");
    let remote_id = rl.get_exmap("远程MQTT客户端ID");

    #[cfg(test)]
    if let Some(response) = TEST_PLATFORM_CALL.with_borrow(|handler| {
        handler.as_ref().map(|handler| {
            handler(
                &platform,
                send_json["action"].as_str().unwrap_or_default(),
                send_json["params"].clone(),
            )
        })
    }) {
        if response["retcode"] != 0 {
            return Err("模拟输出流调用失败".into());
        }
        return Ok(read_json_str(&response["data"], "message_id"));
    }

    // 数据统计
    {
        if msg_type == "group" {
            add_send_group_msg(&platform, &self_id)?;
        } else {
            add_send_private_msg(&platform, &self_id)?;
        }
    }

    let cq_ret = cq_call_api(
        &platform,
        &self_id,
        &passive_id,
        send_json.to_string().as_str(),
        &remote_id,
    );
    let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
    let err = "输出流调用失败,retcode 不为0";
    if ret_json.get("retcode").ok_or(err)?.as_i64().ok_or(err)? != 0 {
        return Err(RedLang::make_err(&format!("{}:{}", err, cq_ret)));
    }
    let err = "输出流调用失败，获取message_id失败";
    let msg_id = read_json_str(ret_json.get("data").ok_or(err)?, "message_id");
    let src_msg_id = rl.get_exmap("消息ID");
    let tm = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    runtime_context()
        .session
        .record_sent_message(&rl.pkg_name, &self_id, &src_msg_id, &msg_id, tm);
    if msg_type == "group" {
        let self_id = rl.get_exmap("机器人ID");
        crate::cqevent::do_group_msg::msg_id_map_insert(
            self_id.to_string(),
            self_id.to_string(),
            sub_id,
            msg_id.clone(),
        )?;
    }
    Ok(msg_id)
}

fn group_member_role(rl: &RedLang, user_id: &str) -> RedCommandResult {
    let platform = get_platform(rl);
    let result = call_platform_api(
        rl,
        &platform,
        "get_group_member_info",
        serde_json::json!({
            "group_id":rl.get_exmap("群ID").as_str(),"user_id":user_id,
        }),
    )?
    .ok_or("获取群成员权限未返回响应")?;
    let RedValue::Object(result) = result.as_ref() else {
        return Err(RedLang::make_err("获取群成员权限返回格式错误"));
    };
    let Some(RedValue::Object(data)) = result.get("data").map(Rc::as_ref) else {
        return Err(RedLang::make_err("获取群成员权限失败"));
    };
    let role = data
        .get("role")
        .ok_or("群成员权限响应缺少 role")?
        .expect_text_value()?;
    let role = match role.as_str() {
        "admin" => "管理",
        "owner" => "群主",
        "member" => "群员",
        _ => return Err(RedLang::make_err("群成员权限响应中的 role 无效")),
    };
    Ok(Some(rv_text(role.to_owned())))
}

fn management_platform(rl: &RedLang) -> Result<String, Box<dyn std::error::Error>> {
    let platform = get_platform(rl);
    if matches!(platform.as_str(), "kook" | "telegram") {
        Ok(platform)
    } else {
        Err(RedLang::make_err("此命令需要 KOOK 或 Telegram 机器人来源"))
    }
}

fn message_handle_list(value: &RedValue) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let ids = match value {
        RedValue::Array(items) => items
            .iter()
            .map(|item| item.expect_text_value())
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![value.expect_text_value()?],
    };
    if ids.iter().any(|id| id.is_empty()) {
        return Err(RedLang::make_err("消息 ID 需要非空文本"));
    }
    Ok(ids)
}

mod commands_cq;
mod commands_event;
mod commands_group;
mod commands_message;
mod commands_misc;

pub fn init_cq_ex_fun_map() {
    crate::bot_commands::register();
    commands_group::register();
    commands_message::register();
    commands_cq::register();
    commands_event::register();
    commands_misc::register();
}
