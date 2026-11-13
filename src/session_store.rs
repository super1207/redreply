use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::RwLock;

use crate::{InputStream, ScriptRelatMsg};

type OnebotSession = (tokio::sync::mpsc::Sender<String>, String, String);

pub struct SessionStore {
    message_ids: RwLock<HashMap<String, VecDeque<(String, String)>>>,
    input_streams: RwLock<Vec<InputStream>>,
    script_related_messages: RwLock<HashMap<String, ScriptRelatMsg>>,
    history_log: RwLock<VecDeque<String>>,
    log_sessions: tokio::sync::RwLock<HashMap<String, tokio::sync::mpsc::Sender<String>>>,
    python_echo_sessions:
        tokio::sync::RwLock<HashMap<String, tokio::sync::mpsc::Sender<serde_json::Value>>>,
    /// python 桥接处理器，附带代次号：新连接接入时旧连接凭代次号感知被替换并自行退出
    python_handler: tokio::sync::RwLock<Option<(u64, tokio::sync::mpsc::Sender<String>)>>,
    python_handler_generation: std::sync::atomic::AtomicU64,
    onebot_sessions: tokio::sync::RwLock<HashMap<String, OnebotSession>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            message_ids: RwLock::new(HashMap::new()),
            input_streams: RwLock::new(Vec::new()),
            script_related_messages: RwLock::new(HashMap::new()),
            history_log: RwLock::new(VecDeque::new()),
            log_sessions: tokio::sync::RwLock::new(HashMap::new()),
            python_echo_sessions: tokio::sync::RwLock::new(HashMap::new()),
            python_handler: tokio::sync::RwLock::new(None),
            python_handler_generation: std::sync::atomic::AtomicU64::new(0),
            onebot_sessions: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub fn insert_message_id(
        &self,
        self_id: String,
        user_id: String,
        group_id: String,
        message_id: String,
    ) -> Result<(), Box<dyn Error>> {
        // 用分隔符拼接，避免 "12"+"3" 与 "1"+"23" 这类无分隔拼接的键冲突
        let flag = format!("{self_id}|{group_id}");
        let mut message_ids = self.message_ids.write().unwrap_or_else(|e| e.into_inner());
        let messages = message_ids.entry(flag).or_default();
        messages.push_front((user_id, message_id));
        if messages.len() > 100 {
            messages.pop_back();
        }
        Ok(())
    }

    pub fn message_ids(
        &self,
        self_id: &str,
        group_id: &str,
    ) -> Result<VecDeque<(String, String)>, Box<dyn Error>> {
        let flag = format!("{self_id}|{group_id}");
        Ok(self
            .message_ids
            .read()
            .unwrap()
            .get(&flag)
            .cloned()
            .unwrap_or_default())
    }

    pub fn add_input_stream(&self, input_stream: InputStream) {
        self.input_streams.write().unwrap_or_else(|e| e.into_inner()).push(input_stream);
    }

    pub fn remove_input_stream(&self, echo: &str) {
        self.input_streams
            .write()
            .unwrap()
            .retain(|input_stream| input_stream.echo != echo);
    }

    pub fn send_group_input_streams(
        &self,
        self_id: &str,
        user_id: &str,
        group_id: &str,
        message: &str,
        message_id: &str,
    ) -> Result<(), Box<dyn Error>> {
        let input_streams = self.input_streams.read().unwrap_or_else(|e| e.into_inner());
        for input_stream in &*input_streams {
            if input_stream.stream_type == "输入流"
                && self_id == input_stream.self_id
                && user_id == input_stream.user_id
                && group_id == input_stream.group_id
            {
                if let Some(sender) = &input_stream.tx {
                    // 单个接收端关闭不应中断其余输入流的投递
                    let _ = sender.lock().unwrap_or_else(|e| e.into_inner()).send(message.to_owned());
                }
            } else if input_stream.stream_type != "输入流"
                && self_id == input_stream.self_id
                && group_id == input_stream.group_id
            {
                let value = serde_json::json!({
                    "发送者ID": user_id,
                    "消息": message,
                    "消息ID": message_id,
                });
                if let Some(sender) = &input_stream.tx {
                    let _ = sender.lock().unwrap_or_else(|e| e.into_inner()).send(value.to_string());
                }
            }
        }
        Ok(())
    }

    pub fn send_private_input_streams(
        &self,
        self_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<(), Box<dyn Error>> {
        let input_streams = self.input_streams.read().unwrap_or_else(|e| e.into_inner());
        for input_stream in &*input_streams {
            if input_stream.stream_type == "输入流"
                && self_id == input_stream.self_id
                && user_id == input_stream.user_id
                && input_stream.group_id.is_empty()
            {
                if let Some(sender) = &input_stream.tx {
                    // 单个接收端关闭不应中断其余输入流的投递
                    let _ = sender.lock().unwrap_or_else(|e| e.into_inner()).send(message.to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn record_sent_message(
        &self,
        pkg_name: &str,
        self_id: &str,
        source_message_id: &str,
        message_id: &str,
        now: u64,
    ) {
        let mut messages = self.script_related_messages.write().unwrap_or_else(|e| e.into_inner());
        if !source_message_id.is_empty() {
            let key = format!("{pkg_name}|{self_id}|{source_message_id}");
            messages
                .entry(key)
                .and_modify(|entry| entry.msg_id_vec.push(message_id.to_owned()))
                .or_insert_with(|| ScriptRelatMsg {
                    self_id: self_id.to_owned(),
                    msg_id_vec: vec![message_id.to_owned()],
                    create_time: now,
                });
        }
        messages.retain(|_, value| now.saturating_sub(value.create_time) <= 300);
    }

    pub fn related_message_ids(&self, key: &str) -> Vec<String> {
        self.script_related_messages
            .read()
            .unwrap()
            .get(key)
            .map(|value| value.msg_id_vec.clone())
            .unwrap_or_default()
    }

    pub fn add_related_message(
        &self,
        key: String,
        self_id: &str,
        message_id: String,
        now: u64,
    ) {
        let mut messages = self.script_related_messages.write().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = messages.get_mut(&key) {
            value.msg_id_vec.push(message_id);
        } else {
            messages.insert(
                key,
                ScriptRelatMsg {
                    self_id: self_id.to_owned(),
                    msg_id_vec: vec![message_id],
                    create_time: now,
                },
            );
        }
    }

    pub fn remove_package_related_messages(&self, pkg_name: &str) {
        let prefix = format!("{pkg_name}|");
        self.script_related_messages
            .write()
            .unwrap()
            .retain(|key, _| !key.starts_with(&prefix));
    }

    pub fn add_history_log(&self, message: &str) {
        let mut history = self.history_log.write().unwrap_or_else(|e| e.into_inner());
        history.push_back(message.to_owned());
        if history.len() > 50 {
            history.pop_front();
        }
    }

    pub fn history_log(&self) -> VecDeque<String> {
        self.history_log.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn add_log_session(
        &self,
        uid: String,
        sender: tokio::sync::mpsc::Sender<String>,
    ) {
        self.log_sessions.write().await.insert(uid, sender);
    }

    pub async fn remove_log_session(&self, uid: &str) {
        self.log_sessions.write().await.remove(uid);
    }

    pub async fn broadcast_log(&self, message: String) {
        // 用 try_send 避免单个卡死的 /watch_log 客户端(通道满)让每条日志
        // 都永久阻塞一个任务；通道已满或已断开的慢客户端直接剔除
        let mut to_remove: Vec<String> = Vec::new();
        {
            let sessions = self.log_sessions.read().await;
            for (uid, sender) in sessions.iter() {
                if sender.try_send(message.clone()).is_err() {
                    to_remove.push(uid.clone());
                }
            }
        }
        if !to_remove.is_empty() {
            let mut sessions = self.log_sessions.write().await;
            for uid in to_remove {
                sessions.remove(&uid);
            }
        }
    }

    pub async fn set_python_handler(&self, sender: tokio::sync::mpsc::Sender<String>) -> u64 {
        use std::sync::atomic::Ordering;
        let generation = self
            .python_handler_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        *self.python_handler.write().await = Some((generation, sender));
        generation
    }

    /// 仅当当前处理器仍是 `generation` 这一代时才清除。
    /// 旧连接退出（读/写任务结束）可能晚于新连接注册处理器，
    /// 无条件清除会把新连接刚注册的处理器一起抹掉，导致 Python 桥接中断。
    pub async fn clear_python_handler_if_current(&self, generation: u64) {
        let mut handler = self.python_handler.write().await;
        if handler.as_ref().map(|(g, _)| *g) == Some(generation) {
            *handler = None;
        }
    }

    pub async fn has_python_handler(&self) -> bool {
        self.python_handler.read().await.is_some()
    }

    /// 当前生效的 python 处理器代次；没有处理器时返回 None。
    pub async fn python_handler_generation(&self) -> Option<u64> {
        self.python_handler.read().await.as_ref().map(|(g, _)| *g)
    }

    pub async fn python_handler(
        &self,
    ) -> Option<tokio::sync::mpsc::Sender<String>> {
        self.python_handler
            .read()
            .await
            .as_ref()
            .map(|(_, sender)| sender.clone())
    }

    pub async fn add_python_echo(
        &self,
        uid: String,
        sender: tokio::sync::mpsc::Sender<serde_json::Value>,
    ) {
        self.python_echo_sessions.write().await.insert(uid, sender);
    }

    pub async fn remove_python_echo(&self, uid: &str) {
        self.python_echo_sessions.write().await.remove(uid);
    }

    pub async fn send_python_echo(
        &self,
        uid: &str,
        value: serde_json::Value,
    ) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let sender = self.python_echo_sessions.read().await.get(uid).cloned();
        if let Some(sender) = sender {
            sender.send(value).await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn add_onebot_session(
        &self,
        uid: String,
        sender: tokio::sync::mpsc::Sender<String>,
        platform: String,
        self_id: String,
    ) {
        self.onebot_sessions
            .write()
            .await
            .insert(uid, (sender, platform, self_id));
    }

    pub async fn remove_onebot_session(&self, uid: &str) {
        self.onebot_sessions.write().await.remove(uid);
    }

    pub async fn send_onebot_event(&self, message: String, platform: &str, self_id: &str) {
        let senders = self
            .onebot_sessions
            .read()
            .await
            .values()
            .filter(|(_, session_platform, session_self_id)| {
                session_platform == platform && session_self_id == self_id
            })
            .map(|(sender, _, _)| sender.clone())
            .collect::<Vec<_>>();
        for sender in senders {
            // 用 try_send 避免慢客户端（不读数据导致通道满）把事件任务无限挂起；
            // 通道满或已断开的会话直接剔除
            if sender.try_send(message.clone()).is_err() {
                self.remove_slow_onebot_sessions(&sender).await;
            }
        }
    }

    /// 定向把 API 响应发回发起请求的会话。
    ///
    /// 会话已断开时直接丢弃：广播给其它客户端会让它们收到无法识别的 echo，
    /// 既没有意义也可能造成信息串流。
    pub async fn send_onebot_api_ret(
        &self,
        target_uid: &str,
        message: String,
        platform: &str,
        self_id: &str,
    ) {
        let target = {
            let sessions = self.onebot_sessions.read().await;
            sessions
                .get(target_uid)
                .filter(|(_, p, s)| p == platform && s == self_id)
                .map(|(sender, _, _)| sender.clone())
        };
        let Some(sender) = target else {
            let _ = crate::cq_add_log_w(&format!(
                "onebot api 响应目标会话不存在，已丢弃：platform={platform}, self_id={self_id}"
            ));
            return;
        };
        if sender.try_send(message).is_err() {
            self.remove_slow_onebot_sessions(&sender).await;
        }
    }

    /// 按发送端剔除对应的会话条目
    async fn remove_slow_onebot_sessions(
        &self,
        stale_sender: &tokio::sync::mpsc::Sender<String>,
    ) {
        let mut to_remove: Vec<String> = Vec::new();
        {
            let sessions = self.onebot_sessions.read().await;
            for (uid, (sender, _, _)) in sessions.iter() {
                if sender.same_channel(stale_sender) {
                    to_remove.push(uid.clone());
                }
            }
        }
        if !to_remove.is_empty() {
            let mut sessions = self.onebot_sessions.write().await;
            for uid in to_remove {
                sessions.remove(&uid);
            }
        }
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionStore;

    #[test]
    fn message_ids_keep_the_latest_hundred_entries() {
        let store = SessionStore::new();
        for index in 0..101 {
            store
                .insert_message_id(
                    "bot".to_owned(),
                    "user".to_owned(),
                    "group".to_owned(),
                    index.to_string(),
                )
                .unwrap();
        }
        let messages = store.message_ids("bot", "group").unwrap();
        assert_eq!(messages.len(), 100);
        assert_eq!(messages.front().map(|(_, id)| id.as_str()), Some("100"));
        assert_eq!(messages.back().map(|(_, id)| id.as_str()), Some("1"));
    }

    #[test]
    fn history_log_is_bounded() {
        let store = SessionStore::new();
        for index in 0..51 {
            store.add_history_log(&index.to_string());
        }
        let history = store.history_log();
        assert_eq!(history.len(), 50);
        assert_eq!(history.front().map(String::as_str), Some("1"));
    }
}
