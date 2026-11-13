use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{atomic::AtomicBool, Arc},
    thread,
    time::{Duration, SystemTime},
};
use uuid::Uuid;

use super::BotConnectTrait;
use crate::mytool::{cq_fix_image_file_in_arr, str_msg_to_arr};
use crate::{async_trait, cqapi::cq_add_log, cqapi::cq_add_log_w, mytool::read_json_str};
use lettre::Tokio1Executor;
use lettre::{
    message::{MultiPart, SinglePart},
    AsyncSmtpTransport, AsyncTransport,
};

/// rustls 包装流，用于 imap 连接
struct RustlsImapStream(rustls::StreamOwned<rustls::ClientConnection, TcpStream>);

impl std::io::Read for RustlsImapStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for RustlsImapStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl imap::extensions::idle::SetReadTimeout for RustlsImapStream {
    fn set_read_timeout(&mut self, dur: Option<Duration>) -> Result<(), imap::Error> {
        self.0
            .get_ref()
            .set_read_timeout(dur)
            .map_err(imap::Error::Io)
    }
}

#[derive(Debug, Clone)]
pub struct EmailConnect {
    pub self_id: Arc<std::sync::RwLock<String>>,
    pub smtp_server: Arc<std::sync::RwLock<String>>,
    pub smtp_port: Arc<std::sync::RwLock<u16>>,
    pub smtp_ssl: Arc<std::sync::RwLock<bool>>,
    pub password: Arc<std::sync::RwLock<String>>,
    pub url: String,
    pub is_stop: Arc<AtomicBool>,
    pub last_uid: Arc<std::sync::RwLock<u32>>,
}

impl EmailConnect {
    pub fn build(url: &str) -> Self {
        EmailConnect {
            self_id: Arc::new(std::sync::RwLock::new("".to_owned())),
            smtp_server: Arc::new(std::sync::RwLock::new("".to_owned())),
            smtp_port: Arc::new(std::sync::RwLock::new(465)),
            smtp_ssl: Arc::new(std::sync::RwLock::new(true)),
            password: Arc::new(std::sync::RwLock::new("".to_owned())),
            url: url.to_owned(),
            is_stop: Arc::new(AtomicBool::new(false)),
            last_uid: Arc::new(std::sync::RwLock::new(0)),
        }
    }

    fn strip_html_tags(html: &str) -> String {
        // 简单去标签：丢弃 <...> 之间的内容，保留纯文本
        let mut ret = String::with_capacity(html.len());
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => ret.push(c),
                _ => {}
            }
        }
        ret
    }

    fn deal_fetcharr(
        &self,
        fetch: &imap::types::Fetches,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use base64::Engine;
        if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let self_id = self.self_id.read().unwrap_or_else(|e| e.into_inner()).to_owned();
        let message = fetch.iter().next().ok_or("no message")?;
        let body = message.body().ok_or("no body")?;
        let raw_message_b64 = base64::engine::general_purpose::STANDARD.encode(body);
        let message = mail_parser::MessageParser::default()
            .parse(body)
            .ok_or("mail_parser err")?;
        let sender = message
            .from()
            .ok_or("sender can't get1")?
            .first()
            .ok_or("sender can't get2")?;
        let user_id = sender.address().ok_or("user_id can't get1")?;
        // 无显示名时降级用邮箱地址本地部分，避免整封邮件被丢弃
        let user_name = match sender.name().filter(|s| !s.trim().is_empty()) {
            Some(name) => name.to_string(),
            None => {
                let addr = user_id.to_string();
                let local = addr.split('@').next().unwrap_or("").to_string();
                if local.is_empty() {
                    addr
                } else {
                    local
                }
            }
        };
        // 纯 HTML 邮件降级取 html 正文(简单去标签)，仍缺失时用空串，不再报错
        let message = match message.body_text(0) {
            Some(text) => text.to_string(),
            None => match message.body_html(0) {
                Some(html) => Self::strip_html_tags(&html),
                None => String::new(),
            },
        };
        if user_id == self_id {
            return Ok(());
        }
        let event_json = serde_json::json!({
            "time":SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "self_id":self_id,
            "post_type":"message",
            "message_type":"private",
            "sub_type":"friend",
            "message_id":Uuid::new_v4().to_string(),
            "user_id":user_id,
            "message":message,
            "raw_message":raw_message_b64,
            "font":0,
            "sender":{
                "user_id":user_id,
                "nickname":user_name,
            },
            "platform":"email"
        });
        let json_str = event_json.to_string();
        let _ = cq_add_log(&format!("EMAIL_OB_EVENT:{json_str}"));
        if let Err(e) = crate::cqevent::do_1207_event(&json_str) {
            let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
        }
        Ok(())
    }

    fn run_imap_loop<T: std::io::Read + std::io::Write + imap::extensions::idle::SetReadTimeout>(
        &self,
        imap_session: &mut imap::Session<T>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = cq_add_log(&format!("邮件协议已经连接:{}", self.url));

        // 首次运行时，获取当前最大 UID 作为基准，不处理历史未读邮件
        // 必须使用 uid_search：search 返回的是 sequence number，不是 UID
        if *self.last_uid.read().unwrap_or_else(|e| e.into_inner()) == 0 {
            let uids = imap_session.uid_search("UNSEEN")?;
            if let Some(&max_uid) = uids.iter().max() {
                *self.last_uid.write().unwrap_or_else(|e| e.into_inner()) = max_uid;
                let _ = cq_add_log(&format!(
                    "首次运行，跳过历史未读邮件，设置基准 UID: {}",
                    max_uid
                ));
            }
        }

        loop {
            if self.is_stop.load(std::sync::atomic::Ordering::Relaxed) {
                imap_session.logout()?;
                break;
            }
            // 使用增量拉取：只搜索 UID 大于 last_uid 的未读邮件
            let last_uid = *self.last_uid.read().unwrap_or_else(|e| e.into_inner());
            // checked_add 防止 u32 溢出 panic
            let query = format!("UID {}:* UNSEEN", last_uid.checked_add(1).unwrap_or(u32::MAX));
            let uids = imap_session.uid_search(&query)?;
            // 防御性过滤：某些 IMAP 服务器在指定 UID 不存在时可能返回意外结果
            let uids: Vec<u32> = uids.into_iter().filter(|&uid| uid > last_uid).collect();
            if uids.is_empty() {
                let mut handle = imap_session.idle();
                let _ = handle
                    .timeout(Duration::from_secs(15))
                    .keepalive(false)
                    .wait_while(|_| false);
                continue;
            } else {
                for uid in uids {
                    let size_fetch = imap_session.uid_fetch(uid.to_string(), "RFC822.SIZE")?;
                    let mut is_large = false;
                    if let Some(m) = size_fetch.iter().next() {
                        if let Some(s) = m.size {
                            let _ = cq_add_log(&format!("邮件大小:uid: {}, size: {}", uid, s));
                            if s > 16 * 1024 * 1024 {
                                let _ = cq_add_log_w(&format!(
                                    "邮件过大(>16MB)，跳过处理。uid: {}, size: {}",
                                    uid, s
                                ));
                                is_large = true;
                            }
                        }
                    }

                    if !is_large {
                        let fetcharr = imap_session.uid_fetch(uid.to_string(), "RFC822")?;
                        if let Err(err) = self.deal_fetcharr(&fetcharr) {
                            let _ = cq_add_log_w(&format!("邮件处理错误:{:?}", err));
                        }
                    }
                    // 设置已读标记，现在不标记已读了
                    // imap_session.uid_store(uid.to_string(), "+FLAGS (\\Seen)")?;
                    // 更新 last_uid
                    let mut last_uid_lock = self.last_uid.write().unwrap_or_else(|e| e.into_inner());
                    if uid > *last_uid_lock {
                        *last_uid_lock = uid;
                    }
                }
            }
        }
        Ok(())
    }

    fn do_connect(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_json_str = self.url.get(8..).ok_or("email url格式错误")?;
        let config_json: serde_json::Value = serde_json::from_str(config_json_str)?;
        let imap_server = config_json
            .get("imap_server")
            .ok_or("email url格式错误:没有 imap_server")?
            .as_str()
            .ok_or("email url格式错误: imap_server 不是字符串")?
            .to_owned();
        let imap_port = config_json["imap_port"]
            .as_u64()
            .ok_or("email url格式错误: imap_port 不是数字")?;
        let imap_ssl = config_json["imap_ssl"]
            .as_bool()
            .ok_or("email url格式错误: imap_ssl 不是布尔值")?;
        let username = config_json["username"]
            .as_str()
            .ok_or("email url格式错误: username 不是字符串")?
            .to_owned();
        let password = config_json["password"]
            .as_str()
            .ok_or("email url格式错误: password 不是字符串")?
            .to_owned();
        let smtp_server = config_json["smtp_server"]
            .as_str()
            .ok_or("email url格式错误: smtp_server 不是字符串")?
            .to_owned();
        let smtp_port = config_json["smtp_port"]
            .as_u64()
            .ok_or("email url格式错误: smtp_port 不是数字")?;
        let smtp_ssl = config_json["smtp_ssl"]
            .as_bool()
            .ok_or("email url格式错误: smtp_ssl 不是布尔值")?;
        *self.smtp_server.write().unwrap_or_else(|e| e.into_inner()) = smtp_server;
        *self.smtp_port.write().unwrap_or_else(|e| e.into_inner()) = smtp_port as u16;
        *self.self_id.write().unwrap_or_else(|e| e.into_inner()) = username.to_owned();
        *self.password.write().unwrap_or_else(|e| e.into_inner()) = password.to_owned();
        *self.smtp_ssl.write().unwrap_or_else(|e| e.into_inner()) = smtp_ssl.to_owned();
        let imap_port_u16 =
            u16::try_from(imap_port).map_err(|_| "email url格式错误: imap_port 非法")?;
        if imap_ssl {
            // 显式连接超时，避免半开连接长时间卡在 connect
            let addr = (imap_server.as_str(), imap_port_u16)
                .to_socket_addrs()?
                .next()
                .ok_or("IMAP地址解析失败")?;
            let tcp_stream = TcpStream::connect_timeout(&addr, Duration::from_secs(15))?;
            let _ = rustls::crypto::ring::default_provider().install_default();
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            let server_name = rustls::pki_types::ServerName::try_from(imap_server.clone())
                .map_err(|e| format!("TLS服务器名称无效: {}", e))?;
            let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
                .map_err(|e| format!("TLS连接创建失败: {}", e))?;
            let mut tls_stream = rustls::StreamOwned::new(conn, tcp_stream);
            use std::io::Write;
            tls_stream
                .flush()
                .map_err(|e| format!("TLS握手失败: {}", e))?;
            let client = imap::Client::new(RustlsImapStream(tls_stream));
            let mut imap_session = client.login(username, password).map_err(|e| e.0)?;
            let caps = imap_session.capabilities()?;
            if caps.has_str("ID") {
                let _ = imap_session
                    .run_command_and_check_ok("ID (\"name\" \"myclient\" \"version\" \"1.0.0\")");
            }
            imap_session.select("INBOX")?;
            self.run_imap_loop(&mut imap_session)?;
        } else {
            let addr = (imap_server.as_str(), imap_port_u16)
                .to_socket_addrs()?
                .next()
                .ok_or("IMAP地址解析失败")?;
            let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(15))?;
            let client = imap::Client::new(stream);
            let mut imap_session = client.login(username, password).map_err(|e| e.0)?;
            let caps = imap_session.capabilities()?;
            if caps.has_str("ID") {
                let _ = imap_session
                    .run_command_and_check_ok("ID (\"name\" \"myclient\" \"version\" \"1.0.0\")");
            }
            imap_session.select("INBOX")?;
            self.run_imap_loop(&mut imap_session)?;
        }
        Ok(())
    }
    fn get_json_bool(js: &serde_json::Value, key: &str) -> bool {
        if let Some(j) = js.get(key) {
            if let Some(value) = j.as_bool() { value } else if let Some(value) = j.as_str() { value == "true" } else {
                false
            }
        } else {
            false
        }
    }
    fn get_auto_escape_from_params(&self, params: &serde_json::Value) -> bool {
        Self::get_json_bool(params, "auto_escape")
    }
    async fn make_email_msg(
        &self,
        message_arr: &serde_json::Value,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        // 返回 (纯文本, html正文)；图片以 <img> 标签并入 html
        let msg_arr = message_arr.as_array().ok_or("message not arr")?;
        let mut plain = String::new();
        let mut html = String::new();
        for it in msg_arr {
            let tp = it.get("type").ok_or("type not found")?;
            if tp == "text" {
                let t = it
                    .get("data")
                    .ok_or("data not found")?
                    .get("text")
                    .ok_or("text not found")?
                    .as_str()
                    .ok_or("text not str")?
                    .to_owned();
                if !plain.is_empty() {
                    plain.push('\n');
                }
                plain.push_str(&t);
                html.push_str(&format!(
                    "<p style=\"white-space:pre-line;\">{}</p>",
                    html_escape::encode_double_quoted_attribute(&t)
                ));
            } else if tp == "image" {
                let uri = it
                    .get("data")
                    .ok_or("data not found")?
                    .get("file")
                    .ok_or("file not found")?
                    .as_str()
                    .ok_or("file not str")?;
                let out;
                if uri.starts_with("http://") || uri.starts_with("https://") {
                    out = format!("<img src={} />", serde_json::json!(uri));
                } else if uri.starts_with("base64://") {
                    let b64 = uri.split_at(9).1;
                    out = format!(
                        "<img src={} />",
                        serde_json::json!("data:image/png;base64,".to_owned() + b64)
                    );
                } else {
                    continue;
                }
                html.push_str(&out);
            }
        }
        Ok((plain, html))
    }
    async fn deal_ob_send_private_msg(
        &self,
        params: &serde_json::Value,
        echo: &serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut message_arr: serde_json::Value;
        let message_rst = params.get("message").ok_or("message not found")?;
        if message_rst.is_string() {
            if self.get_auto_escape_from_params(params) {
                message_arr = serde_json::json!(
                    [{"type":"text","data":{
                        "text": message_rst.as_str()
                    }}]
                );
            } else {
                message_arr = str_msg_to_arr(message_rst)
                    .map_err(|x| format!("str_msg_to_arr err:{:?}", x))?;
            }
        } else {
            message_arr = params.get("message").ok_or("message not found")?.to_owned();
        }
        cq_fix_image_file_in_arr(&mut message_arr);

        let user_id = read_json_str(params, "user_id");
        let bot_id = self.self_id.read().unwrap_or_else(|e| e.into_inner()).to_owned();

        let server = self.smtp_server.read().unwrap_or_else(|e| e.into_inner()).to_owned();
        let port = self.smtp_port.read().unwrap_or_else(|e| e.into_inner()).to_owned();
        let password = self.password.read().unwrap_or_else(|e| e.into_inner()).to_owned();

        let (plain, html) = self.make_email_msg(&message_arr).await?;
        if plain.is_empty() && html.is_empty() {
            return Err("message is empty".into());
        }
        // 纯文本直接发送；否则 alternative(纯文本, related(html正文为根))
        // 图片已通过远程链接/data URI 内嵌在 html 正文中，不再作为并列 part 丢失
        let multipart = if html.is_empty() {
            MultiPart::alternative().singlepart(SinglePart::plain(plain))
        } else {
            let related = MultiPart::related().singlepart(SinglePart::html(html));
            let alt_builder = MultiPart::alternative();
            if plain.is_empty() {
                alt_builder.multipart(related)
            } else {
                alt_builder
                    .singlepart(SinglePart::plain(plain))
                    .multipart(related)
            }
        };
        let email = lettre::Message::builder()
            .from(format!("Bot <{bot_id}>").parse()?)
            .to(format!("User <{user_id}>").parse()?)
            .subject("Dear User")
            .header(lettre::message::header::ContentType::TEXT_PLAIN)
            .multipart(multipart)?;
        let creds = lettre::transport::smtp::authentication::Credentials::new(bot_id, password);
        let use_ssl = self.smtp_ssl.read().unwrap_or_else(|e| e.into_inner()).to_owned();
        let mailer;
        if use_ssl {
            if port == 465 {
                // 465 端口使用隐式 TLS (SMTPS)
                mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&server)?
                    .port(port)
                    .credentials(creds)
                    .build();
            } else {
                // 587 端口使用 STARTTLS (显式 TLS)
                mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&server)?
                    .port(port)
                    .credentials(creds)
                    .build();
            }
        } else {
            mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(server.to_owned())
                .port(port)
                .tls(lettre::transport::smtp::client::Tls::None)
                .credentials(creds)
                .build();
        }
        let _ = cq_add_log(&format!("sending email:{}", message_arr));
        let send_ret = match mailer.send(email).await {
            Ok(_) => "ok".to_owned(),
            Err(e) => format!("{:?}", e),
        };
        if send_ret == "ok" {
            let msg_id = Uuid::new_v4().to_string();
            let _ = cq_add_log(&format!("send email ok:{msg_id}"));
            Ok(serde_json::json!({
                "status": "ok",
                "retcode": 0,
                "data": {
                    "message_id": msg_id
                },
                "echo": echo.to_owned()
            }))
        } else {
            Ok(serde_json::json!({
                "status": "failed",
                "retcode": 1404,
                "data": {
                },
                "message":send_ret,
                "echo": echo.to_owned()
            }))
        }
    }
}

#[async_trait]
impl BotConnectTrait for EmailConnect {
    async fn disconnect(&mut self) {
        self.is_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn get_alive(&self) -> bool {
        !self.is_stop.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let self_t = self.clone();
        thread::spawn(move || {
            if let Err(e) = self_t.do_connect() {
                let _ = crate::cqapi::cq_add_log(format!("{:?}", e).as_str());
            }
            self_t
                .is_stop
                .store(true, std::sync::atomic::Ordering::Relaxed);
        });
        Ok(())
    }

    async fn call_api(
        &self,
        _platform: &str,
        _self_id: &str,
        _passive_id: &str,
        json: &mut serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let action = json
            .get("action")
            .ok_or("action not found")?
            .as_str()
            .ok_or("action not str")?;
        let echo = json.get("echo").unwrap_or(&serde_json::Value::Null);
        let def = serde_json::json!({});
        let params = json.get("params").unwrap_or(&def);

        let send_json = match action {
            "send_private_msg" => {
                // cq_add_log("send_private_msg触发").unwrap();
                self.deal_ob_send_private_msg(params, echo).await?
            }
            _ => {
                serde_json::json!({
                    "status":"failed",
                    "retcode":1404,
                    "echo":echo
                })
            }
        };
        return Ok(send_json);
    }

    fn get_platform_and_self_id(&self) -> Vec<(String, String)> {
        let lk = self.self_id.read().unwrap_or_else(|e| e.into_inner());
        if lk.is_empty() {
            return vec![];
        }
        vec![("email".to_owned(), lk.to_owned())]
    }
}
