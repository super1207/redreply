use std::{sync::{atomic::AtomicBool, Arc, RwLock}, collections::HashMap, time::SystemTime};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use crate::{cqapi::cq_add_log_w, mytool::{read_json_str, read_json_obj_or_null, read_json_or_default}, botconn::qq_guild_all::{SelfData, token_refresh, get_gateway, get_json_dat}};

use super::{BotConnectTrait, qq_guild_all::{MsgSrcType, cq_msg_to_qq, MsgTargetType, get_msg_type, get_reply_id, QQMsgNode, qq_content_to_cqstr, set_event_id, deal_message_reference, deal_attachments, do_qq_json_post, str_msg_to_arr_safe, send_private_msg, send_qqguild_msg, get_login_info, get_group_list, get_group_member_info, get_stranger_info, delete_msg, set_group_ban}};

#[derive(Debug)]
pub struct QQGuildPublicConnect {
    pub url:String,
    pub appid:Arc<std::sync::RwLock<String>>,
    pub appsecret:String,
    pub token:String,
    pub access_token:Arc<std::sync::RwLock<String>>,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx:Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn:Arc<std::sync::RwLock<Option<u64>>>,
    pub id_event_map:Arc<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>,
    pub bot_id:Arc<std::sync::RwLock<String>>
}

impl QQGuildPublicConnect {
    pub fn build(url:&str) -> Self {
        QQGuildPublicConnect {
            url:url.to_owned(),
            token:"".to_owned(),
            tx:None,
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            appid: Arc::new(std::sync::RwLock::new("".to_owned())),
            appsecret: "".to_owned(),
            access_token: Arc::new(RwLock::new("".to_owned())),
            sn:Arc::new(RwLock::new(None)),
            id_event_map:Arc::new(RwLock::new(std::collections::HashMap::new())),
            bot_id:Arc::new(RwLock::new("".to_owned())),
        }
    }
}

async fn conv_event(self_t:&SelfData,root:serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(&root, "t");
    let event_id = set_event_id(self_t, &root, 60 * 5)?;
    let self_id = (*self_t.appid.upgrade().ok_or("No appid")?.read().unwrap()).to_owned();
    if tp == "READY" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(&d, "user");
        let bot_id_t = read_json_str(&user,"id");
        (*self_t.bot_id.upgrade().ok_or("No bot_id")?.write().unwrap()) = bot_id_t;
    }
    else if tp == "AT_MESSAGE_CREATE" {
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(&d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(&d, "content");
        let user = read_json_obj_or_null(&d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname =  read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id,&self_id,&content)?;
        let cq_msg = cq_msg_t + &deal_attachments(&d)?;
        let mut cq_msg = deal_message_reference(d,&self_t.id_event_map)? + &cq_msg;
        let pre1 = format!("[CQ:at,qq={self_id}] /");
        let pre2 = format!("[CQ:at,qq={self_id}] ");
        if cq_msg.starts_with(&pre1){
            cq_msg = cq_msg[pre1.len()..].to_owned();
        }else if cq_msg.starts_with(&pre2){
            cq_msg = cq_msg[pre2.len()..].to_owned();
        }
        let channel_id =read_json_str(&d, "channel_id");
        let guild_id = read_json_str(&d, "guild_id");
        let member = read_json_obj_or_null(&d, "member");
        let card =  read_json_str(&member, "nick");
        let join_time = chrono::DateTime::parse_from_rfc3339(&read_json_str(&member, "joined_at"))?.timestamp();
        let roles_arr = member.get("roles").ok_or("No roles")?.as_array().ok_or("roles not arr")?;
        let roles_arr_t = roles_arr.iter().map(|x|x.as_str().unwrap_or_default()).collect::<Vec<&str>>();
        let mut roles = "member";
        if roles_arr_t.contains(&"4"){
            roles = "owner";
        }else if roles_arr_t.contains(&"2") || roles_arr_t.contains(&"5") {
            roles = "admin";
        }
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqguild_public",
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":event_id,
            "group_id":channel_id,
            "groups_id":guild_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
                "join_time":join_time,
                "card":card,
                "sex":"unknown",
                "age":0,
                "area":"",
                "level":"0",
                "role":roles,
                "title":"",
                "avatar":avatar
            }
        });
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }
    else if tp == "GROUP_AT_MESSAGE_CREATE" {
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(&d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(&d, "content");
        let user = read_json_obj_or_null(&d, "author");
        let user_id = read_json_str(&user, "id");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id,&self_id,&content)?;
        let cq_msg = cq_msg_t + &deal_attachments(&d)?;
        let mut cq_msg = deal_message_reference(&d,&self_t.id_event_map)? + &cq_msg;
        // 去除开头的空格和/
        if cq_msg.starts_with(" /"){
            cq_msg = cq_msg[2..].to_owned();
        }else if cq_msg.starts_with("  "){ // 回复是两个空格
            cq_msg = cq_msg[2..].to_owned();
        }else if cq_msg.starts_with(" "){
            cq_msg = cq_msg[1..].to_owned();
        }
        let group_id =read_json_str(&d, "group_openid");
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
                "title":"",
            }
        });
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }
    else if tp == "DIRECT_MESSAGE_CREATE" {
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(&d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(&d, "content");
        let user = read_json_obj_or_null(&d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname =  read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id,&self_id,&content)?;
        let cq_msg = cq_msg_t + &deal_attachments(&d)?;
        let cq_msg = deal_message_reference(&d,&self_t.id_event_map)? + &cq_msg;
        let  event_json = serde_json::json!({
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
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }else if tp == "GUILD_MEMBER_ADD" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(&d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(&d, "guild_id");
        let operator_id = read_json_str(&d, "op_user_id");
        let sub_type:&str;
        if user_id == operator_id{
            sub_type = "approve";
        }else{
            sub_type = "invite";
        }
        let  event_json = serde_json::json!({
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
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }else if tp == "GUILD_MEMBER_REMOVE" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(&d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(&d, "guild_id");
        let operator_id = read_json_str(&d, "op_user_id");
        let sub_type:&str;
        if user_id == operator_id{
            sub_type = "leave";
        }else{
            sub_type = "kick";
        }
        let  event_json = serde_json::json!({
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
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    } else if tp == "C2C_MESSAGE_CREATE" {
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(&d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(&d, "content");
        let user = read_json_obj_or_null(&d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname =  read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id,&self_id,&content)?;
        let cq_msg = cq_msg_t + &deal_attachments(&d)?;
        let cq_msg = deal_message_reference(&d,&self_t.id_event_map)? + &cq_msg;
        let  event_json = serde_json::json!({
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
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }
    Ok(())
}



fn get_msg_seq(self_t:&SelfData,passive_id:&str) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let mut msg_seq = 0;
    let binding = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?;
    let lk: std::sync::RwLockReadGuard<'_, HashMap<String, (u64, serde_json::Value)>> =  binding.read().unwrap();
    if let Some((_k,v)) = lk.get(passive_id) {
        let my_msg_seq = read_json_str(v, "my_msg_seq");
        if my_msg_seq != "" {
            msg_seq = my_msg_seq.parse::<i32>()?;
        }
    }
    return Ok(msg_seq);
}

fn set_msg_seq(self_t:&SelfData,passive_id:&str,msg_seq:i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let binding = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?;
    let mut lk = binding.write().unwrap();
    if let Some(l) = lk.get_mut(passive_id) {
        l.1.as_object_mut().ok_or("id_event_map is not object")?.insert("my_msg_seq".to_owned(), serde_json::json!(msg_seq));
    }
    Ok(())
}

async fn send_qqgroup_msg(self_t:&SelfData,group_id:&str,to_reply_id:&str,passive_id:&str,qq_msg_node:QQMsgNode) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    if to_reply_id != "" { // 被动消息
        // 获取已经发送的消息的msg_seq
        let mut msg_seq = get_msg_seq(self_t,passive_id)?;
        if msg_seq >= 5 {
            return None.ok_or("回复消息已经超过5条，无法继续回复")?;
        }
        let mut id = "".to_owned();
        if qq_msg_node.content != "" { // 先发送文本
            msg_seq += 1;
            let json_data = serde_json::json!({
                "content":qq_msg_node.content,
                "msg_type":0,
                "msg_seq":msg_seq,
                "msg_id":to_reply_id
            });
            crate::cqapi::cq_add_log(format!("发送qq group API数据(`{}`):{}",group_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/groups/{group_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        // 发送markdown
        if qq_msg_node.markdown != None {
            msg_seq += 1;
            let mut json_data = serde_json::json!(qq_msg_node.markdown);
            let obj = json_data.as_object_mut().ok_or("markdown err")?;
            obj.insert("msg_type".to_owned(), serde_json::json!(2));
            obj.insert("msg_seq".to_owned(), serde_json::json!(msg_seq));
            obj.insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
            crate::cqapi::cq_add_log(format!("发送qq group API数据(`{}`):{}",group_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/groups/{group_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        // 再发送图片、语音、视频、文件
        for img_info in &qq_msg_node.img_infos {
            msg_seq += 1;
            let json_data = serde_json::json!({
                "content":" ", // 文档要求发送一个空格
                "msg_type":7, // 富文本
                "msg_seq":msg_seq,
                "msg_id":to_reply_id,
                "media":{
                    "file_info":img_info
                }
            });
            crate::cqapi::cq_add_log(format!("发送qq group API数据(`{}`):{}",group_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/groups/{group_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?;
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        let event_id = set_event_id(self_t,&serde_json::json!({"t":"send_group_msg","d":{"id":id,"channel_id":group_id}}),5 * 60)?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }));
    }
    else { // 主动消息
        let mut id = "".to_owned();
        if qq_msg_node.content != "" { // 先发送文本
            let json_data = serde_json::json!({
                "content":qq_msg_node.content,
                "msg_type":0,
            });
            crate::cqapi::cq_add_log(format!("发送qq group API数据(`{}`):{}",group_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/groups/{group_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
        }
        // 再发送图片
        for img_info in &qq_msg_node.img_infos {
            let json_data = serde_json::json!({
                "content":" ", // 文档要求发送一个空格
                "msg_type":7, // 富文本
                "media":{
                    "file_info":img_info
                }
            });
            crate::cqapi::cq_add_log(format!("发送qq group API数据(`{}`):{}",group_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/groups/{group_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
        }
        let event_id = set_event_id(self_t,&serde_json::json!({"t":"send_group_msg","d":{"id":id,"channel_id":group_id}}),5 * 60)?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }));
    }
}


pub async fn send_qqpri_msg(self_t:&SelfData,message:&serde_json::Value,passive_id:&str,user_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let reply_id = get_reply_id(self_t,passive_id)?;
    if reply_id.raw_ids.len() > 0 {
        let to_reply_id = &reply_id.raw_ids[0];
        // 获取已经发送的消息的msg_seq
        let mut msg_seq = get_msg_seq(self_t,passive_id)?;
        if msg_seq >= 5 {
            return None.ok_or("回复消息已经超过5条，无法继续回复")?;
        }
        let mut id = "".to_owned();
        let qq_msg_node = cq_msg_to_qq(&self_t,&message,MsgSrcType::QQPri,&user_id).await?;
        if qq_msg_node.content != "" { // 先发送文本
            msg_seq += 1;
            let json_data = serde_json::json!({
                "content":qq_msg_node.content,
                "msg_type":0,
                "msg_seq":msg_seq,
                "msg_id":to_reply_id
            });
            crate::cqapi::cq_add_log(format!("发送qq private API数据(`{}`):{}",user_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/users/{user_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq private API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        // 发送markdown
        if qq_msg_node.markdown != None {
            msg_seq += 1;
            let mut json_data = serde_json::json!(qq_msg_node.markdown);
            let obj = json_data.as_object_mut().ok_or("markdown err")?;
            obj.insert("msg_type".to_owned(), serde_json::json!(2));
            obj.insert("msg_seq".to_owned(), serde_json::json!(msg_seq));
            obj.insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
            crate::cqapi::cq_add_log(format!("发送qq private API数据(`{}`):{}",user_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/users/{user_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq private API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        // 再发送图片、语音、视频、文件
        for img_info in &qq_msg_node.img_infos {
            msg_seq += 1;
            let json_data = serde_json::json!({
                "content":" ", // 文档要求发送一个空格
                "msg_type":7, // 富文本
                "msg_seq":msg_seq,
                "msg_id":to_reply_id,
                "media":{
                    "file_info":img_info
                }
            });
            crate::cqapi::cq_add_log(format!("发送qq private API数据(`{}`):{}",user_id,json_data.to_string()).as_str()).unwrap();
            let api_ret = do_qq_json_post(self_t,&format!("/v2/users/{user_id}/messages"),json_data).await?;
            crate::cqapi::cq_add_log(format!("接收qq private API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?;
            set_msg_seq(self_t,passive_id,msg_seq)?;
        }
        let event_id = set_event_id(self_t,&serde_json::json!({"t":"send_private_msg","d":{"id":id,"channel_id":user_id}}),5 * 60)?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":event_id
            }
        }));
    }
    return Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"can't get reply_id",
        "data":{}
    }));
}






async fn send_group_msg(self_t:&SelfData,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    // 获得参数
    let params = read_json_or_default(json, "params",&serde_json::Value::Null);

    // 判断消息类型
    let msg_target_type: MsgTargetType = get_msg_type(&self_t, &params, passive_id)?;

    // 获得消息(数组格式)
    let mut message = params.get("message").ok_or("message is not exist")?.to_owned();
    if message.is_string() {
        message = str_msg_to_arr_safe(&message)?;
    }

    // 获得群id
    let group_id = read_json_str(&params, "group_id");

    if msg_target_type == MsgTargetType::QQGroup { // 群
        let qq_msg_node = cq_msg_to_qq(&self_t,&message,MsgSrcType::QQGroup,&group_id).await?;
        let reply_id = get_reply_id(&self_t, passive_id)?;
        if reply_id.raw_ids.len() > 0 && !reply_id.is_event { // QQ群不支持对事件进行回复
            let to_reply_id = &reply_id.raw_ids[0];
            return send_qqgroup_msg(self_t, &group_id, &to_reply_id,passive_id,qq_msg_node).await;
        }
    } 
    else if msg_target_type == MsgTargetType::Guild { // 频道
        let qq_msg_node = cq_msg_to_qq(&self_t,&message,MsgSrcType::Guild,&group_id).await?;
        // 获得消息ID
        let reply_id = get_reply_id(&self_t, passive_id)?;
        if reply_id.raw_ids.len() > 0 {
            let to_reply_id = &reply_id.raw_ids[0];
            return send_qqguild_msg(self_t, &group_id, &to_reply_id,passive_id,qq_msg_node,reply_id.is_event).await;
        }
    }
    return Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"msg_target_type not support",
        "data":{}
    }));
    
}





#[async_trait]
impl BotConnectTrait for QQGuildPublicConnect {

    async fn disconnect(&mut self){
        self.is_stop.store(true,std::sync::atomic::Ordering::Relaxed);
        if self.stop_tx.is_some() {
            let _foo = self.stop_tx.clone().unwrap().send(true).await;
        }
    }

    fn get_alive(&self) -> bool {
        return !self.is_stop.load(std::sync::atomic::Ordering::Relaxed);
    }

    
    

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        let config_json_str = self.url.get(17..).ok_or("qqguild_public url格式错误")?;
        let config_json:serde_json::Value =  serde_json::from_str(config_json_str)?;
        println!("{:?}",config_json);
        (*self.appid.write().unwrap()) = config_json.get("AppID").ok_or("qqguild_public AppID格式错误:没有AppID")?.as_str().ok_or("qqguild_public AppID格式错误:AppID不是字符串")?.to_owned();
        let appid = (*self.appid.read().unwrap()).to_owned();
        self.appsecret = config_json.get("AppSecret").ok_or("qqguild_public AppSecret格式错误:没有AppSecret")?.as_str().ok_or("qqguild_public AppSecret格式错误:AppSecret不是字符串")?.to_owned();
        self.token = config_json.get("Token").ok_or("qqguild_public Token格式错误:没有Token")?.as_str().ok_or("qqguild_public Token格式错误:Token不是字符串")?.to_owned();
        let withgroup = config_json.get("qq_withgroup").ok_or("qqguild_public url格式错误:没有 qq_withgroup")?.as_bool().ok_or("qqguild_public url格式错误:qq_withgroup不是bool")?;
        let access_token_struct = token_refresh(&appid,&self.appsecret).await?;
        (*self.access_token.write().unwrap()) = access_token_struct.access_token.to_owned();

        let ws_url = get_gateway(&access_token_struct.access_token,&appid).await?;
        println!("get_gateway:{}",ws_url);
        
        let request = tungstenite::client::IntoClientRequest::into_client_request(&ws_url)?;
        let ws_rst;
        let port_opt  = request.uri().port();
        let port;
        if port_opt.is_none() {
            port = 443;
        }else {
            port  = port_opt.unwrap().into();
        }
        let addr = format!("{}:{}",request.uri().host().unwrap(),port);
        let socket = TcpStream::connect(addr).await?;
        ws_rst = tokio_tungstenite::client_async_tls(request, socket).await?;

        let (mut write_half,mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = ws_url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 刷新access_token
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let appsecret = self.appsecret.clone();
        let access_token = Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(55)).await;
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }else {
                    break; 
                }
                if let Ok(access_token_struct) = token_refresh(&appid,&appsecret).await {
                    if let Some(access_token) = access_token.upgrade(){
                        (*access_token.write().unwrap()) = access_token_struct.access_token;
                    }else{
                        break;
                    }
                }else{
                    break;
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
            appid:Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token:Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map:Arc::<std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>>::downgrade(&self.id_event_map),
            bot_id:Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
        };
        
        tokio::spawn(async move {
            loop {
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }else {
                    break; 
                }
                tokio::select! {
                    Some(msg) = read_halt.next() => {
                        // 获得json数据
                        let json_dat;
                        if let Some(val) =  get_json_dat(msg) {
                            json_dat = val;
                            crate::cqapi::cq_add_log(format!("qqguild_public 收到数据:{}", json_dat.to_string()).as_str()).unwrap();
                        }else{
                            continue;
                        }
                        let op = read_json_str(&json_dat, "op");
                        if op == "10" {
                            let to_send;
                            if withgroup {
                                to_send = serde_json::json!({
                                    "op":2,
                                    "d":{
                                        "token":format!("QQBot {}",access_token_struct.access_token),
                                        "intents":0 | (1 << 0) | (1 << 1) | (1 << 10) | (1 << 12) | (1 << 26) | (1 << 27) | (1 << 30) | (1 << 25),
                                        "shard":[0, 1],
                                    }
                                });
                            }else{
                                to_send = serde_json::json!({
                                    "op":2,
                                    "d":{
                                        "token":format!("QQBot {}",access_token_struct.access_token),
                                        "intents":0 | (1 << 0) | (1 << 1) | (1 << 10) | (1 << 12) | (1 << 26) | (1 << 27) | (1 << 30),
                                        "shard":[0, 1],
                                    }
                                });
                            }
                            
                            let rst = tx_ay_t2.send(to_send).await;
                            if rst.is_err() {
                                break;
                            }
                        }else if op == "0" { // 事件
                            if let Some(sn_opt) = json_dat.get("s") {
                                if let Some(sn_t) = sn_opt.as_u64() {
                                    if let Some(val) = sn.upgrade() {
                                        (*val.write().unwrap()) = Some(sn_t);
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
                                    crate::cqapi::cq_add_log_w(format!("err:{:?}", e).as_str()).unwrap();
                                }
                            });
                        }else if op == "11" { // 心跳
                            
                        }else if op == "7" { // 重连
                            cq_add_log_w("qq要求重连").unwrap();
                            break;
                        }else if op == "9" { // 参数错误
                            cq_add_log_w("qq参数错误").unwrap();
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
            cq_add_log_w(&format!("qqguild_public 连接已经断开(read_halt):{url_str_t}")).unwrap();
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
                    }else {
                        break; 
                    }
                    let sn_t;
                    if let Some(val) = sn.upgrade() {
                        sn_t = val.read().unwrap().clone();
                    }else {
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
                cq_add_log_w(&format!("qqguild_public 心跳已断开:{url_str2}")).unwrap();
            });
            while let Some(msg) = rx_ay.recv().await {
                let rst = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string())).await;
                if rst.is_err() {
                    break;
                }
            }
            // 移除conn
            if let Some(val) = is_stop2.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            cq_add_log_w(&format!("qqguild_public 连接已经断开(write_half):{url_str_t}")).unwrap();
        });
        Ok(())
    }

    async fn call_api(&self,_platform:&str,_self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");

        let self_data = SelfData {
            appid:Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token:Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map:Arc::<std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>>::downgrade(&self.id_event_map),
            bot_id:Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
        };
        if action == "send_group_msg" {
            return send_group_msg(&self_data,json,passive_id).await;
        }
        else if action == "send_private_msg" {
            return send_private_msg(&self_data,json,passive_id).await;
        }
        else if action == "get_login_info" {
            return get_login_info(&self_data).await;
        }
        else if action == "get_group_list" {
            return get_group_list(&self_data,json,passive_id).await;
        }
        else if action == "get_group_member_info" {
            return get_group_member_info(&self_data,json,passive_id).await;
        }
        else if action == "get_stranger_info" {
            return get_stranger_info(&self_data,json,passive_id).await;
        }
        else if action == "delete_msg" {
            return delete_msg(&self_data,json).await;
        }
        else if action == "set_group_ban" {
            return set_group_ban(&self_data,json).await;
        }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String,String)> {
        return vec![("qqguild_public".to_owned(),(*self.appid.read().unwrap()).to_owned()),("qqgroup_public".to_owned(),(*self.appid.read().unwrap()).to_owned())];
    }
}