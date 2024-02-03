use std::{collections::HashMap, path::Path, str::FromStr, sync::{atomic::AtomicBool, Arc, Mutex, RwLock, Weak}};

use async_trait::async_trait;
use reqwest::header::{HeaderName, HeaderValue};

use crate::{cqapi::cq_add_log_w, mytool::{cq_text_encode, read_json_obj, read_json_or_default, read_json_str, str_msg_to_arr}};

use super::BotConnectTrait;


#[derive(Debug)]
pub struct NTQQV1Connect {
    pub self_id:Arc<std::sync::RwLock<String>>,
    pub url:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx :Option<tokio::sync::mpsc::Sender<bool>>,
    pub flag:Arc<std::sync::Mutex<String>>
}

lazy_static!{
    static ref G_UIN_UID_MAP:RwLock<HashMap<String,String>> = RwLock::new(HashMap::new());
}

pub fn str_msg_to_arr_safe(js:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let ret = str_msg_to_arr(js);
    if let Ok(ret) = ret {
        return Ok(ret);
    }else {
        return None.ok_or(format!("str_msg_to_arr error:{}", ret.err().unwrap()))?;
    }
}

async fn http_post(url:&str,json_data:&serde_json::Value,is_post:bool,flag:Weak<std::sync::Mutex<String>>) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let mut uri = reqwest::Url::from_str(url)?;
    client = reqwest::Client::builder().no_proxy().build()?;
    
    let mut req;
    if is_post {
        req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(reqwest::header::HeaderName::from_str("Content-Type")?, reqwest::header::HeaderValue::from_str("application/json")?);
    }else{
        if let Some(f) = flag.upgrade() {
            uri.query_pairs_mut().append_pair("flag", &f.lock().unwrap());
        }
        // println!("{}",uri.as_str());
        req = client.get(uri).build()?;
    }
    
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    //crate::cqapi::cq_add_log(&format!("接收数据:{ret_str}")).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    return Ok(json_val);
}

async fn http_get(url:&str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let uri = reqwest::Url::from_str(url)?;
    client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().insert(HeaderName::from_str("User-Agent")?, HeaderValue::from_str("Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.72 Safari/537.36")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.bytes().await?.to_vec();
    return Ok(ret_str);
}



impl NTQQV1Connect {
    pub fn build(url:&str) -> Self {
        NTQQV1Connect {
            self_id:Arc::new(RwLock::new("".to_owned())),
            url:url.to_owned(),
            tx:None,
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            flag:Arc::new(Mutex::new(uuid::Uuid::new_v4().to_string())),
        }
    }
}


#[derive(Clone)]
pub struct SelfData {
    pub bot_id:Weak<std::sync::RwLock<String>>
}


async fn deal_group_event(self_t:&SelfData,root:serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // let peer_opt = read_json_obj(&root, "peer");
    // if peer_opt.is_none(){
    //     return Ok(());
    // }
    // let peer = peer_opt.unwrap();

    let raw_opt = read_json_obj(&root, "raw");
    if raw_opt.is_none(){
        return Ok(());
    }
    let raw = raw_opt.unwrap();

    let group_id = read_json_str(&raw, "peerUin");
    let user_id = read_json_str(&raw, "senderUin");
    let user_uid = read_json_str(&raw, "senderUid");
    {
        let mut lk = G_UIN_UID_MAP.write().unwrap();
        lk.insert(user_id.to_owned(), user_uid);
    }
    let card = read_json_str(&raw, "sendMemberName");
    let nickname = read_json_str(&raw, "sendNickName");
    let tm_str = read_json_str(&raw, "msgTime");
    let tm = tm_str.parse::<i64>()?;
    let message_id = read_json_str(&raw, "msgId");

    let elements_t = raw.get("elements").ok_or("no elements in raw")?;
    let elements = elements_t.as_array().ok_or("elements not array")?;
    let mut message = String::new();

    for ele in elements {
        let tp = read_json_str(&ele, "elementType");
        if tp == "1" { //text or at
            let text_element = read_json_or_default(&ele, "textElement",&serde_json::Value::Null);
            
            let at_uid = read_json_str(&text_element, "atUid");
            if at_uid != "0" {
                let at_nt_uid = read_json_str(&text_element, "atNtUid");
                {
                    let mut lk = G_UIN_UID_MAP.write().unwrap();
                    lk.insert(at_uid.to_owned(), at_nt_uid);
                }
                message.push_str(&format!("[CQ:at,qq={at_uid}]"));
            } else {
                let content = read_json_str(&text_element, "content");
                message.push_str(&cq_text_encode(&content));
            }
        }
    }
    let mut bot_id = String::new();
    if let Some(bot_id_t) = self_t.bot_id.upgrade() {
        let k = bot_id_t.read().unwrap();
        bot_id = (*k).to_owned();
    }
    let event_json = serde_json::json!({
        "time":tm,
        "self_id":bot_id,
        "platform":"ntqqv1",
        "post_type":"message",
        "message_type":"group",
        "sub_type":"normal",
        "message_id":message_id,
        "group_id":group_id,
        "user_id":user_id,
        "message":message,
        "raw_message":message,
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
    // cq_add_log_w(&format!("{}",event_json.to_string())).unwrap();
    tokio::task::spawn_blocking(move ||{
        if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
        }
    });
    Ok(())
}

async fn conv_event(self_t:&SelfData,root:serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("ret_json:{}",root.to_string());
    let event_name = read_json_str(&root, "event_name");
    if event_name == "new-messages" {
        let peer_opt = read_json_obj(&root, "peer");
        if peer_opt.is_none(){
            return Ok(());
        }
        let peer = peer_opt.unwrap();
        let chat_type = read_json_str(&peer, "chatType");
        if chat_type == "group" {
            deal_group_event(self_t,root).await?;
        }
    }
    Ok(())
}


#[async_trait]
impl BotConnectTrait for NTQQV1Connect {

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

        let config_json_str = self.url.get(9..).ok_or("ntqqv1 url格式错误")?.to_owned();
        let url_t = format!("http://{config_json_str}");
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let (stoptx, _) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        let ret = http_post(&url_t, &serde_json::json!({
                "action":"getAccountInfo",
                "params":[]
        }), true,Weak::new()).await?;
        let uin = read_json_str(&ret, "uin");

        let uid = read_json_str(&ret, "uid");

        if uid == "" || uin == "" {
            return Err("无法获得账号信息".into());
        }

        {
            let mut lk = G_UIN_UID_MAP.write().unwrap();
            lk.insert(uin.to_owned(), uid);
        }
        

        self.self_id = Arc::new(RwLock::new(uin));
        let self_id_ptr = Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.self_id);
        let flag_ptr = Arc::<std::sync::Mutex<std::string::String>>::downgrade(&self.flag);
        let self_data = SelfData{bot_id:self_id_ptr};
        tokio::spawn(async move {
            loop {
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }else {
                    break; 
                }
                let ret_rst = http_post(&url_t, &serde_json::Value::Null, false,flag_ptr.clone()).await;
                if ret_rst.is_err() {
                    break;
                }
                let root = ret_rst.unwrap();
                
                let null_arr_json = serde_json::json!([]);
                let data_arr:&serde_json::Value = read_json_or_default(&root, "data",&null_arr_json);
                if data_arr.is_array() {
                    for data in data_arr.as_array().unwrap() {
                        let data_t = data.to_owned();
                        let self_data_t = self_data.clone();
                        tokio::spawn(async move {
                            if let Err(err) = conv_event(&self_data_t,data_t).await{
                                cq_add_log_w(&format!("err:{err:?}")).unwrap();
                            }
                        });
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            // 移除conn
            if let Some(val) = is_stop.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            cq_add_log_w(&format!("ntqqv1连接已经断开:{config_json_str}")).unwrap();
        });
        
        Ok(())
    }

    fn get_url(&self) -> String {
        return self.url.clone();
    }

    async fn call_api(&self,_platform:&str,_self_id:&str,_passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(&json, "action");
        let config_json_str = self.url.get(9..).ok_or("ntqqv1 url格式错误")?.to_owned();
        let url_t = format!("http://{config_json_str}");
        let params = read_json_or_default(json, "params",&serde_json::Value::Null);
        if action == "send_group_msg" {
            let group_id = read_json_str(&params, "group_id");
            // 获得消息(数组格式)
            let mut message = params.get("message").ok_or("message is not exist")?.to_owned();
            if message.is_string() {
                message = str_msg_to_arr_safe(&message)?;
            }
            let mut nt_msg = vec![];
            let msg_arr = message.as_array().unwrap();
            for msg_node in msg_arr {
                let tp = read_json_str(msg_node, "type");
                let data = read_json_or_default(msg_node, "data", &serde_json::Value::Null);
                if tp == "text" {
                    nt_msg.push(serde_json::json!({
                        "type": "text",
                        "content": read_json_str(data, "text")
                    }));
                }else if tp == "at" {
                    let uin = read_json_str(data, "qq");
                    let lk = G_UIN_UID_MAP.read().unwrap();
                    if let Some(uid) = lk.get(&uin) {
                        nt_msg.push(serde_json::json!({
                            "type": "text",
                            "content": "",
                            "atType":2,
                            "atUid":uin,
                            "atNtUid":uid,
                        }));
                    }
                }
                else if tp == "image" {
                    let file = read_json_str(data, "file");
                    let file_dir;
                    use md5::{Md5, Digest};
                    if file.starts_with("base64://") {
                        let b64_str = file.split_at(9).1;
                        let content = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                            &base64::alphabet::STANDARD,
                            base64::engine::general_purpose::PAD), b64_str)?;
                        let tmpdir = crate::cqapi::get_tmp_dir()?;
                        let mut hasher = Md5::new();
                        hasher.update(content.clone());
                        let result = hasher.finalize();
                        let mut filename = String::new();
                        for ch in result {
                            filename.push_str(&format!("{:02x}",ch));
                        }
                        file_dir = tmpdir + &filename + ".img";
                        let path = Path::new(&file_dir);
                        if !path.is_file() {
                            tokio::fs::write(file_dir.clone(), content).await?;
                        }
                        
                    }else if file.starts_with("http"){
                        let content = http_get(&file).await?;
                        let tmpdir = crate::cqapi::get_tmp_dir()?;
                        let mut hasher = Md5::new();
                        hasher.update(content.clone());
                        let result = hasher.finalize();
                        let mut filename = String::new();
                        for ch in result {
                            filename.push_str(&format!("{:02x}",ch));
                        }
                        file_dir = tmpdir + &filename + ".img";
                        let path = Path::new(&file_dir);
                        if !path.is_file() {
                            tokio::fs::write(file_dir.clone(), content).await?;
                        }
                    }else {
                        let sp = std::path::MAIN_SEPARATOR.to_string();
                        if sp == "\\" { // windows file = file:///
                            file_dir = file.split_at(8).1.to_owned();
                        }else{ // linux file = file://
                            file_dir = file.split_at(7).1.to_owned();
                        }
                    }
                    nt_msg.push(serde_json::json!({
                        "type": "image",
                        "file":file_dir,
                    }));
                }else if tp == "record" {
                    let file = read_json_str(data, "file");
                    let file_dir;
                    use md5::{Md5, Digest};
                    if file.starts_with("base64://") {
                        let b64_str = file.split_at(9).1;
                        let content = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                            &base64::alphabet::STANDARD,
                            base64::engine::general_purpose::PAD), b64_str)?;
                        let tmpdir = crate::cqapi::get_tmp_dir()?;
                        let mut hasher = Md5::new();
                        hasher.update(content.clone());
                        let result = hasher.finalize();
                        let mut filename = String::new();
                        for ch in result {
                            filename.push_str(&format!("{:02x}",ch));
                        }
                        file_dir = tmpdir + &filename + ".ptt";
                        let path = Path::new(&file_dir);
                        if !path.is_file() {
                            tokio::fs::write(file_dir.clone(), content).await?;
                        }
                        
                    }else if file.starts_with("http"){
                        let content = http_get(&file).await?;
                        let tmpdir = crate::cqapi::get_tmp_dir()?;
                        let mut hasher = Md5::new();
                        hasher.update(content.clone());
                        let result = hasher.finalize();
                        let mut filename = String::new();
                        for ch in result {
                            filename.push_str(&format!("{:02x}",ch));
                        }
                        file_dir = tmpdir + &filename + ".ptt";
                        let path = Path::new(&file_dir);
                        if !path.is_file() {
                            tokio::fs::write(file_dir.clone(), content).await?;
                        }
                    }else {
                        let sp = std::path::MAIN_SEPARATOR.to_string();
                        if sp == "\\" { // windows file = file:///
                            file_dir = file.split_at(8).1.to_owned();
                        }else{ // linux file = file://
                            file_dir = file.split_at(7).1.to_owned();
                        }
                    } 
                    nt_msg.push(serde_json::json!({
                        "type": "ptt",
                        "file":file_dir,
                    }));
                }
            }
            // cq_add_log_w(&format!("nt_msg:{nt_msg:?}")).unwrap();
            http_post(&url_t, &serde_json::json!({
                "action":"sendMessage",
                "params":[{
                            "uid": group_id,
                            "chatType": "group"
                        },nt_msg],
                "timeout":0
            }), true,Weak::new()).await?;
            return Ok(serde_json::json!({
                "retcode":0,
                "status":"ok",
                "data":{
                    "message_id":uuid::Uuid::new_v4().to_string()
                }
            }));
        }else if action == "send_like" {
            let user_id = read_json_str(params, "user_id");
            let mut times = 1;
            let tms = read_json_str(params, "times");
            if tms != "" {
                times = tms.parse::<i32>()?;
            }
            let mut uid = String::new();
            {
                let lk = G_UIN_UID_MAP.read().unwrap();
                if let Some(v) = lk.get(&user_id) {
                    uid = v.to_owned();
                }
            }
            if uid != "" {
                http_post(&url_t, &serde_json::json!({
                    "action":"addLike",
                    "params":[uid,times],
                    "timeout":0
                }), true,Weak::new()).await?;
                return Ok(serde_json::json!({
                    "retcode":0,
                    "status":"ok",
                    "data":{}
                }));
            }
        }
        return Ok(serde_json::Value::Null);
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        let lk = self.self_id.read().unwrap();
        let self_id = (*lk).clone();
        let platform = "ntqqv1".to_owned();
        return vec![(platform,self_id)];
    }
}