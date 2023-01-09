use std::{collections::HashMap, sync::Arc};

use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::connect_async;

use crate::{RT_PTR, cqapi::cq_add_log_w};
#[derive(Debug)]
pub struct BotConnect {
    pub id:String,
    pub url:String,
    pub access_token:String,
    pub is_connect:bool,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>
}

impl BotConnect {
    pub fn new() -> BotConnect{
        return BotConnect {
            id: "".to_string(),
            url: "".to_string(),
            access_token: "".to_string(),
            is_connect: false,
            tx: None,
        };
    }
}

lazy_static! {
    static ref G_ECHO_MAP:tokio::sync::RwLock<HashMap<String,tokio::sync::mpsc::Sender<serde_json::Value>>> = tokio::sync::RwLock::new(HashMap::new());
    pub static ref G_BOT_ARR:tokio::sync::RwLock<Vec<Arc<tokio::sync::RwLock<BotConnect>>>> = tokio::sync::RwLock::new(Vec::new());
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

fn get_int_from_json(json:&serde_json::Value,key:&str)-> Option<i64> {
    if let Some(val) = json.get(key) {
        if let Some(val) = val.as_i64() {
            return Some(val);
        }else {
            return None;
        }
    }else {
        return None;
    }
}
async fn add_bot_connect(url_str:&str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    //let url = url::Url::parse("ws://220.167.103.33:10191?access_token=77156").unwrap();
    let url = url::Url::parse(url_str)?;
    let (ws_stream, _) = connect_async(url).await?;
    let (mut write_half,mut read_halt) = ws_stream.split();
    let bot = Arc::new(tokio::sync::RwLock::new(BotConnect::new()));
    let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
    G_BOT_ARR.write().await.push(bot.clone());
    bot.write().await.url = url_str.to_owned();
    bot.write().await.is_connect = true;
    bot.write().await.tx = Some(tx_ay);
    cq_add_log_w(&format!("成功连接到onebot:{}",url_str)).unwrap();
    tokio::spawn(async move {
        while let Some(msg) = read_halt.next().await {
            let json_dat_opt:Option<serde_json::Value>;
            if let Ok(msg) = msg{
                json_dat_opt = get_json_from_msg(msg);
            }else {
                continue;
            }
            //得到json_dat
            let json_dat:serde_json::Value;
            if let Some(json_dat_t) = json_dat_opt {
                json_dat = json_dat_t;
            }else{
                continue;
            }
            crate::cqapi::cq_add_log(format!("收到数据:{}", json_dat.to_string()).as_str()).unwrap();
            let self_id = get_int_from_json(&json_dat, "self_id");
            if self_id != None {
                bot.write().await.id = self_id.unwrap().to_string();
            }
            let echo = get_str_from_json(&json_dat, "echo").to_owned();
            tokio::spawn(async move {
                if echo != "" {
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
                }else {
                    let _foo = tokio::task::spawn_blocking(move ||{
                        if let Err(e) = crate::cqevent::do_1207_event(&json_dat.to_string()) {
                            crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
                        }
                    });
                }
            });
        }
        bot.write().await.is_connect = false;
        bot.write().await.tx = None;
    });
    while let Some(msg) = rx_ay.recv().await {
        let _foo = write_half.send(hyper_tungstenite::tungstenite::Message::Text(msg.to_string())).await;
    }
    cq_add_log_w(&format!("ws连接已经断开:{url_str}")).unwrap();
    let mut lk = G_BOT_ARR.write().await;
    let mut i = 0usize;
    for it in &*lk {
        i += 1;
        if it.read().await.is_connect == false {
            break;
        }
    }
    lk.remove(i - 1);
    Ok(())
}

pub async fn call_api(self_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let echo = uuid::Uuid::new_v4().to_string();
    json.as_object_mut().unwrap().insert("echo".to_string(), serde_json::to_value(&echo)?);
    let mut bot_select= None;
    let bot_arr = G_BOT_ARR.read().await;
    for bot in &*bot_arr {
        if bot.read().await.id == self_id {
            bot_select = Some(bot.clone());
            break;
        }
    }
    if bot_select.is_none() {
        bot_select = Some(G_BOT_ARR.read().await.get(0).ok_or("没有获取到bot")?.clone());
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
    crate::cqapi::cq_add_log(format!("发送数据:{}", json.to_string()).as_str()).unwrap();
    tx.send((*json).clone()).await?;
    tokio::select! {
        std::option::Option::Some(val) = rx_ay.recv() => {
            return Ok(val);
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
            cq_add_log_w(&format!("接收api返回超时")).unwrap();
            return Ok(serde_json::to_value({})?);
        }
    }
}
pub fn do_conn_event() -> Result<i32, Box<dyn std::error::Error>> {
    let config = crate::read_config()?;
    let urls = config.get("ws_urls").ok_or("无法获取ws_urls")?.as_array().ok_or("无法获取web_host")?.to_owned();
    for url in urls {
        RT_PTR.clone().spawn(async move {
            let url_str = url.as_str().ok_or("ws_url不是字符数组").unwrap();
            loop {
                if let Err(_err) = add_bot_connect(url_str).await{
                    cq_add_log_w(&format!("连接到onebot失败:{}",url)).unwrap();
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }
    
    Ok(0)
}