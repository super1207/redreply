use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::RwLock;

use crate::{InputStream, ScriptRelatMsg};

type OnebotSession = (tokio::sync::mpsc::Sender<String>, String, String);

pub(crate) struct SessionStore {
    message_ids: RwLock<HashMap<String, VecDeque<(String, String)>>>,
    input_streams: RwLock<Vec<InputStream>>,
    script_related_messages: RwLock<HashMap<String, ScriptRelatMsg>>,
    history_log: RwLock<VecDeque<String>>,
    log_sessions: tokio::sync::RwLock<HashMap<String, tokio::sync::mpsc::Sender<String>>>,
    python_echo_sessions:
        tokio::sync::RwLock<HashMap<String, tokio::sync::mpsc::Sender<serde_json::Value>>>,
    python_handler: tokio::sync::RwLock<Option<tokio::sync::mpsc::Sender<String>>>,
    onebot_sessions: tokio::sync::RwLock<HashMap<String, OnebotSession>>,
}

impl SessionStore {
    pub(crate) fn new() -> Self {
        Self {
            message_ids: RwLock::new(HashMap::new()),
            input_streams: RwLock::new(Vec::new()),
            script_related_messages: RwLock::new(HashMap::new()),
            history_log: RwLock::new(VecDeque::new()),
            log_sessions: tokio::sync::RwLock::new(HashMap::new()),
            python_echo_sessions: tokio::sync::RwLock::new(HashMap::new()),
            python_handler: tokio::sync::RwLock::new(None),
            onebot_sessions: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub(crate) fn insert_message_id(
        &self,
        self_id: String,
        user_id: String,
        group_id: String,
        message_id: String,
    ) -> Result<(), Box<dyn Error>> {
        let flag = self_id + &group_id;
        let mut message_ids = self.message_ids.write().unwrap();
        let messages = message_ids.entry(flag).or_default();
        messages.push_front((user_id, message_id));
        if messages.len() > 100 {
            messages.pop_back();
        }
        Ok(())
    }

    pub(crate) fn message_ids(
        &self,
        self_id: &str,
        group_id: &str,
    ) -> Result<VecDeque<(String, String)>, Box<dyn Error>> {
        let flag = self_id.to_owned() + group_id;
        Ok(self
            .message_ids
            .read()
            .unwrap()
            .get(&flag)
            .cloned()
            .unwrap_or_default())
    }

    pub(crate) fn add_input_stream(&self, input_stream: InputStream) {
        self.input_streams.write().unwrap().push(input_stream);
    }

    pub(crate) fn remove_input_stream(&self, echo: &str) {
        self.input_streams
            .write()
            .unwrap()
            .retain(|input_stream| input_stream.echo != echo);
    }

    pub(crate) fn send_group_input_streams(
        &self,
        self_id: &str,
        user_id: &str,
        group_id: &str,
        message: &str,
        message_id: &str,
    ) -> Result<(), Box<dyn Error>> {
        let input_streams = self.input_streams.read().unwrap();
        for input_stream in &*input_streams {
            if input_stream.stream_type == "输入流"
                && self_id == input_stream.self_id
                && user_id == input_stream.user_id
                && group_id == input_stream.group_id
            {
                if let Some(sender) = &input_stream.tx {
                    sender.lock().unwrap().send(message.to_owned())?;
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
                    sender.lock().unwrap().send(value.to_string())?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn send_private_input_streams(
        &self,
        self_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<(), Box<dyn Error>> {
        let input_streams = self.input_streams.read().unwrap();
        for input_stream in &*input_streams {
            if input_stream.stream_type == "输入流"
                && self_id == input_stream.self_id
                && user_id == input_stream.user_id
                && input_stream.group_id.is_empty()
            {
                if let Some(sender) = &input_stream.tx {
                    sender.lock().unwrap().send(message.to_owned())?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn record_sent_message(
        &self,
        pkg_name: &str,
        self_id: &str,
        source_message_id: &str,
        message_id: &str,
        now: u64,
    ) {
        let mut messages = self.script_related_messages.write().unwrap();
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

    pub(crate) fn related_message_ids(&self, key: &str) -> Vec<String> {
        self.script_related_messages
            .read()
            .unwrap()
            .get(key)
            .map(|value| value.msg_id_vec.clone())
            .unwrap_or_default()
    }

    pub(crate) fn add_related_message(
        &self,
        key: String,
        self_id: &str,
        message_id: String,
        now: u64,
    ) {
        let mut messages = self.script_related_messages.write().unwrap();
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

    pub(crate) fn remove_package_related_messages(&self, pkg_name: &str) {
        let prefix = format!("{pkg_name}|");
        self.script_related_messages
            .write()
            .unwrap()
            .retain(|key, _| !key.starts_with(&prefix));
    }

    pub(crate) fn add_history_log(&self, message: &str) {
        let mut history = self.history_log.write().unwrap();
        history.push_back(message.to_owned());
        if history.len() > 50 {
            history.pop_front();
        }
    }

    pub(crate) fn history_log(&self) -> VecDeque<String> {
        self.history_log.read().unwrap().clone()
    }

    pub(crate) async fn add_log_session(
        &self,
        uid: String,
        sender: tokio::sync::mpsc::Sender<String>,
    ) {
        self.log_sessions.write().await.insert(uid, sender);
    }

    pub(crate) async fn remove_log_session(&self, uid: &str) {
        self.log_sessions.write().await.remove(uid);
    }

    pub(crate) async fn broadcast_log(&self, message: String) {
        let senders = self
            .log_sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for sender in senders {
            let _ = sender.send(message.clone()).await;
        }
    }

    pub(crate) async fn set_python_handler(&self, sender: tokio::sync::mpsc::Sender<String>) {
        *self.python_handler.write().await = Some(sender);
    }

    pub(crate) async fn clear_python_handler(&self) {
        *self.python_handler.write().await = None;
    }

    pub(crate) async fn has_python_handler(&self) -> bool {
        self.python_handler.read().await.is_some()
    }

    pub(crate) async fn python_handler(&self) -> Option<tokio::sync::mpsc::Sender<String>> {
        self.python_handler.read().await.clone()
    }

    pub(crate) async fn add_python_echo(
        &self,
        uid: String,
        sender: tokio::sync::mpsc::Sender<serde_json::Value>,
    ) {
        self.python_echo_sessions.write().await.insert(uid, sender);
    }

    pub(crate) async fn remove_python_echo(&self, uid: &str) {
        self.python_echo_sessions.write().await.remove(uid);
    }

    pub(crate) async fn send_python_echo(
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

    pub(crate) async fn add_onebot_session(
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

    pub(crate) async fn remove_onebot_session(&self, uid: &str) {
        self.onebot_sessions.write().await.remove(uid);
    }

    pub(crate) async fn send_onebot_event(&self, message: String, platform: &str, self_id: &str) {
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
            let _ = sender.send(message.clone()).await;
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
