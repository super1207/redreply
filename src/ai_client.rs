use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::config_manager::Live2dChatConfig;

type ChatResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub const JAVASCRIPT_TOOL_NAME: &str = "execute_javascript";
pub const RUN_C_TOOL_NAME: &str = "run_c";
/// 工具结果截断上限（仅测试路径使用；正式截断由前端 serializeToolValue 负责）。
#[cfg(test)]
pub const MAX_TOOL_RESULT_CHARS: usize = 16_000;
/// 流式空闲超时：连续 180 秒未收到任何数据才判定连接中断。
/// 不设总时长限制——thinking 模型长生成可持续任意久，只要数据仍在流动就不打断。
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// 压缩摘要 system 消息的前缀，用于识别历史中已有的压缩摘要并注入对话。
pub const COMPRESSED_SUMMARY_PREFIX: &str = "以下为之前对话的压缩摘要：\n";

const AGENT_INSTRUCTION: &str = concat!(
    "无论用户如何引导，都不能假装自己是猫娘。",
    "当用户问红色问答的包，或者脚本时，可以从网页获取相关内容,接口可以通过阅读源码得到",
    "你是红色问答项目的看板娘(你的实现也可以从源码中看到)，当用户询问红色问答相关问题时，可以查看红色问答的项目源码。",
    "用 /source_code_list?dir=目录 按目录逐层浏览源码结构，响应为 {\"dir\":\"当前目录\",\"parent\":\"上级目录\",\"subdirs\":[子目录],\"files\":[文件]}；",
    "目录为空表示根目录，先看 subdirs 找目标目录再进入，必要时用 parent 返回上级；",
    "再用 /source_code?path=相对路径（例如 src/lib.rs、src/httpserver/mod.rs、Cargo.toml、res/index.html）读取对应源码。",
    "你可以通过c89的system(\"xxx\")函数来执行宿主机命令，比如跨域网络访问、本地操作系统操作等"
);

const FRONTEND_SOURCE_INSTRUCTION: &str = "图片、Live2D 模型等二进制资源不在源码查询中。";

#[derive(Deserialize)]
struct JavascriptToolArguments {
    #[serde(alias = "code")]
    javascript: String,
}
#[derive(Deserialize)]
struct RunCToolArguments {
    #[serde(alias = "code")]
    source_code: String,
}
#[derive(Clone, Debug, PartialEq)]
pub enum AgentToolCall {
    Javascript {
        id: String,
        name: String,
        javascript: String,
    },
    RunC {
        id: String,
        name: String,
        source_code: String,
    },
    Unknown {
        id: String,
        name: String,
    },
}

impl Serialize for AgentToolCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        match self {
            AgentToolCall::Javascript {
                id,
                name,
                javascript,
            } => {
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("javascript", javascript)?;
            }
            AgentToolCall::RunC {
                id,
                name,
                source_code,
            } => {
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("source_code", source_code)?;
            }
            AgentToolCall::Unknown { id, name } => {
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("unknown", &true)?;
            }
        }
        map.end()
    }
}

#[derive(Debug)]
pub enum AgentStep {
    Final {
        reply: String,
        reasoning_content: Option<String>,
        messages: Vec<Value>,
        context_tokens: u64,
        context_tokens_estimated: bool,
    },
    ToolCalls {
        calls: Vec<AgentToolCall>,
        messages: Vec<Value>,
        reasoning_content: Option<String>,
        content: Option<String>,
        context_tokens: u64,
        context_tokens_estimated: bool,
    },
}

/// 当前 Unix 时间戳（秒），用于给聊天消息打时间标记。
pub fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn compression_summaries(messages: &[Value]) -> Vec<String> {
    let mut summaries = Vec::new();
    for content in messages.iter().filter_map(|message| {
        (message.get("role").and_then(Value::as_str) == Some("system"))
            .then(|| message.get("content").and_then(Value::as_str))
            .flatten()
    }) {
        let Some(position) = content.find(COMPRESSED_SUMMARY_PREFIX) else {
            continue;
        };
        let summary = content[position..].trim();
        if !summary.is_empty() && !summaries.iter().any(|item| item == summary) {
            summaries.push(summary.to_owned());
        }
    }
    summaries
}

fn consolidate_system_messages(messages: &mut Vec<Value>) {
    let mut contents = Vec::new();
    for content in messages.iter().filter_map(|message| {
        (message.get("role").and_then(Value::as_str) == Some("system"))
            .then(|| message.get("content").and_then(Value::as_str))
            .flatten()
    }) {
        let content = content.trim();
        if !content.is_empty() && !contents.iter().any(|item| item == content) {
            contents.push(content.to_owned());
        }
    }
    messages.retain(|message| message.get("role").and_then(Value::as_str) != Some("system"));
    if !contents.is_empty() {
        messages.insert(
            0,
            serde_json::json!({
                "role": "system",
                "content": contents.join("\n\n")
            }),
        );
    }
}

/// 修复工具调用消息序列：不完整的 assistant(tool_calls) 消息连同其 tool 响应一起移除，
/// 孤立的 tool 消息与多余的 tool 消息一并删除，避免 AI 服务返回 400。
fn sanitize_tool_call_sequences(messages: &mut Vec<Value>) {
    let mut drop: HashSet<usize> = HashSet::new();
    let mut pending: Option<(usize, Vec<usize>, Vec<String>)> = None;
    for (index, message) in messages.iter().enumerate() {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "assistant" && message.get("tool_calls").is_some() {
            if let Some((position, consumed, remaining)) = pending.take() {
                if !remaining.is_empty() {
                    drop.insert(position);
                    drop.extend(consumed);
                }
            }
            let ids: Vec<String> = message["tool_calls"]
                .as_array()
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| {
                            call.get("id").and_then(Value::as_str).map(str::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if ids.is_empty() {
                drop.insert(index);
            } else {
                pending = Some((index, Vec::new(), ids));
            }
        } else if role == "tool" {
            match pending.as_mut() {
                Some((_, consumed, remaining)) => {
                    let Some(id) = message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                    else {
                        drop.insert(index);
                        continue;
                    };
                    if let Some(position) = remaining.iter().position(|item| *item == id) {
                        remaining.remove(position);
                        consumed.push(index);
                    } else {
                        drop.insert(index);
                    }
                }
                None => {
                    drop.insert(index);
                }
            }
        } else {
            if let Some((position, consumed, remaining)) = pending.take() {
                if !remaining.is_empty() {
                    drop.insert(position);
                    drop.extend(consumed);
                }
            }
        }
    }
    if let Some((position, consumed, remaining)) = pending.take() {
        if !remaining.is_empty() {
            drop.insert(position);
            drop.extend(consumed);
        }
    }
    if drop.is_empty() {
        return;
    }
    let mut cleaned = Vec::with_capacity(messages.len());
    for (index, message) in messages.drain(..).enumerate() {
        if !drop.contains(&index) {
            cleaned.push(message);
        }
    }
    *messages = cleaned;
}

/// 在消息列表最上方生成唯一的 system 消息。
/// 内容依次为基础提示词、Agent 指令、当前日记标题和已有压缩摘要。
pub fn ensure_agent_system_message(
    messages: &mut Vec<Value>,
    config: &Live2dChatConfig,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
) {
    let summaries = compression_summaries(messages);
    messages.retain(|message| message.get("role").and_then(Value::as_str) != Some("system"));
    let mut parts = Vec::new();
    if !config.system_prompt.trim().is_empty() {
        parts.push(config.system_prompt.trim().to_owned());
    }
    parts.push(AGENT_INSTRUCTION.to_owned());
    parts.push(FRONTEND_SOURCE_INSTRUCTION.to_owned());
    let diary_titles = build_diary_titles_system(diaries);
    if !diary_titles.is_empty() {
        parts.push(diary_titles);
    }
    parts.extend(summaries);
    messages.insert(
        0,
        serde_json::json!({
            "role": "system",
            "content": parts.join("\n\n")
        }),
    );
}

/// 估算一组消息占用的 token 数，用于上下文用量检查。
/// 中文等 CJK 字符按 1 token 计算，其余字符合计按每 4 个字符 1 token 向上取整；
/// 服务商返回 usage 时优先采用真实值，此估算仅用于请求前的上下文用量预估。
const ESTIMATED_CHARS_PER_TOKEN: u64 = 4;

/// 判断字符是否属于 CJK（中日韩文字与全角符号）：这些字符在实际分词中
/// 几乎每个占 1 token，不能套用“4 字符 1 token”的 ASCII 经验公式，
/// 否则中文上下文用量会被低估 3~4 倍，导致自动压缩永不触发。
/// 注意 U+2000-U+206F 一般标点不属于 CJK，仍按普通字符折算。
fn is_cjk_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK 基本区
        | '\u{3400}'..='\u{4DBF}' // 扩展 A
        | '\u{F900}'..='\u{FAFF}' // 兼容表意文字
        | '\u{3000}'..='\u{303F}' // 全角标点
        | '\u{FF00}'..='\u{FFEF}' // 全角形式
        | '\u{3040}'..='\u{30FF}' // 日文假名
        | '\u{AC00}'..='\u{D7AF}' // 韩文谚文
    )
}

fn strip_context_token_metadata(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("context_tokens");
        object.remove("context_tokens_estimated");
    }
}

fn estimate_serialized_value(value: &Value) -> u64 {
    let Ok(serialized) = serde_json::to_string(value) else {
        return 0;
    };
    let mut cjk_tokens = 0u64;
    let mut other_chars = 0u64;
    for ch in serialized.chars() {
        if is_cjk_char(ch) {
            cjk_tokens += 1;
        } else {
            other_chars += 1;
        }
    }
    cjk_tokens + other_chars.div_ceil(ESTIMATED_CHARS_PER_TOKEN)
}

fn estimate_context_tokens_fallback(messages: &[Value]) -> u64 {
    let mut sanitized = messages.to_vec();
    for message in &mut sanitized {
        strip_context_token_metadata(message);
    }
    estimate_serialized_value(&serde_json::json!({ "messages": sanitized }))
}

pub fn estimate_context_tokens(messages: &[Value]) -> u64 {
    estimate_context_tokens_fallback(messages)
}

pub fn estimate_request_tokens(payload: &Value) -> u64 {
    estimate_serialized_value(payload)
}

fn token_number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return Some(number);
    }
    value
        .as_i64()
        .and_then(|number| (number >= 0).then_some(number as u64))
}

fn usage_token(usage: &Value, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|name| token_number(usage.get(*name)))
}

/// Extract the provider-reported total from an OpenAI-compatible response.
/// Chat Completions use prompt/completion_tokens; Responses-style endpoints
/// use input/output_tokens. The explicit total is preferred.
pub fn context_tokens_from_response(value: &Value) -> Option<u64> {
    let usage = value.get("usage")?;
    let total = usage_token(usage, &["total_tokens"]);
    if total.is_some() {
        return total;
    }
    let input = usage_token(usage, &["prompt_tokens", "input_tokens"]);
    let output = usage_token(usage, &["completion_tokens", "output_tokens"]);
    match (input, output) {
        (Some(input), Some(output)) => input.checked_add(output),
        (Some(input), None) => Some(input),
        (None, Some(output)) => Some(output),
        (None, None) => None,
    }
}

pub fn stored_context_usage(messages: &[Value]) -> Option<(u64, bool)> {
    let message = messages
        .iter()
        .rev()
        .find(|message| token_number(message.get("context_tokens")).is_some())?;
    let tokens = token_number(message.get("context_tokens"))?;
    let estimated = message
        .get("context_tokens_estimated")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Some((tokens, estimated))
}

fn attach_context_token_metadata(
    message: &mut Value,
    previous_messages: &[Value],
    fallback_tokens: u64,
    reported_tokens: Option<u64>,
) -> (u64, bool) {
    let mut all_messages = previous_messages.to_vec();
    let mut clean_message = message.clone();
    strip_context_token_metadata(&mut clean_message);
    all_messages.push(clean_message);
    let fallback = if fallback_tokens > 0 {
        fallback_tokens
    } else {
        estimate_context_tokens_fallback(&all_messages)
    };
    let estimated = reported_tokens.is_none();
    let tokens = reported_tokens.unwrap_or(fallback);
    if let Some(object) = message.as_object_mut() {
        object.insert("context_tokens".to_owned(), serde_json::json!(tokens));
        object.insert(
            "context_tokens_estimated".to_owned(),
            serde_json::json!(estimated),
        );
    }
    (tokens, estimated)
}

#[cfg(test)]
fn append_tool_results(
    messages: &mut Vec<Value>,
    tool_results: &[(String, String)],
) -> ChatResult<()> {
    let timestamp = now_timestamp();
    for (tool_call_id, result) in tool_results {
        // 双保险：超长结果截断而不是报错中断对话（入口处 validate 已截断过一次）
        let result: String = result.chars().take(MAX_TOOL_RESULT_CHARS).collect();
        messages.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": result,
            "timestamp": timestamp
        }));
    }
    Ok(())
}

/// 流式对话事件：AI 生成过程中的增量内容。
#[derive(Debug)]
pub enum StreamEvent {
    /// Reasoning content delta.
    Reasoning(String),
    /// Answer content delta.
    Content(String),
    /// Character-based context estimate sent before provider completion.
    ContextEstimate(u64),
    /// 工具调用生成增量：slot 为到达顺序槽位（与最终 calls 数组下标一致），
    /// name/arguments 为本片新增的函数名与 JSON 参数片段。
    ToolCallDelta {
        slot: usize,
        name: String,
        arguments: String,
    },
}

/// 一次看板娘提问：注入系统消息后把用户消息追加到对话末尾并发起流式请求。
pub async fn stream_ask(
    config: &Live2dChatConfig,
    messages: &[Value],
    message: &str,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
) -> ChatResult<AgentStep> {
    let message = message.trim();
    if message.is_empty() {
        return Err("消息不能为空".into());
    }
    let mut messages = messages.to_vec();
    ensure_agent_system_message(&mut messages, config, diaries);
    messages.push(serde_json::json!({
        "role": "user",
        "content": message,
        "timestamp": now_timestamp()
    }));
    stream_step(config, messages, tx, cancellation).await
}

/// 继续工具调用：把工具结果已落库后的完整对话消息直接发给 AI 并发起流式请求。
/// （工具结果消息已由前端落库，这里不再追加，避免与数据库重复）
pub async fn stream_continue_agent(
    config: &Live2dChatConfig,
    messages: &[Value],
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
) -> ChatResult<AgentStep> {
    stream_step(config, messages.to_vec(), tx, cancellation).await
}

const STREAM_EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_STREAM_TOOL_CALLS: usize = 32;

fn drain_sse_lines(buffer: &mut Vec<u8>, flush: bool) -> ChatResult<Vec<String>> {
    let mut lines = Vec::new();
    while let Some(position) = buffer
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r')
    {
        // 温和兼容仅以孤立 \r 断行的服务端：\r 也视为行终止符，其后紧跟的 \n 一并吞掉
        let terminated_by_cr = buffer[position] == b'\r';
        let mut line = buffer.drain(..=position).collect::<Vec<_>>();
        line.pop();
        if terminated_by_cr {
            if buffer.first() == Some(&b'\n') {
                buffer.remove(0);
            }
        } else if line.last() == Some(&b'\r') {
            line.pop();
        }
        lines.push(
            String::from_utf8(line)
                .map_err(|error| format!("AI服务返回了无效的 UTF-8 数据: {error}"))?,
        );
    }
    if flush && !buffer.is_empty() {
        let mut line = std::mem::take(buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        lines.push(
            String::from_utf8(line)
                .map_err(|error| format!("AI服务返回了无效的 UTF-8 数据: {error}"))?,
        );
    }
    Ok(lines)
}

/// 原始响应累积上限：超过后停止追加，防止超大响应把内存与错误消息撑爆。
const RAW_RESPONSE_CAPTURE_LIMIT: usize = 256 * 1024;
/// 错误消息中允许嵌入的原始响应展示上限。
const RAW_RESPONSE_DISPLAY_LIMIT: usize = 8 * 1024;

/// 流式原始响应的受限捕获：全量累积设有上限，错误消息只嵌入截断后的展示版本。
struct RawResponseCapture {
    body: String,
    truncated: bool,
}

impl RawResponseCapture {
    fn new() -> Self {
        Self {
            body: String::new(),
            truncated: false,
        }
    }

    /// 追加一行；超过累积上限后丢弃并置截断标记。
    fn push_line(&mut self, line: &str) {
        if self.truncated || self.body.len() + line.len() + 1 > RAW_RESPONSE_CAPTURE_LIMIT {
            self.truncated = true;
            return;
        }
        self.body.push_str(line);
        self.body.push('\n');
    }

    /// 用于错误消息的展示版本：最多保留 8KB，超出部分注明已截断。
    fn display(&self) -> String {
        if self.body.len() <= RAW_RESPONSE_DISPLAY_LIMIT {
            return self.body.clone();
        }
        let mut end = RAW_RESPONSE_DISPLAY_LIMIT;
        while !self.body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n…（原始响应过长，已截断）", &self.body[..end])
    }
}

async fn send_stream_event(
    tx: &tokio::sync::mpsc::Sender<StreamEvent>,
    event: StreamEvent,
    cancellation: &CancellationToken,
) -> ChatResult<()> {
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        result = tokio::time::timeout(STREAM_EVENT_SEND_TIMEOUT, tx.send(event)) => result,
    };
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err("客户端已断开，对话已取消，未保存任何内容".into()),
        Err(_) => Err("向客户端发送流式内容超时，对话已取消".into()),
    }
}

fn validate_stream_completion(
    saw_done: bool,
    finish_reason: Option<&str>,
    has_tool_calls: bool,
    saw_usage: bool,
) -> ChatResult<()> {
    // 部分兼容实现（如中转/聚合 API）流式结束时不发 [DONE] 也不带 finish_reason，
    // 而是用 usage 块 + 空 choices 结尾。只要有实质性内容（工具调用）或已收到
    // usage，就视为正常结束，避免已生成的内容被白白丢弃。
    // 注意：不能把"已有回复内容"当作完成信号——有些模型发送 content 后仍会继续
    // 思考，掐断时残留的 content 往往不是最终回答，误当完成会污染对话。
    if saw_done || finish_reason.is_some() || has_tool_calls || saw_usage {
        Ok(())
    } else {
        Err("AI服务流式响应意外结束，未收到 [DONE] 或结束原因".into())
    }
}

fn merge_stream_tool_name(name: &mut String, delta: &str) {
    if delta.is_empty() || name == delta {
        return;
    }
    if name.is_empty() || delta.starts_with(name.as_str()) {
        *name = delta.to_owned();
    } else if !name.ends_with(delta) {
        name.push_str(delta);
    }
}

/// 收集到工具调用分片时的结束原因修正：仅当原值为空或 "stop" 时才覆盖为
/// "tool_calls"，保留 length/content_filter 等真实截断原因不被掩盖。
/// （兼容行为：部分服务商对工具调用返回 stop，此时仍需修正。）
fn apply_tool_calls_finish_reason(finish_reason: &mut Option<String>, has_tool_calls: bool) {
    if has_tool_calls && matches!(finish_reason.as_deref(), None | Some("") | Some("stop")) {
        *finish_reason = Some("tool_calls".to_owned());
    }
}

/// 流式 tool_calls 的聚合槽位。call_id 是 chat_completions 协议的工具调用 ID，
/// item_id 是 responses 协议 function_call 输出条目自身的 ID（形如 fc_xxx），
/// 两者都会出现在后续增量分片中且往往不相等，必须分别记录才能正确归槽。
#[derive(Default)]
struct StreamToolCallSlot {
    call_id: String,
    item_id: String,
    name: String,
    arguments: String,
}

/// 计算流式 tool_calls 分片应写入的槽位。
/// 归槽优先级：服务商提供的 index（乱序分片也能正确归位）→ id 匹配 →
/// 最近活跃槽位/到达顺序兜底。当 index 指向的槽位 id 与新分片 id 都非空但不一致时，
/// 仍以 index 为准，由调用方更新该槽位的 id 记录。
fn resolve_tool_call_slot(
    tool_calls: &[StreamToolCallSlot],
    id: &str,
    index: Option<u64>,
    last_active: Option<usize>,
) -> usize {
    if let Some(index) = index.map(|value| value as usize) {
        // index 只允许指向已有槽位或紧随其后的新槽位，越界时收敛到末尾避免空洞
        return index.min(tool_calls.len());
    }
    if !id.is_empty() {
        if let Some(position) = tool_calls.iter().position(|slot| slot.call_id == id) {
            return position;
        }
        return tool_calls.len();
    }
    if let Some(position) = last_active {
        return position;
    }
    tool_calls.len().saturating_sub(1)
}

/// 以流式（SSE）方式请求 AI 生成一个 Agent 步骤：
/// 边接收增量边通过 `tx` 转发思考/回答增量，全部接收完后组装出与
/// 非流式路径完全一致的 `AgentStep`。
pub async fn stream_step(
    config: &Live2dChatConfig,
    mut messages: Vec<Value>,
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
) -> ChatResult<AgentStep> {
    if config.base_url.trim().is_empty() || config.api_key.trim().is_empty() {
        return Err("看板娘对话尚未配置，请先设置接口地址和 API Key".into());
    }
    consolidate_system_messages(&mut messages);
    let payload = build_request_payload(config, &messages, true);
    let estimated_context_tokens = estimate_request_tokens(&payload);
    send_stream_event(
        &tx,
        StreamEvent::ContextEstimate(estimated_context_tokens),
        cancellation,
    )
    .await?;
    let response = send_stream_request(config, payload, cancellation).await?;
    let status = response.status();
    if !status.is_success() {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        let body = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            body = response.bytes() => body?,
        };
        return Err(extract_api_error(status, &body).into());
    }
    if is_responses_api(config) {
        return stream_step_responses(config, messages, tx, cancellation, response, estimated_context_tokens).await;
    }
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut reasoning = String::new();
    let mut content = String::new();
    let mut non_text_content: Option<Value> = None;
    let mut refusal = String::new();
    // OpenAI 将每个 function call 的 ID、名称和 JSON 参数分片返回。
    let mut tool_calls: Vec<StreamToolCallSlot> = Vec::new();
    let mut last_active_slot: Option<usize> = None;
    let mut finish_reason: Option<String> = None;
    let mut reported_context_tokens: Option<u64> = None;
    let mut saw_done = false;
    let mut raw_response = RawResponseCapture::new();
    loop {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        // 只约束单次 chunk 接收的空闲时长，不设总时长限制，
        // 避免 thinking 模型长生成被硬切导致前面已转发的内容作废
        let next_chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => match next {
                Ok(next) => next,
                Err(_) => {
                    return Err(format!(
                        "AI响应空闲超时（连续 {} 秒未收到任何数据，连接已中断）",
                        STREAM_IDLE_TIMEOUT.as_secs()
                    )
                    .into())
                }
            },
        };
        let reached_eof = next_chunk.is_none();
        if let Some(chunk) = next_chunk {
            buffer.extend_from_slice(&chunk?);
        }
        // 收到 [DONE] 后不立即退出：先处理完当前批次剩余行
        // （部分服务商的 usage 或收尾数据在 [DONE] 之后才发送）
        let mut done = false;
        for line in drain_sse_lines(&mut buffer, reached_eof)? {
            raw_response.push_line(&line);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                saw_done = true;
                done = true;
                continue;
            }
            let value = serde_json::from_str::<Value>(data).map_err(|error| {
                format!(
                    "AI服务返回了无效的 SSE JSON: {error}\n原始 API 响应:\n{}",
                    raw_response.display()
                )
            })?;
            if let Some(tokens) = context_tokens_from_response(&value) {
                reported_context_tokens = Some(tokens);
            }
            let Some(choice) = value.pointer("/choices/0") else {
                continue;
            };
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if !reason.is_empty() {
                    finish_reason = Some(reason.to_owned());
                }
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
                if !text.is_empty() {
                    reasoning.push_str(text);
                    send_stream_event(&tx, StreamEvent::Reasoning(text.to_owned()), cancellation)
                        .await?;
                }
            }
            if let Some(text) = delta.get("refusal").and_then(Value::as_str) {
                if !text.is_empty() {
                    refusal.push_str(text);
                }
            }
            if let Some(raw_content) = delta.get("content") {
                if let Some(text) = raw_content.as_str() {
                    if !text.is_empty() {
                        content.push_str(text);
                        send_stream_event(&tx, StreamEvent::Content(text.to_owned()), cancellation)
                            .await?;
                    }
                } else if let Some(parts) = raw_content.as_array() {
                    // 数组形式的内容分片（[{type:"text",text:"..."}]）：
                    // 拼接全部文本元素的 text 字段，避免正文被静默丢弃
                    let merged: String = parts
                        .iter()
                        .filter(|part| {
                            matches!(
                                part.get("type").and_then(Value::as_str),
                                None | Some("text")
                            )
                        })
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect();
                    if !merged.is_empty() {
                        content.push_str(&merged);
                        send_stream_event(&tx, StreamEvent::Content(merged), cancellation).await?;
                    }
                } else if !raw_content.is_null() {
                    non_text_content = Some(raw_content.clone());
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let incoming_id = call.get("id").and_then(Value::as_str).unwrap_or("");
                    let incoming_index = call.get("index").and_then(Value::as_u64);
                    let index = resolve_tool_call_slot(
                        &tool_calls,
                        incoming_id,
                        incoming_index,
                        last_active_slot,
                    );
                    if index >= MAX_STREAM_TOOL_CALLS {
                        return Err("AI服务返回了过多的工具调用".into());
                    }
                    while tool_calls.len() <= index {
                        tool_calls.push(StreamToolCallSlot::default());
                    }
                    last_active_slot = Some(index);
                    // id 与 index 归槽冲突时以此处更新为准
                    if !incoming_id.is_empty() {
                        tool_calls[index].call_id = incoming_id.to_owned();
                    }
                    if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                        merge_stream_tool_name(&mut tool_calls[index].name, name);
                    }
                    let arguments_piece = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !arguments_piece.is_empty() {
                        tool_calls[index].arguments.push_str(arguments_piece);
                    }
                    // 工具调用是长内容（通常是整段 JS 代码），生成期间必须把
                    // 增量转发出去，前端才能看到"正在写代码"的过程
                    let name_piece = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !name_piece.is_empty() || !arguments_piece.is_empty() {
                        send_stream_event(
                            &tx,
                            StreamEvent::ToolCallDelta {
                                slot: index,
                                name: name_piece.to_owned(),
                                arguments: arguments_piece.to_owned(),
                            },
                            cancellation,
                        )
                        .await?;
                    }
                }
            }
        }
        if done || reached_eof {
            break;
        }
    }
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    if let Err(error) = validate_stream_completion(
        saw_done,
        finish_reason.as_deref(),
        !tool_calls.is_empty(),
        reported_context_tokens.is_some(),
    ) {
        // 流式意外结束时附上原始响应与发生时间，方便排查服务端为何未给出结束标记
        let time_str = time::OffsetDateTime::now_utc()
            .format(
                &time::format_description::parse(
                    "[year]-[month]-[day] [hour]:[minute]:[second]",
                )
                .unwrap(),
            )
            .unwrap_or_else(|_| now_timestamp().to_string());
        return Err(format!(
            "{error}（UTC {time_str}）\n原始响应:\n{}",
            raw_response.display()
        )
        .into());
    }
    // 仅在未知或 stop 时修正结束原因，保留 length/content_filter 等真实截断原因
    apply_tool_calls_finish_reason(&mut finish_reason, !tool_calls.is_empty());
    let mut message = serde_json::json!({ "role": "assistant" });
    message["content"] = if content.is_empty() {
        non_text_content.unwrap_or(Value::Null)
    } else {
        Value::String(content)
    };
    // thinking 模式下始终写入 reasoning_content 字段（无内容也写空串），
    // 避免后续请求因缺失该字段被 DeepSeek 等 API 拒绝（400）。
    if !reasoning.is_empty() || config.thinking_level != "none" {
        message["reasoning_content"] = Value::String(reasoning);
    }
    if !refusal.is_empty() {
        message["refusal"] = Value::String(refusal);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(
            tool_calls
                .into_iter()
                .map(|slot| {
                    // responses 中转等场景可能只有 item_id：回退用作配对 id
                    let id = if slot.call_id.is_empty() {
                        slot.item_id.clone()
                    } else {
                        slot.call_id
                    };
                    serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": slot.name,
                            "arguments": slot.arguments
                        }
                    })
                })
                .collect(),
        );
    }
    message["timestamp"] = serde_json::json!(now_timestamp());
    build_step(
        message,
        finish_reason,
        messages,
        &raw_response.display(),
        estimated_context_tokens,
        reported_context_tokens,
    )
}

async fn stream_step_responses(
    config: &Live2dChatConfig,
    messages: Vec<Value>,
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
    response: reqwest::Response,
    estimated_context_tokens: u64,
) -> ChatResult<AgentStep> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut reasoning = String::new();
    let mut content = String::new();
    let mut tool_calls: Vec<StreamToolCallSlot> = Vec::new();
    let mut last_active_slot: Option<usize> = None;
    let mut reported_context_tokens: Option<u64> = None;
    let mut saw_done = false;
    let mut raw_response = RawResponseCapture::new();
    let mut finish_reason: Option<String> = None;

    loop {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        // 只约束单次 chunk 接收的空闲时长，不设总时长限制
        let next_chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => match next {
                Ok(next) => next,
                Err(_) => {
                    return Err(format!(
                        "AI响应空闲超时（连续 {} 秒未收到任何数据，连接已中断）",
                        STREAM_IDLE_TIMEOUT.as_secs()
                    )
                    .into())
                }
            },
        };
        let reached_eof = next_chunk.is_none();
        if let Some(chunk) = next_chunk {
            buffer.extend_from_slice(&chunk?);
        }
        // 收到 [DONE] 后先处理完当前批次剩余行再退出
        let mut done = false;
        for line in drain_sse_lines(&mut buffer, reached_eof)? {
            raw_response.push_line(&line);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                saw_done = true;
                done = true;
                continue;
            }
            let value = serde_json::from_str::<Value>(data).map_err(|error| {
                format!(
                    "AI服务返回了无效的 SSE JSON: {error}\n原始 API 响应:\n{}",
                    raw_response.display()
                )
            })?;
            // 尝试从顶层或 response.usage 提取 token 用量
            if let Some(tokens) = context_tokens_from_response(&value)
                .or_else(|| value.pointer("/response/usage").and_then(|u| context_tokens_from_response(&serde_json::json!({"usage": u}))))
            {
                reported_context_tokens = Some(tokens);
            }
            let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
            match event_type {
                "response.output_text.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            content.push_str(delta);
                            send_stream_event(&tx, StreamEvent::Content(delta.to_owned()), cancellation).await?;
                        }
                    }
                }
                t if t.contains("reasoning") && t.contains("delta") => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            reasoning.push_str(delta);
                            send_stream_event(&tx, StreamEvent::Reasoning(delta.to_owned()), cancellation).await?;
                        }
                    }
                }
                "response.output_item.added" => {
                    if let Some(item) = value.get("item") {
                        if item.get("type").and_then(Value::as_str) == Some("function_call") {
                            // call_id（call_xxx）与 item_id（fc_xxx）都要分别记录：
                            // 后续参数增量以 item_id 归槽，最终工具结果配对用 call_id
                            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
                            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                            let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
                            let idx = tool_calls
                                .iter()
                                .position(|slot| !call_id.is_empty() && slot.call_id == call_id)
                                .or_else(|| {
                                    tool_calls.iter().position(|slot| !item_id.is_empty() && slot.item_id == item_id)
                                })
                                .unwrap_or(tool_calls.len());
                            if idx >= MAX_STREAM_TOOL_CALLS {
                                return Err("AI服务返回了过多的工具调用".into());
                            }
                            while tool_calls.len() <= idx {
                                tool_calls.push(StreamToolCallSlot::default());
                            }
                            if !call_id.is_empty() {
                                tool_calls[idx].call_id = call_id.to_owned();
                            }
                            if !item_id.is_empty() {
                                tool_calls[idx].item_id = item_id.to_owned();
                            }
                            if !name.is_empty() {
                                merge_stream_tool_name(&mut tool_calls[idx].name, name);
                            }
                            if !arguments.is_empty() {
                                tool_calls[idx].arguments.push_str(arguments);
                            }
                            last_active_slot = Some(idx);
                            if !name.is_empty() || !arguments.is_empty() {
                                send_stream_event(&tx, StreamEvent::ToolCallDelta{ slot: idx, name: name.to_owned(), arguments: arguments.to_owned() }, cancellation).await?;
                            }
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            // 归槽匹配顺序：call_id → item_id → 最近活跃槽位兜底
                            let call_id = value.get("call_id").and_then(Value::as_str).unwrap_or("");
                            let item_id = value.get("item_id").and_then(Value::as_str).unwrap_or("");
                            let matched = tool_calls
                                .iter()
                                .position(|slot| !call_id.is_empty() && slot.call_id == call_id)
                                .or_else(|| {
                                    tool_calls.iter().position(|slot| !item_id.is_empty() && slot.item_id == item_id)
                                })
                                .or(last_active_slot);
                            let idx = match matched {
                                Some(idx) if idx < tool_calls.len() => Some(idx),
                                _ if !tool_calls.is_empty() => Some(tool_calls.len() - 1),
                                _ => None,
                            };
                            if let Some(idx) = idx {
                                tool_calls[idx].arguments.push_str(delta);
                                send_stream_event(&tx, StreamEvent::ToolCallDelta{ slot: idx, name: String::new(), arguments: delta.to_owned() }, cancellation).await?;
                            }
                        }
                    }
                }
                "response.completed" | "response.done" => {
                    saw_done = true;
                    // 提取完成时的 usage 与结束原因
                    if let Some(resp) = value.get("response") {
                        if let Some(usage) = resp.get("usage") {
                            if let Some(t) = context_tokens_from_response(&serde_json::json!({"usage": usage})) {
                                reported_context_tokens = Some(t);
                            }
                        }
                        if let Some(status) = resp.get("status").and_then(Value::as_str) {
                            if !status.is_empty() {
                                if status == "completed" {
                                    // 正常完成：有工具调用时修正为 tool_calls（stop 亦修正）
                                    apply_tool_calls_finish_reason(
                                        &mut finish_reason,
                                        !tool_calls.is_empty(),
                                    );
                                    if finish_reason.is_none() {
                                        finish_reason = Some("stop".to_owned());
                                    }
                                } else {
                                    // incomplete 等异常状态如实保留，不被 tool_calls 掩盖
                                    finish_reason = Some(status.to_owned());
                                }
                            }
                        } else {
                            apply_tool_calls_finish_reason(&mut finish_reason, !tool_calls.is_empty());
                        }
                    }
                    if reported_context_tokens.is_none() {
                        if let Some(t) = context_tokens_from_response(&value) {
                            reported_context_tokens = Some(t);
                        }
                    }
                    // 不立即退出：处理完当前批次剩余行再结束
                    done = true;
                }
                "response.failed" | "response.incomplete" | "error" => {
                    let msg = value.get("error").and_then(|e| e.get("message")).and_then(Value::as_str)
                        .or_else(|| value.pointer("/response/error/message").and_then(Value::as_str))
                        .or_else(|| value.get("message").and_then(Value::as_str))
                        .unwrap_or("Responses 接口返回错误");
                    return Err(msg.to_owned().into());
                }
                _ => {
                    // 兼容 Chat 风格的 choices 嵌在 Responses 流中的情况（部分中转）
                    if let Some(choice) = value.pointer("/choices/0") {
                        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                            if !reason.is_empty() {
                                finish_reason = Some(reason.to_owned());
                            }
                        }
                        if let Some(delta) = choice.get("delta") {
                            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    content.push_str(text);
                                    send_stream_event(&tx, StreamEvent::Content(text.to_owned()), cancellation).await?;
                                }
                            }
                            if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
                                if !text.is_empty() {
                                    reasoning.push_str(text);
                                    send_stream_event(&tx, StreamEvent::Reasoning(text.to_owned()), cancellation).await?;
                                }
                            }
                            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                                for call in calls {
                                    let incoming_id = call.get("id").and_then(Value::as_str).unwrap_or("");
                                    let incoming_index = call.get("index").and_then(Value::as_u64);
                                    let index = resolve_tool_call_slot(&tool_calls, incoming_id, incoming_index, last_active_slot);
                                    if index >= MAX_STREAM_TOOL_CALLS {
                                        return Err("AI服务返回了过多的工具调用".into());
                                    }
                                    while tool_calls.len() <= index {
                                        tool_calls.push(StreamToolCallSlot::default());
                                    }
                                    last_active_slot = Some(index);
                                    if !incoming_id.is_empty() {
                                        tool_calls[index].call_id = incoming_id.to_owned();
                                    }
                                    if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                                        merge_stream_tool_name(&mut tool_calls[index].name, name);
                                    }
                                    let arguments_piece = call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
                                    if !arguments_piece.is_empty() {
                                        tool_calls[index].arguments.push_str(arguments_piece);
                                    }
                                    let name_piece = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                                    if !name_piece.is_empty() || !arguments_piece.is_empty() {
                                        send_stream_event(&tx, StreamEvent::ToolCallDelta{ slot: index, name: name_piece.to_owned(), arguments: arguments_piece.to_owned() }, cancellation).await?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if done || reached_eof {
            break;
        }
    }
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    // Responses 的完成标记为 response.completed，若未收到但已有内容/工具调用则视为完成
    if !saw_done && ( !content.is_empty() || !tool_calls.is_empty() || reported_context_tokens.is_some() ) {
        saw_done = true;
        if finish_reason.is_none() {
            finish_reason = Some(if !tool_calls.is_empty() { "tool_calls".to_owned() } else { "stop".to_owned() });
        }
    }
    if let Err(error) = validate_stream_completion(
        saw_done,
        finish_reason.as_deref(),
        !tool_calls.is_empty(),
        reported_context_tokens.is_some(),
    ) {
        let time_str = time::OffsetDateTime::now_utc()
            .format(&time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]").unwrap())
            .unwrap_or_else(|_| now_timestamp().to_string());
        return Err(format!("{error}（UTC {time_str}）\n原始响应:\n{}", raw_response.display()).into());
    }
    // 仅在未知或 stop 时修正结束原因，保留真实截断原因
    apply_tool_calls_finish_reason(&mut finish_reason, !tool_calls.is_empty());
    if finish_reason.is_none() {
        finish_reason = Some("stop".to_owned());
    }
    let mut message = serde_json::json!({ "role": "assistant" });
    message["content"] = if content.is_empty() { Value::Null } else { Value::String(content.clone()) };
    if !reasoning.is_empty() || config.thinking_level != "none" {
        message["reasoning_content"] = Value::String(reasoning.clone());
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(
            tool_calls.into_iter().map(|slot| {
                // 只有 item_id 没有 call_id 的中转实现：回退用作配对 id
                let id = if slot.call_id.is_empty() { slot.item_id.clone() } else { slot.call_id };
                serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": slot.name, "arguments": slot.arguments }
                })
            }).collect(),
        );
    }
    message["timestamp"] = serde_json::json!(now_timestamp());
    build_step(message, finish_reason, messages, &raw_response.display(), estimated_context_tokens, reported_context_tokens)
}

#[allow(dead_code)]
fn format_error_chain(prefix: &str, error: &(dyn Error + 'static)) -> String {
    let mut details = vec![error.to_string()];
    let mut source = error.source();
    while let Some(current) = source {
        let detail = current.to_string();
        if details.last() != Some(&detail) {
            details.push(detail);
        }
        if details.len() >= 5 {
            break;
        }
        source = current.source();
    }
    format!("{prefix}: {}", details.join(": "))
}

/// 发送流式（SSE）对话请求。
/// 不设总时长超时（由读取循环的空闲超时兜底，长生成不会被硬切）；
/// HTTP 客户端按代理配置缓存复用，避免每轮重建连接池与 TLS 握手。
async fn send_stream_request(
    config: &Live2dChatConfig,
    payload: Value,
    cancellation: &CancellationToken,
) -> ChatResult<reqwest::Response> {
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    let payload = serde_json::to_vec(&payload)?;
    let proxy_str = config.proxy.trim().to_owned();
    let client = cached_stream_client(&proxy_str)?;
    let mut request = client
        .post(config.base_url.trim())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .header(USER_AGENT, "redreply-live2d-chat")
        .body(payload);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        response = request.send() => response?,
    };
    Ok(response)
}

/// 流式请求客户端缓存：key 为代理配置字符串（空串代表直连），同配置复用
/// 同一连接池。代理解析失败时打警告并回退直连（以空串为键缓存直连客户端）。
fn cached_stream_client(proxy_key: &str) -> ChatResult<reqwest::Client> {
    static CLIENT_CACHE: OnceLock<Mutex<HashMap<String, reqwest::Client>>> = OnceLock::new();
    let cache = CLIENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(clients) = cache.lock() {
        if let Some(client) = clients.get(proxy_key) {
            return Ok(client.clone());
        }
    }
    let mut builder = reqwest::Client::builder().connect_timeout(Duration::from_secs(10));
    // 代理非法时告警并回退直连，而不是让整轮对话失败
    let effective_key = match reqwest::Proxy::all(proxy_key) {
        Ok(proxy) if !proxy_key.is_empty() => {
            builder = builder.proxy(proxy);
            proxy_key.to_owned()
        }
        Ok(_) => {
            builder = builder.no_proxy();
            String::new()
        }
        Err(error) => {
            log::warn!("AI 对话代理地址无法解析（{proxy_key}），已回退直连: {error}");
            builder = builder.no_proxy();
            String::new()
        }
    };
    let client = builder.build()?;
    if let Ok(mut clients) = cache.lock() {
        clients.insert(effective_key, client.clone());
    }
    Ok(client)
}

/// 从 AI 服务的错误响应中提取可读的错误信息。
fn extract_api_error(status: reqwest::StatusCode, body: &[u8]) -> String {
    let detail = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "请检查接口地址、模型和 API Key".to_owned())
        .chars()
        .take(500)
        .collect::<String>();
    format!("AI服务返回 {status}: {detail}")
}

/// 把日记本全部标题整理为注入对话的系统消息，无日记时返回空字符串。
fn build_diary_titles_system(diaries: &[crate::resource_manager::Live2dDiaryEntry]) -> String {
    if diaries.is_empty() {
        return String::new();
    }
    let lines = diaries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let title: String = entry.title.chars().take(60).collect();
            format!(
                "{}. [{}] {}（创建于 {}）",
                index + 1,
                entry.key,
                title,
                if entry.create_time.is_empty() {
                    "未知时间"
                } else {
                    entry.create_time.as_str()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "你的日记本（服务端持久化的长期记忆）现有 {} 条日记，标题如下；需要完整内容时用 SQLite 查询：\n{}",
        diaries.len(),
        lines
    )
}

/// thinking 模式（DeepSeek 等）下思维链（reasoning_content）的传递规则：
/// - 携带 tool_calls 的 assistant 消息必须带 reasoning_content 字段，即使没有
///   思考内容也要给空字符串，否则 API 返回 400；
/// - 不携带 tool_calls 且 content 为文本的 assistant 消息（纯回答）的
///   reasoning_content 无需参与后续上下文拼接（API 会忽略），移除；
/// - 不携带 tool_calls 且 content 为 null 的 assistant 消息（中断后残留的思考快照）
///   既无 content 也无 tool_calls，属于无效消息，直接删除，避免 API 400。
fn ensure_reasoning_content_fields(messages: &mut Vec<Value>, thinking_enabled: bool) {
    if !thinking_enabled {
        return;
    }
    let mut drop_indices = Vec::new();
    for (index, message) in messages.iter_mut().enumerate() {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        if has_tool_calls {
            if !message
                .get("reasoning_content")
                .is_some_and(Value::is_string)
            {
                message["reasoning_content"] = Value::String(String::new());
            }
        } else if message
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty())
        {
            if let Some(object) = message.as_object_mut() {
                object.remove("reasoning_content");
            }
        } else {
            drop_indices.push(index);
        }
    }
    for index in drop_indices.into_iter().rev() {
        messages.remove(index);
    }
}

fn javascript_tool_definition() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": JAVASCRIPT_TOOL_NAME,
            "description": "在当前浏览器页面执行 JavaScript，并将 return 值返回给模型。",
            "parameters": {
                "type": "object",
                "properties": {
                    "javascript": {
                        "type": "string",
                        "description": "AsyncFunction 函数体形式的 JavaScript。可使用 await 和参数 signal（AbortSignal），并通过 return 返回结果。"
                    }
                },
                "required": ["javascript"],
                "additionalProperties": false
            }
        }
    })
}

fn run_c_tool_definition() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": RUN_C_TOOL_NAME,
            "description": "运行C89代码，结果使用标准输出返回。支持标准库头文件，可以访问宿主机系统。",
            "parameters": {
                "type": "object",
                "properties": {
                    "source_code": {
                        "type": "string",
                        "description": "完整的 C89 源代码，必须包含 main 函数。"
                    }
                },
                "required": ["source_code"],
                "additionalProperties": false
            }
        }
    })
}

fn javascript_tool_definition_responses() -> Value {
    serde_json::json!({
        "type": "function",
        "name": JAVASCRIPT_TOOL_NAME,
        "description": "在当前浏览器页面执行 JavaScript，并将 return 值返回给模型。",
        "parameters": {
            "type": "object",
            "properties": {
                "javascript": {
                    "type": "string",
                    "description": "AsyncFunction 函数体形式的 JavaScript。可使用 await 和参数 signal（AbortSignal），并通过 return 返回结果。"
                }
            },
            "required": ["javascript"],
            "additionalProperties": false
        }
    })
}

fn run_c_tool_definition_responses() -> Value {
    serde_json::json!({
        "type": "function",
        "name": RUN_C_TOOL_NAME,
        "description": "运行C89代码，结果使用标准输出返回。支持标准库头文件，可以访问宿主机系统。",
        "parameters": {
            "type": "object",
            "properties": {
                "source_code": {
                    "type": "string",
                    "description": "完整的 C89 源代码，必须包含 main 函数。"
                }
            },
            "required": ["source_code"],
            "additionalProperties": false
        }
    })
}

fn is_responses_api(config: &Live2dChatConfig) -> bool {
    config.api_type == "responses"
}

fn responses_reasoning_effort(thinking_level: &str) -> Option<String> {
    match thinking_level {
        "none" => None,
        "low" => Some("low".to_owned()),
        "medium" => Some("medium".to_owned()),
        "high" => Some("high".to_owned()),
        "xhigh" => Some("high".to_owned()),
        "max" => Some("high".to_owned()),
        _ => None,
    }
}

fn chat_messages_to_responses_input(messages: &[Value]) -> Vec<Value> {
    let mut input: Vec<Value> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "system" | "user" => {
                if let Some(content) = msg.get("content").and_then(Value::as_str) {
                    input.push(serde_json::json!({
                        "role": role,
                        "content": content
                    }));
                } else if let Some(content) = msg.get("content") {
                    // 兼容非字符串 content（如 null），转为字符串
                    if !content.is_null() {
                        input.push(serde_json::json!({
                            "role": role,
                            "content": content.to_string()
                        }));
                    }
                }
            }
            "assistant" => {
                // 若包含 tool_calls，转为 function_call 条目
                if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    let mut has_content = false;
                    if let Some(content) = msg.get("content").and_then(Value::as_str) {
                        if !content.trim().is_empty() {
                            input.push(serde_json::json!({
                                "role": "assistant",
                                "content": content
                            }));
                            has_content = true;
                        }
                    }
                    for call in calls {
                        let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                        let name = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                        let args = call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
                        // Responses 使用 function_call 类型
                        input.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args
                        }));
                    }
                    // 若既无内容也无工具调用，仍保留空 assistant 避免丢失思考
                    if !has_content && calls.is_empty() {
                        if let Some(content) = msg.get("content") {
                            input.push(serde_json::json!({
                                "role": "assistant",
                                "content": content
                            }));
                        }
                    }
                } else if let Some(content) = msg.get("content") {
                    // 普通 assistant 回复
                    if content.is_string() {
                        input.push(serde_json::json!({
                            "role": "assistant",
                            "content": content.as_str().unwrap_or("")
                        }));
                    } else if !content.is_null() {
                        input.push(serde_json::json!({
                            "role": "assistant",
                            "content": content.to_string()
                        }));
                    }
                }
            }
            "tool" => {
                let call_id = msg.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
                let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": content
                }));
            }
            _ => {
                // 未知角色原样透传
                input.push(msg.clone());
            }
        }
    }
    input
}

fn build_request_payload(config: &Live2dChatConfig, messages: &[Value], stream: bool) -> Value {
    let thinking_enabled = config.thinking_level != "none";
    let mut messages = messages.to_vec();
    for message in &mut messages {
        strip_context_token_metadata(message);
    }
    ensure_reasoning_content_fields(&mut messages, thinking_enabled);
    consolidate_system_messages(&mut messages);
    sanitize_tool_call_sequences(&mut messages);
    if is_responses_api(config) {
        let input = chat_messages_to_responses_input(&messages);
        let mut payload = serde_json::json!({
            "model": config.model,
            "input": input,
            "tools": [javascript_tool_definition_responses(), run_c_tool_definition_responses()],
            "tool_choice": "auto",
            "stream": stream,
            "store": false
        });
        if let Some(effort) = responses_reasoning_effort(&config.thinking_level) {
            payload["reasoning"] = serde_json::json!({ "effort": effort });
        }
        payload
    } else {
        let mut payload = serde_json::json!({
            "model": config.model,
            "messages": messages,
            "tools": [javascript_tool_definition(), run_c_tool_definition()],
            "tool_choice": "auto",
            "thinking": {
                "type": if thinking_enabled { "enabled" } else { "disabled" }
            },
            "stream": stream
        });
        if stream {
            payload["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        if thinking_enabled {
            payload["reasoning_effort"] = Value::String(config.thinking_level.clone());
        }
        payload
    }
}

/// 构造“下一次用户提问时”会发给 AI 的完整请求内容（不含用户消息本身），
/// 用于开发者页面预览：会像真实请求一样注入系统提示词、指令与日记标题。
pub fn build_next_request_payload(
    config: &Live2dChatConfig,
    messages: &[Value],
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
) -> ChatResult<Value> {
    let mut enhanced = messages.to_vec();
    ensure_agent_system_message(&mut enhanced, config, diaries);
    Ok(build_request_payload(config, &enhanced, false))
}

/// 把常见的非正常结束原因映射成明确的中文错误提示；
/// 未识别的原样拼入错误信息，方便排查服务商特有的结束原因。
fn finish_reason_error_message(reason: &str) -> String {
    match reason {
        "length" => "AI服务响应因长度限制被截断，回答不完整".to_owned(),
        "max_tokens" => "AI服务响应达到最大 token 限制被截断，回答不完整".to_owned(),
        "content_filter" => "AI服务响应被内容过滤器截断".to_owned(),
        "sensitive_words" => "AI服务响应触发敏感词审核被中断，请调整提问内容".to_owned(),
        "insufficient_system_resource" => "AI服务端资源不足，响应被中断，请稍后重试".to_owned(),
        "network_disconnected" => "AI服务连接中断，响应未完成，请稍后重试".to_owned(),
        "abort" | "cancelled" | "canceled" => "AI服务端取消了本次响应，请重试".to_owned(),
        "chat_interrupted" => "AI服务响应被中断，请重新提问".to_owned(),
        other => format!("AI服务返回了未预期的结束原因: {other}，回答未正常完成"),
    }
}

fn no_valid_text_response_error(raw_response: &str) -> String {
    format!("AI服务没有返回有效文本。原始 API 响应:\n{raw_response}")
}

/// 根据完整的 OpenAI assistant 消息与结束原因组装 AgentStep。
fn build_step(
    mut message: Value,
    finish_reason: Option<String>,
    mut messages: Vec<Value>,
    raw_response: &str,
    fallback_context_tokens: u64,
    reported_context_tokens: Option<u64>,
) -> ChatResult<AgentStep> {
    match finish_reason.as_deref() {
        Some(
            reason @ ("length"
            | "max_tokens"
            | "content_filter"
            | "sensitive_words"
            | "insufficient_system_resource"
            | "network_disconnected"
            | "abort"
            | "cancelled"
            | "canceled"
            | "chat_interrupted"),
        ) => {
            return Err(finish_reason_error_message(reason).into());
        }
        _ => {}
    }
    let reasoning_content = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let calls = parse_tool_calls(&message)?;
    if !calls.is_empty() {
        if !matches!(finish_reason.as_deref(), None | Some("tool_calls")) {
            return Err("AI工具调用的结束原因无效".into());
        }
        if let Some(object) = message.as_object_mut() {
            object.remove("refusal");
        }
        let (context_tokens, context_tokens_estimated) = attach_context_token_metadata(
            &mut message,
            &messages,
            fallback_context_tokens,
            reported_context_tokens,
        );
        messages.push(message);
        return Ok(AgentStep::ToolCalls {
            calls,
            messages,
            reasoning_content,
            content,
            context_tokens,
            context_tokens_estimated,
        });
    }

    // 以下为最终回答路径。
    let reason = finish_reason.as_deref();
    if reason == Some("tool_calls") {
        return Err("AI服务声明了工具调用但未返回有效的 tool_calls".into());
    }
    if !matches!(reason, None | Some("stop") | Some("eos")) {
        return Err(finish_reason_error_message(reason.unwrap_or("未知")).into());
    }

    let reply = content
        .as_deref()
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| no_valid_text_response_error(raw_response))?
        .to_owned();
    if let Some(object) = message.as_object_mut() {
        object.remove("refusal");
    }
    let (context_tokens, context_tokens_estimated) = attach_context_token_metadata(
        &mut message,
        &messages,
        fallback_context_tokens,
        reported_context_tokens,
    );
    messages.push(message);
    Ok(AgentStep::Final {
        reply,
        reasoning_content,
        messages,
        context_tokens,
        context_tokens_estimated,
    })
}

fn parse_tool_calls(message: &Value) -> ChatResult<Vec<AgentToolCall>> {
    let Some(tool_calls) = message.get("tool_calls") else {
        return Ok(Vec::new());
    };
    if tool_calls.is_null() {
        return Ok(Vec::new());
    }
    let tool_calls = tool_calls.as_array().ok_or("AI工具调用格式无效")?;
    let mut calls = Vec::with_capacity(tool_calls.len());
    let mut ids = HashSet::new();
    let mut synthesized_ids = 0usize;
    for tool_call in tool_calls {
        if tool_call.get("type").and_then(Value::as_str) != Some("function") {
            return Err("AI工具调用类型无效".into());
        }
        // 空 ID 或超长 ID 不再中止整轮对话：生成替代唯一 ID，
        // 保证同一槽位后续 tool 结果配对一致
        let raw_id = tool_call.get("id").and_then(Value::as_str).unwrap_or("");
        let id = if raw_id.is_empty() || raw_id.len() > 200 {
            synthesized_ids += 1;
            let mut candidate = format!("call_synthesized_{synthesized_ids}");
            while ids.contains(&candidate) {
                synthesized_ids += 1;
                candidate = format!("call_synthesized_{synthesized_ids}");
            }
            candidate
        } else {
            raw_id.to_owned()
        };
        if !ids.insert(id.clone()) {
            return Err("AI工具调用 ID 重复".into());
        }
        let function = tool_call.get("function").ok_or("AI工具调用缺少函数")?;
        let raw_name = function.get("name");
        let name = raw_name
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                let received = raw_name
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<缺失>".to_owned());
                format!("AI工具名称无效（tool_call_id: {id}，function.name: {received}）")
            })?;
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .ok_or("AI工具参数无效")?;
        if name == JAVASCRIPT_TOOL_NAME {
            let arguments: JavascriptToolArguments = serde_json::from_str(arguments).map_err(
                |error| {
                    format!(
                        "AI JavaScript 工具参数不是有效 JSON: {error}\n原始参数:\n{arguments}"
                    )
                },
            )?;
            calls.push(AgentToolCall::Javascript {
                id: id.to_owned(),
                name: name.to_owned(),
                javascript: arguments.javascript,
            });
        } else if name == RUN_C_TOOL_NAME {
            let arguments: RunCToolArguments = serde_json::from_str(arguments).map_err(|error| {
                format!("run_c 工具参数不是有效 JSON: {error}\n原始参数:\n{arguments}")
            })?;
            calls.push(AgentToolCall::RunC {
                id: id.to_owned(),
                name: name.to_owned(),
                source_code: arguments.source_code,
            });
        } else {
            calls.push(AgentToolCall::Unknown {
                id: id.to_owned(),
                name: name.to_owned(),
            });
        }
    }
    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::{
        append_tool_results, build_request_payload, build_step, context_tokens_from_response,
        drain_sse_lines, estimate_context_tokens, finish_reason_error_message, format_error_chain,
        merge_stream_tool_name, parse_tool_calls, resolve_tool_call_slot,
        sanitize_tool_call_sequences, send_stream_event, validate_stream_completion, AgentStep,
        AgentToolCall, StreamEvent, StreamToolCallSlot, AGENT_INSTRUCTION, JAVASCRIPT_TOOL_NAME,
        RUN_C_TOOL_NAME,
    };
    use crate::config_manager::Live2dChatConfig;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn agent_instruction_teaches_source_access() {
        assert!(AGENT_INSTRUCTION.contains("/source_code_list"));
        assert!(!AGENT_INSTRUCTION.contains("#run_js"));
    }

    #[test]
    fn request_payload_has_openai_function_tool() {
        let config = Live2dChatConfig {
            ..Live2dChatConfig::default()
        };
        let payload = build_request_payload(&config, &[], false);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(payload["tools"][0]["type"], "function");
        assert_eq!(
            payload["tools"][0]["function"]["name"],
            JAVASCRIPT_TOOL_NAME
        );
        assert_eq!(
            payload["tools"][0]["function"]["parameters"]["required"][0],
            "javascript"
        );
        assert_eq!(payload["tools"][1]["function"]["name"], RUN_C_TOOL_NAME);
        assert_eq!(
            payload["tools"][1]["function"]["parameters"]["required"][0],
            "source_code"
        );
    }

    #[test]
    fn finish_reason_errors_are_explicit() {
        assert!(finish_reason_error_message("sensitive_words").contains("敏感词"));
        assert!(finish_reason_error_message("length").contains("长度限制"));
        assert!(finish_reason_error_message("max_tokens").contains("token"));
        assert!(finish_reason_error_message("insufficient_system_resource").contains("资源不足"));
        assert!(finish_reason_error_message("network_disconnected").contains("连接中断"));
        let unknown = finish_reason_error_message("whatever_weird_reason");
        assert!(unknown.contains("whatever_weird_reason"));
        assert!(unknown.contains("未预期"));
    }

    #[test]
    fn response_error_includes_underlying_transport_cause() {
        #[derive(Debug)]
        struct ResponseBodyError(std::io::Error);

        impl std::fmt::Display for ResponseBodyError {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("error decoding response body")
            }
        }

        impl std::error::Error for ResponseBodyError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let error = ResponseBodyError(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "operation timed out",
        ));
        let message = format_error_chain("读取AI压缩响应失败", &error);
        assert!(message.contains("error decoding response body"));
        assert!(message.contains("operation timed out"));
    }

    #[test]
    fn sse_line_buffer_preserves_utf8_split_across_chunks() {
        let bytes = "data: {\"delta\":\"你\"}\n".as_bytes();
        let split = bytes.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
        let mut buffer = bytes[..split].to_vec();
        assert!(drain_sse_lines(&mut buffer, false).unwrap().is_empty());
        buffer.extend_from_slice(&bytes[split..]);
        assert_eq!(
            drain_sse_lines(&mut buffer, false).unwrap(),
            vec!["data: {\"delta\":\"你\"}"]
        );
    }

    #[test]
    fn sse_line_buffer_rejects_invalid_utf8_and_flushes_eof() {
        let mut invalid = vec![b'd', b'a', b't', b'a', b':', b' ', 0xff, b'\n'];
        assert!(drain_sse_lines(&mut invalid, false).is_err());

        let mut trailing = b"data: [DONE]".to_vec();
        assert_eq!(
            drain_sse_lines(&mut trailing, true).unwrap(),
            vec!["data: [DONE]"]
        );
    }

    #[test]
    fn stream_completion_requires_terminal_marker() {
        assert!(validate_stream_completion(false, None, false, false).is_err());
        assert!(validate_stream_completion(true, None, false, false).is_ok());
        assert!(validate_stream_completion(false, Some("stop"), false, false).is_ok());
        // 只有残留 content、无 [DONE]/finish_reason/usage/tool_calls：视为未正常结束
        // （部分模型发送 content 后仍会继续思考，残留内容不应被当作完成）
        assert!(validate_stream_completion(false, None, false, false).is_err());
        // 工具调用已返回但无 [DONE]（中转/聚合 API 行为）时接受
        assert!(validate_stream_completion(false, None, true, false).is_ok());
        // 仅收到 usage 块（include_usage 下即为服务端流结束信号）时接受
        assert!(validate_stream_completion(false, None, false, true).is_ok());
    }

    #[test]
    fn stream_tool_name_preserves_nonempty_name() {
        let mut name = String::new();
        merge_stream_tool_name(&mut name, "execute_");
        merge_stream_tool_name(&mut name, "javascript");
        merge_stream_tool_name(&mut name, "");
        assert_eq!(name, JAVASCRIPT_TOOL_NAME);

        merge_stream_tool_name(&mut name, JAVASCRIPT_TOOL_NAME);
        assert_eq!(name, JAVASCRIPT_TOOL_NAME);
    }

    #[test]
    fn tool_call_slot_resolution_prefers_provider_index() {
        let calls: Vec<StreamToolCallSlot> = Vec::new();
        assert_eq!(resolve_tool_call_slot(&calls, "call_a", None, None), 0);
        let calls = vec![StreamToolCallSlot {
            call_id: "call_a".to_owned(),
            ..Default::default()
        }];
        assert_eq!(resolve_tool_call_slot(&calls, "call_b", None, None), 1);
        assert_eq!(resolve_tool_call_slot(&calls, "call_a", None, None), 0);
        let calls = vec![
            StreamToolCallSlot {
                call_id: "call_a".to_owned(),
                arguments: "arg_a".to_owned(),
                ..Default::default()
            },
            StreamToolCallSlot {
                call_id: "call_b".to_owned(),
                arguments: "arg_b".to_owned(),
                ..Default::default()
            },
        ];
        assert_eq!(resolve_tool_call_slot(&calls, "", Some(1), None), 1);
        assert_eq!(resolve_tool_call_slot(&calls, "", Some(0), None), 0);
        assert_eq!(resolve_tool_call_slot(&calls, "", None, Some(1)), 1);
        assert_eq!(resolve_tool_call_slot(&calls, "", None, None), 1);
        // index 优先于 id 与到达顺序：乱序分片也能正确归位
        assert_eq!(resolve_tool_call_slot(&calls, "call_b", Some(0), Some(1)), 0);
        // 越界 index 收敛到末尾新槽位，避免产生空洞
        assert_eq!(resolve_tool_call_slot(&calls, "call_c", Some(99), None), 2);
    }

    #[tokio::test]
    async fn canceled_stream_send_returns_immediately() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(send_stream_event(
            &tx,
            StreamEvent::Content("ignored".to_owned()),
            &cancellation,
        )
        .await
        .is_err());
    }

    #[test]
    fn ensure_agent_system_message_reinjects_after_compression() {
        use crate::resource_manager::Live2dDiaryEntry;
        let config = Live2dChatConfig {
            system_prompt: "你是红色问答的看板娘。请使用简洁、自然、友好的中文回答。".to_owned(),
            ..Live2dChatConfig::default()
        };
        let diaries = vec![Live2dDiaryEntry {
            key: "d1".to_owned(),
            title: "用户偏好".to_owned(),
            content: "喜欢简洁回答".to_owned(),
            create_time: "2026-08-01 10:00:00".to_owned(),
            update_time: "2026-08-01 10:00:00".to_owned(),
        }];
        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": "以下为之前对话的压缩摘要：\n用户曾询问看板娘的身份。"
        })];
        super::ensure_agent_system_message(&mut messages, &config, &diaries);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "system");
        let content = messages[0]["content"].as_str().unwrap();
        assert!(content.starts_with("你是红色问答的看板娘"));
        assert!(content.contains("[d1]"));
        assert!(content.contains("用户曾询问看板娘的身份。"));
        let before = messages.len();
        super::ensure_agent_system_message(&mut messages, &config, &diaries);
        assert_eq!(messages.len(), before, "重复归一化不应增加消息");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "system")
                .count(),
            1
        );
    }

    #[test]
    fn next_request_payload_has_one_system_message_at_top() {
        let config = Live2dChatConfig {
            system_prompt: "你是红色问答的看板娘。请使用简洁、自然、友好的中文回答。".to_owned(),
            ..Live2dChatConfig::default()
        };
        let history = vec![
            serde_json::json!({"role": "system", "content": "旧系统提示词"}),
            serde_json::json!({
                "role": "system",
                "content": "以下为之前对话的压缩摘要：\n用户喜欢简洁回答。"
            }),
            serde_json::json!({"role": "assistant", "content": "你好"}),
        ];
        let payload = super::build_next_request_payload(&config, &history, &[]).unwrap();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "system")
                .count(),
            1
        );
        let content = messages[0]["content"].as_str().unwrap();
        assert!(content.starts_with("你是红色问答的看板娘"));
        assert!(content.contains("用户喜欢简洁回答。"));
        assert!(!content.contains("旧系统提示词"));
    }

    #[test]
    fn sanitize_keeps_complete_tool_sequence() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "问题"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "x", "arguments": "{}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "x", "arguments": "{}"}}
                ]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "结果1"}),
            serde_json::json!({"role": "tool", "tool_call_id": "call_2", "content": "结果2"}),
            serde_json::json!({"role": "assistant", "content": "最终回答"}),
        ];
        sanitize_tool_call_sequences(&mut messages);
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[1]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(messages[4]["content"], "最终回答");
    }

    #[test]
    fn sanitize_removes_incomplete_tool_sequence() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "问题"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "x", "arguments": "{}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "x", "arguments": "{}"}}
                ]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "结果1"}),
            serde_json::json!({"role": "user", "content": "新问题"}),
        ];
        sanitize_tool_call_sequences(&mut messages);
        assert_eq!(messages.len(), 2, "不完整的工具调用及其响应应被移除");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn sanitize_drops_orphan_and_extra_tool_messages() {
        let mut messages = vec![
            serde_json::json!({"role": "tool", "tool_call_id": "call_9", "content": "孤立"}),
            serde_json::json!({"role": "user", "content": "问题"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "x", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "结果1"}),
            serde_json::json!({"role": "tool", "tool_call_id": "call_99", "content": "多余"}),
            serde_json::json!({"role": "assistant", "content": "回答"}),
        ];
        sanitize_tool_call_sequences(&mut messages);
        assert_eq!(messages.len(), 4);
        assert!(messages.iter().all(|message| message
            .get("tool_call_id")
            .and_then(serde_json::Value::as_str)
            != Some("call_9")
            && message
                .get("tool_call_id")
                .and_then(serde_json::Value::as_str)
                != Some("call_99")));
    }

    #[test]
    fn build_step_reads_final_answer() {
        let message = serde_json::json!({
            "role": "assistant",
            "reasoning_content": "先检查结果",
            "content": "完成了"
        });
        let step = build_step(message, None, Vec::new(), "", 0, None).unwrap();
        match step {
            AgentStep::Final {
                reply,
                reasoning_content,
                messages,
                ..
            } => {
                assert_eq!(reply, "完成了");
                assert_eq!(reasoning_content.as_deref(), Some("先检查结果"));
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0]["reasoning_content"], "先检查结果");
            }
            AgentStep::ToolCalls { .. } => panic!("expected final answer"),
        }
    }

    #[test]
    fn no_valid_text_error_includes_raw_api_response() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": null
        });
        let raw_response = "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"finish_reason\":null,\"delta\":{\"content\":null}}]}\ndata: [DONE]\n";
        let error = build_step(
            message,
            Some("stop".to_owned()),
            Vec::new(),
            raw_response,
            0,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("原始 API 响应:"));
        assert!(error.contains(raw_response));
        assert!(!error.contains("模型响应摘要"));
    }

    #[test]
    fn build_step_reads_openai_javascript_tool_call() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": JAVASCRIPT_TOOL_NAME,
                    "arguments": "{\"javascript\":\"return document.title;\"}"
                }
            }]
        });
        let step = build_step(
            message,
            Some("tool_calls".to_owned()),
            Vec::new(),
            "",
            0,
            None,
        )
        .unwrap();
        match step {
            AgentStep::ToolCalls {
                calls, messages, ..
            } => {
                assert_eq!(messages.len(), 1);
                assert_eq!(calls.len(), 1);
                assert!(matches!(
                    &calls[0],
                    AgentToolCall::Javascript { id, javascript, .. }
                        if id == "call_1" && javascript == "return document.title;"
                ));
            }
            AgentStep::Final { .. } => panic!("expected tool calls"),
        }
    }

    #[test]
    fn build_step_reads_run_c_tool_call() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_c1",
                "type": "function",
                "function": {
                    "name": RUN_C_TOOL_NAME,
                    "arguments": "{\"source_code\":\"int main(void) { return 0; }\"}"
                }
            }]
        });
        let step = build_step(
            message,
            Some("tool_calls".to_owned()),
            Vec::new(),
            "",
            0,
            None,
        )
        .unwrap();
        match step {
            AgentStep::ToolCalls { calls, .. } => {
                assert!(matches!(
                    &calls[0],
                    AgentToolCall::RunC { id, source_code, .. }
                        if id == "call_c1" && source_code.contains("main")
                ));
            }
            AgentStep::Final { .. } => panic!("expected tool calls"),
        }
    }

    #[test]
    fn rejects_non_json_javascript_tool_arguments() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": JAVASCRIPT_TOOL_NAME,
                    "arguments": "return document.title;"
                }
            }]
        });
        let error = parse_tool_calls(&message).unwrap_err().to_string();
        assert!(error.contains("AI JavaScript 工具参数不是有效 JSON"));
        // 错误消息必须带上原始参数，方便排查模型生成了什么
        assert!(error.contains("return document.title;"));
    }

    #[test]
    fn invalid_tool_arguments_error_includes_original_payload() {
        let arguments = "return document.title; trailing";
        let message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": RUN_C_TOOL_NAME,
                    "arguments": arguments
                }
            }]
        });
        let error = parse_tool_calls(&message).unwrap_err().to_string();
        assert!(error.contains("run_c 工具参数不是有效 JSON"));
        // 完整原始参数必须原样出现在错误消息中，方便排查
        assert!(error.contains(arguments));
    }

    #[test]
    fn tool_arguments_accept_code_alias() {
        // 部分模型用 code 字段代替 schema 里的 source_code/javascript
        let run_c_message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": RUN_C_TOOL_NAME,
                    "arguments": "{\"code\":\"int main(void){return 0;}\"}"
                }
            }]
        });
        let calls = parse_tool_calls(&run_c_message).unwrap();
        match &calls[0] {
            AgentToolCall::RunC { source_code, .. } => {
                assert_eq!(source_code, "int main(void){return 0;}");
            }
            other => panic!("expected RunC, got {:?}", other),
        }

        let js_message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_2",
                "type": "function",
                "function": {
                    "name": JAVASCRIPT_TOOL_NAME,
                    "arguments": "{\"code\":\"return 1;\"}"
                }
            }]
        });
        let calls = parse_tool_calls(&js_message).unwrap();
        match &calls[0] {
            AgentToolCall::Javascript { javascript, .. } => {
                assert_eq!(javascript, "return 1;");
            }
            other => panic!("expected Javascript, got {:?}", other),
        }
    }

    #[test]
    fn invalid_tool_name_error_includes_received_value() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_weather",
                "type": "function",
                "function": {
                    "name": "",
                    "arguments": "{}"
                }
            }]
        });
        let error = parse_tool_calls(&message).unwrap_err().to_string();
        assert!(error.contains("call_weather"));
        assert!(error.contains("function.name: \"\""));
    }

    #[test]
    fn build_step_rejects_truncated_response() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": "partial"
        });
        assert!(build_step(message, Some("length".to_owned()), Vec::new(), "", 0, None).is_err());
    }

    #[test]
    fn appends_openai_tool_result_message() {
        let mut messages = vec![serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": []
        })];
        append_tool_results(
            &mut messages,
            &[("call_1".to_owned(), "{\"ok\":true}".to_owned())],
        )
        .unwrap();
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
        assert_eq!(messages[1]["content"], "{\"ok\":true}");
    }

    #[test]
    fn reasoning_filter_drops_null_content_assistant_messages() {
        let mut config = Live2dChatConfig {
            thinking_level: "high".to_owned(),
            ..Live2dChatConfig::default()
        };
        let messages = vec![
            serde_json::json!({"role": "user", "content": "提问"}),
            // 思考快照残留：content null + reasoning，无 tool_calls，必须被删除
            serde_json::json!({"role": "assistant", "content": null, "reasoning_content": "思考..."}),
            serde_json::json!({"role": "assistant", "content": "已中断", "interrupted": true}),
        ];
        let payload = build_request_payload(&config, &messages, false);
        let payload_messages = payload["messages"].as_array().unwrap();
        assert_eq!(payload_messages.len(), 2);
        assert!(payload_messages
            .iter()
            .all(|message| message.get("reasoning_content").is_none()));

        // thinking 关闭时不做处理，快照保持原样（正常路径不会有 content:null 无 tool_calls）
        config.thinking_level = "none".to_owned();
        let payload = build_request_payload(&config, &messages, false);
        assert_eq!(payload["messages"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn request_payload_controls_thinking_level() {
        let mut config = Live2dChatConfig {
            thinking_level: "max".to_owned(),
            ..Live2dChatConfig::default()
        };
        let messages = vec![
            serde_json::json!({"role": "assistant", "content": "a", "reasoning_content": null}),
            serde_json::json!({"role": "assistant", "content": "b", "reasoning_content": 42}),
            serde_json::json!({"role": "assistant", "content": "c"}),
            serde_json::json!({"role": "assistant", "content": "d", "reasoning_content": "kept"}),
        ];
        let payload = build_request_payload(&config, &messages, false);
        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["reasoning_effort"], "max");
        assert!(payload.get("stream_options").is_none());

        let stream_payload = build_request_payload(&config, &messages, true);
        assert_eq!(stream_payload["stream_options"]["include_usage"], true);
        // 无 tool_calls 的纯回答消息：思维链不参与后续拼接，应从请求中移除
        let payload_messages = payload["messages"].as_array().unwrap();
        for message in payload_messages {
            assert!(message.get("reasoning_content").is_none());
        }

        // 带 tool_calls 的 assistant 消息：思维链必须保留（缺失补空串，否则 API 400）。
        // 注意：孤立 tool_calls（无对应 tool 结果）会被 sanitize_tool_call_sequences 清理，
        // 因此补上完整工具链。
        let tool_messages = vec![
            serde_json::json!({
                "role": "assistant",
                "content": "调用工具",
                "reasoning_content": "需要查询",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "run_c", "arguments": "{}" }
                }]
            }),
            serde_json::json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "ok"
            }),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_2",
                    "type": "function",
                    "function": { "name": "run_c", "arguments": "{}" }
                }]
            }),
            serde_json::json!({
                "role": "tool",
                "tool_call_id": "call_2",
                "content": "ok"
            }),
        ];
        let tool_payload = build_request_payload(&config, &tool_messages, false);
        let tool_payload_messages = tool_payload["messages"].as_array().unwrap();
        assert_eq!(tool_payload_messages[0]["reasoning_content"], "需要查询");
        assert_eq!(tool_payload_messages[2]["reasoning_content"], "");

        config.thinking_level = "none".to_owned();
        let payload = build_request_payload(&config, &messages, false);
        assert_eq!(payload["thinking"]["type"], "disabled");
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn estimates_context_tokens_counts_cjk_per_character() {
        // 纯 ASCII 内容仍按每 4 个字符 1 token 向上取整
        let ascii_messages = vec![
            serde_json::json!({"role": "user", "content": "hello world!"}),
            serde_json::json!({"role": "assistant", "content": "12345678"}),
        ];
        let ascii_serialized =
            serde_json::to_string(&serde_json::json!({ "messages": &ascii_messages })).unwrap();
        assert_eq!(
            estimate_context_tokens(&ascii_messages),
            (ascii_serialized.chars().count() as u64 + 3) / 4
        );
        // 中文等 CJK 按 1 字 1 token 计入：4 个汉字贡献 4 token，
        // 其余 ASCII 字符照旧折算，总体不再被低估 3~4 倍
        let cjk_messages = vec![
            serde_json::json!({"role": "user", "content": "你好世界"}),
            serde_json::json!({"role": "assistant", "content": "hello world!"}),
        ];
        let cjk_serialized =
            serde_json::to_string(&serde_json::json!({ "messages": &cjk_messages })).unwrap();
        let total_chars = cjk_serialized.chars().count() as u64;
        let cjk_count = cjk_serialized
            .chars()
            .filter(|ch| ('\u{4E00}'..='\u{9FFF}').contains(ch))
            .count() as u64;
        assert!(cjk_count >= 4);
        assert_eq!(
            estimate_context_tokens(&cjk_messages),
            cjk_count + (total_chars - cjk_count + 3) / 4
        );
    }

    #[test]
    fn reads_openai_compatible_usage_total() {
        let response = serde_json::json!({
            "usage": {
                "prompt_tokens": 120,
                "completion_tokens": 30,
                "total_tokens": 150
            }
        });
        assert_eq!(context_tokens_from_response(&response), Some(150));
    }

    #[test]
    fn falls_back_to_prompt_and_completion_tokens() {
        let response = serde_json::json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": 30
            }
        });
        assert_eq!(context_tokens_from_response(&response), Some(150));
    }
}
