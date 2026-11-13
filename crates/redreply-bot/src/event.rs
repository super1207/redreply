use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 框架统一事件。
///
/// 线格式保持 OneBot 风格 JSON（与旧版 `cqevent::do_1207_event` 输入一致），
/// 便于适配器、MQTT、反向 WS 等继续共用同一套协议。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub platform: String,
    pub self_id: String,
    pub post_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
    pub raw: Value,
}

impl Event {
    /// 从 OneBot 风格 JSON 构造事件。
    pub fn from_json(raw: Value) -> std::result::Result<Self, String> {
        if !raw.is_object() {
            return Err("事件必须是 JSON 对象".to_string());
        }
        let platform = raw
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let self_id = raw.get("self_id").map(value_to_string).unwrap_or_default();
        let post_type = raw
            .get("post_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let message_type = raw
            .get("message_type")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Self {
            platform,
            self_id,
            post_type,
            message_type,
            raw,
        })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.raw.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.raw.get(key).and_then(Value::as_str)
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// 事件处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAction {
    /// 继续把事件交给后续订阅者。
    Continue,
    /// 消费该事件，不再向后传递（对应旧版过滤器返回「真」）。
    Stop,
}

/// 事件过滤器。
#[derive(Clone, Default)]
pub struct EventFilter {
    platforms: Option<Vec<String>>,
    post_types: Option<Vec<String>>,
    message_types: Option<Vec<String>>,
    custom: Option<Arc<dyn Fn(&Event) -> bool + Send + Sync>>,
}

impl EventFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.platforms.get_or_insert_with(Vec::new).push(platform.into());
        self
    }

    pub fn post_type(mut self, post_type: impl Into<String>) -> Self {
        self.post_types
            .get_or_insert_with(Vec::new)
            .push(post_type.into());
        self
    }

    pub fn message_type(mut self, message_type: impl Into<String>) -> Self {
        self.message_types
            .get_or_insert_with(Vec::new)
            .push(message_type.into());
        self
    }

    pub fn custom<F>(mut self, filter: F) -> Self
    where
        F: Fn(&Event) -> bool + Send + Sync + 'static,
    {
        self.custom = Some(Arc::new(filter));
        self
    }

    pub fn matches(&self, event: &Event) -> bool {
        if let Some(platforms) = &self.platforms {
            if !platforms.iter().any(|platform| platform == &event.platform) {
                return false;
            }
        }
        if let Some(post_types) = &self.post_types {
            if !post_types.iter().any(|post_type| post_type == &event.post_type) {
                return false;
            }
        }
        if let Some(message_types) = &self.message_types {
            let Some(message_type) = &event.message_type else {
                return false;
            };
            if !message_types.iter().any(|item| item == message_type) {
                return false;
            }
        }
        if let Some(custom) = &self.custom {
            if !custom(event) {
                return false;
            }
        }
        true
    }
}

type EventHandler = Arc<dyn Fn(&Event) -> EventAction + Send + Sync + 'static>;

struct Subscriber {
    id: u64,
    owner: String,
    priority: i32,
    filter: EventFilter,
    handler: EventHandler,
}

/// 事件订阅句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SubscriptionId(pub u64);

/// 一次事件分发的统计。
#[derive(Clone, Debug, Serialize)]
pub struct PublishReport {
    /// 实际执行的处理函数数量。
    pub delivered: usize,
    /// 消费掉该事件的订阅者 owner。
    pub stopped_by: Option<String>,
}

/// 事件总线。
///
/// 订阅者按 priority 从高到低执行；返回 [`EventAction::Stop`] 的订阅者
/// 会终止后续分发。适配器收到平台事件后调用 [`EventBus::publish`]。
#[derive(Default)]
pub struct EventBus {
    subscribers: RwLock<Vec<Subscriber>>,
    next_id: AtomicU64,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 订阅事件；`owner` 用于插件停用时回收（框架/主程序可传固定名字）。
    pub fn subscribe<F>(
        &self,
        owner: &str,
        priority: i32,
        filter: EventFilter,
        handler: F,
    ) -> SubscriptionId
    where
        F: Fn(&Event) -> EventAction + Send + Sync + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let mut subscribers = self.subscribers.write().unwrap_or_else(|e| e.into_inner());
        subscribers.push(Subscriber {
            id,
            owner: owner.to_string(),
            priority,
            filter,
            handler: Arc::new(handler),
        });
        SubscriptionId(id)
    }

    pub fn unsubscribe(&self, id: SubscriptionId) -> bool {
        let mut subscribers = self.subscribers.write().unwrap_or_else(|e| e.into_inner());
        let before = subscribers.len();
        subscribers.retain(|subscriber| subscriber.id != id.0);
        subscribers.len() != before
    }

    /// 移除某个 owner（插件）的全部订阅，返回移除数量。
    pub(crate) fn unsubscribe_owner(&self, owner: &str) -> usize {
        let mut subscribers = self.subscribers.write().unwrap_or_else(|e| e.into_inner());
        let before = subscribers.len();
        subscribers.retain(|subscriber| subscriber.owner != owner);
        before - subscribers.len()
    }

    /// 发布事件，按优先级依次执行订阅者。
    pub fn publish(&self, event: &Event) -> PublishReport {
        let mut matched: Vec<(i32, u64, String, EventHandler)> = {
            let subscribers = self.subscribers.read().unwrap_or_else(|e| e.into_inner());
            subscribers
                .iter()
                .filter(|subscriber| subscriber.filter.matches(event))
                .map(|subscriber| {
                    (
                        subscriber.priority,
                        subscriber.id,
                        subscriber.owner.clone(),
                        subscriber.handler.clone(),
                    )
                })
                .collect()
        };
        matched.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        let mut delivered = 0;
        let mut stopped_by = None;
        for (_, _, owner, handler) in matched {
            delivered += 1;
            if handler(event) == EventAction::Stop {
                stopped_by = Some(owner);
                break;
            }
        }
        PublishReport {
            delivered,
            stopped_by,
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn event() -> Event {
        Event::from_json(serde_json::json!({
            "platform": "onebot11",
            "self_id": "10001",
            "post_type": "message",
            "message_type": "group"
        }))
        .unwrap()
    }

    #[test]
    fn priority_and_stop() {
        let bus = EventBus::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let low_order = order.clone();
        bus.subscribe("low", 0, EventFilter::new(), move |_| {
            low_order.lock().unwrap_or_else(|e| e.into_inner()).push("low");
            EventAction::Continue
        });
        let high_order = order.clone();
        bus.subscribe("high", 10, EventFilter::new(), move |_| {
            high_order.lock().unwrap_or_else(|e| e.into_inner()).push("high");
            EventAction::Continue
        });
        let report = bus.publish(&event());
        assert_eq!(report.delivered, 2);
        assert_eq!(*order.lock().unwrap_or_else(|e| e.into_inner()), vec!["high", "low"]);
    }

    #[test]
    fn stop_prevents_later_subscribers() {
        let bus = EventBus::new();
        let called = Arc::new(AtomicUsize::new(0));
        let stopper_called = called.clone();
        bus.subscribe("stopper", 10, EventFilter::new(), move |_| {
            stopper_called.fetch_add(1, Ordering::SeqCst);
            EventAction::Stop
        });
        let later_called = called.clone();
        bus.subscribe("later", 0, EventFilter::new(), move |_| {
            later_called.fetch_add(1, Ordering::SeqCst);
            EventAction::Continue
        });
        let report = bus.publish(&event());
        assert_eq!(report.delivered, 1);
        assert_eq!(report.stopped_by.as_deref(), Some("stopper"));
        assert_eq!(called.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn filter_matches_fields() {
        let bus = EventBus::new();
        let called = Arc::new(AtomicUsize::new(0));
        let handler_called = called.clone();
        bus.subscribe(
            "filtered",
            0,
            EventFilter::new().platform("telegram").post_type("message"),
            move |_| {
                handler_called.fetch_add(1, Ordering::SeqCst);
                EventAction::Continue
            },
        );
        bus.publish(&event());
        assert_eq!(called.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unsubscribe_by_owner() {
        let bus = EventBus::new();
        bus.subscribe("plugin-a", 0, EventFilter::new(), |_| EventAction::Continue);
        bus.subscribe("plugin-b", 0, EventFilter::new(), |_| EventAction::Continue);
        assert_eq!(bus.unsubscribe_owner("plugin-a"), 1);
        assert_eq!(bus.subscriber_count(), 1);
    }
}
