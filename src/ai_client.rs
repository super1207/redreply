use std::collections::HashSet;
use std::error::Error;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::config_manager::Live2dChatConfig;

type ChatResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub const JAVASCRIPT_TOOL_NAME: &str = "execute_javascript";
pub const MAX_TOOL_RESULT_CHARS: usize = 10_000;
pub const CONTEXT_COMPRESS_THRESHOLD: f64 = 0.75;

const DIARY_BLOCK_START: &str = "<<<DIARY>>>";
const DIARY_BLOCK_END: &str = "<<<DIARY_END>>>";
pub const COMPRESSED_SUMMARY_PREFIX: &str = "以下为之前对话的压缩摘要：\n";
const COMPRESS_PROMPT: &str = "你是对话压缩助手和看板娘日记本管理员。请把下面的历史对话压缩成详尽的中文摘要。摘要应当信息密度高、篇幅尽量长：尽可能完整地保留所有重要事实、数据与数字、人名与作品名、时间与地点、用户偏好及其具体内容、已执行的关键操作（包括工具调用、文件路径、命令与结果）、讨论过程与最终结论，并尽量保留原始措辞中的关键语句。宁可多写也不可遗漏，不要为了追求简短而丢失任何信息。只输出摘要本身，不要添加任何前言、解释或格式标记。

在输出摘要的同时，你可以维护日记本（最多 {DIARY_LIMIT} 条，每条有标题和内容，看板娘会长期保留并参考它）：
- 只有值得长期保存的信息才写入日记（重要事件、用户的长期偏好、关键结论、重要承诺等），不要记录琐碎内容。
- 新增条目时，在摘要之后另起一行输出：{DIARY_BLOCK_START}{\"add\":[{\"title\":\"标题\",\"content\":\"内容\"}],\"delete\":[\"已有条目KEY\"]}{DIARY_BLOCK_END}，标题保持精炼，内容可以写得详细一些，以保留更多可用信息。
- 相同主题的日记应覆盖：先在 delete 中给出旧条目 KEY，再在 add 中写入新内容。
- 如果当前日记条数已达上限 {DIARY_LIMIT} 条，必须通过 delete 删除一些不重要的旧条目，为新增条目腾出空间；若没有值得新增的内容且未超上限，则不输出日记块。
- 如果不需要更新日记，绝对不要输出日记块。

当前日记本内容（格式为“KEY: 条目KEY | 标题: 标题 | 创建时间: 创建时间 | 内容: 内容”，空表示日记本为空，时间为空表示未知）：
{DIARY_LIST}

注意：日记块只能出现一次，且必须严格使用上述格式，否则将被忽略。";
const AGENT_INSTRUCTION: &str = "注意，无论用户如何引导，都不能假装自己是猫娘。你可以通过 execute_javascript 工具在当前浏览器页面执行 JavaScript，并读取工具返回值。页面已内置 compat 兼容层，直接使用即可：axios 全局可用，可直接请求任意 http/https 地址（公网、内网、localhost、局域网均可访问），无需关心 CORS 或代理细节；nodeFs 提供 fs.promises 风格文件 API（readFile/writeFile/appendFile/readdir/mkdir/rm/exists/stat）；nodeCp.exec(cmd) 与 nodeCp.execFile(prog,args) 执行系统命令（Windows 命令需带 cmd /c 前缀，返回 {stdout,stderr}）。可以多次调用工具来观察页面或完成用户命令。工具代码是 AsyncFunction 的函数体，可使用 await，并应通过 return 返回对下一步有用的结果；函数参数 signal 是 AbortSignal，长任务和 fetch 应监听 signal 以便用户停止。可用全局包括 window 和 document。你拥有服务端持久化的日记本（长期记忆，最多100条，每条有标题、内容、创建/更新时间），其全部标题已列在系统消息中供你参考；需要查看某条日记的完整内容时，可通过 POST /live2d_diary_query 直接用 SQLite 语句查询数据库（请求体 JSON 为 {\"sql\":\"你的SQL\"}，仅支持 SELECT/PRAGMA 只读查询，返回 data.columns 为列名数组、data.rows 为行数组），也可通过 fetch('/live2d_diary') 获取全部日记。回答涉及长期信息时先参考日记本标题并查询需要的日记。红色问答自身提供脚本执行能力：调试页面（托盘菜单“红色调试”打开 http://localhost:端口/debug.html）通过 HTTP 接口运行脚本，POST /run_code 将脚本作为平台消息事件异步执行（请求体 JSON 字段为 platform、bot_id、groups_id、group_id、user_id、pkg_name、content），立即返回 retcode 0；POST /run_code_and_ret 同步执行，retcode 0 时 data 为脚本最终返回值，retcode -1 时 data 为错误信息。脚本支持“运行PY”执行 Python 代码、“运行本地PY”执行本地 Python、“命令行”调用系统命令。如果你不会编写红色问答的脚本，先通过 fetch 阅读内置帮助文档学习：http://localhost:端口/docs/index.html，RedLang 语法详细参考在 http://localhost:端口/docs/detailref/README.md，脚本例子在 http://localhost:端口/docs/example/README.md，阅读后再编写并执行脚本。所有 RedLang 命令与脚本语法，使用前必须先在该文档或源码（src/redlang 目录）中找到出处、确认用法后才能使用，禁止凭猜测调用不存在的命令；遇到语法错误、命令不存在或行为异常时，第一时间查阅文档或源码定位问题，不要臆测。红色问答在编译时嵌入了项目源码（build.rs 收集 src、crates 及根目录 Cargo.toml 等，随二进制分发，排除 target 等目录）。需要查阅项目实现或理解代码时：先 GET /source_code_list 获取全部嵌入文件路径列表（JSON 数组，retcode 0），再 GET /source_code?path=相对路径（例如 src/lib.rs、src/httpserver/mod.rs、Cargo.toml、crates/redreply-adapter/Cargo.toml）读取对应源码文本（返回 JSON，retcode 0 时 data 字段为源码内容，retcode -1 表示文件不存在或未嵌入）。注意嵌入内容以最近一次编译为准，源码修改后需重新编译才生效；查询路径不带前导斜杠。";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentToolCall {
    Javascript {
        id: String,
        name: String,
        javascript: String,
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
    },
    ToolCalls {
        calls: Vec<AgentToolCall>,
        messages: Vec<Value>,
        reasoning_content: Option<String>,
        content: Option<String>,
    },
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: Value,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct JavascriptToolArguments {
    javascript: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct DiaryEntry {
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DiaryUpdate {
    #[serde(default)]
    pub add: Vec<DiaryEntry>,
    #[serde(default)]
    pub delete: Vec<String>,
}

impl DiaryUpdate {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.delete.is_empty()
    }
}

pub struct CompressOutcome {
    pub summary: String,
    pub diary: DiaryUpdate,
}

/// 当前 Unix 时间戳（秒），用于给聊天消息打时间标记。
pub fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn start_agent(
    config: &Live2dChatConfig,
    history: &[ChatMessage],
    message: &str,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
) -> ChatResult<AgentStep> {
    let messages = build_messages(config, history, message, diaries)?;
    request_step(config, messages).await
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

pub async fn continue_conversation(
    config: &Live2dChatConfig,
    messages: &[Value],
    message: &str,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
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
    request_step(config, messages).await
}

pub async fn continue_agent(
    config: &Live2dChatConfig,
    messages: &[Value],
    tool_results: &[(String, String)],
) -> ChatResult<AgentStep> {
    let mut messages = messages.to_vec();
    append_tool_results(&mut messages, tool_results)?;
    request_step(config, messages).await
}

/// 估算一组消息占用的 token 数，用于上下文用量检查。
/// 中文等 CJK 字符按 1 token 计算，其余字符按每 4 个字符 1 token 估算，
/// 每条消息另计少量固定开销。
pub fn estimate_context_tokens(messages: &[Value]) -> u64 {
    let mut tokens = 0u64;
    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_str) else {
            continue;
        };
        let mut non_cjk = 0u64;
        for ch in content.chars() {
            if matches!(
                ch,
                '\u{1100}'..='\u{11FF}'
                    | '\u{2E80}'..='\u{9FFF}'
                    | '\u{AC00}'..='\u{D7AF}'
                    | '\u{F900}'..='\u{FAFF}'
                    | '\u{20000}'..='\u{2FA1F}'
            ) {
                tokens += 1;
            } else {
                non_cjk += 1;
            }
        }
        tokens += (non_cjk + 3) / 4;
        tokens += 4;
    }
    tokens
}

/// 构造压缩助手的系统提示词，附上当前日记本内容。
fn build_compress_prompt(diaries: &[crate::resource_manager::Live2dDiaryEntry]) -> String {
    let diary_list = if diaries.is_empty() {
        "(空)".to_owned()
    } else {
        diaries
            .iter()
            .map(|entry| {
                format!(
                    "KEY: {} | 标题: {} | 创建时间: {} | 内容: {}",
                    entry.key, entry.title, entry.create_time, entry.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    COMPRESS_PROMPT
        .replace(
            "{DIARY_LIMIT}",
            &crate::resource_manager::LIVE2D_DIARY_LIMIT.to_string(),
        )
        .replace("{DIARY_BLOCK_START}", DIARY_BLOCK_START)
        .replace("{DIARY_BLOCK_END}", DIARY_BLOCK_END)
        .replace("{DIARY_LIST}", &diary_list)
}

/// 把历史对话压缩为摘要，并解析压缩助手对日记本的更新。
/// `diaries` 为当前日记本条目，按写入顺序排列。
pub async fn compress_conversation(
    config: &Live2dChatConfig,
    messages: &[Value],
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
    cancellation: Option<&CancellationToken>,
) -> ChatResult<CompressOutcome> {
    let mut compress_prompt = build_compress_prompt(diaries);
    for summary in compression_summaries(messages) {
        compress_prompt.push_str("\n\n");
        compress_prompt.push_str(&summary);
    }
    // 压缩指令作为最后一条 user 消息，与正常聊天时让看板娘总结保持一致：
    // 实测 deepseek 系列对 system 位置的任务指令执行不稳定，user 请求则稳定执行。
    let mut compress_messages: Vec<Value> = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
        .cloned()
        .collect();
    sanitize_tool_call_sequences(&mut compress_messages);
    compress_messages.push(serde_json::json!({
        "role": "user",
        "content": compress_prompt
    }));
    // 请求参数与主对话保持一致：thinking 跟随配置并带上 reasoning_effort。
    let thinking_enabled = config.thinking_level != "none";
    let mut payload = serde_json::json!({
        "model": config.model,
        "messages": compress_messages,
        "thinking": {"type": if thinking_enabled { "enabled" } else { "disabled" }},
        "stream": false
    });
    if thinking_enabled {
        payload["reasoning_effort"] = Value::String(config.thinking_level.clone());
    }
    let body = send_chat_request(config, payload, cancellation).await?;
    let response: ChatCompletionResponse = serde_json::from_slice(&body)?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or("AI服务没有返回有效消息")?;
    let raw = choice
        .message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or("AI服务没有返回压缩摘要")?
        .to_owned();
    let (summary, diary) = parse_diary_block(&raw)?;
    Ok(CompressOutcome { summary, diary })
}

/// 从压缩结果中提取 <<<DIARY>>> 块，并从正文中移除；解析失败时忽略日记部分。
fn parse_diary_block(raw: &str) -> ChatResult<(String, DiaryUpdate)> {
    let Some(start_pos) = raw.find(DIARY_BLOCK_START) else {
        return Ok((raw.to_owned(), DiaryUpdate::default()));
    };
    let block_start = start_pos + DIARY_BLOCK_START.len();
    let block_end = raw[block_start..]
        .find(DIARY_BLOCK_END)
        .map(|pos| block_start + pos)
        .unwrap_or(raw.len());
    let block = raw[block_start..block_end].trim();
    let after_block = raw.len().min(block_end + DIARY_BLOCK_END.len());
    let summary = format!("{}\n{}", raw[..start_pos].trim(), raw[after_block..].trim())
        .trim()
        .to_owned();
    let mut diary = DiaryUpdate::default();
    let Ok(mut parsed) = serde_json::from_str::<DiaryUpdate>(block) else {
        return Ok((summary, diary));
    };
    let mut kept_entries = Vec::new();
    for entry in parsed.add.drain(..) {
        let title = entry.title.trim();
        let content = entry.content.trim();
        if title.is_empty() || content.is_empty() {
            continue;
        }
        kept_entries.push(DiaryEntry {
            title: title.to_owned(),
            content: content.to_owned(),
        });
    }
    diary.add = kept_entries;
    diary.delete = parsed.delete.drain(..).take(16).collect();
    diary
        .delete
        .retain(|key| !key.trim().is_empty() && key.chars().count() <= 64);
    Ok((summary, diary))
}

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

async fn request_step(
    config: &Live2dChatConfig,
    mut messages: Vec<Value>,
) -> ChatResult<AgentStep> {
    if !config.enabled {
        return Err("看板娘对话尚未启用，请先打开对话设置".into());
    }
    consolidate_system_messages(&mut messages);
    let payload = build_request_payload(config, &messages, false);
    let body = send_chat_request(config, payload, None).await?;
    parse_step(&body, messages)
}

/// 流式对话事件：AI 生成过程中的增量内容。
#[derive(Debug)]
pub enum StreamEvent {
    /// 思考内容增量
    Reasoning(String),
    /// 回答内容增量
    Content(String),
    /// 工具调用增量：index 为并行调用序号，name 为当前已知函数名，
    /// arguments 为该次增量后累积的参数（前端直接覆盖显示即可）。
    ToolCall {
        index: usize,
        name: String,
        arguments: String,
    },
}

pub async fn stream_start_agent(
    config: &Live2dChatConfig,
    history: &[ChatMessage],
    message: &str,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
) -> ChatResult<AgentStep> {
    let messages = build_messages(config, history, message, diaries)?;
    stream_step(config, messages, tx, cancellation).await
}

pub async fn stream_continue_conversation(
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

pub async fn stream_continue_agent(
    config: &Live2dChatConfig,
    messages: &[Value],
    tool_results: &[(String, String)],
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
    cancellation: &CancellationToken,
) -> ChatResult<AgentStep> {
    let mut messages = messages.to_vec();
    append_tool_results(&mut messages, tool_results)?;
    stream_step(config, messages, tx, cancellation).await
}

const STREAM_EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_STREAM_TOOL_CALLS: usize = 32;
const MAX_STREAM_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;

fn drain_sse_lines(buffer: &mut Vec<u8>, flush: bool) -> ChatResult<Vec<String>> {
    let mut lines = Vec::new();
    while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
        let mut line = buffer.drain(..=position).collect::<Vec<_>>();
        line.pop();
        if line.last() == Some(&b'\r') {
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

fn validate_stream_completion(saw_done: bool, finish_reason: Option<&str>) -> ChatResult<()> {
    if saw_done || finish_reason.is_some() {
        Ok(())
    } else {
        Err("AI服务流式响应意外结束，未收到 [DONE] 或结束原因".into())
    }
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
    if !config.enabled {
        return Err("看板娘对话尚未启用，请先打开对话设置".into());
    }
    consolidate_system_messages(&mut messages);
    let payload = build_request_payload(config, &messages, true);
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
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut reasoning = String::new();
    let mut content = String::new();
    // (index, id, name, arguments)
    let mut tool_calls: Vec<(usize, String, String, String)> = Vec::new();
    let mut finish_reason: Option<String> = None;
    let mut saw_done = false;
    'outer: loop {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        let next_chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            next = stream.next() => next,
        };
        let reached_eof = next_chunk.is_none();
        if let Some(chunk) = next_chunk {
            buffer.extend_from_slice(&chunk?);
        }
        for line in drain_sse_lines(&mut buffer, reached_eof)? {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                saw_done = true;
                break 'outer;
            }
            let value = serde_json::from_str::<Value>(data)
                .map_err(|error| format!("AI服务返回了无效的 SSE JSON: {error}"))?;
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
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    content.push_str(text);
                    send_stream_event(&tx, StreamEvent::Content(text.to_owned()), cancellation)
                        .await?;
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    if index >= MAX_STREAM_TOOL_CALLS {
                        return Err("AI服务返回了过多的工具调用".into());
                    }
                    while tool_calls.len() <= index {
                        tool_calls.push((index, String::new(), String::new(), String::new()));
                    }
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        tool_calls[index].1 = id.to_owned();
                    }
                    if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                        tool_calls[index].2 = name.to_owned();
                    }
                    if let Some(args) = call.pointer("/function/arguments").and_then(Value::as_str)
                    {
                        if tool_calls[index].3.len() + args.len() > MAX_STREAM_TOOL_ARGUMENT_BYTES {
                            return Err("AI工具调用参数过长".into());
                        }
                        tool_calls[index].3.push_str(args);
                    }
                    // 流式推送工具调用增量：前端可实时看到函数名与参数逐字生成
                    send_stream_event(
                        &tx,
                        StreamEvent::ToolCall {
                            index,
                            name: tool_calls[index].2.clone(),
                            arguments: tool_calls[index].3.clone(),
                        },
                        cancellation,
                    )
                    .await?;
                }
            }
        }
        if reached_eof {
            break;
        }
    }
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    validate_stream_completion(saw_done, finish_reason.as_deref())?;
    let mut message = serde_json::json!({ "role": "assistant" });
    message["content"] = if content.is_empty() {
        Value::Null
    } else {
        Value::String(content)
    };
    if !reasoning.is_empty() {
        message["reasoning_content"] = Value::String(reasoning);
    }
    let calls_value: Vec<Value> = tool_calls
        .iter()
        .filter(|(_, id, name, _)| !id.is_empty() && !name.is_empty())
        .map(|(_, id, name, arguments)| {
            serde_json::json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": arguments }
            })
        })
        .collect();
    if !calls_value.is_empty() {
        message["tool_calls"] = Value::Array(calls_value);
    }
    message["timestamp"] = serde_json::json!(now_timestamp());
    build_step(message, finish_reason, messages)
}

async fn send_chat_request(
    config: &Live2dChatConfig,
    payload: Value,
    cancellation: Option<&CancellationToken>,
) -> ChatResult<Vec<u8>> {
    let payload = serde_json::to_vec(&payload)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()?;
    let mut request = client
        .post(config.base_url.trim())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, "redreply-live2d-chat")
        .body(payload);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }

    let response = if let Some(cancellation) = cancellation {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            response = request.send() => response?,
        }
    } else {
        request.send().await?
    };
    let status = response.status();
    let body = if let Some(cancellation) = cancellation {
        if cancellation.is_cancelled() {
            return Err("客户端已断开，对话已取消，未保存任何内容".into());
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err("客户端已断开，对话已取消，未保存任何内容".into());
            }
            body = response.bytes() => body?,
        }
    } else {
        response.bytes().await?
    };
    if !status.is_success() {
        let detail = serde_json::from_slice::<Value>(&body)
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
        return Err(format!("AI服务返回 {status}: {detail}").into());
    }
    Ok(body.to_vec())
}

/// 发送流式（SSE）对话请求。流式响应可能持续较长时间，总超时放宽到
/// `timeout_seconds` 与 300 秒中的较大值。
async fn send_stream_request(
    config: &Live2dChatConfig,
    payload: Value,
    cancellation: &CancellationToken,
) -> ChatResult<reqwest::Response> {
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消，未保存任何内容".into());
    }
    let payload = serde_json::to_vec(&payload)?;
    let timeout_seconds = config.timeout_seconds.max(300);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(timeout_seconds))
        .build()?;
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

fn build_messages(
    config: &Live2dChatConfig,
    history: &[ChatMessage],
    message: &str,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
) -> ChatResult<Vec<Value>> {
    let message = message.trim();
    if message.is_empty() {
        return Err("消息不能为空".into());
    }
    for item in history {
        if !matches!(item.role.as_str(), "user" | "assistant") {
            return Err("历史消息角色不合法".into());
        }
        if item.content.chars().count() == 0 {
            return Err("历史消息内容不合法".into());
        }
    }

    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.extend(history.iter().map(|item| {
        serde_json::json!({
            "role": item.role,
            "content": item.content
        })
    }));
    ensure_agent_system_message(&mut messages, config, diaries);
    messages.push(serde_json::json!({
        "role": "user",
        "content": message,
        "timestamp": now_timestamp()
    }));
    Ok(messages)
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

fn javascript_tool_definition() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": JAVASCRIPT_TOOL_NAME,
            "description": "在当前浏览器页面主线程执行任意 JavaScript，并把 return 值返回给你。可用于读取页面状态、操作 DOM 或调用页面函数。",
            "parameters": {
                "type": "object",
                "properties": {
                    "javascript": {
                        "type": "string",
                        "description": "AsyncFunction 函数体形式的 JavaScript。可使用 await 和参数 signal（AbortSignal），并使用 return 返回执行结果。"
                    }
                },
                "required": ["javascript"],
                "additionalProperties": false
            }
        }
    })
}

fn build_request_payload(config: &Live2dChatConfig, messages: &[Value], stream: bool) -> Value {
    let thinking_enabled = config.thinking_level != "none";
    let mut messages = messages.to_vec();
    consolidate_system_messages(&mut messages);
    sanitize_tool_call_sequences(&mut messages);
    let mut payload = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "tools": [javascript_tool_definition()],
        "tool_choice": "auto",
        "thinking": {
            "type": if thinking_enabled { "enabled" } else { "disabled" }
        },
        "stream": stream
    });
    if thinking_enabled {
        payload["reasoning_effort"] = Value::String(config.thinking_level.clone());
    }
    payload
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

fn parse_step(body: &[u8], messages: Vec<Value>) -> ChatResult<AgentStep> {
    let response: ChatCompletionResponse = serde_json::from_slice(body)?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or("AI服务没有返回有效消息")?;
    let finish_reason = choice.finish_reason;
    let mut message = choice.message;
    message["timestamp"] = serde_json::json!(now_timestamp());
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err("AI服务返回的消息角色无效".into());
    }
    build_step(message, finish_reason, messages)
}

/// 把常见的非正常结束原因映射成明确的中文错误提示；
/// 未识别的原样拼入错误信息，方便排查服务商特有的结束原因。
fn finish_reason_error_message(reason: &str) -> String {
    match reason {
        "length" => "AI服务响应因长度限制被截断，回答不完整".to_owned(),
        "max_tokens" => "AI服务响应达到最大 token 限制被截断，回答不完整".to_owned(),
        "content_filter" => "AI服务响应被内容过滤器截断".to_owned(),
        "sensitive_words" => "AI服务响应触发敏感词审核被中断，请调整提问内容".to_owned(),
        "insufficient_system_resource" => {
            "AI服务端资源不足，响应被中断，请稍后重试".to_owned()
        }
        "network_disconnected" => "AI服务连接中断，响应未完成，请稍后重试".to_owned(),
        "abort" | "cancelled" | "canceled" => "AI服务端取消了本次响应，请重试".to_owned(),
        "chat_interrupted" => "AI服务响应被中断，请重新提问".to_owned(),
        other => format!("AI服务返回了未预期的结束原因: {other}，回答未正常完成"),
    }
}

/// 根据完整的 assistant 消息与结束原因组装 AgentStep（流式与非流式共用）。
fn build_step(
    message: Value,
    finish_reason: Option<String>,
    mut messages: Vec<Value>,
) -> ChatResult<AgentStep> {
    match finish_reason.as_deref() {
        Some(reason @ ("length" | "max_tokens" | "content_filter" | "sensitive_words"
        | "insufficient_system_resource" | "network_disconnected" | "abort"
        | "cancelled" | "canceled" | "chat_interrupted")) => {
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
        messages.push(message);
        return Ok(AgentStep::ToolCalls {
            calls,
            messages,
            reasoning_content,
            content,
        });
    }
    // 以下为最终回答路径。
    // 部分中转/推理服务会以 tool_calls 收尾却未携带任何工具调用内容：
    // 此时若已有正文则按最终回答继续，正文为空则视为异常。
    let reason = finish_reason.as_deref();
    if reason == Some("tool_calls") {
        let has_text = content
            .as_deref()
            .map(str::trim)
            .is_some_and(|text| !text.is_empty());
        if !has_text {
            return Err("AI服务声明了工具调用但未返回任何工具调用内容，请重试".into());
        }
    } else if !matches!(reason, None | Some("stop") | Some("eos")) {
        return Err(finish_reason_error_message(reason.unwrap_or("未知")).into());
    }

    let reply = content
        .as_deref()
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or("AI服务没有返回有效文本")?
        .to_owned();
    messages.push(message);
    Ok(AgentStep::Final {
        reply,
        reasoning_content,
        messages,
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
    for tool_call in tool_calls {
        if tool_call.get("type").and_then(Value::as_str) != Some("function") {
            return Err("AI工具调用类型无效".into());
        }
        let id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 200)
            .ok_or("AI工具调用 ID 无效")?;
        if !ids.insert(id.to_owned()) {
            return Err("AI工具调用 ID 重复".into());
        }
        let function = tool_call.get("function").ok_or("AI工具调用缺少函数")?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or("AI工具名称无效")?;
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .ok_or("AI工具参数无效")?;
        match name {
            JAVASCRIPT_TOOL_NAME => {
                let arguments: JavascriptToolArguments = serde_json::from_str(arguments)?;
                calls.push(AgentToolCall::Javascript {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    javascript: arguments.javascript,
                });
            }
            _ => {
                // 未知工具调用不中断对话：前端无法执行时结果缺失，
                // validate 会自动补占位失败说明，模型可自行纠正。
                calls.push(AgentToolCall::Unknown {
                    id: id.to_owned(),
                    name: name.to_owned(),
                });
            }
        }
    }
    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::{
        append_tool_results, build_compress_prompt, build_messages, build_request_payload,
        drain_sse_lines, estimate_context_tokens, finish_reason_error_message, parse_diary_block,
        parse_step, parse_tool_calls, sanitize_tool_call_sequences, send_stream_event,
        validate_stream_completion, AgentStep, AgentToolCall, ChatMessage, StreamEvent,
        JAVASCRIPT_TOOL_NAME,
    };
    use crate::config_manager::Live2dChatConfig;
    use tokio_util::sync::CancellationToken;

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
    fn parse_step_tolerates_tool_calls_finish_without_calls_when_text_present() {
        let body = serde_json::json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {"role": "assistant", "content": "好的，我直接回答你"}
            }]
        })
        .to_string()
        .into_bytes();
        let step = parse_step(&body, Vec::new()).unwrap();
        match step {
            AgentStep::Final { reply, .. } => assert_eq!(reply, "好的，我直接回答你"),
            other => panic!("应为最终回答，实际: {other:?}"),
        }
    }

    #[test]
    fn parse_step_rejects_tool_calls_finish_without_calls_and_text() {
        let body = br#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null}}]}"#;
        let error = parse_step(body, Vec::new()).unwrap_err().to_string();
        assert!(error.contains("工具调用"), "错误信息应说明工具调用缺失: {error}");
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
        assert!(validate_stream_completion(false, None).is_err());
        assert!(validate_stream_completion(true, None).is_ok());
        assert!(validate_stream_completion(false, Some("stop")).is_ok());
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
    fn build_messages_injects_agent_instruction() {
        let config = Live2dChatConfig {
            enabled: true,
            ..Live2dChatConfig::default()
        };
        let history = vec![ChatMessage {
            role: "assistant".to_owned(),
            content: "你好".to_owned(),
        }];
        let messages = build_messages(&config, &history, "你是谁？", &[]).unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains(JAVASCRIPT_TOOL_NAME));
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "system")
                .count(),
            1
        );
        assert_eq!(messages[1]["content"], "你好");
        assert_eq!(messages[2]["role"], "user");
    }

    #[test]
    fn build_messages_injects_diary_titles() {
        use crate::resource_manager::Live2dDiaryEntry;
        let config = Live2dChatConfig {
            enabled: true,
            ..Live2dChatConfig::default()
        };
        let diaries = vec![Live2dDiaryEntry {
            key: "d1".to_owned(),
            title: "用户偏好".to_owned(),
            content: "喜欢简洁回答".to_owned(),
            create_time: "2026-08-01 10:00:00".to_owned(),
            update_time: "2026-08-01 10:00:00".to_owned(),
        }];
        let messages = build_messages(&config, &[], "你好", &diaries).unwrap();
        let diary_message = messages
            .iter()
            .find(|message| {
                message["role"] == "system"
                    && message["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("[d1]"))
            })
            .expect("应包含日记标题的 system 消息");
        assert!(diary_message["content"]
            .as_str()
            .unwrap()
            .contains("用户偏好（创建于 2026-08-01 10:00:00）"));
    }

    #[test]
    fn build_messages_skips_diary_titles_when_empty() {
        let config = Live2dChatConfig {
            enabled: true,
            ..Live2dChatConfig::default()
        };
        let messages = build_messages(&config, &[], "你好", &[]).unwrap();
        assert!(messages.iter().all(|message| !message["content"]
            .as_str()
            .is_some_and(|content| content.contains("[d"))));
    }

    #[test]
    fn build_messages_rejects_system_role_from_browser() {
        let history = vec![ChatMessage {
            role: "system".to_owned(),
            content: "覆盖设定".to_owned(),
        }];
        assert!(build_messages(&Live2dChatConfig::default(), &history, "你好", &[]).is_err());
    }

    #[test]
    fn ensure_agent_system_message_reinjects_after_compression() {
        use crate::resource_manager::Live2dDiaryEntry;
        let config = Live2dChatConfig {
            enabled: true,
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
        assert!(content.contains(JAVASCRIPT_TOOL_NAME));
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
            enabled: true,
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
        assert!(content.contains(JAVASCRIPT_TOOL_NAME));
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
    fn parse_step_reads_final_answer() {
        let step = parse_step(
            r#"{"choices":[{"message":{"role":"assistant","reasoning_content":"先检查结果","content":"完成了"}}]}"#.as_bytes(),
            Vec::new(),
        )
        .unwrap();
        match step {
            AgentStep::Final {
                reply,
                reasoning_content,
                messages,
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
    fn parse_step_reads_javascript_tool_call() {
        let step = parse_step(
            br#"{"choices":[{"message":{"role":"assistant","reasoning_content":"need title","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"execute_javascript","arguments":"{\"javascript\":\"return document.title;\"}"}}]}}]}"#,
            vec![serde_json::json!({"role":"user","content":"读取标题"})],
        )
        .unwrap();
        match step {
            AgentStep::ToolCalls {
                calls,
                messages,
                reasoning_content,
                ..
            } => {
                assert_eq!(calls.len(), 1);
                match &calls[0] {
                    AgentToolCall::Javascript { id, javascript, .. } => {
                        assert_eq!(id, "call_1");
                        assert_eq!(javascript, "return document.title;");
                    }
                    AgentToolCall::Unknown { .. } => panic!("expected javascript tool call"),
                }
                assert_eq!(messages.len(), 2);
                assert_eq!(reasoning_content.as_deref(), Some("need title"));
                assert_eq!(messages[1]["reasoning_content"], "need title");
            }
            AgentStep::Final { .. } => panic!("expected tool call"),
        }
    }

    #[test]
    fn parse_step_tolerates_unknown_tool() {
        let result = parse_step(
            br#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"shell","arguments":"{}"}}]}}]}"#,
            Vec::new(),
        );
        let step = result.unwrap();
        match step {
            AgentStep::ToolCalls { calls, .. } => {
                assert_eq!(calls.len(), 1);
                match &calls[0] {
                    AgentToolCall::Unknown { name, .. } => assert_eq!(name, "shell"),
                    _ => panic!("expected unknown tool call"),
                }
            }
            _ => panic!("expected tool calls step"),
        }
    }

    #[test]
    fn parse_unknown_tool_call_is_tolerated() {
        let message = serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_x",
                "type": "function",
                "function": {
                    "name": "run_lua",
                    "arguments": "{\"code\":\"return 1\"}"
                }
            }]
        });
        let calls = parse_tool_calls(&message).unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            AgentToolCall::Unknown { name, .. } => assert_eq!(name, "run_lua"),
            _ => panic!("expected unknown tool call"),
        }
    }

    #[test]
    fn parse_step_rejects_truncated_response() {
        let result = parse_step(
            br#"{"choices":[{"finish_reason":"length","message":{"role":"assistant","content":"partial"}}]}"#,
            Vec::new(),
        );
        assert!(result.is_err());
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
    fn request_payload_controls_thinking_level() {
        let mut config = Live2dChatConfig {
            enabled: true,
            thinking_level: "max".to_owned(),
            ..Live2dChatConfig::default()
        };
        let payload = build_request_payload(&config, &[], false);
        assert_eq!(payload["thinking"]["type"], "enabled");
        assert_eq!(payload["reasoning_effort"], "max");

        config.thinking_level = "none".to_owned();
        let payload = build_request_payload(&config, &[], false);
        assert_eq!(payload["thinking"]["type"], "disabled");
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn estimates_cjk_and_ascii_tokens() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "你好世界"}),
            serde_json::json!({"role": "assistant", "content": "hello world!"}),
        ];
        let tokens = estimate_context_tokens(&messages);
        assert_eq!(tokens, (4 + 4) + ((12 + 3) / 4 + 4));
    }

    #[test]
    fn compression_prompt_contains_diary_rules_and_list() {
        use crate::resource_manager::Live2dDiaryEntry;
        let diaries = vec![
            Live2dDiaryEntry {
                key: "d1".to_owned(),
                title: "用户偏好".to_owned(),
                content: "喜欢简洁回答".to_owned(),
                create_time: "2026-08-01 10:00:00".to_owned(),
                update_time: "2026-08-01 10:00:00".to_owned(),
            },
            Live2dDiaryEntry {
                key: "d2".to_owned(),
                title: "重大事件".to_owned(),
                content: "某日完成了部署".to_owned(),
                create_time: "2026-08-02 09:00:00".to_owned(),
                update_time: String::new(),
            },
        ];
        let prompt = build_compress_prompt(&diaries);
        assert!(prompt.contains("压缩"));
        assert!(prompt.contains("日记本"));
        assert!(prompt.contains("<<<DIARY>>>"));
        assert!(prompt.contains(
            "KEY: d1 | 标题: 用户偏好 | 创建时间: 2026-08-01 10:00:00 | 内容: 喜欢简洁回答"
        ));
        assert!(prompt.contains(
            "KEY: d2 | 标题: 重大事件 | 创建时间: 2026-08-02 09:00:00 | 内容: 某日完成了部署"
        ));

        let empty_prompt = build_compress_prompt(&[]);
        assert!(empty_prompt.contains("(空)"));
    }

    #[test]
    fn parse_diary_block_extracts_and_removes_block() {
        let raw = "这是摘要内容。\n<<<DIARY>>>{\"add\":[{\"title\":\"偏好\",\"content\":\"喜欢中文\"}],\"delete\":[\"d1\"]}<<<DIARY_END>>>\n结束。";
        let (summary, diary) = parse_diary_block(raw).unwrap();
        assert_eq!(summary, "这是摘要内容。\n结束。");
        assert_eq!(diary.add.len(), 1);
        assert_eq!(diary.add[0].title, "偏好");
        assert_eq!(diary.add[0].content, "喜欢中文");
        assert_eq!(diary.delete, vec!["d1".to_owned()]);
    }

    #[test]
    fn parse_diary_block_ignores_invalid_json() {
        let raw = "摘要内容\n<<<DIARY>>>不是JSON<<<DIARY_END>>>";
        let (summary, diary) = parse_diary_block(raw).unwrap();
        assert_eq!(summary, "摘要内容");
        assert!(diary.is_empty());
    }

    #[test]
    fn parse_diary_block_rejects_empty_entries() {
        let long_title = "长".repeat(101);
        let raw = format!(
            "摘要\n<<<DIARY>>>{{\"add\":[{{\"title\":\"{long_title}\",\"content\":\"x\"}},{{\"title\":\"\",\"content\":\"y\"}},{{\"title\":\"正常\",\"content\":\"内容\"}}]}}<<<DIARY_END>>>"
        );
        let (_summary, diary) = parse_diary_block(&raw).unwrap();
        assert_eq!(diary.add.len(), 2);
        assert_eq!(diary.add[0].title, long_title);
        assert_eq!(diary.add[1].title, "正常");
    }

    #[test]
    fn parse_diary_block_returns_plain_summary_without_block() {
        let (summary, diary) = parse_diary_block("只有摘要").unwrap();
        assert_eq!(summary, "只有摘要");
        assert!(diary.is_empty());
    }
}
