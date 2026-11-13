use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, RwLock},
    time::SystemTime,
};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use crate::{
    cqapi::cq_add_log_w,
    mytool::{cq_fix_image_file_in_arr, read_json_obj_or_null, read_json_or_default, read_json_str},
    qq_guild_all::{get_gateway, get_json_dat, token_refresh, SelfData},
};

use super::{
    qq_guild_all::{
        deal_attachments, deal_message_reference, delete_msg, get_group_list,
        get_group_member_info, get_login_info, get_msg_type, get_reply_id, get_stranger_info,
        qq_content_to_cqstr, send_private_msg, send_qqguild_msg, set_event_id, set_group_ban,
        str_msg_to_arr_safe, MsgTargetType,
    },
    qqguild_public::{conv_group_c2c_event, send_group_msg as send_qq_group_msg},
    BotConnectTrait,
};

#[derive(Debug)]
pub struct QQGuildPrivateConnect {
    pub url: String,
    pub appid: Arc<std::sync::RwLock<String>>,
    pub appsecret: String,
    pub token: String,
    pub access_token: Arc<std::sync::RwLock<String>>,
    pub tx: Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop: Arc<AtomicBool>,
    pub stop_tx: Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn: Arc<std::sync::RwLock<Option<u64>>>,
    pub id_event_map:
        Arc<std::sync::RwLock<std::collections::HashMap<String, (u64, serde_json::Value)>>>,
    pub bot_id: Arc<std::sync::RwLock<String>>,
    // 审核结果通道必须是连接级共享：call_api 注册等待者、WS 事件路径投递结果，
    // 两边要看到同一张 map，各自 new 会导致等待者永远收不到通知
    pub qqgroup_audit_map:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, tokio::sync::mpsc::Sender<serde_json::Value>>>>,
    // 审核事件先于 waiter 注册到达时的待领缓存
    pub qqgroup_audit_pending_cache:
        Arc<tokio::sync::RwLock<HashMap<String, (u64, serde_json::Value)>>>,
}

impl QQGuildPrivateConnect {
    pub fn build(url: &str) -> Self {
        QQGuildPrivateConnect {
            url: url.to_owned(),
            token: "".to_owned(),
            tx: None,
            is_stop: Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            appid: Arc::new(std::sync::RwLock::new("".to_owned())),
            appsecret: "".to_owned(),
            access_token: Arc::new(RwLock::new("".to_owned())),
            sn: Arc::new(RwLock::new(None)),
            id_event_map: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bot_id: Arc::new(RwLock::new("".to_owned())),
            qqgroup_audit_map: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            qqgroup_audit_pending_cache: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

async fn conv_event(
    self_t: &SelfData,
    root: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(&root, "t");
    let self_id = (*self_t.appid.upgrade().ok_or("No appid")?.read().unwrap_or_else(|e| e.into_inner())).to_owned();
    if tp == "READY" {
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let bot_id_t = read_json_str(&user, "id");
        (*self_t.bot_id.upgrade().ok_or("No bot_id")?.write().unwrap_or_else(|e| e.into_inner())) = bot_id_t;
    } else if conv_group_c2c_event(self_t, &root).await? {
    } else if tp == "DIRECT_MESSAGE_CREATE" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(d, "content");
        let user = read_json_obj_or_null(d, "author");
        let user_id = read_json_str(&user, "id");
        let avatar = read_json_str(&user, "avatar");
        let nickname = read_json_str(&user, "username");
        let cq_msg_t = qq_content_to_cqstr(&self_t.bot_id, &self_id, &content)?;
        let cq_msg = cq_msg_t + &deal_attachments(d)?;
        let cq_msg = deal_message_reference(d, &self_t.id_event_map)? + &cq_msg;
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqguild_private",
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":event_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":nickname,
                "remark":nickname,
                "avatar":avatar
            }
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    } else if tp == "GUILD_MEMBER_ADD" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(d, "guild_id");
        let operator_id = read_json_str(d, "op_user_id");
        let sub_type = if user_id == operator_id {
            "approve"
        } else {
            "invite"
        };
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"notice",
            "platform":"qqguild_private",
            "notice_type":"group_increase",
            "message_id":event_id,
            "sub_type":sub_type,
            "groups_id":guild_id,
            "operator_id":operator_id,
            "user_id":user_id,
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    } else if tp == "GUILD_MEMBER_REMOVE" {
        let event_id = set_event_id(self_t, &root, 60 * 5)?;
        let d = root.get("d").ok_or("No d")?;
        let user = read_json_obj_or_null(d, "user");
        let user_id = read_json_str(&user, "id");
        let guild_id = read_json_str(d, "guild_id");
        let operator_id = read_json_str(d, "op_user_id");
        let sub_type = if user_id == operator_id {
            "leave"
        } else {
            "kick"
        };
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"notice",
            "platform":"qqguild_private",
            "notice_type":"group_decrease",
            "message_id":event_id,
            "sub_type":sub_type,
            "groups_id":guild_id,
            "operator_id":operator_id,
            "user_id":user_id,
        });
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
        });
    }
    Ok(())
}

async fn send_group_msg(
    self_t: &SelfData,
    json: &serde_json::Value,
    passive_id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    // 获得参数
    let params = read_json_or_default(json, "params", &serde_json::Value::Null);

    // 判断消息类型
    let msg_target_type: MsgTargetType = get_msg_type(self_t, params, passive_id)?;

    // 获得消息(数组格式)
    let mut message = params
        .get("message")
        .ok_or("message is not exist")?
        .to_owned();
    if message.is_string() {
        message = str_msg_to_arr_safe(&message)?;
    }
    cq_fix_image_file_in_arr(&mut message);

    // 获得群id
    let group_id = read_json_str(params, "group_id");

    if msg_target_type == MsgTargetType::Guild {
        // 频道
        // 获得消息ID
        let reply_id = get_reply_id(self_t, passive_id)?;
        if !reply_id.raw_ids.is_empty() {
            let to_reply_id = &reply_id.raw_ids[0];
            return send_qqguild_msg(
                self_t,
                &group_id,
                to_reply_id,
                passive_id,
                &message,
                reply_id.is_event,
            )
            .await;
        }
    }
    Ok(serde_json::json!({
        "retcode":1404,
        "status":"failed",
        "message":"msg_target_type not support",
        "data":{}
    }))
}

#[async_trait]
impl BotConnectTrait for QQGuildPrivateConnect {
    async fn disconnect(&mut self) {
        self.is_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(stop_tx) = &self.stop_tx { let _ = stop_tx.send(true).await; }
    }

    fn get_alive(&self) -> bool {
        !self.is_stop.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(18..).ok_or("qqguild_private url格式错误")?;
        let config_json: serde_json::Value = serde_json::from_str(config_json_str)?;
        println!("{:?}", config_json);
        (*self.appid.write().unwrap_or_else(|e| e.into_inner())) = config_json
            .get("AppID")
            .ok_or("qqguild_private AppID格式错误:没有AppID")?
            .as_str()
            .ok_or("qqguild_private AppID格式错误:AppID不是字符串")?
            .to_owned();
        let appid = (*self.appid.read().unwrap_or_else(|e| e.into_inner())).to_owned();
        self.appsecret = config_json
            .get("AppSecret")
            .ok_or("qqguild_private AppSecret格式错误:没有AppSecret")?
            .as_str()
            .ok_or("qqguild_private AppSecret格式错误:AppSecret不是字符串")?
            .to_owned();
        self.token = config_json
            .get("Token")
            .ok_or("qqguild_private Token格式错误:没有Token")?
            .as_str()
            .ok_or("qqguild_private Token格式错误:Token不是字符串")?
            .to_owned();
        let access_token_struct = token_refresh(&appid, &self.appsecret).await?;
        (*self.access_token.write().unwrap_or_else(|e| e.into_inner())) = access_token_struct.access_token.to_owned();

        let ws_url = get_gateway(&access_token_struct.access_token, &appid).await?;
        println!("get_gateway:{}", ws_url);

        let request = tungstenite::client::IntoClientRequest::into_client_request(&ws_url)?;
        let ws_rst;
        let port = request.uri().port().map(Into::into).unwrap_or(443);
        let host = request.uri().host().ok_or("ws url 缺少 host")?;
        let addr = format!("{host}:{port}");
        // 显式超时，避免半开连接长时间卡在 connecting
        let socket = tokio::time::timeout(std::time::Duration::from_secs(15), TcpStream::connect(addr))
            .await
            .map_err(|_| "TCP连接超时(15s)")??;
        ws_rst = tokio_tungstenite::client_async_tls(request, socket).await?;

        let (mut write_half, mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = ws_url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) = tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 刷新access_token
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let appsecret = self.appsecret.clone();
        let access_token = Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token);
        tokio::spawn(async move {
            // 连续失败达到 3 次才放弃并重建连接，单次失败只重试
            let mut fail_count = 0u32;
            let mut refresh_interval = std::time::Duration::from_secs(55);
            loop {
                tokio::time::sleep(refresh_interval).await;
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                } else {
                    break;
                }
                match token_refresh(&appid, &appsecret).await {
                    Ok(access_token_struct) => {
                        fail_count = 0;
                        if access_token_struct.expires_in > 0 {
                            // 按 min(expires_in-300, 3600) 调整刷新间隔，下限 30s 防止热循环
                            let secs = access_token_struct
                                .expires_in
                                .saturating_sub(300).clamp(30, 3600);
                            refresh_interval = std::time::Duration::from_secs(secs);
                        }
                        if let Some(access_token) = access_token.upgrade() {
                            (*access_token.write().unwrap_or_else(|e| e.into_inner())) = access_token_struct.access_token;
                        } else {
                            break;
                        }
                    }
                    Err(err) => {
                        fail_count += 1;
                        let _ = cq_add_log_w(&format!(
                            "刷新access_token失败({fail_count}/3):{err}"
                        ));
                        if fail_count >= 3 {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
            // 移除conn
            if let Some(val) = is_stop.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });

        // 这里使用弱引用，防止可能的循环依赖
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let tx_ay_t2 = tx_ay_t.clone();
        let sn = Arc::<std::sync::RwLock<Option<u64>>>::downgrade(&self.sn);

        let self_data = SelfData {
            appid: Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token: Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map: Arc::<
                std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>,
            >::downgrade(&self.id_event_map),
            bot_id: Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
            qqgroup_audit_map: Arc::clone(&self.qqgroup_audit_map),
            qqgroup_audit_pending_cache: Arc::clone(&self.qqgroup_audit_pending_cache),
        };

        tokio::spawn(async move {
            while let Some(val) = is_stop.upgrade() {
                if val.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                    msg = read_halt.next() => {
                        let Some(msg) = msg else {
                            break;
                        };
                        // 获得json数据
                        let json_dat;
                        if let Some(val) =  get_json_dat(msg) {
                            json_dat = val;
                            let _ = crate::cqapi::cq_add_log(format!("qqguild_private 收到数据:{}", json_dat).as_str());
                        }else{
                            continue;
                        }
                        let op = read_json_str(&json_dat, "op");
                        if op == "10" {
                            let to_send = serde_json::json!({
                                "op":2,
                                "d":{
                                    "token":format!("QQBot {}",access_token_struct.access_token),
                                    "intents":(1 << 0) | (1 << 1) | (1 << 10) | (1 << 12) | (1 << 25) | (1 << 26) | (1 << 27) | (1 << 28),
                                    "shard":[0, 1],
                                }
                            });

                            let rst = tx_ay_t2.send(to_send).await;
                            if rst.is_err() {
                                break;
                            }
                        }else if op == "0" { // 事件
                            if let Some(sn_opt) = json_dat.get("s") {
                                if let Some(sn_t) = sn_opt.as_u64() {
                                    if let Some(val) = sn.upgrade() {
                                        (*val.write().unwrap_or_else(|e| e.into_inner())) = Some(sn_t);
                                    }else {
                                        break;
                                    }
                                }else{
                                    break;
                                }
                            }else{
                                break;
                            }
                            // 处理事件
                            let self_data = self_data.clone();
                            tokio::spawn(async move {
                                if let Err(e) = conv_event(&self_data,json_dat).await {
                                    let _ = crate::cqapi::cq_add_log_w(format!("err:{:?}", e).as_str());
                                }
                            });
                        }else if op == "11" { // 心跳

                        }else if op == "7" { // 重连
                            let _ = cq_add_log_w("qq要求重连");
                            break;
                        }else if op == "9" { // 参数错误
                            let _ = cq_add_log_w("qq参数错误");
                            break;
                        }else if op == "12" { // HTTP Callback ACK

                        }
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
            let _ = cq_add_log_w(&format!(
                "qqguild_private 连接已经断开(read_halt):{url_str_t}"
            ));
        });
        let url_str_t = self.url.clone();
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let sn = Arc::<std::sync::RwLock<Option<u64>>>::downgrade(&self.sn);
        tokio::spawn(async move {
            let url_str2 = url_str_t.clone();
            let is_stop2 = is_stop.clone();
            // 构造特殊心跳,防止长时间连接导致防火墙不处理数据
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Some(val) = is_stop.upgrade() {
                        if val.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                    } else {
                        break;
                    }
                    let sn_t;
                    if let Some(val) = sn.upgrade() {
                        sn_t = *val.read().unwrap_or_else(|e| e.into_inner());
                    } else {
                        break;
                    }
                    let to_send = serde_json::json!({
                        "op":1,
                        "d":sn_t
                    });
                    let rst = tx_ay_t.send(to_send).await;
                    if rst.is_err() {
                        break;
                    }
                }

                // 移除conn
                if let Some(val) = is_stop.upgrade() {
                    val.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = cq_add_log_w(&format!("qqguild_private 心跳已断开:{url_str2}"));
            });
            while let Some(msg) = rx_ay.recv().await {
                let rst = write_half
                    .send(hyper_tungstenite::tungstenite::Message::Text(
                        msg.to_string().into(),
                    ))
                    .await;
                if rst.is_err() {
                    break;
                }
            }
            // 移除conn
            if let Some(val) = is_stop2.upgrade() {
                val.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let _ = cq_add_log_w(&format!(
                "qqguild_private 连接已经断开(write_half):{url_str_t}"
            ));
        });
        Ok(())
    }

    async fn call_api(
        &self,
        platform: &str,
        _self_id: &str,
        passive_id: &str,
        json: &mut serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");

        let self_data = SelfData {
            appid: Arc::<std::sync::RwLock<String>>::downgrade(&self.appid),
            access_token: Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token),
            id_event_map: Arc::<
                std::sync::RwLock<HashMap<std::string::String, (u64, serde_json::Value)>>,
            >::downgrade(&self.id_event_map),
            bot_id: Arc::<std::sync::RwLock<std::string::String>>::downgrade(&self.bot_id),
            qqgroup_audit_map: Arc::clone(&self.qqgroup_audit_map),
            qqgroup_audit_pending_cache: Arc::clone(&self.qqgroup_audit_pending_cache),
        };

        if action == "send_group_msg" {
            if platform == "qqgroup_public" {
                return send_qq_group_msg(&self_data, json, passive_id, platform).await;
            }
            return send_group_msg(&self_data, json, passive_id).await;
        } else if action == "send_private_msg" {
            return send_private_msg(&self_data, json, passive_id).await;
        } else if action == "send_stream_msg" {
            return crate::qqguild_public::send_c2c_stream_msg(&self_data, json, passive_id).await;
        } else if action == "get_login_info" {
            return get_login_info(&self_data).await;
        } else if action == "get_group_list" {
            return get_group_list(&self_data, json, passive_id).await;
        } else if action == "get_group_member_info" {
            return get_group_member_info(&self_data, json, passive_id).await;
        } else if action == "get_stranger_info" {
            return get_stranger_info(&self_data, json, passive_id).await;
        } else if action == "delete_msg" {
            return delete_msg(&self_data, json).await;
        } else if action == "set_group_ban" {
            return set_group_ban(&self_data, json).await;
        }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        let appid = (*self.appid.read().unwrap_or_else(|e| e.into_inner())).to_owned();
        vec![
            ("qqguild_private".to_owned(), appid.clone()),
            ("qqgroup_public".to_owned(), appid),
        ]
    }
}
