use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::time::SystemTime;

use super::BotConnectTrait;
use crate::cqapi::{cq_add_log, cq_add_log_w};
use crate::mytool::{
    cq_fix_image_file_in_arr, cq_params_encode, cq_text_encode, read_json_str, str_msg_to_arr,
};
use crate::{native_api, AdapterResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TeleTramConnect {
    pub self_id: Arc<RwLock<String>>,
    pub token: Arc<RwLock<String>>,
    pub url: String,
    pub proxy: Arc<RwLock<String>>,
    pub is_stop: Arc<AtomicBool>,
    msg_ids: Arc<RwLock<VecDeque<MsgIdPair>>>,
    username2userid: Arc<RwLock<HashMap<String, i64>>>,
    allowed_updates: Vec<String>,
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
        Self {
            self_id: Arc::new(RwLock::new("".to_owned())),
            token: Arc::new(RwLock::new("".to_owned())),
            url: url.to_owned(),
            proxy: Arc::new(RwLock::new("".to_owned())),
            is_stop: Arc::new(AtomicBool::new(false)),
            msg_ids: Arc::new(RwLock::new(VecDeque::new())),
            username2userid: Arc::new(RwLock::new(HashMap::new())),
            allowed_updates: Vec::new(),
        }
    }

    fn add_username2userid(&self, username: &str, user_id: i64) {
        let mut lk = self.username2userid.write().unwrap_or_else(|e| e.into_inner());
        // 简单防膨胀：超过上限先清空
        if lk.len() > 100_000 {
            lk.clear();
        }
        lk.insert(username.to_owned(), user_id);
    }

    fn get_username2userid(&self, username: &str) -> Option<i64> {
        self.username2userid.read().unwrap_or_else(|e| e.into_inner()).get(username).cloned()
    }

    fn request_client_builder(&self) -> AdapterResult<reqwest::ClientBuilder> {
        let proxy = self.proxy.read().unwrap_or_else(|e| e.into_inner());
        let builder = reqwest::Client::builder().timeout(Duration::from_secs(60));
        if proxy.is_empty() {
            Ok(builder.no_proxy())
        } else {
            Ok(builder.proxy(reqwest::Proxy::all(proxy.as_str())?))
        }
    }

    fn get_req_client(&self) -> AdapterResult<reqwest::Client> {
        Ok(self.request_client_builder()?.build()?)
    }

    async fn call_native_api(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let method = params["method"]
            .as_str()
            .ok_or("缺少 Telegram API 方法名")?;
        let token = self.token.read().unwrap_or_else(|e| e.into_inner()).clone();
        let url = native_api::telegram_url(&token, method)?;
        let empty = json!({});
        let payload = params.get("body").unwrap_or(&empty);
        let client = self
            .request_client_builder()?
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(25))
            .build()?;
        let request = native_api::post(client.post(url), payload, params.get("files"))?;
        let (status, body) = native_api::response(request, "Telegram").await?;
        let mut reply = native_api::telegram_reply(status, &body, echo);
        // Methods that return messages preserve provider IDs and expose an OB handle.
        let result = &reply["data"];
        let messages: Vec<_> = result
            .as_array()
            .map_or_else(|| vec![result], |items| items.iter().collect());
        let ids: Vec<_> = messages
            .into_iter()
            .filter_map(|message| {
                Some(RawMsgId {
                    msg_id: message["message_id"].as_i64()?.to_string(),
                    chat_id: message["chat"]["id"].as_i64()?.to_string(),
                })
            })
            .collect();
        if !ids.is_empty() {
            reply["redreply_message_id"] = self.add_msg_id(&ids).into();
        }
        Ok(reply)
    }

    fn updates_url(&self, offset: Option<i64>, timeout: u64) -> AdapterResult<reqwest::Url> {
        let mut url = native_api::telegram_url(&self.token.read().unwrap_or_else(|e| e.into_inner()), "getUpdates")?;
        let mut query = url.query_pairs_mut();
        query.append_pair("timeout", &timeout.to_string());
        query.append_pair(
            "allowed_updates",
            &serde_json::to_string(&self.allowed_updates)?,
        );
        if let Some(offset) = offset {
            query.append_pair("offset", &offset.to_string());
        }
        drop(query);
        Ok(url)
    }

    fn native_update(&self, kind: &str, event: &Value) -> Value {
        let data = &event[kind];
        let self_id = self.self_id.read().unwrap_or_else(|e| e.into_inner()).clone();
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let chat = data.get("chat").unwrap_or(&data["message"]["chat"]);
        let from = &data["from"];
        let mut notice = json!({
            "time": timestamp,
            "platform": "telegram",
            "self_id": self_id,
            "post_type": "notice",
            "notice_type": "telegram",
            "sub_type": kind,
            "update_id": event["update_id"],
            "telegram": data,
        });
        if let Some(user_id) = from["id"].as_i64() {
            notice["user_id"] = user_id.to_string().into();
            let name = from["username"]
                .as_str()
                .or_else(|| from["first_name"].as_str())
                .unwrap_or_default();
            notice["sender"] = json!({"user_id": user_id.to_string(), "nickname": name});
        }
        if let Some(chat_id) = chat["id"].as_i64() {
            let chat_id = chat_id.to_string();
            notice["chat_id"] = chat_id.clone().into();
            if let Some(chat_type) = chat["type"].as_str() {
                notice["chat_type"] = chat_type.into();
                if chat_type != "private" {
                    notice["group_id"] = chat_id.clone().into();
                }
            }
            if kind == "callback_query" {
                if let Some(message_id) = data["message"]["message_id"].as_i64() {
                    notice["message_id"] = self
                        .add_msg_id(&vec![RawMsgId {
                            msg_id: message_id.to_string(),
                            chat_id,
                        }])
                        .into();
                }
            }
        }
        notice
    }

    async fn proxyrequest(
        &self,
        url: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let request = self.get_req_client()?.get(url);
        let request = if url.contains("/getUpdates") {
            request.timeout(Duration::from_secs(15))
        } else {
            request
        };
        let response = request.send().await.map_err(|err| err.without_url())?;
        Ok(response.text().await.map_err(|err| err.without_url())?)
    }

    fn parse_updates_result(ret: &str) -> Result<Vec<serde_json::Value>, String> {
        let event_json: serde_json::Value =
            serde_json::from_str(ret).map_err(|err| format!("invalid json: {err}; raw={ret}"))?;
        let result = event_json
            .get("result")
            .and_then(|value| value.as_array())
            .ok_or_else(|| format!("result not found; response={event_json}"))?;
        Ok(result.clone())
    }

    fn utf16_slice(
        text: &str,
        start: usize,
        end: usize,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if start > end || end > units.len() {
            return Err(format!(
                "telegram entity range out of bounds: start={start}, end={end}, len={}",
                units.len()
            )
            .into());
        }
        Ok(String::from_utf16_lossy(&units[start..end]))
    }

    fn make_msg_with_at(
        &self,
        text_message: &str,
        entities: &serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut ret_cq_msg = "".to_owned();
        if let Some(ents) = entities.as_array() {
            // Telegram Bot API uses UTF-16 code units for entity offset/length.
            let mut last_end: usize = 0;
            for ent in ents {
                let tp = ent["type"].as_str().ok_or("type not str")?;
                if tp != "mention" {
                    continue;
                }
                let offset = ent["offset"].as_i64().ok_or("offset not i64")?;
                let length = ent["length"].as_i64().ok_or("length not i64")?;
                if offset < 0 || length < 0 {
                    return Err("telegram entity offset/length must be non-negative".into());
                }
                let start = offset as usize;
                let end = (offset + length) as usize;
                if start < last_end {
                    return Err("telegram entity ranges are not ordered".into());
                }
                let text_before = Self::utf16_slice(text_message, last_end, start)?;
                ret_cq_msg.push_str(&cq_text_encode(&text_before));
                let mention = Self::utf16_slice(text_message, start, end)?;
                if let Some(userid) = self.get_username2userid(&mention) {
                    ret_cq_msg.push_str(&format!("[CQ:at,qq={}]", userid));
                } else {
                    ret_cq_msg.push_str(&cq_text_encode(&mention));
                }
                last_end = end;
            }
            let text_end =
                Self::utf16_slice(text_message, last_end, text_message.encode_utf16().count())?;
            ret_cq_msg.push_str(&cq_text_encode(&text_end));
        } else {
            ret_cq_msg.push_str(&cq_text_encode(text_message));
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
            let is_automatic_forward =
                Self::get_json_bool(reply_to_message, "is_automatic_forward");
            if !is_automatic_forward {
                let raw_reply_id = reply_to_message["message_id"]
                    .as_i64()
                    .ok_or("message_id not i64")?
                    .to_string();
                // 优先映射回内部UUID，插件回传时才能通过 get_msg_id 解析；
                // 超出映射缓存窗口时回退为 telegram 原生 message_id
                reply_id = self.get_cq_msg_id(&raw_reply_id).unwrap_or(raw_reply_id);
            }
        }

        let text_message = read_json_str(message_obj, "text");
        let entities = &message_obj["entities"];
        ret_cq_msg.push_str(&self.make_msg_with_at(&text_message, entities)?);

        let caption_message = read_json_str(message_obj, "caption");
        let caption_entities = &message_obj["caption_entities"];
        ret_cq_msg.push_str(&self.make_msg_with_at(&caption_message, caption_entities)?);

        if message_obj.get("photo").is_some() {
            let photo = message_obj["photo"].as_array().ok_or("photo not array")?;
            if !photo.is_empty() {
                let photo_len = photo.len();
                let photo_file_id = photo[photo_len - 1]["file_id"]
                    .as_str()
                    .ok_or("file_id not str")?;
                let get_file_path_ret = self
                    .proxyrequest(
                        format!(
                            "https://api.telegram.org/bot{}/getFile?file_id={}",
                            self.token.read().unwrap_or_else(|e| e.into_inner()),
                            photo_file_id
                        )
                        .as_str(),
                    )
                    .await?;
                let get_file_path_json: serde_json::Value =
                    serde_json::from_str(&get_file_path_ret)?;
                let file_path = get_file_path_json["result"]["file_path"]
                    .as_str()
                    .ok_or("file_path not str")?;
                let file_url = format!(
                    "https://api.telegram.org/file/bot{}/{}",
                    self.token.read().unwrap_or_else(|e| e.into_inner()),
                    file_path
                );
                ret_cq_msg.push_str(&format!("[CQ:image,file={}]", &cq_params_encode(&file_url)));
            }
        }
        if !reply_id.is_empty() {
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
        let nickname = chat["first_name"].as_str().unwrap_or("");
        let username = chat["username"].as_str().unwrap_or("");
        let self_id = self.self_id.read().unwrap_or_else(|e| e.into_inner()).clone();

        let message_obj = &event["message"];

        if !username.is_empty() {
            self.add_username2userid(&format!("@{username}"), user_id.parse()?);
        }

        let message_id = message_obj["message_id"]
            .as_i64()
            .ok_or("message_id not i64")?
            .to_string();

        // 存msg_id
        let msg_id = self.add_msg_id(&vec![RawMsgId {
            msg_id: message_id.to_owned(),
            chat_id: user_id.to_string(),
        }]);

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
            "platform":"telegram",
            "telegram": message_obj,
        });
        tokio::task::spawn_blocking(move || {
            let json_str = event_json.to_string();
            let _ = cq_add_log(&format!("TELEGRAM_OB_EVENT:{json_str}"));
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        Ok(())
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
        if !username.is_empty() {
            self.add_username2userid(&format!("@{username}"), user_id);
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
        let msg_id = self.add_msg_id(&vec![RawMsgId {
            msg_id: message_id.to_owned(),
            chat_id: group_id.to_string(),
        }]);

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
            "self_id":*self.self_id.read().unwrap_or_else(|e| e.into_inner()),
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
            "platform":"telegram",
            "telegram": event["message"],
        });
        tokio::task::spawn_blocking(move || {
            let json_str = event_json.to_string();
            let _ = cq_add_log(&format!("TELEGRAM_OB_EVENT:{json_str}"));
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        Ok(())
    }

    async fn deal_event(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_chat_type = event["message"]["chat"]["type"].as_str();
        let ordinary_message =
            matches!(message_chat_type, Some("private" | "group" | "supergroup"));
        let update = event
            .as_object()
            .and_then(|fields| fields.keys().find(|key| key.as_str() != "update_id"));
        if let Some(kind) = update.filter(|_| !ordinary_message) {
            let notice = self.native_update(kind, event);
            tokio::task::spawn_blocking(move || {
                if let Err(error) = crate::cqevent::do_1207_event(&notice.to_string()) {
                    let _ = cq_add_log_w(&format!("Telegram 原生事件处理失败：{error}"));
                }
            });
        }
        let is_msg = event.get("message").is_some();
        if is_msg {
            if let Some(from_id) = event["message"]["from"]["id"]
                .as_i64()
                .map(|id| id.to_string())
            {
                if from_id == *self.self_id.read().unwrap_or_else(|e| e.into_inner()) {
                    return Ok(());
                }
            }
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
        Ok(())
    }
    async fn do_recv_loop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut last_update_id = None;

        let url = self.updates_url(None, 0)?;
        let result = match self.proxyrequest(url.as_str()).await {
            Ok(ret) => match Self::parse_updates_result(&ret) {
                Ok(result) => result,
                Err(err) => {
                    let _ = cq_add_log_w(&format!(
                        "telegram bot {} recv warning:{}",
                        self.self_id.read().unwrap_or_else(|e| e.into_inner()),
                        err
                    ));
                    Vec::new()
                }
            },
            Err(err) => {
                let _ = cq_add_log_w(&format!(
                    "telegram bot {} recv warning:{}",
                    self.self_id.read().unwrap_or_else(|e| e.into_inner()),
                    err
                ));
                Vec::new()
            }
        };
        if !result.is_empty() {
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
            let url = self.updates_url(last_update_id, 8)?;
            let ret = match self.proxyrequest(url.as_str()).await {
                Ok(ret) => ret,
                Err(err) => {
                    let _ = cq_add_log_w(&format!(
                        "telegram bot {} recv warning:{}",
                        self.self_id.read().unwrap_or_else(|e| e.into_inner()),
                        err
                    ));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let result = match Self::parse_updates_result(&ret) {
                Ok(result) => result,
                Err(err) => {
                    let _ = cq_add_log_w(&format!(
                        "telegram bot {} recv warning:{}",
                        self.self_id.read().unwrap_or_else(|e| e.into_inner()),
                        err
                    ));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if !result.is_empty() {
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
                let _ = cq_add_log(&format!(
                    "telegram bot {} recv:{}",
                    self.self_id.read().unwrap_or_else(|e| e.into_inner()),
                    event
                ));
                let self_clone = self.clone();
                tokio::spawn(async move {
                    match self_clone.deal_event(&event).await {
                        Ok(_) => {}
                        Err(err) => {
                            let _ = cq_add_log_w(&format!(
                                "telegram bot {} deal event error:{}",
                                self_clone.self_id.read().unwrap_or_else(|e| e.into_inner()),
                                err
                            ));
                        }
                    }
                });
            }
        }
        Ok(())
    }

    fn get_json_bool(js: &serde_json::Value, key: &str) -> bool {
        if let Some(j) = js.get(key) {
            if let Some(value) = j.as_bool() { value } else if let Some(value) = j.as_str() { value == "true" } else {
                false
            }
        } else {
            false
        }
    }

    fn get_auto_escape_from_params(&self, params: &serde_json::Value) -> bool {
        Self::get_json_bool(params, "auto_escape")
    }
    pub fn to_json_str(val: &serde_json::Value) -> String {
        if let Some(value) = val.as_i64() { return value.to_string(); } if let Some(value) = val.as_u64() { return value.to_string(); } if let Some(value) = val.as_str() { return value.to_string(); }
        "".to_owned()
    }

    fn get_msg_id(&self, msg_id: &str) -> Vec<RawMsgId> {
        let lk = self.msg_ids.read().unwrap_or_else(|e| e.into_inner());
        for msg_id_pair in lk.iter() {
            if msg_id_pair.msg_id == msg_id {
                return msg_id_pair.raw_msg_ids.to_owned();
            }
        }
        vec![]
    }

    /// 由 telegram 原生 message_id 反查内部 UUID（类似 kook 的 get_cq_msg_id）
    fn get_cq_msg_id(&self, raw_msg_id: &str) -> Option<String> {
        let lk = self.msg_ids.read().unwrap_or_else(|e| e.into_inner());
        for msg_id_pair in lk.iter() {
            for raw_msg in &msg_id_pair.raw_msg_ids {
                if raw_msg.msg_id == raw_msg_id {
                    return Some(msg_id_pair.msg_id.clone());
                }
            }
        }
        None
    }

    async fn make_telegram_msg(
        &self,
        message_arr: &serde_json::Value,
        group_id: &str,
    ) -> Result<(Vec<(&str, String, String)>, String), Box<dyn std::error::Error + Send + Sync>>
    {
        // 每个元素为 (类型, 内容, caption)；caption 仅对图片类型有意义
        let mut to_send_data: Vec<(&str, String, String)> = vec![];
        let mut quote = String::new();
        let mut last_type = "text";
        let is_group = !group_id.is_empty();
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
                if last_type == "text" && !to_send_data.is_empty() {
                    if let Some(last) = to_send_data.last_mut() { last.1.push_str(&s); }
                } else {
                    to_send_data.push(("text", s, String::new()));
                    last_type = "text";
                }
            } else if tp == "image" {
                let data = it.get("data").ok_or("data not found")?;
                let file = data
                    .get("file")
                    .ok_or("file not found")?
                    .as_str()
                    .ok_or("file not str")?;
                // 【设置图片参数@summary@文字】→ 图片命令输出 [CQ:image,file=...,summary=文字]
                // str_msg_to_arr 将其解析为 data.summary 字段
                let caption = data
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                if file.starts_with("http") {
                    to_send_data.push(("url_image", file.to_owned(), caption));
                    last_type = "url_image";
                } else if file.starts_with("base64://") {
                    let b64_str = file.get(9..).ok_or("base64 uri 长度不足")?;
                    to_send_data.push(("file_image", b64_str.to_owned(), caption));
                    last_type = "file_image";
                }
            } else if tp == "reply" {
                if !quote.is_empty() {
                    continue;
                }
                let cq_id = Self::to_json_str(
                    it.get("data")
                        .ok_or("data not found")?
                        .get("id")
                        .ok_or("reply not found")?,
                );
                let telegram_id = self.get_msg_id(&cq_id);
                quote = match telegram_id.first() {
                    Some(raw) => raw.msg_id.to_owned(),
                    // 查不到内部映射：可能是超出缓存窗口的 telegram 原生 message_id
                    None => cq_id,
                };
            } else if tp == "at" {
                if !is_group {
                    continue;
                }
                let qq = Self::to_json_str(
                    it.get("data")
                        .ok_or("data not found")?
                        .get("qq")
                        .ok_or("qq not found")?,
                );
                let ret = self
                    .proxyrequest(
                        format!(
                            "https://api.telegram.org/bot{}/getChatMember?chat_id={}&user_id={}",
                            self.token.read().unwrap_or_else(|e| e.into_inner()),
                            group_id,
                            qq
                        )
                        .as_str(),
                    )
                    .await?;
                let ret_json: serde_json::Value = ret.parse()?;
                let username = ret_json["result"]["user"]["username"]
                    .as_str()
                    .ok_or("username not found")?;
                let at_str = &format!(" @{} ", username);
                if last_type == "text" && !to_send_data.is_empty() {
                    if let Some(last) = to_send_data.last_mut() { last.1.push_str(at_str); }
                } else {
                    to_send_data.push(("text", at_str.to_owned(), String::new()));
                    last_type = "text";
                }
            } else if tp == "record" {
                let data = it.get("data").ok_or("data not found")?;
                let file = read_json_str(data, "file");
                if file.starts_with("http") {
                    to_send_data.push(("url_record", file.to_owned(), String::new()));
                    last_type = "url_record";
                } else if file.starts_with("base64://") {
                    let b64_str = file.get(9..).ok_or("base64 uri 长度不足")?;
                    to_send_data.push(("file_record", b64_str.to_owned(), String::new()));
                    last_type = "file_record";
                }
            } else if tp == "music" {
                let music_type = it
                    .get("data")
                    .ok_or("data not found")?
                    .get("type")
                    .ok_or("type not found")?
                    .as_str()
                    .ok_or("type not str")?;
                if music_type == "custom" {
                    let data = it.get("data").ok_or("data not found")?;
                    let mut audio = read_json_str(data, "audio");
                    if audio.is_empty() {
                        audio = read_json_str(data, "voice");
                    }
                    let title = read_json_str(data, "title");
                    let image = read_json_str(data, "image");
                    let url = read_json_str(data, "url");
                    let content = read_json_str(data, "content");
                    let js = serde_json::json!({
                        "type": "audio",
                        "title": title,
                        "audio": audio,
                        "image": image,
                        "content": content,
                        "url": url
                    });
                    to_send_data.push(("music", js.to_string(), String::new()));
                    last_type = "music";
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
        let mut lk = self.msg_ids.write().unwrap_or_else(|e| e.into_inner());
        lk.push_back(msg_id_pair);
        while lk.len() > 9999 {
            lk.pop_front();
        }
        new_id
    }

    fn telegram_message_id(ret_json: &serde_json::Value) -> Result<String, String> {
        if ret_json
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .is_some_and(|ok| !ok)
        {
            return Err(format!("telegram api returned error: {ret_json}"));
        }

        ret_json["result"]["message_id"]
            .as_i64()
            .map(|id| id.to_string())
            .ok_or_else(|| format!("telegram api response missing result.message_id: {ret_json}"))
    }

    async fn send_message_inner(
        &self,
        to_send_data: Vec<(&str, String, String)>,
        mut quote: String,
        chat_id: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut msg_ids = vec![];
        for (tp, msg, caption) in &to_send_data.clone() {
            if *tp == "text" {
                // do post
                let client = self.get_req_client()?;
                let mut data_json = serde_json::json!({
                    "chat_id":chat_id.parse::<i64>()?,
                    "text":msg,
                });
                if !quote.is_empty() {
                    data_json["reply_parameters"] = serde_json::json!({
                        "message_id":quote.parse::<i64>()?
                    });
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendMessage",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
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
                let msg_id = Self::telegram_message_id(&ret_json)?;
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
                if !caption.is_empty() {
                    data_json["caption"] = serde_json::Value::String(caption.clone());
                }
                if !quote.is_empty() {
                    data_json["reply_parameters"] = serde_json::json!({
                        "message_id":quote.parse::<i64>()?
                    });
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendPhoto",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
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
                let msg_id = Self::telegram_message_id(&ret_json)?;
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
                if !caption.is_empty() {
                    form = form.part("caption", reqwest::multipart::Part::text(caption.clone()));
                }
                if !quote.is_empty() {
                    form = form.part(
                        "reply_parameters",
                        reqwest::multipart::Part::text(
                            serde_json::json!({
                                "message_id":quote.parse::<i64>()?
                            })
                            .to_string(),
                        ),
                    );
                    quote = "".to_owned();
                }
                let req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendPhoto",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
                        )
                        .as_str(),
                    )
                    .multipart(form)
                    .build()?;
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                let msg_id = Self::telegram_message_id(&ret_json)?;
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            } else if *tp == "url_record" {
                let client = self.get_req_client()?;
                let mut data_json = serde_json::json!({
                    "chat_id":chat_id.parse::<i64>()?,
                    "audio":msg,
                });
                if !quote.is_empty() {
                    data_json["reply_parameters"] = serde_json::json!({
                        "message_id":quote.parse::<i64>()?
                    });
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendAudio",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
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
                let msg_id = Self::telegram_message_id(&ret_json)?;
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            } else if *tp == "file_record" {
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
                        "audio",
                        reqwest::multipart::Part::bytes(file_bin).file_name("语音"),
                    )
                    .part(
                        "chat_id",
                        reqwest::multipart::Part::text(chat_id.to_owned()),
                    );
                if !quote.is_empty() {
                    form = form.part(
                        "reply_parameters",
                        reqwest::multipart::Part::text(
                            serde_json::json!({
                                "message_id":quote.parse::<i64>()?
                            })
                            .to_string(),
                        ),
                    );
                    quote = "".to_owned();
                }
                let req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendAudio",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
                        )
                        .as_str(),
                    )
                    .multipart(form)
                    .build()?;
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                let msg_id = Self::telegram_message_id(&ret_json)?;
                msg_ids.push(RawMsgId {
                    msg_id,
                    chat_id: chat_id.to_owned(),
                });
            } else if *tp == "music" {
                let client = self.get_req_client()?;
                let msg_json: serde_json::Value = serde_json::from_str(msg)?;
                let title = msg_json["title"]
                    .as_str()
                    .ok_or("title not str")?
                    .to_owned();
                let audio = msg_json["audio"]
                    .as_str()
                    .ok_or("audio not str")?
                    .to_owned();
                let image = msg_json["image"].as_str().ok_or("image not str")?;
                let content = msg_json["content"]
                    .as_str()
                    .ok_or("content not str")?
                    .to_owned();
                let url = msg_json["url"].as_str().ok_or("url not str")?.to_owned();

                let mut form = reqwest::multipart::Form::new();

                if !image.is_empty() {
                    let cli = self.get_req_client()?;
                    let resp = cli.get(image).send().await?;
                    let image_bin = resp.bytes().await?;
                    form = form.part(
                        "thumbnail",
                        reqwest::multipart::Part::bytes(image_bin.to_vec()).file_name("image.jpg"),
                    )
                }

                {
                    let cli = self.get_req_client()?;
                    let resp = cli.get(audio).send().await?;
                    let audio_bin = resp.bytes().await?;
                    form = form.part(
                        "audio",
                        reqwest::multipart::Part::bytes(audio_bin.to_vec())
                            .file_name(title.clone()),
                    )
                }

                if !title.is_empty() {
                    form = form.part("title", reqwest::multipart::Part::text(title));
                }

                if !content.is_empty() {
                    form = form.part("performer", reqwest::multipart::Part::text(content));
                }

                if !url.is_empty() {
                    form = form.part("caption", reqwest::multipart::Part::text(url));
                }

                form = form.part(
                    "chat_id",
                    reqwest::multipart::Part::text(chat_id.to_owned()),
                );

                let req = client
                    .post(
                        format!(
                            "https://api.telegram.org/bot{}/sendAudio",
                            self.token.read().unwrap_or_else(|e| e.into_inner())
                        )
                        .as_str(),
                    )
                    .multipart(form)
                    .build()?;
                let resp = client.execute(req).await?;
                let ret_json: serde_json::Value = resp.text().await?.parse()?;
                let _ = cq_add_log(&format!("music card ret:{:?}", ret_json));
                let msg_id = Self::telegram_message_id(&ret_json)?;
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
        let mut message_arr: serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;

        if message_rst.is_string() {
            if self.get_auto_escape_from_params(params) {
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
        cq_fix_image_file_in_arr(&mut message_arr);

        let (to_send_data, quote) = self.make_telegram_msg(&message_arr, &group_id).await?;

        let msg_id = self
            .send_message_inner(to_send_data, quote, &group_id)
            .await?;

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
        let mut message_arr: serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;

        if message_rst.is_string() {
            if self.get_auto_escape_from_params(params) {
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
        cq_fix_image_file_in_arr(&mut message_arr);

        let (to_send_data, quote) = self.make_telegram_msg(&message_arr, "").await?;

        let msg_id = self
            .send_message_inner(to_send_data, quote, &user_id)
            .await?;

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

    async fn deal_ob_delete_msg(
        &self,
        params: &serde_json::Value,
        echo: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let msg_id = read_json_str(params, "message_id");
        let msg_ids = self.get_msg_id(&msg_id);
        if msg_ids.is_empty() {
            return Err("msg_id not found")?;
        }
        for it in msg_ids {
            let ret_text = self
                .proxyrequest(
                    format!(
                        "https://api.telegram.org/bot{}/deleteMessage?chat_id={}&message_id={}",
                        self.token.read().unwrap_or_else(|e| e.into_inner()),
                        it.chat_id,
                        it.msg_id
                    )
                    .as_str(),
                )
                .await?;
            let ret_json: serde_json::Value = serde_json::from_str(&ret_text)?;
            let result = ret_json
                .get("result")
                .ok_or("result not found")?
                .as_bool()
                .ok_or("result not bool")?;
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
    async fn deal_ob_get_login_info(
        &self,
        _params: &serde_json::Value,
        echo: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let ret_text = self
            .proxyrequest(
                format!(
                    "https://api.telegram.org/bot{}/getMe",
                    self.token.read().unwrap_or_else(|e| e.into_inner())
                )
                .as_str(),
            )
            .await?;
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
        !self.is_stop.load(std::sync::atomic::Ordering::Relaxed)
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
        // 先重置再赋值，避免二次 connect 时累积双份数据
        *self.proxy.write().unwrap_or_else(|e| e.into_inner()) = proxy.to_owned();
        self.allowed_updates = match config_json.get("AllowedUpdates") {
            Some(updates) => serde_json::from_value(updates.clone())
                .map_err(|_| "Telegram AllowedUpdates 必须是字符串数组")?,
            None => Vec::new(),
        };
        let ret_text = self
            .proxyrequest(format!("https://api.telegram.org/bot{}/getMe", token).as_str())
            .await?;
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
        self.add_username2userid(&format!("@{username}"), self_id.parse()?);
        *self.self_id.write().unwrap_or_else(|e| e.into_inner()) = self_id.clone();
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = token.to_owned();
        let _ = cq_add_log(&format!("telegram bot {} connect success", self_id));
        let self_t = self.clone();
        tokio::task::spawn(async move {
            match self_t.do_recv_loop().await {
                Ok(_) => {}
                Err(err) => {
                    let _ = cq_add_log(&format!(
                        "telegram bot {} recv error:{}",
                        self_t.self_id.read().unwrap_or_else(|e| e.into_inner()),
                        err
                    ));
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
            "telegram_api" => {
                native_api::action_result(self.call_native_api(params, echo).await, echo)
            }
            "send_group_msg" => self.deal_ob_send_group_msg(params, echo).await?,
            "send_private_msg" => self.deal_ob_send_private_msg(params, echo).await?,
            "send_msg" => {
                let group_id = read_json_str(params, "group_id");
                if !group_id.is_empty() {
                    self.deal_ob_send_group_msg(params, echo).await?
                } else {
                    self.deal_ob_send_private_msg(params, echo).await?
                }
            }
            "delete_msg" => self.deal_ob_delete_msg(params, echo).await?,
            "get_login_info" => self.deal_ob_get_login_info(params, echo).await?,
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
        let lk = self.self_id.read().unwrap_or_else(|e| e.into_inner());
        if lk.is_empty() {
            return vec![];
        }
        vec![("telegram".to_owned(), lk.to_owned())]
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn callback_retains_provider_and_redreply_chat_context() {
        let bot = TeleTramConnect::build("");
        let event = json!({"update_id":42,"callback_query":{
            "id":"callback", "data":"next", "from":{"id":7,"first_name":"User"},
            "message":{"message_id":9,"chat":{"id":-100123,"type":"supergroup"}}
        }});
        let notice = bot.native_update("callback_query", &event);
        assert_eq!(notice["sub_type"], "callback_query");
        assert_eq!(notice["group_id"], "-100123");
        assert_eq!(notice["telegram"]["id"], "callback");
        let mapped = bot.get_msg_id(notice["message_id"].as_str().unwrap());
        assert_eq!(mapped[0].msg_id, "9");
        assert_eq!(mapped[0].chat_id, "-100123");
    }

    #[test]
    fn inline_callback_has_no_synthetic_chat() {
        let bot = TeleTramConnect::build("");
        let event = json!({"update_id":43,"callback_query":{
            "id":"callback", "from":{"id":7}, "inline_message_id":"inline"
        }});
        let notice = bot.native_update("callback_query", &event);
        assert!(notice.get("chat_id").is_none());
        assert_eq!(notice["telegram"]["inline_message_id"], "inline");
    }

    #[test]
    fn polling_sets_allowed_updates_even_if_previous_session_filtered_callbacks() {
        let mut bot = TeleTramConnect::build("");
        let url = bot.updates_url(Some(42), 8).unwrap();
        let query = url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(query["allowed_updates"], "[]");
        assert_eq!(query["offset"], "42");
        bot.allowed_updates = vec![
            "message".to_owned(),
            "callback_query".to_owned(),
            "chat_member".to_owned(),
        ];
        let url = bot.updates_url(None, 0).unwrap();
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "allowed_updates")
                .unwrap()
                .1,
            r#"["message","callback_query","chat_member"]"#
        );
    }
}
