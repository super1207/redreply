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


mod chat;
mod runtime;
mod tools;

pub(crate) use chat::*;
pub(crate) use runtime::*;
pub(crate) use tools::*;

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
