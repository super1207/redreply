use std::{sync::{atomic::AtomicBool, Arc, RwLock}, str::FromStr, collections::HashMap};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite, connect_async};

use crate::{cqapi::cq_add_log_w, mytool::{read_json_str, read_json_obj, read_json_obj_or_null, cq_text_encode, cq_params_encode, str_msg_to_arr}};

use super::BotConnectTrait;

#[derive(Debug)]
pub struct Satoriv1Connect {
    pub url:String,
    pub http_url:String,
    pub token:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub platforms:Arc<std::sync::RwLock<Vec<(String,String)>>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx :Option<tokio::sync::mpsc::Sender<bool>>,
    pub user_channel_map:Arc<std::sync::RwLock<std::collections::HashMap<String,String>>>,
    pub group_groups_map:Arc<std::sync::RwLock<std::collections::HashMap<String,String>>>,
}


async fn http_post(url:&str,platform:&str,self_id:&str,token:&str,json_data:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let uri = reqwest::Url::from_str(url)?;
    client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Platform")?, reqwest::header::HeaderValue::from_str(platform)?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("X-Self-ID")?, reqwest::header::HeaderValue::from_str(self_id)?);
    req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    if token != "" {
        req.headers_mut().append(reqwest::header::HeaderName::from_str("Authorization")?, reqwest::header::HeaderValue::from_str(&format!("Bearer {}",token))?);
    }
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    crate::cqapi::cq_add_log(&format!("接收数据:{ret_str}")).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    return Ok(json_val);
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
    
    return Some(json_dat);
}

struct SatoriNode {
    pub tag_name:String,
    pub attr_map:HashMap<String,String>,
    pub content:String
}

fn pre_parse_satori_msg(html:&str) -> Result<Vec<SatoriNode>,Box<dyn std::error::Error + Send + Sync>> {
    let mut to_ret = vec![];
    let mut deep = 0;
    for Ok(token) in html5gum::Tokenizer::new(html) {
        match token {
            html5gum::Token::StartTag(tag) => {
                let name = tag.name.0;
                    let mut mp = HashMap::new();
                    for (k,v) in tag.attributes {
                        mp.insert(String::from_utf8(k.0)?, String::from_utf8(v.0.clone())?);
                    }
                    let to_add = SatoriNode {
                        tag_name: String::from_utf8(name)?,
                        attr_map: mp,
                        content: "".to_owned()
                    };
                    to_ret.push(to_add);
                if !tag.self_closing {
                    deep += 1;
                }
            }
            html5gum::Token::String(text) => {
                if deep == 0 {
                    to_ret.push({
                        SatoriNode{
                            tag_name: "text".to_owned(),
                            attr_map: HashMap::new(),
                            content: String::from_utf8(text.0.clone())?,
                        }
                    });
                }
            }
            html5gum::Token::EndTag(_tag) => {
                deep -= 1;
            }
            _ => {

            },
        }
    }
    Ok(to_ret)
}


impl Satoriv1Connect {
    pub fn build(url:&str) -> Self {
        Satoriv1Connect {
            url:url.to_owned(),
            http_url:"".to_owned(),
            token:"".to_owned(),
            tx:None,
            platforms:Arc::new(RwLock::new(Vec::new())),
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            user_channel_map:Arc::new(RwLock::new(std::collections::HashMap::new())),
            group_groups_map:Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    fn satori_msg_to_cq_msg(html:&str,channel_id:&str,platform:&str) -> Result<String,Box<dyn std::error::Error + Send + Sync>> {
        let mut out = String::new();
        let childen = pre_parse_satori_msg(html)?;

        for child in childen {
            if child.tag_name == "at" {
                
                if let Some(tp) = child.attr_map.get("type") {
                    let tp_t = tp;
                    if tp_t == "all" {
                        out += "[CQ:at,qq=all]";
                    }
                } else {
                    let id_str = child.attr_map.get("id").ok_or("No id at at element")?;
                    let id = html_escape::decode_html_entities(&id_str);
                    out += &format!("[CQ:at,qq={}]", cq_params_encode(&id));
                }
            } else if child.tag_name == "img" || child.tag_name == "image" {
                let img_str = child.attr_map.get("src").ok_or("No src at img element")?;
                let img = html_escape::decode_html_entities(&img_str);
                let cq_img =  cq_params_encode(&img);
                out += &format!("[CQ:image,file={cq_img},url={cq_img}]");
            }
            else if child.tag_name == "quote" {
                let id_str = child.attr_map.get("id").ok_or("No id at quote element")?;
                let id = html_escape::decode_html_entities(&id_str);
                let cq_id =  cq_params_encode(&id);
                let channel_id_t = cq_params_encode(&channel_id);
                let cq_id_t = format!("{cq_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id_t}");
                out += &format!("[CQ:reply,id={cq_id_t}]");
            }
            else if child.tag_name == "text" {
                let text_str = child.content;
                let text = html_escape::decode_html_entities(&text_str);
                out += &cq_text_encode(&text);
            }
            else if child.tag_name == "face" {
                if platform == "llonebot" {
                    let face_id = child.attr_map.get("id").ok_or("No id at llonebot face element")?;
                    let face_id_t = cq_params_encode(&face_id);
                    out += &format!("[CQ:face,id={face_id_t}]");
                }
            }
        }
        Ok(out)
    }

    fn cq_msg_to_satori(js_arr:&serde_json::Value,platform:&str) -> Result<String,Box<dyn std::error::Error + Send + Sync>> {
        // println!("js_arr:{:?}", js_arr);
        let arr = js_arr.as_array().ok_or("js_arr not an err")?;
        let mut out = String::new();
        for it in arr {
            let tp = it.get("type").ok_or("type not found")?;
            if tp == "text" {
                let text = it.get("data").ok_or("data not found")?.get("text").ok_or("text not found")?.as_str().ok_or("text not a string")?;
                out += &html_escape::encode_double_quoted_attribute(text);
            } else if tp == "at" {
                let qq = it.get("data").ok_or("data not found")?.get("qq").ok_or("qq not found")?.as_str().ok_or("qq not a string")?;
                if qq == "all" {
                    out += "<at type=\"all\" />"
                }else {
                    out += &format!("<at id={} />", serde_json::json!(qq));
                }
            }
            else if tp == "image" {
                let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    out += &format!("<img src={} />", serde_json::json!(file));
                }else if file.starts_with("base64://") {
                    let b64 = file.split_at(9).1;
                    out += &format!("<img src={} />", serde_json::json!("data:image/png;base64,".to_owned() + b64));
                }
            }
            else if tp == "reply" {
                let id = it.get("data").ok_or("data not found")?.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?;
                let t = id.split("b73d7536-d8fa-4dda-b194-4acc51898a91").collect::<Vec<&str>>();
                let real_id = t.get(0).ok_or("can't get message_id")?;
                out += &format!("<quote id={} />", serde_json::json!(real_id));
            }
            else if tp == "record" {
                let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
                if file.starts_with("http://") ||  file.starts_with("https://") {
                    out += &format!("<audio src={} />", serde_json::json!(file));
                }else if file.starts_with("base64://") {
                    let b64 = file.split_at(9).1;
                    out += &format!("<audio src={} />", serde_json::json!("data:audio/mp3;base64,".to_owned() + b64));
                }
            }
            else if tp == "poke" {
                if platform == "chronocat" {
                    let id = it.get("data").ok_or("data not found")?.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?;
                    out += &format!("<chronocat:poke user-id={} />", serde_json::json!(id));
                }
                
            }
            else if tp == "face" {
                if platform == "llonebot" {
                    let id: &str = it.get("data").ok_or("data not found")?.get("id").ok_or("id not found")?.as_str().ok_or("id not a string")?;
                    out += &format!("<face id={} />", serde_json::json!(id));
                }
            }
        }
        Ok(out)
    }


    async fn conv_event(json_data:serde_json::Value,platforms:std::sync::Weak<std::sync::RwLock<Vec<(String,String)>>>,user_channel_map:std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,String>>>,group_groups_map:std::sync::Weak<std::sync::RwLock<std::collections::HashMap<String,String>>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let op = read_json_str(&json_data, "op");
        if op != "2"{
            crate::cqapi::cq_add_log(format!("SATORI收到数据:{}", json_data.to_string()).as_str()).unwrap();
        }
        if op == "2"{
            // 心跳回复
        }else if op == "4"{
            let platforms_t = platforms.upgrade().ok_or("upgrade platforms失败")?;
            let body = json_data.get("body").ok_or("body不存在")?;
            let logins = body.get("logins").ok_or("logins不存在")?.as_array().ok_or("logins不是数组")?;
            let mut lk = platforms_t.write().unwrap();
            lk.clear();
            for item in logins {
                let platform = read_json_str(item, "platform");
                let self_id = item["user"]["id"].as_str().ok_or("login中不存在user-id字段")?;
                cq_add_log_w(&format!("add account:`{}`:`{}`",platform,self_id)).unwrap();
                lk.push((platform,self_id.to_owned()));
            }
        }else if op == "0" {
            // 业务
            let body = json_data.get("body").ok_or("body不存在")?;
            let type_t = read_json_str(body, "type");
            if type_t == "login-removed" || type_t == "login-added" || type_t == "login-updated" {
                let login = body.get("login").ok_or("login 不存在")?;
                let self_id = read_json_str(login, "self_id");
                let platform = read_json_str(login, "platform");
                if self_id == "" || platform == "" {
                    return Ok(());
                }
                let platforms_t = platforms.upgrade().ok_or("upgrade platforms失败")?;
                let mut index = usize::MAX;
                let mut lk = platforms_t.write().unwrap();
                for i in 0..lk.len() {
                    if lk[i].0 == platform && lk[i].1 == self_id {
                        index = i;
                        break;
                    }
                }
                if type_t == "login-removed" {
                    if index != usize::MAX {
                        lk.remove(index);
                        cq_add_log_w(&format!("remove account:`{}`:`{}`",platform,self_id)).unwrap();
                    }
                }else{
                    if index == usize::MAX {
                        cq_add_log_w(&format!("add account:`{}`:`{}`",platform,self_id)).unwrap();
                        lk.push((self_id,platform));
                    }
                }
            }else if type_t == "message-created" {
                let guild_opt = read_json_obj(body, "guild");
                let tm = body.get("timestamp").ok_or("timestamp 不存在")?.as_u64().ok_or("timestamp不是数字")? / 1000;
                let self_id = read_json_str(body, "self_id");
                let platform = read_json_str(body, "platform");
                let message = read_json_obj(body, "message").ok_or("message 不存在")?; // 没有message算什么消息
                let message_id = read_json_str(message, "id");
                let user = read_json_obj_or_null(body, "user"); // 可以没有发送者
                let user_id = read_json_str(&user, "id");
                let nickname =  read_json_str(&user, "name");
                let content = read_json_str(message, "content");
                let channel = body.get("channel").ok_or("channel 不存在")?; // 没有channel就无法回复
                let channel_id =read_json_str(channel, "id");
                let cq_msg = Self::satori_msg_to_cq_msg(&content,&channel_id,&platform)?;
                if guild_opt.is_some(){ //group
                    if user_id == self_id { //机器人自己的消息，忽略
                        return Ok(());
                    }
                    let guild = guild_opt.unwrap();
                    let guild_id = read_json_str(guild, "id");
                    let member = read_json_obj_or_null(body, "member"); // 可以没有member
                    let card =  read_json_str(&member, "nick");
                    let key = format!("{platform} {self_id} {channel_id}");
                    group_groups_map.upgrade().ok_or("upgrade group_groups_map失败")?.write().unwrap().insert(key,guild_id.to_owned());
                    if content.starts_with("<chronocat:poke") {
                        // chronocat的戳一戳事件
                        let childen = pre_parse_satori_msg(&content)?;
                        for child in childen {
                            if child.tag_name == "chronocat:poke" {
                                let id_str = child.attr_map.get("user-id").ok_or("No user-id at chronocat:poke element")?;
                                let id = html_escape::decode_html_entities(&id_str);
                                let oid_str = child.attr_map.get("operator-id").ok_or("No operator-id at chronocat:poke element")?;
                                let oid = html_escape::decode_html_entities(&oid_str);
                                let event_json = serde_json::json!({
                                    "time":tm,
                                    "self_id":self_id,
                                    "platform":platform,
                                    "post_type":"notice",
                                    "message_id":format!("{message_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id}"), // 这里仍然加入msg_id，以方便进行回复
                                    "notice_type":"notify",
                                    "sub_type":"poke",
                                    "group_id":channel_id,
                                    "groups_id":guild_id,
                                    "user_id":oid,
                                    "target_id":id,
                                });
                                tokio::task::spawn_blocking(move ||{
                                    if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                                        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                                    }
                                });
                            }
                        }
                    } else {
                        // 普通群聊
                        let event_json = serde_json::json!({
                            "time":tm,
                            "self_id":self_id,
                            "platform":platform,
                            "post_type":"message",
                            "message_type":"group",
                            "sub_type":"normal",
                            "message_id":format!("{message_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id}"),
                            "group_id":channel_id,
                            "groups_id":guild_id,
                            "user_id":user_id,
                            "message":cq_msg,
                            "raw_message":content,
                            "font":0,
                            "sender":{
                                "user_id":user_id,
                                "nickname":nickname,
                                "card":card,
                                "sex":"unknown",
                                "age":0,
                                "area":"",
                                "level":"0",
                                "role":"member",
                                "title":""
                            }
                        });
                        tokio::task::spawn_blocking(move ||{
                            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                            }
                        });
                    }
                }else { //private
                    let key = format!("{platform} {self_id} {user_id}");
                    user_channel_map.upgrade().ok_or("upgrade user_channel_map失败")?.write().unwrap().insert(key,channel_id.clone());
                    let event_json = serde_json::json!({
                        "time":tm,
                        "self_id":self_id,
                        "platform":platform,
                        "post_type":"message",
                        "message_type":"private",
                        "sub_type":"friend",
                        "message_id":format!("{message_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id}"),
                        "user_id":user_id,
                        "message":cq_msg,
                        "raw_message":content,
                        "font":0,
                        "sender":{
                            "user_id":user_id,
                            "nickname":nickname,
                            "sex":"unknown",
                            "age":0,
                        }
                    });
                    tokio::task::spawn_blocking(move ||{
                        if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                        }
                    });
                }
            } else if type_t == "guild-member-added" {
                let tm = body.get("timestamp").ok_or("timestamp 不存在")?.as_u64().ok_or("timestamp不是数字")? / 1000;
                let self_id = read_json_str(body, "self_id");
                let platform = read_json_str(body, "platform");
                let guild = read_json_obj(body, "guild").ok_or("guild 不存在")?;
                let guild_id = read_json_str(guild, "id");
                let user = read_json_obj_or_null(body, "user");
                let user_id = read_json_str(&user, "id");

                let event_json = serde_json::json!({
                    "time":tm,
                    "self_id":self_id,
                    "platform":platform,
                    "post_type":"notice",
                    "notice_type":"group_increase",
                    "sub_type":"approve",
                    "groups_id":guild_id,
                    "user_id":user_id
                });

                tokio::task::spawn_blocking(move ||{
                    if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                    }
                });
            } else if type_t == "guild-member-removed" {
                let tm = body.get("timestamp").ok_or("timestamp 不存在")?.as_u64().ok_or("timestamp不是数字")? / 1000;
                let self_id = read_json_str(body, "self_id");
                let platform = read_json_str(body, "platform");
                let guild = read_json_obj(body, "guild").ok_or("guild 不存在")?;
                let guild_id = read_json_str(guild, "id");
                let user = read_json_obj_or_null(body, "user");
                let user_id = read_json_str(&user, "id");

                let mut sub_type = "leave".to_owned();
                let mut operator_id = user_id.to_owned();
                if user_id == self_id {
                    sub_type = "kick_me".to_owned();
                    operator_id = "".to_owned();
                }

                let event_json = serde_json::json!({
                    "time":tm,
                    "self_id":self_id,
                    "platform":platform,
                    "post_type":"notice",
                    "sub_type":sub_type,
                    "notice_type":"group_decrease",
                    "groups_id":guild_id,
                    "user_id":user_id,
                    "operator_id":operator_id
                });

                tokio::task::spawn_blocking(move ||{
                    if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                    }
                });
            }  
            else if type_t == "message-deleted" {
                let tm = body.get("timestamp").ok_or("timestamp 不存在")?.as_u64().ok_or("timestamp不是数字")? / 1000;
                let self_id = read_json_str(body, "self_id");
                let platform = read_json_str(body, "platform");
                let user = body.get("user").ok_or("user 不存在")?;
                let user_id = read_json_str(&user, "id");
                let channel = body.get("channel").ok_or("channel 不存在")?;
                let channel_id =read_json_str(channel, "id");
                let operator = read_json_obj_or_null(body, "operator");
                let mut operator_id =read_json_str(&operator, "id");
                if operator_id == "" {
                    operator_id = user_id.to_owned();
                }
                let message = body.get("message").ok_or("message 不存在")?;
                let raw_message_id = read_json_str(message, "id");
                let message_id = format!("{raw_message_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id}");


                if let Some(guild) = read_json_obj(body, "guild") {
                    // 群消息撤回
                    let guild_id = read_json_str(guild, "id");
                    let event_json = serde_json::json!({
                        "time":tm,
                        "self_id":self_id,
                        "platform":platform,
                        "post_type":"notice",
                        "notice_type":"group_recall",
                        "groups_id":guild_id,
                        "group_id":channel_id,
                        "user_id":user_id,
                        "operator_id":operator_id,
                        "message_id":message_id
                    });
                    tokio::task::spawn_blocking(move ||{
                        if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                        }
                    });
                } else {
                    // 私聊消息撤回
                    // 更新 user 和 channel 的对应关系
                    let key = format!("{platform} {self_id} {user_id}");
                    user_channel_map.upgrade().ok_or("upgrade user_channel_map失败")?.write().unwrap().insert(key,channel_id.clone());
                    let event_json = serde_json::json!({
                        "time":tm,
                        "self_id":self_id,
                        "platform":platform,
                        "post_type":"notice",
                        "notice_type":"friend_recall",
                        "user_id":user_id,
                        "message_id":message_id
                    });
                    tokio::task::spawn_blocking(move ||{
                        if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                        }
                    });
                }
            }
        }
        Ok(())
    }

    async fn send_group_msg(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let params = read_json_obj_or_null(json, "params");
            
        let group_id = read_json_str(&params, "group_id");
        let message = params.get("message").ok_or("message is not exist")?;
        let to_send;
        if message.is_array() {
            let mut satori_content = Self::cq_msg_to_satori(message,platform)?;
            if passive_id != "" {
                satori_content = format!("<passive id={} />{}", serde_json::json!(passive_id),satori_content)
            }
            to_send = serde_json::json!({
                "channel_id":group_id,
                "content":satori_content
            });
            
        }else{
            
            let msg_arr_rst = str_msg_to_arr(message);
            if let Ok(msg_arr) = msg_arr_rst {
                let mut satori_content = Self::cq_msg_to_satori(&msg_arr,platform)?;
                if passive_id != "" {
                    satori_content = format!("<passive id={} />{}", serde_json::json!(passive_id),satori_content)
                }
                to_send = serde_json::json!({
                    "channel_id":group_id,
                    "content":satori_content
                });
            }else{
                return None.ok_or("call str_msg_to_arr err")?;
            }
            
        }
        
        // 处理日志
        {
            let js_str = to_send.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}", js_str).as_str()).unwrap();
            }
        }

        let ret = http_post(&format!("{}/message.create",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;
        let msg_id = ret[0]["id"].as_str().ok_or("id is not str")?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":format!("{msg_id}b73d7536-d8fa-4dda-b194-4acc51898a91{group_id}")
            }
        }));
    }
    async fn send_private_msg(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let params = read_json_obj_or_null(json, "params");
        let user_id = read_json_str(&params, "user_id");
        let key = format!("{platform} {self_id} {user_id}");
        let channel_id = self_t.user_channel_map.read().unwrap().get(&key).ok_or("user_id not match any channel")?.to_owned();
        let message = params.get("message").ok_or("message is not exist")?;
        let to_send;
        if message.is_array() {
            let mut satori_content = Self::cq_msg_to_satori(message,platform)?;
            if passive_id != "" {
                satori_content = format!("<passive id={} />{}", serde_json::json!(passive_id),satori_content)
            }
            to_send = serde_json::json!({
                "channel_id":channel_id,
                "content":satori_content
            });
            
        }else{
            
            let msg_arr_rst = str_msg_to_arr(message);
            if let Ok(msg_arr) = msg_arr_rst {
                let mut satori_content = Self::cq_msg_to_satori(&msg_arr,platform)?;
                if passive_id != "" {
                    satori_content = format!("<passive id={} />{}", serde_json::json!(passive_id),satori_content)
                }
                to_send = serde_json::json!({
                    "channel_id":channel_id,
                    "content":satori_content
                });
            }else{
                return None.ok_or("call str_msg_to_arr err")?;
            }
            
        }
        
        // 处理日志
        {
            let js_str = to_send.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}", js_str).as_str()).unwrap();
            }
        }

        let ret = http_post(&format!("{}/message.create",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;
        let msg_id = ret[0]["id"].as_str().ok_or("id is not str")?;
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":format!("{msg_id}b73d7536-d8fa-4dda-b194-4acc51898a91{channel_id}")
            }
        }));
   
    }
    async fn get_login_info(self_t:&Satoriv1Connect,_json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let to_send = serde_json::json!({
        });
        
        // 处理日志
        {
            let js_str = to_send.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}", js_str).as_str()).unwrap();
            }
        }

        let ret = http_post(&format!("{}/login.get",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;
        
        let user = read_json_obj_or_null(&ret, "user");
        let mut nickname = read_json_str(&user, "name");
        if nickname == "" {
            nickname = read_json_str(&user, "nick");
        }

        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "user_id":self_id,
                "nickname":nickname
            }
        }));
   
    }

    async fn get_group_list(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let params = read_json_obj_or_null(json, "params");
            
        let groups_id = read_json_str(&params, "groups_id");
        let to_send = serde_json::json!({
            "guild_id":groups_id
        });


        let mut cl = vec![];

        let ret = http_post(&format!("{}/channel.list",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;

        let channel_list = ret.get("data").ok_or("data is not exist")?.as_array().ok_or("data is not array")?;
        for channel in channel_list {
            cl.push(channel.to_owned());
        }
        let mut next = read_json_str(&ret, "next");
        while next != "" {
            let to_send = serde_json::json!({
                "guild_id":groups_id,
                "next":next
            });
            let ret = http_post(&format!("{}/channel.list",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;
            let channel_list = ret.get("data").ok_or("data is not exist")?.as_array().ok_or("data is not array")?;
            for channel in channel_list {
                cl.push(channel.to_owned());
            }
            next = read_json_str(&ret, "next");
        }
        
        let mut ret_group: Vec<serde_json::Value> = vec![];
        for channel in cl {
            let id = channel.get("id").ok_or("id is not exist")?.as_str().ok_or("id is not string")?;
            let key = format!("{platform} {self_id} {id}");
            self_t.group_groups_map.write().unwrap().insert(key,groups_id.to_owned());
            ret_group.push(serde_json::json!({
                "group_id":id,
                "group_name":read_json_str(&channel,"name")
            }));
        }

        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":ret_group
        }));
   
    }

    async fn get_group_member_info(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let params = read_json_obj_or_null(json, "params");

        let mut groups_id = read_json_str(&params, "groups_id");
        let group_id = read_json_str(&params, "group_id");
        if groups_id == "" {
            let key = format!("{platform} {self_id} {group_id}");
            groups_id = self_t.group_groups_map.read().unwrap().get(&key).ok_or("groups_id is not exist")?.to_owned();
        }
        let user_id = read_json_str(&params, "user_id");
            
        let to_send = serde_json::json!({
            "guild_id":groups_id,
            "user_id":user_id
        });


        let ret = http_post(&format!("{}/guild.member.get",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;
        let card = read_json_str(&ret, "nick");
        let user = read_json_obj_or_null(&ret, "user");
        let join_time_str = read_json_str(&ret, "joined_at");
        let mut join_time = None;
        if join_time_str != "" {
            join_time = Some(join_time_str.parse::<u64>()? / 1000);
        }
        let mut nickname = read_json_str(&user, "name");
        if nickname == "" {
            nickname = read_json_str(&user, "nick");
        }
        let mut avatar = read_json_str(&ret, "avatar");
        if avatar == "" {
            avatar = read_json_str(&user, "avatar");
        }
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "group_id":group_id,
                "user_id":user_id,
                "groups_id":groups_id,
                "nickname":nickname,
                "card":card,
                "join_time":join_time,
                "avatar":avatar,
                "role":"member"
            }
        }));
   
    }

    async fn get_stranger_info(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let params = read_json_obj_or_null(json, "params");

        let user_id = read_json_str(&params, "user_id");
            
        let to_send = serde_json::json!({
            "user_id":user_id
        });

        crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}", to_send).as_str()).unwrap();
        let user = http_post(&format!("{}/user.get",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;

        let mut nickname = read_json_str(&user, "name");
        if nickname == "" {
            nickname = read_json_str(&user, "nick");
        }
        let mut avatar = read_json_str(&user, "avatar");
        if avatar == "" {
            avatar = read_json_str(&user, "avatar");
        }
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
    async fn delete_msg(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let params = read_json_obj_or_null(json, "params");

        let new_message_id = read_json_str(&params, "message_id");

        let t = new_message_id.split("b73d7536-d8fa-4dda-b194-4acc51898a91").collect::<Vec<&str>>();
        let message_id = t.get(0).ok_or("can't get message_id")?;
        let channel_id = t.get(1).ok_or("can't get channel_id")?;
            
        let to_send = serde_json::json!({
            "channel_id":channel_id,
            "message_id":message_id,
        });

        crate::cqapi::cq_add_log(format!("发送数据(platform:{platform},self_id:{self_id}):{}", to_send).as_str()).unwrap();

        http_post(&format!("{}/message.delete",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;

        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
            }
        }));
   
    }
    async fn get_msg(self_t:&Satoriv1Connect,json:&serde_json::Value,platform:&str,self_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let params = read_json_obj_or_null(json, "params");

        let new_message_id = read_json_str(&params, "message_id");

        let t = new_message_id.split("b73d7536-d8fa-4dda-b194-4acc51898a91").collect::<Vec<&str>>();
        let message_id = t.get(0).ok_or("can't get message_id")?;
        let channel_id = t.get(1).ok_or("can't get channel_id")?;
            
        let to_send = serde_json::json!({
            "channel_id":channel_id,
            "message_id":message_id,
        });

        let satori_msg = http_post(&format!("{}/message.get",self_t.http_url),platform,self_id,&self_t.token,&to_send).await?;

        let content = satori_msg["content"].as_str().ok_or("not found content in satori message")?;

        let cq_message = Self::satori_msg_to_cq_msg(content,channel_id,platform)?;
        let created_at_str = read_json_str(&satori_msg, "created_at");
        let mut created_at = 0;
        if created_at_str != "" {
            created_at = created_at_str.parse::<u64>()? / 1000;
        }
        let guild = &satori_msg["guild"];
        let mut message_type = "group".to_owned();
        if !guild.is_object() || guild.as_object().unwrap().is_empty() {
            message_type = "private".to_owned();
        }

        let user = &satori_msg["user"];
        let sender;
        if user.is_object() && !user.as_object().unwrap().is_empty() {
            if message_type == "group" {
                sender = serde_json::json!({
                    "user_id":read_json_str(&user, "id"),
                    "nickname":read_json_str(&user, "name"),
                    "card":read_json_str(&user, "nick"),
                    "sex":"unknown",
                    "age":0,
                    "area":"",
                    "level":"0",
                    "role":"member",
                    "title":""
                });
            }else {
                let nick = read_json_str(&user, "nick");
                let name = read_json_str(&user, "name");
                let nickname;
                if nick != "" {
                    nickname = nick;
                }else {
                    nickname = name;
                }
                sender = serde_json::json!({
                    "user_id":read_json_str(&user, "id"),
                    "nickname":nickname,
                    "sex":"unknown",
                    "age":0,
                });
            }
        }else {
            sender = serde_json::Value::Null;
        }
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "time":created_at,
                "message_type":message_type,
                "message_id":new_message_id,
                "sender":sender,
                "message":cq_message
            }
        }));
   
    }
}


#[async_trait]
impl BotConnectTrait for Satoriv1Connect {

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

        //println!("正在连接satori：{}",self.url);
        let config_json_str = self.url.get(9..).ok_or("satori url格式错误")?;
        let config_json:serde_json::Value =  serde_json::from_str(config_json_str)?;
        let url_ws = config_json.get("uri").ok_or("satori url格式错误:没有uri")?.as_str().ok_or("satori url格式错误:uri不是字符串")?;
        let is_ssl = config_json.get("use_tls").ok_or("satori url格式错误:没有use_tls")?.as_bool().ok_or("satori url格式错误:use_tls不是bool")?;
        self.token = config_json.get("token").ok_or("satori url格式错误:没有token")?.as_str().ok_or("satori url格式错误:token不是字符串")?.to_owned();
        let ws_url;
        if is_ssl {
            ws_url = format!("wss://{url_ws}/events");
            self.http_url = format!("https://{url_ws}");
        }else {
            ws_url = format!("ws://{url_ws}/events");
            self.http_url = format!("http://{url_ws}");
        }
        let request = tungstenite::client::IntoClientRequest::into_client_request(&ws_url)?;
        let ws_rst;
        if is_ssl {
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
        }else {
            ws_rst = connect_async(request).await?;
        }

        let (mut write_half,mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = ws_url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        tx_ay_t.send(serde_json::json!({
            "op":3,
            "body":{
                "token":self.token
            }
        })).await?;

        // 这里使用弱引用，防止可能的循环依赖
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let platforms = Arc::<std::sync::RwLock<Vec<(String,String)>>>::downgrade(&self.platforms);
        let user_channel_map = Arc::<std::sync::RwLock<HashMap<String,String>>>::downgrade(&self.user_channel_map);
        let group_groups_map = Arc::<std::sync::RwLock<HashMap<String,String>>>::downgrade(&self.group_groups_map);
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
                        let platforms_t = platforms.clone();
                        let user_channel_map_t = user_channel_map.clone();
                        let group_groups_map_t = group_groups_map.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Satoriv1Connect::conv_event(json_dat,platforms_t,user_channel_map_t,group_groups_map_t).await {
                                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                            }
                        });
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
            cq_add_log_w(&format!("satori连接已经断开(read_halt):{url_str_t}")).unwrap();
        });
        let url_str_t = self.url.clone();
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
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
                    let rst = tx_ay_t.send(serde_json::json!({
                        "op":1,
                    })).await;
                    if rst.is_err() {
                        break;
                    }
                }
                // 移除conn
                if let Some(val) = is_stop.upgrade() {
                    val.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                cq_add_log_w(&format!("satori心跳已断开:{url_str2}")).unwrap();
            });
            while let Some(msg) = rx_ay.recv().await {
                let rst = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await;
                if rst.is_err() {
                    break;
                }
            }
            // 移除conn
            if let Some(val) = is_stop2.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            cq_add_log_w(&format!("satori连接已经断开(write_half):{url_str_t}")).unwrap();
        });
        Ok(())
    }

    async fn call_api(&self,platform:&str,self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");

        let passive_id2;
        if passive_id != "" {
            let t = passive_id.split("b73d7536-d8fa-4dda-b194-4acc51898a91").collect::<Vec<&str>>();
            passive_id2 = t.get(0).ok_or("can't get passive_id")?.to_owned();
        }else {
            passive_id2 = "";
        }

        if action == "send_group_msg" {
            return Self::send_group_msg(self,json,platform,self_id,passive_id2).await;
        }
        else if action == "send_private_msg" {
            return Self::send_private_msg(self,json,platform,self_id,passive_id2).await;
        }
        else if action == "get_login_info" {
            return Self::get_login_info(self,json,platform,self_id).await;
        }
        else if action == "get_group_list" {
            return Self::get_group_list(self,json,platform,self_id).await;
        }
        else if action == "get_group_member_info" {
            return Self::get_group_member_info(self,json,platform,self_id).await;
        }
        else if action == "get_stranger_info" {
            return Self::get_stranger_info(self,json,platform,self_id).await;
        }
        else if action == "delete_msg" {
            return Self::delete_msg(self,json,platform,self_id).await;
        }
        else if action == "get_msg" {
            return Self::get_msg(self,json,platform,self_id).await;
        }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String,String)> {
        let lk = self.platforms.read().unwrap();
        let platforms = (*lk).clone();
        return platforms;
    }
}