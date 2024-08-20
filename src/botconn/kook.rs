use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use std::time::SystemTime;

use async_trait::async_trait;
use fancy_regex::Regex;
use flate2::read::ZlibDecoder;
use futures_util::SinkExt;
use futures_util::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite;
use uuid::Uuid;

use crate::cqapi::cq_add_log;
use crate::cqapi::cq_add_log_w;
use crate::mytool::cq_params_encode;
use crate::mytool::cq_text_encode;
use crate::mytool::json_to_cq_str;
use crate::mytool::read_json_str;
use crate::mytool::str_msg_to_arr;

use super::BotConnectTrait;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;


#[derive(Debug,Clone)]
struct RawMsgId {
    msg_id:String,
    user_id:String
}

#[derive(Debug,Clone)]
struct MsgIdPair {
    msg_id:String,
    raw_msg_ids:Vec<RawMsgId>
}

#[derive(Debug,Clone)]
pub struct KookConnect {
    pub self_id:Arc<std::sync::RwLock<String>>,
    pub url:String,
    pub token:String,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx :Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn:Arc<AtomicI64>,
    msg_ids:Arc<RwLock<VecDeque<MsgIdPair>>>,
    recieve_pong:Arc<AtomicBool>,
}

impl KookConnect {
    pub fn build(url:&str) -> Self {
        KookConnect {
            self_id:Arc::new(std::sync::RwLock::new("".to_owned())),
            url:url.to_owned(),
            token:"".to_owned(),
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            sn: Arc::new(AtomicI64::new(0)),
            msg_ids:Arc::new(RwLock::new(VecDeque::new())),
            recieve_pong:Arc::new(AtomicBool::new(false)),
        }
    }
    async fn http_get_json(&self,uri:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        cq_add_log(&format!("发送KOOK_GET:{uri}")).unwrap();
        let uri = reqwest::Url::from_str(&format!("https://www.kookapp.cn/api/v3{uri}"))?;
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().build()?;
        let mut req = client.get(uri).build()?;
        let token = &self.token;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("Bot {token}"))?);
        let ret = client.execute(req).await?;
        let retbin = ret.bytes().await?.to_vec();
        let ret_str = String::from_utf8(retbin)?;
        cq_add_log(&format!("KOOK_GET响应:{ret_str}")).unwrap();
        let js:serde_json::Value = serde_json::from_str(&ret_str)?;
        let ret = js.get("data").ok_or("get data err")?;
        Ok(ret.to_owned())
    }

    async fn send_to_onebot_client(&self,js:&serde_json::Value) {
        let json_str = js.to_string();
        // cq_add_log(&format!("KOOK_OB_EVENT:{json_str}")).unwrap();
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }

    async fn http_get_json_t(&self,uri:&str,use_cache:bool) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        lazy_static! {
            static ref CACHE : std::sync::RwLock<VecDeque<(String,serde_json::Value,u64)>>  = std::sync::RwLock::new(VecDeque::from([]));
        }
        let tm = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        // 清除久远的记录
        {
            let mut lk = CACHE.write().unwrap();
            loop {
                let mut remove_index = 0;
                for it in &*lk {
                    if tm - it.2 > 60 {
                        break;
                    }
                    remove_index += 1;
                }
                if remove_index == lk.len() {
                    break;
                }
                lk.remove(remove_index);
            }
        }
        // 从缓存中返回数据
        if use_cache {
            let lk = CACHE.read().unwrap();
            for it in &*lk {
                if it.0 ==uri {
                    return Ok(it.1.clone());
                }
            }
        }
        // 缓存失效或者不使用缓存
        let ret_val = self.http_get_json(uri).await?;
        // 更新缓存
        {
            let mut lk = CACHE.write().unwrap();
            lk.push_back((uri.to_string(),ret_val.clone(),tm));
        }
        return Ok(ret_val)

    }

    async fn get_gateway(&self)-> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let ret_json = self.http_get_json(&format!("/gateway/index?compress=1")).await?;
        Ok(ret_json.get("url").ok_or("get url err")?.as_str().ok_or("url not str")?.to_owned())
    }
    async fn send_private_msg(&self,tp:i32,user_id:&str,message:&str,quote:&str)-> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["content"] = message.into();
        json["target_id"] = user_id.into();
        json["type"] = tp.into();
        if quote != "" {
            json["quote"] = quote.into();
        }
        let ret_json = self.http_post_json("/direct-message/create",&json).await?;
        let msg_id = ret_json.get("msg_id").ok_or("msg_id not found")?.as_str().ok_or("msg_id not str")?;
        Ok(msg_id.to_owned())
    }
    async fn deal_ob_send_private_msg(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let user_id = read_json_str(params,"user_id");
        let message_arr:serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;

        if message_rst.is_string() {
            if self.get_auto_escape_from_params(&params) {
                message_arr = serde_json::json!(
                    [{"type":"text","data":{
                        "text": message_rst.as_str()
                    }}]
                );
            } else {
                message_arr = str_msg_to_arr(message_rst).map_err(|x|{
                    format!("str_msg_to_arr err:{:?}",x)
                })?;
            }
        }else {
            message_arr = params.get("message").ok_or("message not found")?.to_owned();
        }
        
        let (to_send_data, mut quote) = self.make_kook_msg(&message_arr,true).await?;

        let mut msg_ids = vec![];
        for (tp,msg) in & to_send_data.clone() {
            let msg_id = self.send_private_msg(*tp,&user_id,msg,&quote).await?;

            msg_ids.push(RawMsgId{
                msg_id,
                user_id:self.self_id.read().unwrap().to_owned(),
            });
            quote = "".to_owned();
        }
        let msg_id = self.add_msg_id(&msg_ids);
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

    async fn get_guild_member_info(&self,guild_id:&str,user_id:&str,use_cache:bool)-> Result<GroupMemberInfo, Box<dyn std::error::Error + Send + Sync>> {
        let stranger_info = self.http_get_json_t(&format!("/user/view?user_id={user_id}&guild_id={guild_id}"),use_cache).await?;
        let guild_info = self.http_get_json_t(&format!("/guild/view?guild_id={guild_id}"),use_cache).await?;
        let owner_id = guild_info.get("user_id").ok_or("get user_id err")?.as_str().ok_or("user_id not str")?;
        let role;
        if owner_id == user_id {
            role = "owner";
        }else {
            let roles = stranger_info.get("roles").ok_or("get roles err")?.as_array().ok_or("roles not arr")?;
            if roles.len() != 0 { 
                role = "admin";
            } else {
                role = "member";
            }
        }
        Ok(GroupMemberInfo {
            group_id:"".to_owned(),
            groups_id:guild_id.to_owned(),
            user_id:user_id.to_owned(),
            nickname:stranger_info.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?.to_owned(),
            card:stranger_info.get("nickname").ok_or("get nickname err")?.as_str().ok_or("nickname not str")?.to_owned(),
            sex:"unknown".to_owned(),
            age:0,
            area:"".to_owned(),
            join_time:(stranger_info.get("joined_at").ok_or("get joined_at err")?.as_u64().ok_or("joined_at not u64")? / 1000) as i32,
            last_sent_time:(stranger_info.get("active_time").ok_or("get active_time err")?.as_u64().ok_or("active_time not u64")? / 1000) as i32,
            level:"0".to_owned(),
            role:role.to_owned(),
            unfriendly:false,
            title:"".to_owned(),
            title_expire_time:0,
            card_changeable:false,
            avatar:stranger_info.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?.to_owned()
        })
    }

    fn reformat_dates(before: &str) -> String {

        fn kook_msg_f(msg: &str) -> String {
            let mut ret = String::new();
            let mut is_f = false;
            for ch in msg.chars() {
                if is_f {
                    is_f = false;
                    ret.push(ch);
                }else if ch == '\\' {
                    is_f = true
                }else {
                    ret.push(ch);
                }
            }
            return ret;
        }
            
        let mut ret = String::new();
        let sp = before.split("(met)");
        let mut index = 0;
        for it in sp{
            if index % 2 == 0 {
                ret.push_str(&cq_text_encode(&kook_msg_f(it)));
            } else {
                if it == "all" {
                    ret.push_str("[CQ:at,qq=all]");
                }else{
                    ret.push_str(&format!("[CQ:at,qq={}]", it));
                }
            }
            index += 1;
        }
        ret
    }

    pub fn kook_msg_to_cq(msg_type:i64,message:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {

        let ret_msg;
        if msg_type == 2 { // 图片消息
            ret_msg = format!("[CQ:image,file={},url={}]",cq_params_encode(&message),cq_params_encode(&message));
        } else {
            ret_msg = Self::reformat_dates(message);
        }
        
        Ok(ret_msg)
    }

    fn add_msg_id(&self,raw_msg_ids:&Vec<RawMsgId>) -> String {
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

    fn get_msg_id(&self,msg_id:&str) -> Vec<RawMsgId> {
        let lk = self.msg_ids.read().unwrap();
        for msg_id_pair in lk.iter() {
            if msg_id_pair.msg_id == msg_id {
                return msg_id_pair.raw_msg_ids.to_owned();
            }
        }
        return vec![];
    }

    pub fn get_cq_msg_id(&self,raw_msg_id:&str) -> (String,String) {
        let lk = self.msg_ids.read().unwrap();
        for msg_id_pair in lk.iter() {
            for raw_msg in &msg_id_pair.raw_msg_ids {
                if raw_msg.msg_id == raw_msg_id {
                    return (msg_id_pair.msg_id.clone(),raw_msg.user_id.clone());
                }
            }
        }
        return ("".to_owned(),"".to_owned());
    }
    async fn deal_private_message_event(&self,data:&serde_json::Value,user_id:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message = data.get("content").ok_or("content not found")?.as_str().ok_or("content not str")?.to_owned();
        
        let extra = data.get("extra").ok_or("extra not found")?;
        let author = extra.get("author").ok_or("author not found")?;

        let username = author.get("username").ok_or("username not found")?.as_str().ok_or("username not str")?;
        
        let avatar = author.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?;

        let sender: FriendInfo = FriendInfo {
            user_id: user_id.to_owned(),
            nickname: username.to_owned(),
            remark: username.to_owned(),
            avatar: avatar.to_owned()
        };

        let msg_type = data.get("type").ok_or("type not found")?.as_i64().ok_or("type not i64")?;

        let mut msg = String::new();

        // 处理卡牌消息
        if msg_type == 10 { // 卡牌消息
            self.deal_card_msg(data,&mut msg,false,"").await?;
        }else {
            // 处理回复
            if let Some(quote) = extra.get("quote") {
                let rong_id = read_json_str(quote, "rong_id");
                let cq_id = self.get_cq_msg_id(&rong_id).0;
                msg.push_str(&format!("[CQ:reply,id={cq_id}]"));
            }

            // 转为CQ格式
            msg.push_str(&Self::kook_msg_to_cq(msg_type,&message)?);
        }

        if msg == "" {
            return Ok(());
        }

        let raw_msg_id = data.get("msg_id").ok_or("msg_id not found")?.as_str().ok_or("msg_id not str")?;
        let msg_id = self.add_msg_id(&vec![RawMsgId{msg_id:raw_msg_id.to_owned(),user_id:user_id.to_owned()}]);

        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self.self_id.read().unwrap().to_owned(),
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":msg_id,
            "user_id":user_id,
            "message":msg,
            "raw_message":msg,
            "font":0,
            "sender":sender,
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }

    async fn deal_card_msg(&self,data:&serde_json::Value,msg:&mut String,is_group:bool,guild_id:&str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let message = data.get("content").ok_or("content not found")?.as_str().ok_or("content not str")?.to_owned();
        let err = "parse card error";
        let js_arr:serde_json::Value = serde_json::from_str(&message)?;
        let card_arr = js_arr.as_array().ok_or(err)?;
        for card in card_arr {
            let md_arr = card["modules"].as_array().ok_or(err)?;
            for md in  md_arr {
                let tp = md["type"].as_str().ok_or(err)?;
                if tp == "header" {
                    let content = md["text"]["content"].as_str().ok_or(err)?;
                    msg.push_str(&cq_text_encode(content));
                } else if tp == "section" {
                    let text = &md["text"];
                    let accessory = &md["accessory"];
                    let mode = md["mode"].as_str().ok_or(err)?;
                    let mut text_cq = "".to_owned();
                    let mut accessory_cq = "".to_owned();
                    if !text.is_null() {
                        let tp = text["type"].as_str().ok_or(err)?;
                        if tp == "plain-text" {
                            let content = text["content"].as_str().ok_or(err)?;
                            text_cq.push_str(&cq_text_encode(content));
                        } else if tp == "kmarkdown" {
                            let content = text["content"].as_str().ok_or(err)?;
                            let txt = Self::kook_msg_to_cq(9,content)?;
                            text_cq.push_str(&txt);
                        } else if tp == "paragraph" {
                            let fields = text["fields"].as_array().ok_or(err)?;
                            for field in fields {
                                let tp = field["type"].as_str().ok_or(err)?;
                                if tp == "plain-text" {
                                    let content = field["content"].as_str().ok_or(err)?;
                                    text_cq.push_str(&cq_text_encode(content));
                                } else if tp == "kmarkdown" {
                                    let content = field["content"].as_str().ok_or(err)?;
                                    let txt = Self::kook_msg_to_cq(9,content)?;
                                    text_cq.push_str(&txt);
                                }
                            }
                        }
                    }
                    if !accessory.is_null() {
                        let tp = accessory["type"].as_str().ok_or(err)?;
                        if tp == "image" {
                            let url = accessory["src"].as_str().ok_or(err)?;
                            let url_t = cq_params_encode(&url);
                            accessory_cq.push_str(&format!("[CQ:image,file={url_t},url={url_t}]"));
                        }
                    }
                    if mode == "left" {
                        msg.push_str(&accessory_cq);
                        msg.push_str(&text_cq);
                    } else if mode == "right" {
                        msg.push_str(&text_cq);
                        msg.push_str(&accessory_cq);
                    }
                } else if tp == "image-group" {
                    let images = md["elements"].as_array().ok_or(err)?;
                    for image in images {
                        let url = image["src"].as_str().ok_or(err)?;
                        let url_t = cq_params_encode(&url);
                        msg.push_str(&format!("[CQ:image,file={url_t},url={url_t}]"));
                    }
                }else if tp == "container" {
                    let elements = md["elements"].as_array().ok_or(err)?;
                    for element in elements {
                        let url = element["src"].as_str().ok_or(err)?;
                        let url_t = cq_params_encode(&url);
                        msg.push_str(&format!("[CQ:image,file={url_t},url={url_t}]"));
                    }
                } 
                else if tp == "context" {
                    let elements = md["elements"].as_array().ok_or(err)?;
                    for element in elements {
                        let tp = element["type"].as_str().ok_or(err)?;
                        if tp == "plain-text" {
                            let content = element["content"].as_str().ok_or(err)?;
                            msg.push_str(&cq_text_encode(content));
                        } else if tp == "kmarkdown" {
                            let content = element["content"].as_str().ok_or(err)?;
                            let txt = Self::kook_msg_to_cq(9,content)?;
                            msg.push_str(&txt);
                        } else if tp == "image" {
                            let url = element["src"].as_str().ok_or(err)?;
                            let url_t = cq_params_encode(&url);
                            msg.push_str(&format!("[CQ:image,file={url_t},url={url_t}]"));
                        }
                    }
                } else if tp == "file" {
                    if is_group {
                        let group_id = data.get("target_id").ok_or("target_id not found")?.as_str().ok_or("target_id not str")?;
                        let user_id = data.get("author_id").ok_or("author_id not found")?.as_str().ok_or("author_id not str")?;
                        let url = md["src"].as_str().ok_or(err)?;
                        let name = md.get("title").ok_or(err)?.as_str().ok_or(err)?.to_owned();
                        let size = md.get("size").ok_or(err)?.as_i64().ok_or(err)?.to_owned();
                        let  event_json = serde_json::json!({
                            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
                            "self_id":self.self_id.read().unwrap().to_owned(),
                            "post_type":"notice",
                            "notice_type":"group_upload",
                            "group_id":group_id.to_owned(),
                            "groups_id":guild_id.to_owned(),
                            "user_id":user_id,
                            "file": {
                                "url":url.to_owned(),
                                "name":name,
                                "size":size,
                                "busid":0
                            },
                            "platform":"kook"
                        });
                        self.send_to_onebot_client(&event_json).await;
                    }
                } else if tp == "audio" {
                    let title = read_json_str(md, "title");
                    let cover = read_json_str(md, "cover");
                    if title == "" && cover == "" {  // 说明是语音
                        let url = md["src"].as_str().ok_or(err)?;
                        let url_t = cq_params_encode(url);
                        msg.push_str(&format!("[CQ:record,file={},url={}]",url_t,url_t));
                    }
                } else if tp == "video" {
                    let url = md["src"].as_str().ok_or(err)?;
                    let url_t = cq_params_encode(url);
                    msg.push_str(&format!("[CQ:video,file={},url={}]",url_t,url_t));
                }
            }
        }
        return Ok(true);
    }
    async fn deal_group_message_event(&self,data:&serde_json::Value,user_id:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        
        let group_id = data.get("target_id").ok_or("target_id not found")?.as_str().ok_or("target_id not str")?;
        let message = data.get("content").ok_or("content not found")?.as_str().ok_or("content not str")?.to_owned();
        let extra = data.get("extra").ok_or("extra not found")?;

        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;

        // 获取发送者
        let mut sender: GroupMemberInfo = self.get_guild_member_info(guild_id,user_id,true).await?;
        sender.groups_id = guild_id.to_owned();

        // 获取消息类型
        let msg_type = data.get("type").ok_or("type not found")?.as_i64().ok_or("type not i64")?;

        let mut msg = String::new();

        // 处理卡牌消息
        if msg_type == 10 { // 卡牌消息
            self.deal_card_msg(data,&mut msg,true,guild_id).await?;
        } else {
            // 处理回复
            if let Some(quote) = extra.get("quote") {
                let rong_id = read_json_str(quote, "rong_id");
                let cq_id = self.get_cq_msg_id(&rong_id).0;
                msg.push_str(&format!("[CQ:reply,id={cq_id}]"));
            }

            // 转为CQ格式
            msg.push_str(&Self::kook_msg_to_cq(msg_type,&message)?);
        }

        if msg == "" {
            return Ok(());
        }

        // 存msg_id
        let raw_msg_id = data.get("msg_id").ok_or("msg_id not found")?.as_str().ok_or("msg_id not str")?;
        let msg_id = self.add_msg_id(&vec![RawMsgId{msg_id:raw_msg_id.to_owned(),user_id:user_id.to_owned()}]);

        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":*self.self_id.read().unwrap(),
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":msg_id,
            "groups_id":guild_id,
            "group_id":group_id,
            "user_id":user_id,
            "message":msg,
            "raw_message":msg,
            "font":0,
            "sender":sender,
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }

    // async fn get_welcome_channel(&self,guild_id:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    //     let ret_json = self.http_get_json_t(&format!("/guild/view?guild_id={guild_id}"),false).await?;
    //     let welcome_channel_id = read_json_str(&ret_json, "welcome_channel_id");
    //     Ok(welcome_channel_id)
    // }
    async fn deal_group_increase_event(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = data.get("target_id").ok_or("target_id not found")?.as_str().ok_or("target_id not str")?;
        // let welcome_channel_id = self.get_welcome_channel(guild_id).await?;
        let user_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("user_id").ok_or("user_id not found")?
                                .as_str().ok_or("user_id not str")?;
        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self.self_id.read().unwrap().to_owned(),
            "post_type":"notice",
            "notice_type":"group_increase",
            "sub_type":"approve",
            "groups_id":guild_id.to_owned(),
            "operator_id":user_id.to_owned(),
            "user_id":user_id.to_owned(),
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }

    async fn deal_group_decrease_event(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = data.get("target_id").ok_or("target_id not found")?.as_str().ok_or("target_id not str")?;
        // let welcome_channel_id = self.get_welcome_channel(guild_id).await?;
        let user_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("user_id").ok_or("user_id not found")?
                                .as_str().ok_or("user_id not str")?;
            
        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self.self_id.read().unwrap().to_owned(),
            "post_type":"notice",
            "notice_type":"group_decrease",
            "sub_type":"leave",
            "groups_id":guild_id.to_owned(),
            "operator_id":user_id.to_owned(),
            "user_id":user_id.to_owned(),
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }
    async fn deal_group_recall(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let msg_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("msg_id").ok_or("msg_id not found")?
                                .as_str().ok_or("msg_id not str")?;
        let group_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("channel_id").ok_or("channel_id not found")?
                                .as_str().ok_or("channel_id not str")?;
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let (cq_id,user_id) = self.get_cq_msg_id(msg_id);
        // self.get_msg(msg_id_str).await?;
        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self.self_id.read().unwrap().to_owned(),
            "post_type":"notice",
            "notice_type":"group_recall",
            "groups_id":guild_id.to_owned(),
            "group_id":group_id.to_owned(),
            "user_id": user_id,
            "operator_id":user_id,
            "message_id": cq_id,
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }

    async fn deal_private_recall(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let msg_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("msg_id").ok_or("msg_id not found")?
                                .as_str().ok_or("msg_id not str")?;
        let user_id = data.get("extra").ok_or("extra not found")?
                                .get("body").ok_or("body not found")?
                                .get("author_id").ok_or("author_id not found")?
                                .as_str().ok_or("author_id not str")?;
        let (cq_id,_user_id) = self.get_cq_msg_id(msg_id);
        // self.get_msg(msg_id_str).await?;
        let  event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self.self_id.read().unwrap().to_owned(),
            "post_type":"notice",
            "notice_type":"friend_recall",
            "user_id": user_id,
            "message_id": cq_id,
            "platform":"kook"
        });
        self.send_to_onebot_client(&event_json).await;
        Ok(())
    }
    async fn deal_group_event(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let user_id = data.get("author_id").ok_or("author_id not found")?.as_str().ok_or("author_id not str")?;
        if user_id == "1" { // 系统消息
            let tp = data.get("type").ok_or("type not found")?.as_i64().ok_or("type not i64")?;
            if tp != 255 {
                return Ok(()); // 不是系统消息，直接返回
            }
            let sub_type = data.get("extra").ok_or("extra not found")?.get("type").ok_or("type not found")?.as_str().ok_or("type not str")?;
            if sub_type == "exited_guild" {
                self.deal_group_decrease_event(data).await?;
            } else if sub_type == "joined_guild" {
                self.deal_group_increase_event(data).await?;
            } else if sub_type == "deleted_message" {
                self.deal_group_recall(data).await?;
            }
        } else {
            let self_id = self.self_id.read().unwrap().to_owned();
            if user_id != &self_id {
                self.deal_group_message_event(data,user_id).await?;
            }
            
        }
        Ok(())
    }
    pub async fn get_login_info(&self)-> Result<LoginInfo, Box<dyn std::error::Error + Send + Sync>> {
        let login_info = self.http_get_json("/user/me").await?;
        let user_id = login_info.get("id").ok_or("get id err")?.as_str().ok_or("id not str")?;
        let nickname = login_info.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?;
        let avatar = login_info.get("avatar").ok_or("get avatar err")?.as_str().ok_or("avatar not str")?;
        Ok(LoginInfo {
            user_id:user_id.to_owned(),
            nickname:nickname.to_owned(),
            avatar:avatar.to_owned()
        })
    }

    async fn deal_person_event(&self,data:&serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let user_id = data.get("author_id").ok_or("author_id not found")?.as_str().ok_or("author_id not str")?;
        if user_id == "1" { // 系统消息
            let tp = data.get("type").ok_or("type not found")?.as_i64().ok_or("type not i64")?;
            if tp != 255 {
                return Ok(()); // 不是系统消息，直接返回
            }
            let sub_type = data.get("extra").ok_or("extra not found")?.get("type").ok_or("type not found")?.as_str().ok_or("type not str")?;
            if sub_type == "self_exited_guild" {
                // self.deal_group_kick_me_event(data).await?;
            } else if sub_type == "deleted_private_message" {
                self.deal_private_recall(data).await?;
            }
        } else {
            let self_id = self.self_id.clone();
            if user_id != *self_id.read().unwrap() {
                self.deal_private_message_event(data,user_id).await?;
            }
        }
        Ok(())
    }
    async fn deal_kook_event(&self,data:serde_json::Value)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tp = data.get("channel_type").ok_or("channel_type not found")?.as_str().ok_or("channel_type not str")?;
        if tp == "GROUP" {
            self.deal_group_event(&data).await?;
        }else if tp == "PERSON" {
            self.deal_person_event(&data).await?;
        }
        Ok(())
    }
    async fn conv_event(self:&KookConnect,s:String) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
        let js:serde_json::Value = serde_json::from_str(&s)?;
        let s = js.get("s").ok_or("s not found")?.as_i64().ok_or("s not i64")?;
        if s == 5 {
            cq_add_log_w("要求重连").unwrap();
            return Ok(5);
        }else if s == 1 {
            cq_add_log("连接KOOK成功").unwrap();
        }else if s == 3 {
            cq_add_log("KOOK心跳接收成功").unwrap();
            self.recieve_pong.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        else if s == 0 {
            cq_add_log(&format!("收到KOOK事件:{}", js.to_string())).unwrap();
            let d = js.get("d").ok_or("d not found")?;
            let new_sn = js.get("sn").ok_or("sn not found")?.as_i64().ok_or("sn not i64")?;
            self.sn.store(new_sn, std::sync::atomic::Ordering::Relaxed);
            let rst = self.deal_kook_event(d.clone()).await;
            if rst.is_err() {
                cq_add_log_w(&format!("处理KOOK事件出错:{}",rst.err().unwrap())).unwrap();
            }
        }
        Ok(0)
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
    pub fn make_kook_text(text:&str) -> String {
        let mut s = String::new();
        for it in text.chars() {
            if it == '\\' || it == '*' || it == '~' || it == '[' || it == '(' || it == ')' || it == ']' || it == '-' || it == '>' || it == '`'{
                s.push('\\');
            }
            s.push(it);
        }
        s
    }
    async fn http_post(url:&str,data:Vec<u8>,headers:&HashMap<String, String>,is_post:bool) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
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
    async fn upload_asset(&self,uri:&str)-> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
        
        let uri = reqwest::Url::from_str(&format!("https://www.kookapp.cn/api/v3/asset/create"))?;
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().build()?;
        let form = reqwest::multipart::Form::new().part("file", reqwest::multipart::Part::bytes(file_bin).file_name("test"));
        let mut req = client.post(uri).multipart(form).build()?;
        let token = &self.token;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("Bot {token}"))?);
        let ret = client.execute(req).await?;
        let retbin = ret.bytes().await?.to_vec();
        let ret_str = String::from_utf8(retbin)?;
        let js:serde_json::Value = serde_json::from_str(&ret_str)?;
        let ret = js.get("data").ok_or("get data err")?.get("url").ok_or("url not found")?.as_str().ok_or("url not str")?;
        Ok(ret.to_owned())
    }
    async fn make_kook_msg(&self,message_arr:&serde_json::Value,is_group:bool) -> Result<(Vec<(i32, String)>,String), Box<dyn std::error::Error + Send + Sync>> {
        let mut to_send_data: Vec<(i32, String)> = vec![];
        let mut quote = String::new();
        let mut last_type = 1;
        for it in message_arr.as_array().ok_or("message not arr")? {
            let tp = it.get("type").ok_or("type not found")?;
            if tp == "text"{
                let t = it.get("data").ok_or("data not found")?.get("text").ok_or("text not found")?.as_str().ok_or("text not str")?.to_owned();
                let s = Self::make_kook_text(&t);
                if last_type == 1 && to_send_data.len() != 0 {
                    let l = to_send_data.len();
                    to_send_data.get_mut(l - 1).unwrap().1.push_str(&s);
                } else {
                    to_send_data.push((1,s));
                    last_type = 1;
                }
            } else if tp == "image"{
                let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not str")?;
                let file_url = self.upload_asset(file).await?;
                to_send_data.push((2,file_url));
                last_type = 2;
            }
            else if tp == "at"{
                if !is_group {
                    continue;
                }
                let qq = Self::to_json_str(it.get("data").ok_or("data not found")?.get("qq").ok_or("qq not found")?);
                let at_str = &format!("(met){}(met)",qq);
                if last_type == 1 && to_send_data.len() != 0 {
                    let l = to_send_data.len();
                    to_send_data.get_mut(l - 1).unwrap().1.push_str(at_str);
                } else {
                    to_send_data.push((1,at_str.to_owned()));
                    last_type = 1;
                }
            } else if tp == "reply"{
                if quote !=  "" {
                    continue;
                }
                let cq_id = Self::to_json_str(it.get("data").ok_or("data not found")?.get("id").ok_or("reply not found")?);
                let kook_id = self.get_msg_id(&cq_id);
                quote = kook_id.get(0).ok_or("get kook msg_id err")?.msg_id.to_owned();
            }
            else if tp == "music"{
                let music_type = it.get("data").ok_or("data not found")?.get("type").ok_or("type not found")?.as_str().ok_or("type not str")?;
                if music_type == "custom" {
                    let data = it.get("data").ok_or("data not found")?;
                    let mut audio = read_json_str(data, "audio");
                    if audio == "" {
                        audio = read_json_str(data, "voice");
                    }
                    let title = read_json_str(data, "title");
                    let image = read_json_str(data, "image");
                    let js = serde_json::json!([{
                            "type": "card",
                            "theme": "secondary",
                            "size": "lg",
                            "modules": [
                            {
                                "type": "audio",
                                "title": title,
                                "src": audio,
                                "cover": image
                            }]
                    }]);
                    to_send_data.push((10,js.to_string()));
                    last_type = 10;
                }else if music_type == "163" {
                    let data = it.get("data").ok_or("data not found")?;
                    let id = read_json_str(data, "id");
                    let url = format!("https://api.gumengya.com/Api/Netease?format=json&id={id}");
                    let mut header: HashMap<String, String> = HashMap::new();
                    header.insert("User-Agent".to_owned(), "https://github.com/super1207/KookOneBot".to_owned());
                    let ret = Self::http_post(&url,vec![],&header,false).await?;
                    let ret_json:serde_json::Value = serde_json::from_str(&String::from_utf8(ret)?)?;
                    let music_data = ret_json.get("data").ok_or("data not found")?;
                    let audio = read_json_str(music_data, "url");
                    let title = read_json_str(music_data, "title");
                    let image = read_json_str(music_data, "pic");
                    let js = serde_json::json!([{
                        "type": "card",
                        "theme": "secondary",
                        "size": "lg",
                        "modules": [
                        {
                            "type": "audio",
                            "title": title,
                            "src": audio,
                            "cover": image
                        }]
                    }]);
                    to_send_data.push((10,js.to_string()));
                    last_type = 10;
                }else if music_type == "qq" {
                    let data = it.get("data").ok_or("data not found")?;
                    let id = read_json_str(data, "id");
                    let url = format!("https://api.gumengya.com/Api/Tencent?format=json&id={id}");
                    let mut header: HashMap<String, String> = HashMap::new();
                    header.insert("User-Agent".to_owned(), "https://github.com/super1207/KookOneBot".to_owned());
                    let ret = Self::http_post(&url,vec![],&header,false).await?;
                    let ret_json:serde_json::Value = serde_json::from_str(&String::from_utf8(ret)?)?;
                    let music_data = ret_json.get("data").ok_or("data not found")?;
                    let mut audio = read_json_str(music_data, "url");
                    lazy_static! {
                        static ref AT_REGEX : Regex = Regex::new(
                            r"://(.+)/amobile"
                            ).unwrap();
                    }
                    audio = AT_REGEX.replace_all(&audio, "://aqqmusic.tc.qq.com/amobile").to_string();
                    let title = read_json_str(music_data, "title");
                    let image = read_json_str(music_data, "pic");
                    let js = serde_json::json!([{
                        "type": "card",
                        "theme": "secondary",
                        "size": "lg",
                        "modules": [
                        {
                            "type": "audio",
                            "title": title,
                            "src": audio,
                            "cover": image
                        }]
                    }]);
                    to_send_data.push((10,js.to_string()));
                    last_type = 10;
                }
            }
            else if tp == "record" {
                let data = it.get("data").ok_or("data not found")?;
                let file = read_json_str(data, "file");
                let url = self.upload_asset(&file).await?;
                let js = serde_json::json!([{
                        "type": "card",
                        "theme": "secondary",
                        "size": "lg",
                        "modules": [
                        {
                            "type": "audio",
                            "src": url,
                        }]
                }]);
                to_send_data.push((10,js.to_string()));
                last_type = 10;
            }
            else {
                let j = serde_json::json!([it]);
                let s = json_to_cq_str(&j).map_err(|x|{
                    format!("json_to_cq_str err:{:?}",x)
                })?;
                let s2 = Self::make_kook_text(&s);
                if last_type == 1 && to_send_data.len() != 0 {
                    let l = to_send_data.len();
                    to_send_data.get_mut(l - 1).unwrap().1.push_str(&s2);
                } else {
                    to_send_data.push((1,s2));
                    last_type = 1;
                }
            }
        }
        Ok((to_send_data,quote))
    }
    fn get_json_bool(js:&serde_json::Value,key:&str) -> bool {
        if let Some(j) = js.get(key) {
            if j.is_boolean() {
                return j.as_bool().unwrap();
            } else if j.is_string(){
                if j.as_str().unwrap() == "true" {
                    return true;
                } else {
                    return false;
                }
            }
            else {
                return false;
            }
        } else {
            return false;
        }
    }
    fn get_auto_escape_from_params(&self,params:&serde_json::Value) -> bool {
        let is_auto_escape = Self::get_json_bool(params, "auto_escape");
        return is_auto_escape;
    }
    async fn http_post_json(&self,uri:&str,json:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>{
        let json_str = json.to_string();
        cq_add_log(&format!("发送KOOK_POST:{uri}\n{}", json_str)).unwrap();
        let uri = reqwest::Url::from_str(&format!("https://www.kookapp.cn/api/v3{uri}"))?;
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).no_proxy().build()?;
        let mut req = client.post(uri).body(reqwest::Body::from(json_str)).build()?;
        let token = &self.token;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("Bot {token}"))?);
        req.headers_mut().append(HeaderName::from_str("Content-type")?, HeaderValue::from_str("application/json")?);
        let ret = client.execute(req).await?;
        let retbin = ret.bytes().await?.to_vec();
        let ret_str = String::from_utf8(retbin)?;
        cq_add_log(&format!("KOOK_POST响应:{ret_str}")).unwrap();
        let js:serde_json::Value = serde_json::from_str(&ret_str)?;
        let ret = js.get("data").ok_or("get data err")?;
        Ok(ret.to_owned())
    }
    async fn send_group_msg(&self,tp:i32,group_id:&str,message:&str,quote:&str)-> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["content"] = message.into();
        json["target_id"] = group_id.into();
        json["type"] = tp.into();
        if quote != "" {
            json["quote"] = quote.into();
        }
        let ret_json = self.http_post_json("/message/create",&json).await?;
        let msg_id = ret_json.get("msg_id").ok_or("msg_id not found")?.as_str().ok_or("msg_id not str")?;
        Ok(msg_id.to_owned())
    }
    async fn deal_ob_send_group_msg(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let message_arr:serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;
        
        if message_rst.is_string() {
            if self.get_auto_escape_from_params(&params) {
                message_arr = serde_json::json!(
                    [{"type":"text","data":{
                        "text": message_rst.as_str()
                    }}]
                );
            } else {
                message_arr = str_msg_to_arr(message_rst).map_err(|x|{
                    format!("str_msg_to_arr err:{:?}",x)
                })?;
            }
        }else {
            message_arr = params.get("message").ok_or("message not found")?.to_owned();
        }
        
        let (to_send_data, mut quote) = self.make_kook_msg(&message_arr,true).await?;

        let mut msg_ids = vec![];
        for (tp,msg) in & to_send_data.clone() {
            let msg_id = self.send_group_msg(*tp,&group_id,msg,&quote).await?;
            msg_ids.push(RawMsgId{
                msg_id,
                user_id:self.self_id.read().unwrap().to_owned(),
            });
            quote = "".to_owned();
        }
        let msg_id = self.add_msg_id(&msg_ids);
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
    async fn delete_msg(&self,msg_id:&str)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["msg_id"] = msg_id.into();
        let _ret_json = self.http_post_json("/message/delete",&json).await?;
        Ok(())
    }
    async fn deal_ob_delete_msg(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let msg_id = read_json_str(params,"message_id");
        let msg_ids = self.get_msg_id(&msg_id);
        for it in msg_ids {
            self.delete_msg(&it.msg_id).await?;
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
        let info: LoginInfo = self.get_login_info().await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn get_stranger_info(&self,user_id:&str,use_cache:bool)-> Result<StrangerInfo, Box<dyn std::error::Error + Send + Sync>> {
        let stranger_info = self.http_get_json_t(&format!("/user/view?user_id={user_id}"),use_cache).await?;
        let user_id = stranger_info.get("id").ok_or("get id err")?.as_str().ok_or("id not str")?;
        let nickname = stranger_info.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?;
        Ok(StrangerInfo {
            user_id:user_id.to_owned(),
            nickname:nickname.to_owned(),
            sex:"unknown".to_owned(),
            age:0,
            avatar:stranger_info.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?.to_owned()
        })
    }
    async fn deal_ob_get_stranger_info(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let user_id = read_json_str(params,"user_id");
        let use_cache = !Self::get_json_bool(params,"no_cache");
        let info = self.get_stranger_info(&user_id,use_cache).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn get_group_info(&self,group_id:&str,use_cache:bool)-> Result<GroupInfo, Box<dyn std::error::Error + Send + Sync>> {
        let stranger_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),use_cache).await?;
        let group_id = stranger_info.get("id").ok_or("get id err")?.as_str().ok_or("id not str")?;
        let group_name = stranger_info.get("name").ok_or("get name err")?.as_str().ok_or("name not str")?;
        Ok(GroupInfo {
            group_id:group_id.to_owned(),
            group_name:group_name.to_owned(),
            member_count:0,
            max_member_count:0
        })
    }
    async fn deal_ob_get_group_info(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let use_cache = !Self::get_json_bool(params,"no_cache");
        let info = self.get_group_info(&group_id,use_cache).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }

    async fn get_group_list(&self,guild_id:&str) -> Result<Vec<GroupInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let ret_json = self.http_get_json_t(&format!("/channel/list?guild_id={guild_id}"),false).await?;
        let channel_arr = ret_json.get("items").ok_or("get items err")?.as_array().ok_or("items not arr")?;
        let mut ret_arr = vec![];
        for it2 in channel_arr {
            let id = it2.get("id").ok_or("get id err")?.as_str().ok_or("id not str")?;

            let group_name = it2.get("name").ok_or("get name err")?.as_str().ok_or("name not str")?;

            let tp = it2.get("type").ok_or("get type err")?.as_i64().ok_or("type not i64")?;
            let is_category = Self::get_json_bool(it2, "is_category");

            if !is_category && tp == 1 {
                ret_arr.push(GroupInfo {
                    group_id:id.to_owned(),
                    group_name:group_name.to_owned(),
                    member_count:0,
                    max_member_count:0
                });
            }
        }
        Ok(ret_arr)
    }
    async fn deal_ob_get_group_list(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = read_json_str(params, "groups_id");
        let info = self.get_group_list(&guild_id).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn get_group_member_info(&self,group_id:&str,user_id:&str,use_cache:bool)-> Result<GroupMemberInfo, Box<dyn std::error::Error + Send + Sync>> {
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),use_cache).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let stranger_info = self.http_get_json_t(&format!("/user/view?user_id={user_id}&guild_id={guild_id}"),use_cache).await?;
        let guild_info = self.http_get_json_t(&format!("/guild/view?guild_id={guild_id}"),use_cache).await?;
        let owner_id = guild_info.get("user_id").ok_or("get user_id err")?.as_str().ok_or("user_id not str")?;
        let role;
        if owner_id == user_id {
            role = "owner";
        }else {
            let roles = stranger_info.get("roles").ok_or("get roles err")?.as_array().ok_or("roles not arr")?;
            if roles.len() != 0 { 
                role = "admin";
            } else {
                role = "member";
            }
        }
        Ok(GroupMemberInfo {
            group_id:group_id.to_owned(),
            user_id:user_id.to_owned(),
            groups_id:guild_id.to_owned(),
            nickname:stranger_info.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?.to_owned(),
            card:stranger_info.get("nickname").ok_or("get nickname err")?.as_str().ok_or("nickname not str")?.to_owned(),
            sex:"unknown".to_owned(),
            age:0,
            area:"".to_owned(),
            join_time:(stranger_info.get("joined_at").ok_or("get joined_at err")?.as_u64().ok_or("joined_at not u64")? / 1000) as i32,
            last_sent_time:(stranger_info.get("active_time").ok_or("get active_time err")?.as_u64().ok_or("active_time not u64")? / 1000) as i32,
            level:"0".to_owned(),
            role:role.to_owned(),
            unfriendly:false,
            title:"".to_owned(),
            title_expire_time:0,
            card_changeable:false,
            avatar:stranger_info.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?.to_owned()
        })
    }
    async fn deal_ob_get_group_member_info(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let user_id = read_json_str(params,"user_id");
        let use_cache = !Self::get_json_bool(params,"no_cache");
        let info = self.get_group_member_info(&group_id, &user_id,use_cache).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn set_group_kick(&self,group_id:&str,user_id:&str)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["guild_id"] = guild_id.into();
        json["target_id"] = user_id.into();
        let _ret_json = self.http_post_json("/guild/kickout",&json).await?;
        Ok(())
    }
    async fn deal_ob_set_group_kick(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let user_id = read_json_str(params,"user_id");
        self.set_group_kick(&group_id, &user_id).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
    }
    async fn set_group_leave(&self,group_id:&str)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["guild_id"] = guild_id.into();
        let _ret_json = self.http_post_json("/guild/leave",&json).await?;
        Ok(())
    }
    async fn deal_ob_set_group_leave(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        self.set_group_leave(&group_id).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
    }
    async fn set_group_name(&self,group_id:&str,name:&str)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["channel_id"] = group_id.into();
        json["name"] = name.into();
        let _ret_json = self.http_post_json("/channel/update",&json).await?;
        Ok(())
    }
    async fn deal_ob_set_group_name(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let group_name = read_json_str(params,"group_name");
        self.set_group_name(&group_id,&group_name).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
    }
    async fn set_group_card(&self,group_id:&str,user_id:&str,card:&str)-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let mut json:serde_json::Value = serde_json::from_str("{}")?;
        json["guild_id"] = guild_id.into();
        json["user_id"] = user_id.into();
        json["nickname"] = card.into();
        let _ret_json = self.http_post_json("/guild/nickname",&json).await?;
        Ok(())
    }
    async fn deal_ob_set_group_card(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let user_id = read_json_str(params,"user_id");
        let card = read_json_str(params,"card");
        self.set_group_card(&group_id,&user_id,&card).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": {},
            "echo":echo
        });
        Ok(send_json)
    }
    pub async fn get_friend_list(&self)-> Result<Vec<FriendInfo>, Box<dyn std::error::Error + Send + Sync>> {

        let mut ret_vec = vec![];
        let friend_list = self.http_get_json_t(&format!("/user-chat/list"),false).await?;
        for it in friend_list.get("items").ok_or("items not found")?.as_array().ok_or("items not arr")? {
            let target_info = it.get("target_info").ok_or("target_info not found")?;
            let id = target_info.get("id").ok_or("id not found")?.as_str().ok_or("id not str")?;
            let username = target_info.get("username").ok_or("username not found")?.as_str().ok_or("username not str")?;
            let avatar = target_info.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?;
            ret_vec.push(FriendInfo {
                user_id: id.to_owned(),
                nickname: username.to_owned(),
                remark: username.to_owned(),
                avatar: avatar.to_owned()
            });
        }
        let meta = friend_list.get("meta").ok_or("meta not found")?;
        let page_total = meta.get("page_total").ok_or("page_total not found")?.as_i64().ok_or("page_total not i32")?;
        for page in 1..page_total{
            let friend_list = self.http_get_json_t(&format!("/user-chat/list?page={page}"),false).await?;
            for it in friend_list.get("items").ok_or("items not found")?.as_array().ok_or("items not arr")? {
                let target_info = it.get("target_info").ok_or("target_info not found")?;
                let id = target_info.get("id").ok_or("id not found")?.as_str().ok_or("id not str")?;
                let username = target_info.get("username").ok_or("username not found")?.as_str().ok_or("username not str")?;
                let avatar = target_info.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?;
                ret_vec.push(FriendInfo {
                    user_id: id.to_owned(),
                    nickname: username.to_owned(),
                    remark: username.to_owned(),
                    avatar: avatar.to_owned()
                });
            }
        }
        Ok(ret_vec)
    }
    async fn deal_ob_get_friend_list(&self,_params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let info = self.get_friend_list().await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn get_group_member_list(&self,group_id:&str) -> Result<Vec<GroupMemberInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let group_info = self.http_get_json_t(&format!("/channel/view?target_id={group_id}"),true).await?;
        let guild_id = group_info.get("guild_id").ok_or("get guild_id err")?.as_str().ok_or("guild_id not str")?;
        let mut ret_vec:Vec<GroupMemberInfo> = vec![];
        let ret_json = self.http_get_json_t(&format!("/guild/user-list?guild_id={guild_id}"),false).await?;
        let items = ret_json.get("items").ok_or("get items err")?.as_array().ok_or("items not arr")?;
        for it in items {
            let role;
            let is_master = Self::get_json_bool(it, "is_master");
            if is_master {
                role = "owner";
            }else{
                let roles = it.get("roles").ok_or("get roles err")?.as_array().ok_or("roles not arr")?;
                if roles.len() != 0 { 
                    role = "admin";
                } else {
                    role = "member";
                }
            }
            let user_id = read_json_str(it, "id");
            let info = GroupMemberInfo {
                group_id:group_id.to_owned(),
                user_id:user_id.to_owned(),
                groups_id:guild_id.to_owned(),
                nickname:it.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?.to_owned(),
                card:it.get("nickname").ok_or("get nickname err")?.as_str().ok_or("nickname not str")?.to_owned(),
                sex:"unknown".to_owned(),
                age:0,
                area:"".to_owned(),
                join_time:(it.get("joined_at").ok_or("get joined_at err")?.as_u64().ok_or("joined_at not u64")? / 1000) as i32,
                last_sent_time:(it.get("active_time").ok_or("get active_time err")?.as_u64().ok_or("active_time not u64")? / 1000) as i32,
                level:"0".to_owned(),
                role:role.to_owned(),
                unfriendly:false,
                title:"".to_owned(),
                title_expire_time:0,
                card_changeable:false,
                avatar:it.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?.to_owned()
            };
            ret_vec.push(info);
        }
        let meta = ret_json.get("meta").ok_or("meta not found")?;
        let page_total = meta.get("page_total").ok_or("page_total not found")?.as_i64().ok_or("page_total not i32")?;
        for page in 1..page_total {
            let ret_json = self.http_get_json_t(&format!("/guild/user-list?guild_id={guild_id}&page={page}"),false).await?;
            for it in ret_json.get("items").ok_or("items not found")?.as_array().ok_or("items not arr")? {
                let role;
                let is_master = Self::get_json_bool(it, "is_master");
                if is_master {
                    role = "owner";
                }else{
                    let roles = it.get("roles").ok_or("get roles err")?.as_array().ok_or("roles not arr")?;
                    if roles.len() != 0 { 
                        role = "admin";
                    } else {
                        role = "member";
                    }
                }
                let user_id = read_json_str(it, "id");
                let info = GroupMemberInfo {
                    group_id:group_id.to_owned(),
                    user_id:user_id.to_owned(),
                    groups_id:guild_id.to_owned(),
                    nickname:it.get("username").ok_or("get username err")?.as_str().ok_or("username not str")?.to_owned(),
                    card:it.get("nickname").ok_or("get nickname err")?.as_str().ok_or("nickname not str")?.to_owned(),
                    sex:"unknown".to_owned(),
                    age:0,
                    area:"".to_owned(),
                    join_time:(it.get("joined_at").ok_or("get joined_at err")?.as_u64().ok_or("joined_at not u64")? / 1000) as i32,
                    last_sent_time:(it.get("active_time").ok_or("get active_time err")?.as_u64().ok_or("active_time not u64")? / 1000) as i32,
                    level:"0".to_owned(),
                    role:role.to_owned(),
                    unfriendly:false,
                    title:"".to_owned(),
                    title_expire_time:0,
                    card_changeable:false,
                    avatar:it.get("avatar").ok_or("avatar not found")?.as_str().ok_or("avatar not str")?.to_owned()
                };
                ret_vec.push(info);
            }
        }
        Ok(ret_vec)
    }
    async fn deal_ob_get_group_member_list(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let group_id = read_json_str(params,"group_id");
        let info = self.get_group_member_list(&group_id).await?;
        let send_json = serde_json::json!({
            "status":"ok",
            "retcode":0,
            "data": info,
            "echo":echo
        });
        Ok(send_json)
    }
    async fn deal_ob_get_cookies(&self,params:&serde_json::Value,echo:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let domain = read_json_str(params,"domain");
        if domain == "token" {
            let send_json = serde_json::json!({
                "status":"ok",
                "retcode":0,
                "data": {
                    "cookies":self.token
                },
                "echo":echo
            });
            return Ok(send_json);
        }
        return None.ok_or(format!("`{domain}` not support"))?;
    }

    
}

#[async_trait]
impl BotConnectTrait for KookConnect {

    async fn disconnect(&mut self){
        self.is_stop.store(true,std::sync::atomic::Ordering::Relaxed);
        if self.stop_tx.is_some() {
            let _foo = self.stop_tx.clone().unwrap().send_timeout(true, Duration::from_secs(1)).await;
        }
    }

    fn get_alive(&self) -> bool {
        return !self.is_stop.load(std::sync::atomic::Ordering::Relaxed);
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(7..).ok_or("kook url格式错误")?;
        let config_json:serde_json::Value =  serde_json::from_str(config_json_str)?;
        self.token = config_json.get("Token").ok_or("kook url格式错误:没有Token")?.as_str().ok_or("kook url格式错误:Token不是字符串")?.to_owned();

        let login_info = self.get_login_info().await?;
        *self.self_id.write().unwrap() = login_info.user_id; 

        let wss_url = self.get_gateway().await?;
        let (ws_stream, _) = connect_async(wss_url).await?;
        let (mut write_halt,mut read_halt) = ws_stream.split();
        let sn_ptr = self.sn.clone();
        let is_stop = self.is_stop.clone();
        let recieve_pong = self.recieve_pong.clone();
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx.clone());
        let stop_tx = stoptx.clone();
        tokio::spawn(async move {
            let mut index = 0;
            let mut index_lost_pong = -1;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                index += 1;

                if index == 7 {
                    if recieve_pong.load(std::sync::atomic::Ordering::Relaxed) {
                        recieve_pong.store(false, std::sync::atomic::Ordering::Relaxed);
                        index_lost_pong = 0;
                    } else {
                        index_lost_pong += 1;
                    }
                    if index_lost_pong >= 2 {
                        cq_add_log_w("接收KOOK心跳失败").unwrap();
                        break;
                    }
                }

                if index == 30 {
                    index = 0;
                    let json_str = serde_json::json!({
                        "s": 2,
                        "sn": sn_ptr.load(std::sync::atomic::Ordering::Relaxed)
                    }).to_string();
                    // cq_add_log(&format!("发送KOOK心跳:{json_str}")).unwrap();
                    let foo = write_halt.send(tungstenite::Message::Text(json_str)).await;
                    if foo.is_err() {
                        cq_add_log_w("发送KOOK心跳发送失败").unwrap();
                        break;
                    }else {
                        cq_add_log("KOOK心跳发送成功").unwrap();
                    }
                }
            }
            // 断开连接
            is_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _foo = stop_tx.send_timeout(true, Duration::from_secs(1)).await;
            cq_add_log_w("KOOK心跳断开").unwrap();
        });
        let is_stop = self.is_stop.clone();
        let url_str_t = self.url.clone();
        let kobj = self.clone();
        let stop_tx = stoptx.clone();
        tokio::spawn(async move {
            let is_stop = is_stop;
            let stop_tx = stop_tx.clone();
            loop {
                let is_stop = is_stop.clone();
                let stop_tx = stop_tx.clone();
                let kobj = kobj.clone();
                if is_stop.clone().load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                    Some(msg_rst) = read_halt.next() => {
                        if let Ok(raw_msg) = msg_rst {
                            let bin = raw_msg.into_data();
                            let mut d = ZlibDecoder::new(bin.as_slice());
                            let mut s = String::new();
                            if let Ok(_) = d.read_to_string(&mut s) {
                                tokio::spawn(async move {
                                    let is_stop = is_stop.clone();
                                    let stop_tx = stop_tx.clone();
                                    let kobj = kobj.clone();
                                    let rst = KookConnect::conv_event(&kobj,s).await;
                                    if rst.is_err() {
                                        crate::cqapi::cq_add_log(format!("{:?}", rst.err().unwrap()).as_str()).unwrap();
                                    } else {
                                        let code = rst.unwrap();
                                        // 断开连接
                                        if code != 0 {
                                            is_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                            let _foo = stop_tx.clone().send_timeout(true, Duration::from_secs(1)).await;
                                        }
                                    }
                                });
                            }else {
                                cq_add_log_w(&format!("kook ws获取数据错误2")).unwrap();
                                break;
                            }
                        }else {
                            cq_add_log_w(&format!("kook ws获取数据错误1")).unwrap();
                            break;
                        }
                    },
                    _ = stoprx.recv() => {  
                        break;
                    }
                }
            }
            // 移除conn
            is_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            cq_add_log_w(&format!("kook连接已经断开(read_halt):{url_str_t}")).unwrap();
        });
        Ok(())
    }

    
    
    async fn call_api(&self,_platform:&str,_self_id:&str,_passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = json.get("action").ok_or("action not found")?.as_str().ok_or("action not str")?;
        let echo = json.get("echo").unwrap_or(&serde_json::Value::Null);
        let def = serde_json::json!({});
        let params = json.get("params").unwrap_or(&def);
        let send_json = match action {
            "send_group_msg" => {
                self.deal_ob_send_group_msg(&params,&echo).await?
            },
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
            "get_stranger_info" => {
                self.deal_ob_get_stranger_info(&params,&echo).await?
            },
            "get_group_info" => {
                self.deal_ob_get_group_info(&params,&echo).await?
            },
            "get_group_list" => {
                self.deal_ob_get_group_list(&params,&echo).await?
            },
            "get_group_member_info" => {
                self.deal_ob_get_group_member_info(&params,&echo).await?
            },
            "set_group_kick" => {
                self.deal_ob_set_group_kick(&params,&echo).await?
            },
            "set_group_leave" => {
                self.deal_ob_set_group_leave(&params,&echo).await?
            },
            "set_group_name" => {
                self.deal_ob_set_group_name(&params,&echo).await?
            },
            "set_group_card" => {
                self.deal_ob_set_group_card(&params,&echo).await?
            },
            "get_friend_list" => {
                self.deal_ob_get_friend_list(&params,&echo).await?
            },
            "get_group_member_list" => {
                self.deal_ob_get_group_member_list(&params,&echo).await?
            },
            "get_cookies" => {
                self.deal_ob_get_cookies(&params,&echo).await?
            },
            "can_send_image" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {"yes":true},
                    "echo":echo
                })
            },
            "can_send_record" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {"yes":false},
                    "echo":echo
                })
            },
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
            },
            "get_version_info" => {
                serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {
                        "app_name":"kook_redreply",
                        "app_version":"0.0.1",
                        "protocol_version":"v1"
                    },
                    "echo":echo
                })
            },
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

    fn get_platform_and_self_id(&self) -> Vec<(String,String)> {
        let lk = self.self_id.read().unwrap();
        if lk.is_empty() {
            return vec![];
        }
        return vec![("kook".to_owned(),lk.to_owned())];
    }
}


#[derive(Serialize, Deserialize, Debug)]
pub struct LoginInfo {
    pub user_id:String,
    pub nickname:String,
    avatar:String
}

#[derive(Serialize, Deserialize, Debug,Clone)]
struct GroupMemberInfo {
    group_id:String,
    groups_id:String,
    user_id:String,
    nickname:String,
    card:String,
    sex:String,
    age:i32,
    area:String,
    join_time:i32,
    last_sent_time:i32,
    level:String,
    role:String,
    unfriendly:bool,
    title:String,
    title_expire_time:i32,
    card_changeable:bool,
    avatar:String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FriendInfo {
    user_id:String,
    nickname:String,
    remark:String,
    avatar:String
}

#[derive(Serialize, Deserialize, Debug)]
struct StrangerInfo {
    user_id:String,
    nickname:String,
    sex:String,
    age:i32,
    avatar:String
}

#[derive(Serialize, Deserialize, Debug)]
struct GroupInfo {
    group_id:String,
    group_name:String,
    member_count:i32,
    max_member_count:i32
}
