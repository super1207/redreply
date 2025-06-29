use std::{collections::{HashMap, HashSet}, ops::{Index, IndexMut}, str::FromStr, sync::{atomic::AtomicBool, Arc, RwLock}};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use hyper::header::HeaderValue;
use crate::mytool::all_to_silk::all_to_silk;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite, connect_async};

use crate::{cqapi::{cq_add_log, cq_add_log_w, cq_get_app_directory1}, mytool::{read_json_str, str_msg_to_arr}, RT_PTR};

use super::BotConnectTrait;

use base64::{Engine as _, engine::{self, general_purpose}, alphabet};
const BASE64_CUSTOM_ENGINE: engine::GeneralPurpose = engine::GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

#[derive(Debug)]
pub struct OneBot11Connect {
    pub self_id:Arc<std::sync::RwLock<String>>,
    pub url:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx :Option<tokio::sync::mpsc::Sender<bool>>,
    pub real_platform:Arc<std::sync::RwLock<Option<String>>>
}

lazy_static! {
    static ref G_ECHO_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<serde_json::Value>>> = tokio::sync::RwLock::new(HashMap::new());
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


impl OneBot11Connect {
    pub fn build(url:&str) -> Self {
        OneBot11Connect {
            self_id:Arc::new(RwLock::new("".to_owned())), 
            url:url.to_owned(),
            tx:None,
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            real_platform:Arc::new(RwLock::new(None))
        }
    }

    async fn get_poke_segment(&self, json: &mut serde_json::Value) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let platform = self.get_platform().await?;
        if !(platform == "lagrange" || platform == "llonebot" || platform == "napcat") {
            return Ok(vec![]);
        }
        let mut ret_ids = Vec::new();
        let params = json.get_mut("params").ok_or("params is none")?;
        if let Some(message) = params.get_mut("message") {
            let msg_arr = message.as_array_mut().ok_or("message is not array")?;
            // 收集poke id并移除poke元素
            let mut i = 0;
            while i < msg_arr.len() {
                let tp = read_json_str(&msg_arr[i], "type");
                if tp == "poke" {
                    let mut ret_id = read_json_str(&msg_arr[i]["data"], "id");
                    if ret_id == "" {
                        ret_id = read_json_str(&msg_arr[i]["data"], "qq");
                    }
                    if !ret_id.is_empty() {
                        ret_ids.push(ret_id);
                    }
                    msg_arr.remove(i);
                    // 不递增i，因为移除了当前元素
                } else {
                    i += 1;
                }
            }
        }
        Ok(ret_ids)
    }

    async fn deal_music_segment(&self,json: &mut serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        let platform = self.get_platform().await?;
        if !(platform == "lagrange" || platform == "llonebot" || platform == "cqhttp" || platform == "napcat") {
            return Ok(());
        }

        let mut music_card_sign = String::new();

        

        let params = json.get_mut("params").ok_or("params is none")?;
        if let Some(message) = params.get_mut("message") {
            for msgobj in message.as_array_mut().ok_or("message is not array")? {
                if msgobj["type"].as_str().ok_or("type is not str")? != "music" {
                    continue;
                }
                if msgobj["data"]["type"].as_str().ok_or("data type is not str")? != "custom" {
                    continue;
                }

                if music_card_sign == "" {
                    let plus_dir = cq_get_app_directory1()?;
                    let config = plus_dir + "adapter_onebot11_config.json";
                    let config_str;
                    if let Ok(config_str_t) = tokio::fs::read_to_string(config).await {
                        config_str = config_str_t;
                    }else {
                        return Ok(());
                    }
                    let config_json:serde_json::Value = serde_json::from_str(&config_str)?;
                    music_card_sign = read_json_str(&config_json, "music_card_sign");
                    if music_card_sign == "" {
                        return  Ok(());
                    }
                    if !music_card_sign.ends_with("/") {
                        music_card_sign.push_str("/");
                    }  
                }

                if music_card_sign == "" {
                    return Ok(());
                }


                let data = &msgobj["data"];
    
                let url = read_json_str(data, "url");
                let mut audio = read_json_str(data, "audio");
                let title = read_json_str(data, "title");
                let mut content = read_json_str(data, "content");  
                let image = read_json_str(data, "image");
    
                // 兼容sm
                if content.is_empty() {
                    content = read_json_str(data, "singer");
                }
    
                // 兼容gocq末期
                if audio.is_empty() {
                    audio = read_json_str(data, "voice");
                }
                
    
                let url_t:String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
                let audio_t:String = url::form_urlencoded::byte_serialize(audio.as_bytes()).collect();
                let title_t:String = url::form_urlencoded::byte_serialize(title.as_bytes()).collect();
                let content_t:String = url::form_urlencoded::byte_serialize(content.as_bytes()).collect();
                let image_t:String = url::form_urlencoded::byte_serialize(image.as_bytes()).collect();
    
                cq_add_log(&format!("使用`{music_card_sign}`进行音乐卡片签名")).unwrap();
                let get_url = format!("{music_card_sign}?url={audio_t}&song={title_t}&singer={content_t}&cover={image_t}&jump={url_t}&format=bilibili");
                let api_get = reqwest::get(get_url).await?;
                let ret_text = api_get.text().await?;
                let j:serde_json::Value = serde_json::from_str(&ret_text)?;
                let code = read_json_str(&j, "code");
                if code != "1" { 
                    cq_add_log_w(&format!("使用`{}`进行音乐卡片签名失败:{}",music_card_sign,ret_text)).unwrap();
                    continue;
                }else {
                    cq_add_log(&format!("使用`{music_card_sign}`进行音乐卡片签名成功")).unwrap();
                }
                let music_json = j["message"].as_str().ok_or("music message is not str")?;
                *msgobj.get_mut("type").ok_or("get type err")? = serde_json::json!("json");
                *msgobj.get_mut("data").ok_or("get data err")? = serde_json::json!({
                    "data": music_json
                });
            }
        }
        Ok(())
    }

    async fn all_to_silk_async(input:&Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let rt_ptr = RT_PTR.clone();
        let input = input.clone();
        let silk_bin = rt_ptr.spawn_blocking(move || {
            // Ensure the error type is Send + Sync + 'static
            all_to_silk(&input).map_err(|e| {
                // Convert error to Box<dyn Error + Send + Sync>
                // Convert error to a string and wrap it in a std::io::Error
                let s = format!("{:?}", e);
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, s)) as Box<dyn std::error::Error + Send + Sync>
            })
        }).await??;
        Ok(silk_bin)
    }

    async fn deal_record_segment(&self,json: &mut serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        let platform = self.get_platform().await?;
        if !(platform == "lagrange" || platform == "llonebot" || platform == "cqhttp" || platform == "napcat") {
            return Ok(());
        }

        let mut is_convert = false; 

        
        let params = json.get_mut("params").ok_or("params is none")?;
        if let Some(message) = params.get_mut("message") {
            for msgobj in message.as_array_mut().ok_or("message is not array")? {
                if msgobj["type"].as_str().ok_or("type is not str")? != "record" {
                    continue;
                }

                if is_convert == false {
                    let plus_dir = cq_get_app_directory1()?;
                    let config = plus_dir + "adapter_onebot11_config.json";
                    let config_str;
                    if let Ok(config_str_t) = tokio::fs::read_to_string(&config).await {
                        config_str = config_str_t;
                    }else {
                        return Ok(());
                    }
                    let config_json:serde_json::Value = serde_json::from_str(&config_str)?;
                    let auto_convert_record = &config_json["auto_convert_record"];
                    if auto_convert_record.as_bool().unwrap_or_default() == false {
                        return Ok(());
                    }
                    is_convert = true;
                }

                let data = &msgobj["data"];
    
                let uri = read_json_str(data, "file");
                let file_bin;
                if uri.starts_with("base64://") {
                    let b64_str = uri.get(9..).unwrap();
                    file_bin = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                &base64::alphabet::STANDARD,
                base64::engine::general_purpose::PAD), b64_str)?;
                }else if uri.starts_with("file://") {
                    let file_path;
                    if cfg!(target_os = "windows") {
                        file_path = uri.get(8..).ok_or("can't get file_path")?;
                    } else {
                        file_path = uri.get(7..).ok_or("can't get file_path")?;
                    }
                    let path = std::path::Path::new(&file_path);
                    file_bin = tokio::fs::read(path).await?;
                } else {
                    continue;
                }
                cq_add_log_w("自动将语音文件转换到silk").unwrap();
                let silk_bin = Self::all_to_silk_async(&file_bin).await?;
                // 用 silk_bin 替换原来的 file
                let b64_str = BASE64_CUSTOM_ENGINE.encode(silk_bin);
                msgobj["data"]["file"] = serde_json::json!(format!("base64://{}", b64_str));
            }
        }
        Ok(())
    }

    async fn deal_set_msg_emoji_like(&self,json: &mut serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let platform = self.get_platform().await?;
        if platform == "lagrange"{
            let params = json.get_mut("params").ok_or("params is none")?;
            let group_id = read_json_str(params, "group_id");
            let code = read_json_str(params, "emoji_id");
            params["code"] = serde_json::json!(code);
            params["group_id"] = serde_json::json!(group_id.parse::<u64>()?);
            params["is_add"] = serde_json::Value::Bool(true);
            params.as_object_mut().ok_or("params is not object")?.remove("emoji_id");
            json["action"] = serde_json::json!("set_group_reaction");
        } else if platform == "cqhttp" {
            let params = json.get_mut("params").ok_or("params is none")?;
            let code = read_json_str(params, "emoji_id");
            params["icon_id"] = serde_json::json!(code);
            params["is_add"] = serde_json::Value::Bool(true);
            if code.len() >= 4 {
                params["icon_type"] = serde_json::json!(2);
            } else {
                params["icon_type"] = serde_json::json!(1);
            }
            params.as_object_mut().ok_or("params is not object")?.remove("emoji_id");
            json["action"] = serde_json::json!("set_group_reaction");
        }
        else if platform == "napcat" || platform == "llonebot"  {
            // do nothing
        }
        Ok(())
    }

    pub async fn get_platform(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let read_platform = self.real_platform.read().unwrap().to_owned();
        if read_platform == None {
            let mut send_json;
            send_json = serde_json::json!({
                "action":"get_version_info",
                "params":{}
            });
            let self_id = self.self_id.read().unwrap().to_owned();
            let ret = self.call_api("onebot11", &self_id, "", &mut send_json).await?;
            let platform = ret["data"]["app_name"].as_str().unwrap_or("").to_ascii_lowercase();
            if platform.contains("lagrange") {
                *self.real_platform.write().unwrap() = Some("lagrange".to_string());
            } else if platform.contains("llonebot") {
                *self.real_platform.write().unwrap() = Some("llonebot".to_string());
            } else if platform.contains("cqhttp") {
                *self.real_platform.write().unwrap() = Some("cqhttp".to_string());
            } else if platform.contains("napcat") {
                *self.real_platform.write().unwrap() = Some("napcat".to_string());
            } else {
                *self.real_platform.write().unwrap() = Some("".to_string());
            }
        }
        let read_platform = self.real_platform.read().unwrap().to_owned();
        if let Some(platform) = read_platform {
            return Ok(platform);
        }
        return Ok("".to_string());
    }
    pub async fn get_avatar(&self,user_id:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let platform = self.get_platform().await?;
        if platform == "lagrange" || platform == "llonebot" || platform == "cqhttp" || platform == "napcat" {
            return Ok(format!("https://thirdqq.qlogo.cn/g?b=qq&nk={user_id}&s=640"));
        }
        return Err("can't get avatar".into());
    }
}




fn change_id_to_str(root:&mut serde_json::Value){
    lazy_static! {
        static ref ID_SET:HashSet<String> = {
            let mut st = HashSet::new();
            st.insert("target_id".to_owned());
            st.insert("user_id".to_owned());
            st.insert("group_id".to_owned());
            st.insert("self_id".to_owned());
            st.insert("message_id".to_owned());
            st.insert("operator_id".to_owned());
            st
        };


    }
    if root.is_object() {
        for (k,v) in root.as_object_mut().unwrap() {
            if ID_SET.contains(k) {
                if v.is_i64() {
                    (*v) = serde_json::to_value(v.as_i64().unwrap().to_string()).unwrap();
                }
            }else if v.is_array() || v.is_object() {
                change_id_to_str(v);
            }
        }
    }else if root.is_array() {
        for v in root.as_array_mut().unwrap() {
            change_id_to_str(v);
        }
    }
}


fn deal_cq_arr(root:&mut serde_json::Value){
    let message:&serde_json::Value = &root["message"];
    if message.is_string() {
        if let Ok(msg) = crate::mytool::str_msg_to_arr(&message) {
            root["message"] = msg;
        }   
    }
    let message_arr = root.index("message");
    if !message_arr.is_array() {
        return;
    }
    let message_arr = root.index_mut("message");
    if message_arr.is_array() {
        for it in message_arr.as_array_mut().unwrap() {
            let tp = &it["type"];
            if tp == "at"{
                let qq = &it["data"]["qq"];
                it["data"] = serde_json::json!({
                    "qq":qq
                });
            }else if tp == "image" {
                let url = &it["data"]["url"];
                if !url.is_string() {
                    it["data"]["url"] = it["data"]["http_file"].clone();
                }
            }
        }
    }
}


#[async_trait]
impl BotConnectTrait for OneBot11Connect {

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
        // println!("正在连接ws：{}",self.url);
        let url = url::Url::parse(&self.url)?;
        use tungstenite::client::IntoClientRequest;
        let mut request = url.as_str().into_client_request()?;
        let mp = crate::httpevent::get_params_from_uri(&hyper::Uri::from_str(&self.url)?);
        if let Some(access_token) = mp.get("access_token") {
            request.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Bearer {}",access_token)).unwrap());
        }
        let ws_rst;
        if self.url.starts_with("wss://") {
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
        let url_str_t = self.url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 这里使用弱引用，防止可能的循环依赖
        let self_id_ptr = Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.self_id);
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
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
                        let mut json_dat;
                        if let Some(val) =  get_json_dat(msg) {
                            json_dat = val;
                        }else{
                            continue;
                        }

                        // 设置self_id
                        let self_id = read_json_str(&json_dat, "self_id");
                        if self_id != "" {
                            if let Some(val) = self_id_ptr.upgrade() {
                                *val.write().unwrap() = self_id;
                            }
                            else{
                                break;
                            }
                        }
                        // 获得echo
                        let echo = read_json_str(&json_dat, "echo");
                        let post_type = read_json_str(&json_dat, "post_type");
                        let meta_event_type = read_json_str(&json_dat, "meta_event_type");
                        if meta_event_type != "heartbeat" && echo != "CBC949B6-8C9F-8060-A149-A045ED9AD405" {
                            crate::cqapi::cq_add_log(format!("OB11收到数据:{}", json_dat.to_string()).as_str()).unwrap();
                        }else{
                            continue;
                        }
                        // 添加平台标记
                        let json_obj = json_dat.as_object_mut().unwrap();
                        json_obj.insert("platform".to_string(), serde_json::to_value("onebot11").unwrap());

                        // 处理message,规范化数据
                        if post_type != "" {
                            deal_cq_arr(&mut json_dat);
                        }
                        
                        // 将ID转换为字符串
                        change_id_to_str(&mut json_dat);
                        tokio::spawn(async move {
                            if post_type == "" { // 是api回复
                                let tx;
                                {
                                    let echo_lk = G_ECHO_MAP.read().await;
                                    let ttt =  echo_lk.get(&echo);
                                    if let Some(ttt) = ttt {
                                        tx = ttt.clone();
                                    }else{
                                        return ();
                                    }   
                                }
                                let _foo = tx.send(json_dat).await;
                            }else { // 是事件
                                tokio::task::spawn_blocking(move ||{
                                    if let Err(e) = crate::cqevent::do_1207_event(&json_dat.to_string()) {
                                        crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                                    }
                                });
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
            cq_add_log_w(&format!("ws连接已经断开(read_halt):{url_str_t}")).unwrap();
        });
        let url_str_t = self.url.clone();
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        tokio::spawn(async move {
            let url_str2 = url_str_t.clone();
            let is_stop2 = is_stop.clone();
            // 构造特殊心跳,防止长时间连接导致防火墙不处理数据
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    if let Some(val) = is_stop.upgrade() {
                        if val.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                    }else {
                        break; 
                    }
                    let rst = tx_ay_t.send(serde_json::json!({
                        "action":"get_version_info",
                        "params":{},
                        "echo":"CBC949B6-8C9F-8060-A149-A045ED9AD405"
                    })).await;
                    if rst.is_err() {
                        break;
                    }
                }
                // 移除conn
                if let Some(val) = is_stop.upgrade() {
                    val.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                cq_add_log_w(&format!("ws心跳已断开:{url_str2}")).unwrap();
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
            cq_add_log_w(&format!("ws连接已经断开(write_half):{url_str_t}")).unwrap();
        });
        Ok(())
    }

    async fn call_api(&self,_platform:&str,_self_id:&str,_passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let echo = uuid::Uuid::new_v4().to_string();
        let json_obj = json.as_object_mut().ok_or("json is not object")?;
        json_obj.insert("echo".to_string(), serde_json::to_value(&echo)?);
        if let Some(params) = json_obj.get_mut("params") {
            if let Some(params_obj) = params.as_object_mut() {
                if let Some(group_id) = params_obj.get_mut("group_id") {
                    if let Some(group_id_str) = group_id.as_str() {
                        let val = serde_json::to_value(group_id_str.parse::<u64>()?)?;
                        params_obj["group_id"] = val;
                    }
                }
                if let Some(user_id) = params_obj.get_mut("user_id") {
                    if let Some(user_id_str) = user_id.as_str() {
                        let val = serde_json::to_value(user_id_str.parse::<u64>()?)?;
                        params_obj["user_id"] = val;
                    }
                }
                if let Some(message_id) = params_obj.get_mut("message_id") {
                    if let Some(message_id_str) = message_id.as_str() {
                        let val = serde_json::to_value(message_id_str.parse::<i64>()?)?;
                        params_obj["message_id"] = val;
                    }
                }
            }
        }


        let mut is_add_avatar = false;
        let action: String = read_json_str(json, "action");
        if action == "get_stranger_info" {
            is_add_avatar = true;
        }


        if action == "send_msg" || action == "send_group_msg" || action == "send_private_msg" {
            {
                let message = json["params"].get_mut("message").ok_or("not found message segment")?;
                if message.is_string() {
                    let message_arr = str_msg_to_arr(message).map_err(|x|{
                        format!("str_msg_to_arr err:{:?}",x)
                    })?;
                    *message = message_arr;
                }
            }
            self.deal_music_segment(json).await?;
            self.deal_record_segment(json).await?;
            let to_poke = self.get_poke_segment(json).await?;
            if !to_poke.is_empty() {
                let group_id = read_json_str(&json["params"], "group_id");
                for poke_id in to_poke.iter() {
                    if group_id != "" {
                        let group_id = &json["params"]["group_id"];
                        let mut to_send = serde_json::json!({
                            "action":"group_poke",
                            "params":{
                                "group_id": group_id,
                                "user_id": poke_id.parse::<u64>()?
                            }
                        });
                        self.call_api("", "", "", &mut to_send).await?;
                    }else {
                        let mut to_send = serde_json::json!({
                            "action":"friend_poke",
                            "params":{
                                "user_id": poke_id.parse::<u64>()?
                            }
                        });
                        self.call_api("", "", "", &mut to_send).await?;
                    }
                }
                
                let ret_json = serde_json::json!({
                    "status":"ok",
                    "retcode":0,
                    "data": {
                        "message_id":crate::redlang::get_random()?.to_string()
                    },
                    "echo":echo
                });
                // 如果 message里面已经没有其它元素了，就直接返回，否则就继续发送
                let message = json["params"].get_mut("message").ok_or("not found message segment")?;
                if message.is_array() {
                    let arr = message.as_array().unwrap();
                    if arr.len() == 0 {
                        return Ok(ret_json);
                    }
                }
            }
        } else if action == "set_msg_emoji_like" {
            self.deal_set_msg_emoji_like(json).await?;
        } 

        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(1);
        G_ECHO_MAP.write().await.insert(echo.clone(), tx_ay);
        let _guard = scopeguard::guard(echo, |echo| {
            RT_PTR.spawn(async move {
                G_ECHO_MAP.write().await.remove(&echo);
            });
        });


        crate::cqapi::cq_add_log(format!("发送数据:{}", json.to_string()).as_str()).unwrap();
        
        self.tx.clone().ok_or("tx is none")?.send((*json).clone()).await?;

        tokio::select! {
            std::option::Option::Some(mut val) = rx_ay.recv() => {
                if is_add_avatar {
                    // 要补充头像
                    let user_id = read_json_str(&val["data"],"user_id");
                    val["data"]["avatar"] = serde_json::json!(self.get_avatar(&user_id).await.unwrap_or("".to_owned()));
                }
                return Ok(val);
            },
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                cq_add_log_w(&format!("接收api返回超时")).unwrap();
                return Ok(serde_json::json!(""));
            }
        }
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        let lk = self.self_id.read().unwrap();
        let self_id = (*lk).clone();
        let platform = "onebot11".to_owned();
        return vec![(platform,self_id)];
    }
}

