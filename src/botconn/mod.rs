mod onebot11;
mod onebot115;
mod satoriv1;
mod qqguild_private;
mod qqguild_public;

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;

use tokio::sync::RwLock;

use crate::{cqapi::cq_add_log_w, RT_PTR};

use self::{onebot11::OneBot11Connect, onebot115::OneBot115Connect, satoriv1::Satoriv1Connect, qqguild_private::QQGuildPrivateConnect, qqguild_public::QQGuildPublicConnect};

#[async_trait]
trait BotConnectTrait:Send + Sync {
    async fn call_api(&self,platform:&str,self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>;
    fn get_platform_and_self_id(&self) -> Vec<(String,String)>;
    fn get_url(&self) -> String;
    fn get_alive(&self) -> bool;
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn disconnect(&mut self);
}


lazy_static! {
    static ref G_BOT_MAP:RwLock<HashMap<String,Arc<RwLock<dyn BotConnectTrait>>>> = RwLock::new(HashMap::new());
}


pub async fn call_api(platform:&str,self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut bot_select = None;
    let platform_t = platform.to_owned();
    let self_id_t = self_id.to_owned();
    // 挑选出对应的bot
    for bot in &*G_BOT_MAP.read().await {
        let platform_and_self_id = bot.1.read().await.get_platform_and_self_id();
        for (platform,self_id) in platform_and_self_id {
            if platform == platform_t && self_id == self_id_t {
                bot_select = Some(bot.1.clone());
                break;
            }
        }
    }
    // 使用挑选出来的bot发送消息
    if bot_select.is_some() {
        return bot_select.unwrap().read().await.call_api(&platform_t, &self_id_t, passive_id,json).await;
    }
    cq_add_log_w(&format!("no such bot:platform:`{platform}`,self_id:`{self_id}`")).unwrap();
    return Ok(serde_json::json!(""));
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
                // 删除所有不在列表中的url和死去的bot
                {
                    let mut earse_urls = vec![];
                    let mut earse_bot = vec![];
                    // 找到这些bot
                    {
                        let bot_map = G_BOT_MAP.read().await;
                        for (url,bot) in &*bot_map {
                            if !config_urls.contains(url) || bot.read().await.get_alive() == false {
                                earse_bot.push(bot.clone());
                                earse_urls.push(url.clone());
                            }
                        }
                    }
                    // 移除这些bot
                    for index in 0..earse_urls.len() {
                        earse_bot[index].write().await.disconnect().await;
                        G_BOT_MAP.write().await.remove(&earse_urls[index]);
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
                            if url_t.starts_with("ws://") || url_t.starts_with("wss://") {
                                let mut bot = OneBot11Connect::build(&url_t);
                                if let Err(_err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到onebot失败:{}",url_t)).unwrap();
                                } else {
                                    G_BOT_MAP.write().await.insert(url_t,Arc::new(RwLock::new(bot)));
                                }
                            }else if url_t.starts_with("ovo://") {
                                let mut bot = OneBot115Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到ovo失败:{url_t},{err:?}")).unwrap();
                                } else {
                                    G_BOT_MAP.write().await.insert(url_t,Arc::new(RwLock::new(bot)));
                                }
                            }
                            else if url_t.starts_with("satori://") {
                                let mut bot = Satoriv1Connect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到satori失败:{url_t},{err:?}")).unwrap();
                                } else {
                                    G_BOT_MAP.write().await.insert(url_t,Arc::new(RwLock::new(bot)));
                                }
                            }
                            else if url_t.starts_with("qqguild_private://") {
                                let mut bot = QQGuildPrivateConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到qqguild_private失败:{url_t},{err:?}")).unwrap();
                                } else {
                                    G_BOT_MAP.write().await.insert(url_t,Arc::new(RwLock::new(bot)));
                                }
                            }
                            else if url_t.starts_with("qqguild_public://") {
                                let mut bot = QQGuildPublicConnect::build(&url_t);
                                if let Err(err) = bot.connect().await {
                                    cq_add_log_w(&format!("连接到qqguild_public失败:{url_t},{err:?}")).unwrap();
                                } else {
                                    G_BOT_MAP.write().await.insert(url_t,Arc::new(RwLock::new(bot)));
                                }
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