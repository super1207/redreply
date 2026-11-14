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

use crate::core::{
    add_fun, get_const_val, get_temp_const_val, rv_array, rv_bin, rv_empty, rv_object, rv_text,
    set_const_val, set_temp_const_val, RedCommandResult, RedLang, RedValue, RedValueData, CLEAR_UUID,
};
use crate::exfun::json_value_to_red;
use crate::native_args;
use redlang::cqapi::{cq_add_log_w, cq_call_api, cq_get_app_directory1, cq_get_app_directory2};
use crate::lifecycle::pkg_can_run;
use redlang::mytool::{cq_params_encode, cq_text_encode, json_to_cq_str, read_json_str};
use redlang::resource_manager::{add_file_lock, del_file_lock};
use redlang::runtime;
use redlang::runtime_context;
use crate::status::{add_send_group_msg, add_send_private_msg};
use redlang::PAGING_UUID;
use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
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
        return Err(RedLang::make_err(&format!("此命令需要 {platform} 机器人来源")));
    }
    #[cfg(test)]
    if let Some(response) = TEST_PLATFORM_CALL.with_borrow(|handler| {
        handler.as_ref().map(|handler| handler(platform, action, params.clone()))
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
        return Err(RedLang::make_err("原生 API 未收到响应，请检查机器人是否已连接"));
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
    Some((method, serde_json::json!({"chat_id": chat_id, "user_id": user_id})))
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
    let code = fields.get("retcode").ok_or("缺少 retcode")?.expect_text_value()?;
    if code != "0" {
        let message = fields.get("message").ok_or("缺少 message")?.expect_text_value()?;
        return Err(RedLang::make_err(&format!("{action}失败（{code}）{message}")));
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
    let original_chat = native["chat"]["id"].as_i64()
        .or_else(|| native["message"]["chat"]["id"].as_i64());
    if original_chat.is_none() || chat_id.parse::<i64>().ok() != original_chat
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
    let id = id.parse::<i64>().ok().filter(|id| *id > 0)
        .ok_or_else(|| RedLang::make_err("Telegram 话题 ID 需要正整数"))?;
    Ok(Some((field, id)))
}

pub fn send_one_msg(rl: &RedLang, msg: &str) -> Result<String, Box<dyn std::error::Error>> {
    send_one_msg_with_options(rl, msg, None, None)
}

fn send_one_msg_with_options(
    rl: &RedLang,
    msg: &str,
    reply_markup: Option<serde_json::Value>,
    topic: Option<(&'static str, i64)>,
) -> Result<String, Box<dyn std::error::Error>> {
    if msg.is_empty() {
        return Ok("".to_string());
    }

    let arr_msg = redlang::mytool::str_msg_to_arr(&serde_json::json!(msg))?;
    let msg_type: &'static str = crate::cqevent::get_msg_type(rl);
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
                "user_id":(*rl.get_exmap("发送者ID")),
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
    let platform = get_platform(rl);
    if platform == "telegram" {
        if let Some(reply_markup) = reply_markup {
            send_json["params"]["reply_markup"] = reply_markup;
        }
        let source = rl.get_exmap("原始事件");
        let target = if msg_type == "group" { sub_id.clone() } else { rl.get_exmap("发送者ID").to_string() };
        if let Some((field, id)) = topic {
            send_json["params"][field] = id.into();
        } else if let Ok(event) = serde_json::from_str(&source) {
            apply_telegram_topic(&event, &self_id, &target, &mut send_json["params"]);
        }
    }
    let passive_id = rl.get_exmap("消息ID");
    let remote_id = rl.get_exmap("远程MQTT客户端ID");

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
    let result = call_platform_api(rl, &platform, "get_group_member_info", serde_json::json!({
        "group_id":rl.get_exmap("群ID").as_str(),"user_id":user_id,
    }))?.ok_or("获取群成员权限未返回响应")?;
    let RedValue::Object(result) = result.as_ref() else {
        return Err(RedLang::make_err("获取群成员权限返回格式错误"));
    };
    let Some(RedValue::Object(data)) = result.get("data").map(Rc::as_ref) else {
        return Err(RedLang::make_err("获取群成员权限失败"));
    };
    let role = data.get("role").ok_or("群成员权限响应缺少 role")?.expect_text_value()?;
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
        RedValue::Array(items) => items.iter().map(|item| item.expect_text_value()).collect::<Result<Vec<_>, _>>()?,
        _ => vec![value.expect_text_value()?],
    };
    if ids.iter().any(|id| id.is_empty()) {
        return Err(RedLang::make_err("消息 ID 需要非空文本"));
    }
    Ok(ids)
}

pub fn init_cq_ex_fun_map() {
    crate::bot_commands::register();
    add_fun(vec!["取群列表", "群列表"], |self_t, params| {
        let groups_id = self_t.get_exmap("群组ID");
        let send_json = if get_platform(self_t) == "kook" {
            let explicit = self_t.get_param_text(params, 0)?;
            let guild_id = if explicit.is_empty() { groups_id.as_str() } else { &explicit };
            serde_json::json!({
                "action":"get_group_list",
                "params":{"groups_id":guild_id,"group_id":self_t.get_exmap("群ID").as_str()},
            })
        } else if groups_id.is_empty() {
            serde_json::json!({
                "action":"get_group_list",
                "params":{}
            })
        } else {
            serde_json::json!({
                "action":"get_group_list",
                "params":{
                    "groups_id":groups_id.as_str()
                }
            })
        };
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取群列表失败:{ret_json}");

        let group_list = ret_json.get("data").ok_or(err)?;

        let mut to_ret: Vec<serde_json::Value> = vec![];
        for group in group_list.as_array().ok_or("获取群列表失败")? {
            to_ret.push(serde_json::json!({
                "群ID":read_json_str(group, "group_id"),
                "群名":read_json_str(group, "group_name"),
            }));
        }
        return Ok(Some(json_value_to_red(&serde_json::json!(to_ret))?));
    });

    fn get_stranger_info(
        self_t: &mut RedLang,
        user_id: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let user_id_t = if user_id.is_empty() {
            (*self_t.get_exmap("发送者ID")).to_owned()
        } else {
            user_id.to_owned()
        };
        let send_json = serde_json::json!({
            "action":"get_stranger_info",
            "params":{
                "user_id":user_id_t
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
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let err = format!("获取发送者信息失败:{ret_json}");
        let data = ret_json.get("data").ok_or(err)?;
        Ok(data.to_owned())
    }
    add_fun(vec!["取发送者信息"], |self_t, _params| {
        let data = get_stranger_info(self_t, "")?;
        let nickname = read_json_str(&data, "nickname");
        self_t.set_exmap("发送者昵称", &nickname);
        let to_ret = serde_json::json!({
            "用户ID":read_json_str(&data, "user_id"),
            "用户名":nickname
        });
        return Ok(Some(json_value_to_red(&to_ret)?));
    });
    add_fun(vec!["取头像"], |self_t, params| {
        let user_id = self_t.get_param_text(params, 0).unwrap_or_default();
        let platform = get_platform(self_t);
        if platform == "telegram" && self_t.get_param_text(params, 1)? == "文件ID列表" {
            let offset = self_t.get_param_text(params, 2)?;
            let limit = self_t.get_param_text(params, 3)?;
            return crate::bot_commands::management::telegram_avatars(self_t, &user_id, &offset, &limit);
        }
        // 官方QQ平台：头像 CDN 支持用 AppID + OpenID 直接拼 URL
        if platform == "qqgroup_public"
            || platform == "qqguild_public"
            || platform == "qqguild_private"
        {
            // 官方QQ头像 CDN：https://q.qlogo.cn/qqapp/{AppID}/{OpenID}/{尺寸}
            // （honoka-bot 实测：返回 image/jpeg，无需调用任何 API）
            let appid = self_t.get_exmap("机器人ID").to_string();
            let openid = if user_id.is_empty() {
                self_t.get_exmap("发送者ID").to_string()
            } else {
                user_id.clone()
            };
            if !appid.is_empty() && !openid.is_empty() {
                return Ok(Some(rv_text(format!(
                    "https://q.qlogo.cn/qqapp/{}/{}/640",
                    appid, openid
                ))));
            }
            return Ok(Some(rv_text(
                "获取头像失败：缺少AppID或用户OpenID".to_string(),
            )));
        }
        let data = get_stranger_info(self_t, &user_id).unwrap_or_default();
        let avatar = read_json_str(&data, "avatar");
        Ok(Some(rv_text(avatar)))
    });
    add_fun(vec!["发送者ID", "发送者QQ"], |self_t, _params| {
        let qq = self_t.get_exmap("发送者ID");
        Ok(Some(rv_text(qq.to_string())))
    });
    add_fun(
        vec!["当前群号", "群号", "群ID"],
        |self_t, _params| {
            let group = self_t.get_exmap("群ID");
            Ok(Some(rv_text(group.to_string())))
        },
    );
    add_fun(
        vec!["当前群组", "群组号", "群组ID"],
        |self_t, _params| {
            let groups = self_t.get_exmap("群组ID");
            Ok(Some(rv_text(groups.to_string())))
        },
    );
    add_fun(vec!["发送者昵称"], |self_t, _params| {
        let mut nickname = &*self_t.get_exmap("发送者昵称");
        let card = &*self_t.get_exmap("发送者名片");
        if !card.is_empty() {
            nickname = card;
        }
        if nickname.is_empty() {
            if let Ok(data) = get_stranger_info(self_t, "") {
                let name = read_json_str(&data, "nickname");
                if !name.is_empty() {
                    self_t.set_exmap("发送者昵称", &name);
                    return Ok(Some(rv_text(name)));
                }
            }
        }
        Ok(Some(rv_text(nickname.to_string())))
    });
    add_fun(vec!["机器人QQ"], |self_t, _params| {
        let qq = self_t.get_exmap("机器人ID");
        Ok(Some(rv_text(qq.to_string())))
    });
    add_fun(vec!["机器人ID"], |self_t, _params| {
        let qq: String = self_t.get_exmap("机器人ID").to_string();
        Ok(Some(rv_text(qq)))
    });
    add_fun(vec!["机器人名字"], |self_t, _params| {
        let send_json = serde_json::json!({
            "action":"get_login_info",
            "params":{}
        });
        let self_id = self_t.get_exmap("机器人ID");
        let platform = get_platform(self_t);
        let passive_id = self_t.get_exmap("消息ID");
        let remote_id = self_t.get_exmap("远程MQTT客户端ID");
        let cq_ret = cq_call_api(
            &platform,
            &self_id,
            &passive_id,
            send_json.to_string().as_str(),
            &remote_id,
        );
        let ret_json: serde_json::Value = serde_json::from_str(&cq_ret)?;
        let bot_name = &ret_json["data"]["nickname"];
        if let Some(name) = bot_name.as_str() {
            if name.is_empty() {
                return Ok(Some(rv_text("露娜sama".to_owned())));
            }
            Ok(Some(rv_text(name.to_owned())))
        } else {
            Ok(Some(rv_text("露娜sama".to_owned())))
        }
    });
    add_fun(vec!["权限", "发送者权限"], |self_t, _params| {
        let user = self_t.get_exmap("发送者ID");
        if get_platform(self_t) == "telegram"
            && !self_t.get_exmap("群ID").is_empty()
            && user.parse::<i64>().is_ok_and(|id| id > 0)
        {
            return group_member_role(self_t, &user);
        }
        Ok(Some(rv_text(self_t.get_exmap("发送者权限").to_string())))
    });
    // add_fun(vec!["发送者名片"],|self_t,_params|{
    //     let card = self_t.get_exmap("发送者名片");
    //     return Ok(Some(card.to_string()));
    // });
    // add_fun(vec!["发送者专属头衔"],|self_t,_params|{
    //     let title = self_t.get_exmap("发送者专属头衔");
    //     return Ok(Some(title.to_string()));
    // });
    add_fun(vec!["消息ID"], |self_t, params| {
        let qq = self_t.get_param_text(params, 0)?;
        if qq.is_empty() {
            let msg_id = self_t.get_exmap("消息ID");
            Ok(Some(rv_text(msg_id.to_string())))
        } else {
            let group_id = self_t.get_exmap("群ID");
            let self_id = self_t.get_exmap("机器人ID");
            let message_ids = runtime_context().session.message_ids(&self_id, &group_id)?;
            let ret_arr = message_ids
                .into_iter()
                .filter(|(user_id, _)| user_id == &qq)
                .map(|(_, message_id)| rv_text(message_id))
                .collect::<Vec<Rc<RedValue>>>();
            Ok(Some(rv_array(ret_arr)))
        }
    });
    add_fun(vec!["群历史消息"], |self_t, params| {
        let num_str = self_t.get_param_text(params, 0)?;
        let num = num_str.parse::<usize>()?;
        let mut to_ret: Vec<String> = vec![];
        let group_id = self_t.get_exmap("群ID");
        let self_id = self_t.get_exmap("机器人ID");
        let message_ids = runtime_context().session.message_ids(&self_id, &group_id)?;
        for (_, message_id) in message_ids {
            to_ret.push(message_id);
            if to_ret.len() >= num {
                break;
            }
        }
        let ret = to_ret
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["设置图片参数"], |self_t, params| {
        let key = self_t.get_param_text(params, 0)?;
        let val = self_t.get_param_text(params, 1)?;
        if key.to_uppercase() == "SUMMARY" {
            self_t.set_exmap("图片SUMMARY", val.as_str());
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["图片"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        let summary = self_t.get_exmap("图片SUMMARY");
        if let Ok(bin) = pic_rv.expect_bin_value() {
            if bin.is_empty() {
                let _ = cq_add_log_w("图片字节集长度为0");
                return Ok(Some(rv_empty()));
            }
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            if !(*summary).is_empty() {
                ret = format!(
                    "[CQ:image,file=base64://{},summary={}]",
                    b64_str,
                    cq_params_encode(&summary)
                );
            } else {
                ret = format!("[CQ:image,file=base64://{}]", b64_str);
            }
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    if !(*summary).is_empty() {
                        ret = format!(
                            "[CQ:image,file={},cache=0,summary={}]",
                            cq_params_encode(&pic),
                            cq_params_encode(&summary)
                        );
                    } else {
                        ret = format!("[CQ:image,file={},cache=0]", cq_params_encode(&pic));
                    }
                } else {
                    if !(*summary).is_empty() {
                        ret = format!(
                            "[CQ:image,file={},summary={}]",
                            cq_params_encode(&pic),
                            cq_params_encode(&summary)
                        );
                    } else {
                        ret = format!("[CQ:image,file={}]", cq_params_encode(&pic));
                    }
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                if !(*summary).is_empty() {
                    ret = format!(
                        "[CQ:image,file=base64://{},summary={}]",
                        b64_str,
                        cq_params_encode(&summary)
                    );
                } else {
                    ret = format!("[CQ:image,file=base64://{}]", b64_str);
                }
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["语音"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:record,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    ret = format!("[CQ:record,file={},cache=0]", cq_params_encode(&pic));
                } else {
                    ret = format!("[CQ:record,file={}]", cq_params_encode(&pic));
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:record,file=base64://{}]", b64_str);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["媒体组"], |rl, params| {
        management_platform(rl)?;
        let items = rl.get_param(params, 0)?;
        let group = native_args::media_group(&items)?;
        let content = cq_params_encode(&group.to_string());
        Ok(Some(rv_text(format!("[CQ:media_group,content={content}]"))))
    });
    add_fun(vec!["贴纸"], |rl, params| {
        if get_platform(rl) != "telegram" {
            return Err(RedLang::make_err("【贴纸】需要 Telegram 机器人来源"));
        }
        let source = rl.get_param(params, 0)?;
        let filename = rl.get_param_text(params, 1)?;
        let emoji = rl.get_param_text(params, 2)?;
        let data = if let RedValue::Bin(bytes) = source.as_ref() {
            let lower = filename.to_ascii_lowercase();
            let mime = if lower.ends_with(".webp") {
                "image/webp"
            } else if lower.ends_with(".tgs") {
                "application/x-tgsticker"
            } else if lower.ends_with(".webm") {
                "video/webm"
            } else {
                return Err(RedLang::make_err("Telegram 贴纸上传文件名需以 .webp、.tgs 或 .webm 结尾"));
            };
            let mut data = serde_json::json!({"upload":native_args::file_part(&filename, bytes, Some(mime))});
            if !emoji.is_empty() {
                data["emoji"] = emoji.into();
            }
            data
        } else {
            let file = source.expect_text_value()?;
            if file.is_empty() || !filename.is_empty() || !emoji.is_empty() {
                return Err(RedLang::make_err("文件 ID 或链接只填第一项，文件名和表情用于上传字节集"));
            }
            serde_json::json!({"file_id":file})
        };
        Ok(Some(rv_text(format!("[CQ:sticker,content={}]", cq_params_encode(&data.to_string())))))
    });
    add_fun(vec!["文件"], |self_t, params| {
        let file_rv = self_t.get_param(params, 0)?;
        let name = self_t.get_param_text(params, 1)?;
        let mut ret: String = String::new();
        if let Ok(bin) = file_rv.expect_bin_value() {
            let name = cq_params_encode(&resolve_file_name(&name, None));
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:file,file=base64://{},name={}]", b64_str, name);
        } else if let Ok(file) = file_rv.expect_text_value() {
            let name = cq_params_encode(&resolve_file_name(&name, Some(&file)));
            if file.starts_with("http://") || file.starts_with("https://") {
                ret = format!("[CQ:file,file={},name={}]", cq_params_encode(&file), name);
            } else {
                let path = Path::new(&*file);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:file,file=base64://{},name={}]", b64_str, name);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["视频"], |self_t, params| {
        let pic_rv = self_t.get_param(params, 0)?;
        let mut ret: String = String::new();
        if let Ok(bin) = pic_rv.expect_bin_value() {
            let b64_str = BASE64_CUSTOM_ENGINE.encode(&bin);
            ret = format!("[CQ:video,file=base64://{}]", b64_str);
        } else if let Ok(pic) = pic_rv.expect_text_value() {
            if pic.starts_with("http://") || pic.starts_with("https://") {
                let not_use_cache = self_t.get_param_text(params, 1)?;
                if not_use_cache.as_str() == "假" {
                    ret = format!("[CQ:video,file={},cache=0]", cq_params_encode(&pic));
                } else {
                    ret = format!("[CQ:video,file={}]", cq_params_encode(&pic));
                }
            } else {
                let path = Path::new(&*pic);
                let bin = std::fs::read(path)?;
                let b64_str = BASE64_CUSTOM_ENGINE.encode(bin);
                ret = format!("[CQ:video,file=base64://{}]", b64_str);
            }
        }
        Ok(Some(rv_text(ret)))
    });
    add_fun(vec!["撤回"], |self_t, params| {
        let msg_id_rv = self_t.get_param(params, 0)?;
        let platform = get_platform(self_t);
        if matches!(platform.as_str(), "kook" | "telegram") && matches!(msg_id_rv.as_ref(), RedValue::Array(_)) {
            let ids = message_handle_list(&msg_id_rv)?;
            return call_platform_api(
                self_t, &platform, "delete_msgs", serde_json::json!({"message_ids":ids}),
            );
        }
        let current_msg_id = self_t.get_exmap("消息ID");
        let msg_id_vec: Vec<String> = match &*msg_id_rv {
            RedValue::Text(s) if s.is_empty() => vec![current_msg_id.to_string()],
            RedValue::Text(s) => vec![s.to_string()],
            RedValue::Array(arr) => arr
                .iter()
                .filter_map(|item| item.expect_text_value().ok())
                .collect(),
            _ => vec![],
        };
        for it in msg_id_vec {
            let send_json = serde_json::json!({
                "action":"delete_msg",
                "params":{
                    "message_id":it
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
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["编辑消息"], |rl, params| {
        let platform = management_platform(rl)?;
        let mut id = rl.get_param_text(params, 0)?;
        let explicit_id = !id.is_empty();
        if id.is_empty() {
            id = rl.get_exmap("消息ID").to_string();
        }
        let content = rl.get_param(params, 1)?;
        let kind = rl.get_param_text(params, 2)?;
        let mut body = if params.len() > 3 {
            let options = rl.get_param(params, 3)?;
            match options.as_ref() {
                RedValue::Text(text) if text.is_empty() => serde_json::json!({}),
                RedValue::Text(text) if platform == "telegram" => {
                    let mode = match text.as_str() {
                        "Markdown" => "MarkdownV2",
                        "HTML" => "HTML",
                        _ => return Err(RedLang::make_err("格式请填 Markdown、HTML 或留空")),
                    };
                    serde_json::json!({"parse_mode":mode})
                }
                _ => native_args::object(&options)?,
            }
        } else {
            serde_json::json!({})
        };
        let mut upload = None;
        let (kind, field, value) = match (platform.as_str(), kind.as_str()) {
            ("telegram", "" | "文本") => ("text", "text", content.expect_text_value()?.into()),
            ("telegram", "说明") => ("caption", "caption", content.expect_text_value()?.into()),
            ("telegram", "键盘") => {
                ("keyboard", "reply_markup", crate::bot_commands::edit_keyboard(&content)?)
            }
            ("telegram", "媒体") if content.as_array().is_some() => {
                let (mut media, files) = native_args::media_item(&content, 0)?;
                if let Some(mode) = body.as_object_mut().and_then(|obj| obj.remove("parse_mode")) {
                    media["parse_mode"] = mode;
                }
                if !files.is_empty() {
                    upload = Some(serde_json::Value::Object(files));
                }
                ("media", "media", media)
            }
            ("telegram", "媒体") => ("media", "media", native_args::object(&content)?),
            ("kook", "" | "文本") => ("text", "content", content.expect_text_value()?.into()),
            ("kook", "卡片") => {
                ("card", "content", crate::bot_commands::card_for_edit(&content)?)
            }
            _ => return Err(RedLang::make_err("当前平台不支持这种编辑类型")),
        };
        body[field] = value;
        let mut request = serde_json::json!({"message_id":id,"kind":kind,"body":body});
        if platform == "telegram" {
            let attachments = rl.get_param(params, 4)?;
            match attachments.as_ref() {
                RedValue::Text(text) if text.is_empty() => (),
                RedValue::Object(fields) if fields.contains_key("inline_keyboard") => {
                    request["body"]["reply_markup"] = crate::bot_commands::edit_keyboard(&attachments)?;
                }
                _ if upload.is_some() => return Err(RedLang::make_err("媒体数组已经包含上传内容")),
                _ => upload = native_args::files_value(&attachments)?,
            }
            if let Some(files) = upload {
                request["files"] = files;
            }
        }
        match rl.get_param_text(params, 5)?.as_str() {
            "" => (),
            "行内" if platform == "telegram" => {
                if !explicit_id {
                    return Err(RedLang::make_err("编辑行内消息需填写平台行内消息ID"));
                }
                if request.get("files").is_some() {
                    return Err(RedLang::make_err("Telegram 行内媒体编辑请使用已有文件ID或直链"));
                }
                let method = match kind {
                    "text" => "editMessageText",
                    "caption" => "editMessageCaption",
                    "keyboard" => "editMessageReplyMarkup",
                    _ => "editMessageMedia",
                };
                let mut body = request["body"].take();
                body["inline_message_id"] = id.into();
                return call_platform_api(rl,&platform,"telegram_api",serde_json::json!({"method":method,"body":body}));
            }
            _ => return Err(RedLang::make_err("编辑目标类型请留空或在 Telegram 中填行内")),
        }
        call_platform_api(rl, &platform, "edit_msg", request)
    });
    add_fun(vec!["转发消息"], |rl, params| {
        let platform = management_platform(rl)?;
        let source = rl.get_param(params, 0)?;
        let ids = match source.as_ref() {
            RedValue::Text(text) if text.is_empty() => vec![rl.get_exmap("消息ID").to_string()],
            _ => message_handle_list(&source)?,
        };
        let mut target = rl.get_param_text(params, 1)?;
        let kind = rl.get_param_text(params, 2)?;
        let is_private = match kind.as_str() {
            "" => crate::cqevent::get_msg_type(rl) == "private",
            "群聊" | "频道" => false,
            "私聊" => true,
            _ => return Err(RedLang::make_err("转发目标类型需为群聊、频道或私聊")),
        };
        if target.is_empty() {
            target = rl.get_exmap(if is_private { "发送者ID" } else { "群ID" }).to_string();
        }
        let options = rl.get_param(params, 3)?;
        let options = match options.as_ref() {
            RedValue::Text(copy) => {
                let silent = rl.get_param_text(params, 4)?;
                for value in [copy.as_str(), silent.as_str()] {
                    if !matches!(value, "" | "真" | "假") {
                        return Err(RedLang::make_err("复制和静默请填真或假"));
                    }
                }
                serde_json::json!({"复制":copy.as_str(),"静默":silent,
                    "话题ID":rl.get_param_text(params, 5)?,"话题类型":rl.get_param_text(params, 6)?})
            }
            _ => native_args::object(&options)?,
        };
        if ids.is_empty() || ids.iter().any(|id| id.is_empty()) || target.is_empty() {
            return Err(RedLang::make_err("转发需要消息 ID 和目标聊天"));
        }
        let mut request = serde_json::json!({
            "message_ids":ids,"target_id":target,"private":is_private,
            "copy":options["复制"] == "真","silent":options["静默"] == "真",
        });
        if platform == "telegram" {
            let read_option = |key| options.get(key).and_then(serde_json::Value::as_str).unwrap_or_default();
            if let Some((field, id)) = telegram_output_topic(read_option("话题ID"), read_option("话题类型"))? {
                request[field] = id.into();
            } else if let Ok(event) = serde_json::from_str(&rl.get_exmap("原始事件")) {
                apply_telegram_topic(&event, &rl.get_exmap("机器人ID"), &target, &mut request);
            }
        }
        call_platform_api(rl, &platform, "forward_msgs", request)
    });
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
            return Err(RedLang::make_err("【置顶消息】需要 KOOK 或 Telegram 机器人来源"));
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
        let result = call_platform_api(rl, &platform, "set_msg_pin", serde_json::json!({
            "message_id":id,"remove":remove,"silent":silent,
        }))?;
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
                    if fields.get("retcode").and_then(|value| value.expect_text_value().ok()).as_deref() != Some("0") {
                        let message = fields.get("message").and_then(|value| value.expect_text_value().ok()).unwrap_or_default();
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
        let markup = if get_platform(self_t) == "telegram" && params.len() > 1 {
            let value = self_t.get_param(params, 1)?;
            match value.as_ref() {
                RedValue::Text(text) if text.is_empty() => None,
                _ => Some(native_args::object(&value)?),
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
        match send_one_msg_with_options(self_t, &msg, markup, topic) {
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
        let response = call_platform_api(rl, "telegram", "telegram_download", serde_json::json!({"file_id": file_id}))?
            .ok_or("Telegram 下载未返回响应")?;
        let RedValue::Object(fields) = response.as_ref() else {
            return Err(RedLang::make_err("Telegram 下载返回格式错误"));
        };
        if fields.get("retcode").and_then(|value| value.expect_text_value().ok()).as_deref() != Some("0") {
            let message = fields.get("message").and_then(|value| value.expect_text_value().ok())
                .filter(|message| !message.is_empty()).unwrap_or_else(|| "Telegram 下载失败".to_owned());
            return Err(RedLang::make_err(&message));
        }
        let Some(RedValue::Object(data)) = fields.get("data").map(Rc::as_ref) else {
            return Err(RedLang::make_err("Telegram 下载未返回文件"));
        };
        let base64 = data.get("base64").ok_or("Telegram 下载未返回文件")?.expect_text_value()?;
        Ok(Some(rv_bin(BASE64_CUSTOM_ENGINE.decode(base64)?)))
    });
    add_fun(vec!["KOOK调用"], |rl, params| {
        let method = rl.get_param_text(params, 0)?.to_ascii_uppercase();
        let endpoint = rl.get_param_text(params, 1)?;
        let body = native_args::body(rl, params, 2)?;
        let mut request = serde_json::json!({"http_method": method, "endpoint": endpoint, "body": body});
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
        let volume = if volume.is_empty() { 100 } else { volume.parse::<u64>()? };
        let mut request = serde_json::json!({"channel_id":channel,"volume":volume});
        match source.as_ref() {
            RedValue::Array(items) => {
                let sources = items.iter().map(|item| item.expect_text_value()).collect::<Result<Vec<_>, _>>()?;
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
        let response = call_platform_api_value(rl, "kook", "kook_voice_status", serde_json::json!({}))?;
        if response["retcode"] != 0 {
            return Err(RedLang::make_err(response["message"].as_str().unwrap_or("无法查询播放状态")));
        }
        let data = &response["data"];
        let value = match field.as_str() {
            "状态" => crate::bot_commands::playback_state(&data["state"]),
            "当前曲目" => data["track"].clone(),
            "曲目总数" => data["total"].clone(),
            "频道ID" => data["channel_id"].clone(),
            "错误" => data["error"].clone(),
            _ => return Err(RedLang::make_err("播放状态可查询状态、当前曲目、曲目总数、频道ID或错误")),
        };
        Ok(Some(json_value_to_red(&value)?))
    });
    add_fun(vec!["CQ码解析"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let pos1 = data_str.find(",").ok_or("CQ码解析失败")?;
        let tp = data_str.get(4..pos1).ok_or("CQ码解析失败")?;
        let mut sub_key_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        sub_key_obj.insert("type".to_string(), rv_text(tp.to_string()));
        let re = fancy_regex::Regex::new("[:,]([^\\[\\],]+?)=([^\\[\\],]*?)(?=[\\],])")?;

        for cap_iter in re.captures_iter(&data_str) {
            let cap = cap_iter?;
            let len = cap.len();
            if len == 3 {
                let key = &cap[1];
                let val = &cap[2];
                let key = key.replace("&#91;", "[");
                let key = key.replace("&#93;", "]");
                let key = key.replace("&#44;", ",");
                let key = key.replace("&amp;", "&");
                let val = val.replace("&#91;", "[");
                let val = val.replace("&#93;", "]");
                let val = val.replace("&#44;", ",");
                let val = val.replace("&amp;", "&");
                sub_key_obj.insert(key, rv_text(val));
            }
        }
        Ok(Some(rv_object(sub_key_obj)))
    });
    add_fun(vec!["CQ解析"], |self_t, params| {
        let data_str = self_t.get_param_text(params, 0)?;
        let json_arr = redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?;
        return Ok(Some(json_value_to_red(&json_arr)?));
    });
    add_fun(vec!["CQ反转义"], |self_t, params| {
        let content = self_t.get_param_text(params, 0)?;
        let content = content.replace("&#91;", "[");
        let content = content.replace("&#93;", "]");
        let content = content.replace("&amp;", "&");
        Ok(Some(rv_text(content)))
    });
    add_fun(vec!["定义常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        set_const_val(&self_t.pkg_name, &k, RedValueData::from_red_value(&v)?)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["常量"], |self_t, params| {
        let params_len = params.len();
        if params_len == 1 {
            // 取当前包的常量
            let k = self_t.get_param_text(params, 0)?;
            if let Some(v) = get_const_val(&self_t.pkg_name, &k)? {
                return Ok(Some(v.into_rc_value()?));
            }
            Ok(Some(rv_empty()))
        } else {
            // 取其它包的常量
            let pkg_name = self_t.get_param_text(params, 0)?;
            let k = self_t.get_param_text(params, 1)?;
            if let Some(v) = get_const_val(&pkg_name, &k)? {
                return Ok(Some(v.into_rc_value()?));
            }
            Ok(Some(rv_empty()))
        }
    });
    add_fun(vec!["定义临时常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        let v = self_t.get_param(params, 1)?;
        let mut tm = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        tm += self_t.get_param_text(params, 2)?.parse::<u128>()?;
        set_temp_const_val(&self_t.pkg_name, &k, RedValueData::from_red_value(&v)?, tm)?;
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["临时常量"], |self_t, params| {
        let k = self_t.get_param_text(params, 0)?;
        if let Some(v) = get_temp_const_val(&self_t.pkg_name, &k)? {
            return Ok(Some(v.into_rc_value()?));
        }
        Ok(Some(rv_empty()))
    });
    add_fun(vec!["进程ID"], |_self_t, _params| {
        Ok(Some(rv_text(std::process::id().to_string())))
    });
    add_fun(vec!["读词库文件"], |self_t, params| {
        let path = self_t.get_param_text(params, 0)?;
        let path_t = path.clone();
        let file_dat = fs::read_to_string(path)?;
        let file_dat_without_r = file_dat.replace('\r', "");
        let words_list = file_dat_without_r.split("\n\n");
        let mut dict_obj: BTreeMap<String, Rc<RedValue>> = BTreeMap::new();
        let err = format!("词库文件格式错误:`{}`", &path_t);
        for words in words_list {
            if words.trim().is_empty() {
                continue;
            }
            let word_list = words.split('\n').collect::<Vec<&str>>();
            let key: &str = word_list.first().ok_or(err.clone())?;
            let word_list_t = word_list.get(1..).ok_or(err.clone())?;
            let mut arr_val: Vec<Rc<RedValue>> = vec![];
            for word in word_list_t {
                arr_val.push(rv_text((*word).to_string()));
            }
            dict_obj.insert(key.to_owned(), rv_array(arr_val));
        }
        Ok(Some(rv_object(dict_obj)))
    });
    add_fun(vec!["应用目录"], |self_t, _params| {
        let app_dir = get_app_dir(&self_t.pkg_name)?;
        Ok(Some(rv_text(app_dir)))
    });
    add_fun(vec!["取艾特"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        let mut ret_vec: Vec<&str> = vec![];
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "at" {
                let qq = it
                    .get("data")
                    .ok_or(err)?
                    .get("qq")
                    .ok_or(err)?
                    .as_str()
                    .ok_or(err)?;
                ret_vec.push(qq);
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(|x| rv_text(x.to_string()))
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["取回复ID"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "reply" {
                let id = it
                    .get("data")
                    .ok_or(err)?
                    .get("id")
                    .ok_or(err)?
                    .as_str()
                    .ok_or(err)?;
                return Ok(Some(rv_text(id.to_owned())));
            }
        }
        Ok(Some(rv_empty()))
    });

    add_fun(vec!["取图片"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };

        let mut ret_vec: Vec<String> = vec![];
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "image" || tp == "mface" {
                let data = it.get("data").ok_or("data not found in image cq code")?;
                let mut url = read_json_str(data, "url");
                if url.is_empty() {
                    url = read_json_str(data, "file");
                }
                if url.starts_with("http://") || url.starts_with("https://") {
                    ret_vec.push(url);
                }
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
    add_fun(vec!["取文本"], |self_t, params| {
        let err = "获取message失败";
        let message = if !params.is_empty() {
            let data_str = self_t.get_param_text(params, 0)?;
            redlang::mytool::str_msg_to_arr(&serde_json::json!(data_str))?
                .as_array()
                .ok_or(err)?
                .clone()
        } else {
            let raw_data = self_t.get_exmap("原始事件");
            let raw_json: serde_json::Value = serde_json::from_str(&raw_data)?;
            raw_json
                .get("message")
                .ok_or(err)?
                .as_array()
                .ok_or(err)?
                .clone()
        };
        let mut ret_vec: Vec<String> = vec![];
        let mut should_new = 1;
        for it in &message {
            let tp = it.get("type").ok_or(err)?.as_str().ok_or(err)?;
            if tp == "text" {
                let data = it.get("data").ok_or("data not found in text cq code")?;
                let text = read_json_str(data, "text");
                if should_new == 1 {
                    ret_vec.push(text);
                } else {
                    let len = ret_vec.len();
                    ret_vec[len - 1].push_str(&text);
                }
                should_new = 0;
            } else {
                should_new = 1;
            }
        }
        let ret = ret_vec
            .into_iter()
            .map(rv_text)
            .collect::<Vec<Rc<RedValue>>>();
        Ok(Some(rv_array(ret)))
    });
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
        let old_group_id = self_t.get_exmap("群ID"); // 备份群ID
        let old_user_id = self_t.get_exmap("发送者ID"); // 备份用户ID
        self_t.set_exmap("群ID", "");
        if !user_id.is_empty() {
            self_t.set_exmap("发送者ID", &user_id);
        }
        match send_one_msg(self_t, &msg) {
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
