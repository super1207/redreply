//! 平台事件 → `/onebot` 反向接口订阅者。
//!
//! 转发由本插件自行订阅框架事件总线完成，不依赖 redlang 插件：
//! 脚本引擎停用时，外部 OneBot 客户端仍能收到平台事件。
//! MQTT 远端事件同样由 mqtt 插件发布到事件总线，因此多实例场景下也能转发。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use redlang::mytool::read_json_str;
use redreply_bot::Event;

/// 去重窗口：同一 `(self_id, message_id)` 在窗口内只转发一次，
/// 避免同一事件经本地适配器与 MQTT 双通道到达时重复推送。
const DEDUP_TTL: Duration = Duration::from_secs(300);

fn dedup_table() -> &'static Mutex<HashMap<String, Instant>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 返回 true 表示该事件已在去重窗口内转发过。
fn is_duplicate(event: &Event) -> bool {
    let message_id = read_json_str(&event.raw, "message_id");
    if message_id.is_empty() {
        return false;
    }
    let key = format!("{}|{message_id}", event.self_id);
    let now = Instant::now();
    let mut table = dedup_table()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // 惰性清理过期条目，防止表随时间无限增长
    table.retain(|_, seen_at| now.saturating_duration_since(*seen_at) < DEDUP_TTL);
    table.insert(key, now).is_some()
}

/// 与脚本管线一致：过旧的消息不再转发（协议端重放/本地补发时常见）。
fn is_stale(event: &Event) -> bool {
    let Some(time) = event.get("time").and_then(serde_json::Value::as_i64) else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    time < now - redlang::config_manager::skip_msg_time()
}

/// 把一条框架事件转发给对应的 OneBot 反向接口订阅者。
pub fn forward_event(event: &Event, runtime: &tokio::runtime::Handle) {
    if is_stale(event) || is_duplicate(event) {
        return;
    }
    let (root, platform, self_id) = match crate::convert::event_to_onebot(&event.raw) {
        Ok(value) => value,
        Err(error) => {
            let _ = redlang::cq_add_log_w(&format!("onebot event forward error:{error}"));
            return;
        }
    };
    let runtime = runtime.clone();
    runtime.spawn(async move {
        redlang::runtime_context()
            .session
            .send_onebot_event(root.to_string(), &platform, &self_id)
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(message_id: i64) -> Event {
        Event::from_json(json!({
            "platform": "onebot11",
            "self_id": 10001,
            "post_type": "message",
            "message_type": "group",
            "message_id": message_id,
            "time": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }))
        .unwrap()
    }

    #[test]
    fn same_message_is_forwarded_once() {
        let first = event(1);
        assert!(!is_duplicate(&first));
        assert!(is_duplicate(&first));
        assert!(!is_duplicate(&event(2)));
    }

    #[test]
    fn event_without_message_id_is_not_deduplicated() {
        let event = Event::from_json(json!({
            "platform": "onebot11",
            "self_id": 10001,
            "post_type": "meta_event"
        }))
        .unwrap();
        assert!(!is_duplicate(&event));
        assert!(!is_duplicate(&event));
    }

    #[test]
    fn old_message_is_stale() {
        let mut event = event(3);
        event.raw["time"] = json!(0);
        assert!(is_stale(&event));
    }
}
