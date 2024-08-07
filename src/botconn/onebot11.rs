use std::{collections::{HashMap, HashSet}, ops::{Index, IndexMut}, str::FromStr, sync::{atomic::AtomicBool, Arc, RwLock}};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use hyper::header::HeaderValue;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite, connect_async};

use crate::{RT_PTR, cqapi::cq_add_log_w, mytool::read_json_str};

use super::BotConnectTrait;

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
    pub async fn get_avatar(&self,user_id:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
            if platform.contains("lagrange") || platform.contains("llonebot") || platform.contains("cqhttp") || platform.contains("napcat") {
                *self.real_platform.write().unwrap() = Some("qq".to_string());
            } else {
                *self.real_platform.write().unwrap() = Some("".to_string());
            }
        }
        let read_platform = self.real_platform.read().unwrap().to_owned();
        if read_platform == Some("qq".to_owned()) {
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
                let rst = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string())).await;
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

        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(1);
        G_ECHO_MAP.write().await.insert(echo.clone(), tx_ay);
        let _guard = scopeguard::guard(echo, |echo| {
            RT_PTR.spawn(async move {
                G_ECHO_MAP.write().await.remove(&echo);
            });
        });

        // 处理日志
        {
            let js_str = json.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", json.to_string()).as_str()).unwrap();
            }
        }
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