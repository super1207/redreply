use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::collections::VecDeque;
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
mod send_options;
use base64::{engine::general_purpose::STANDARD, Engine as _};
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMsgId {
    msg_id: String,
    chat_id: String,
    business_id: String,
}

impl RawMsgId {
    fn from_message(message: &Value, chat_id: &Value, business_id: &str) -> Option<Self> {
        let id = message["message_id"].as_i64().filter(|id| *id > 0)?;
        Some(Self {
            msg_id: id.to_string(),
            chat_id: chat_id.as_i64()?.to_string(),
            business_id: message["business_connection_id"].as_str().unwrap_or(business_id).to_owned(),
        })
    }
}

#[derive(Debug, Clone)]
struct MsgIdPair {
    msg_id: String,
    raw_msg_ids: Vec<RawMsgId>,
}

const MAX_DOWNLOAD_BYTES: usize = 20 * 1024 * 1024;

fn download_url(token: &str, path: &str) -> AdapterResult<reqwest::Url> {
    let mut url = reqwest::Url::parse("https://api.telegram.org")?;
    let mut segments = url
        .path_segments_mut()
        .expect("Telegram base URL supports path segments");
    segments.push("file").push(&format!("bot{token}"));
    for segment in path.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") || segment.contains('\\') {
            return Err("Telegram 返回了无效的下载路径".into());
        }
        segments.push(segment);
    }
    drop(segments);
    Ok(url)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Topic {
    thread: Option<i64>,
    direct: Option<i64>,
}

impl Topic {
    fn from_message(message: &Value) -> Self {
        Self {
            thread: (message["is_topic_message"] == true)
                .then(|| message["message_thread_id"].as_i64())
                .flatten(),
            direct: message["direct_messages_topic"]["topic_id"].as_i64(),
        }
    }

    fn from_params(params: &Value) -> AdapterResult<Self> {
        fn parse(params: &Value, field: &str) -> AdapterResult<Option<i64>> {
            let Some(value) = params.get(field).filter(|value| !value.is_null()) else {
                return Ok(None);
            };
            let id = value
                .as_i64()
                .or_else(|| value.as_str()?.parse().ok())
                .filter(|id| *id > 0)
                .ok_or_else(|| format!("Telegram {field} 必须为正整数"))?;
            Ok(Some(id))
        }
        let thread = parse(params, "message_thread_id")?;
        let direct = parse(params, "direct_messages_topic_id")?;
        if thread.is_some() && direct.is_some() {
            return Err("Telegram 两类话题 ID 不能同时发送".into());
        }
        Ok(Self { thread, direct })
    }

    fn fields(&self) -> impl Iterator<Item = (&'static str, i64)> {
        [
            ("message_thread_id", self.thread),
            ("direct_messages_topic_id", self.direct),
        ]
        .into_iter()
        .filter_map(|(name, id)| Some((name, id?)))
    }

    fn apply(&self, payload: &mut Value) {
        for (field, id) in self.fields() {
            payload[field] = id.into();
        }
    }

}

const SEND_PERMISSIONS: &[&str] = &[
    "can_send_messages", "can_send_audios", "can_send_documents", "can_send_photos",
    "can_send_videos", "can_send_video_notes", "can_send_voice_notes", "can_send_polls",
    "can_send_other_messages", "can_add_web_page_previews",
];
const OTHER_PERMISSIONS: &[&str] = &[
    "can_react_to_messages", "can_edit_tag", "can_change_info", "can_invite_users",
    "can_pin_messages", "can_manage_topics",
];

fn mute_permissions(chat: &Value, member: &Value, muted: bool) -> Value {
    let defaults = &chat["permissions"];
    let current = if member["status"] == "restricted" { member } else { defaults };
    let mut permissions = serde_json::Map::new();
    for &field in SEND_PERMISSIONS {
        permissions.insert(field.to_owned(), Value::Bool(!muted && defaults[field] == true));
    }
    for &field in OTHER_PERMISSIONS {
        let value = current[field].as_bool()
            .or_else(|| match field {
                "can_react_to_messages" => current["can_send_messages"].as_bool(),
                "can_edit_tag" | "can_manage_topics" => current["can_pin_messages"].as_bool(),
                _ => None,
            })
            .unwrap_or(false);
        permissions.insert(field.to_owned(), value.into());
    }
    Value::Object(permissions)
}

fn topic_service_event(message: &Value) -> Option<&'static str> {
    [
        "forum_topic_created", "forum_topic_edited", "forum_topic_closed",
        "forum_topic_reopened", "general_forum_topic_hidden", "general_forum_topic_unhidden",
    ]
    .into_iter()
    .find(|field| message[*field].is_object())
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
        let mut lk = self
            .username2userid
            .write()
            .unwrap_or_else(|e| e.into_inner());
        // 简单防膨胀：超过上限先清空
        if lk.len() > 100_000 {
            lk.clear();
        }
        lk.insert(username.to_owned(), user_id);
    }

    fn get_username2userid(&self, username: &str) -> Option<i64> {
        self.username2userid
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(username)
            .cloned()
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

    async fn native_request(
        &self,
        method: &str,
        payload: &Value,
        files: Option<&Value>,
        timeout: Duration,
        echo: &Value,
    ) -> AdapterResult<Value> {
        let token = self.token.read().unwrap_or_else(|e| e.into_inner()).clone();
        let url = native_api::telegram_url(&token, method)?;
        let client = self
            .request_client_builder()?
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()?;
        let request = native_api::post(client.post(url), payload, files)?;
        let (status, body) = native_api::response(request, "Telegram").await?;
        Ok(native_api::telegram_reply(status, &body, echo))
    }

    async fn call_native_api(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let method = params["method"]
            .as_str()
            .ok_or("缺少 Telegram API 方法名")?;
        let empty = json!({});
        let payload = params.get("body").unwrap_or(&empty);
        let mut reply = self
            .native_request(method, payload, params.get("files"), Duration::from_secs(25), echo)
            .await?;
        let ids = Self::response_message_ids(method, payload, &reply["data"]);
        if !ids.is_empty() {
            let handles: Vec<_> = ids.iter().cloned().map(|raw| self.message_handle(raw)).collect();
            let handle = if handles.len() == 1 {
                handles[0].clone()
            } else {
                self.add_msg_id(&ids)
            };
            reply["redreply_message_id"] = handle.into();
            reply["redreply_message_ids"] = json!(handles);
        }
        Ok(reply)
    }

    fn response_message_ids(method: &str, payload: &Value, result: &Value) -> Vec<RawMsgId> {
        let ids_only = matches!(method, "copyMessage" | "copyMessages" | "forwardMessages");
        let numeric_target = read_json_str(payload, "chat_id").parse::<i64>().ok().map(Value::from);
        let business_id = payload["business_connection_id"].as_str().unwrap_or_default();
        let messages: &[Value] = result.as_array().map_or(std::slice::from_ref(result), Vec::as_slice);
        messages.iter().filter_map(|message| {
            let chat_id = if ids_only { numeric_target.as_ref()? } else { &message["chat"]["id"] };
            RawMsgId::from_message(message, chat_id, business_id)
        }).collect()
    }

    async fn download_file(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let file_id = params["file_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Telegram 下载需要 file_id")?;
        let metadata = self
            .call_native_api(
                &json!({"method": "getFile", "body": {"file_id": file_id}}),
                echo,
            )
            .await?;
        if metadata["retcode"] != 0 {
            return Ok(metadata);
        }
        let file = &metadata["data"];
        let path = file["file_path"]
            .as_str()
            .ok_or("Telegram getFile 未返回 file_path")?;
        let token = self
            .token
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let url = download_url(&token, path)?;
        let limit = MAX_DOWNLOAD_BYTES as u64;
        let limit_error = "Telegram 文件超过 20 MB 下载上限";
        if file["file_size"].as_u64().is_some_and(|size| size > limit) {
            return Err(limit_error.into());
        }
        let mut response = self
            .request_client_builder()?
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(25))
            .build()?
            .get(url)
            .send()
            .await
            .map_err(|error| error.without_url())?;
        if !response.status().is_success() {
            return Ok(native_api::reply(
                response.status().as_u16().into(),
                &Value::Null,
                "Telegram 下载失败",
                echo,
            ));
        }
        if response.content_length().is_some_and(|size| size > limit) {
            return Err(limit_error.into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| error.without_url())?
        {
            if chunk.len() > MAX_DOWNLOAD_BYTES - bytes.len() {
                return Err(limit_error.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(native_api::reply(
            0,
            &json!({
                "file_id": file["file_id"],
                "file_unique_id": file["file_unique_id"],
                "file_size": bytes.len(),
                "base64": STANDARD.encode(&bytes),
            }),
            "",
            echo,
        ))
    }

    async fn react(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let id = read_json_str(params, "message_id");
        let target = self.get_msg_id(&id);
        let Some(message) = target.last() else {
            return Err("Telegram 表情回应需要当前消息或 RedReply 消息 ID".into());
        };
        if !message.business_id.is_empty() {
            return Err("Telegram 业务消息不支持此表情操作".into());
        }
        let (method, body) = Self::reaction_request(message, params)?;
        self.native_request(method, &body, None, Duration::from_secs(25), echo).await
    }

    fn reaction_request(message: &RawMsgId, params: &Value) -> AdapterResult<(&'static str, Value)> {
        let mut body = json!({
            "chat_id":message.chat_id.parse::<i64>()?,
            "message_id":message.msg_id.parse::<i64>()?,
        });
        let remove = params["remove"] == true;
        let user = read_json_str(params, "user_id");
        let actor = read_json_str(params, "actor_chat_id");
        if !user.is_empty() || !actor.is_empty() {
            if !remove || (!user.is_empty() && !actor.is_empty()) {
                return Err("删除他人回应时需指定一个用户或聊天 ID".into());
            }
            if !user.is_empty() {
                body["user_id"] = user.parse::<i64>()?.into();
            } else {
                body["actor_chat_id"] = actor.parse::<i64>()?.into();
            }
            return Ok(("deleteMessageReaction", body));
        }
        let legacy = read_json_str(params, "emoji_id");
        let emoji = params["emoji"].as_str().unwrap_or(&legacy);
        body["reaction"] = if remove || emoji.is_empty() {
            json!([])
        } else if params["custom"] == true {
            json!([{"type":"custom_emoji","custom_emoji_id":emoji}])
        } else {
            let decoded = emoji.parse::<u32>().ok().and_then(char::from_u32);
            let mut text = [0; 4];
            let emoji = decoded.map(|ch| ch.encode_utf8(&mut text) as &str).unwrap_or(emoji);
            json!([{"type":"emoji","emoji":emoji}])
        };
        if !remove && params["is_big"] == true {
            body["is_big"] = true.into();
        }
        Ok(("setMessageReaction", body))
    }

    fn updates_url(&self, offset: Option<i64>, timeout: u64) -> AdapterResult<reqwest::Url> {
        let mut url = native_api::telegram_url(
            &self.token.read().unwrap_or_else(|e| e.into_inner()),
            "getUpdates",
        )?;
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
        self.notice_for_update(kind, &event[kind], &event["update_id"])
    }

    fn notice_for_update(&self, kind: &str, data: &Value, update_id: &Value) -> Value {
        let self_id = self
            .self_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let chat = data.get("chat").unwrap_or(&data["message"]["chat"]);
        let from = data
            .get("from")
            .or_else(|| data.get("user"))
            .unwrap_or(&Value::Null);
        let mut notice = json!({
            "time": timestamp,
            "platform": "telegram",
            "self_id": self_id,
            "post_type": "notice",
            "notice_type": "telegram",
            "sub_type": kind,
            "update_id": update_id,
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
        if let Some(id) = data["actor_chat"]["id"].as_i64() {
            notice["actor_chat_id"] = id.to_string().into();
        }
        let message = data.get("message").unwrap_or(data);
        Topic::from_message(message).apply(&mut notice);
        if let Some(chat_id) = chat["id"].as_i64() {
            let chat_id = chat_id.to_string();
            notice["chat_id"] = chat_id.clone().into();
            if let Some(chat_type) = chat["type"].as_str() {
                notice["chat_type"] = chat_type.into();
                if chat_type != "private" {
                    notice["group_id"] = chat_id.clone().into();
                }
            }
            if message["ephemeral_message_id"].is_null() {
                if let Some(raw) = RawMsgId::from_message(message, &chat["id"], "") {
                    notice["message_id"] = self.message_handle(raw).into();
                }
            }
            if kind == "deleted_business_messages" {
                if let (Some(business_id), Some(ids)) = (
                    data["business_connection_id"].as_str(), data["message_ids"].as_array(),
                ) {
                    let handles: Vec<_> = ids.iter().filter_map(|id| {
                        let raw = RawMsgId::from_message(&json!({"message_id":id}), &chat["id"], business_id)?;
                        Some(self.message_handle(raw))
                    }).collect();
                    notice["message_ids"] = json!(handles);
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
                reply_id = self
                    .get_cq_msg_id(&read_json_str(&message_obj["chat"], "id"), &raw_reply_id)
                    .unwrap_or(raw_reply_id);
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
        if let Some(file_id) = message_obj["sticker"]["file_id"].as_str() {
            let sticker = json!({"file_id":file_id});
            ret_cq_msg.push_str(&format!("[CQ:sticker,content={}]", cq_params_encode(&sticker.to_string())));
        }
        for kind in ["voice", "video_note", "animation"] {
            if let Some(file_id) = message_obj[kind]["file_id"].as_str() {
                // Keep the caption as visible text in the ordinary message as before.
                let media = json!({"kind":kind,"body":{kind:file_id},"files":{}});
                ret_cq_msg.push_str(&format!("[CQ:telegram_media,content={}]", cq_params_encode(&media.to_string())));
                break;
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
        let self_id = self
            .self_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let message_obj = &event["message"];

        if !username.is_empty() {
            self.add_username2userid(&format!("@{username}"), user_id.parse()?);
        }

        let message_id = message_obj["message_id"]
            .as_i64()
            .ok_or("message_id not i64")?
            .to_string();

        // 存msg_id
        let msg_id = self.message_handle(RawMsgId {
            msg_id: message_id.to_owned(),
            chat_id: user_id.to_string(),
            business_id: String::new(),
        });

        let message = self.to_cq_msg(event).await?;

        let mut event_json = serde_json::json!({
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
        Topic::from_message(&event["message"]).apply(&mut event_json);
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
        let msg_id = self.message_handle(RawMsgId {
            msg_id: message_id.to_owned(),
            chat_id: group_id.to_string(),
            business_id: String::new(),
        });

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
        let mut event_json = serde_json::json!({
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
        Topic::from_message(&event["message"]).apply(&mut event_json);
        tokio::task::spawn_blocking(move || {
            let json_str = event_json.to_string();
            let _ = cq_add_log(&format!("TELEGRAM_OB_EVENT:{json_str}"));
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
        Ok(())
    }

    fn album_item(message: &Value) -> Option<Value> {
        let mut item = if let Some(photo) = message["live_photo"].as_object() {
            let still = photo.get("photo").and_then(Value::as_array)
                .or_else(|| message["photo"].as_array())?.last()?["file_id"].as_str()?;
            let video = photo.get("file_id")?.as_str()?;
            json!({"type":"live_photo","media":video,"photo":still})
        } else if let Some(photo) = message["photo"].as_array().and_then(|photos| photos.last()) {
            json!({"type":"photo","media":photo["file_id"].as_str()?})
        } else {
            ["video", "audio", "document"].into_iter().find_map(|kind| {
                message[kind]["file_id"].as_str().map(|file| json!({"type":kind,"media":file}))
            })?
        };
        for field in ["caption", "caption_entities"] {
            if let Some(value) = message.get(field) {
                item[field] = value.clone();
            }
        }
        Some(item)
    }

    fn media_notices(&self, kind: &str, event: &Value) -> Vec<Value> {
        if !matches!(kind, "message" | "channel_post" | "business_message") {
            return Vec::new();
        }
        let message = &event[kind];
        let mut notices = Vec::new();
        for (field, notice_type) in [("media_group_id", "media_group"), ("sticker", "sticker")] {
            if message.get(field).is_some_and(|value| !value.is_null()) {
                let mut notice = self.notice_for_update(notice_type, message, &event["update_id"]);
                notice["telegram_update_type"] = kind.into();
                if let Some(id) = message["media_group_id"].as_str() {
                    notice["media_group_id"] = id.into();
                    if let Some(item) = Self::album_item(message) {
                        notice["media"] = item;
                    }
                }
                notices.push(notice);
            }
        }
        notices
    }

    fn forwarded_notice(&self, kind: &str, event: &Value) -> Option<Value> {
        let message = &event[kind];
        if !matches!(kind, "message" | "channel_post" | "business_message")
            || !message["forward_origin"].is_object()
        {
            return None;
        }
        let mut notice = self.notice_for_update("forwarded_message", message, &event["update_id"]);
        notice["telegram_update_type"] = kind.into();
        Some(notice)
    }

    fn structured_message_notice(&self, kind: &str, event: &Value) -> Option<Value> {
        if !matches!(kind, "message" | "channel_post" | "business_message"
            | "edited_message" | "edited_channel_post" | "edited_business_message") {
            return None;
        }
        let message = &event[kind];
        let service = if kind.starts_with("edited_") {
            [("location", "edited_location"), ("voice", "edited_voice"),
             ("video_note", "edited_video_note"), ("animation", "edited_animation")]
                .into_iter().find_map(|(field, notice)| message[field].is_object().then_some(notice))?
        } else {
            ["suggested_post_approved", "suggested_post_approval_failed", "suggested_post_declined",
             "suggested_post_paid", "suggested_post_refunded", "suggested_post_info",
             "successful_payment", "refunded_payment", "venue", "location", "contact",
             "voice", "video_note", "animation"].into_iter()
                .find(|key| message[*key].is_object())?
        };
        let mut notice = self.notice_for_update(service, message, &event["update_id"]);
        notice["telegram_update_type"] = kind.into();
        Some(notice)
    }

    fn dispatch_notice(notice: Value) {
        tokio::task::spawn_blocking(move || {
            if let Err(error) = crate::cqevent::do_1207_event(&notice.to_string()) {
                let _ = cq_add_log_w(&format!("Telegram 原生事件处理失败：{error}"));
            }
        });
    }

    async fn deal_event(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_chat_type = event["message"]["chat"]["type"].as_str();
        for (field, notice) in [("message", "ephemeral_message"), ("edited_message", "edited_ephemeral_message")] {
            if event[field]["ephemeral_message_id"].is_number() {
                Self::dispatch_notice(self.notice_for_update(notice, &event[field], &event["update_id"]));
                return Ok(());
            }
        }
        let ordinary_message =
            matches!(message_chat_type, Some("private" | "group" | "supergroup"));
        let update = event
            .as_object()
            .and_then(|fields| fields.keys().find(|key| key.as_str() != "update_id"));
        let notice = if ordinary_message {
            let message = &event["message"];
            topic_service_event(message).map(|kind| {
                self.notice_for_update(kind, message, &event["update_id"])
            })
        } else {
            update.map(|kind| self.native_update(kind, event))
        };
        if let Some(notice) = notice {
            Self::dispatch_notice(notice);
        }
        if let Some(notice) = update.and_then(|kind| self.forwarded_notice(kind, event)) {
            Self::dispatch_notice(notice);
        }
        if let Some(kind) = update {
            if let Some(notice) = self.structured_message_notice(kind,event) {
                Self::dispatch_notice(notice);
            }
            for notice in self.media_notices(kind, event) {
                Self::dispatch_notice(notice);
            }
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
            if let Some(value) = j.as_bool() {
                value
            } else if let Some(value) = j.as_str() {
                value == "true"
            } else {
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
        if let Some(value) = val.as_i64() {
            return value.to_string();
        }
        if let Some(value) = val.as_u64() {
            return value.to_string();
        }
        if let Some(value) = val.as_str() {
            return value.to_string();
        }
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
    fn get_cq_msg_id(&self, chat_id: &str, raw_msg_id: &str) -> Option<String> {
        let lk = self.msg_ids.read().unwrap_or_else(|e| e.into_inner());
        for msg_id_pair in lk.iter().rev() {
            for raw_msg in &msg_id_pair.raw_msg_ids {
                if raw_msg.business_id.is_empty() && raw_msg.chat_id == chat_id && raw_msg.msg_id == raw_msg_id {
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
        is_group: bool,
        business_id: &str,
    ) -> Result<(Vec<(&str, String, String)>, String), Box<dyn std::error::Error + Send + Sync>>
    {
        // 每个元素为 (类型, 内容, caption)；caption 仅对图片类型有意义
        let mut to_send_data: Vec<(&str, String, String)> = vec![];
        let mut quote = String::new();
        let mut last_type = "text";
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
                    if let Some(last) = to_send_data.last_mut() {
                        last.1.push_str(&s);
                    }
                } else {
                    to_send_data.push(("text", s, String::new()));
                    last_type = "text";
                }
            } else if matches!(tp.as_str(),Some("sticker" | "media_group" | "telegram_checklist" | "telegram_invoice"
                | "telegram_paid_media" | "telegram_location" | "telegram_venue" | "telegram_contact"
                | "telegram_media")) {
                let content = it["data"]["content"].as_str().ok_or("Telegram 媒体消息缺少内容")?;
                let kind = match tp.as_str() {
                    Some("sticker") => "sticker",
                    Some("media_group") => "media_group",
                    Some("telegram_invoice") => "telegram_invoice",
                    Some("telegram_paid_media") => "telegram_paid_media",
                    Some("telegram_location") => "telegram_location",
                    Some("telegram_venue") => "telegram_venue",
                    Some("telegram_contact") => "telegram_contact",
                    Some("telegram_media") => "telegram_media",
                    _ => "telegram_checklist",
                };
                to_send_data.push((kind, content.to_owned(), String::new()));
                last_type = kind;
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
                    Some(raw) if raw.chat_id == group_id && raw.business_id == business_id => raw.msg_id.to_owned(),
                    Some(_) => return Err("Telegram 引用消息不属于目标聊天或商务连接".into()),
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
                    if let Some(last) = to_send_data.last_mut() {
                        last.1.push_str(at_str);
                    }
                } else {
                    to_send_data.push(("text", at_str.to_owned(), String::new()));
                    last_type = "text";
                }
            } else if matches!(tp.as_str(), Some("file" | "video")) {
                let data = &it["data"];
                let file = read_json_str(data, "file");
                let filename = read_json_str(data, "name");
                if file.is_empty() {
                    return Err("Telegram 媒体缺少 file".into());
                }
                let media_type = if tp == "file" { "document" } else { "video" };
                to_send_data.push((media_type, file, filename));
                last_type = media_type;
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
    fn message_handle(&self, raw: RawMsgId) -> String {
        let mut ids = self.msg_ids.write().unwrap_or_else(|e| e.into_inner());
        if let Some(pair) = ids.iter().rev().find(|pair| pair.raw_msg_ids.as_slice() == std::slice::from_ref(&raw)) {
            return pair.msg_id.clone();
        }
        Self::store_msg_id(&mut ids, vec![raw])
    }

    fn add_msg_id(&self, raw_msg_ids: &[RawMsgId]) -> String {
        let mut ids = self.msg_ids.write().unwrap_or_else(|e| e.into_inner());
        Self::store_msg_id(&mut ids, raw_msg_ids.to_vec())
    }

    fn store_msg_id(ids: &mut VecDeque<MsgIdPair>, raw_msg_ids: Vec<RawMsgId>) -> String {
        let msg_id = Uuid::new_v4().to_string();
        ids.push_back(MsgIdPair { msg_id: msg_id.clone(), raw_msg_ids });
        if ids.len() > 9999 {
            ids.pop_front();
        }
        msg_id
    }

    fn message_payload(
        chat_id: &str,
        topic: &Topic,
        quote: Option<&str>,
        reply_markup: Option<&Value>,
    ) -> AdapterResult<Value> {
        let mut payload = json!({});
        if !chat_id.is_empty() {
            payload["chat_id"] = chat_id.parse::<i64>()?.into();
        }
        topic.apply(&mut payload);
        if let Some(id) = quote.filter(|id| !id.is_empty()) {
            payload["reply_parameters"] = json!({"message_id": id.parse::<i64>()?});
        }
        if let Some(markup) = reply_markup {
            payload["reply_markup"] = markup.clone();
        }
        Ok(payload)
    }

    fn album_items(group: &Value) -> AdapterResult<&[Value]> {
        let items = group["media"].as_array().ok_or("Telegram 媒体组需要数组")?;
        if !(2..=10).contains(&items.len()) {
            return Err("Telegram 媒体组需要 2 至 10 项".into());
        }
        let first = items[0]["type"].as_str().unwrap_or_default();
        for item in items {
            let kind = item["type"].as_str().unwrap_or_default();
            let compatible = match first {
                "audio" | "document" => kind == first,
                _ => matches!(kind, "photo" | "video" | "live_photo"),
            };
            if !compatible || item["media"].as_str().is_none_or(str::is_empty) {
                return Err("Telegram 媒体组类型不兼容或缺少 media".into());
            }
            if kind == "live_photo" && item["photo"].as_str().is_none_or(str::is_empty) {
                return Err("Telegram 实况照片需要 photo 和 media".into());
            }
        }
        Ok(items)
    }

    fn native_media(data: &Value, payload: &mut Value) -> AdapterResult<(&'static str, Value)> {
        let (method, required, fields): (_, _, &[&str]) = match data["kind"].as_str() {
            Some("voice") => ("sendVoice", "voice", &["duration", "caption", "parse_mode"]),
            Some("video_note") => ("sendVideoNote", "video_note", &["duration", "length", "thumbnail"]),
            Some("animation") => ("sendAnimation", "animation", &[
                "duration", "width", "height", "thumbnail", "caption", "parse_mode",
                "show_caption_above_media", "has_spoiler",
            ]),
            _ => return Err("Telegram 媒体消息类型无效".into()),
        };
        let body = &data["body"];
        let source = body[required].as_str().filter(|source| !source.is_empty())
            .ok_or("Telegram 媒体消息缺少来源")?;
        let uploads = native_api::object(&data["files"])?;
        if let Some(part) = source.strip_prefix("attach://") {
            if part != "media" || !uploads.contains_key("media") {
                return Err("Telegram 媒体消息缺少上传文件".into());
            }
        } else if uploads.contains_key("media") || (required == "video_note" && source.contains("://")) {
            return Err("Telegram 媒体消息来源无效".into());
        }
        if uploads.keys().any(|key| key != "media" && key != "preview")
            || uploads.contains_key("preview") != body.get("thumbnail").is_some()
            || (uploads.contains_key("preview") &&
                (!uploads.contains_key("media") || !fields.contains(&"thumbnail") ||
                 body["thumbnail"] != "attach://preview"))
        {
            return Err("Telegram 媒体消息的附件无效".into());
        }
        payload[required] = source.into();
        for field in fields {
            if let Some(value) = body.get(*field) {
                payload[*field] = value.clone();
            }
        }
        Ok((method, data["files"].clone()))
    }

    async fn send_message_inner(
        &self,
        to_send_data: Vec<(&str, String, String)>,
        quote: String,
        chat_id: &str,
        topic: &Topic,
        reply_markup: Option<&Value>,
        business_id: &str,
        options: &Value,
    ) -> AdapterResult<String> {
        let private = chat_id.parse::<i64>().is_ok_and(|id| id > 0);
        send_options::validate(options, &to_send_data, topic, private, business_id, reply_markup)?;
        if chat_id.is_empty() && !(options["ephemeral_message_parameters"]["callback_query_id"].is_string()
            && options["ephemeral_message_parameters"]["receiver_user_id"].is_null())
        {
            return Err("Telegram 缺少目标会话".into());
        }
        if let Some(markup) = reply_markup {
            native_api::object(markup)?;
            if to_send_data.is_empty() {
                return Err("Telegram 键盘需要可发送的消息".into());
            }
        }
        let mut msg_ids = Vec::with_capacity(to_send_data.len());
        let last = to_send_data.len().saturating_sub(1);
        if reply_markup.is_some() && to_send_data.last().is_some_and(|part| part.0 == "media_group") {
            return Err("Telegram 媒体组不支持附加键盘".into());
        }
        if business_id.is_empty() && to_send_data.iter().any(|part| part.0 == "telegram_checklist") {
            return Err("Telegram 清单需要商务连接".into());
        }
        if !business_id.is_empty() && reply_markup.is_some_and(|markup| !markup["inline_keyboard"].is_array()) {
            return Err("Telegram 商务消息的键盘需要使用行内模式".into());
        }
        if *topic != Topic::default() && to_send_data.iter().any(|part| part.0 == "telegram_checklist") {
            return Err("Telegram 商务清单没有话题参数".into());
        }
        if !business_id.is_empty() && to_send_data.iter().any(|part| part.0 == "telegram_invoice") {
            return Err("发送发票需使用机器人身份，商务账户可创建星币发票链接".into());
        }
        if let Some(markup) = reply_markup {
            let first = &markup["inline_keyboard"][0][0]["pay"];
            let pays = markup["inline_keyboard"].as_array().into_iter().flatten()
                .flat_map(|row| row.as_array().into_iter().flatten()).filter(|button| button["pay"] == true).count();
            let invoice = to_send_data.last().is_some_and(|part| part.0 == "telegram_invoice");
            if (invoice && (first != &Value::Bool(true) || pays != 1)) || (!invoice && pays != 0) {
                return Err("自定义支付键盘只能用于发票，且首个按钮必须是唯一的支付按钮".into());
            }
        }
        for (index, (kind, message, caption)) in to_send_data.into_iter().enumerate() {
            let mut payload = Self::message_payload(
                chat_id,
                topic,
                (index == 0).then_some(quote.as_str()),
                reply_markup.filter(|_| index == last),
            )?;
            if !business_id.is_empty() {
                payload["business_connection_id"] = business_id.into();
            }
            let mut files = json!({});
            let method = match kind {
                "text" => {
                    payload["text"] = message.into();
                    "sendMessage"
                }
                "sticker" => {
                    let sticker: Value = serde_json::from_str(&message)?;
                    if let Some(upload) = sticker.get("upload") {
                        files["sticker"] = upload.clone();
                        if let Some(emoji) = sticker["emoji"].as_str() {
                            payload["emoji"] = emoji.into();
                        }
                    } else {
                        payload["sticker"] = sticker["file_id"].as_str().ok_or("Telegram 贴纸缺少 file_id 或上传文件")?.into();
                    }
                    "sendSticker"
                }
                "media_group" => {
                    let group: Value = serde_json::from_str(&message)?;
                    let media = Self::album_items(&group)?;
                    payload["media"] = Value::Array(media.to_vec());
                    if let Some(uploads) = group.get("files") {
                        files = uploads.clone();
                    }
                    "sendMediaGroup"
                }
                "telegram_checklist" => {
                    payload["checklist"] = serde_json::from_str(&message)?;
                    "sendChecklist"
                }
                "telegram_invoice" => {
                    let invoice: Value = serde_json::from_str(&message)?;
                    for field in ["title","description","payload","provider_token","currency","prices","max_tip_amount",
                        "suggested_tip_amounts","provider_data","photo_url","photo_size","photo_width","photo_height",
                        "need_name","need_phone_number","need_email","need_shipping_address","send_phone_number_to_provider",
                        "send_email_to_provider","is_flexible","start_parameter"] {
                        if let Some(value) = invoice.get(field) {
                            payload[field] = value.clone();
                        }
                    }
                    "sendInvoice"
                }
                "telegram_paid_media" => {
                    let data: Value = serde_json::from_str(&message)?;
                    for field in ["star_count","media","payload","caption","show_caption_above_media"] {
                        if let Some(value) = data["body"].get(field) {
                            payload[field] = value.clone();
                        }
                    }
                    files = data["files"].clone();
                    "sendPaidMedia"
                }
                "telegram_media" => {
                    let data = serde_json::from_str(&message)?;
                    let (method, uploads) = Self::native_media(&data, &mut payload)?;
                    files = uploads;
                    method
                }
                "telegram_location" | "telegram_venue" | "telegram_contact" => {
                    let details: Value = serde_json::from_str(&message)?;
                    let (method,fields): (&str,&[&str]) = match kind {
                        "telegram_location" => ("sendLocation",&["latitude","longitude","horizontal_accuracy","live_period","heading","proximity_alert_radius"]),
                        "telegram_venue" => ("sendVenue",&["latitude","longitude","title","address","foursquare_id","foursquare_type","google_place_id","google_place_type"]),
                        _ => ("sendContact",&["phone_number","first_name","last_name","vcard"]),
                    };
                    for field in fields {
                        if let Some(value) = details.get(*field) {
                            payload[*field] = value.clone();
                        }
                    }
                    method
                }
                "url_image" | "file_image" | "url_record" | "file_record" | "document" | "video" => {
                    let (method, field, filename) = match kind {
                        "url_image" | "file_image" => ("sendPhoto", "photo", "redpic"),
                        "url_record" | "file_record" => ("sendAudio", "audio", "语音"),
                        "document" => ("sendDocument", "document", "file"),
                        _ => ("sendVideo", "video", "video.mp4"),
                    };
                    if kind == "file_image" || kind == "file_record" {
                        files[field] = json!({"filename":filename,"base64":message});
                    } else if let Some(base64) = message.strip_prefix("base64://") {
                        let name = if caption.is_empty() { filename } else { &caption };
                        files[field] = json!({"filename":name,"base64":base64});
                    } else {
                        payload[field] = message.into();
                    }
                    if matches!(kind, "url_image" | "file_image" | "url_record") && !caption.is_empty() {
                        payload["caption"] = caption.into();
                    }
                    method
                }
                "music" => {
                    let music: Value = serde_json::from_str(&message)?;
                    let title = read_json_str(&music, "title");
                    let image = read_json_str(&music, "image");
                    let audio = read_json_str(&music, "audio");
                    let client = self.get_req_client()?;
                    let encoder = base64::engine::general_purpose::STANDARD;
                    if !image.is_empty() {
                        let bytes = client.get(image).send().await?.bytes().await?;
                        files["thumbnail"] = json!({"filename":"image.jpg","base64":encoder.encode(bytes)});
                    }
                    let bytes = client.get(audio).send().await?.bytes().await?;
                    let filename = if title.is_empty() { "audio" } else { &title };
                    files["audio"] = json!({"filename":filename,"base64":encoder.encode(bytes)});
                    for (key, value) in [
                        ("title", title),
                        ("performer", read_json_str(&music, "content")),
                        ("caption", read_json_str(&music, "url")),
                    ] {
                        if !value.is_empty() {
                            payload[key] = value.into();
                        }
                    }
                    "sendAudio"
                }
                _ => return Err(format!("Telegram 不支持的消息类型：{kind}").into()),
            };
            send_options::apply(options, &mut payload);
            let reply = self
                .native_request(method, &payload, Some(&files), Duration::from_secs(60), &Value::Null)
                .await?;
            if reply["retcode"] != 0 {
                return Err(format!("Telegram {method}: {}", reply["message"]).into());
            }
            if options.get("ephemeral_message_parameters").is_some() {
                return Ok(reply["data"]["ephemeral_message_id"].as_i64().filter(|id| *id > 0)
                    .ok_or("Telegram 未返回临时消息ID")?.to_string());
            }
            let returned = Self::response_message_ids(method, &payload, &reply["data"]);
            if returned.is_empty() {
                return Err("Telegram 未返回 message_id".into());
            }
            if returned.len() > 1 {
                for raw in &returned {
                    self.message_handle(raw.clone());
                }
            }
            msg_ids.extend(returned);
        }
        if let [message] = msg_ids.as_slice() {
            return Ok(self.message_handle(message.clone()));
        }
        Ok(self.add_msg_id(&msg_ids))
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

        let (to_send_data, quote) = self
            .make_telegram_msg(&message_arr, &group_id, true, "")
            .await?;

        let msg_id = self
            .send_message_inner(
                to_send_data,
                quote,
                &group_id,
                &Topic::from_params(params)?,
                params.get("reply_markup"),
                "",
                &params["send_options"],
            )
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
        let business_id = params["business_connection_id"].as_str().unwrap_or_default();
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

        let (to_send_data, quote) = self
            .make_telegram_msg(&message_arr, &user_id, false, business_id)
            .await?;

        let msg_id = self
            .send_message_inner(
                to_send_data,
                quote,
                &user_id,
                &Topic::from_params(params)?,
                params.get("reply_markup"),
                business_id,
                &params["send_options"],
            )
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

    async fn deal_ob_delete_msg(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let id = read_json_str(params, "message_id");
        let messages = self.get_msg_id(&id);
        if messages.is_empty() {
            return Ok(native_api::reply(1404, &Value::Null, "Telegram 找不到 RedReply 消息 ID", echo));
        }
        if let [message] = messages.as_slice() {
            if message.business_id.is_empty() {
                return self.native_request(
                    "deleteMessage",
                    &json!({"chat_id":message.chat_id.parse::<i64>()?,"message_id":message.msg_id.parse::<i64>()?}),
                    None, Duration::from_secs(25), echo,
                ).await;
            }
        }
        self.delete_messages(&[id.as_str()], echo).await
    }

    fn message_batches(
        &self,
        ids: &[&str],
    ) -> AdapterResult<(BTreeMap<(i64, String), BTreeSet<i64>>, Vec<String>)> {
        let mut by_chat = BTreeMap::<_, BTreeSet<_>>::new();
        let mut missing = Vec::new();
        for id in ids.iter().copied().collect::<BTreeSet<_>>() {
            let messages = self.get_msg_id(id);
            if messages.is_empty() {
                missing.push(id.to_owned());
            }
            for raw in messages {
                by_chat.entry((raw.chat_id.parse::<i64>()?, raw.business_id))
                    .or_default()
                    .insert(raw.msg_id.parse::<i64>()?);
            }
        }
        Ok((by_chat, missing))
    }

    async fn delete_messages(&self, ids: &[&str], echo: &Value) -> AdapterResult<Value> {
        let (groups, missing) = self.message_batches(ids)?;
        let mut batch = native_api::DeleteBatch::default();
        for id in missing {
            batch.record(json!({"message_id":id}), Err("Telegram 找不到 RedReply 消息 ID".into()));
        }
        for ((chat_id, business_id), ids) in groups {
            let ids: Vec<_> = ids.into_iter().collect();
            for chunk in ids.chunks(100) {
                let mut body = json!({"message_ids":chunk});
                let method = if business_id.is_empty() {
                    body["chat_id"] = chat_id.into();
                    "deleteMessages"
                } else {
                    body["business_connection_id"] = business_id.as_str().into();
                    "deleteBusinessMessages"
                };
                let result = self.native_request(method, &body, None, Duration::from_secs(25), echo).await;
                batch.record(json!({"chat_id":chat_id,"message_ids":chunk}), result);
            }
        }
        Ok(batch.finish(echo))
    }

    async fn edit_message(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let messages = self.get_msg_id(&read_json_str(params, "message_id"));
        let [raw] = messages.as_slice() else {
            return Err("编辑需要指向一条 Telegram 消息的 RedReply ID".into());
        };
        let method = match params["kind"].as_str() {
            Some("text") => "editMessageText",
            Some("caption") => "editMessageCaption",
            Some("keyboard") => "editMessageReplyMarkup",
            Some("media") => "editMessageMedia",
            Some("live_location") => "editMessageLiveLocation",
            Some("stop_location") => "stopMessageLiveLocation",
            Some("checklist") if !raw.business_id.is_empty() => "editMessageChecklist",
            Some("checklist") => return Err("Telegram 清单编辑需要商务消息".into()),
            _ => return Err("未知的 Telegram 消息编辑类型".into()),
        };
        let mut body = native_api::object(&params["body"])?.clone();
        if body.contains_key("inline_message_id") {
            return Err("行内消息请通过 Telegram调用 指定 inline_message_id".into());
        }
        body.insert("chat_id".to_owned(), raw.chat_id.parse::<i64>()?.into());
        body.insert("message_id".to_owned(), raw.msg_id.parse::<i64>()?.into());
        if !raw.business_id.is_empty() {
            body.insert("business_connection_id".to_owned(), raw.business_id.as_str().into());
        } else {
            body.remove("business_connection_id");
        }
        self.native_request(method, &Value::Object(body), params.get("files"), Duration::from_secs(25), echo).await
    }

    async fn forward_messages(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let (method, body) = self.forward_request(params)?;
        self.call_native_api(&json!({"method":method,"body":body}), echo).await
    }

    fn forward_request(&self, params: &Value) -> AdapterResult<(&'static str, Value)> {
        let ids = native_api::message_handles(params)?;
        let (groups, missing) = self.message_batches(&ids)?;
        if !missing.is_empty() || groups.len() != 1 {
            return Err("Telegram 转发需要同一聊天中已知的 RedReply 消息 ID".into());
        }
        let Some(((source, business_id), ids)) = groups.into_iter().next() else {
            return Err("Telegram 没有可转发的消息".into());
        };
        if !business_id.is_empty() || ids.len() > 100 {
            return Err("Telegram 转发支持同一普通聊天内的 1 至 100 条消息".into());
        }
        let target = read_json_str(params, "target_id");
        let chat_id = match target.parse::<i64>() {
            Ok(id) => id.into(),
            Err(_) if target.starts_with('@') && target.len() > 1 => Value::String(target),
            Err(_) => return Err("Telegram 转发目标需为聊天 ID 或用户名".into()),
        };
        let mut body = json!({"chat_id":chat_id,"from_chat_id":source});
        Topic::from_params(params)?.apply(&mut body);
        if params["silent"] == true {
            body["disable_notification"] = true.into();
        }
        let copy = params["copy"] == true;
        let method = if let Some(id) = ids.first().filter(|_| ids.len() == 1) {
            body["message_id"] = (*id).into();
            if copy { "copyMessage" } else { "forwardMessage" }
        } else {
            body["message_ids"] = json!(ids);
            if copy { "copyMessages" } else { "forwardMessages" }
        };
        let options = &params["send_options"];
        if options.get("ephemeral_message_parameters").is_some()
            || options.get("message_effect_id").is_some() && (ids.len() != 1 || !body["chat_id"].as_i64().is_some_and(|id| id > 0))
            || options.get("suggested_post_parameters").is_some() && (ids.len() != 1 || !body["direct_messages_topic_id"].is_number())
        {
            return Err("转发的消息效果只适用于单条私聊消息，建议帖只适用于单条频道私信".into());
        }
        send_options::apply(options, &mut body);
        Ok((method, body))
    }

    fn message_target(message: &RawMsgId) -> AdapterResult<Value> {
        let mut body = json!({
            "chat_id":message.chat_id.parse::<i64>()?,
            "message_id":message.msg_id.parse::<i64>()?,
        });
        if !message.business_id.is_empty() {
            body["business_connection_id"] = message.business_id.as_str().into();
        }
        Ok(body)
    }

    async fn pin_message(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let id = read_json_str(params, "message_id");
        let messages = self.get_msg_id(&id);
        let message = messages.last().ok_or("Telegram 找不到 RedReply 消息 ID")?;
        let method = if params["remove"] == true { "unpinChatMessage" } else { "pinChatMessage" };
        let mut body = Self::message_target(message)?;
        if params["remove"] != true && params["silent"] == true {
            body["disable_notification"] = true.into();
        }
        self.native_request(method, &body, None, Duration::from_secs(25), echo).await
    }

    async fn read_business_message(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let messages = self.get_msg_id(&read_json_str(params, "message_id"));
        let [message] = messages.as_slice() else {
            return Err("请指定一条 Telegram 商务消息的 RedReply 消息 ID".into());
        };
        if message.business_id.is_empty() {
            return Err("标记商务消息已读需要商务连接".into());
        }
        self.native_request("readBusinessMessage", &Self::message_target(message)?, None, Duration::from_secs(25), echo).await
    }

    async fn set_group_ban(&self, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let chat_id = read_json_str(params, "group_id").parse::<i64>()?;
        let user_id = read_json_str(params, "user_id").parse::<i64>()?;
        let duration = params["duration"].as_u64().ok_or("Telegram 禁言时间需要非负整数")?;
        if duration != 0 && !(30..=366 * 86400).contains(&duration) {
            return Err("Telegram 定时禁言支持 30 秒至 366 天，0 表示解除禁言".into());
        }

        let timeout = Duration::from_secs(8);
        let chat_request = json!({"chat_id":chat_id});
        let member_request = json!({"chat_id":chat_id,"user_id":user_id});
        let (chat, member) = tokio::try_join!(
            self.native_request("getChat", &chat_request, None, timeout, echo),
            self.native_request("getChatMember", &member_request, None, timeout, echo),
        )?;
        if chat["retcode"] != 0 {
            return Ok(chat);
        }
        if member["retcode"] != 0 {
            return Ok(member);
        }
        let chat = &chat["data"];
        let member = &member["data"];
        if chat["type"] != "supergroup" || !chat["permissions"].is_object() {
            return Err("Telegram 禁言需要能读取默认权限的超级群".into());
        }
        if duration == 0 && member["status"] != "restricted" {
            return Ok(native_api::reply(0, &Value::Bool(true), "", echo));
        }
        if duration != 0 && member["status"] != "member" && member["status"] != "restricted" {
            return Err("Telegram 禁言只适用于当前群的普通成员".into());
        }
        if member["status"] == "restricted" && member["is_member"] != true {
            return Err("目标用户已不在 Telegram 群中".into());
        }
        let mut body = json!({
            "chat_id": chat_id, "user_id": user_id, "use_independent_chat_permissions": true,
            "permissions": mute_permissions(chat, member, duration != 0),
        });
        if duration != 0 {
            // Measure from the write, after fetching the current permissions.
            body["until_date"] = (chrono::Utc::now().timestamp() + duration as i64).into();
        }
        self.native_request("restrictChatMember", &body, None, timeout, echo).await
    }

    async fn chat_info(&self, action: &str, params: &Value, echo: &Value) -> AdapterResult<Value> {
        let group_id = read_json_str(params, "group_id").parse::<i64>()?;
        let mut body = json!({"chat_id":group_id});
        let member = action == "get_group_member_info";
        if member {
            body["user_id"] = read_json_str(params, "user_id").parse::<i64>()?.into();
        }
        let method = if member { "getChatMember" } else { "getChat" };
        let mut result = self
            .call_native_api(&json!({"method":method,"body":body}), echo)
            .await?;
        if result["retcode"] == 0 {
            let data = &result["data"];
            let mapped = if member {
                let user = &data["user"];
                let name = [
                    user["first_name"].as_str().unwrap_or_default(),
                    user["last_name"].as_str().unwrap_or_default(),
                ]
                .join(" ");
                let role = match data["status"].as_str() {
                    Some("creator") => "owner",
                    Some("administrator") => "admin",
                    _ => "member",
                };
                json!({"group_id":group_id.to_string(),"user_id":read_json_str(user,"id"),
                    "nickname":name.trim(),"card":"","sex":"unknown","age":0,"area":"",
                    "level":"","role":role,"title":data["custom_title"].as_str().unwrap_or_default(),
                    "telegram":data})
            } else {
                json!({"group_id":group_id.to_string(),"group_name":data["title"].as_str().unwrap_or_default(),
                    "telegram":data})
            };
            result["data"] = mapped;
        }
        Ok(result)
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
            "telegram_download" => {
                native_api::action_result(self.download_file(params, echo).await, echo)
            }
            "set_msg_emoji_like" => native_api::action_result(self.react(params, echo).await, echo),
            "telegram_read_business_message" => {
                native_api::action_result(self.read_business_message(params, echo).await, echo)
            }
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
            "delete_msgs" => {
                let result = match native_api::message_handles(params) {
                    Ok(ids) => self.delete_messages(&ids, echo).await,
                    Err(error) => Err(error),
                };
                native_api::action_result(result, echo)
            }
            "telegram_stop_poll" => {
                let result = async {
                    let id = read_json_str(params, "message_id");
                    let messages = self.get_msg_id(&id);
                    let [message] = messages.as_slice() else {
                        return Err("请指定一条 Telegram 投票的 RedReply 消息 ID".into());
                    };
                    let mut body = json!({
                        "chat_id":message.chat_id.parse::<i64>()?,
                        "message_id":message.msg_id.parse::<i64>()?,
                    });
                    if !message.business_id.is_empty() {
                        body["business_connection_id"] = message.business_id.as_str().into();
                    }
                    self.native_request("stopPoll", &body, None, Duration::from_secs(25), echo).await
                }.await;
                native_api::action_result(result, echo)
            }
            "edit_msg" => native_api::action_result(self.edit_message(params, echo).await, echo),
            "forward_msgs" => native_api::action_result(self.forward_messages(params, echo).await, echo),
            "delete_msg" => self.deal_ob_delete_msg(params, echo).await?,
            "get_login_info" => self.deal_ob_get_login_info(params, echo).await?,
            // "get_stranger_info" => {
            //     self.deal_ob_get_stranger_info(&params,&echo).await?
            // },
            "get_group_info" | "get_group_member_info" => {
                native_api::action_result(self.chat_info(action, params, echo).await, echo)
            }
            "set_msg_pin" => {
                native_api::action_result(self.pin_message(params, echo).await, echo)
            }
            "set_group_ban" => {
                native_api::action_result(self.set_group_ban(params, echo).await, echo)
            }
            "set_group_leave" | "set_group_name" => {
                let id = read_json_str(params, "group_id");
                let chat_id = id.parse::<i64>()?;
                let (method, body) = if action == "set_group_name" {
                    let title = read_json_str(params, "group_name");
                    ("setChatTitle", json!({"chat_id":chat_id,"title":title}))
                } else {
                    ("leaveChat", json!({"chat_id":chat_id}))
                };
                native_api::action_result(
                    self.native_request(method, &body, None, Duration::from_secs(25), echo).await,
                    echo,
                )
            }
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
                    "data": {"yes":true},
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
    fn custom_reactions_keep_ids_and_moderation_uses_actor_chat() {
        let raw = RawMsgId::from_message(&json!({"message_id":9}), &json!(-100), "").unwrap();
        let (method, body) = TeleTramConnect::reaction_request(&raw, &json!({
            "custom":true,"emoji":"5421234567890123456","is_big":true
        })).unwrap();
        assert_eq!(method, "setMessageReaction");
        assert_eq!(body["reaction"][0], json!({
            "type":"custom_emoji","custom_emoji_id":"5421234567890123456"
        }));
        assert_eq!(body["is_big"], true);
        let (_, removed) = TeleTramConnect::reaction_request(&raw, &json!({"remove":true})).unwrap();
        assert_eq!(removed["reaction"], json!([]));
        let (method, body) = TeleTramConnect::reaction_request(&raw, &json!({
            "remove":true,"actor_chat_id":"-1"
        })).unwrap();
        assert_eq!(method, "deleteMessageReaction");
        assert_eq!(body["actor_chat_id"], -1);

        let bot = TeleTramConnect::build("");
        let event = json!({"update_id":12,"message_reaction":{
            "chat":{"id":-100,"type":"supergroup"},"message_id":9,"actor_chat":{"id":-1},
            "old_reaction":[],"new_reaction":[{"type":"custom_emoji","custom_emoji_id":"5421234567890123456"}]
        }});
        let notice = bot.native_update("message_reaction", &event);
        assert!(notice.get("user_id").is_none());
        assert_eq!(notice["actor_chat_id"], "-1");
        assert_eq!(bot.get_msg_id(notice["message_id"].as_str().unwrap())[0], raw);
    }

    #[tokio::test]
    async fn album_and_sticker_events_reuse_file_ids_and_message_handles() {
        let bot = TeleTramConnect::build("");
        let media = json!({"media":[
            {"type":"photo","media":"photo-id"},
            {"type":"live_photo","media":"video-id","photo":"still-id"}
        ]});
        assert!(TeleTramConnect::album_items(&media).is_ok());
        let mixed = json!({"media":[{"type":"photo","media":"p"},{"type":"audio","media":"a"}]});
        assert!(TeleTramConnect::album_items(&mixed).is_err());
        let event = json!({"update_id":12,"message":{
            "chat":{"id":-100,"type":"supergroup"},"message_id":9,"media_group_id":"99",
            "live_photo":{"file_id":"video-id"},"photo":[{"file_id":"still-id"}]
        }});
        let album = bot.media_notices("message", &event);
        assert_eq!(album[0]["sub_type"], "media_group");
        assert_eq!(album[0]["media_group_id"], "99");
        assert_eq!(album[0]["media"], media["media"][1]);

        let event = json!({"message":{
            "chat":{"id":-100,"type":"supergroup"},"message_id":10,
            "sticker":{"file_id":"sticker-id","is_animated":true}
        }});
        let notice = bot.media_notices("message", &event);
        assert_eq!(notice[0]["telegram"]["sticker"]["file_id"], "sticker-id");
        let cq = bot.to_cq_msg(&event).await.unwrap();
        let segments = str_msg_to_arr(&json!(cq)).unwrap();
        let (parts, _) = bot.make_telegram_msg(&segments, "-100", true, "").await.unwrap();
        assert_eq!(parts[0].0, "sticker");
        assert_eq!(serde_json::from_str::<Value>(&parts[0].1).unwrap()["file_id"], "sticker-id");
    }

    #[test]
    fn forwarding_sorts_unique_ids_and_respects_chat_boundaries() {
        let bot = TeleTramConnect::build("");
        let handle = |chat, id| bot.message_handle(RawMsgId::from_message(
            &json!({"message_id":id}), &json!(chat), "",
        ).unwrap());
        let first = handle(-100, 9);
        let second = handle(-100, 3);
        let other = handle(-200, 3);
        let input = json!({
            "message_ids":[first,second,first],"target_id":"-400",
            "copy":true,"silent":true,"message_thread_id":8
        });
        let (method, body) = bot.forward_request(&input).unwrap();
        assert_eq!(method, "copyMessages");
        assert_eq!(body, json!({
            "chat_id":-400,"from_chat_id":-100,"message_ids":[3,9],
            "disable_notification":true,"message_thread_id":8
        }));
        assert!(bot.forward_request(&json!({
            "message_ids":[first,other],"target_id":"-400"
        })).is_err());
        assert!(bot.forward_request(&json!({
            "message_ids":[first,"missing"],"target_id":"-400"
        })).is_err());

        let returned = TeleTramConnect::response_message_ids(
            method, &body, &json!([{"message_id":11},{"message_id":12}]),
        );
        assert_eq!(returned[0].chat_id, "-400");
        assert_eq!(returned[1].msg_id, "12");
    }

    #[test]
    fn edited_and_deleted_business_notices_reuse_connection_scoped_handles() {
        let bot = TeleTramConnect::build("");
        let message = json!({
            "message_id":8, "chat":{"id":12,"type":"private"},
            "business_connection_id":"business-1",
            "forward_origin":{"type":"hidden_user","sender_user_name":"Example"}
        });
        let event = json!({"update_id":1,"business_message":message});
        let original = bot.native_update("business_message", &event);
        let forwarded = bot.forwarded_notice("business_message", &event).unwrap();
        let edited = bot.native_update("edited_business_message", &json!({
            "edited_business_message":message
        }));
        let deleted = bot.native_update("deleted_business_messages", &json!({
            "deleted_business_messages":{
                "chat":{"id":12,"type":"private"},
                "business_connection_id":"business-1","message_ids":[8]
            }
        }));
        assert_eq!(original["message_id"], forwarded["message_id"]);
        assert_eq!(original["message_id"], edited["message_id"]);
        assert_eq!(original["message_id"], deleted["message_ids"][0]);
        let regular = bot.message_handle(RawMsgId::from_message(&json!({"message_id":8}), &json!(12), "").unwrap());
        assert_ne!(original["message_id"], regular);
        let (groups, missing) = bot.message_batches(&[
            original["message_id"].as_str().unwrap(), &regular, "missing",
        ]).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(missing, ["missing"]);
    }

    #[test]
    fn topic_service_notice_keeps_the_message_and_thread() {
        let bot = TeleTramConnect::build("");
        let message = json!({
            "message_id":12,"message_thread_id":9,"is_topic_message":true,
            "chat":{"id":-100123,"type":"supergroup"},
            "from":{"id":1},"forum_topic_closed":{}
        });
        let kind = topic_service_event(&message).unwrap();
        assert_eq!(kind, "forum_topic_closed");
        let notice = bot.notice_for_update(kind, &message, &json!(42));
        assert_eq!(notice["update_id"], 42);
        assert_eq!(notice["group_id"], "-100123");
        assert_eq!(notice["message_thread_id"], 9);
        assert_eq!(notice["telegram"], message);
        assert!(topic_service_event(&json!({"text":"normal"})).is_none());
    }

    #[test]
    fn mute_changes_sending_rights_without_granting_extra_permissions() {
        let chat = json!({"permissions":{
            "can_send_messages":true,"can_send_photos":false,"can_invite_users":true,
            "can_pin_messages":false
        }});
        let member = json!({"status":"restricted","can_react_to_messages":true,"can_invite_users":false});
        let muted = mute_permissions(&chat, &member, true);
        assert!(SEND_PERMISSIONS.iter().all(|field| muted[*field] == false));
        assert_eq!(muted["can_react_to_messages"], true);
        assert_eq!(muted["can_invite_users"], false);
        let restored = mute_permissions(&chat, &member, false);
        assert_eq!(restored["can_send_messages"], true);
        assert_eq!(restored["can_send_photos"], false);
        assert_eq!(restored["can_invite_users"], false);
        assert_eq!(restored["can_pin_messages"], false);
    }

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
    fn topic_ids_are_extracted_and_kept_separate_from_normal_replies() {
        let regular = Topic::from_message(&json!({"message_thread_id":5}));
        assert_eq!(regular, Topic::default());
        let topic = Topic::from_message(&json!({"message_thread_id":5,"is_topic_message":true}));
        let direct = Topic::from_message(&json!({"direct_messages_topic":{"topic_id":8}}));
        let mut payload = json!({"text":"test"});
        topic.apply(&mut payload);
        assert_eq!(payload["message_thread_id"], 5);
        assert_eq!(Topic::from_params(&payload).unwrap(), topic);
        direct.apply(&mut payload);
        assert_eq!(payload["direct_messages_topic_id"], 8);
        assert!(Topic::from_params(&payload).is_err());
    }

    #[test]
    fn keyboard_payload_keeps_native_topic_and_reply() {
        let keyboard = json!({"inline_keyboard":[[{"text":"点选","callback_data":"001"}]]});
        let topic = Topic::from_message(&json!({"message_thread_id":5,"is_topic_message":true}));
        let payload = TeleTramConnect::message_payload("-100", &topic, Some("8"), Some(&keyboard)).unwrap();
        assert_eq!(payload, json!({
            "chat_id":-100, "message_thread_id":5,
            "reply_parameters":{"message_id":8}, "reply_markup":keyboard
        }));
    }

    #[test]
    fn telegram_file_download_paths_are_fixed_to_the_bot_origin() {
        let url = download_url("123:token", "documents/sample.pdf").unwrap();
        assert_eq!(url.path(), "/file/bot123:token/documents/sample.pdf");
        assert!(download_url("123:token", "../other").is_err());
        assert!(download_url("123:token", "https://example.org/file").is_err());
    }

    #[tokio::test]
    async fn legacy_media_commands_and_chat_scoped_replies_use_native_ids() {
        let bot = TeleTramConnect::build("");
        let a = bot.add_msg_id(&vec![RawMsgId {
            msg_id: "9".into(),
            chat_id: "-100".into(),
            business_id: String::new(),
        }]);
        let b = bot.add_msg_id(&vec![RawMsgId {
            msg_id: "9".into(),
            chat_id: "-200".into(),
            business_id: String::new(),
        }]);
        assert_eq!(bot.get_cq_msg_id("-100", "9").as_deref(), Some(a.as_str()));
        let message = json!([
            {"type":"reply","data":{"id":b}},
            {"type":"file","data":{"file":"base64://YWJj","name":"sample.txt"}},
            {"type":"video","data":{"file":"https://example.org/test.mp4"}}
        ]);
        let (parts, quote) = bot.make_telegram_msg(&message, "-200", true, "").await.unwrap();
        assert_eq!(quote, "9");
        assert_eq!(parts[0].0, "document");
        assert_eq!(parts[0].2, "sample.txt");
        assert_eq!(parts[1].0, "video");
        assert!(bot.make_telegram_msg(&message, "-100", true, "").await.is_err());
    }

    #[tokio::test]
    async fn business_checklists_and_quotes_keep_their_connection() {
        let bot = TeleTramConnect::build("");
        let raw = RawMsgId {
            msg_id: "5".into(), chat_id: "22".into(), business_id: "business-a".into(),
        };
        let id = bot.message_handle(raw.clone());
        let list = json!({"title":"订单","tasks":[{"id":1,"text":"发货"}]});
        let message = json!([
            {"type":"reply","data":{"id":id}},
            {"type":"telegram_checklist","data":{"content":list.to_string()}}
        ]);
        assert!(bot.make_telegram_msg(&message,"22",false,"business-b").await.is_err());
        let (parts, quote) = bot.make_telegram_msg(&message,"22",false,"business-a").await.unwrap();
        assert_eq!(quote,"5");
        assert_eq!(parts[0].0,"telegram_checklist");
        assert_eq!(serde_json::from_str::<Value>(&parts[0].1).unwrap(),list);
        assert_eq!(TeleTramConnect::message_target(&raw).unwrap(),json!({
            "chat_id":22,"message_id":5,"business_connection_id":"business-a"
        }));
        let returned = TeleTramConnect::response_message_ids("sendChecklist",
            &json!({"business_connection_id":"business-a"}), &json!({"message_id":5,"chat":{"id":22}}));
        assert_eq!(returned,vec![raw]);
    }

    #[tokio::test]
    async fn invoice_segments_and_payment_events_keep_their_payloads() {
        let bot = TeleTramConnect::build("");
        let invoice = json!({"title":"手册","description":"电子版","payload":"0001",
            "currency":"XTR","provider_token":"","prices":[{"label":"手册","amount":25}]});
        let message = json!([{"type":"telegram_invoice","data":{"content":invoice.to_string()}}]);
        let (parts,_) = bot.make_telegram_msg(&message,"49",false,"").await.unwrap();
        assert_eq!(parts[0].0,"telegram_invoice");
        assert_eq!(serde_json::from_str::<Value>(&parts[0].1).unwrap(),invoice);
        let invalid_keyboard = json!({"inline_keyboard":[[{"text":"下一步","callback_data":"a"}]]});
        let error = bot.send_message_inner(parts,String::new(),"49",&Topic::default(),Some(&invalid_keyboard),"",&Value::Null).await.unwrap_err();
        assert!(error.to_string().contains("支付按钮"));
        for field in ["successful_payment","refunded_payment"] {
            let event = json!({"update_id":10,"message":{"message_id":5,"chat":{"id":49,"type":"private"},
                "from":{"id":49},field:{"invoice_payload":"0001","total_amount":25,"telegram_payment_charge_id":"p1"}}});
            let notice = bot.structured_message_notice("message",&event).unwrap();
            assert_eq!(notice["sub_type"],field);
            assert_eq!(notice["user_id"],"49");
            assert_eq!(notice["telegram"][field]["invoice_payload"],"0001");
            assert!(bot.structured_message_notice("edited_message",&event).is_none());
        }
    }

    #[tokio::test]
    async fn native_notes_and_animation_keep_their_method_and_message_scope() {
        let bot = TeleTramConnect::build("");
        for (field, method, edited) in [
            ("voice", "sendVoice", "edited_voice"),
            ("video_note", "sendVideoNote", "edited_video_note"),
            ("animation", "sendAnimation", "edited_animation"),
        ] {
            let data = json!({"kind":field,"body":{field:"media-id","chat_id":99},"files":{}});
            let mut payload = json!({"chat_id":49,"business_connection_id":"business"});
            let (actual, files) = TeleTramConnect::native_media(&data, &mut payload).unwrap();
            assert_eq!(actual, method);
            assert_eq!(files, json!({}));
            assert_eq!(payload, json!({"chat_id":49,"business_connection_id":"business",field:"media-id"}));
            let message = json!({"message_id":5,"chat":{"id":49,"type":"private"},
                "from":{"id":49},"message_thread_id":3,"is_topic_message":true,
                "business_connection_id":"business",
                "caption":"说明",field:{"file_id":"media-id","duration":12}});
            let ordinary = json!({"update_id":10,"message":message});
            let cq = bot.to_cq_msg(&ordinary).await.unwrap();
            assert!(cq.starts_with("说明[CQ:telegram_media,"));
            for (kind, expected) in [("message",field), ("channel_post",field), ("business_message",field),
                ("edited_message",edited), ("edited_channel_post",edited), ("edited_business_message",edited)] {
                let event = json!({"update_id":10,kind:message});
                let notice = bot.structured_message_notice(kind, &event).unwrap();
                assert_eq!(notice["sub_type"], expected);
                assert_eq!(notice["telegram_update_type"], kind);
                assert_eq!(notice["message_thread_id"], 3);
                assert_eq!(notice["telegram"][field]["file_id"], "media-id");
                assert_eq!(notice["telegram"]["business_connection_id"], "business");
            }
        }

        let upload = json!({"kind":"animation","body":{"animation":"attach://media","thumbnail":"attach://preview"},
            "files":{"media":{"filename":"movie.gif","base64":"YWJj"},"preview":{"filename":"thumb.jpg","base64":"YWJj"}}});
        let mut payload = json!({"chat_id":49});
        let (method, files) = TeleTramConnect::native_media(&upload, &mut payload).unwrap();
        assert_eq!(method, "sendAnimation");
        assert_eq!(files, upload["files"]);
        assert_eq!(payload["thumbnail"], "attach://preview");
        let invalid = json!({"kind":"video_note","body":{"video_note":"https://example.org/round.mp4"},"files":{}});
        assert!(TeleTramConnect::native_media(&invalid, &mut payload).is_err());
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
