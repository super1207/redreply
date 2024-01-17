use std::{sync::{atomic::AtomicBool, Arc, RwLock}, str::FromStr, collections::HashMap, time::SystemTime};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use hyper::header::{HeaderValue, HeaderName};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use crate::{cqapi::cq_add_log_w, mytool::{read_json_str, read_json_obj_or_null, cq_text_encode, cq_params_encode, str_msg_to_arr}};

use super::BotConnectTrait;

#[derive(Debug)]
pub struct QQGuildPublicConnect {
    pub url:String,
    pub appid:String,
    pub appsecret:String,
    pub token:String,
    pub access_token:Arc<std::sync::RwLock<String>>,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx:Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn:Arc<std::sync::RwLock<Option<u64>>>,
    pub user_guild_dms_map:Arc<std::sync::RwLock<std::collections::HashMap<String,String>>>,
    pub id_event_map:Arc<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>,
    pub bot_id:Arc<std::sync::RwLock<String>>
}

struct AccessTokenStruct {
    access_token:String,
    _expires_in:u64,
}

async fn token_refresh(appid:&str,client_secret:&str) -> Result<AccessTokenStruct, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://bots.qq.com/app/getAppAccessToken")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let json_data:serde_json::Value = serde_json::json!({
        "appId":appid,
        "clientSecret":client_secret
    });
    let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    // println!("token_refresh:{}",ret_str);
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    let expires_in = read_json_str(&json_val, "expires_in").parse::<u64>()?;
    Ok(AccessTokenStruct {
        access_token: json_val.get("access_token").ok_or("No access_token")?.as_str().ok_or("access_token not str")?.to_owned(),
        _expires_in:expires_in,
    })
}

async fn get_gateway(access_token:&str,appid:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://api.sgroup.qq.com/gateway")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {access_token}"))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(appid)?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    Ok(json_val.get("url").ok_or("No url")?.as_str().ok_or("url not str")?.to_owned())
}

fn get_json_from_msg(msg:hyper_tungstenite::tungstenite::Message) -> Option<serde_json::Value> {
    if let Ok(msg_text) = msg.to_text() {
        if let Ok(json_dat_t) = serde_json::from_str::<serde_json::Value>(&msg_text) {
            if json_dat_t.is_object() {
                return Some(json_dat_t);
            }else {
                return None;
            }
        } else {
            return None;
        }
    }else {
        return None;
    }
}

fn get_json_dat(msg:Result<hyper_tungstenite::tungstenite::Message, hyper_tungstenite::tungstenite::Error>) -> Option<serde_json::Value> {
    let json_dat_opt:Option<serde_json::Value>;
    if let Ok(msg) = msg{
        json_dat_opt = get_json_from_msg(msg);
    }else {
        return None;
    }
    //得到json_dat
    let json_dat:serde_json::Value;
    if let Some(json_dat_t) = json_dat_opt {
        json_dat = json_dat_t;
    }else{
        return None;
    }
    crate::cqapi::cq_add_log(format!("qqguild_public 收到数据:{}", json_dat.to_string()).as_str()).unwrap();
    return Some(json_dat);
}


impl QQGuildPublicConnect {
    pub fn build(url:&str) -> Self {
        QQGuildPublicConnect {
            url:url.to_owned(),
            token:"".to_owned(),
            tx:None,
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            appid: "".to_owned(),
            appsecret: "".to_owned(),
            access_token: Arc::new(RwLock::new("".to_owned())),
            sn:Arc::new(RwLock::new(None)),
            user_guild_dms_map:Arc::new(RwLock::new(std::collections::HashMap::new())),
            id_event_map:Arc::new(RwLock::new(std::collections::HashMap::new())),
            bot_id:Arc::new(RwLock::new("".to_owned())),
        }
    }
}

fn deal_attachments(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut retstr = String::new();
    if let Some(attachments) = root.get("attachments") {
        if let Some(attachments_t) = attachments.as_array() {
            for it in attachments_t {
                if read_json_str(it, "content_type").starts_with("image/") {
                    let url = read_json_str(it, "url");
                    let url_t = cq_params_encode(&url);
                    retstr += format!("[CQ:image,file={url_t},url={url_t}]").as_str();
                }
            }
            return Ok(retstr);
        }else{
            return Ok("".to_owned());
        }
    }else{
        return Ok("".to_owned());
    }

}

fn deal_message_reference(root:&serde_json::Value,id_event_map:std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    
    if let Some(message_reference) = root.get("message_reference") {
        let raw_id = read_json_str(message_reference, "message_id");
        if raw_id == "" {
            return Ok("".to_owned());
        }
        let id_event_map_t = id_event_map.upgrade().ok_or("id_event_map not exist")?;
        let lk = id_event_map_t.read().unwrap();
        for (key,(_,json)) in &*lk {
            let tp = json.get("t").ok_or("no t in id_event_map")?;
            if tp == "GROUP_AT_MESSAGE_CREATE" || tp == "AT_MESSAGE_CREATE" ||tp == "DIRECT_MESSAGE_CREATE" || tp == "send_private_msg" || tp == "send_group_msg" {
                let d = json.get("d").ok_or("no d in id_event_map")?;
                let id = d.get("id").ok_or("no id in msg id_event_map")?.as_str().ok_or("id not str")?;
                if id.contains(&raw_id) {
                    return Ok(format!("[CQ:reply,id={key}]"));
                }
            }
        }
        return Ok("".to_owned());
     }else{
         return Ok("".to_owned());
     }

}

async fn conv_event(bot_id:std::sync::Weak<std::sync::RwLock<String>>,self_id:&str,root:serde_json::Value,user_guild_dms_map:std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,String>>>,id_event_map:std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(&root, "t");
    let event_id;
    {
        let curr_tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let event_id_struct = root.clone();
        let event_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 60 * 5;
        event_id = uuid::Uuid::new_v4().to_string();
        let id_event_map_t = id_event_map.upgrade().ok_or("No id_event_map")?;
        let mut lk =  id_event_map_t.write().unwrap();
        lk.insert(event_id.to_owned(), (event_time,event_id_struct));
        let mut to_remove = vec![];
        for (key,(key_tm,_)) in &*lk{
            if curr_tm > *key_tm {
                to_remove.push(key.to_owned());
            }
        }
        for key in to_remove {
            lk.remove(&key);
        }
    }
    if tp == "READY" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(&d, "user");
        let bot_id_t = read_json_str(&user,"id");
        (*bot_id.upgrade().ok_or("No bot_id")?.write().unwrap()) = bot_id_t;
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
        let cq_msg_t = qq_content_to_cqstr(bot_id,self_id,&content)?;
        let cq_msg = deal_attachments(&d)? + &cq_msg_t;
        let mut cq_msg = deal_message_reference(&d,id_event_map)? + &cq_msg;
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
        let cq_msg_t = qq_content_to_cqstr(bot_id,self_id,&content)?;
        let cq_msg = deal_attachments(&d)? + &cq_msg_t;
        let mut cq_msg = deal_message_reference(&d,id_event_map)? + &cq_msg;
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
            "platform":"qqguild_public",
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
        let cq_msg_t = qq_content_to_cqstr(bot_id,self_id,&content)?;
        let cq_msg = deal_attachments(&d)? + &cq_msg_t;
        let cq_msg = deal_message_reference(&d,id_event_map)? + &cq_msg;
        let guild_id = read_json_str(&d, "guild_id");
        user_guild_dms_map.upgrade().ok_or("upgrade user_guild_dms_map 失败")?.write().unwrap().insert(user_id.to_owned(),guild_id);
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
    }
    Ok(())
}

fn make_qq_text(text:&str) -> String {
    let mut ret = String::new();
    for ch in text.chars() {
        match ch {
            '&' => {
                ret += "&amp;";
            }
            '<' => {
                ret += "&lt;";
            }
            '>' => {
                ret += "&gt;";
            }
            _ => {
                ret += &ch.to_string();
            }
        }
    }
    ret
}

struct QQMsgNode{
    content:String,
    imgs:Vec<Vec<u8>>,
    img_infos:Vec<String>,
    message_reference:Option<String>,
    markdown:Option<serde_json::Value>
}

// fn get_raw_msg_id() -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

// }


async fn cq_msg_to_qq(self_t:&QQGuildPublicConnect,js_arr:&serde_json::Value,is_group:bool,group_id:&str) -> Result<QQMsgNode,Box<dyn std::error::Error + Send + Sync>> {
    let mut msg_node = QQMsgNode{
        content: "".to_string(),
        imgs: vec![],
        img_infos:vec![],
        message_reference:None,
        markdown: None,
    };
    let arr = js_arr.as_array().ok_or("js_arr not an err")?;
    // let mut out = String::new();
    for it in arr {
        let tp = it.get("type").ok_or("type not found")?;
        if tp == "text" {
            let text = it.get("data").ok_or("data not found")?.get("text").ok_or("text not found")?.as_str().ok_or("text not a string")?;
            msg_node.content += &make_qq_text(&text);
        } else if tp == "at" {
            if !is_group{
                let qq = it.get("data").ok_or("data not found")?.get("qq").ok_or("qq not found")?.as_str().ok_or("qq not a string")?;
                if qq == "all" {
                    msg_node.content += "@全体成员"
                }else {
                    msg_node.content += &format!("<@{}>", make_qq_text(qq));
                }
            }
            
        }
        else if tp == "image" {
            let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
            if !is_group{
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    let client = reqwest::Client::builder().no_proxy().build()?;
                    let req = client.get(file).build()?;
                    let ret = client.execute(req).await?;
                    let img_buffer =  ret.bytes().await?.to_vec();
                    msg_node.imgs.push(img_buffer);
                }else if file.starts_with("base64://") {
                    let b64_str = file.split_at(9).1;
                    let img_buffer = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        base64::engine::general_purpose::PAD), b64_str)?;
                    msg_node.imgs.push(img_buffer);
                }
            }else{
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/files"))?;
                    let client = reqwest::Client::builder().no_proxy().build()?;
                    let json_data = serde_json::json!({
                        "file_type":1,
                        "url":file,
                        "srv_send_msg":false
                    });
                    let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
                    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
                    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
                    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
                    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
                    let ret = client.execute(req).await?;
                    let ret_str =  ret.text().await?; 
                    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
                    crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
                    msg_node.img_infos.push(json_val.get("file_info").ok_or("file_info not found")?.as_str().ok_or("file_info not a string")?.to_owned());
                }
            }
            
        } else if tp == "face" {
            if !is_group {
                let face_id = it.get("data").ok_or("data not found")?.get("id").ok_or("face id not found")?.as_str().ok_or("face id not a string")?;
                msg_node.content += &format!("<emoji:{}>", make_qq_text(face_id));
            }
        }
        else if tp == "reply" {
            if !is_group {
                let reply_id = it.get("data").ok_or("data not found")?.get("id").ok_or("reply_id not found")?.as_str().ok_or("reply_id not a string")?;
                let lk = self_t.id_event_map.read().unwrap();
                if let Some((_,event)) = lk.get(reply_id) {
                    let d = read_json_obj_or_null(event, "d");
                    let message_id = read_json_str(&d, "id");
                    if !message_id.contains("|") {
                        msg_node.message_reference = Some(message_id);
                    }else{
                        let ids = message_id.split("|").collect::<Vec<&str>>();
                        let id = ids[ids.len() - 1];
                        msg_node.message_reference = Some(id.to_owned());
                    }
                } else {
                    cq_add_log_w(&format!("消息ID`{reply_id}`失效")).unwrap();
                }
            }
        }
        // else if tp == "markdown" {
        //     let markdown_data = it.get("data").ok_or("data not found")?.get("data").ok_or("markdown data not found")?.as_str().ok_or("markdown data not a string")?;
        //     if markdown_data.starts_with("base64://"){
        //         let b64_str = markdown_data.split_at(9).1;
        //         let markdown_buffer = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
        //             &base64::alphabet::STANDARD,
        //             base64::engine::general_purpose::PAD), b64_str)?;
        //         let json:serde_json::Value = serde_json::from_str(&String::from_utf8(markdown_buffer)?)?;
        //         msg_node.markdown = Some(serde_json::json!({
        //             "custom_template_id": "101993071_1658748972",
        //             "params":json
        //         }));
        //     }
        // }
    }
    Ok(msg_node)
}


fn str_msg_to_arr_safe(js:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let ret = str_msg_to_arr(js);
    if let Ok(ret) = ret {
        return Ok(ret);
    }else {
        return None.ok_or(format!("str_msg_to_arr error:{}", ret.err().unwrap()))?;
    }
}


async fn _send_guild_msg(self_t:&QQGuildPublicConnect,json:&serde_json::Value,to_reply_id:&str,is_event:bool) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>  {
    let params = read_json_obj_or_null(json, "params");  
    let group_id = read_json_str(&params, "group_id");


    let message = params.get("message").ok_or("message is not exist")?;
    let qq_msg_node;
    if message.is_array() {
        qq_msg_node = cq_msg_to_qq(self_t,message,false,"").await?;
        
    }else{
        
        let msg_arr_rst = str_msg_to_arr_safe(message);
        if let Ok(msg_arr) = msg_arr_rst {
            qq_msg_node = cq_msg_to_qq(self_t,&msg_arr,false,"").await?;
        }else{
            return None.ok_or("call str_msg_to_arr err")?;
        }
        
    }

    let to_reply_id_opt;
    if to_reply_id != "" {
        to_reply_id_opt = Some(to_reply_id.to_owned());
    }else{
        to_reply_id_opt = None;
    }

    let mut id = String::new();
    if qq_msg_node.imgs.len() == 0 {
        let mut json_data = serde_json::json!({
            "content":qq_msg_node.content,
        });
        if is_event {
            json_data.as_object_mut().unwrap().insert("event_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }else{
            json_data.as_object_mut().unwrap().insert("msg_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }
        if qq_msg_node.message_reference != None {
            json_data.as_object_mut().unwrap().insert("message_reference".to_owned(), serde_json::json!({
                "message_id":qq_msg_node.message_reference,
            }));
        }
        if qq_msg_node.markdown != None {
            json_data.as_object_mut().unwrap().insert("markdown".to_owned(), qq_msg_node.markdown.unwrap());
        }
        // 处理日志
        {
            let js_str = json_data.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", js_str).as_str()).unwrap();
            }
        }
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{group_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        id = json_val.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
    } else {
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{group_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut form = reqwest::multipart::Form::new().part(
        "file_image",
        reqwest::multipart::Part::bytes(qq_msg_node.imgs[0].clone()).file_name("pic.png"),
        );
        if to_reply_id_opt != None {
            if is_event {
                form = form.text("event_id", to_reply_id.to_owned());
            }else{
                form = form.text("msg_id", to_reply_id.to_owned());
            }
            
        }
        if qq_msg_node.message_reference != None {
            form = form.text("message_reference",qq_msg_node.message_reference.clone().unwrap());
        }
        form = form.text("content",qq_msg_node.content);
        let mut req = client.post(uri.to_owned()).multipart(form).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        
        for it in qq_msg_node.imgs.get(1..).unwrap() {
            let mut form = reqwest::multipart::Form::new().part(
                "file_image",
                reqwest::multipart::Part::bytes(it.clone()).file_name("pic.png"),
                );
            if is_event {
                form = form.text("event_id", to_reply_id.to_owned());
            }else{
                form = form.text("msg_id", to_reply_id.to_owned());
            }
            let mut req = client.post(uri.to_owned()).multipart(form).build()?;
            req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
            req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
            req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
            req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
            //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
            let ret = client.execute(req).await?;
            let ret_str =  ret.text().await?; 
            let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
            crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
            id += "|";
            id += &read_json_str(&json_val, "id");
        }
    }
    let event_id;
    {
        let curr_tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let event_id_struct = serde_json::json!({
            "t":"send_group_msg",
            "d":{
                "id":id,
                "channel_id":group_id
            }
        });
        let event_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 60 * 5;
        event_id = uuid::Uuid::new_v4().to_string();
        let id_event_map_t = &self_t.id_event_map;
        let mut lk =  id_event_map_t.write().unwrap();
        lk.insert(event_id.to_owned(), (event_time,event_id_struct));
        let mut to_remove = vec![];
        for (key,(key_tm,_)) in &*lk{
            if curr_tm > *key_tm {
                to_remove.push(key.to_owned());
            }
        }
        for key in to_remove {
            lk.remove(&key);
        }
    }
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "message_id":event_id
        }
    }));
}

async fn send_group_msg(self_t:&QQGuildPublicConnect,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");  
    let group_id = read_json_str(&params, "group_id");

    let is_event;
    let is_group;

    let mut to_reply_id:String = "".to_owned();
    if let Some((_tm,event)) = self_t.id_event_map.read().unwrap().get(passive_id) {
        let tp = read_json_str(event, "t");
        if tp == "GUILD_MEMBER_REMOVE" || tp == "GUILD_MEMBER_ADD"{
            to_reply_id = read_json_str(&event, "id");
            is_event = true;
        }else{
            let d = read_json_obj_or_null(event, "d");
            to_reply_id = read_json_str(&d, "id");
            is_event = false;
        }
        if tp == "GROUP_AT_MESSAGE_CREATE" {
            is_group = true;
        }else{
            is_group = false;
        } 
    }else {
        is_event = false;
        is_group = false;
    }

    if !is_group {
        return _send_guild_msg(self_t, json, &to_reply_id, is_event).await;
    }

    let message = params.get("message").ok_or("message is not exist")?;
    let qq_msg_node;
    if message.is_array() {
        qq_msg_node = cq_msg_to_qq(self_t,message,true,&group_id).await?;
        
    }else{
        
        let msg_arr_rst = str_msg_to_arr_safe(message);
        if let Ok(msg_arr) = msg_arr_rst {
            qq_msg_node = cq_msg_to_qq(self_t,&msg_arr,true,&group_id).await?;
        }else{
            return None.ok_or("call str_msg_to_arr err")?;
        }
        
    }

    let to_reply_id_opt;
    if to_reply_id != "" {
        to_reply_id_opt = Some(to_reply_id.to_owned());
    }else{
        to_reply_id_opt = None;
    }

    let mut id = String::new();

    // 获取已经发送的消息
    let mut msg_seq = 0;
    if passive_id != ""{
        let lk: std::sync::RwLockReadGuard<'_, HashMap<String, (u64, serde_json::Value)>> =  self_t.id_event_map.read().unwrap();
        if let Some((_k,v)) = lk.get(passive_id) {
            let my_msg_seq = read_json_str(v, "my_msg_seq");
            if my_msg_seq != "" {
                msg_seq = my_msg_seq.parse::<i32>()?;
            }
            
        }
    }

    // 若有文本，先发送文本
    if qq_msg_node.content != "" {
        msg_seq += 1;
        let mut json_data = serde_json::json!({
            "content":qq_msg_node.content,
            "msg_type":0,
            "msg_seq":msg_seq
        });
        if is_event {
            json_data.as_object_mut().unwrap().insert("event_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }else{
            json_data.as_object_mut().unwrap().insert("msg_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }
        if qq_msg_node.message_reference != None {
            json_data.as_object_mut().unwrap().insert("message_reference".to_owned(), serde_json::json!({
                "message_id":qq_msg_node.message_reference,
            }));
        }
        // 处理日志
        {
            let js_str = json_data.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", js_str).as_str()).unwrap();
            }
        }
        let uri= reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/messages"))?;
        
        let client = reqwest::Client::builder().no_proxy().build()?;
        
        let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
        if id != "" {
            id += "|";
        }
        id += &json_val.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();

        // 保存msg_seq
        {
            let mut lk = self_t.id_event_map.write().unwrap();
            if let Some(l) = lk.get_mut(passive_id) {
                l.1.as_object_mut().ok_or("id_event_map is not object")?.insert("my_msg_seq".to_owned(), serde_json::json!(msg_seq));
            }
        }
    }

    // 然后再发送图片
    for img_info in &qq_msg_node.img_infos {
        msg_seq += 1;
        let mut json_data = serde_json::json!({
            "content":" ", // 文档要求发送一个空格
            "msg_type":7, // 富文本
            "msg_seq":msg_seq,
            "media":{
                "file_info":img_info
            }
        });
        if is_event {
            json_data.as_object_mut().unwrap().insert("event_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }else{
            json_data.as_object_mut().unwrap().insert("msg_id".to_owned(), serde_json::json!(to_reply_id_opt));
        }
        if qq_msg_node.message_reference != None {
            json_data.as_object_mut().unwrap().insert("message_reference".to_owned(), serde_json::json!({
                "message_id":qq_msg_node.message_reference,
            }));
        }
        // 处理日志
        {
            let js_str = json_data.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", js_str).as_str()).unwrap();
            }
        }
        let uri= reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/messages"))?;
        
        let client = reqwest::Client::builder().no_proxy().build()?;
        
        let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
        if id != "" {
            id += "|";
        }
        id += &json_val.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
        // 保存msg_seq
        {
            let mut lk = self_t.id_event_map.write().unwrap();
            if let Some(l) = lk.get_mut(passive_id) {
                l.1.as_object_mut().ok_or("id_event_map is not object")?.insert("my_msg_seq".to_owned(), serde_json::json!(msg_seq));
            }
        }
    }
    let event_id;
    {
        let curr_tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let event_id_struct = serde_json::json!({
            "t":"send_group_msg",
            "d":{
                "id":id,
                "channel_id":group_id
            }
        });
        let event_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 60 * 5;
        event_id = uuid::Uuid::new_v4().to_string();
        let id_event_map_t = &self_t.id_event_map;
        let mut lk =  id_event_map_t.write().unwrap();
        lk.insert(event_id.to_owned(), (event_time,event_id_struct));
        let mut to_remove = vec![];
        for (key,(key_tm,_)) in &*lk{
            if curr_tm > *key_tm {
                to_remove.push(key.to_owned());
            }
        }
        for key in to_remove {
            lk.remove(&key);
        }
    }
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "message_id":event_id
        }
    }));

}


async fn send_private_msg(self_t:&QQGuildPublicConnect,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let user_id = read_json_str(&params, "user_id");

    let guild_id = self_t.user_guild_dms_map.read().unwrap().get(&user_id).ok_or("user_id not match any guild")?.to_owned();

    let mut to_reply_id:String = "".to_owned();
    if let Some((_tm,event)) = self_t.id_event_map.read().unwrap().get(passive_id) {
        let d = read_json_obj_or_null(event, "d");
        to_reply_id = read_json_str(&d, "id");
    }

    let message = params.get("message").ok_or("message is not exist")?;
    let qq_msg_node;
    if message.is_array() {
        qq_msg_node = cq_msg_to_qq(self_t,message,false,"").await?;
        
    }else{
        
        let msg_arr_rst = str_msg_to_arr_safe(message);
        if let Ok(msg_arr) = msg_arr_rst {
            qq_msg_node = cq_msg_to_qq(self_t,&msg_arr,false,"").await?;
        }else{
            return None.ok_or("call str_msg_to_arr err")?;
        }
        
    }

    let to_reply_id_opt;
    if to_reply_id != "" {
        to_reply_id_opt = Some(to_reply_id.to_owned());
    }else{
        to_reply_id_opt = None;
    }
    let mut id = String::new();
    if qq_msg_node.imgs.len() == 0 {
        let json_data = serde_json::json!({
            "msg_id":to_reply_id_opt,
            "content":qq_msg_node.content
        });
        // 处理日志
        {
            let js_str = json_data.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", js_str).as_str()).unwrap();
            }
        }
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/dms/{guild_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        id = json_val.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
    } else {
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/dms/{guild_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut form = reqwest::multipart::Form::new().part(
        "file_image",
        reqwest::multipart::Part::bytes(qq_msg_node.imgs[0].clone()).file_name("pic.png"),
        );
        if to_reply_id_opt != None {
            form = form.text("msg_id", to_reply_id.to_owned());
        }
        form = form.text("content",qq_msg_node.content);
        let mut req = client.post(uri.to_owned()).multipart(form).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        for it in qq_msg_node.imgs.get(1..).unwrap() {
            let mut form = reqwest::multipart::Form::new().part(
                "file_image",
                reqwest::multipart::Part::bytes(it.clone()).file_name("pic.png"),
                );
            if to_reply_id_opt != None {
                form = form.text("msg_id", to_reply_id.to_owned());
            }
            let mut req = client.post(uri.to_owned()).multipart(form).build()?;
            req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
            req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
            req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
            req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
            //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
            let ret = client.execute(req).await?;
            let ret_str =  ret.text().await?; 
            let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
            crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
            id += "|";
            id += &read_json_str(&json_val, "id");
        }
    }
    let event_id;
    {
        let curr_tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let event_id_struct = serde_json::json!({
            "t":"send_private_msg",
            "d":{
                "id":id,
                "guild_id":guild_id
            }
        });
        let event_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 60 * 5;
        event_id = uuid::Uuid::new_v4().to_string();
        let id_event_map_t = &self_t.id_event_map;
        let mut lk =  id_event_map_t.write().unwrap();
        lk.insert(event_id.to_owned(), (event_time,event_id_struct));
        let mut to_remove = vec![];
        for (key,(key_tm,_)) in &*lk{
            if curr_tm > *key_tm {
                to_remove.push(key.to_owned());
            }
        }
        for key in to_remove {
            lk.remove(&key);
        }
    }
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "message_id":event_id
        }
    }));

}

async fn get_login_info(self_t:&QQGuildPublicConnect) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/users/@me"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let user: serde_json::Value = serde_json::from_str(&ret_str)?;
    let nickname = read_json_str(&user, "username");
    let avatar = read_json_str(&user, "avatar");
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "user_id":self_t.appid,
            "nickname":nickname,
            "avatar":avatar,
        }
    }));

}

async fn get_group_list(self_t:&QQGuildPublicConnect,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");
        
    let mut groups_id = read_json_str(&params, "groups_id");

    if groups_id == ""{
        if let Some((_tm,event)) = self_t.id_event_map.read().unwrap().get(passive_id) {
            let d = read_json_obj_or_null(event, "d");
            groups_id = read_json_str(&d, "guild_id");
        } 
    }

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/{groups_id}/channels"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let group_list: serde_json::Value = serde_json::from_str(&ret_str)?;

    let mut ret_group = vec![];
    for it in group_list.as_array().ok_or("group_list is not array，maybe groups_id is wrong")? {
        let channel_type = read_json_str(it, "type");
        if channel_type == "0" {
            let group_name = read_json_str(it, "name");
            let group_id = read_json_str(it, "id");
            ret_group.push(serde_json::json!({
                "group_name":group_name,
                "group_id":group_id,
                "member_count":0,
                "max_member_count":0
            }));
        }
    }


    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":ret_group
    }));

}

async fn get_stranger_info(self_t:&QQGuildPublicConnect,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let user_id = read_json_str(&params, "user_id");

    if user_id == self_t.appid {
        return get_login_info(self_t).await;
    }

    let mut guild_id:String = "".to_owned();
    if let Some((_tm,event)) = self_t.id_event_map.read().unwrap().get(passive_id) {
        let d = read_json_obj_or_null(event, "d");
        guild_id = read_json_str(&d, "guild_id");
    }

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/{guild_id}/members/{user_id}"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    //crate::cqapi::cq_add_log(format!("json_val:{:?}", json_val).as_str()).unwrap();
    let user = read_json_obj_or_null(&json_val, "user");
    let nickname = read_json_str(&user, "username");
    let avatar = read_json_str(&user, "avatar");
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "user_id":user_id,
            "nickname":nickname,
            "avatar":avatar,
        }
    }));

}

async fn delete_msg(self_t:&QQGuildPublicConnect,json:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let message_id = read_json_str(&params, "message_id");

    let event:serde_json::Value = self_t.id_event_map.read().unwrap().get(&message_id).ok_or("event is not found")?.1.to_owned();
    
    let tp = read_json_str(&event, "t");
    if tp == "MESSAGE_CREATE" || tp == "send_group_msg" {
        let d = read_json_obj_or_null(&event, "d");
        let channel_id = read_json_str(&d, "channel_id");
        let message_id = read_json_str(&d, "id");
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{channel_id}/messages/{message_id}?hidetip=false"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut req = client.delete(uri).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        let ret = client.execute(req).await?;
        if ret.status() != 200 {
            let ret_str =  ret.text().await?; 
            cq_add_log_w(&format!("delete_msg:{:?}", &ret_str)).unwrap();
        }
        
    }else if tp == "send_private_msg" {
        let d = read_json_obj_or_null(&event, "d");
        let guild_id = read_json_str(&d, "guild_id");
        let message_id = read_json_str(&d, "id");
        if !message_id.contains("|") {
            let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/dms/{guild_id}/messages/{message_id}?hidetip=false"))?;
            let client = reqwest::Client::builder().no_proxy().build()?;
            let mut req = client.delete(uri).build()?;
            req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
            req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
            req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
            req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
            let ret = client.execute(req).await?;
            if ret.status() != 200 {
                let ret_str =  ret.text().await?; 
                cq_add_log_w(&format!("delete_msg:{:?}", &ret_str)).unwrap();
            }
            
        }else{
            let ids = message_id.split("|").collect::<Vec<&str>>();
            for message_id in ids {
                let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/dms/{guild_id}/messages/{message_id}?hidetip=false"))?;
                let client = reqwest::Client::builder().no_proxy().build()?;
                let mut req = client.delete(uri).build()?;
                req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
                req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
                req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
                req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
                let ret = client.execute(req).await?;
                if ret.status() != 200 {
                    let ret_str =  ret.text().await?; 
                    cq_add_log_w(&format!("delete_msg:{:?}", &ret_str)).unwrap();
                }
            }
        }
        
    }
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{}
    }));

}

async fn get_group_member_info(self_t:&QQGuildPublicConnect,json:&serde_json::Value,_passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let mut user_id = read_json_str(&params, "user_id");

    if user_id == self_t.appid {
        user_id = self_t.bot_id.read().unwrap().to_owned();
    }

    let group_id = read_json_str(&params, "group_id");


    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{}",group_id))?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let channel_info: serde_json::Value = serde_json::from_str(&ret_str)?;
    let guild_id = read_json_str(&channel_info, "guild_id");

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/guilds/{guild_id}/members/{user_id}"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let member: serde_json::Value = serde_json::from_str(&ret_str)?;
    crate::cqapi::cq_add_log(format!("member:{:?}", member).as_str()).unwrap();
    let card = read_json_str(&member, "nick");
    let user = read_json_obj_or_null(&member, "user");
    let nickname = read_json_str(&user, "username");
    let avatar = read_json_str(&user, "avatar");
    let roles_arr = member.get("roles").ok_or("No roles")?.as_array().ok_or("roles not arr")?;
    let roles_arr_t = roles_arr.iter().map(|x|x.as_str().unwrap_or_default()).collect::<Vec<&str>>();
    let mut roles = "member";
    if roles_arr_t.contains(&"4"){
        roles = "owner";
    }else if roles_arr_t.contains(&"2") || roles_arr_t.contains(&"5") {
        roles = "admin";
    }
    let join_time = chrono::DateTime::parse_from_rfc3339(&read_json_str(&member, "joined_at"))?.timestamp();
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "group_id":group_id,
            "user_id":user_id,
            "groups_id":guild_id,
            "nickname":nickname,
            "card":card,
            "join_time":join_time,
            "avatar":avatar,
            "role":roles
        }
    }));

}


async fn set_group_ban(self_t:&QQGuildPublicConnect,json:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let mut user_id = read_json_str(&params, "user_id");

    let mut duration = read_json_str(&params, "duration");
    if duration == "" {
        duration = "1800".to_owned();
    }

    if user_id == self_t.appid {
        user_id = self_t.bot_id.read().unwrap().to_owned();
    }

    let group_id = read_json_str(&params, "group_id");


    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{}",group_id))?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let channel_info: serde_json::Value = serde_json::from_str(&ret_str)?;
    let guild_id = read_json_str(&channel_info, "guild_id");

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/{guild_id}/members/{user_id}/mute"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.patch(uri).body(serde_json::json!({
        "mute_seconds":duration
    }).to_string()).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
    //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
    let ret = client.execute(req).await?;
    if ret.status() != 204 {
        let code = ret.status().as_u16();
        let ret_str =  ret.text().await?; 
        cq_add_log_w(&format!("set_group_ban:{:?}", &ret_str)).unwrap();
        return Ok(serde_json::json!({
            "retcode":code + 1000,
            "status":"failed",
            "data":{
            }
        }));
    }else{
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
            }
        }));
    }
        
    

}

pub fn qq_content_to_cqstr(bot_id:std::sync::Weak<std::sync::RwLock<String>>,self_id:&str,qqstr:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let cqstr = qqstr.chars().collect::<Vec<char>>();
    let mut out_str = String::new();
    
    let mut text = "".to_owned();
    let mut stat = 0;
    let mut i = 0usize;
    while i < cqstr.len() {
        let cur_ch = cqstr[i];
        if stat == 0 {
            if cur_ch == '<' {
                stat = 1;
                out_str += &cq_text_encode(&text);
                text.clear();
                text.push(cur_ch);
                i += 1;
            }else if cur_ch == '&' {
                let t;
                if i+5 < cqstr.len(){
                    t = &cqstr[i..i+5];
                }else if i + 4 < cqstr.len(){
                    t = &cqstr[i..i+4];
                }else{
                    t =  &cqstr[i..i];
                }
                if t.starts_with(&['&','g','t',';']) {
                    text.push('>');
                    i += 4;
                }else if t.starts_with(&['&','l','t',';']) {
                    text.push('<');
                    i += 4;
                }else if t.starts_with(&['&','a','m','p',';']) {
                    text.push('&');
                    i += 5;
                }
                else{
                    text.push('&');
                    i += 1;
                }
            }else{
                text.push(cur_ch);
                i += 1;
            }
        }else{
            if cur_ch == '>' {
                stat = 0;
                text += ">";

                if text.starts_with("<@!"){
                    let user_id = text.get(3..text.len()-1).ok_or("get at id error1")?;
                    let bot_id = bot_id.upgrade().ok_or("get bot id error")?.read().unwrap().to_owned();
                    if bot_id == user_id {
                        out_str += &format!("[CQ:at,qq={}]",cq_params_encode(self_id));
                    }else{
                        out_str += &format!("[CQ:at,qq={}]",cq_params_encode(user_id));
                    }
                    
                }else if text.starts_with("<@"){
                    let user_id = text.get(2..text.len()-1).ok_or("get at id error1")?;
                    let bot_id = bot_id.upgrade().ok_or("get bot id error")?.read().unwrap().to_owned();
                    if bot_id == user_id {
                        out_str += &format!("[CQ:at,qq={}]",cq_params_encode(self_id));
                    }else{
                        out_str += &format!("[CQ:at,qq={}]",cq_params_encode(user_id));
                    }
                }else if text.starts_with("<emoji:"){
                    let face_id = text.get(7..text.len()-1).ok_or("get face id error")?;
                    out_str += &format!("[CQ:face,id={}]",cq_params_encode(face_id));
                }
                text.clear();
                i += 1;
            }else{
                i += 1;
                text.push(cur_ch);
            }
        }
    }
    if text.len() != 0 {
        out_str += &cq_text_encode(&text);
    }
    Ok(out_str)
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
        self.appid = config_json.get("AppID").ok_or("qqguild_public AppID格式错误:没有AppID")?.as_str().ok_or("qqguild_public AppID格式错误:AppID不是字符串")?.to_owned();
        self.appsecret = config_json.get("AppSecret").ok_or("qqguild_public AppSecret格式错误:没有AppSecret")?.as_str().ok_or("qqguild_public AppSecret格式错误:AppSecret不是字符串")?.to_owned();
        self.token = config_json.get("Token").ok_or("qqguild_public Token格式错误:没有Token")?.as_str().ok_or("qqguild_public Token格式错误:Token不是字符串")?.to_owned();
        let withgroup = config_json.get("qq_withgroup").ok_or("qqguild_public url格式错误:没有 qq_withgroup")?.as_bool().ok_or("qqguild_public url格式错误:qq_withgroup不是bool")?;
        let access_token_struct = token_refresh(&self.appid,&self.appsecret).await?;
        (*self.access_token.write().unwrap()) = access_token_struct.access_token.to_owned();

        let ws_url = get_gateway(&access_token_struct.access_token,&self.appid).await?;
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
        let socket = TcpStream::connect(addr).await.unwrap();
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
        let appid = self.appid.clone();
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
        let user_guild_dms_map = Arc::<std::sync::RwLock<HashMap<String,String>>>::downgrade(&self.user_guild_dms_map);
        let id_event_map = Arc::<std::sync::RwLock<HashMap<String,(u64,serde_json::Value)>>>::downgrade(&self.id_event_map);
        let appid = self.appid.clone();
        let bot_id = Arc::<std::sync::RwLock<String>>::downgrade(&self.bot_id);
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
                            let appid_t = appid.clone();
                            let user_guild_dms_map = user_guild_dms_map.clone();
                            let id_event_map = id_event_map.clone();
                            let bot_id = bot_id.clone();
                            // 处理事件
                            tokio::spawn(async move {
                                if let Err(e) = conv_event(bot_id,&appid_t,json_dat,user_guild_dms_map,id_event_map).await {
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


    fn get_url(&self) -> String {
        return self.url.clone();
    }

    async fn call_api(&self,_platform:&str,_self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");
        if action == "send_group_msg" {
            return send_group_msg(self,json,passive_id).await;
        }
        else if action == "send_private_msg" {
            return send_private_msg(self,json,passive_id).await;
        }
        else if action == "get_login_info" {
            return get_login_info(self).await;
        }
        else if action == "get_group_list" {
            return get_group_list(self,json,passive_id).await;
        }
        else if action == "get_group_member_info" {
            return get_group_member_info(self,json,passive_id).await;
        }
        else if action == "get_stranger_info" {
            return get_stranger_info(self,json,passive_id).await;
        }
        else if action == "delete_msg" {
            return delete_msg(self,json).await;
        }
        else if action == "set_group_ban" {
            return set_group_ban(self,json).await;
        }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String,String)> {
        return vec![("qqguild_public".to_owned(),self.appid.to_owned())];
    }
}