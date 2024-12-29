use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use super::BotConnectTrait;
use crate::cqapi::{cq_add_log, cq_add_log_w};
use crate::mytool::{cq_params_encode, cq_text_encode, read_json_str, str_msg_to_arr};
use crate::RT_PTR;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TeleTramConnect {
    pub self_id: Arc<RwLock<String>>,
    pub token: Arc<RwLock<String>>,
    pub url: String,
    pub proxy: Arc<RwLock<String>>,
    pub is_stop: Arc<AtomicBool>,
    msg_ids: Arc<RwLock<VecDeque<MsgIdPair>>>,
}


lazy_static! {
    static ref G_USERNAME2USERID:std::sync::RwLock<HashMap<String,i64>> = std::sync::RwLock::new(HashMap::new());
}

fn add_username2userid(username:&str,user_id:i64) {
    let mut lk = G_USERNAME2USERID.write().unwrap();
    lk.insert(username.to_owned(),user_id);
}

fn get_username2userid(username:&str) -> Option<i64> {
    let lk = G_USERNAME2USERID.read().unwrap();
    return lk.get(username).cloned();
}


#[derive(Debug, Clone)]
struct RawMsgId {
    msg_id: String,
    chat_id: String,
}

#[derive(Debug, Clone)]
struct MsgIdPair {
    msg_id: String,
    raw_msg_ids: Vec<RawMsgId>,
}




impl TeleTramConnect {
    pub fn build(url: &str) -> Self {
        return Self {
            self_id: Arc::new(RwLock::new("".to_owned())),
            token: Arc::new(RwLock::new("".to_owned())),
            url: url.to_owned(),
            proxy: Arc::new(RwLock::new("".to_owned())),
            is_stop: Arc::new(AtomicBool::new(false)),
            msg_ids: Arc::new(RwLock::new(VecDeque::new())),
        };
    }


    fn get_req_client(&self) -> Result<reqwest::Client, Box<dyn std::error::Error + Send + Sync>> {
        let proxy;
        {
            let lk = self.proxy.read().unwrap();
            proxy = lk.to_owned();
        }
        if proxy == "" {
            Ok(reqwest::Client::builder()
                .no_proxy()
                .build()?)
        } else {
            Ok(reqwest::Client::builder()
                .proxy(reqwest::Proxy::all(proxy.as_str())?)
                .build()?)
        }
    }

    async fn proxyrequest(&self, url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.get_req_client()?.get(url).send().await?.text().await?)
    }

    fn make_msg_with_at(text_message:&str,entities:&serde_json::Value) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut ret_cq_msg = "".to_owned(); 
        if let Some(ents) = entities.as_array() {
            let mut last_end = 0;
            let text = text_message.chars().collect::<Vec<char>>();
            for ent in ents {
                let tp = ent["type"].as_str().ok_or("type not str")?;
                if tp != "mention" {
                    continue;
                }
                let offset = ent["offset"].as_i64().ok_or("offset not i64")?;
                let length = ent["length"].as_i64().ok_or("length not i64")?;
                let end = offset + length;
                let text_before = &text[last_end as usize..offset as usize];
                ret_cq_msg.push_str(&cq_text_encode(&text_before.iter().collect::<String>()));
                let mention = text[offset as usize..end as usize]
                    .iter()
                    .collect::<String>();
                if let Some(userid) = get_username2userid(&mention) {
                    ret_cq_msg.push_str(&format!("[CQ:at,qq={}]", userid));
                }
                last_end = end;
            }
            let text_end = &text[last_end as usize..];
            ret_cq_msg.push_str(&cq_text_encode(&text_end.iter().collect::<String>()));
        } else {
            ret_cq_msg.push_str(&cq_text_encode(&text_message));
        }
        Ok(ret_cq_msg)
    }

    async fn to_cq_msg(
        &self,
        event: &serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut ret_cq_msg = "".to_owned();
        let message_obj = &event["message"];
        let reply_to_message = &message_obj["reply_to_message"];
        let mut reply_id = "".to_owned();
        if reply_to_message.is_object() {
            // 频道里面的自动转发消息不是回复
            let is_automatic_forward = Self::get_json_bool(reply_to_message, "is_automatic_forward");
            if !is_automatic_forward {
                reply_id = reply_to_message["message_id"]
                .as_i64()
                .ok_or("message_id not i64")?
                .to_string();
            }
        }

        let text_message = read_json_str(&message_obj, "text");
        let entities = &message_obj["entities"];
        ret_cq_msg.push_str(&Self::make_msg_with_at(&text_message,entities)?);

        let caption_message = read_json_str(&message_obj, "caption");
        let caption_entities = &message_obj["caption_entities"];
        ret_cq_msg.push_str(&Self::make_msg_with_at(&caption_message,caption_entities)?);
        
        if message_obj.get("photo").is_some() {
            let photo = message_obj["photo"].as_array().ok_or("photo not array")?;
            let photo_len = photo.len();
            let photo_file_id = photo[photo_len - 1]["file_id"]
                .as_str()
                .ok_or("file_id not str")?;
            let get_file_path_ret = self.proxyrequest(
                format!(
                    "https://api.telegram.org/bot{}/getFile?file_id={}",
                    self.token.read().unwrap(),
                    photo_file_id
                )
                .as_str(),
            )
            .await?;
            let get_file_path_json: serde_json::Value = serde_json::from_str(&get_file_path_ret)?;
            let file_path = get_file_path_json["result"]["file_path"]
                .as_str()
                .ok_or("file_path not str")?;
            let file_url = format!(
                "https://api.telegram.org/file/bot{}/{}",
                self.token.read().unwrap(),
                file_path
            );
            ret_cq_msg.push_str(&format!("[CQ:image,file={}]", &cq_params_encode(&file_url)));
        }
        if reply_id != "" {
            ret_cq_msg = format!("[CQ:reply,id={}]{}", reply_id, ret_cq_msg);
        }
        Ok(ret_cq_msg)
    }

    async fn deal_private_msg(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let chat = &event["message"]["chat"];
        let user_id = chat["id"].as_i64().ok_or("id not i64")?.to_string();
        let nickname = chat["first_name"].as_str().ok_or("first_name not str")?;
        let username = chat["username"].as_str().ok_or("username not str")?;
        let self_id = self.self_id.read().unwrap().clone();
        
        let message_obj = &event["message"];

        add_username2userid(&format!("@{username}"), user_id.parse()?);

        let message_id = message_obj["message_id"]
            .as_i64()
            .ok_or("message_id not i64")?
            .to_string();

        // 存msg_id
        let msg_id = self.add_msg_id(&vec![RawMsgId{msg_id:message_id.to_owned(),chat_id:user_id.to_string()}]);

        let message = self.to_cq_msg(event).await?;

        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":msg_id,
            "user_id":user_id,
            "message":message,
            "raw_message":message,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
            },
            "platform":"telegram"
        });
        RT_PTR.spawn_blocking(move || {
            let json_str = event_json.to_string();
            cq_add_log(&format!("TELEGRAM_OB_EVENT:{json_str}")).unwrap();
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
        return Ok(());
    }

    async fn deal_group_msg(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_obj = &event["message"];
        let chat = &event["message"]["chat"];
        let from = &event["message"]["from"];

        let mut username = read_json_str(from, "username");
        let user_id = from["id"].as_i64().ok_or("id not i64")?;
        if username != "" {
            add_username2userid(&format!("@{username}"), user_id);
        } else {
            // 如果是频道消息，username为空
            username = read_json_str(from, "first_name");
        }
        let group_id = chat["id"].as_i64().ok_or("id not i64")?.to_string();
        let message_id = message_obj["message_id"]
            .as_i64()
            .ok_or("message_id not i64")?
            .to_string();

        // 存msg_id
        let msg_id = self.add_msg_id(&vec![RawMsgId{msg_id:message_id.to_owned(),chat_id:group_id.to_string()}]);

        let sender = serde_json::json!({
            "user_id":user_id.to_string(),
            "nickname":username,
            "card":from["first_name"].as_str().ok_or("first_name not str")?,
            "sex":"unknown",
            "age":0,
            "area":"",
            "level": "0".to_owned(),
            "role":"member",
            "title":""
        });
        let message = self.to_cq_msg(event).await?;
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":*self.self_id.read().unwrap(),
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":msg_id,
            "group_id":group_id,
            "user_id":user_id,
            "message":message,
            "raw_message":message,
            "font":0,
            "sender":sender,
            "platform":"telegram"
        });
        RT_PTR.spawn_blocking(move || {
            let json_str = event_json.to_string();
            cq_add_log(&format!("TELEGRAM_OB_EVENT:{json_str}")).unwrap();
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
        return Ok(());
    }

    async fn deal_event(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let is_msg = event.get("message").is_some();
        if is_msg {
            let msg_type = event["message"]["chat"]["type"]
                .as_str()
                .ok_or("chat type not str")?;
            let is_private_msg = msg_type == "private";
            if is_private_msg {
                self.deal_private_msg(event).await?;
            }
            let is_group_msg = msg_type == "group" || msg_type == "supergroup"; 
            if is_group_msg {
                self.deal_group_msg(event).await?;
            }
        }
        return Ok(());
    }
    async fn do_recv_loop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut last_update_id = None;

        let mut url = format!(
            "https://api.telegram.org/bot{}/getUpdates",
            self.token.read().unwrap()
        );
        let ret = self.proxyrequest(&url).await?;
        let event_json: serde_json::Value = serde_json::from_str(&ret)?;
        let result = event_json
            .get("result")
            .ok_or("result not found")?
            .as_array()
            .ok_or("result not array")?;
        if result.len() != 0 {
            last_update_id = Some(
                result[result.len() - 1]
                    .get("update_id")
                    .ok_or("update_id not found")?
                    .as_i64()
                    .ok_or("update_id not i64")?
                    + 1,
            );
        }

        loop {
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if last_update_id.is_none() {
                url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?timeout=30",
                    self.token.read().unwrap()
                );
            } else {
                url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                    self.token.read().unwrap(),
                    last_update_id.unwrap()
                );
            }
            let ret = self.proxyrequest(&url).await?;
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let event_json: serde_json::Value = serde_json::from_str(&ret)?;
            let result = event_json
                .get("result")
                .ok_or("result not found")?
                .as_array()
                .ok_or("result not array")?;
            if result.len() != 0 {
                last_update_id = Some(
                    result[result.len() - 1]
                        .get("update_id")
                        .ok_or("update_id not found")?
                        .as_i64()
                        .ok_or("update_id not i64")?
                        + 1,
                );
            }
            for event in result.clone() {
                cq_add_log(&format!(
                    "telegram bot {} recv:{}",
                    self.self_id.read().unwrap(),
                    event
                ))
                .unwrap();
                let self_clone = self.clone();
                tokio::spawn(async move {
                    match self_clone.deal_event(&event).await {
                        Ok(_) => {}
                        Err(err) => {
                            cq_add_log_w(&format!(
                                "telegram bot {} deal event error:{}",
                                self_clone.self_id.read().unwrap(),
                                err
                            ))
                            .unwrap();
                        }
                    }
                });
            }
        }
        return Ok(());
    }

    fn get_json_bool(js: &serde_json::Value, key: &str) -> bool {
        if let Some(j) = js.get(key) {
            if j.is_boolean() {
                return j.as_bool().unwrap();
            } else if j.is_string() {
                if j.as_str().unwrap() == "true" {
                    return true;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    }

    fn get_auto_escape_from_params(&self, params: &serde_json::Value) -> bool {
        let is_auto_escape = Self::get_json_bool(params, "auto_escape");
        return is_auto_escape;
    }
    pub fn to_json_str(val:&serde_json::Value) -> String {
        if val.is_i64() {
            return val.as_i64().unwrap().to_string();
        }
        if val.is_u64() {
            return val.as_u64().unwrap().to_string();
        }
        if val.is_string() {
            return val.as_str().unwrap().to_string();
        }
        return "".to_owned();
    }

    fn get_msg_id(&self,msg_id:&str) -> Vec<RawMsgId> {
        let lk = self.msg_ids.read().unwrap();
        for msg_id_pair in lk.iter() {
            if msg_id_pair.msg_id == msg_id {
                return msg_id_pair.raw_msg_ids.to_owned();
            }
        }
        return vec![];
    }

    async fn make_telegram_msg(
        &self,
        message_arr: &serde_json::Value,
        group_id: &str
    ) -> Result<(Vec<(&str, String)>, String), Box<dyn std::error::Error + Send + Sync>> {
        let mut to_send_data: Vec<(&str, String)> = vec![];
        let mut quote = String::new();
        let mut last_type = "text";
        let is_group = group_id != "";
        for it in message_arr.as_array().ok_or("message not arr")? {
            let tp = it.get("type").ok_or("type not found")?;
            if tp == "text" {
                let t = it
                    .get("data")
                    .ok_or("data not found")?
                    .get("text")
                    .ok_or("text not found")?
                    .as_str()
                    .ok_or("text not str")?
                    .to_owned();
                let s = t;
                if last_type == "text" && to_send_data.len() != 0 {
                    let l = to_send_data.len();
                    to_send_data.get_mut(l - 1).unwrap().1.push_str(&s);
                } else {
                    to_send_data.push(("text", s));
                    last_type = "text";
                }
            } else if tp == "image" {
                let file = it
                    .get("data")
                    .ok_or("data not found")?
                    .get("file")
                    .ok_or("file not found")?
                    .as_str()
                    .ok_or("file not str")?;
                if file.starts_with("http") {
                    to_send_data.push(("url_image", file.to_owned()));
                    last_type = "url_image";
                } else {
                    if file.starts_with("base64://") {
                        let b64_str = file.get(9..).unwrap();
                        to_send_data.push(("file_image", b64_str.to_owned()));
                        last_type = "file_image";
                    }
                }
            }
            else if tp == "reply" {
                if quote !=  "" {
                    continue;
                }
                let cq_id = Self::to_json_str(it.get("data").ok_or("data not found")?.get("id").ok_or("reply not found")?);
                let telegram_id = self.get_msg_id(&cq_id);
                quote = telegram_id.get(0).ok_or("get telegram msg_id err")?.msg_id.to_owned();
            }
            else if tp == "at" {
                if !is_group {
                    continue;
                }
                let qq = Self::to_json_str(it.get("data").ok_or("data not found")?.get("qq").ok_or("qq not found")?);
                let ret = self.proxyrequest(&format!("https://api.telegram.org/bot{}/getChatMember?chat_id={}&user_id={}",self.token.read().unwrap(),group_id,qq).as_str()).await?;
                let ret_json: serde_json::Value = ret.parse()?;
                let username = ret_json["result"]["user"]["username"].as_str().ok_or("username not found")?;
                let at_str = &format!(" @{} ",username);
                if last_type == "text" && to_send_data.len() != 0 {
                    let l = to_send_data.len();
                    to_send_data.get_mut(l - 1).unwrap().1.push_str(at_str);
                } else {
                    to_send_data.push(("text",at_str.to_owned()));
                    last_type = "text";
                }
            }
        }
        Ok((to_send_data, quote))
    }
    fn add_msg_id(&self, raw_msg_ids: &Vec<RawMsgId>) -> String {
        let new_id = Uuid::new_v4().to_string();
        let msg_id_pair = MsgIdPair {
            msg_id: new_id.to_owned(),
            raw_msg_ids: raw_msg_ids.to_owned(),
        };
        let mut lk = self.msg_ids.write().unwrap();
        lk.push_back(msg_id_pair);
        while lk.len() > 9999 {
            lk.pop_front();
        }
        return new_id;
    }


    async fn send_message_inner(&self,to_send_data:Vec<(&str, String)>,mut quote:String,chat_id:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut msg_ids = vec![];
        for (tp, msg) in &to_send_data.clone() {
            if *tp == "text" {
                // do post
                let client = self.get_req_client()?;
                let mut data_json = serde_json::json!({
                    "chat_id":chat_id.parse::<i64>()?,
                    "text":msg,
                });
                if quote != "" {
                    data_json["reply_parameters"] = serde_json::json!({
                        "message_id":quote.parse::<i64>()?
                    });
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendMessage",
                            self.token.read().unwrap()
                        )
                        .as_str(),
                    )
                    .body(reqwest::Body::from(data_json.to_string()))
                    .build()?;
                req.headers_mut().append(
                    HeaderName::from_str("Content-type")?,
                    HeaderValue::from_str("application/json")?,
                );
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                // cq_add_log(&format!("ret_json:{:?}",ret_json)).unwrap();
                let msg_id = ret_json["result"]["message_id"]
                    .as_i64()
                    .ok_or("message_id not i64")?
                    .to_string();
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            } else if *tp == "url_image" {
                let client = self.get_req_client()?;
                let mut data_json = serde_json::json!({
                    "chat_id":chat_id.parse::<i64>()?,
                    "photo":msg,
                });
                if quote != "" {
                    data_json["reply_parameters"] = serde_json::json!({
                        "message_id":quote.parse::<i64>()?
                    });
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendPhoto",
                            self.token.read().unwrap()
                        )
                        .as_str(),
                    )
                    .body(reqwest::Body::from(data_json.to_string()))
                    .build()?;
                req.headers_mut().append(
                    HeaderName::from_str("Content-type")?,
                    HeaderValue::from_str("application/json")?,
                );
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                // cq_add_log(&format!("ret_json:{:?}", ret_json)).unwrap();
                let msg_id = ret_json["result"]["message_id"]
                    .as_i64()
                    .ok_or("message_id not i64")?
                    .to_string();
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            } else if *tp == "file_image" {
                let file_bin = base64::Engine::decode(
                    &base64::engine::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        base64::engine::general_purpose::PAD,
                    ),
                    msg,
                )?;

                let client = self.get_req_client()?;
                let mut form = reqwest::multipart::Form::new()
                    .part(
                        "photo",
                        reqwest::multipart::Part::bytes(file_bin).file_name("redpic"),
                    )
                    .part(
                        "chat_id",
                        reqwest::multipart::Part::text(chat_id.to_owned()),
                    );
                    if quote != "" {
                        form = form.part(
                            "reply_parameters",
                            reqwest::multipart::Part::text(serde_json::json!({
                                "message_id":quote.parse::<i64>()?
                            }).to_string()),
                        );
                        quote = "".to_owned();
                    }
                let req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendPhoto",
                            self.token.read().unwrap()
                        )
                        .as_str(),
                    )
                    .multipart(form)
                    .build()?;
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                // cq_add_log(&format!("ret_json:{:?}", ret_json)).unwrap();
                let msg_id = ret_json["result"]["message_id"]
                    .as_i64()
                    .ok_or("message_id not i64")?
                    .to_string();
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            }
        }
        let msg_id = self.add_msg_id(&msg_ids);
        Ok(msg_id)
    }

    async fn deal_ob_send_group_msg(
        &self,
        params: &serde_json::Value,
        echo: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params, "group_id");
        let message_arr: serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;

        if message_rst.is_string() {
            if self.get_auto_escape_from_params(&params) {
                message_arr = serde_json::json!(
                    [{"type":"text","data":{
                        "text": message_rst.as_str()
                    }}]
                );
            } else {
                message_arr = str_msg_to_arr(message_rst)
                    .map_err(|x| format!("str_msg_to_arr err:{:?}", x))?;
            }
        } else {
            message_arr = params.get("message").ok_or("message not found")?.to_owned();
        }

        let (to_send_data, quote) = self.make_telegram_msg(&message_arr, &group_id).await?;

        let msg_id = self.send_message_inner(to_send_data,quote,&group_id).await?;

        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {
                "message_id":msg_id
            },
            "echo":echo
        });
        Ok(send_json)
    }

    async fn deal_ob_send_private_msg(
        &self,
        params: &serde_json::Value,
        echo: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let user_id = read_json_str(params, "user_id");
        let message_arr: serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;

        if message_rst.is_string() {
            if self.get_auto_escape_from_params(&params) {
                message_arr = serde_json::json!(
                    [{"type":"text","data":{
                        "text": message_rst.as_str()
                    }}]
                );
            } else {
                message_arr = str_msg_to_arr(message_rst)
                    .map_err(|x| format!("str_msg_to_arr err:{:?}", x))?;
            }
        } else {
            message_arr = params.get("message").ok_or("message not found")?.to_owned();
        }

        let (to_send_data, quote) = self.make_telegram_msg(&message_arr, "").await?;

        let msg_id = self.send_message_inner(to_send_data,quote,&user_id).await?;

        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {
                "message_id":msg_id
            },
            "echo":echo
        });
        Ok(send_json)
    }

    async fn deal_ob_delete_msg(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let msg_id = read_json_str(params,"message_id");
        let msg_ids = self.get_msg_id(&msg_id);
        if msg_ids.len() == 0 {
            return Err("msg_id not found")?;
        }
        for it in msg_ids {
            let ret_text = self.proxyrequest(&format!("https://api.telegram.org/bot{}/deleteMessage?chat_id={}&message_id={}",self.token.read().unwrap(),it.chat_id,it.msg_id).as_str()).await?;
            let ret_json: serde_json::Value = serde_json::from_str(&ret_text)?;
            let result = ret_json.get("result").ok_or("result not found")?.as_bool().ok_or("result not bool")?;
            if !result {
                return Err("delete message error")?;
            }
        }
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
    }
    async fn deal_ob_get_login_info(&self,_params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let ret_text = self.proxyrequest(&format!("https://api.telegram.org/bot{}/getMe",self.token.read().unwrap()).as_str()).await?;
        let ret_json: serde_json::Value = serde_json::from_str(&ret_text)?;
        let info = serde_json::json!({
            "user_id":ret_json["result"]["id"].as_i64().ok_or("id not i64")?,
            "nickname":ret_json["result"]["first_name"].as_str().ok_or("first_name not str")?,
        });
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
}



#[async_trait]
impl BotConnectTrait for TeleTramConnect {
    async fn disconnect(&mut self) {
        self.is_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn get_alive(&self) -> bool {
        return !self.is_stop.load(std::sync::atomic::Ordering::Relaxed);
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(11..).ok_or("telegram url格式错误")?;
        let config_json: serde_json::Value = serde_json::from_str(config_json_str)?;
        let token = config_json
            .get("Token")
            .ok_or("token not found")?
            .as_str()
            .ok_or("token not str")?;
        let proxy = config_json
            .get("Proxy")
            .ok_or("proxy not found")?
            .as_str()
            .ok_or("proxy not str")?;
        self.proxy.write().unwrap().push_str(proxy);
        let ret_text =
            self.proxyrequest(format!("https://api.telegram.org/bot{}/getMe", token).as_str()).await?;
        let ret_json: serde_json::Value = serde_json::from_str(&ret_text)?;
        let self_id = ret_json
            .get("result")
            .ok_or("result not found")?
            .get("id")
            .ok_or("id not found")?
            .as_i64()
            .ok_or("id not i64")?
            .to_string();
        let username = ret_json
            .get("result")
            .ok_or("result not found")?
            .get("username")
            .ok_or("username not found")?
            .as_str()
            .ok_or("username not str")?;
        add_username2userid(&format!("@{username}"), self_id.parse()?);
        self.self_id.write().unwrap().push_str(self_id.as_str());
        self.token.write().unwrap().push_str(token);
        cq_add_log(&format!("telegram bot {} connect success", self_id)).unwrap();
        let self_t = self.clone();
        tokio::task::spawn(async move {
            match self_t.do_recv_loop().await {
                Ok(_) => {}
                Err(err) => {
                    cq_add_log(&format!(
                        "telegram bot {} recv error:{}",
                        self_t.self_id.read().unwrap(),
                        err
                    ))
                    .unwrap();
                }
            };
            self_t
                .is_stop
                .store(true, std::sync::atomic::Ordering::Relaxed);
        });
        Ok(())
    }

    async fn call_api(
        &self,
        _platform: &str,
        _self_id: &str,
        _passive_id: &str,
        json: &mut serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = json
            .get("action")
            .ok_or("action not found")?
            .as_str()
            .ok_or("action not str")?;
        let echo = json.get("echo").unwrap_or(&serde_json::Value::Null);
        let def = serde_json::json!({});
        let params = json.get("params").unwrap_or(&def);
        let send_json = match action {
            "send_group_msg" => self.deal_ob_send_group_msg(&params, &echo).await?,
            "send_private_msg" => {
                self.deal_ob_send_private_msg(&params,&echo).await?
            },
            "send_msg" => {
                let group_id = read_json_str(params, "group_id");
                if group_id != "" {
                    self.deal_ob_send_group_msg(&params,&echo).await?
                }else {
                    self.deal_ob_send_private_msg(&params,&echo).await?
                }
            },
            "delete_msg" => {
                self.deal_ob_delete_msg(&params,&echo).await?
            },
            "get_login_info" => {
                self.deal_ob_get_login_info(&params,&echo).await?
            },
            // "get_stranger_info" => {
            //     self.deal_ob_get_stranger_info(&params,&echo).await?
            // },
            // "get_group_info" => {
            //     self.deal_ob_get_group_info(&params,&echo).await?
            // },
            // "get_group_list" => {
            //     self.deal_ob_get_group_list(&params,&echo).await?
            // },
            // "get_group_member_info" => {
            //     self.deal_ob_get_group_member_info(&params,&echo).await?
            // },
            // "set_group_kick" => {
            //     self.deal_ob_set_group_kick(&params,&echo).await?
            // },
            // "set_group_leave" => {
            //     self.deal_ob_set_group_leave(&params,&echo).await?
            // },
            // "set_group_name" => {
            //     self.deal_ob_set_group_name(&params,&echo).await?
            // },
            // "set_group_card" => {
            //     self.deal_ob_set_group_card(&params,&echo).await?
            // },
            // "get_friend_list" => {
            //     self.deal_ob_get_friend_list(&params,&echo).await?
            // },
            // "get_group_member_list" => {
            //     self.deal_ob_get_group_member_list(&params,&echo).await?
            // },
            // "get_cookies" => {
            //     self.deal_ob_get_cookies(&params,&echo).await?
            // },
            "can_send_image" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {"yes":true},
                    "echo":echo
                })
            }
            "can_send_record" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {"yes":false},
                    "echo":echo
                })
            }
            "get_status" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {
                        "online":true,
                        "good":true
                    },
                    "echo":echo
                })
            }
            "get_version_info" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {
                        "app_name":"telegram_redreply",
                        "app_version":"0.0.1",
                        "protocol_version":"v1"
                    },
                    "echo":echo
                })
            }
            _ => {
                serde_json::json!({
                    "status":"failed",
                    "retcode":1404,
                    "echo":echo
                })
            }
        };
        return Ok(send_json);
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        let lk = self.self_id.read().unwrap();
        if lk.is_empty() {
            return vec![];
        }
        return vec![("telegram".to_owned(), lk.to_owned())];
    }
}
