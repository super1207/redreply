use std::{collections::HashMap, sync::Arc};

use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::connect_async;

use crate::{RT_PTR, cqapi::cq_add_log_w, mytool::read_json_str};
#[derive(Debug)]
pub struct BotConnect {
    pub ws_uuid:String,
    pub id:String,
    pub url:String,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>
}

impl BotConnect {
    pub fn new() -> BotConnect{
        return BotConnect {
            ws_uuid:"".to_string(),
            id: "".to_string(),
            url: "".to_string(),
            tx: None,
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

async fn add_bot_connect(url_str:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = url::Url::parse(url_str)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write_half,mut read_halt) = ws_stream.split();
    let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
    let ws_uuid = uuid::Uuid::new_v4().to_string();
    {
        // 将bot放入全局bot表
        if G_BOT_MAP.read().await.contains_key(url_str) {
            return Ok(());
        }
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
            // 判断是否断开连接
            if let Some(bot) = G_BOT_MAP.read().await.get(&url_str_t) {
                if bot.read().await.ws_uuid != ws_uuid {
                    break;
                }
            }else{
                break;
            }
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
                    let _foo = tokio::task::spawn_blocking(move ||{
                        if let Err(e) = crate::cqevent::do_1207_event(&json_dat.to_string()) {
                            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                        }
                    });
                }
            });
        }
        let mut lk = G_BOT_MAP.write().await;
        let can_remove;
        if let Some(bot) = lk.get(&url_str_t) {
            if bot.read().await.ws_uuid == ws_uuid {
                can_remove = true;
            }else{
                can_remove = false;
            }
        }else{
            can_remove = false;
        }
        if can_remove {
            cq_add_log_w(&format!("ws连接已经断开(read_halt):{url_str_t}")).unwrap();
            lk.remove(&url_str_t);
        }
    });
    let url_str_t = url_str.to_string();
    tokio::spawn(async move {
        while let Some(msg) = rx_ay.recv().await {
            let _foo = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string())).await;
        }
        let mut lk = G_BOT_MAP.write().await;
        let can_remove;
        if let Some(bot) = lk.get(&url_str_t) {
            if bot.read().await.ws_uuid == ws_uuid_t {
                can_remove = true;
            }else{
                can_remove = false;
            }
        }else{
            can_remove = false;
        }
        if can_remove {
            cq_add_log_w(&format!("ws连接已经断开(read_halt):{url_str_t}")).unwrap();
            lk.remove(&url_str_t);
        }
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
    let _foo = std::thread::spawn(move ||{
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