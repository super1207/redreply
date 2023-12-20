use std::{sync::{atomic::AtomicBool, Arc, RwLock}, collections::HashSet, str::FromStr};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use hyper::header::HeaderValue;
use tokio_tungstenite::{tungstenite, connect_async};

use crate::{cqapi::cq_add_log_w, mytool::read_json_str};

use super::BotConnectTrait;

#[derive(Debug)]
pub struct OneBot115Connect {
    pub self_ids:Arc<std::sync::RwLock<HashSet<String>>>,
    pub url:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub platforms:Arc<std::sync::RwLock<HashSet<String>>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx :Option<tokio::sync::mpsc::Sender<bool>>,
}


async fn http_post(url:&str,json_data:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let client;
    let uri = reqwest::Url::from_str(url)?;
    client = reqwest::Client::builder().no_proxy().build()?;
    let req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
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
    crate::cqapi::cq_add_log(format!("收到数据:{}", json_dat.to_string()).as_str()).unwrap();
    return Some(json_dat);
}


impl OneBot115Connect {
    pub fn build(url:&str) -> Self {
        OneBot115Connect {
            self_ids:Arc::new(RwLock::new(HashSet::new())),
            url:url.to_owned(),
            tx:None,
            platforms:Arc::new(RwLock::new(HashSet::new())),
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None

        }
    }
}



#[async_trait]
impl BotConnectTrait for OneBot115Connect {

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

        println!("正在连接ovo：{}",self.url);

        {
            let mut to_send = serde_json::json!({
                "action":"get_bot_list"
            });
            let myself = self.call_api("", "", &mut to_send).await?;
            cq_add_log_w(&format!("收到数据：{}",myself.to_string())).unwrap();
            let bot_arr = myself.get("data").ok_or("data not found")?.as_array().ok_or("data not array")?;
            for bot in bot_arr {
                if let Some(self_id) = bot.get("self_id") {
                    self.self_ids.write().unwrap().insert(self_id.as_str().ok_or("self_id not string")?.to_string());
                }
                if let Some(platform) = bot.get("platform") {
                    self.platforms.write().unwrap().insert(platform.as_str().ok_or("platform not string")?.to_string());
                }
            }
        }
        let url_ws = self.url.replacen("ovo://", "ws://", 1);
        let url = url::Url::parse(&format!("{url_ws}/v1/events"))?;
        let mut request = tungstenite::client::IntoClientRequest::into_client_request(url).unwrap();
        let mp = crate::httpevent::get_params_from_uri(&hyper::Uri::from_str(&self.url)?);
        if let Some(access_token) = mp.get("access_token") {
            request.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Bearer {}",access_token)).unwrap());
        }
        let ws_rst = connect_async(request).await?;
        let (mut write_half,mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = self.url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 这里使用弱引用，防止可能的循环依赖
        let self_id_ptr = Arc::<std::sync::RwLock<HashSet<String>>>::downgrade(&self.self_ids);
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
                        let json_dat;
                        if let Some(val) =  get_json_dat(msg) {
                            json_dat = val;
                        }else{
                            continue;
                        }

                        // 设置self_id
                        let self_id = read_json_str(&json_dat, "self_id");
                        if self_id != "" {
                            if let Some(val) = self_id_ptr.upgrade() {
                                val.write().unwrap().insert(self_id);
                            }
                            else{
                                break;
                            }
                        }
                        // 获得echo
                        //let echo = read_json_str(&json_dat, "echo").to_owned();
                        let post_type = read_json_str(&json_dat, "post_type").to_owned();
                        tokio::spawn(async move {
                            if post_type == "" { // 是api回复
                                // 只可能是心跳，do nothing
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
            cq_add_log_w(&format!("ovo连接已经断开(read_halt):{url_str_t}")).unwrap();
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
                    })).await;
                    if rst.is_err() {
                        break;
                    }
                }
                // 移除conn
                if let Some(val) = is_stop.upgrade() {
                    val.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                cq_add_log_w(&format!("ovo心跳已断开:{url_str2}")).unwrap();
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
            cq_add_log_w(&format!("ovo连接已经断开(write_half):{url_str_t}")).unwrap();
        });
        Ok(())
    }

    fn get_platform(&self) -> Vec<String> {
        let ret_vec = self.platforms.read().unwrap().iter().map(|x| x.to_string()).collect();
        return ret_vec;
    }

    fn get_url(&self) -> String {
        return self.url.clone();
    }

    async fn call_api(&mut self,platform:&str,self_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

        let json_obj = json.as_object_mut().ok_or("json is not object")?;
        json_obj.insert("self_id".to_string(), serde_json::to_value(&self_id)?);
        json_obj.insert("platform".to_string(), serde_json::to_value(&platform)?);

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

        let mut http_url = self.url.to_owned();
        http_url = http_url.replacen("ovo://", "http://", 1);
        let ret = http_post(&format!("{http_url}/v1/api"),json).await?;
        return Ok(ret)
    }

    fn get_self_id(&self) -> Vec<String> {
        let lk = self.self_ids.read().unwrap();
        let self_ids = (*lk).clone();
        return self_ids.iter().map(|x| x.to_string()).collect();
    }
}