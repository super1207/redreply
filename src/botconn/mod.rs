use std::{collections::HashMap, sync::{Arc, atomic::AtomicBool}};

use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::connect_async;

use crate::{RT_PTR, cqapi::{cq_add_log_w, cq_add_log}, mytool::read_json_str};
#[derive(Debug)]
pub struct BotConnect {
    pub ws_uuid:String,
    pub id:String,
    pub url:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_alive:AtomicBool
}

impl BotConnect {
    pub fn new() -> BotConnect{
        return BotConnect {
            ws_uuid:"".to_string(),
            id: "".to_string(),
            url: "".to_string(),
            tx: None,
            is_alive:AtomicBool::new(false)
        };
    }
}

lazy_static! {
    static ref G_ECHO_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<serde_json::Value>>> = tokio::sync::RwLock::new(HashMap::new());
    pub static ref G_BOT_MAP:tokio::sync::RwLock<HashMap<String,Arc<tokio::sync::RwLock<BotConnect>>>> = tokio::sync::RwLock::new(HashMap::new());
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

fn get_str_from_json<'a>(json:&'a  serde_json::Value,key:&'a str)-> &'a str {
    if let Some(val) = json.get(key) {
        if let Some(val) = val.as_str() {
            return val;
        }else {
            return "";
        }
    }else {
        return "";
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
async fn get_bot_uuid_by_url(url:&str) -> String {
    let lk = G_BOT_MAP.read().await;
    let conn = lk.get(url);
    if let Some(c) = conn {
        return c.read().await.ws_uuid.to_owned();
    }else{
        return String::new();
    }
}
async fn add_bot_connect(url_str:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("正在连接ws：{}",url_str);
    let url = url::Url::parse(url_str)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write_half,mut read_halt) = ws_stream.split();
    let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
    let ws_uuid = uuid::Uuid::new_v4().to_string();
    let tx_ay_t = tx_ay.clone();
    {
        // 先将原本存在的移除
        G_BOT_MAP.write().await.remove(url_str);
        // 将bot放入全局bot表
        let bot = Arc::new(tokio::sync::RwLock::new(BotConnect::new()));
        bot.write().await.url = url_str.to_owned();
        bot.write().await.tx = Some(tx_ay);
        bot.write().await.ws_uuid = ws_uuid.clone();
        G_BOT_MAP.write().await.insert(url_str.to_string(), bot.clone());
        cq_add_log_w(&format!("成功连接到onebot:{}",url_str)).unwrap();
    }
    let url_str_t = url_str.to_string();
    let ws_uuid_t = ws_uuid.to_string();
    tokio::spawn(async move {
        while let Some(msg) = read_halt.next().await {  
            // 判断是否断开连接,bot列表中不存在了，自然要退出循环
            if G_BOT_MAP.read().await.get(&url_str_t).is_none() {
                break;
            }
            // 获得json数据
            let json_dat;
            if let Some(val) =  get_json_dat(msg) {
                json_dat = val;
            }else{
                continue;
            }
            if let Some(bot) = G_BOT_MAP.write().await.get(&url_str_t) {
                bot.write().await.is_alive.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            // 设置self_id
            let self_id = read_json_str(&json_dat, "self_id");
            if self_id != "" {
                if let Some(bot) = G_BOT_MAP.read().await.get(&url_str_t) {
                    bot.write().await.id = self_id;
                }
            }
            // 获得echo
            let echo = get_str_from_json(&json_dat, "echo").to_owned();
            tokio::spawn(async move {
                if echo != "" { // 是api回复
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
        }
        // 移除conn
        let exist_uuid = get_bot_uuid_by_url(&url_str_t).await;
        if exist_uuid == ws_uuid {
            G_BOT_MAP.write().await.remove(&url_str_t);
        }
        cq_add_log_w(&format!("ws连接已经断开(read_halt):{url_str_t}")).unwrap();
    });
    let url_str_t = url_str.to_string();
    tokio::spawn(async move {
        let uuid2 = ws_uuid_t.clone();
        let url_str2 = url_str_t.clone();

        // 构造特殊心跳,防止长时间连接导致防火墙不处理数据
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if let Some(bot) = G_BOT_MAP.read().await.get(&url_str2) {
                    let is_alive = bot.read().await.is_alive.load(std::sync::atomic::Ordering::Relaxed);
                    if is_alive == false {
                        break;
                    }else {
                        cq_add_log(&format!("ws alive:{url_str2}")).unwrap();
                    }
                } else {
                    break;
                }
                if let Some(bot) = G_BOT_MAP.write().await.get(&url_str2) {
                    bot.write().await.is_alive.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                let rst = tx_ay_t.send(serde_json::json!({
                    "action":"get_version_info",
                    "params":{},
                    "echo":uuid2
                })).await;
                if rst.is_err() {
                    break;
                }
            }
            // 移除conn
            let exist_uuid = get_bot_uuid_by_url(&url_str2).await;
            if exist_uuid == uuid2 {
                G_BOT_MAP.write().await.remove(&url_str2);
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
        let exist_uuid = get_bot_uuid_by_url(&url_str_t).await;
        if exist_uuid == ws_uuid_t {
            G_BOT_MAP.write().await.remove(&url_str_t);
        }
        cq_add_log_w(&format!("ws连接已经断开(write_half):{url_str_t}")).unwrap();
    });
    Ok(())
}

pub async fn call_api(self_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let echo = uuid::Uuid::new_v4().to_string();
    json.as_object_mut().unwrap().insert("echo".to_string(), serde_json::to_value(&echo)?);
    let mut bot_select= None;
    let mut bot_url = "".to_string();
    for bot in &*G_BOT_MAP.read().await {
        if bot.1.read().await.id == self_id {
            bot_select = Some(bot.1.clone());
            break;
        }
        bot_url = bot.0.clone();
    }
    if bot_select.is_none() {
        bot_select = Some(G_BOT_MAP.read().await.get(&bot_url).ok_or("没有获取到bot")?.clone());
    }
    let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(1);
    G_ECHO_MAP.write().await.insert(echo.clone(), tx_ay);
    let _guard = scopeguard::guard(echo, |echo| {
        RT_PTR.spawn(async move {
            G_ECHO_MAP.write().await.remove(&echo);
        });
    });
    let tx;
    {
        let bot = bot_select.unwrap();
        let ttt = bot.read().await;
        let tttt = &ttt.tx;
        if let Some(tx_t) = tttt {
            tx = tx_t.clone();
        }else {
            cq_add_log_w(&format!("无法发送数据")).unwrap();
            return Ok(serde_json::to_value({})?);
        }
    }
    {
        let js_str = json.to_string();
        let out_str = js_str.get(0..2000);
        if out_str.is_some() {
            crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
        }else {
            crate::cqapi::cq_add_log(format!("发送数据:{}", json.to_string()).as_str()).unwrap();
        }
    }
    tx.send((*json).clone()).await?;
    tokio::select! {
        std::option::Option::Some(val) = rx_ay.recv() => {
            return Ok(val);
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
            cq_add_log_w(&format!("接收api返回超时")).unwrap();
            return Ok(serde_json::json!(""));
        }
    }
}


pub fn do_conn_event() -> Result<i32, Box<dyn std::error::Error>> {
    std::thread::spawn(move ||{
        loop {
            // 得到配置文件中的url
            let config = crate::read_config().unwrap();
            let urls_val = config.get("ws_urls").ok_or("无法获取ws_urls").unwrap().as_array().ok_or("无法获取web_host").unwrap().to_owned();
            let mut config_urls = vec![];
            for url in &urls_val {
                let url_str = url.as_str().ok_or("ws_url不是字符数组").unwrap().to_string();
                config_urls.push(url_str);
            }
            
            RT_PTR.clone().block_on(async move {
                // 删除所有不在列表中的url
                {
                    let mut earse_vec = vec![];
                    let mut bot_map = G_BOT_MAP.write().await;
                    for (url,_bot) in &*bot_map {
                        if !config_urls.contains(url) {
                            earse_vec.push(url.clone());
                        }
                    }
                    for url in &earse_vec {
                        bot_map.remove(url);
                    }
                }
                // 连接未在bot_map中的url
                for url in &config_urls {
                    let is_exist;
                    if G_BOT_MAP.read().await.contains_key(url) {
                        is_exist = true;
                    }else{
                        is_exist = false;
                    }
                    if !is_exist {
                        let url_t = url.clone();
                        RT_PTR.clone().spawn(async move {
                            if let Err(_err) = add_bot_connect(&url_t).await{
                                cq_add_log_w(&format!("连接到onebot失败:{}",url_t)).unwrap();
                            }
                        });
                    }
                }
            });
            
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    Ok(0)
}