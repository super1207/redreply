//! 模型调用明细：进程内环形缓冲，配置页「调用明细」选项卡读取。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

/// 最多保留的调用记录数；超出后丢弃最旧记录。
const MAX_RECORDS: usize = 500;
const MAX_ERROR_CHARS: usize = 300;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CallRecord {
    pub time: i64,
    pub session: String,
    pub model: String,
    pub round: u32,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub latency_ms: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct CallStats {
    pub total_calls: u64,
    pub failed_calls: u64,
    pub total_prompt_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_completion_tokens: u64,
    pub cache_hit_rate: f64,
    pub avg_latency_ms: u64,
}

/// 最近一次触发中的单次模型调用详情。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct CallDetail {
    pub round: u32,
    pub time: i64,
    pub latency_ms: u64,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub success: bool,
    /// 实际发送给模型的完整请求体（messages/tools 等）。
    pub request: Value,
    pub response: String,
    /// 该轮模型的 reasoning_content。
    pub reasoning: String,
    pub tool_calls: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 最近一次触发（一个用户触发可能包含多轮工具调用）。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TurnDetail {
    pub id: u64,
    pub time: i64,
    pub session: String,
    pub model: String,
    /// 触发事件的原始 JSON。
    pub trigger: String,
    pub calls: Vec<CallDetail>,
    pub finished: bool,
    pub suppressed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn records() -> &'static Mutex<VecDeque<CallRecord>> {
    static RECORDS: OnceLock<Mutex<VecDeque<CallRecord>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn latest_turn_state() -> &'static Mutex<Option<TurnDetail>> {
    static LATEST: OnceLock<Mutex<Option<TurnDetail>>> = OnceLock::new();
    LATEST.get_or_init(|| Mutex::new(None))
}

fn next_turn_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn push(record: CallRecord) {
    let mut guard = records().lock().unwrap_or_else(|error| error.into_inner());
    guard.push_back(record);
    while guard.len() > MAX_RECORDS {
        guard.pop_front();
    }
}

/// 开始记录最近一次触发；返回本轮的 turn id。
pub(crate) fn begin_turn(session: &str, model: &str, trigger: &str) -> u64 {
    let id = next_turn_id();
    let mut guard = latest_turn_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *guard = Some(TurnDetail {
        id,
        time: now_secs(),
        session: session.to_owned(),
        model: model.to_owned(),
        trigger: trigger.to_owned(),
        calls: Vec::new(),
        finished: false,
        suppressed: false,
        reply: None,
        error: None,
    });
    id
}

/// 结束最近一次触发的记录；旧 turn 的完成回调会被忽略。
pub(crate) fn finish_turn(id: u64, reply: Option<&str>, error: Option<&str>, suppressed: bool) {
    let mut guard = latest_turn_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(turn) = guard.as_mut() {
        if turn.id == id {
            turn.finished = true;
            turn.suppressed = suppressed;
            turn.reply = reply.map(str::to_owned);
            turn.error = error.map(|text| text.chars().take(4_000).collect());
        }
    }
}

/// 返回最近一次触发的详情。
pub(crate) fn latest_turn() -> Option<TurnDetail> {
    latest_turn_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn record_turn_call(turn_id: Option<u64>, detail: CallDetail) {
    let Some(turn_id) = turn_id else {
        return;
    };
    let mut guard = latest_turn_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(turn) = guard.as_mut() {
        if turn.id == turn_id {
            turn.calls.push(detail);
        }
    }
}

/// 记录一次成功的模型调用。
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_success(
    session: &str,
    model: &str,
    round: u32,
    prompt_tokens: u64,
    cached_tokens: u64,
    completion_tokens: u64,
    latency: Duration,
    turn_id: Option<u64>,
    request: &Value,
    response: &str,
    reasoning: &str,
    tool_calls: Value,
) {
    let time = now_secs();
    let latency_ms = latency.as_millis().min(u64::MAX as u128) as u64;
    push(CallRecord {
        time,
        session: session.to_owned(),
        model: model.to_owned(),
        round,
        prompt_tokens,
        cached_tokens,
        completion_tokens,
        total_tokens: prompt_tokens.saturating_add(completion_tokens),
        latency_ms,
        success: true,
        error: None,
    });
    record_turn_call(
        turn_id,
        CallDetail {
            round,
            time,
            latency_ms,
            prompt_tokens,
            cached_tokens,
            completion_tokens,
            success: true,
            request: request.clone(),
            response: response.to_owned(),
            reasoning: reasoning.to_owned(),
            tool_calls,
            error: None,
        },
    );
}

/// 记录一次失败的模型调用。
pub(crate) fn record_failure(
    session: &str,
    model: &str,
    round: u32,
    latency: Duration,
    error: &str,
    turn_id: Option<u64>,
    request: &Value,
) {
    let time = now_secs();
    let latency_ms = latency.as_millis().min(u64::MAX as u128) as u64;
    let short_error: String = error.chars().take(MAX_ERROR_CHARS).collect();
    push(CallRecord {
        time,
        session: session.to_owned(),
        model: model.to_owned(),
        round,
        prompt_tokens: 0,
        cached_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        latency_ms,
        success: false,
        error: Some(short_error),
    });
    record_turn_call(
        turn_id,
        CallDetail {
            round,
            time,
            latency_ms,
            prompt_tokens: 0,
            cached_tokens: 0,
            completion_tokens: 0,
            success: false,
            request: request.clone(),
            response: String::new(),
            reasoning: String::new(),
            tool_calls: Value::Null,
            error: Some(error.chars().take(4_000).collect()),
        },
    );
}

/// 返回最近的记录（新记录在前）与汇总统计。
pub(crate) fn snapshot() -> (Vec<CallRecord>, CallStats) {
    let guard = records().lock().unwrap_or_else(|error| error.into_inner());
    let mut list: Vec<CallRecord> = guard.iter().cloned().collect();
    list.reverse();

    let mut stats = CallStats {
        total_calls: guard.len() as u64,
        ..CallStats::default()
    };
    let mut latency_sum = 0u64;
    for record in guard.iter() {
        if !record.success {
            stats.failed_calls = stats.failed_calls.saturating_add(1);
        }
        stats.total_prompt_tokens = stats
            .total_prompt_tokens
            .saturating_add(record.prompt_tokens);
        stats.total_cached_tokens = stats
            .total_cached_tokens
            .saturating_add(record.cached_tokens);
        stats.total_completion_tokens = stats
            .total_completion_tokens
            .saturating_add(record.completion_tokens);
        latency_sum = latency_sum.saturating_add(record.latency_ms);
    }
    stats.cache_hit_rate = if stats.total_prompt_tokens > 0 {
        stats.total_cached_tokens as f64 * 100.0 / stats.total_prompt_tokens as f64
    } else {
        0.0
    };
    stats.avg_latency_ms = if stats.total_calls > 0 {
        latency_sum / stats.total_calls
    } else {
        0
    };
    (list, stats)
}

pub(crate) const CAPACITY: usize = MAX_RECORDS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_capped_and_newest_first() {
        // 使用独立测试进程中的全局记录；记录数不会影响其它测试断言。
        for index in 0..(MAX_RECORDS + 3) {
            record_success(
                &format!("session-{index}"),
                "test-model",
                1,
                10,
                2,
                5,
                Duration::from_millis(3),
                None,
                &Value::Null,
                "",
                "",
                Value::Null,
            );
        }
        let (list, stats) = snapshot();
        assert_eq!(list.len(), MAX_RECORDS);
        assert!(stats.total_calls >= MAX_RECORDS as u64);
        // 并行测试可能同时写入记录，只断言最后写入的记录仍在窗口内
        assert!(list
            .iter()
            .any(|record| record.session == format!("session-{}", MAX_RECORDS + 2)));
    }

    #[test]
    fn latest_turn_records_each_call() {
        let id = begin_turn("session", "model", "{\"post_type\":\"message\"}");
        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        record_success(
            "session",
            "model",
            1,
            10,
            2,
            5,
            Duration::from_millis(7),
            Some(id),
            &request,
            "hello",
            "thinking",
            serde_json::json!([{"name": "run_command"}]),
        );
        record_failure(
            "session",
            "model",
            2,
            Duration::from_millis(5),
            "boom",
            Some(id),
            &request,
        );
        finish_turn(id, Some("hello"), None, false);

        let turn = latest_turn().expect("最近触发记录应存在");
        assert_eq!(turn.id, id);
        assert!(turn.finished);
        assert_eq!(turn.calls.len(), 2);
        assert_eq!(turn.calls[0].request, request);
        assert_eq!(turn.calls[0].response, "hello");
        assert_eq!(turn.calls[0].reasoning, "thinking");
        assert_eq!(turn.calls[1].success, false);
        assert_eq!(turn.calls[1].error.as_deref(), Some("boom"));
        assert_eq!(turn.reply.as_deref(), Some("hello"));
    }
}
