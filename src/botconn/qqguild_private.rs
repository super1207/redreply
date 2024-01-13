use std::{sync::{atomic::AtomicBool, Arc, RwLock}, str::FromStr};

use async_trait::async_trait;
use futures_util::{StreamExt, SinkExt};
use hyper::header::{HeaderValue, HeaderName};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite;

use crate::{cqapi::cq_add_log_w, mytool::{read_json_str, read_json_obj_or_null, cq_text_encode, cq_params_encode, str_msg_to_arr}};

use super::BotConnectTrait;

#[derive(Debug)]
pub struct QQGuildPrivateConnect {
    pub url:String,
    pub appid:String,
    pub appsecret:String,
    pub token:String,
    pub access_token:Arc<std::sync::RwLock<String>>,
    pub tx:Option<tokio::sync::mpsc::Sender<serde_json::Value>>,
    pub is_stop:Arc<AtomicBool>,
    pub stop_tx:Option<tokio::sync::mpsc::Sender<bool>>,
    pub sn:Arc<std::sync::RwLock<Option<u64>>>,
}

struct AccessTokenStruct {
    access_token:String,
    _expires_in:u64,
}

async fn token_refresh(appid:&str,client_secret:&str) -> Result<AccessTokenStruct, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://bots.qq.com/app/getAppAccessToken")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let json_data:serde_json::Value = serde_json::json!({
        "appId":appid,
        "clientSecret":client_secret
    });
    let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
    req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    let expires_in = read_json_str(&json_val, "expires_in").parse::<u64>()?;
    Ok(AccessTokenStruct {
        access_token: json_val.get("access_token").ok_or("No access_token")?.as_str().ok_or("access_token not str")?.to_owned(),
        _expires_in:expires_in,
    })
}

async fn get_gateway(access_token:&str,appid:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let uri = reqwest::Url::from_str("https://api.sgroup.qq.com/gateway")?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let mut req = client.get(uri).build()?;
    req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {access_token}"))?);
    req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(appid)?);
    let ret = client.execute(req).await?;
    let ret_str =  ret.text().await?;
    let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
    Ok(json_val.get("url").ok_or("No url")?.as_str().ok_or("url not str")?.to_owned())
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
    crate::cqapi::cq_add_log(format!("qqguild_private 收到数据:{}", json_dat.to_string()).as_str()).unwrap();
    return Some(json_dat);
}


impl QQGuildPrivateConnect {
    pub fn build(url:&str) -> Self {
        QQGuildPrivateConnect {
            url:url.to_owned(),
            token:"".to_owned(),
            tx:None,
            is_stop:Arc::new(AtomicBool::new(false)),
            stop_tx: None,
            appid: "".to_owned(),
            appsecret: "".to_owned(),
            access_token: Arc::new(RwLock::new("".to_owned())),
            sn:Arc::new(RwLock::new(None)),
        }
    }
}

async fn conv_event(self_id:&str,root:serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tp = read_json_str(&root, "t");
    if tp == "MESSAGE_CREATE" {
        let d = root.get("d").ok_or("No d")?;
        let tm_str = read_json_str(&d, "timestamp");
        let tm = chrono::DateTime::parse_from_rfc3339(&tm_str)?.timestamp();
        let content = read_json_str(&d, "content"); // 没有message算什么消息
        let message_id = read_json_str(&d, "id");
        let user = read_json_obj_or_null(&d, "author"); // 可以没有发送者
        let user_id = read_json_str(&user, "id");
        let nickname =  read_json_str(&user, "username");
        let cq_msg = qq_content_to_cqstr(&content)?;
        let channel_id =read_json_str(&d, "channel_id"); // 没有channel就无法回复
        let guild_id = read_json_str(&d, "guild_id");
        let member = read_json_obj_or_null(&d, "member"); // 可以没有member
        let card =  read_json_str(&member, "nick");
        let event_json = serde_json::json!({
            "time":tm,
            "self_id":self_id,
            "platform":"qqguild_private",
            "post_type":"message",
            "message_type":"group",
            "sub_type":"normal",
            "message_id":message_id,
            "group_id":channel_id,
            "groups_id":guild_id,
            "user_id":user_id,
            "message":cq_msg,
            "raw_message":content,
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
        tokio::task::spawn_blocking(move ||{
            if let Err(e) = crate::cqevent::do_1207_event(&event_json.to_string()) {
                crate::cqapi::cq_add_log(format!("{:?}", e).as_str()).unwrap();
            }
        });
    }
    Ok(())
}

fn make_qq_text(text:&str) -> String {
    let mut ret = String::new();
    for ch in text.chars() {
        match ch {
            '&' => {
                ret += "&amp;";
            }
            '<' => {
                ret += "&lt;";
            }
            '>' => {
                ret += "&gt;";
            }
            _ => {
                ret += &ch.to_string();
            }
        }
    }
    ret
}

struct QQMsgNode{
    content:String,
    imgs:Vec<Vec<u8>>
}

async fn cq_msg_to_qq(js_arr:&serde_json::Value) -> Result<QQMsgNode,Box<dyn std::error::Error + Send + Sync>> {
    let mut msg_node = QQMsgNode{
        content: "".to_string(),
        imgs: vec![],
    };
    let arr = js_arr.as_array().ok_or("js_arr not an err")?;
    // let mut out = String::new();
    for it in arr {
        let tp = it.get("type").ok_or("type not found")?;
        if tp == "text" {
            let text = it.get("data").ok_or("data not found")?.get("text").ok_or("text not found")?.as_str().ok_or("text not a string")?;
            msg_node.content += &make_qq_text(&text);
        } else if tp == "at" {
            let qq = it.get("data").ok_or("data not found")?.get("qq").ok_or("qq not found")?.as_str().ok_or("qq not a string")?;
            if qq == "all" {
                msg_node.content += "@全体成员"
            }else {
                msg_node.content += &format!("<@{}>", make_qq_text(qq));
            }
        }
        else if tp == "image" {
            let file = it.get("data").ok_or("data not found")?.get("file").ok_or("file not found")?.as_str().ok_or("file not a string")?;
            if file.starts_with("http://") ||  file.starts_with("https://") {
                let client = reqwest::Client::builder().no_proxy().build()?;
                let req = client.get(file).build()?;
                let ret = client.execute(req).await?;
                let img_buffer =  ret.bytes().await?.to_vec();
                msg_node.imgs.push(img_buffer);
            }else if file.starts_with("base64://") {
                let b64_str = file.split_at(9).1;
                let img_buffer = base64::Engine::decode(&base64::engine::GeneralPurpose::new(
                    &base64::alphabet::STANDARD,
                    base64::engine::general_purpose::PAD), b64_str)?;
                msg_node.imgs.push(img_buffer);
            }
        }
    }
    Ok(msg_node)
}


fn str_msg_to_arr_safe(js:&serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let ret = str_msg_to_arr(js);
    if let Ok(ret) = ret {
        return Ok(ret);
    }else {
        return None.ok_or(format!("str_msg_to_arr error:{}", ret.err().unwrap()))?;
    }
}


async fn send_group_msg(self_t:&QQGuildPrivateConnect,json:&serde_json::Value,passive_id:&str) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {

    let params = read_json_obj_or_null(json, "params");  
    let group_id = read_json_str(&params, "group_id");

    let message = params.get("message").ok_or("message is not exist")?;
    let qq_msg_node;
    if message.is_array() {
        qq_msg_node = cq_msg_to_qq(message).await?;
        
    }else{
        
        let msg_arr_rst = str_msg_to_arr_safe(message);
        if let Ok(msg_arr) = msg_arr_rst {
            qq_msg_node = cq_msg_to_qq(&msg_arr).await?;
        }else{
            return None.ok_or("call str_msg_to_arr err")?;
        }
        
    }

    let passive_id_opt;
    if passive_id != "" {
        passive_id_opt = Some(passive_id);
    }else{
        passive_id_opt = None;
    }

    if qq_msg_node.imgs.len() == 0 {
        let json_data = serde_json::json!({
            "msg_id":passive_id_opt,
            "content":qq_msg_node.content
        });
        // 处理日志
        {
            let js_str = json_data.to_string();
            let out_str = js_str.get(0..2000);
            if out_str.is_some() {
                crate::cqapi::cq_add_log(format!("发送数据:{}...", out_str.unwrap()).as_str()).unwrap();
            }else {
                crate::cqapi::cq_add_log(format!("发送数据:{}", js_str).as_str()).unwrap();
            }
        }
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{group_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut req = client.post(uri).body(reqwest::Body::from(json_data.to_string())).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("application/json")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":read_json_str(&json_val, "id")
            }
        }));
    } else {
        let uri = reqwest::Url::from_str(&format!("https://api.sgroup.qq.com/channels/{group_id}/messages"))?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let mut form = reqwest::multipart::Form::new().part(
        "file_image",
        reqwest::multipart::Part::bytes(qq_msg_node.imgs[0].clone()).file_name("pic.png"),
        );
        if passive_id_opt != None {
            form = form.text("msg_id", passive_id.to_owned());
        }
        form = form.text("content",qq_msg_node.content);
        let mut req = client.post(uri.to_owned()).multipart(form).build()?;
        req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
        req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
        req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
        req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
        //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
        let ret = client.execute(req).await?;
        let ret_str =  ret.text().await?; 
        let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
        crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
        let mut id = read_json_str(&json_val, "id");
        for it in qq_msg_node.imgs.get(1..).unwrap() {
            let mut form = reqwest::multipart::Form::new().part(
                "file_image",
                reqwest::multipart::Part::bytes(it.clone()).file_name("pic.png"),
                );
            if passive_id_opt != None {
                form = form.text("msg_id", passive_id.to_owned());
            }
            let mut req = client.post(uri.to_owned()).multipart(form).build()?;
            req.headers_mut().append(HeaderName::from_str("Authorization")?, HeaderValue::from_str(&format!("QQBot {}",self_t.access_token.read().unwrap()))?);
            req.headers_mut().append(HeaderName::from_str("X-Union-Appid")?, HeaderValue::from_str(&self_t.appid)?);
            req.headers_mut().append(HeaderName::from_str("Content-Type")?, HeaderValue::from_str("multipart/form-data")?);
            req.headers_mut().append(HeaderName::from_str("Accept")?, HeaderValue::from_str("application/json")?);
            //crate::cqapi::cq_add_log(format!("headers_mut:{:?}", req.headers_mut()).as_str()).unwrap();
            let ret = client.execute(req).await?;
            let ret_str =  ret.text().await?; 
            let json_val: serde_json::Value = serde_json::from_str(&ret_str)?;
            crate::cqapi::cq_add_log(format!("接收qq guild API数据:{}", json_val.to_string()).as_str()).unwrap();
            id += "|";
            id += &read_json_str(&json_val, "id");
        }
        return Ok(serde_json::json!({
            "retcode":0,
            "status":"ok",
            "data":{
                "message_id":id
            }
        }));
    }

}

pub fn qq_content_to_cqstr(qqstr:&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let cqstr = qqstr.chars().collect::<Vec<char>>();
    let mut out_str = String::new();
    
    let mut text = "".to_owned();
    let mut stat = 0;
    let mut i = 0usize;
    while i < cqstr.len() {
        let cur_ch = cqstr[i];
        if stat == 0 {
            if cur_ch == '<' {
                stat = 1;
                out_str += &cq_text_encode(&text);
                text.clear();
                text.push(cur_ch);
                i += 1;
            }else if cur_ch == '&' {
                let t;
                if i+5 < cqstr.len(){
                    t = &cqstr[i..i+5];
                }else if i + 4 < cqstr.len(){
                    t = &cqstr[i..i+4];
                }else{
                    t =  &cqstr[i..i];
                }
                if t.starts_with(&['&','g','t',';']) {
                    text.push('>');
                    i += 4;
                }else if t.starts_with(&['&','l','t',';']) {
                    text.push('<');
                    i += 4;
                }else if t.starts_with(&['&','a','m','p',';']) {
                    text.push('&');
                    i += 5;
                }
                else{
                    text.push('&');
                    i += 1;
                }
            }else{
                text.push(cur_ch);
                i += 1;
            }
        }else{
            if cur_ch == '>' {
                stat = 0;
                text += ">";
                println!("code:{text}");

                if text.starts_with("<@!"){
                    let user_id = text.get(3..text.len()-1).ok_or("error")?;
                    out_str += &format!("[CQ:at,qq={}]",cq_params_encode(user_id));
                }else if text.starts_with("<@"){
                    let user_id = text.get(2..text.len()-1).ok_or("error")?;
                    out_str += &format!("[CQ:at,qq={}]",cq_params_encode(user_id));
                }
                text.clear();
                i += 1;
            }else{
                i += 1;
                text.push(cur_ch);
            }
        }
    }
    if text.len() != 0 {
        out_str += &cq_text_encode(&text);
    }
    Ok(out_str)
}

#[async_trait]
impl BotConnectTrait for QQGuildPrivateConnect {

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

        //token_refresh()?;
        //println!("正在连接satori：{}",self.url);
        let config_json_str = self.url.get(18..).ok_or("qqguild_private url格式错误")?;
        let config_json:serde_json::Value =  serde_json::from_str(config_json_str)?;
        println!("{:?}",config_json);
        self.appid = config_json.get("AppID").ok_or("qqguild_private AppID格式错误:没有AppID")?.as_str().ok_or("qqguild_private AppID格式错误:AppID不是字符串")?.to_owned();
        self.appsecret = config_json.get("AppSecret").ok_or("qqguild_private AppSecret格式错误:没有AppSecret")?.as_str().ok_or("qqguild_private AppSecret格式错误:AppSecret不是字符串")?.to_owned();
        self.token = config_json.get("Token").ok_or("qqguild_private Token格式错误:没有Token")?.as_str().ok_or("qqguild_private Token格式错误:Token不是字符串")?.to_owned();
        
        let access_token_struct = token_refresh(&self.appid,&self.appsecret).await?;
        (*self.access_token.write().unwrap()) = access_token_struct.access_token.to_owned();

        let ws_url = get_gateway(&access_token_struct.access_token,&self.appid).await?;
        println!("get_gateway:{}",ws_url);
        
        let request = tungstenite::client::IntoClientRequest::into_client_request(&ws_url)?;
        let ws_rst;
        let port_opt  = request.uri().port();
        let port;
        if port_opt.is_none() {
            port = 443;
        }else {
            port  = port_opt.unwrap().into();
        }
        let addr = format!("{}:{}",request.uri().host().unwrap(),port);
        let socket = TcpStream::connect(addr).await.unwrap();
        ws_rst = tokio_tungstenite::client_async_tls(request, socket).await?;

        let (mut write_half,mut read_halt) = ws_rst.0.split();
        let (tx_ay, mut rx_ay) =  tokio::sync::mpsc::channel::<serde_json::Value>(128);
        let tx_ay_t = tx_ay.clone();
        let url_str_t = ws_url.clone();
        self.tx = Some(tx_ay_t.clone());
        let (stoptx, mut stoprx) =  tokio::sync::mpsc::channel::<bool>(1);
        self.stop_tx = Some(stoptx);

        // 刷新access_token
        let is_stop = Arc::<AtomicBool>::downgrade(&self.is_stop);
        let appid = self.appid.clone();
        let appsecret = self.appsecret.clone();
        let access_token = Arc::<std::sync::RwLock<String>>::downgrade(&self.access_token);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(55)).await;
                if let Some(val) = is_stop.upgrade() {
                    if val.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }else {
                    break; 
                }
                if let Ok(access_token_struct) = token_refresh(&appid,&appsecret).await {
                    if let Some(access_token) = access_token.upgrade(){
                        (*access_token.write().unwrap()) = access_token_struct.access_token;
                    }else{
                        break;
                    }
                }else{
                    break;
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
        let appid = self.appid.clone();
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
                        let op = read_json_str(&json_dat, "op");
                        if op == "10" {
                            let to_send = serde_json::json!({
                                "op":2,
                                "d":{
                                    "token":format!("QQBot {}",access_token_struct.access_token),
                                    "intents":0 | (1 << 0) | (1 << 1) | (1 << 9) | (1 << 10) | (1 << 12) | (1 << 26) | (1 << 27) | (1 << 28),
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
                                        (*val.write().unwrap()) = Some(sn_t);
                                    }else {
                                        break;
                                    }
                                }else{
                                    break;
                                }
                            }else{
                                break;
                            }
                            let appid_t = appid.clone();
                            // 处理事件
                            tokio::spawn(async move {
                                if let Err(e) = conv_event(&appid_t,json_dat).await {
                                    crate::cqapi::cq_add_log_w(format!("err:{:?}", e).as_str()).unwrap();
                                }
                            });
                        }else if op == "0" { // 心跳
                            
                        }else if op == "7" { // 重连
                            cq_add_log_w("qq要求重连").unwrap();
                            break;
                        }else if op == "9" { // 参数错误
                            cq_add_log_w("qq参数错误").unwrap();
                            break;
                        }else if op == "11" { // HTTP Callback ACK
                            
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
            cq_add_log_w(&format!("qqguild_private 连接已经断开(read_halt):{url_str_t}")).unwrap();
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
                    }else {
                        break; 
                    }
                    let sn_t;
                    if let Some(val) = sn.upgrade() {
                        sn_t = val.read().unwrap().clone();
                    }else {
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
                cq_add_log_w(&format!("satori心跳已断开:{url_str2}")).unwrap();
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
            cq_add_log_w(&format!("satori连接已经断开(write_half):{url_str_t}")).unwrap();
        });
        Ok(())
    }


    fn get_url(&self) -> String {
        return self.url.clone();
    }

    async fn call_api(&self,_platform:&str,_self_id:&str,passive_id:&str,json:&mut serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = read_json_str(json, "action");
        if action == "send_group_msg" {
            return send_group_msg(self,json,passive_id).await;
        }
        // Ok(serde_json::json!({}))
        // else if action == "send_private_msg" {
        //     return Self::send_private_msg(self,json,platform,self_id).await;
        // }
        // else if action == "get_login_info" {
        //     return Self::get_login_info(self,json,platform,self_id).await;
        // }
        // else if action == "get_group_list" {
        //     return Self::get_group_list(self,json,platform,self_id).await;
        // }
        // else if action == "get_group_member_info" {
        //     return Self::get_group_member_info(self,json,platform,self_id).await;
        // }
        // else if action == "get_stranger_info" {
        //     return Self::get_stranger_info(self,json,platform,self_id).await;
        // }
        return Ok(serde_json::json!({
            "retcode":1404,
            "status":"failed"
        }));
    }

    fn get_platform_and_self_id(&self) -> Vec<(String,String)> {
        return vec![("qqguild_private".to_owned(),self.appid.to_owned())];
    }
}