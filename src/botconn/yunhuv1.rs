// 服务端代码见：https://file.uhsea.com/2507/1a128d2616547c71e78ff3b37bf6b8324A.txt

use hyper::header::HeaderName;
use hyper::header::HeaderValue;
use regex::Regex;
use serde::de::Error;
use serde_json::json;
use serde_json::Value;
use uuid::Uuid;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use super::BotConnectTrait;
use crate::cqapi::{cq_add_log, cq_add_log_w};
use crate::mytool::str_msg_to_arr;
use crate::RT_PTR;
use crate::mytool::read_json_str;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Yunhuv1Connect {
    pub self_id: Arc<RwLock<String>>,
    pub token: Arc<RwLock<String>>,
    pub url: String,
    pub url_proxy: Arc<RwLock<String>>,
    pub is_stop: Arc<AtomicBool>,
    msg_ids: Arc<RwLock<VecDeque<MsgIdPair>>>,
}


#[derive(Debug, Clone)]
struct RawMsgId {
    msg_id: String,
    chat_id: String,
    chat_type:String,
}

#[derive(Debug, Clone)]
struct MsgIdPair {
    msg_id: String,
    raw_msg_ids: Vec<RawMsgId>,
}



lazy_static! {
    static ref G_USERNAME2USERID:std::sync::RwLock<HashMap<String,String>> = std::sync::RwLock::new(HashMap::new());
}

impl Yunhuv1Connect {
    pub fn build(url: &str) -> Self {
        return Self {
            self_id: Arc::new(RwLock::new("".to_owned())),
            token: Arc::new(RwLock::new("".to_owned())),
            url: url.to_owned(),
            url_proxy:Arc::new(RwLock::new("".to_owned())),
            is_stop: Arc::new(AtomicBool::new(false)),
            msg_ids: Arc::new(RwLock::new(VecDeque::new())),
        };
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



    fn get_req_client(&self) -> Result<reqwest::Client, Box<dyn std::error::Error + Send + Sync>> {

        Ok(reqwest::Client::builder()
            .no_proxy()
            .build()?)
       
    }

    async fn proxyrequest(&self, url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Duration;
        use tokio::time::sleep;
        use tokio::select;
        let client = self.get_req_client()?;
        let fut = async {
            let resp = client.get(url).send().await?;
            let text = resp.text().await?;
            Ok::<String, Box<dyn std::error::Error + Send + Sync>>(text)
        };
        select! {
            res = fut => res,
            _ = sleep(Duration::from_secs(30)) => Err("proxyrequest timeout".into()),
        }
    }

    fn to_ob_event(
        &self,
        self_id:&str,
        yh_event_json: &serde_json::Value,
    ) -> Result<serde_json::Value, serde_json::Error> {
        let event_type = yh_event_json["header"]["eventType"].as_str().unwrap_or("");

        if event_type != "message.receive.normal" {
            return Err(serde_json::Error::custom("Unsupported event type"));
        }

        let yh_event = &yh_event_json["event"];
        let yh_message = &yh_event["message"];
        let yh_sender = &yh_event["sender"];
        let yh_chat = &yh_event["chat"];
        let content_type = yh_message["contentType"].as_str().unwrap_or("");
        let raw_msg_id = yh_message["msgId"].as_str().unwrap_or("").to_string();
        let chat_id = yh_chat["chatId"].as_str().unwrap_or("").to_string();
        let chat_type = yh_chat["chatType"].as_str().unwrap_or("").to_string();
        let ob_msg_id = self.add_msg_id(&vec![RawMsgId{
            msg_id:raw_msg_id,
            chat_id:chat_id.to_owned(),
            chat_type:chat_type.to_owned()
        }]);

        let mut ob_event = json!({
            "time": yh_event_json["header"]["eventTime"].as_i64().unwrap_or(0) / 1000,
            "self_id": self_id,
            "post_type": "message",
            "message_type": if yh_chat["chatType"] == "group" { "group" } else { "private" },
            "sub_type": "friend",
            "message_id": ob_msg_id,
            "user_id": yh_sender["senderId"].as_str().unwrap_or("").to_string(),
            "font": 0,
            "platform":"yunhu",
            "sender": {
                "user_id": yh_sender["senderId"].as_str().unwrap_or("").to_string(),
                "nickname": yh_sender["senderNickname"].as_str().unwrap_or(""),
                "card": "",
                "role": match yh_sender["senderUserLevel"].as_str() {
                    Some("owner") => "owner",
                    Some("administrator") => "admin",
                    _ => "member",
                },
            }
        });

        if chat_type == "group" {
            ob_event["group_id"] = json!(chat_id);
        }

        let mut segments: Vec<Value> = Vec::new();
        let yh_content = &yh_message["content"];

        let raw_message = match content_type {
            "text" | "markdown" => yh_content["text"].as_str().unwrap_or("").to_string(),
            "image" => {
                let image_name = yh_content["imageName"].as_str().unwrap_or("");
                format!("[CQ:image,file={}]", image_name)
            },
            _ => "".to_string(),
        };
        

        if let Some(parent_id) = yh_message["parentId"].as_str().filter(|s| !s.is_empty()) {
            let ob_msg_id = self.add_msg_id(&vec![RawMsgId{
                msg_id:parent_id.to_owned(),
                chat_id,
                chat_type
            }]);
            segments.push(json!({ "type": "reply", "data": { "id": ob_msg_id } }));
        }

        match content_type {
            "text" | "markdown" => {
                let text_content = yh_content["text"].as_str().unwrap_or("");
                if let Some(at_users) = yh_content["at"].as_array().filter(|a| !a.is_empty()) {
                    let mut last_index = 0;
                    let at_ids: Vec<&str> = at_users.iter().filter_map(|v| v.as_str()).collect();
                    let re = Regex::new(r"@[^\s​]+").unwrap();

                    for (i, mat) in re.find_iter(text_content).enumerate() {
                        if mat.start() > last_index {
                            segments.push(json!({ "type": "text", "data": { "text": &text_content[last_index..mat.start()] } }));
                        }
                        if let Some(user_id) = at_ids.get(i) {
                            segments.push(json!({ "type": "at", "data": { "qq": *user_id } }));
                        }
                        last_index = mat.end();
                    }
                    if last_index < text_content.len() {
                        segments.push(json!({ "type": "text", "data": { "text": &text_content[last_index..] } }));
                    }
                } else {
                    segments.push(json!({ "type": "text", "data": { "text": text_content } }));
                }
            }
            "image" => {
                let image_name = yh_content["imageName"].as_str().unwrap_or("");
                let image_url = yh_content["imageUrl"].as_str().unwrap_or("");
                segments.push(json!({
                    "type": "image",
                    "data": {
                        "file": image_name,
                        "url": image_url
                    }
                }));
            }
            _ => {}
        }

        ob_event["message"] = json!(segments);
        ob_event["raw_message"] = json!(raw_message);

        Ok(ob_event)
    }


    

    async fn deal_event(
        &self,
        event: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ob_evt = self.to_ob_event(&self.self_id.read().unwrap(), event)?;
        RT_PTR.spawn_blocking(move || {
            let json_str = ob_evt.to_string();
            cq_add_log(&format!("YUNTU_OB_EVENT:{json_str}")).unwrap();
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
        return Ok(());
    }

    async fn do_recv_loop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        loop {
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let url_proxy = self.url_proxy.read().unwrap().to_owned();
            let ret = self.proxyrequest(&url_proxy).await?;
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let event_json: serde_json::Value = serde_json::from_str(&ret)?;
            let eventarr = event_json.as_array().ok_or("eventarr not array")?;
            for event in eventarr.to_owned() {
                cq_add_log(&format!(
                    "yunhu bot {} recv:{}",
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
                                "yunhu bot {} deal event error:{}",
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


    fn get_auto_escape_from_params(&self, params: &serde_json::Value) -> bool {
        let is_auto_escape = Self::get_json_bool(params, "auto_escape");
        return is_auto_escape;
    }

    async fn send_message_inner(&self,to_send_data:Vec<(&str, String)>,mut quote:String,chat_id:&str,chat_type:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        
        let token = self.token.read().unwrap().to_owned();
        let client = self.get_req_client()?;
        let url_t = format!("https://chat-go.jwzhd.com/open-apis/v1/bot/send?token={}",token);
        

        let mut raw_msg_ids: Vec<RawMsgId> = vec![];
        for (data_type,data) in to_send_data {
            if data_type == "text" {
                let mut to_send_json = json!({
                    "recvId":chat_id,
                    "recvType":chat_type,
                    "contentType":"text",
                    "content": {
                        "text": data
                    }
                });
                if quote != "" {
                    to_send_json["parentId"] = json!(quote);
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(&url_t)
                    .body(to_send_json.to_string())
                    .build()?;
                req.headers_mut().append(HeaderName::from_str("Content-type")?, HeaderValue::from_str("application/json; charset=utf-8")?);
                let ret = client.execute(req).await?;
                let retbin = ret.bytes().await?.to_vec();
                let ret_str = String::from_utf8(retbin)?;
                cq_add_log(&format!("YUNTU_POST响应:{ret_str}")).unwrap();
                let ret_json:Value = serde_json::from_str(&ret_str)?;
                let raw_msg_id = ret_json["data"]["messageInfo"]["msgId"].as_str().ok_or("msgId not found")?.to_owned();
                raw_msg_ids.push(RawMsgId {
                    msg_id: raw_msg_id,
                    chat_id: chat_id.to_owned(),
                    chat_type: chat_type.to_owned(),
                });
            } else if data_type == "image" {
                let mut to_send_json = json!({
                    "recvId":chat_id,
                    "recvType":chat_type,
                    "contentType":"image",
                    "content": {
                        "imageKey": data,
                    }
                });
                if quote != "" {
                    to_send_json["parentId"] = json!(quote);
                    quote = "".to_owned();
                }
                let mut req = client
                    .post(&url_t)
                    .body(to_send_json.to_string())
                    .build()?;
                req.headers_mut().append(HeaderName::from_str("Content-type")?, HeaderValue::from_str("application/json; charset=utf-8")?);
                let ret = client.execute(req).await?;
                let retbin = ret.bytes().await?.to_vec();
                let ret_str = String::from_utf8(retbin)?;
                cq_add_log(&format!("YUNTU_POST响应:{ret_str}")).unwrap();
                let ret_json:Value = serde_json::from_str(&ret_str)?;
                let raw_msg_id = ret_json["data"]["messageInfo"]["msgId"].as_str().ok_or("msgId not found")?.to_owned();
                raw_msg_ids.push(RawMsgId {
                    msg_id: raw_msg_id,
                    chat_id: chat_id.to_owned(),
                    chat_type: chat_type.to_owned(),
                });
            } else {
                cq_add_log_w(&format!("YUNTU_POST不支持的消息类型:{data_type}")).unwrap();
            }
        }
        let ob_msg_id = self.add_msg_id(&raw_msg_ids);
        Ok(ob_msg_id)

    }

    async fn deal_ob_send_msg(
        &self,
        params: &serde_json::Value,
        echo: &serde_json::Value,
        chat_type: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let chat_id;
        if chat_type == "group" {
            chat_id = read_json_str(params, "group_id");
        } else {
            chat_id = read_json_str(params, "user_id");
        }

        // let group_id = read_json_str(params, "group_id");
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

        let (to_send_data, quote) = self.make_yunhu_msg(&message_arr).await?;

        let msg_id = self.send_message_inner(to_send_data,quote,&chat_id,chat_type).await?;

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


        let msg_id = read_json_str(params,"message_id");
        let msg_ids = self.get_msg_id(&msg_id);
        if msg_ids.len() == 0 {
            return Err("msg_id not found")?;
        }

        let token = self.token.read().unwrap().to_owned();
        let client = self.get_req_client()?;
        let url_t = format!("https://chat-go.jwzhd.com/open-apis/v1/bot/recall?token={}",token);


        for msg_id in msg_ids {
            let to_send_json = json!({
                "chatId":msg_id.chat_id,
                "chatType":msg_id.chat_type,
                "msgId":msg_id.msg_id
            });
            let mut req = client
                .post(&url_t)
                .body(to_send_json.to_string())
                .build()?;
            req.headers_mut().append(HeaderName::from_str("Content-type")?, HeaderValue::from_str("application/json; charset=utf-8")?);
            let ret = client.execute(req).await?;
            let retbin = ret.bytes().await?.to_vec();
            let ret_str = String::from_utf8(retbin)?;
            cq_add_log(&format!("YUNTU_POST响应:{ret_str}")).unwrap();
        }
        
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
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

    async fn upload_image(&self,file_bin:Vec<u8>)-> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let token = self.token.read().unwrap().to_owned();
        let uri = reqwest::Url::from_str(&format!("https://chat-go.jwzhd.com/open-apis/v1/image/upload?token={token}"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let form = reqwest::multipart::Form::new().part("image", reqwest::multipart::Part::bytes(file_bin).file_name("test.png"));
        let req = client.post(uri).multipart(form).build()?;
        let ret = client.execute(req).await?;
        let retbin = ret.bytes().await?.to_vec();
        let ret_str = String::from_utf8(retbin)?;
        cq_add_log(&format!("YUNTU_UPLOAD响应:{ret_str}")).unwrap();
        let js:serde_json::Value = serde_json::from_str(&ret_str)?;
        let image_key = js.get("data").ok_or("get data err")?.get("imageKey").ok_or("imageKey not found")?.as_str().ok_or("imageKey not str")?;
        Ok(image_key.to_owned())
    }

    pub async fn http_post(url:&str,data:Vec<u8>,headers:&HashMap<String, String>,is_post:bool) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let client;
        let uri = reqwest::Url::from_str(url)?;
        if uri.scheme() == "http" {
            client = reqwest::Client::builder().no_proxy().build()?;
        } else {
            client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().build()?;
        }
        let mut req;
        if is_post {
            req = client.post(uri).body(reqwest::Body::from(data)).build()?;
        }else {
            req = client.get(uri).build()?;
        }
        for (key,val) in headers {
            req.headers_mut().append(HeaderName::from_str(key)?, HeaderValue::from_str(val)?);
        }
        let retbin;
        let ret = client.execute(req).await?;
        retbin = ret.bytes().await?.to_vec();
        return Ok(retbin);
    }

    async fn get_asset(&self,uri:&str)-> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let file_bin;
        if uri.starts_with("http") {
            file_bin = Self::http_post(uri,vec![],&HashMap::new(),false).await?;
        }else if uri.starts_with("base64://") {
            let b64_str = uri.get(9..).unwrap();
            file_bin = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                &base64::alphabet::STANDARD,
                base64::engine::general_purpose::PAD), b64_str)?;
        }else {
            let file_path;
            if cfg!(target_os = "windows") {
                file_path = uri.get(8..).ok_or("can't get file_path")?;
            } else {
                file_path = uri.get(7..).ok_or("can't get file_path")?;
            }
            let path = Path::new(&file_path);
            file_bin = tokio::fs::read(path).await?;
        }
        Ok(file_bin)
    }


    async fn make_yunhu_msg(
        &self,
        message_arr: &serde_json::Value,
    ) -> Result<(Vec<(&str, String)>, String), Box<dyn std::error::Error + Send + Sync>> {
        let mut to_send_data: Vec<(&str, String)> = vec![];
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

                let file_bin = self.get_asset(file).await?;
                let image_key = self.upload_image(file_bin).await?;
                to_send_data.push(("image", image_key));
                last_type = "image";

            }
            else if tp == "reply" {
                if quote !=  "" {
                    continue;
                }
                let cq_id = Self::to_json_str(it.get("data").ok_or("data not found")?.get("id").ok_or("reply not found")?);
                let yunhu_id = self.get_msg_id(&cq_id);
                quote = yunhu_id.get(0).ok_or("get yunhu msg_id err")?.msg_id.to_owned();
            }
        }
        Ok((to_send_data, quote))
    }
    fn to_json_str(val:&serde_json::Value) -> String {
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
}



#[async_trait]
impl BotConnectTrait for Yunhuv1Connect {
    async fn disconnect(&mut self) {
        self.is_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn get_alive(&self) -> bool {
        return !self.is_stop.load(std::sync::atomic::Ordering::Relaxed);
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(8..).ok_or("yunhu url格式错误")?;
        let config_json: serde_json::Value = serde_json::from_str(config_json_str)?;
        let token = config_json
            .get("token")
            .ok_or("token not found")?
            .as_str()
            .ok_or("token not str")?;
        let url_str = config_json
            .get("url")
            .ok_or("url not found")?
            .as_str()
            .ok_or("url not str")?;


        let self_id = config_json
            .get("self_id")
            .ok_or("self_id not found")?
            .as_str()
            .ok_or("self_id not str")?;

        let url_proxy: url::Url = url_str.parse()?;
        let mut url_proxy = url_proxy.join("get_event")?;
        url_proxy.query_pairs_mut().append_pair("userKey", self_id);
        url_proxy.query_pairs_mut().append_pair("timeout", "10000");
        

        self.self_id.write().unwrap().push_str(self_id);
        self.url_proxy.write().unwrap().push_str(url_proxy.as_str());
        self.token.write().unwrap().push_str(token);
        cq_add_log(&format!("yunhu bot {} connect success", self_id)).unwrap();
        let self_t = self.clone();
        tokio::task::spawn(async move {
            match self_t.do_recv_loop().await {
                Ok(_) => {}
                Err(err) => {
                    cq_add_log(&format!(
                        "yunhu bot {} recv error:{}",
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
            "send_group_msg" => self.deal_ob_send_msg(&params, &echo, "group").await?,
            "send_private_msg" => {
                self.deal_ob_send_msg(&params, &echo, "user").await?
            },
            "send_msg" => {
                let group_id = read_json_str(params, "group_id");
                if group_id != "" {
                    self.deal_ob_send_msg(&params, &echo, "group").await?
                }else {
                    self.deal_ob_send_msg(&params, &echo, "user").await?
                }
            },
            "delete_msg" => {
                self.deal_ob_delete_msg(&params,&echo).await?
            },
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
        return vec![("yunhu".to_owned(), lk.to_owned())];
    }
}
