use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, RwLock},
    time::SystemTime,
};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use redreply_adapter_core::{
    cqapi::cq_add_log_w,
    mytool::{
        cq_fix_image_file_in_arr, read_json_obj_or_null, read_json_or_default, read_json_str,
    },
};

use crate::qq_guild_all::{get_gateway, get_json_dat, token_refresh, SelfData};

use super::qq_guild_all::{
    cq_msg_to_qq, deal_attachments, deal_message_reference, deal_qqgroup_audit_event, delete_msg,
    do_qq_json_post, get_group_list, get_group_member_info, get_login_info, get_msg_type,
    get_reply_id, get_stranger_info, qq_content_to_cqstr, send_private_msg, send_qqguild_msg,
    set_event_id, set_group_ban, str_msg_to_arr_safe, wait_qqgroup_audit_result, MsgSrcType,
    MsgTargetType, QQMsgNode,
};
use redreply_adapter_core::BotConnectTrait;

#[derive(Debug)]
pub struct QQGuildPublicConnect {
    pub url: String,
    pub appid: Arc<std::sync::RwLock<String>>,
    pub appsecret: String,
    pub token: String,
    pub access_token: Arc<std::sync::RwLock<String>>,
    pub tx: Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop: Arc<AtomicBool>,
    pub stop_tx: Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn: Arc<std::sync::RwLock<Option<u64>>>,
    pub id_event_map:
        Arc<std::sync::RwLock<std::collections::HashMap<String, (u64, serde_json::Value)>>>,
    pub bot_id: Arc<std::sync::RwLock<String>>,
    // 审核结果通道必须是连接级共享：call_api 注册等待者、WS 事件路径投递结果，
    // 两边要看到同一张 map，各自 new 会导致等待者永远收不到通知
    pub qqgroup_audit_map: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<String, tokio::sync::mpsc::Sender<serde_json::Value>>,
        >,
    >,
    // 审核事件先于 waiter 注册到达时的待领缓存
    pub qqgroup_audit_pending_cache:
        Arc<tokio::sync::RwLock<HashMap<String, (u64, serde_json::Value)>>>,
}

impl QQGuildPublicConnect {
    pub fn build(url: &str) -> Self {
        QQGuildPublicConnect {
            url: url.to_owned(),
            token: "".to_owned(),
            tx: None,
            is_stop: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            appid: Arc::new(std::sync::RwLock::new("".to_owned())),
            appsecret: "".to_owned(),
            access_token: Arc::new(RwLock::new("".to_owned())),
            sn: Arc::new(RwLock::new(None)),
            id_event_map: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bot_id: Arc::new(RwLock::new("".to_owned())),
            qqgroup_audit_map: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            qqgroup_audit_pending_cache: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

pub async fn conv_group_c2c_event(
    self_t: &SelfData,
    root: &serde_json::Value,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(root, "t");
    let self_id = (*self_t
        .appid
        .upgrade()
        .ok_or("No appid")?
        .read()
        .unwrap_or_else(|e| e.into_inner()))
    .to_owned();
    if tp == "MESSAGE_AUDIT_PASS" || tp == "MESSAGE_AUDIT_REJECT" {
        deal_qqgroup_audit_event(self_t, root.clone()).await?;
        return Ok(true);
    } else if tp == "GROUP_AT_MESSAGE_CREATE" {
        let event_id = set_event_id(self_t, root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(d, "content");
        let user = read_json_obj_or_null(d, "author");
        let user_id = read_json_str(&user, "id");
        let role = read_json_str(&user, "member_role");
        let role = if role.is_empty() {
            "member".to_owned()
        } else {
            role
        };
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id, &self_id, &content)?;
        let cq_msg = cq_msg_t + &deal_attachments(d)?;
        let mut cq_msg = deal_message_reference(d, &self_t.id_event_map)? + &cq_msg;
        // 去除开头的空格和/
        if cq_msg.starts_with(" /") {
            cq_msg = cq_msg[2..].to_owned();
        } else if cq_msg.starts_with("  ") {
            // 回复是两个空格
            cq_msg = cq_msg[2..].to_owned();
        } else if cq_msg.starts_with(" ") {
            cq_msg = cq_msg[1..].to_owned();
        }
        let group_id = read_json_str(d, "group_openid");
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqgroup_public",
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":event_id,
            "group_id":group_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":"",
                "sex":"unknown",
                "age":0,
                "area":"",
                "level":"0",
                "role":role,
                "title":"",
            }
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        return Ok(true);
    } else if tp == "GROUP_MESSAGE_CREATE" {
        let event_id = set_event_id(self_t, root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(d, "content");
        let user = read_json_obj_or_null(d, "author");
        let user_id = read_json_str(&user, "id");
        let nickname = read_json_str(&user, "username");
        let role = read_json_str(&user, "member_role");
        let role = if role.is_empty() {
            "member".to_owned()
        } else {
            role
        };
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id, &self_id, &content)?;
        let cq_msg = cq_msg_t + &deal_attachments(d)?;
        let cq_msg = deal_message_reference(d, &self_t.id_event_map)? + &cq_msg;
        let group_id = read_json_str(d, "group_openid");
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqgroup_public",
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":event_id,
            "group_id":group_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
                "sex":"unknown",
                "age":0,
                "area":"",
                "level":"0",
                "role":role,
                "title":"",
            }
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        return Ok(true);
    } else if tp == "GUILD_MEMBER_ADD" || tp == "GUILD_MEMBER_REMOVE" {
        return Ok(false);
    } else if tp == "C2C_MESSAGE_CREATE" {
        let event_id = set_event_id(self_t, root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(d, "content");
        let user = read_json_obj_or_null(d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname = read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id, &self_id, &content)?;
        let cq_msg = cq_msg_t + &deal_attachments(d)?;
        let cq_msg = deal_message_reference(d, &self_t.id_event_map)? + &cq_msg;
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqgroup_public",
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":event_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
                "remark":nickname,
                "avatar":avatar
            }
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        return Ok(true);
    }
    Ok(false)
}

async fn conv_event(
    self_t: &SelfData,
    root: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(&root, "t");
    let self_id = (*self_t
        .appid
        .upgrade()
        .ok_or("No appid")?
        .read()
        .unwrap_or_else(|e| e.into_inner()))
    .to_owned();
    if tp == "READY" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let bot_id_t = read_json_str(&user, "id");
        (*self_t
            .bot_id
            .upgrade()
            .ok_or("No bot_id")?
            .write()
            .unwrap_or_else(|e| e.into_inner())) = bot_id_t;
    } else if conv_group_c2c_event(self_t, &root).await? {
    } else if tp == "DIRECT_MESSAGE_CREATE" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(d, "content");
        let user = read_json_obj_or_null(d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname = read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id, &self_id, &content)?;
        let cq_msg = cq_msg_t + &deal_attachments(d)?;
        let cq_msg = deal_message_reference(d, &self_t.id_event_map)? + &cq_msg;
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqguild_public",
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":event_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
                "remark":nickname,
                "avatar":avatar
            }
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    } else if tp == "GUILD_MEMBER_ADD" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(d, "guild_id");
        let operator_id = read_json_str(d, "op_user_id");
        let sub_type = if user_id == operator_id {
            "approve"
        } else {
            "invite"
        };
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"notice",
            "platform":"qqguild_public",
            "notice_type":"group_increase",
            "message_id":event_id,
            "sub_type":sub_type,
            "groups_id":guild_id,
            "operator_id":operator_id,
            "user_id":user_id,
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    } else if tp == "GUILD_MEMBER_REMOVE" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(d, "guild_id");
        let operator_id = read_json_str(d, "op_user_id");
        let sub_type = if user_id == operator_id {
            "leave"
        } else {
            "kick"
        };
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"notice",
            "platform":"qqguild_public",
            "notice_type":"group_decrease",
            "message_id":event_id,
            "sub_type":sub_type,
            "groups_id":guild_id,
            "operator_id":operator_id,
            "user_id":user_id,
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = redreply_adapter_core::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = redreply_adapter_core::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    }
    Ok(())
}

fn get_msg_seq(
    self_t: &SelfData,
    passive_id: &str,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let mut msg_seq = 0;
    let binding = self_t
        .id_event_map
        .upgrade()
        .ok_or("id_event_map not upgrade")?;
    let lk: std::sync::RwLockReadGuard<'_, HashMap<String, (u64, serde_json::Value)>> =
        binding.read().unwrap_or_else(|e| e.into_inner());
    if let Some((_k, v)) = lk.get(passive_id) {
        let my_msg_seq = read_json_str(v, "my_msg_seq");
        if !my_msg_seq.is_empty() {
            msg_seq = my_msg_seq.parse::<i32>()?;
        }
    }
    Ok(msg_seq)
}

fn set_msg_seq(
    self_t: &SelfData,
    passive_id: &str,
    msg_seq: i32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let binding = self_t
        .id_event_map
        .upgrade()
        .ok_or("id_event_map not upgrade")?;
    let mut lk = binding.write().unwrap_or_else(|e| e.into_inner());
    if let Some(l) = lk.get_mut(passive_id) {
        l.1.as_object_mut()
            .ok_or("id_event_map is not object")?
            .insert("my_msg_seq".to_owned(), serde_json::json!(msg_seq));
    }
    Ok(())
}

fn get_passive_event_type(
    self_t: &SelfData,
    passive_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if passive_id.is_empty() {
        return Ok("".to_owned());
    }
    let binding = self_t
        .id_event_map
        .upgrade()
        .ok_or("id_event_map not upgrade")?;
    let lk = binding.read().unwrap_or_else(|e| e.into_inner());
    if let Some((_tm, event)) = lk.get(passive_id) {
        return Ok(read_json_str(event, "t"));
    }
    Ok("".to_owned())
}

fn set_qqgroup_message_reference(
    json_data: &mut serde_json::Value,
    qq_msg_node: &QQMsgNode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(message_reference) = &qq_msg_node.message_reference {
        json_data
            .as_object_mut()
            .ok_or("qq group message data not object")?
            .insert(
                "message_reference".to_owned(),
                serde_json::json!({
                    "message_id":message_reference,
                }),
            );
    }
    Ok(())
}

fn append_msg_id(id: &mut String, msg_id: &str) {
    if msg_id.is_empty() {
        return;
    }
    if !id.is_empty() {
        *id += "|";
    }
    *id += msg_id;
}

async fn do_qqgroup_json_post_wait_audit(
    self_t: &SelfData,
    group_id: &str,
    json_data: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let api_ret = do_qq_json_post(
        self_t,
        &format!("/v2/groups/{group_id}/messages"),
        json_data,
    )
    .await?;
    wait_qqgroup_audit_result(self_t, api_ret).await
}

fn get_cq_reply_event_id(
    message: &serde_json::Value,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let arr = message.as_array().ok_or("message not an array")?;
    for it in arr {
        let tp = read_json_str(it, "type");
        if tp == "reply" {
            let data = read_json_or_default(it, "data", &serde_json::Value::Null);
            let reply_id = read_json_str(data, "id");
            if !reply_id.is_empty() {
                return Ok(Some(reply_id));
            }
        }
    }
    Ok(None)
}

fn set_qqgroup_reply_info(qq_msg_node: &mut QQMsgNode, reply_event_id: &str, to_reply_id: &str) {
    if !reply_event_id.is_empty() {
        qq_msg_node.reply_event_id = Some(reply_event_id.to_owned());
    }
    if !to_reply_id.is_empty() {
        let ids = to_reply_id.split("|").collect::<Vec<&str>>();
        if let Some(message_reference) = ids.last() {
            qq_msg_node.message_reference = Some((*message_reference).to_owned());
        }
    }
}

fn qq_msg_node_has_output(qq_msg_node: &QQMsgNode) -> bool {
    !qq_msg_node.content.is_empty()
        || qq_msg_node.markdown.is_some()
        || !qq_msg_node.img_infos.is_empty()
        || !qq_msg_node.sent_img_ids.is_empty()
}

fn append_qqgroup_send_result_ids(
    self_t: &SelfData,
    id: &mut String,
    api_ret: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let null_val = serde_json::Value::Null;
    let data = api_ret.get("data").unwrap_or(&null_val);
    let event_id = read_json_str(data, "message_id");
    if event_id.is_empty() {
        return Ok(());
    }

    let raw_id = {
        let binding = self_t
            .id_event_map
            .upgrade()
            .ok_or("id_event_map not upgrade")?;
        let lk = binding.read().unwrap_or_else(|e| e.into_inner());
        if let Some((_tm, event)) = lk.get(&event_id) {
            let d = read_json_or_default(event, "d", &null_val);
            read_json_str(d, "id")
        } else {
            "".to_owned()
        }
    };

    if !raw_id.is_empty() {
        append_msg_id(id, &raw_id);
    } else {
        append_msg_id(id, &event_id);
    }
    Ok(())
}

struct QqGroupMsgChunkContext<'a> {
    group_id: &'a str,
    to_reply_id: &'a str,
    passive_id: &'a str,
    reply_event_id: &'a str,
    srv_send_msg: bool,
}

async fn send_qqgroup_msg_chunk(
    self_t: &SelfData,
    context: &QqGroupMsgChunkContext<'_>,
    chunk: &[serde_json::Value],
    id: &mut String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if chunk.is_empty() {
        return Ok(());
    }
    let mut qq_msg_node = cq_msg_to_qq(
        self_t,
        &serde_json::Value::Array(chunk.to_owned()),
        MsgSrcType::QQGroup,
        context.group_id,
        context.srv_send_msg,
    )
    .await?;
    set_qqgroup_reply_info(
        &mut qq_msg_node,
        context.reply_event_id,
        context.to_reply_id,
    );
    if !qq_msg_node_has_output(&qq_msg_node) {
        return Ok(());
    }
    let api_ret = send_qqgroup_msg(
        self_t,
        context.group_id,
        context.to_reply_id,
        context.passive_id,
        qq_msg_node,
    )
    .await?;
    append_qqgroup_send_result_ids(self_t, id, &api_ret)?;
    Ok(())
}

async fn send_qqgroup_msg_ordered(
    self_t: &SelfData,
    group_id: &str,
    to_reply_id: &str,
    passive_id: &str,
    reply_event_id: &str,
    message: &serde_json::Value,
    srv_send_msg: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let arr = message.as_array().ok_or("message not an array")?;
    let mut id = "".to_owned();
    let context = QqGroupMsgChunkContext {
        group_id,
        to_reply_id,
        passive_id,
        reply_event_id,
        srv_send_msg,
    };
    let mut chunk: Vec<serde_json::Value> = vec![];
    for it in arr {
        let tp = read_json_str(it, "type");
        if tp == "reply" {
            continue;
        }
        if tp == "image" || tp == "record" || tp == "video" || tp == "file" || tp == "qmarkdown" {
            send_qqgroup_msg_chunk(self_t, &context, &chunk, &mut id).await?;
            chunk.clear();
            send_qqgroup_msg_chunk(self_t, &context, std::slice::from_ref(it), &mut id).await?;
        } else {
            chunk.push(it.clone());
        }
    }
    send_qqgroup_msg_chunk(self_t, &context, &chunk, &mut id).await?;

    let event_id = set_event_id(
        self_t,
        &serde_json::json!({"t":"send_group_msg","d":{"id":id,"group_openid":group_id}}),
        5 * 60,
    )?;
    Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "message_id":event_id
        }
    }))
}

async fn send_qqgroup_msg(
    self_t: &SelfData,
    group_id: &str,
    to_reply_id: &str,
    passive_id: &str,
    qq_msg_node: QQMsgNode,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    if !to_reply_id.is_empty() {
        // 被动消息
        // 获取已经发送的消息的msg_seq
        let mut msg_seq = get_msg_seq(self_t, passive_id)?;
        if msg_seq >= 5 {
            return None.ok_or("回复消息已经超过5条，无法继续回复")?;
        }
        let mut id = "".to_owned();
        if !qq_msg_node.content.is_empty() {
            // 先发送文本
            msg_seq += 1;
            let mut json_data = serde_json::json!({
                "content":qq_msg_node.content,
                "msg_type":0,
                "msg_seq":msg_seq,
                "msg_id":to_reply_id
            });
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
            set_msg_seq(self_t, passive_id, msg_seq)?;
        }
        // 发送markdown
        if qq_msg_node.markdown.is_some() {
            msg_seq += 1;
            let mut json_data = serde_json::json!(qq_msg_node.markdown);
            let obj = json_data.as_object_mut().ok_or("markdown err")?;
            obj.insert("msg_type".to_owned(), serde_json::json!(2));
            obj.insert("msg_seq".to_owned(), serde_json::json!(msg_seq));
            obj.insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
            set_msg_seq(self_t, passive_id, msg_seq)?;
        }
        // 再发送图片、语音、视频、文件
        for img_id in &qq_msg_node.sent_img_ids {
            if !id.is_empty() {
                id += "|";
            }
            id += img_id;
        }
        for img_info in &qq_msg_node.img_infos {
            msg_seq += 1;
            let mut json_data = serde_json::json!({
                "msg_type":7, // 富文本
                "msg_seq":msg_seq,
                "msg_id":to_reply_id,
                "media":{
                    "file_info":img_info
                }
            });
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
            set_msg_seq(self_t, passive_id, msg_seq)?;
        }
        let event_id = set_event_id(
            self_t,
            &serde_json::json!({"t":"send_group_msg","d":{"id":id,"group_openid":group_id}}),
            5 * 60,
        )?;
        Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }))
    } else {
        // 主动消息
        let mut id = "".to_owned();
        if !qq_msg_node.content.is_empty() {
            // 先发送文本
            let mut json_data = serde_json::json!({
                "content":qq_msg_node.content,
                "msg_type":0,
            });
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
        }
        // 发送markdown
        if qq_msg_node.markdown.is_some() {
            let mut json_data = serde_json::json!(qq_msg_node.markdown);
            let obj = json_data.as_object_mut().ok_or("markdown err")?;
            obj.insert("msg_type".to_owned(), serde_json::json!(2));
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
        }
        // 再发送图片
        for img_id in &qq_msg_node.sent_img_ids {
            if !id.is_empty() {
                id += "|";
            }
            id += img_id;
        }
        for img_info in &qq_msg_node.img_infos {
            let mut json_data = serde_json::json!({
                "msg_type":7, // 富文本
                "media":{
                    "file_info":img_info
                }
            });
            set_qqgroup_message_reference(&mut json_data, &qq_msg_node)?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("发送qq group API数据(`{}`):{}", group_id, json_data).as_str(),
            );
            let api_ret = do_qqgroup_json_post_wait_audit(self_t, group_id, json_data).await?;
            let _ = redreply_adapter_core::cqapi::cq_add_log(
                format!("接收qq group API数据:{}", api_ret).as_str(),
            );
            // 构造消息id
            if !id.is_empty() {
                id += "|";
            }
            id += api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?;
        }
        let event_id = set_event_id(
            self_t,
            &serde_json::json!({"t":"send_group_msg","d":{"id":id,"group_openid":group_id}}),
            5 * 60,
        )?;
        Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }))
    }
}

async fn send_qqpri_msg_chunk(
    self_t: &SelfData,
    user_id: &str,
    to_reply_id: &str,
    passive_id: &str,
    chunk: &[serde_json::Value],
    id: &mut String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if chunk.is_empty() {
        return Ok(());
    }
    // 获取已经发送的消息的msg_seq
    let mut msg_seq = get_msg_seq(self_t, passive_id)?;
    if msg_seq >= 5 {
        return None.ok_or("回复消息已经超过5条，无法继续回复")?;
    }
    let qq_msg_node = cq_msg_to_qq(
        self_t,
        &serde_json::Value::Array(chunk.to_owned()),
        MsgSrcType::QQPri,
        user_id,
        false,
    )
    .await?;
    if !qq_msg_node_has_output(&qq_msg_node) {
        return Ok(());
    }
    if !qq_msg_node.content.is_empty() {
        // 先发送文本
        msg_seq += 1;
        let json_data = serde_json::json!({
            "content":qq_msg_node.content,
            "msg_type":0,
            "msg_seq":msg_seq,
            "msg_id":to_reply_id
        });
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("发送qq private API数据(`{}`):{}", user_id, json_data).as_str(),
        );
        let api_ret =
            do_qq_json_post(self_t, &format!("/v2/users/{user_id}/messages"), json_data).await?;
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("接收qq private API数据:{}", api_ret).as_str(),
        );
        // 构造消息id
        append_msg_id(
            id,
            api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?,
        );
        set_msg_seq(self_t, passive_id, msg_seq)?;
    }
    // 发送markdown
    if qq_msg_node.markdown.is_some() {
        msg_seq += 1;
        let mut json_data = serde_json::json!(qq_msg_node.markdown);
        let obj = json_data.as_object_mut().ok_or("markdown err")?;
        obj.insert("msg_type".to_owned(), serde_json::json!(2));
        obj.insert("msg_seq".to_owned(), serde_json::json!(msg_seq));
        obj.insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("发送qq private API数据(`{}`):{}", user_id, json_data).as_str(),
        );
        let api_ret =
            do_qq_json_post(self_t, &format!("/v2/users/{user_id}/messages"), json_data).await?;
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("接收qq private API数据:{}", api_ret).as_str(),
        );
        // 构造消息id
        append_msg_id(
            id,
            api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?,
        );
        set_msg_seq(self_t, passive_id, msg_seq)?;
    }
    // 再发送图片、语音、视频、文件
    for img_id in &qq_msg_node.sent_img_ids {
        append_msg_id(id, img_id);
    }
    for img_info in &qq_msg_node.img_infos {
        msg_seq += 1;
        let json_data = serde_json::json!({
            "msg_type":7, // 富文本
            "msg_seq":msg_seq,
            "msg_id":to_reply_id,
            "media":{
                "file_info":img_info
            }
        });
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("发送qq private API数据(`{}`):{}", user_id, json_data).as_str(),
        );
        let api_ret =
            do_qq_json_post(self_t, &format!("/v2/users/{user_id}/messages"), json_data).await?;
        let _ = redreply_adapter_core::cqapi::cq_add_log(
            format!("接收qq private API数据:{}", api_ret).as_str(),
        );
        // 构造消息id
        append_msg_id(
            id,
            api_ret
                .get("id")
                .ok_or("id not found")?
                .as_str()
                .ok_or("id not a string")?,
        );
        set_msg_seq(self_t, passive_id, msg_seq)?;
    }
    Ok(())
}

async fn send_qqpri_msg_ordered(
    self_t: &SelfData,
    user_id: &str,
    to_reply_id: &str,
    passive_id: &str,
    message: &serde_json::Value,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let arr = message.as_array().ok_or("message not an array")?;
    let mut id = "".to_owned();
    let mut chunk: Vec<serde_json::Value> = vec![];
    for it in arr {
        let tp = read_json_str(it, "type");
        if tp == "reply" {
            continue;
        }
        if tp == "image" || tp == "record" || tp == "video" || tp == "file" || tp == "qmarkdown" {
            send_qqpri_msg_chunk(self_t, user_id, to_reply_id, passive_id, &chunk, &mut id).await?;
            chunk.clear();
            send_qqpri_msg_chunk(
                self_t,
                user_id,
                to_reply_id,
                passive_id,
                std::slice::from_ref(it),
                &mut id,
            )
            .await?;
        } else {
            chunk.push(it.clone());
        }
    }
    send_qqpri_msg_chunk(self_t, user_id, to_reply_id, passive_id, &chunk, &mut id).await?;
    Ok(id)
}

pub async fn send_qqpri_msg(
    self_t: &SelfData,
    message: &serde_json::Value,
    passive_id: &str,
    user_id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let reply_id = get_reply_id(self_t, passive_id)?;
    if !reply_id.raw_ids.is_empty() {
        let to_reply_id = &reply_id.raw_ids[0];
        let id = send_qqpri_msg_ordered(self_t, user_id, to_reply_id, passive_id, message).await?;
        let event_id = set_event_id(
            self_t,
            &serde_json::json!({"t":"send_private_msg","d":{"id":id,"user_openid":user_id}}),
            5 * 60,
        )?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }));
    }
    Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"can't get reply_id",
        "data":{}
    }))
}

fn stream_err(message: &str) -> serde_json::Value {
    serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":message,
        "data":{}
    })
}

/// C2C 流式消息：start/append/end 三态，正文由调用方每次全量携带（replace 模式）。
/// 不保存任何会话状态：后续分片的 stream_msg_id 由调用方回传。
pub async fn send_c2c_stream_msg(
    self_t: &SelfData,
    json: &serde_json::Value,
    passive_id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let params = read_json_or_default(json, "params", &serde_json::Value::Null);
    let mode = read_json_str(params, "mode");
    let content = read_json_str(params, "content");
    let user_id = read_json_str(params, "user_id");
    if user_id.is_empty() {
        return Ok(stream_err("user_id is empty"));
    }
    let input_state = match mode.as_str() {
        "start" | "append" => 1,
        "end" => 10,
        _ => return Ok(stream_err("unknown mode")),
    };
    // index 从 0 递增：首片固定 0，后续分片由调用方回传
    let index = if mode == "start" {
        0
    } else {
        match params.get("index").and_then(serde_json::Value::as_u64) {
            Some(value) => value,
            None => return Ok(stream_err("index is missing")),
        }
    };

    let reply_id = match get_reply_id(self_t, passive_id) {
        Ok(value) => value,
        Err(error) => return Ok(stream_err(&format!("{error}"))),
    };
    let passive_msg_id = reply_id.raw_ids.first().cloned().unwrap_or_default();
    if mode == "start" && passive_msg_id.is_empty() {
        return Ok(stream_err("can't get reply_id"));
    }

    // 流式帧与普通回复共用同一序号计数：开始时分配一次，后续帧复用，
    // 避免固定 1 与普通回复的 msg_seq 撞车触发 QQ 去重(40054005)
    let msg_seq = if mode == "start" {
        let seq = get_msg_seq(self_t, passive_id)? + 1;
        set_msg_seq(self_t, passive_id, seq)?;
        seq
    } else {
        let seq = get_msg_seq(self_t, passive_id)?;
        if seq == 0 {
            1
        } else {
            seq
        }
    };
    let mut json_data = serde_json::json!({
        "input_mode": "append",
        "input_state": input_state,
        "index": index,
        "content_type": "markdown",
        "content_raw": content,
        "msg_seq": msg_seq,
    });
    let obj = json_data.as_object_mut().ok_or("stream json not object")?;
    if !passive_msg_id.is_empty() {
        obj.insert("msg_id".to_owned(), serde_json::json!(passive_msg_id));
    }
    if mode != "start" {
        let session_id = read_json_str(params, "session_id");
        if session_id.is_empty() {
            return Ok(stream_err("session_id is empty"));
        }
        obj.insert("stream_msg_id".to_owned(), serde_json::json!(session_id));
    }

    let api_ret = match do_qq_json_post(
        self_t,
        &format!("/v2/users/{user_id}/stream_messages"),
        json_data,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return Ok(stream_err(&format!("{error}"))),
    };
    if mode == "start" {
        let Some(stream_msg_id) = api_ret.get("id").and_then(serde_json::Value::as_str) else {
            return Ok(stream_err(&format!("响应缺少 id:{api_ret}")));
        };
        return Ok(serde_json::json!({
            "retcode": 0,
            "status": "ok",
            "data": {"session_id": stream_msg_id}
        }));
    }
    if mode == "end" {
        // QQ 不支持撤回流式消息，这里返回会话ID供脚本记录/续流使用
        let session_id = read_json_str(params, "session_id");
        return Ok(serde_json::json!({
            "retcode": 0,
            "status": "ok",
            "data": {"message_id": session_id}
        }));
    }
    Ok(serde_json::json!({
        "retcode": 0,
        "status": "ok",
        "data": {}
    }))
}

pub async fn send_group_msg(
    self_t: &SelfData,
    json: &serde_json::Value,
    passive_id: &str,
    platform: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    // 获得参数
    let params = read_json_or_default(json, "params", &serde_json::Value::Null);

    // 获得群id
    let group_id = read_json_str(params, "group_id");

    // 判断消息类型
    let msg_target_type =
        if passive_id.is_empty() && platform == "qqgroup_public" && !group_id.is_empty() {
            MsgTargetType::QQGroup
        } else {
            get_msg_type(self_t, params, passive_id)?
        };

    // 获得消息(数组格式)
    let mut message = params
        .get("message")
        .ok_or("message is not exist")?
        .to_owned();
    if message.is_string() {
        message = str_msg_to_arr_safe(&message)?;
    }
    cq_fix_image_file_in_arr(&mut message);

    if msg_target_type == MsgTargetType::QQGroup {
        // 群
        let passive_event_type = get_passive_event_type(self_t, passive_id)?;
        if passive_id.is_empty() || passive_event_type != "GROUP_AT_MESSAGE_CREATE" {
            if let Some(reply_event_id) = get_cq_reply_event_id(&message)? {
                let reply_id = get_reply_id(self_t, &reply_event_id)?;
                if !reply_id.raw_ids.is_empty() && !reply_id.is_event {
                    let to_reply_id = &reply_id.raw_ids[0];
                    return send_qqgroup_msg_ordered(
                        self_t,
                        &group_id,
                        to_reply_id,
                        &reply_event_id,
                        &reply_event_id,
                        &message,
                        true,
                    )
                    .await;
                }
            }
            return send_qqgroup_msg_ordered(self_t, &group_id, "", passive_id, "", &message, true)
                .await;
        }
        let reply_id = get_reply_id(self_t, passive_id)?;
        if !reply_id.raw_ids.is_empty() && !reply_id.is_event {
            // QQ群不支持对事件进行回复
            let to_reply_id = &reply_id.raw_ids[0];
            return send_qqgroup_msg_ordered(
                self_t,
                &group_id,
                to_reply_id,
                passive_id,
                passive_id,
                &message,
                false,
            )
            .await;
        }
    } else if msg_target_type == MsgTargetType::Guild {
        // 频道
        // 获得消息ID
        let reply_id = get_reply_id(self_t, passive_id)?;
        if !reply_id.raw_ids.is_empty() {
            let to_reply_id = &reply_id.raw_ids[0];
            return send_qqguild_msg(
                self_t,
                &group_id,
                to_reply_id,
                passive_id,
                &message,
                reply_id.is_event,
            )
            .await;
        }
    }
    Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"msg_target_type not support",
        "data":{}
    }))
}

#[async_trait]
impl BotConnectTrait for QQGuildPublicConnect {
    async fn disconnect(&mut self) {
        self.is_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(stop_tx) = &self.stop_tx {
            let _ = stop_tx.send(true).await;
        }
    }

    fn get_alive(&self) -> bool {
        !self.is_stop.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(17..).ok_or("qqguild_public url格式错误")?;
        let config_json: serde_json::Value = serde_json::from_str(config_json_str)?;
        println!("{:?}", config_json);
        (*self.appid.write().unwrap_or_else(|e| e.into_inner())) = config_json
            .get("AppID")
            .ok_or("qqguild_public AppID格式错误:没有AppID")?
            .as_str()
            .ok_or("qqguild_public AppID格式错误:AppID不是字符串")?
            .to_owned();
        let appid = (*self.appid.read().unwrap_or_else(|e| e.into_inner())).to_owned();
        self.appsecret = config_json
            .get("AppSecret")
            .ok_or("qqguild_public AppSecret格式错误:没有AppSecret")?
            .as_str()
            .ok_or("qqguild_public AppSecret格式错误:AppSecret不是字符串")?
            .to_owned();
        self.token = config_json
            .get("Token")
            .ok_or("qqguild_public Token格式错误:没有Token")?
            .as_str()
            .ok_or("qqguild_public Token格式错误:Token不是字符串")?
            .to_owned();
        let access_token_struct = token_refresh(&appid, &self.appsecret).await?;
        (*self.access_token.write().unwrap_or_else(|e| e.into_inner())) =
            access_token_struct.access_token.to_owned();

        let ws_url = get_gateway(&access_token_struct.access_token, &appid).await?;
        println!("get_gateway:{}", ws_url);

        let request = tungstenite::client::IntoClientRequest::into_client_request(&ws_url)?;
        let ws_rst;
        let port = request.uri().port().map(Into::into).unwrap_or(443);
        let host = request.uri().host().ok_or("ws url 缺少 host")?;
        let addr = format!("{host}:{port}");
        // 显式超时，避免半开连接长时间卡在 connecting
        let socket =
            tokio::time::timeout(std::time::Duration::from_secs(15), TcpStream::connect(addr))
                .await
                .map_err(|_| "TCP连接超时(15s)")??;
        ws_rst = tokio_tungstenite::client_async_tls(request, socket).await?;

        let (mut write_half, mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = ws_url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) = tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 刷新access_token
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let appsecret = self.appsecret.clone();
        let access_token = Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token);
        tokio::spawn(async move {
            // 连续失败达到 3 次才放弃并重建连接，单次失败只重试
            let mut fail_count = 0u32;
            let mut refresh_interval = std::time::Duration::from_secs(55);
            loop {
                tokio::time::sleep(refresh_interval).await;
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                } else {
                    break;
                }
                match token_refresh(&appid, &appsecret).await {
                    Ok(access_token_struct) => {
                        fail_count = 0;
                        if access_token_struct.expires_in > 0 {
                            // 按 min(expires_in-300, 3600) 调整刷新间隔，下限 30s 防止热循环
                            let secs = access_token_struct
                                .expires_in
                                .saturating_sub(300)
                                .clamp(30, 3600);
                            refresh_interval = std::time::Duration::from_secs(secs);
                        }
                        if let Some(access_token) = access_token.upgrade() {
                            (*access_token.write().unwrap_or_else(|e| e.into_inner())) =
                                access_token_struct.access_token;
                        } else {
                            break;
                        }
                    }
                    Err(err) => {
                        fail_count += 1;
                        let _ =
                            cq_add_log_w(&format!("刷新access_token失败({fail_count}/3):{err}"));
                        if fail_count >= 3 {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
            // 移除conn
            if let Some(val) = is_stop.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });

        // 这里使用弱引用，防止可能的循环依赖
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let tx_ay_t2 = tx_ay_t.clone();
        let sn = Arc::<std::sync::RwLock<Option<u64>>>::downgrade(&self.sn);

        let self_data = SelfData {
            appid: Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token: Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map: Arc::<
                std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>,
            >::downgrade(&self.id_event_map),
            bot_id: Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
            qqgroup_audit_map: Arc::clone(&self.qqgroup_audit_map),
            qqgroup_audit_pending_cache: Arc::clone(&self.qqgroup_audit_pending_cache),
        };

        tokio::spawn(async move {
            while let Some(val) = is_stop.upgrade() {
                if val.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                    msg = read_halt.next() => {
                        let Some(msg) = msg else {
                            break;
                        };
                        // 获得json数据
                        let json_dat;
                        if let Some(val) =  get_json_dat(msg) {
                            json_dat = val;
                            let _ = redreply_adapter_core::cqapi::cq_add_log(format!("qqguild_public 收到数据:{}", json_dat).as_str());
                        }else{
                            continue;
                        }
                        let op = read_json_str(&json_dat, "op");
                        if op == "10" {
                            let to_send = serde_json::json!({
                                "op":2,
                                "d":{
                                    "token":format!("QQBot {}",access_token_struct.access_token),
                                    "intents":(1 << 0) | (1 << 1) | (1 << 10) | (1 << 12) | (1 << 25) | (1 << 26) | (1 << 27) | (1 << 30),
                                    "shard":[0, 1],
                                }
                            });

                            let rst = tx_ay_t2.send(to_send).await;
                            if rst.is_err() {
                                break;
                            }
                        }else if op == "0" { // 事件
                            if let Some(sn_opt) = json_dat.get("s") {
                                if let Some(sn_t) = sn_opt.as_u64() {
                                    if let Some(val) = sn.upgrade() {
                                        (*val.write().unwrap_or_else(|e| e.into_inner())) = Some(sn_t);
                                    }else {
                                        break;
                                    }
                                }else{
                                    break;
                                }
                            }else{
                                break;
                            }
                            // 处理事件
                            let self_data = self_data.clone();
                            tokio::spawn(async move {
                                if let Err(e) = conv_event(&self_data,json_dat).await {
                                    let _ = redreply_adapter_core::cqapi::cq_add_log_w(format!("err:{:?}", e).as_str());
                                }
                            });
                        }else if op == "11" { // 心跳

                        }else if op == "7" { // 重连
                            let _ = cq_add_log_w("qq要求重连");
                            break;
                        }else if op == "9" { // 参数错误
                            let _ = cq_add_log_w("qq参数错误");
                            break;
                        }else if op == "12" { // HTTP Callback ACK

                        }
                    },
                    _ = stoprx.recv() => {
                        break;
                    }
                }
            }
            // 移除conn
            if let Some(val) = is_stop.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let _ = cq_add_log_w(&format!(
                "qqguild_public 连接已经断开(read_halt):{url_str_t}"
            ));
        });
        let url_str_t = self.url.clone();
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let sn = Arc::<std::sync::RwLock<Option<u64>>>::downgrade(&self.sn);
        tokio::spawn(async move {
            let url_str2 = url_str_t.clone();
            let is_stop2 = is_stop.clone();
            // 构造特殊心跳,防止长时间连接导致防火墙不处理数据
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Some(val) = is_stop.upgrade() {
                        if val.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                    } else {
                        break;
                    }
                    let sn_t;
                    if let Some(val) = sn.upgrade() {
                        sn_t = *val.read().unwrap_or_else(|e| e.into_inner());
                    } else {
                        break;
                    }
                    let to_send = serde_json::json!({
                        "op":1,
                        "d":sn_t
                    });
                    let rst = tx_ay_t.send(to_send).await;
                    if rst.is_err() {
                        break;
                    }
                }

                // 移除conn
                if let Some(val) = is_stop.upgrade() {
                    val.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = cq_add_log_w(&format!("qqguild_public 心跳已断开:{url_str2}"));
            });
            while let Some(msg) = rx_ay.recv().await {
                let rst = write_half
                    .send(hyper_tungstenite::tungstenite::Message::Text(
                        msg.to_string().into(),
                    ))
                    .await;
                if rst.is_err() {
                    break;
                }
            }
            // 移除conn
            if let Some(val) = is_stop2.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let _ = cq_add_log_w(&format!(
                "qqguild_public 连接已经断开(write_half):{url_str_t}"
            ));
        });
        Ok(())
    }

    async fn call_api(
        &self,
        platform: &str,
        _self_id: &str,
        passive_id: &str,
        json: &mut serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");

        let self_data = SelfData {
            appid: Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token: Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map: Arc::<
                std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>,
            >::downgrade(&self.id_event_map),
            bot_id: Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
            qqgroup_audit_map: Arc::clone(&self.qqgroup_audit_map),
            qqgroup_audit_pending_cache: Arc::clone(&self.qqgroup_audit_pending_cache),
        };
        if action == "send_group_msg" {
            return send_group_msg(&self_data, json, passive_id, platform).await;
        } else if action == "send_private_msg" {
            return send_private_msg(&self_data, json, passive_id).await;
        } else if action == "send_stream_msg" {
            return send_c2c_stream_msg(&self_data, json, passive_id).await;
        } else if action == "get_login_info" {
            return get_login_info(&self_data).await;
        } else if action == "get_group_list" {
            return get_group_list(&self_data, json, passive_id).await;
        } else if action == "get_group_member_info" {
            return get_group_member_info(&self_data, json, passive_id).await;
        } else if action == "get_stranger_info" {
            return get_stranger_info(&self_data, json, passive_id).await;
        } else if action == "delete_msg" {
            return delete_msg(&self_data, json).await;
        } else if action == "set_group_ban" {
            return set_group_ban(&self_data, json).await;
        }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        return vec![
            (
                "qqguild_public".to_owned(),
                (*self.appid.read().unwrap_or_else(|e| e.into_inner())).to_owned(),
            ),
            (
                "qqgroup_public".to_owned(),
                (*self.appid.read().unwrap_or_else(|e| e.into_inner())).to_owned(),
            ),
        ];
    }
}
