// 函数调用判定：
// 有群ID   ->  send_group_msg
// 无群ID但有user_id   ->  send_private_msg
// 否则  ->  不处理

// 消息目标判定（QQ频道）（暂未完全实现）：
//     send_group_msg:根据群ID反查群组ID:
//         能查到：qq频道
//             有msg_id且根据msg_id反查出的群ID与参数中的群ID:
//                 一致：被动频道消息
//                 不一致：主动频道消息
//                 无msg_id:主动频道消息
//         查不到：qq群
//                 有msg_id且根据msg_id反查出的群ID与参数中的群ID:
//                 一致：被动群消息
//                 不一致：主动群消息
//                 无msg_id:主动群消息
//     send_private_msg:
//         有msg_id：
//             反查guild_id:
//                 能查到:
//                     发送频道私聊被动
//                 否则：
//                     不处理
//         无msg_id：
//             不处理(暂时不支持主动创建私聊会话)

use std::{str::FromStr, sync::Weak, time::SystemTime};

use base64::{alphabet, engine::{self, general_purpose}, Engine};

use crate::{mytool::{read_json_obj_or_null, read_json_str, read_json_or_default, cq_text_encode, cq_params_encode, str_msg_to_arr}, cqapi::cq_add_log_w};

const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

#[derive(PartialEq)]
#[allow(dead_code)]
pub enum MsgSrcType {
    GuildPub,GroupPub,GuildPri,GroupPri
}

#[derive(PartialEq)]
#[allow(dead_code)]
pub enum MsgTargetType {
    GuildZd,GuildBd,GroupZd,GroupBd,GroupPriBd,GroupPriZd,GuildPriBd,GuildPriZd
}

#[derive(Clone)]
pub struct SelfData {
    pub appid:Weak<std::sync::RwLock<String>>,
    pub access_token:Weak<std::sync::RwLock<String>>,
    pub id_event_map:Weak<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>,
    pub bot_id:Weak<std::sync::RwLock<String>>
}
 

pub struct QQMsgNode{
    pub content:String,
    pub imgs:Vec<Vec<u8>>,
    pub img_infos:Vec<String>,
    pub message_reference:Option<String>,
    pub markdown:Option<serde_json::Value>,
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


pub fn get_msg_type(self_t:&SelfData,params:&serde_json::Value,passive_id:&str) -> Result<MsgTargetType, Box<dyn std::error::Error + Send + Sync>> {
    let id_event_map_t = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?;
    let group_id_p = read_json_str(&params, "group_id");
    let lk = id_event_map_t.read().unwrap();
    // 判断是否是群消息
    let mut is_group = true;
    for (_key,(_tm,event)) in &*lk {
        let d = read_json_or_default(event, "d",&serde_json::Value::Null);
        let channel_id = read_json_str(d, "channel_id");
        if group_id_p == channel_id {
            let guild_id = read_json_str(d, "guild_id");
            if guild_id != "" {
                is_group = false;
            }
        }
    }

    // 判断主动被动
    if is_group {
        if passive_id == "" {
            return Ok(MsgTargetType::GroupZd);
        }
        if let Some((_tm,event)) = lk.get(passive_id) {
            let d = read_json_or_default(event, "d",&serde_json::Value::Null);
            let group_id = read_json_str(d, "group_openid");
            if group_id == group_id_p {
                return Ok(MsgTargetType::GroupBd);
            }else{
                return Ok(MsgTargetType::GroupZd);
            }
        }else {
            return Ok(MsgTargetType::GroupZd);
        }
    }else{
        if passive_id == "" {
            return Ok(MsgTargetType::GuildZd);
        }
        if let Some((_tm,event)) = lk.get(passive_id) {
            let d = read_json_or_default(event, "d",&serde_json::Value::Null);
            let channel_id = read_json_str(d, "channel_id");
            if channel_id == group_id_p {
                return Ok(MsgTargetType::GuildBd);
            }else{
                return Ok(MsgTargetType::GuildZd);
            }
        }else{
            return Ok(MsgTargetType::GuildZd);
        }
    }
}

pub async fn cq_msg_to_qq(self_t:&SelfData,js_arr:&serde_json::Value,msg_type:MsgSrcType,group_id:&str) -> Result<QQMsgNode,Box<dyn std::error::Error + Send + Sync>> {
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
            if msg_type == MsgSrcType::GuildPub {
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
            if msg_type == MsgSrcType::GuildPri || msg_type == MsgSrcType::GuildPub {
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

                let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/files"))?;
                let client = reqwest::Client::builder().no_proxy().build()?;
                let json_data: serde_json::Value;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    json_data = serde_json::json!({
                        "file_type":1, // 图片
                        "url":file,
                        "srv_send_msg":false
                    });
                } else { // base64://
                    json_data = serde_json::json!({
                        "file_type":1, // 图片
                        "file_data":file.get(9..).ok_or("img not base64")?,
                        "srv_send_msg":false,
                    });
                }
                let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",&self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
                let ret = client.execute(req).await?;
                let ret_str =  ret.text().await?; 
                let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
                crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
                msg_node.img_infos.push(json_val.get("file_info").ok_or("file_info not found")?.as_str().ok_or("file_info not a string")?.to_owned());
            }
            
        } else if tp == "face" {
            if msg_type != MsgSrcType::GroupPub {
                let face_id = it.get("data").ok_or("data not found")?.get("id").ok_or("face id not found")?.as_str().ok_or("face id not a string")?;
                msg_node.content += &format!("<emoji:{}>", make_qq_text(face_id));
            }
        }
        else if tp == "reply" {
            if msg_type == MsgSrcType::GuildPri || msg_type == MsgSrcType::GuildPub {
                let reply_id = it.get("data").ok_or("data not found")?.get("id").ok_or("reply_id not found")?.as_str().ok_or("reply_id not a string")?;
                let lk_arc = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?;
                let lk = lk_arc.read().unwrap();
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
        else if tp == "record" { // only qq group
                let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
                let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/files"))?;
                let json_data: serde_json::Value;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    let client = reqwest::Client::builder().no_proxy().build()?;
                    let req = client.get(file).build()?;
                    let ret = client.execute(req).await?;
                    let retbin = ret.bytes().await?.to_vec();
                    let ret_silk = crate::mytool::all_to_silk::all_to_silk(&retbin).map_err(|_x|{"can't convert to silk".to_owned()})?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(ret_silk);
                    json_data = serde_json::json!({
                        "file_type":3, // 语音
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                } else { // base64://
                    let retbin = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        base64::engine::general_purpose::PAD), file.get(9..).ok_or("record not base64")?)?;
                    let ret_silk = crate::mytool::all_to_silk::all_to_silk(&retbin).map_err(|_x|{"can't convert to silk".to_owned()})?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(ret_silk);
                    json_data = serde_json::json!({
                        "file_type":3, // 语音
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                }
                let client = reqwest::Client::builder().no_proxy().build()?;
                let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",&self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
                let ret = client.execute(req).await?;
                let ret_str =  ret.text().await?; 
                let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
                crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
                msg_node.img_infos.push(json_val.get("file_info").ok_or("file_info not found")?.as_str().ok_or("file_info not a string")?.to_owned());
        }
        else if tp == "video" { // only qq group
            let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
                let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/files"))?;
                let json_data: serde_json::Value;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    let client = reqwest::Client::builder().no_proxy().build()?;
                    let req = client.get(file).build()?;
                    let ret = client.execute(req).await?;
                    let retbin = ret.bytes().await?.to_vec();
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(retbin);
                    json_data = serde_json::json!({
                        "file_type":2, // 视频
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                } else { // base64://
                    let retbin = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        base64::engine::general_purpose::PAD), file.get(9..).ok_or("record not base64")?)?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(retbin);
                    json_data = serde_json::json!({
                        "file_type":2, // 视频
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                }
                let client = reqwest::Client::builder().no_proxy().build()?;
                let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",&self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
                let ret = client.execute(req).await?;
                let ret_str =  ret.text().await?; 
                let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
                crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
                msg_node.img_infos.push(json_val.get("file_info").ok_or("file_info not found")?.as_str().ok_or("file_info not a string")?.to_owned());
        }
        else if tp == "file" { // only qq group ，暂时不可用
            let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
                let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/v2/groups/{group_id}/files"))?;
                let json_data: serde_json::Value;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    let client = reqwest::Client::builder().no_proxy().build()?;
                    let req = client.get(file).build()?;
                    let ret = client.execute(req).await?;
                    let retbin = ret.bytes().await?.to_vec();
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(retbin);
                    json_data = serde_json::json!({
                        "file_type":4, // 文件
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                } else { // base64://
                    let retbin = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        base64::engine::general_purpose::PAD), file.get(9..).ok_or("record not base64")?)?;
                    let b64_str = BASE64_CUSTOM_ENGINE.encode(retbin);
                    json_data = serde_json::json!({
                        "file_type":4, // 文件
                        "file_data":b64_str,
                        "srv_send_msg":false,
                    });
                }
                let client = reqwest::Client::builder().no_proxy().build()?;
                let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",&self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
                let ret = client.execute(req).await?;
                let ret_str =  ret.text().await?; 
                let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
                crate::cqapi::cq_add_log(format!("接收qq group API数据:{}", json_val.to_string()).as_str()).unwrap();
                msg_node.img_infos.push(json_val.get("file_info").ok_or("file_info not found")?.as_str().ok_or("file_info not a string")?.to_owned());
        }
        else if tp == "qmarkdown" {
            let markdown_data = it.get("data").ok_or("data not found")?.get("data").ok_or("markdown data not found")?.as_str().ok_or("markdown data not a string")?;
            let b64_str = markdown_data.split_at(9).1;
            let markdown_buffer = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                &base64::alphabet::STANDARD,
                base64::engine::general_purpose::PAD), b64_str)?;
            let json:serde_json::Value = serde_json::from_str(&String::from_utf8(markdown_buffer)?)?;
            msg_node.markdown = Some(json);
        }
    }
    Ok(msg_node)
}



pub struct MsgIdStruct {
    pub is_event:bool,
    pub raw_ids:Vec<String>,
    pub guild_id:String,
}


pub fn get_reply_id(self_t:&SelfData,passive_id:&str) -> Result<MsgIdStruct, Box<dyn std::error::Error + Send + Sync>> {
    let mut msg_id = MsgIdStruct{
        is_event: false,
        raw_ids: vec![],
        guild_id:"".to_owned(),
    };
    if let Some((_tm,event)) = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?.read().unwrap().get(passive_id) {
        let tp = read_json_str(event, "t");
        if tp == "GUILD_MEMBER_REMOVE" || tp == "GUILD_MEMBER_ADD"{
            let to_reply_id = read_json_str(&event, "id");
            if to_reply_id != "" {
                msg_id.is_event = true;
                msg_id.raw_ids.push(to_reply_id);
                return Ok(msg_id);
            }
        }else{
            let d = read_json_or_default(event, "d",&serde_json::Value::Null);
            let to_reply_id = read_json_str(&d, "id");
            if to_reply_id != "" {
                msg_id.is_event = false;
                msg_id.raw_ids.push(to_reply_id);
                msg_id.guild_id = read_json_str(&d, "guild_id");
                return Ok(msg_id);
            }
        }
    }
    return Ok(msg_id);
}


pub struct AccessTokenStruct {
    pub access_token:String,
    _expires_in:u64,
}


pub async fn token_refresh(appid:&str,client_secret:&str) -> Result<AccessTokenStruct, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://bots.qq.com/app/getAppAccessToken")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let json_data:serde_json::Value = serde_json::json!({
        "appId":appid,
        "clientSecret":client_secret
    });
    let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
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


pub fn qq_content_to_cqstr(bot_id:&std::sync::Weak<std::sync::RwLock<String>>,self_id:&str,qqstr:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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

pub fn set_event_id(self_t:&SelfData,data:&serde_json::Value,tm:u64) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let curr_tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let event_time = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + tm;
    let event_id = uuid::Uuid::new_v4().to_string();
    let id_event_map_t = &self_t.id_event_map;
    let binding = id_event_map_t.upgrade().ok_or("id_event_map not upgrade")?;
    let mut lk =  binding.write().unwrap();
    lk.insert(event_id.to_owned(), (event_time,data.clone()));
    let mut to_remove = vec![];
    for (key,(key_tm,_)) in &*lk{
        if curr_tm > *key_tm {
            to_remove.push(key.to_owned());
        }
    }
    for key in to_remove {
        lk.remove(&key);
    }
    Ok(event_id)
}


pub fn deal_message_reference(root:&serde_json::Value,id_event_map:&std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,(u64,serde_json::Value)>>>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    
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

pub fn deal_attachments(root:&serde_json::Value) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut retstr = String::new();
    if let Some(attachments) = root.get("attachments") {
        if let Some(attachments_t) = attachments.as_array() {
            for it in attachments_t {
                if read_json_str(it, "content_type").starts_with("image/") {
                    let mut url = read_json_str(it, "url");
                    if !url.starts_with("https://") {
                        url = format!("https://{url}");
                    }
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


pub async fn get_gateway(access_token:&str,appid:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://api.sgroup.qq.com/gateway")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {access_token}"))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(appid)?);
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

pub fn get_json_dat(msg:Result<hyper_tungstenite::tungstenite::Message, hyper_tungstenite::tungstenite::Error>) -> Option<serde_json::Value> {
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
    if read_json_str(&json_dat, "op") == "11" { // 不打印心跳
        return None;
    }
    return Some(json_dat);
}


pub async fn do_qq_json_post(self_t:&SelfData,path:&str,json:serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri= reqwest::Url::from_str(&format!("https://api.sgroup.qq.com{path}"))?;
    let mut req = client.post(uri).body(reqwest::Body::from(json.to_string())).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    Ok(json_val)
}

pub fn str_msg_to_arr_safe(js:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let ret = str_msg_to_arr(js);
    if let Ok(ret) = ret {
        return Ok(ret);
    }else {
        return None.ok_or(format!("str_msg_to_arr error:{}", ret.err().unwrap()))?;
    }
}

pub async fn send_private_msg(self_t:&SelfData,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    // 获得参数
    let params = read_json_or_default(json, "params",&serde_json::Value::Null);

    // let user_id = read_json_str(params, "user_id");

    // 获得消息(数组格式)
    let mut message = params.get("message").ok_or("message is not exist")?.to_owned();
    if message.is_string() {
        message = str_msg_to_arr_safe(&message)?;
    }

    if passive_id != "" {
        let reply_id = get_reply_id(self_t,passive_id)?;
        if reply_id.raw_ids.len() > 0 {
            let to_reply_id = &reply_id.raw_ids[0];
            let mut qq_msg_node = cq_msg_to_qq(&self_t,&message,MsgSrcType::GuildPri,"").await?;

            let guild_id = reply_id.guild_id;
            let mut id = "".to_owned();
            if qq_msg_node.imgs.len() == 0 { // 没图
                let mut json_data = serde_json::json!({
                    "content":qq_msg_node.content,
                });
                qq_msg_node.content = "".to_owned();

                if reply_id.is_event {
                    json_data.as_object_mut().unwrap().insert("event_id".to_owned(), serde_json::json!(to_reply_id));
                }else{
                    json_data.as_object_mut().unwrap().insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
                }
                
                if qq_msg_node.message_reference != None {
                    json_data.as_object_mut().unwrap().insert("message_reference".to_owned(), serde_json::json!({
                        "message_id":qq_msg_node.message_reference,
                    }));
                }
                crate::cqapi::cq_add_log(format!("发送qq guild_pri API数据(`{}`):{}",guild_id,json_data.to_string()).as_str()).unwrap();
                let api_ret = do_qq_json_post(self_t,&format!("/dms/{guild_id}/messages"),json_data).await?;
                crate::cqapi::cq_add_log(format!("接收qq guild_pri API数据:{}", api_ret.to_string()).as_str()).unwrap();
                // 构造消息id
                if id != "" {
                    id += "|";
                }
                id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
            }
            else // 有图
            {
                // 再发送图片
                for img in &qq_msg_node.imgs {
                    let mut form = reqwest::multipart::Form::new().part(
                    "file_image",
                    reqwest::multipart::Part::bytes(img.clone()).file_name("pic.png"),
                    );
                    
                    if reply_id.is_event {
                        form = form.text("event_id", to_reply_id.to_owned());
                    }else{
                        form = form.text("msg_id", to_reply_id.to_owned());
                    }
                    
                    if qq_msg_node.message_reference != None {
                        // form 不支持回复
                    }
        
                    if qq_msg_node.content != "" {
                        form = form.text("content",qq_msg_node.content);
                        qq_msg_node.content = "".to_owned();
                    }
                    
                    crate::cqapi::cq_add_log(format!("发送qq guild_pri API数据(`{}`):{:?}",guild_id,form).as_str()).unwrap();
                    let api_ret = do_qq_form_post(self_t,&format!("/dms/{guild_id}/messages"),form).await?;
                    crate::cqapi::cq_add_log(format!("接收qq guild_pri API数据:{}", api_ret.to_string()).as_str()).unwrap();
                    // 构造消息id
                    if id != "" {
                        id += "|";
                    }
                    id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
                }
            }
            
            let event_id = set_event_id(self_t,&serde_json::json!({"t":"send_private_msg","d":{"id":id,"guild_id":guild_id}}),5 * 60)?;
            return Ok(serde_json::json!({
                "retcode":0,
                "status":"ok",
                "data":{
                    "message_id":event_id
                }
            }));
        }
    }

    return Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"msg_target_type not support",
        "data":{}
    }));

}

pub async fn do_qq_form_post(self_t:&SelfData,path:&str,form:reqwest::multipart::Form) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri= reqwest::Url::from_str(&format!("https://api.sgroup.qq.com{path}"))?;
    let mut req = client.post(uri.to_owned()).multipart(form).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("multipart/form-data")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    Ok(json_val)
}

pub async fn send_qqguild_msg(self_t:&SelfData,channel_id:&str,to_reply_id:&str,_passive_id:&str,mut qq_msg_node:QQMsgNode,is_event:bool) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut id = "".to_owned();
    if qq_msg_node.imgs.len() == 0 { // 没图
        let mut json_data = serde_json::json!({
            "content":qq_msg_node.content,
        });
        qq_msg_node.content = "".to_owned();
        if to_reply_id != "" {
            if is_event {
                json_data.as_object_mut().unwrap().insert("event_id".to_owned(), serde_json::json!(to_reply_id));
            }else{
                json_data.as_object_mut().unwrap().insert("msg_id".to_owned(), serde_json::json!(to_reply_id));
            }
        }
        
        if qq_msg_node.message_reference != None {
            json_data.as_object_mut().unwrap().insert("message_reference".to_owned(), serde_json::json!({
                "message_id":qq_msg_node.message_reference,
            }));
        }
        crate::cqapi::cq_add_log(format!("发送qq guild API数据(`{}`):{}",channel_id,json_data.to_string()).as_str()).unwrap();
        let api_ret = do_qq_json_post(self_t,&format!("/channels/{channel_id}/messages"),json_data).await?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", api_ret.to_string()).as_str()).unwrap();
        // 构造消息id
        if id != "" {
            id += "|";
        }
        id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
    }
    else // 有图
    {
        // 再发送图片
        for img in &qq_msg_node.imgs {
            let mut form = reqwest::multipart::Form::new().part(
            "file_image",
            reqwest::multipart::Part::bytes(img.clone()).file_name("pic.png"),
            );
            if to_reply_id != "" {
                if is_event {
                    form = form.text("event_id", to_reply_id.to_owned());
                }else{
                    form = form.text("msg_id", to_reply_id.to_owned());
                }
            }
            
            if qq_msg_node.message_reference != None {
                // form 不支持回复
            }

            if qq_msg_node.content != "" {
                form = form.text("content",qq_msg_node.content);
                qq_msg_node.content = "".to_owned();
            }
            
            crate::cqapi::cq_add_log(format!("发送qq guild API数据(`{}`):{:?}",channel_id,form).as_str()).unwrap();
            let api_ret = do_qq_form_post(self_t,&format!("/channels/{channel_id}/messages"),form).await?;
            crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", api_ret.to_string()).as_str()).unwrap();
            // 构造消息id
            if id != "" {
                id += "|";
            }
            id += &api_ret.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?.to_owned();
        }
    }
    
    let event_id = set_event_id(self_t,&serde_json::json!({"t":"send_group_msg","d":{"id":id,"channel_id":channel_id}}),5 * 60)?;
    return Ok(serde_json::json!({
        "retcode":0,
        "status":"ok",
        "data":{
            "message_id":event_id
        }
    }));

}


pub async fn get_login_info(self_t:&SelfData) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/users/@me"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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
            "user_id":*self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap(),
            "nickname":nickname,
            "avatar":avatar,
        }
    }));

}

pub async fn get_group_list(self_t:&SelfData,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");
        
    let mut groups_id = read_json_str(&params, "groups_id");

    if groups_id == ""{
        if let Some((_tm,event)) = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?.read().unwrap().get(passive_id) {
            let d = read_json_obj_or_null(event, "d");
            groups_id = read_json_str(&d, "guild_id");
        } 
    }

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/{groups_id}/channels"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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

pub async fn get_stranger_info(self_t:&SelfData,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let user_id = read_json_str(&params, "user_id");

    if user_id == *self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap() {
        return get_login_info(self_t).await;
    }

    let mut guild_id:String = "".to_owned();
    if let Some((_tm,event)) = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?.read().unwrap().get(passive_id) {
        let d = read_json_obj_or_null(event, "d");
        guild_id = read_json_str(&d, "guild_id");
    }

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/{guild_id}/members/{user_id}"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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

pub async fn delete_msg(self_t:&SelfData,json:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let message_id = read_json_str(&params, "message_id");

    let event:serde_json::Value = self_t.id_event_map.upgrade().ok_or("id_event_map not upgrade")?.read().unwrap().get(&message_id).ok_or("event is not found")?.1.to_owned();
    let tp = read_json_str(&event, "t");
    if tp == "MESSAGE_CREATE" || tp == "send_group_msg" {
        let d = read_json_obj_or_null(&event, "d");
        let channel_id = read_json_str(&d, "channel_id");
        let message_id = read_json_str(&d, "id");
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{channel_id}/messages/{message_id}?hidetip=false"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut req = client.delete(uri).build()?;
        req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
        req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
        req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
        req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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
            req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
            req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
            req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
            req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
                req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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

pub async fn get_group_member_info(self_t:&SelfData,json:&serde_json::Value,_passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let mut user_id = read_json_str(&params, "user_id");

    if user_id == *self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap() {
        user_id = self_t.bot_id.upgrade().ok_or("bot_id not upgrade")?.read().unwrap().to_owned();
    }

    let group_id = read_json_str(&params, "group_id");


    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{}",group_id))?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?; 
    let channel_info: serde_json::Value = serde_json::from_str(&ret_str)?;
    let guild_id = read_json_str(&channel_info, "guild_id");

    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/guilds/guilds/{guild_id}/members/{user_id}"))?;
    //println!("uri:{}", &uri);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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


pub async fn set_group_ban(self_t:&SelfData,json:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");

    let mut user_id = read_json_str(&params, "user_id");

    let mut duration = read_json_str(&params, "duration");
    if duration == "" {
        duration = "1800".to_owned();
    }

    if user_id == *self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap() {
        user_id = self_t.bot_id.upgrade().ok_or("bot_id not upgrade")?.read().unwrap().to_owned();
    }

    let group_id = read_json_str(&params, "group_id");


    let client = reqwest::Client::builder().no_proxy().build()?;
    let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{}",group_id))?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.upgrade().ok_or("access_token not upgrade")?.read().unwrap()))?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Union-Appid")?, reqwest::header::HeaderValue::from_str(&self_t.appid.upgrade().ok_or("appid not upgrade")?.read().unwrap())?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Accept")?, reqwest::header::HeaderValue::from_str("application/json")?);
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
