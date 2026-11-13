mod calls;
mod config;
mod session;
mod tokenizer;

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hyper::body::Incoming;
use hyper::{Method, Request, StatusCode};
use redreply_bot::http::{collect_body, json_response};
use redreply_bot::{
    Event, EventAction, EventFilter, HttpError, HttpResponse, Plugin, PluginContext, PluginMeta,
    Result, RouteAccess, RouteContext,
};
use serde_json::{json, Value};

use config::ChatConfig;

const LOG_TAG: &str = "chatgpt";
const SUMMARY_PREFIX: &str = "【历史对话摘要】\n";
const SUMMARY_INSTRUCTION: &str = "请把以下对话压缩成一段简洁的中文摘要，保留关键事实（人名、称呼、用户偏好、已确认的事项、待办），不要脑补原文没有的信息，直接输出摘要正文，不要加多余的寒暄，摘要不超过 3000 字。";
const CONFIG_CACHE_TTL: Duration = Duration::from_secs(5);
const COMMAND_TOOL_NAME: &str = "run_command";
/// 调用后本轮不向聊天输出任何文本的工具
const NO_REPLY_TOOL_NAME: &str = "no_reply";
const BOT_API_PATH: &str = "/chatgpt/api/call";

/// 插件注册时保存的框架上下文：让插件日志同时进入框架管理台「日志」页。
static PLUGIN_CTX: std::sync::OnceLock<PluginContext> = std::sync::OnceLock::new();

fn log_info(message: impl Into<String>) {
    let message = message.into();
    if let Some(ctx) = PLUGIN_CTX.get() {
        ctx.logger().info(LOG_TAG, message.clone());
    }
    let _ = redlang::cqapi::cq_add_log(&message);
}

fn log_warn(message: impl Into<String>) {
    let message = message.into();
    if let Some(ctx) = PLUGIN_CTX.get() {
        ctx.logger().warn(LOG_TAG, message.clone());
    }
    let _ = redlang::cq_add_log_w(&message);
}

#[derive(Clone, Debug)]
struct ChatMsg {
    role: &'static str,
    content: String,
}

/// 流式响应里按 index 分片的工具调用增量
#[derive(Clone, Debug, Default)]
struct ToolCallDelta {
    index: u64,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

/// 拼接完成的工具调用
#[derive(Clone, Debug)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// 本轮对话的上下文：原始事件 JSON。
#[derive(Clone, Default)]
struct TurnContext {
    event_json: String,
}

/// 工具调用增量累加器（同 index 的 id/name 取首个非空，arguments 逐段拼接）
#[derive(Clone, Debug, Default)]
struct ToolCallAcc {
    index: u64,
    id: String,
    name: String,
    arguments: String,
}

fn merge_tool_calls(acc: &mut Vec<ToolCallAcc>, deltas: &[ToolCallDelta]) {
    for delta in deltas {
        if let Some(slot) = acc.iter_mut().find(|call| call.index == delta.index) {
            if let Some(id) = &delta.id {
                if !id.is_empty() {
                    slot.id = id.clone();
                }
            }
            if let Some(name) = &delta.name {
                if !name.is_empty() {
                    slot.name = name.clone();
                }
            }
            if let Some(arguments) = &delta.arguments {
                slot.arguments.push_str(arguments);
            }
            continue;
        }
        acc.push(ToolCallAcc {
            index: delta.index,
            id: delta.id.clone().unwrap_or_default(),
            name: delta.name.clone().unwrap_or_default(),
            arguments: delta.arguments.clone().unwrap_or_default(),
        });
    }
}

fn finalize_tool_calls(acc: Vec<ToolCallAcc>) -> Vec<ToolCall> {
    acc.into_iter()
        .filter(|call| !call.name.is_empty())
        .map(|call| ToolCall {
            id: call.id,
            name: call.name,
            arguments: call.arguments,
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct ChatReply {
    pub text: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
}

/// 一轮对话的结果：用量 + 是否被 no_reply 静默
struct ChatOutcome {
    usage: StreamUsage,
    suppressed: bool,
    /// 本轮工具调用产生的中间 assistant/tool 消息，按原顺序保存。
    generated: Vec<Value>,
    /// 最后一次模型调用返回的 reasoning_content。
    reasoning: String,
}

static CONFIG_CACHE: std::sync::OnceLock<Mutex<(Option<Instant>, ChatConfig)>> =
    std::sync::OnceLock::new();

fn cached_config() -> ChatConfig {
    let cache = CONFIG_CACHE.get_or_init(|| Mutex::new((None, ChatConfig::default())));
    let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
    let fresh = guard
        .0
        .is_some_and(|loaded_at| loaded_at.elapsed() < CONFIG_CACHE_TTL);
    if !fresh {
        guard.1 = ChatConfig::load();
        guard.0 = Some(Instant::now());
    }
    guard.1.clone()
}

pub struct ChatGptPlugin;

impl Plugin for ChatGptPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("chatgpt", env!("CARGO_PKG_VERSION"))
            .description("ChatGPT 聊天机器人：群聊 @ 或前缀触发，私聊默认回复（OpenAI 兼容接口，可选命令行执行工具）")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        let _ = PLUGIN_CTX.set(ctx.clone());
        config::register(ctx)?;
        ctx.route(
            Method::POST,
            BOT_API_PATH,
            RouteAccess::Write,
            bot_api_call,
        );
        // 远端 MQTT 事件只用于脚本/转发；AI 对话仍只响应本实例收到的消息，
        // 否则集群里每个实例都会对同一条远端消息各自产生一次回复。
        ctx.subscribe(
            0,
            EventFilter::new()
                .post_type("message")
                .custom(|event| event.get_str("mqtt_client_id").unwrap_or("").is_empty()),
            |event: &Event| {
                handle_event(event);
                EventAction::Continue
            },
        );
        Ok(())
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info(LOG_TAG, "ChatGPT 插件已启动");
        if !cached_config().is_ready() {
            ctx.logger().warn(
                LOG_TAG,
                "尚未配置 API Key，插件暂不回复；请在管理台「chatgpt → 配置」中填写",
            );
        }
        Ok(())
    }
}

fn handle_event(event: &Event) {
    if is_self_event(event) {
        return;
    }
    let session_key = session_key(event);
    if session_key.is_empty() {
        return;
    }

    let config = cached_config();
    if !group_allowed(event, &config) {
        return;
    }
    // 即使没有文本（例如纯图片），也要记录原始事件 JSON 作为上下文。
    let text = extract_text(event).unwrap_or_default();
    let triggered =
        config.is_ready() && !text.is_empty() && match_trigger(event, &text, &config).is_some();
    session::dispatch(session_key, event.clone(), triggered);
}

fn is_self_event(event: &Event) -> bool {
    let user_id = event
        .get("user_id")
        .map(value_to_string)
        .unwrap_or_default();
    !user_id.is_empty() && user_id == event.self_id
}

/// 群白名单为空时所有群都不启用；非空时仅白名单内的群会进入上下文并触发回复。
fn group_allowed(event: &Event, config: &ChatConfig) -> bool {
    if event.message_type.as_deref() != Some("group") {
        return true;
    }
    let group_id = event
        .get("group_id")
        .map(value_to_string)
        .unwrap_or_default();
    !group_id.is_empty() && config.group_whitelist.iter().any(|allowed| allowed == &group_id)
}

async fn call_api_json(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    remote_id: &str,
    action: Value,
) -> Value {
    let text = action.to_string();
    let platform = platform.to_owned();
    let self_id = self_id.to_owned();
    let passive_id = passive_id.to_owned();
    let remote_id = remote_id.to_owned();
    let send_handle = redlang::runtime().spawn_blocking(move || {
        redlang::cqapi::cq_call_api(&platform, &self_id, &passive_id, &text, &remote_id)
    });
    match send_handle.await {
        Ok(ret) => serde_json::from_str(&ret).unwrap_or(json!({"retcode": -1, "raw": ret})),
        Err(_) => json!({"retcode": -1}),
    }
}

fn api_ok(ret: &Value) -> bool {
    ret.get("retcode").and_then(Value::as_i64) == Some(0)
}

async fn bot_api_call(
    request: Request<Incoming>,
    _context: RouteContext,
) -> std::result::Result<HttpResponse, HttpError> {
    let body = collect_body(request.into_body(), usize::MAX).await?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))?;
    let platform = required_api_text(&payload, "platform")?;
    let self_id = required_api_text(&payload, "self_id")?;
    let passive_id = optional_api_text(&payload, "passive_id");
    let remote_id = optional_api_text(&payload, "remote_id");
    let action = required_api_text(&payload, "action")?;
    let params = match payload.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => return Err(HttpError::bad_request("params 必须是 JSON 对象")),
    };
    let mut call = json!({"action": action});
    if params.as_object().is_some_and(|params| !params.is_empty()) {
        call["params"] = params;
    }
    log_info(format!("ChatGPT Bot API 调用：{action}"));
    let result = call_api_json(&platform, &self_id, &passive_id, &remote_id, call).await;
    Ok(json_response(StatusCode::OK, &result))
}

fn required_api_text(
    payload: &Value,
    key: &str,
) -> std::result::Result<String, HttpError> {
    let text = match payload.get(key) {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    if text.is_empty() {
        return Err(HttpError::bad_request(format!("缺少 {key}")));
    }
    Ok(text)
}

fn optional_api_text(payload: &Value, key: &str) -> String {
    match payload.get(key) {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

const STREAM_FLUSH_CHARS: usize = 120;

enum ReplySink {
    Whole,
    QqStream(QqStreamState),
}

struct QqStreamState {
    platform: String,
    self_id: String,
    passive_id: String,
    remote_id: String,
    user_id: String,
    session_id: Option<String>,
    index: u64,
    pending: String,
    failed: bool,
}

impl ReplySink {
    fn for_event(
        platform: &str,
        message_type: &str,
        self_id: &str,
        passive_id: &str,
        remote_id: &str,
        user_id: Option<&Value>,
    ) -> Self {
        let user_id = user_id.map(value_to_string).unwrap_or_default();
        if platform == "qqgroup_public" && message_type == "private" && !user_id.is_empty() {
            ReplySink::QqStream(QqStreamState {
                platform: platform.to_owned(),
                self_id: self_id.to_owned(),
                passive_id: passive_id.to_owned(),
                remote_id: remote_id.to_owned(),
                user_id,
                session_id: None,
                index: 1,
                pending: String::new(),
                failed: false,
            })
        } else {
            ReplySink::Whole
        }
    }

    async fn push(&mut self, piece: &str) {
        if let ReplySink::QqStream(state) = self {
            state.push(piece).await;
        }
    }

    async fn finish_ok(&mut self) -> bool {
        match self {
            ReplySink::Whole => false,
            ReplySink::QqStream(state) => state.finish_ok().await,
        }
    }

    async fn finish_err(&mut self, err_text: Option<&str>) -> Option<String> {
        match self {
            ReplySink::Whole => err_text.map(str::to_owned),
            ReplySink::QqStream(state) => {
                state.finish_err(err_text).await;
                None
            }
        }
    }
}

impl QqStreamState {
    async fn push(&mut self, piece: &str) {
        if self.failed || piece.is_empty() {
            return;
        }
        self.pending.push_str(piece);
        if self.pending.chars().count() >= STREAM_FLUSH_CHARS {
            self.flush().await;
        }
    }

    async fn flush(&mut self) {
        if self.failed || self.pending.is_empty() {
            return;
        }
        let content = std::mem::take(&mut self.pending);
        if self.session_id.is_none() {
            match self.call_start(&content).await {
                Ok(session_id) => {
                    self.session_id = Some(session_id);
                }
                Err(error) => {
                    self.failed = true;
                    log_warn(format!("ChatGPT 流式开始失败:{error}"));
                }
            }
            return;
        }
        let index = self.index;
        self.index += 1;
        if let Err(error) = self.call("append", &content, index).await {
            self.failed = true;
            log_warn(format!("ChatGPT 流式发送失败:{error}"));
        }
    }

    async fn finish_ok(&mut self) -> bool {
        self.flush().await;
        if self.failed || self.session_id.is_none() {
            return false;
        }
        let index = self.index;
        self.index += 1;
        if let Err(error) = self.call("end", "", index).await {
            log_warn(format!("ChatGPT 流式结束失败:{error}"));
        }
        true
    }

    async fn finish_err(&mut self, err_text: Option<&str>) {
        if self.session_id.is_none() {
            return;
        }
        let index = self.index;
        self.index += 1;
        let _ = self.call("end", err_text.unwrap_or(""), index).await;
    }

    async fn call_start(&self, content: &str) -> std::result::Result<String, String> {
        let ret = self.call("start", content, 0).await?;
        ret.get("data")
            .and_then(|data| data.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("流式开始未返回 session_id:{ret}"))
    }

    async fn call(
        &self,
        mode: &str,
        content: &str,
        index: u64,
    ) -> std::result::Result<Value, String> {
        let mut params = json!({
            "mode": mode,
            "user_id": self.user_id,
            "content": content,
        });
        if mode != "start" {
            params["session_id"] = json!(self.session_id.clone().unwrap_or_default());
            params["index"] = json!(index);
        }
        let ret = call_api_json(
            &self.platform,
            &self.self_id,
            &self.passive_id,
            &self.remote_id,
            json!({"action": "send_stream_msg", "params": params}),
        )
        .await;
        if !api_ok(&ret) {
            return Err(ret.to_string());
        }
        Ok(ret)
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_reply_text(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    remote_id: &str,
    message_type: &str,
    group_id: Option<&Value>,
    user_id: Option<&Value>,
    text: &str,
) {
    let Some(send_json) = build_reply(message_type, group_id, user_id, text) else {
        return;
    };
    let ret = call_api_json(platform, self_id, passive_id, remote_id, send_json).await;
    if !api_ok(&ret) {
        log_warn(format!("ChatGPT 回复失败:{ret}"));
    }
}

fn extract_text(event: &Event) -> Option<String> {
    let message = event.get("message")?;
    let text = match message {
        Value::String(text) => text.clone(),
        Value::Array(segments) => segments
            .iter()
            .filter(|segment| segment.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|segment| {
                segment
                    .get("data")
                    .and_then(|data| data.get("text"))
                    .and_then(Value::as_str)
            })
            .collect::<String>(),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

fn is_at_self(event: &Event) -> bool {
    let Some(Value::Array(segments)) = event.get("message") else {
        return false;
    };
    let self_id = event.self_id.trim();
    if self_id.is_empty() {
        return false;
    }
    segments
        .iter()
        .filter(|segment| segment.get("type").and_then(Value::as_str) == Some("at"))
        .filter_map(|segment| segment.get("data"))
        .any(|data| {
            ["qq", "id", "user_id"]
                .iter()
                .any(|key| data.get(key).map(value_to_string).as_deref() == Some(self_id))
        })
}

fn match_trigger(event: &Event, text: &str, config: &ChatConfig) -> Option<(String, String)> {
    let prompt = match event.message_type.as_deref() {
        Some("group") => {
            if is_at_self(event) {
                text.to_owned()
            } else {
                strip_prefix(text, &config.prefixes)?
            }
        }
        Some("private") => {
            if config.private_always {
                strip_prefix(text, &config.prefixes).unwrap_or_else(|| text.to_owned())
            } else {
                strip_prefix(text, &config.prefixes)?
            }
        }
        _ => return None,
    };
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    Some((prompt.to_owned(), session_key(event)))
}

fn strip_prefix(text: &str, prefixes: &[String]) -> Option<String> {
    prefixes
        .iter()
        .find(|prefix| text.starts_with(prefix.as_str()))
        .map(|prefix| text[prefix.len()..].to_owned())
}

fn session_key(event: &Event) -> String {
    let target = match event.message_type.as_deref() {
        Some("group") => event
            .get("group_id")
            .map(value_to_string)
            .unwrap_or_default(),
        _ => event
            .get("user_id")
            .map(value_to_string)
            .unwrap_or_default(),
    };
    format!("{}:{}:{target}", event.platform, event.self_id)
}

fn build_messages(
    config: &ChatConfig,
    history: &[ChatMsg],
    prompt: &str,
    event_json: &str,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(history.len() + 3);
    if !config.system_prompt.trim().is_empty() {
        messages.push(json!({"role": "system", "content": config.system_prompt}));
    }
    for item in history {
        messages.push(json!({"role": item.role, "content": item.content}));
    }
    let user_content = if event_json.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}\n\n[当前事件原始JSON]\n{event_json}")
    };
    messages.push(json!({"role": "user", "content": user_content}));
    messages
}

fn command_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": COMMAND_TOOL_NAME,
            "description": r#"在本机命令行执行命令并返回标准输出与标准错误。Windows 使用 cmd.exe /C，Linux/macOS 使用 sh -c。适合查看系统信息、列目录、运行已安装的命令行程序、用 curl 访问网络接口等；不支持交互式输入和长期运行的程序。命令以机器人进程权限执行，请谨慎使用。若环境变量 REDREPLY_API_URL 存在，可向该地址 POST JSON 调用 Bot API：{"platform":"onebot11","self_id":"10001","passive_id":"40004","remote_id":"","action":"send_group_msg","params":{"group_id":20002,"message":[{"type":"text","data":{"text":"hi"}}]}}。platform/self_id 必填，从当前事件原始 JSON 获取；passive_id 通常取 message_id，remote_id 通常取 mqtt_client_id，均可省略。Windows PowerShell 可用：Invoke-RestMethod -Method Post -Uri $env:REDREPLY_API_URL -ContentType 'application/json' -Body '<JSON>'；也可用 curl -s -X POST "$REDREPLY_API_URL" -H "Content-Type: application/json" -d '<JSON>'。可先运行 echo %REDREPLY_API_URL%（Windows cmd）或 echo $REDREPLY_API_URL（PowerShell/Unix）确认地址。"#,
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要执行的完整命令行"
                    }
                },
                "required": ["command"]
            }
        }
    })
}

/// 静默工具：调用后本轮不输出任何文本
fn no_reply_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NO_REPLY_TOOL_NAME,
            "description": "静默本轮：调用后本轮不会再向聊天发送任何文本回复。适用于你已经决定不回复、或判断这条消息不需要回复时。",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }
    })
}

fn build_request_body(config: &ChatConfig, messages: &[Value]) -> Value {
    let mut body = json!({
        "model": config.model,
        "messages": messages,
        "temperature": config.temperature,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if !config.reasoning_effort.trim().is_empty() {
        body["reasoning_effort"] = json!(config.reasoning_effort);
    }
    if config.command_enabled {
        body["tools"] = json!([command_tool_definition(), no_reply_tool_definition()]);
        body["tool_choice"] = json!("auto");
    }
    body
}

/// 把模型的 tool_calls 回合还原成 assistant 消息。
///
/// DeepSeek 在带 tools 的对话中要求回传该轮的 reasoning_content。
fn assistant_tool_call_message(text: &str, calls: &[ToolCall], reasoning: &str) -> Value {
    let tool_calls = calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": call.arguments},
            })
        })
        .collect::<Vec<_>>();
    let mut message = json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { json!(text) },
        "tool_calls": tool_calls,
    });
    if !reasoning.trim().is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    message
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StreamUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// 缓存命中的 prompt token 数（OpenAI `prompt_tokens_details.cached_tokens` /
    /// DeepSeek `prompt_cache_hit_tokens` / 部分代理 `cache_read_input_tokens`）
    pub cached_tokens: u64,
}

pub(crate) async fn chat_once(
    config: &ChatConfig,
    session_id: &str,
    history: &[ChatMsg],
    prompt: &str,
) -> std::result::Result<ChatReply, String> {
    let mut full = String::new();
    let mut sink = ReplySink::Whole;
    let outcome = chat_dialog(
        config,
        session_id,
        history,
        prompt,
        &mut full,
        &mut sink,
        &TurnContext::default(),
    )
    .await?;
    let usage = outcome.usage;
    Ok(ChatReply {
        text: full,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_tokens: usage.cached_tokens,
    })
}

struct SseEvent {
    piece: String,
    reasoning: String,
    tool_calls: Vec<ToolCallDelta>,
    usage: Option<StreamUsage>,
    done: bool,
}

/// 解析 usage：prompt/completion 必须存在；缓存命中数兼容多种字段命名。
fn parse_usage(usage: &Value) -> Option<StreamUsage> {
    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64)?;
    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_u64)?;
    let cached_tokens = usage
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64))
        .or_else(|| usage.get("cache_read_input_tokens").and_then(Value::as_u64))
        .unwrap_or(0);
    Some(StreamUsage {
        prompt_tokens,
        completion_tokens,
        cached_tokens,
    })
}

fn parse_sse_line(line: &str) -> std::result::Result<SseEvent, String> {
    let empty = SseEvent {
        piece: String::new(),
        reasoning: String::new(),
        tool_calls: Vec::new(),
        usage: None,
        done: false,
    };
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() || line.starts_with(':') {
        return Ok(empty);
    }
    let payload = match line.strip_prefix("data:") {
        Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
        None => return Ok(empty),
    };
    if payload == "[DONE]" {
        return Ok(SseEvent {
            done: true,
            ..empty
        });
    }
    let body: Value =
        serde_json::from_str(payload).map_err(|error| format!("流式响应解析失败：{error}"))?;
    if let Some(error) = body.get("error") {
        let message = match error {
            Value::String(text) => text.clone(),
            _ => error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_owned(),
        };
        return Err(format!("模型返回错误：{message}"));
    }
    let usage = body.get("usage").and_then(parse_usage);
    let delta = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"));
    let piece = delta.and_then(extract_text_content).unwrap_or_default();
    let reasoning = delta
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let tool_calls = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|item| ToolCallDelta {
                    index: item.get("index").and_then(Value::as_u64).unwrap_or(0),
                    id: item.get("id").and_then(Value::as_str).map(str::to_owned),
                    name: item
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    arguments: item
                        .get("function")
                        .and_then(|function| function.get("arguments"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SseEvent {
        piece,
        reasoning,
        tool_calls,
        usage,
        done: false,
    })
}

fn extract_text_content(obj: &Value) -> Option<String> {
    match obj.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        part.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

/// 单轮流式请求：返回本轮文本、工具调用与用量。
/// live=true 时文本边收边推送到 sink；false 时只累积，由调用方决定何时推送。
async fn chat_stream(
    config: &ChatConfig,
    session_id: &str,
    body: &Value,
    sink: &mut ReplySink,
    live: bool,
) -> std::result::Result<(String, Vec<ToolCall>, StreamUsage, String), String> {
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .user_agent("redreply-chatgpt/1.0")
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|error| format!("创建网络客户端失败：{error}"))?;
    let mut response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", config.api_key.trim()))
        .header("x-opencode-session", session_id)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .body(body.to_string())
        .send()
        .await
        .map_err(|error| format!("网络请求失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response
            .text()
            .await
            .map_err(|error| format!("读取响应失败：{error}"))?;
        return Err(format!("HTTP {status}：{text}"));
    }
    let mut full = String::new();
    let mut reasoning = String::new();
    let mut calls: Vec<ToolCallAcc> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut usage: Option<StreamUsage> = None;
    let mut finished = false;
    while !finished {
        let chunk = response
            .chunk()
            .await
            .map_err(|error| format!("读取流失败：{error}"))?;
        let Some(bytes) = chunk else {
            break;
        };
        buf.extend_from_slice(&bytes);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = buf.drain(..=pos).collect();
            let event = parse_sse_line(&String::from_utf8_lossy(&raw))?;
            if event.done {
                finished = true;
                break;
            }
            if let Some(tokens) = event.usage {
                usage = Some(tokens);
            }
            if !event.reasoning.is_empty() {
                reasoning.push_str(&event.reasoning);
            }
            if !event.tool_calls.is_empty() {
                merge_tool_calls(&mut calls, &event.tool_calls);
            }
            if !event.piece.is_empty() {
                full.push_str(&event.piece);
                if live {
                    sink.push(&event.piece).await;
                }
            }
        }
    }
    if !buf.is_empty() {
        let event = parse_sse_line(&String::from_utf8_lossy(&buf))?;
        if let Some(tokens) = event.usage {
            usage = Some(tokens);
        }
        if !event.reasoning.is_empty() {
            reasoning.push_str(&event.reasoning);
        }
        if !event.tool_calls.is_empty() {
            merge_tool_calls(&mut calls, &event.tool_calls);
        }
        if !event.piece.is_empty() {
            full.push_str(&event.piece);
            if live {
                sink.push(&event.piece).await;
            }
        }
    }
    let tool_calls = finalize_tool_calls(calls);
    if full.trim().is_empty() && tool_calls.is_empty() {
        return Err("模型返回了空回复".to_string());
    }
    let usage = usage.ok_or_else(|| "接口未返回 usage，无法统计 token".to_string())?;
    Ok((full, tool_calls, usage, reasoning))
}

/// 完整对话：模型请求工具时执行并回填结果，直到模型给出最终文本。
/// 任一轮调用 no_reply 后,本轮不再向聊天输出文本(suppressed=true)。
async fn chat_dialog(
    config: &ChatConfig,
    session_id: &str,
    history: &[ChatMsg],
    prompt: &str,
    full: &mut String,
    sink: &mut ReplySink,
    context: &TurnContext,
) -> std::result::Result<ChatOutcome, String> {
    let messages = build_messages(config, history, prompt, &context.event_json);
    chat_dialog_with_messages(config, session_id, messages, full, sink, None).await
}

/// 使用已经构建好的消息列表执行完整对话（会话记忆链路使用）。
///
/// `turn_id` 用于把本次触发的每次模型调用写入「最近一次触发」详情。
async fn chat_dialog_with_messages(
    config: &ChatConfig,
    session_id: &str,
    mut messages: Vec<Value>,
    full: &mut String,
    sink: &mut ReplySink,
    turn_id: Option<u64>,
) -> std::result::Result<ChatOutcome, String> {
    // 开启工具调用后中间轮可能夹带文本，改为整段发送，避免半截内容先流出。
    let live = !config.command_enabled;
    let mut calls = 0usize;
    let mut total_prompt_tokens = 0u64;
    let mut total_completion_tokens = 0u64;
    let mut total_cached_tokens = 0u64;
    let mut round = 0usize;
    let mut suppressed = false;
    let mut generated: Vec<Value> = Vec::new();
    let mut final_reasoning: String;
    loop {
        let body = build_request_body(config, &messages);
        let estimated = tokenizer::estimate_request_tokens(&body);
        let call_round = calls as u32 + 1;
        let started = Instant::now();
        let (text, tool_calls, usage, reasoning) =
            match chat_stream(config, session_id, &body, sink, live).await {
                Ok(value) => {
                    let tool_calls_json = Value::Array(
                        value
                            .1
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id,
                                    "name": call.name,
                                    "arguments": call.arguments,
                                })
                            })
                            .collect(),
                    );
                    crate::calls::record_success(
                        session_id,
                        &config.model,
                        call_round,
                        value.2.prompt_tokens,
                        value.2.cached_tokens,
                        value.2.completion_tokens,
                        started.elapsed(),
                        turn_id,
                        &body,
                        &value.0,
                        &value.3,
                        tool_calls_json,
                    );
                    value
                }
                Err(error) => {
                    crate::calls::record_failure(
                        session_id,
                        &config.model,
                        call_round,
                        started.elapsed(),
                        &error,
                        turn_id,
                        &body,
                    );
                    return Err(error);
                }
            };
        calls += 1;
        final_reasoning = reasoning;
        total_prompt_tokens += usage.prompt_tokens;
        total_completion_tokens += usage.completion_tokens;
        total_cached_tokens += usage.cached_tokens;
        let estimate_text = estimated
            .map(|tokens| tokens.to_string())
            .unwrap_or_else(|| "-".to_string());
        log_info(format!(
            "ChatGPT 模型调用完成（会话 {session_id} 第 {calls} 次）：估算 prompt_tokens={estimate_text}，实际 prompt_tokens={}，cached_tokens={}，completion_tokens={}，total_tokens={}",
            usage.prompt_tokens,
            usage.cached_tokens,
            usage.completion_tokens,
            usage.prompt_tokens + usage.completion_tokens
        ));
        if tool_calls.is_empty() {
            if calls > 1 {
                log_info(format!(
                    "ChatGPT 会话 {session_id} 本轮共 {calls} 次模型调用：prompt_tokens={total_prompt_tokens}，cached_tokens={total_cached_tokens}，completion_tokens={total_completion_tokens}，total_tokens={}",
                    total_prompt_tokens + total_completion_tokens
                ));
            }
            if suppressed {
                log_info(format!(
                    "ChatGPT 会话 {session_id} 命中 {NO_REPLY_TOOL_NAME}，本轮不输出文本"
                ));
                *full = String::new();
                return Ok(ChatOutcome {
                    usage,
                    suppressed: true,
                    generated,
                    reasoning: final_reasoning,
                });
            }
            if !live && !text.is_empty() {
                sink.push(&text).await;
            }
            *full = text;
            return Ok(ChatOutcome {
                usage,
                suppressed: false,
                generated,
                reasoning: final_reasoning,
            });
        }
        round += 1;
        let names = tool_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        log_info(format!(
            "ChatGPT 会话 {session_id} 第 {round} 轮请求工具：{names}"
        ));
        let assistant_message = assistant_tool_call_message(&text, &tool_calls, &final_reasoning);
        messages.push(assistant_message.clone());
        generated.push(assistant_message);
        for call in &tool_calls {
            let tool_message = if call.name == NO_REPLY_TOOL_NAME {
                suppressed = true;
                json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": "ok：本轮不会再向聊天输出文本",
                })
            } else {
                let result = execute_tool_call(config, call).await;
                json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": result,
                })
            };
            messages.push(tool_message.clone());
            generated.push(tool_message);
        }
    }
}

async fn execute_tool_call(config: &ChatConfig, call: &ToolCall) -> String {
    if call.name != COMMAND_TOOL_NAME {
        return format!("未知工具：{}", call.name);
    }
    let Some(command_text) = command_from_arguments(&call.arguments) else {
        return "参数解析失败：需要 JSON 对象 {\"command\": \"...\"}".to_string();
    };
    log_info(format!(
        "ChatGPT 运行命令行（超时 {} 秒）：{}",
        config.command_timeout_secs,
        command_text.chars().take(120).collect::<String>()
    ));
    let timeout_secs = config.command_timeout_secs;
    let max_output = config.command_max_output;
    match redlang::runtime()
        .spawn_blocking(move || run_command(&command_text, timeout_secs, max_output))
        .await
    {
        Ok(result) => result,
        Err(error) => format!("执行线程失败：{error}"),
    }
}

fn command_from_arguments(arguments: &str) -> Option<String> {
    let value: Value = serde_json::from_str(arguments).ok()?;
    let command = value.get("command").and_then(Value::as_str)?;
    if command.trim().is_empty() {
        return None;
    }
    Some(command.to_owned())
}

fn shell_command(text: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/D", "/S", "/C", text]);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c").arg(text);
        command
    }
}

fn bot_api_url() -> Option<String> {
    let addr = redlang::framework()?.http_addr()?;
    if addr.port() == 0 {
        return None;
    }
    let host = match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    Some(format!("http://{host}:{}{BOT_API_PATH}", addr.port()))
}

/// 同步执行命令行（在 spawn_blocking 中调用），返回给模型的文本结果。
fn run_command(text: &str, timeout_secs: u64, max_output: usize) -> String {
    let mut command = shell_command(text);
    if let Some(url) = bot_api_url() {
        command.env("REDREPLY_API_URL", url);
    }
    match run_command_process(
        command,
        Duration::from_secs(timeout_secs.max(1)),
        max_output,
    ) {
        Ok((status, stdout, stderr, timed_out)) => format_command_result(
            status,
            timed_out,
            timeout_secs,
            &stdout,
            &stderr,
            max_output,
        ),
        Err(error) => format!("启动命令行失败：{error}"),
    }
}

/// (退出状态, stdout, stderr, 是否超时)
type CommandOutput = (Option<std::process::ExitStatus>, Vec<u8>, Vec<u8>, bool);

fn run_command_process(
    mut command: Command,
    timeout: Duration,
    max_output: usize,
) -> std::io::Result<CommandOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (line_tx, line_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let stdout_handle = stdout.map(|pipe| {
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            let mut reader = std::io::BufReader::new(pipe);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if line_tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
    });
    let stderr_handle = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut timed_out = false;
    let mut stdout_bytes = Vec::new();
    loop {
        drain_command_lines(&line_rx, &mut stdout_bytes, max_output);
        if let Some(current) = child.try_wait()? {
            status = Some(current);
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_tree(&mut child);
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }
    drain_command_lines(&line_rx, &mut stdout_bytes, max_output);
    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok((status, stdout_bytes, stderr, timed_out))
}

fn drain_command_lines(
    lines: &std::sync::mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    max_output: usize,
) {
    while let Ok(line) = lines.try_recv() {
        // 粗略限制累积量，避免失控输出吃内存；最终仍按字符数截断。
        if output.len() < max_output.saturating_mul(8).max(64 * 1024) {
            output.extend_from_slice(&line);
        }
    }
}

#[cfg(windows)]
fn kill_process_tree(child: &mut std::process::Child) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let pid = child.id().to_string();
    let _ = Command::new("taskkill")
        .args(["/PID", pid.as_str(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(windows))]
fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    let _ = Command::new("kill")
        .args(["-9", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

fn format_command_result(
    status: Option<std::process::ExitStatus>,
    timed_out: bool,
    timeout_secs: u64,
    stdout: &[u8],
    stderr: &[u8],
    max_output: usize,
) -> String {
    let mut out = String::new();
    if timed_out {
        out.push_str(&format!("执行超时（{timeout_secs} 秒），进程已终止\n"));
    } else if let Some(status) = status {
        out.push_str(&format!("退出码：{}\n", status.code().unwrap_or(-1)));
    }
    out.push_str("stdout:\n");
    out.push_str(decode_command_output(stdout).trim_end());
    out.push('\n');
    out.push_str("stderr:\n");
    out.push_str(decode_command_output(stderr).trim_end());
    truncate_chars(&mut out, max_output);
    out
}

fn decode_command_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    #[cfg(windows)]
    {
        let (text, _, _) = encoding_rs::GBK.decode(bytes);
        text.into_owned()
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn truncate_chars(text: &mut String, max_chars: usize) {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return;
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text.truncate(cut);
    text.push_str("\n…（输出已截断）");
}

fn build_reply(
    message_type: &str,
    group_id: Option<&Value>,
    user_id: Option<&Value>,
    text: &str,
) -> Option<Value> {
    let message = json!([{"type": "text", "data": {"text": text}}]);
    match message_type {
        "group" => {
            let group_id = group_id?.clone();
            Some(json!({
                "action": "send_group_msg",
                "params": {"group_id": group_id, "message": message}
            }))
        }
        "private" => {
            let user_id = user_id?.clone();
            let mut params = json!({"user_id": user_id, "message": message});
            if let Some(group_id) = group_id {
                params["group_id"] = group_id.clone();
            }
            Some(json!({"action": "send_private_msg", "params": params}))
        }
        _ => None,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ChatConfig {
        ChatConfig {
            prefixes: vec!["问 ".to_string(), "gpt ".to_string()],
            private_always: true,
            ..ChatConfig::default()
        }
    }

    fn event(message: Value, message_type: &str) -> Event {
        Event::from_json(json!({
            "platform": "onebot11",
            "self_id": "10001",
            "post_type": "message",
            "message_type": message_type,
            "group_id": 20002,
            "user_id": 30003,
            "message_id": 40004,
            "message": message
        }))
        .unwrap()
    }

    #[test]
    fn reads_text_from_string_and_segments() {
        let string_event = event(json!("问 今天天气"), "group");
        assert_eq!(extract_text(&string_event).as_deref(), Some("问 今天天气"));

        let segment_event = event(
            json!([
                {"type": "at", "data": {"qq": "10001"}},
                {"type": "text", "data": {"text": " 你好 "}}
            ]),
            "group",
        );
        assert_eq!(extract_text(&segment_event).as_deref(), Some("你好"));

        // 纯图片消息没有可触发文本，但原始事件 JSON 仍会进入上下文
        let image_only = event(
            json!([{
                "type": "image",
                "data": {"file": "a.png", "url": "https://example.com/a.png"}
            }]),
            "group",
        );
        assert!(extract_text(&image_only).is_none());
    }

    #[test]
    fn group_triggers_on_prefix() {
        let config = test_config();
        let evt = event(json!("问 今天天气怎么样"), "group");
        let (prompt, session) = match_trigger(&evt, "问 今天天气怎么样", &config).unwrap();
        assert_eq!(prompt, "今天天气怎么样");
        assert_eq!(session, "onebot11:10001:20002");
    }

    #[test]
    fn group_ignores_plain_text_without_at() {
        let config = test_config();
        let evt = event(json!("大家好"), "group");
        assert!(match_trigger(&evt, "大家好", &config).is_none());
    }

    #[test]
    fn group_triggers_on_at_self() {
        let config = test_config();
        let evt = event(
            json!([
                {"type": "at", "data": {"qq": "10001"}},
                {"type": "text", "data": {"text": "讲个笑话"}}
            ]),
            "group",
        );
        let (prompt, _) = match_trigger(&evt, "讲个笑话", &config).unwrap();
        assert_eq!(prompt, "讲个笑话");
    }

    #[test]
    fn group_ignores_at_others() {
        let config = test_config();
        let evt = event(
            json!([
                {"type": "at", "data": {"qq": "99999"}},
                {"type": "text", "data": {"text": "你好"}}
            ]),
            "group",
        );
        assert!(match_trigger(&evt, "你好", &config).is_none());
        let (prompt, _) = match_trigger(&evt, "问 你好", &config).unwrap();
        assert_eq!(prompt, "你好");
    }

    #[test]
    fn private_replies_by_default() {
        let config = test_config();
        let evt = event(json!("你好"), "private");
        let (prompt, session) = match_trigger(&evt, "你好", &config).unwrap();
        assert_eq!(prompt, "你好");
        assert_eq!(session, "onebot11:10001:30003");
    }

    #[test]
    fn private_prefix_only_mode() {
        let mut config = test_config();
        config.private_always = false;
        let evt = event(json!("你好"), "private");
        assert!(match_trigger(&evt, "你好", &config).is_none());
        let (prompt, _) = match_trigger(&evt, "gpt 你好", &config).unwrap();
        assert_eq!(prompt, "你好");
    }

    #[test]
    fn empty_prompt_after_prefix_does_not_trigger() {
        let config = test_config();
        let evt = event(json!("问 "), "group");
        assert!(match_trigger(&evt, "问", &config).is_none());
    }

    #[test]
    fn builds_group_and_private_replies() {
        let send = build_reply("group", Some(&json!(20002)), Some(&json!(30003)), "你好").unwrap();
        assert_eq!(send["action"], "send_group_msg");
        assert_eq!(send["params"]["group_id"], 20002);

        let send =
            build_reply("private", Some(&json!(20002)), Some(&json!(30003)), "你好").unwrap();
        assert_eq!(send["action"], "send_private_msg");
        assert_eq!(send["params"]["user_id"], 30003);
        assert_eq!(send["params"]["group_id"], 20002);
    }

    #[test]
    fn builds_request_body_with_system_and_history() {
        let config = test_config();
        let history = vec![ChatMsg {
            role: "user",
            content: "hi".to_string(),
        }];
        let messages = build_messages(&config, &history, "你好", "");
        let body = build_request_body(&config, &messages);
        assert_eq!(body["model"], config.model);
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
        assert_eq!(body["messages"][2]["role"], "user");
        assert!(body.get("tools").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn reasoning_effort_added_only_when_configured() {
        let mut config = test_config();
        let messages = build_messages(&config, &[], "你好", "");
        config.reasoning_effort = "high".to_string();
        let body = build_request_body(&config, &messages);
        assert_eq!(body["reasoning_effort"], "high");
        config.reasoning_effort = "none".to_string();
        let body = build_request_body(&config, &messages);
        assert_eq!(body["reasoning_effort"], "none");
    }

    #[test]
    fn command_tool_added_only_when_enabled() {
        let mut config = test_config();
        let messages = build_messages(&config, &[], "你好", "");
        config.command_enabled = true;
        let body = build_request_body(&config, &messages);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], COMMAND_TOOL_NAME);
        assert_eq!(body["tools"][1]["function"]["name"], NO_REPLY_TOOL_NAME);
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn command_tool_description_mentions_shell() {
        let definition = command_tool_definition();
        let description = definition["function"]["description"].as_str().unwrap();
        assert!(description.contains("命令行"), "{description}");
        assert!(description.contains("cmd.exe"), "{description}");
        assert!(description.contains("sh -c"), "{description}");
    }

    #[test]
    fn merges_streamed_tool_call_deltas() {
        let first = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"\"}}]}}]}\n",
        )
        .unwrap();
        assert_eq!(first.tool_calls.len(), 1);
        assert_eq!(first.piece, "");
        let second = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"echo 6*7\\\"}\"}}]}}]}\n",
        )
        .unwrap();
        let mut acc = Vec::new();
        merge_tool_calls(&mut acc, &first.tool_calls);
        merge_tool_calls(&mut acc, &second.tool_calls);
        assert_eq!(acc.len(), 1);
        assert_eq!(acc[0].id, "call_1");
        assert_eq!(acc[0].name, "run_command");
        assert_eq!(acc[0].arguments, "{\"command\":\"echo 6*7\"}");
        let calls = finalize_tool_calls(acc);
        assert_eq!(
            command_from_arguments(&calls[0].arguments).as_deref(),
            Some("echo 6*7")
        );
    }

    #[test]
    fn assistant_message_carries_tool_calls() {
        let calls = vec![ToolCall {
            id: "call_1".to_string(),
            name: "run_command".to_string(),
            arguments: "{\"command\":\"echo 1\"}".to_string(),
        }];
        let message = assistant_tool_call_message("", &calls, "先想想");
        assert_eq!(message["role"], "assistant");
        assert!(message["content"].is_null());
        assert_eq!(message["reasoning_content"], "先想想");
        assert_eq!(message["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            message["tool_calls"][0]["function"]["name"],
            "run_command"
        );
    }

    #[test]
    fn rejects_bad_tool_arguments() {
        assert!(command_from_arguments("not json").is_none());
        assert!(command_from_arguments("{}").is_none());
        assert!(command_from_arguments("{\"command\":\"   \"}").is_none());
        assert_eq!(
            command_from_arguments("{\"command\":\"echo 1\"}").as_deref(),
            Some("echo 1")
        );
    }

    #[test]
    fn truncates_long_output() {
        let long = "a".repeat(100);
        let text = format_command_result(None, false, 30, long.as_bytes(), b"", 10);
        assert!(text.contains("已截断"));
        assert!(text.chars().count() < 60);
    }

    #[test]
    fn runs_command_when_available() {
        let result = run_command("echo 42", 10, 4000);
        if result.contains("启动命令行失败") {
            return;
        }
        assert!(result.contains("42"), "{result}");
        assert!(result.contains("退出码：0"), "{result}");
    }

    #[test]
    fn command_timeout_kills_process() {
        let command = if cfg!(windows) {
            "ping 127.0.0.1 -n 30 >nul"
        } else {
            "sleep 30"
        };
        let result = run_command(command, 1, 4000);
        if result.contains("启动命令行失败") {
            return;
        }
        assert!(result.contains("执行超时"), "{result}");
    }

    #[test]
    fn user_message_carries_raw_event_json() {
        let config = test_config();
        let messages = build_messages(&config, &[], "你好", "{\"group_id\":20002}");
        let content = messages.last().unwrap()["content"].as_str().unwrap();
        assert!(content.starts_with("你好"));
        assert!(content.contains("[当前事件原始JSON]"));
        assert!(content.contains("20002"));
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn read_http_body(stream: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let Ok(read) = stream.read(&mut buf) else {
                break;
            };
            if read == 0 {
                break;
            }
            data.extend_from_slice(&buf[..read]);
            let Some(header_end) = find_subslice(&data, b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = header_end + 4;
            if data.len() >= body_start + content_length {
                return String::from_utf8_lossy(&data[body_start..body_start + content_length])
                    .to_string();
            }
        }
        String::new()
    }

    fn sse_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    #[tokio::test]
    async fn tool_round_trip_executes_command_and_feeds_result_back() {
        use std::io::Write as _;
        use std::net::TcpListener;

        if run_command("echo 1", 5, 100).contains("启动命令行失败") {
            return;
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for round in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_http_body(&mut stream));
                let body = if round == 0 {
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先计算 6*7\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"echo 42\\\"}\"}}]}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
                        "data: [DONE]\n\n"
                    )
                } else {
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"最终答案\",\"content\":\"答案是42\"}}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":6}}\n\n",
                        "data: [DONE]\n\n"
                    )
                };
                stream.write_all(sse_response(body).as_bytes()).unwrap();
                let _ = stream.flush();
            }
            requests
        });

        let mut config = test_config();
        config.api_key = "sk-test".to_string();
        config.base_url = format!("http://{addr}");
        config.command_enabled = true;

        let mut full = String::new();
        let mut sink = ReplySink::Whole;
        let outcome = chat_dialog(
            &config,
            "test-tools",
            &[],
            "6*7 等于几",
            &mut full,
            &mut sink,
            &TurnContext::default(),
        )
        .await
        .unwrap();
        assert!(!outcome.suppressed);
        assert_eq!(full, "答案是42");
        assert_eq!(outcome.usage.prompt_tokens, 20);

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let second: Value = serde_json::from_str(&requests[1]).unwrap();
        assert_eq!(second["tools"][0]["function"]["name"], COMMAND_TOOL_NAME);
        let messages = second["messages"].as_array().unwrap();
        assert!(messages.iter().any(|message| {
            message["role"] == "assistant" && message["tool_calls"][0]["id"] == "call_1"
        }));
        assert!(messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "call_1"
                && message["content"].as_str().unwrap_or("").contains("42")
        }));
        // DeepSeek 带 tools 时要求回传上一轮的 reasoning_content
        assert!(messages.iter().any(|message| {
            message["role"] == "assistant"
                && message["reasoning_content"] == "先计算 6*7"
        }));
        assert_eq!(outcome.reasoning, "最终答案");
        assert!(!outcome.generated.is_empty());
    }

    #[tokio::test]
    async fn no_reply_tool_suppresses_output() {
        use std::io::Write as _;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for round in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_http_body(&mut stream));
                let body = if round == 0 {
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_9\",\"type\":\"function\",\"function\":{\"name\":\"no_reply\",\"arguments\":\"{}\"}}]}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
                        "data: [DONE]\n\n"
                    )
                } else {
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"这段文字不应被输出\"}}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":6}}\n\n",
                        "data: [DONE]\n\n"
                    )
                };
                stream.write_all(sse_response(body).as_bytes()).unwrap();
                let _ = stream.flush();
            }
            requests
        });

        let mut config = test_config();
        config.api_key = "sk-test".to_string();
        config.base_url = format!("http://{addr}");
        config.command_enabled = true;

        let mut full = String::new();
        let mut sink = ReplySink::Whole;
        let outcome = chat_dialog(
            &config,
            "test-no-reply",
            &[],
            "在吗",
            &mut full,
            &mut sink,
            &TurnContext::default(),
        )
        .await
        .unwrap();
        assert!(outcome.suppressed);
        assert!(full.is_empty());

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let second: Value = serde_json::from_str(&requests[1]).unwrap();
        let messages = second["messages"].as_array().unwrap();
        assert!(messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "call_9"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("不会再向聊天输出文本")
        }));
    }

    #[tokio::test]
    async fn logs_token_usage_after_model_call() {
        use std::io::Write as _;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_http_body(&mut stream);
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n\n",
                "data: [DONE]\n\n"
            );
            stream.write_all(sse_response(body).as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let mut config = test_config();
        config.api_key = "sk-test".to_string();
        config.base_url = format!("http://{addr}");
        let mut full = String::new();
        let mut sink = ReplySink::Whole;
        let outcome = chat_dialog(
            &config,
            "test-token-log",
            &[],
            "hi",
            &mut full,
            &mut sink,
            &TurnContext::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.usage.prompt_tokens, 7);
        assert_eq!(outcome.usage.completion_tokens, 3);
        assert_eq!(outcome.usage.cached_tokens, 4);
        server.join().unwrap();

        let logs = redlang::cqapi::get_history_log();
        assert!(
            logs.iter().any(|line| line.contains("test-token-log")
                && line.contains("prompt_tokens=7")
                && line.contains("cached_tokens=4")
                && line.contains("completion_tokens=3")
                && line.contains("total_tokens=10")),
            "{logs:?}"
        );
    }

    #[test]
    fn parses_sse_pieces_usage_and_done() {
        let event = parse_sse_line("data: {\"choices\": [{\"delta\": {\"content\": \"你好\"}}]}\n")
            .unwrap();
        assert_eq!(event.piece, "你好");
        assert!(event.usage.is_none());
        assert!(!event.done);

        let event = parse_sse_line(
            "data: {\"choices\": [{\"delta\": {\"reasoning_content\": \"先想一想\"}}]}\n",
        )
        .unwrap();
        assert_eq!(event.reasoning, "先想一想");
        assert_eq!(event.piece, "");

        let event = parse_sse_line(
            "data: {\"choices\": [{\"delta\": {\"role\": \"assistant\"}}], \"usage\": {\"prompt_tokens\": 10, \"completion_tokens\": 5}}\n",
        )
        .unwrap();
        assert_eq!(event.piece, "");
        assert_eq!(
            event.usage,
            Some(StreamUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                cached_tokens: 0
            })
        );

        let event = parse_sse_line(
            "data: {\"choices\": [{\"delta\": {\"content\": [{\"type\": \"text\", \"text\": \"好\"}]}}]}\n",
        )
        .unwrap();
        assert_eq!(event.piece, "好");

        let event = parse_sse_line("data: [DONE]\n").unwrap();
        assert!(event.done);

        let event = parse_sse_line(": keep-alive\n").unwrap();
        assert!(!event.done);
        assert_eq!(event.piece, "");

        let event = parse_sse_line("\n").unwrap();
        assert!(!event.done);

        assert!(parse_sse_line("data: {\"error\": {\"message\": \"bad key\"}}\n").is_err());
        assert!(parse_sse_line("data: not-json\n").is_err());
    }

    #[test]
    fn parses_cached_tokens_from_supported_shapes() {
        let openai = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n",
        )
        .unwrap();
        assert_eq!(openai.usage.unwrap().cached_tokens, 4);

        let deepseek = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"prompt_cache_hit_tokens\":7,\"prompt_cache_miss_tokens\":3}}\n",
        )
        .unwrap();
        assert_eq!(deepseek.usage.unwrap().cached_tokens, 7);

        let anthropic_style = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"cache_read_input_tokens\":9}}\n",
        )
        .unwrap();
        assert_eq!(anthropic_style.usage.unwrap().cached_tokens, 9);
    }
}
