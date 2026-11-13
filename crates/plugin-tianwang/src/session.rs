//! 会话级内存上下文：记录全部消息、串行回复、后台分块摘要。
//!
//! 每个会话（群/私聊）一个 actor：
//! - 未触发 AI 的消息也会进入消息链；
//! - 同一会话同时最多一个回复任务，触发消息在回复期间排队；
//! - 摘要只压缩近期窗口之外的旧消息，分块进行，避免摘要请求本身超上下文；
//! - 摘要失败不会阻塞回复：主请求始终只带「摘要 + 最近窗口」。
//!
//! 当前不持久化，进程重启后清空。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::ChatConfig;
use crate::{
    cached_config, chat_dialog_with_messages, log_info, log_warn, send_reply_text, value_to_string,
    ReplySink, SUMMARY_INSTRUCTION, SUMMARY_PREFIX,
};
use redreply_bot::Event;

const MAX_SEEN_MESSAGES: usize = 2_000;
const MAX_CHAIN_MESSAGES: usize = 20_000;
const MAX_SUMMARY_CHARS: usize = 4_000;

#[derive(Clone, Debug)]
struct ChainMsg {
    role: &'static str,
    content: String,
    /// assistant/tool 等需要原样回传给模型的消息（含 reasoning_content/tool_calls）。
    openai: Option<Value>,
}

#[derive(Clone)]
struct ReplyRequest {
    platform: String,
    self_id: String,
    message_type: String,
    group_id: Option<Value>,
    user_id: Option<Value>,
    passive_id: String,
    remote_id: String,
    turn_id: u64,
}

impl ReplyRequest {
    fn from_event(event: &Event, turn_id: u64) -> Self {
        let passive_id = event
            .get("message_id")
            .map(value_to_string)
            .unwrap_or_default();
        let remote_id = event
            .get_str("mqtt_client_id")
            .unwrap_or_default()
            .to_owned();
        Self {
            platform: event.platform.clone(),
            self_id: event.self_id.clone(),
            message_type: event.message_type.clone().unwrap_or_default(),
            group_id: event.get("group_id").cloned(),
            user_id: event.get("user_id").cloned(),
            passive_id,
            remote_id,
            turn_id,
        }
    }
}

struct ReplyOutcome {
    reply: String,
    suppressed: bool,
    generated: Vec<Value>,
    reasoning: String,
}

enum Command {
    Message {
        event: Event,
        triggered: bool,
    },
    ReplyDone {
        generation: u64,
        outcome: Result<ReplyOutcome, String>,
    },
    CompressionDone {
        generation: u64,
        epoch: u64,
        result: Result<Option<(usize, String)>, String>,
    },
}

struct Actor {
    key: String,
    tx: mpsc::UnboundedSender<Command>,
    chain: Vec<ChainMsg>,
    summary: Option<String>,

    replying: bool,
    pending_reply: Option<ReplyRequest>,
    reply_generation: u64,

    compressing: bool,
    compression_pending: bool,
    compression_generation: u64,
    trim_epoch: u64,

    seen_messages: HashSet<String>,
    seen_order: VecDeque<String>,
}

impl Actor {
    fn new(key: String, tx: mpsc::UnboundedSender<Command>) -> Self {
        Self {
            key,
            tx,
            chain: Vec::new(),
            summary: None,
            replying: false,
            pending_reply: None,
            reply_generation: 0,
            compressing: false,
            compression_pending: false,
            compression_generation: 0,
            trim_epoch: 0,
            seen_messages: HashSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Command>) {
        while let Some(command) = rx.recv().await {
            match command {
                Command::Message { event, triggered } => self.on_message(event, triggered),
                Command::ReplyDone {
                    generation,
                    outcome,
                } => self.on_reply_done(generation, outcome),
                Command::CompressionDone {
                    generation,
                    epoch,
                    result,
                } => self.on_compression_done(generation, epoch, result),
            }
        }
    }

    fn on_message(&mut self, event: Event, triggered: bool) {
        if self.is_duplicate(&event) {
            return;
        }
        let content = format_context_message(&event);
        let pending_request = if triggered {
            let config = cached_config();
            let turn_id = crate::calls::begin_turn(&self.key, &config.model, &content);
            Some(ReplyRequest::from_event(&event, turn_id))
        } else {
            None
        };
        self.push_chain(ChainMsg {
            role: "user",
            content,
            openai: None,
        });

        if let Some(request) = pending_request {
            if self.replying {
                self.pending_reply = Some(request);
            } else {
                self.start_reply(request);
            }
        }
        self.maybe_start_compression();
    }

    fn message_key(event: &Event) -> Option<String> {
        let message_id = event.get("message_id").map(value_to_string)?;
        if message_id.is_empty() {
            return None;
        }
        Some(format!(
            "{}:{}:{}",
            event.platform, event.self_id, message_id
        ))
    }

    fn is_duplicate(&mut self, event: &Event) -> bool {
        let Some(key) = Self::message_key(event) else {
            return false;
        };
        if self.seen_messages.contains(&key) {
            return true;
        }
        self.seen_messages.insert(key.clone());
        self.seen_order.push_back(key);
        while self.seen_order.len() > MAX_SEEN_MESSAGES {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen_messages.remove(&old);
            }
        }
        false
    }

    fn push_chain(&mut self, message: ChainMsg) {
        self.chain.push(message);
        if self.chain.len() > MAX_CHAIN_MESSAGES {
            let excess = self.chain.len() - MAX_CHAIN_MESSAGES;
            self.chain.drain(..excess);
            // 链被裁剪后，正在进行的压缩结果不能再按旧下标应用
            self.trim_epoch = self.trim_epoch.wrapping_add(1);
        }
    }

    fn start_reply(&mut self, request: ReplyRequest) {
        if self.chain.is_empty() {
            crate::calls::finish_turn(
                request.turn_id,
                None,
                Some("没有可回复的消息"),
                false,
            );
            return;
        }
        let config = cached_config();
        let mut messages = build_memory_messages(&config, &self.chain, self.summary.as_deref());
        if messages.is_empty() {
            crate::calls::finish_turn(
                request.turn_id,
                None,
                Some("没有可发送给模型的消息"),
                false,
            );
            return;
        }
        if messages.last().and_then(|item| item["role"].as_str()) != Some("user") {
            messages.push(json!({
                "role": "user",
                "content": "请根据以上群聊内容，回复最新的用户消息。"
            }));
        }

        self.reply_generation = self.reply_generation.wrapping_add(1);
        let generation = self.reply_generation;
        self.replying = true;

        let key = self.key.clone();
        let tx = self.tx.clone();
        let turn_id = request.turn_id;
        tokio::spawn(async move {
            let outcome = execute_reply(config, key, messages, request, turn_id).await;
            let _ = tx.send(Command::ReplyDone {
                generation,
                outcome,
            });
        });
    }

    fn on_reply_done(&mut self, generation: u64, outcome: Result<ReplyOutcome, String>) {
        if generation != self.reply_generation {
            return;
        }
        self.replying = false;

        match outcome {
            Ok(outcome) => {
                // 工具调用产生的 assistant/tool 中间消息必须原样留在上下文里，
                // 否则下一轮工具对话会丢失 reasoning_content 和 tool 结果。
                for message in outcome.generated {
                    let role = match message["role"].as_str() {
                        Some("tool") => "tool",
                        Some("assistant") => "assistant",
                        _ => "assistant",
                    };
                    let content = message["content"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_default();
                    self.push_chain(ChainMsg {
                        role,
                        content,
                        openai: Some(message),
                    });
                }
                if !outcome.suppressed && !outcome.reply.trim().is_empty() {
                    let mut message = json!({
                        "role": "assistant",
                        "content": outcome.reply.clone(),
                    });
                    if !outcome.reasoning.trim().is_empty() {
                        message["reasoning_content"] = json!(outcome.reasoning);
                    }
                    self.push_chain(ChainMsg {
                        role: "assistant",
                        content: outcome.reply,
                        openai: Some(message),
                    });
                }
            }
            Err(error) => {
                log_warn(format!("ChatGPT 会话 {} 回复失败:{error}", self.key));
            }
        }

        if let Some(request) = self.pending_reply.take() {
            self.start_reply(request);
        }
        self.maybe_start_compression();
    }

    fn maybe_start_compression(&mut self) {
        // 回复期间不压缩，避免把尚未回复的触发消息摘要掉；等回复结束后再压缩。
        if self.replying {
            self.compression_pending = true;
            return;
        }
        let config = cached_config();
        if !needs_compression(&config, &self.chain, self.summary.as_deref()) {
            self.compression_pending = false;
            return;
        }
        if self.compressing {
            self.compression_pending = true;
            return;
        }

        let generation = self.compression_generation.wrapping_add(1);
        self.compression_generation = generation;
        let epoch = self.trim_epoch;
        let key = self.key.clone();
        let tx = self.tx.clone();
        let chain = self.chain.clone();
        let summary = self.summary.clone();
        self.compressing = true;
        self.compression_pending = false;

        tokio::spawn(async move {
            let result = compress_chain(&config, &key, &chain, summary.as_deref()).await;
            let _ = tx.send(Command::CompressionDone {
                generation,
                epoch,
                result,
            });
        });
    }

    fn on_compression_done(
        &mut self,
        generation: u64,
        epoch: u64,
        result: Result<Option<(usize, String)>, String>,
    ) {
        if generation != self.compression_generation {
            return;
        }
        self.compressing = false;

        // 压缩期间消息链被裁剪过，旧下标失效，直接丢弃本次摘要
        if epoch == self.trim_epoch {
            match result {
                Ok(Some((covered, summary))) => {
                    let covered = covered.min(self.chain.len());
                    if covered > 0 {
                        self.chain.drain(..covered);
                        self.summary = Some(summary);
                        log_info(format!(
                            "ChatGPT 会话 {} 长期记忆已更新（压缩 {} 条消息）",
                            self.key, covered
                        ));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    log_warn(format!("ChatGPT 会话 {} 历史压缩失败:{error}", self.key));
                }
            }
        }

        if self.compression_pending {
            self.compression_pending = false;
            self.maybe_start_compression();
        }
    }
}

/// 把消息交给会话 actor；首次出现时创建 actor。
pub(crate) fn dispatch(session_key: String, event: Event, triggered: bool) {
    let tx = sender_for(&session_key);
    let _ = tx.send(Command::Message { event, triggered });
}

fn sender_for(session_key: &str) -> mpsc::UnboundedSender<Command> {
    static ACTORS: OnceLock<Mutex<HashMap<String, mpsc::UnboundedSender<Command>>>> =
        OnceLock::new();
    let mut actors = ACTORS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(sender) = actors.get(session_key) {
        return sender.clone();
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let actor = Actor::new(session_key.to_owned(), tx.clone());
    redlang::runtime().spawn(actor.run(rx));
    actors.insert(session_key.to_owned(), tx.clone());
    tx
}

async fn execute_reply(
    config: ChatConfig,
    session_key: String,
    messages: Vec<Value>,
    request: ReplyRequest,
    turn_id: u64,
) -> Result<ReplyOutcome, String> {
    let mut sink = ReplySink::for_event(
        &request.platform,
        &request.message_type,
        &request.self_id,
        &request.passive_id,
        &request.remote_id,
        request.user_id.as_ref(),
    );
    let mut full = String::new();
    let outcome = match chat_dialog_with_messages(
        &config,
        &session_key,
        messages,
        &mut full,
        &mut sink,
        Some(turn_id),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            log_warn(format!("ChatGPT 请求失败:{error}"));
            let tip = if config.send_error_tip {
                Some(format!("AI 请求失败：{error}"))
            } else {
                None
            };
            if let Some(text) = sink.finish_err(tip.as_deref()).await {
                send_reply_text(
                    &request.platform,
                    &request.self_id,
                    &request.passive_id,
                    &request.remote_id,
                    &request.message_type,
                    request.group_id.as_ref(),
                    request.user_id.as_ref(),
                    &text,
                )
                .await;
            }
            crate::calls::finish_turn(turn_id, None, Some(&error), false);
            return Err(error);
        }
    };

    let usage = outcome.usage;
    let generated = outcome.generated;
    let reasoning = outcome.reasoning;
    log_info(format!(
        "ChatGPT 会话 {session_key} 回复完成（prompt_tokens={}，cached_tokens={}，completion_tokens={}，total_tokens={}）",
        usage.prompt_tokens,
        usage.cached_tokens,
        usage.completion_tokens,
        usage.prompt_tokens + usage.completion_tokens
    ));

    if outcome.suppressed {
        crate::calls::finish_turn(turn_id, Some(&full), None, true);
        return Ok(ReplyOutcome {
            reply: String::new(),
            suppressed: true,
            generated,
            reasoning,
        });
    }

    if !sink.finish_ok().await {
        send_reply_text(
            &request.platform,
            &request.self_id,
            &request.passive_id,
            &request.remote_id,
            &request.message_type,
            request.group_id.as_ref(),
            request.user_id.as_ref(),
            &full,
        )
        .await;
    }

    crate::calls::finish_turn(turn_id, Some(&full), None, false);
    Ok(ReplyOutcome {
        reply: full,
        suppressed: false,
        generated,
        reasoning,
    })
}

fn format_context_message(event: &Event) -> String {
    serde_json::to_string(&event.raw).unwrap_or_default()
}

fn estimate_msg_tokens(message: &ChainMsg) -> usize {
    let text = match &message.openai {
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => message.content.clone(),
    };
    crate::tokenizer::count_text_tokens(&text).saturating_add(4)
}

fn trigger_tokens(config: &ChatConfig) -> usize {
    (config.max_tokens.saturating_mul(3) / 4).max(1)
}

fn target_tokens(config: &ChatConfig) -> usize {
    (config.max_tokens / 2).max(500)
}

fn chunk_tokens(config: &ChatConfig) -> usize {
    (config.max_tokens / 3).clamp(300, 32_000)
}

fn budget_after(summary: Option<&str>, system_tokens: usize, base: usize) -> usize {
    let summary_tokens = summary
        .map(crate::tokenizer::count_text_tokens)
        .unwrap_or(0);
    base.saturating_sub(summary_tokens)
        .saturating_sub(system_tokens)
}

/// 从完整消息链切出「近期窗口」与「冷区」，至少保留最后一条消息。
fn split_window(chain: &[ChainMsg], budget: usize) -> (Vec<ChainMsg>, Vec<ChainMsg>) {
    if chain.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut used = 0usize;
    let mut split_at = chain.len();
    for (index, message) in chain.iter().enumerate().rev() {
        let cost = estimate_msg_tokens(message);
        if index + 1 < chain.len() && used + cost > budget {
            break;
        }
        used = used.saturating_add(cost);
        split_at = index;
    }
    if split_at >= chain.len() {
        split_at = chain.len() - 1;
    }
    (chain[..split_at].to_vec(), chain[split_at..].to_vec())
}

fn needs_compression(config: &ChatConfig, chain: &[ChainMsg], summary: Option<&str>) -> bool {
    if chain.is_empty() {
        return false;
    }
    // 只用 trigger 预算扫描尾部：放得下就说明整条链都在窗口内，不需要压缩。
    let system_tokens = crate::tokenizer::count_text_tokens(&config.system_prompt);
    let recent_budget = budget_after(summary, system_tokens, trigger_tokens(config));
    let (cold, _) = split_window(chain, recent_budget);
    !cold.is_empty()
}

fn build_memory_messages(
    config: &ChatConfig,
    chain: &[ChainMsg],
    summary: Option<&str>,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(chain.len() + 2);
    if !config.system_prompt.trim().is_empty() {
        messages.push(json!({"role": "system", "content": config.system_prompt}));
    }
    if let Some(summary) = summary.filter(|text| !text.trim().is_empty()) {
        messages.push(json!({
            "role": "user",
            "content": format!("{SUMMARY_PREFIX}{}", summary.trim())
        }));
    }
    let system_tokens = crate::tokenizer::count_text_tokens(&config.system_prompt);
    let recent_budget = budget_after(summary, system_tokens, trigger_tokens(config));
    let (_, mut recent) = split_window(chain, recent_budget);
    if recent.is_empty() {
        if let Some(last) = chain.last() {
            recent.push(last.clone());
        }
    }
    append_chain_messages(&mut messages, &recent);
    messages
}

/// 把消息链里的 assistant/tool 结构原样还原；对窗口截断造成的缺失/孤儿
/// tool 消息做修补，避免 API 因 tool_calls 不完整而拒绝请求。
fn append_chain_messages(messages: &mut Vec<Value>, recent: &[ChainMsg]) {
    fn flush_missing(messages: &mut Vec<Value>, pending: &mut Vec<String>) {
        for id in pending.drain(..) {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": "工具结果缺失（上下文窗口截断）",
            }));
        }
    }

    let mut pending: Vec<String> = Vec::new();
    for message in recent {
        let Some(value) = &message.openai else {
            flush_missing(messages, &mut pending);
            messages.push(json!({"role": "user", "content": message.content}));
            continue;
        };
        match value["role"].as_str() {
            Some("assistant") => {
                flush_missing(messages, &mut pending);
                if let Some(calls) = value["tool_calls"].as_array() {
                    for call in calls {
                        if let Some(id) = call["id"].as_str().filter(|id| !id.is_empty()) {
                            pending.push(id.to_owned());
                        }
                    }
                }
                messages.push(value.clone());
            }
            Some("tool") => {
                let id = value["tool_call_id"].as_str().unwrap_or_default();
                if pending.iter().any(|pending_id| pending_id == id) {
                    pending.retain(|pending_id| pending_id != id);
                    messages.push(value.clone());
                } else {
                    // 孤立的 tool 消息（对应的 assistant tool_call 已被窗口截掉）
                    messages.push(json!({
                        "role": "user",
                        "content": format!("[工具结果] {}", value["content"].as_str().unwrap_or(""))
                    }));
                }
            }
            _ => {
                flush_missing(messages, &mut pending);
                messages.push(value.clone());
            }
        }
    }
    flush_missing(messages, &mut pending);
}

async fn compress_chain(
    config: &ChatConfig,
    session_key: &str,
    chain: &[ChainMsg],
    old_summary: Option<&str>,
) -> Result<Option<(usize, String)>, String> {
    let system_tokens = crate::tokenizer::count_text_tokens(&config.system_prompt);
    let recent_budget = budget_after(old_summary, system_tokens, target_tokens(config));
    let (cold, _) = split_window(chain, recent_budget);
    if cold.is_empty() {
        return Ok(None);
    }

    let mut summary = old_summary.unwrap_or_default().trim().to_owned();
    let mut covered = 0usize;
    let chunk_budget = chunk_tokens(config);
    while covered < cold.len() {
        let start = covered;
        let mut used = 0usize;
        while covered < cold.len() {
            let cost = estimate_msg_tokens(&cold[covered]).max(1);
            if covered > start && used + cost > chunk_budget {
                break;
            }
            used = used.saturating_add(cost);
            covered += 1;
        }
        let chunk = &cold[start..covered];
        let prompt = build_summary_prompt(&summary, chunk);
        // 摘要调用不需要工具，避免模型在后台摘要时误触发命令行执行
        let mut summary_config = config.clone();
        summary_config.command_enabled = false;
        if !summary_config.reasoning_effort.is_empty() {
            summary_config.reasoning_effort = "none".to_string();
        }
        let reply = crate::chat_once(
            &summary_config,
            &format!("{session_key}:summary"),
            &[],
            &prompt,
        )
        .await?;
        let text = reply.text.trim();
        if text.is_empty() {
            return Err("摘要模型返回空内容".to_owned());
        }
        let summary_limit = (config.max_tokens / 2).clamp(500, MAX_SUMMARY_CHARS);
        summary = text.chars().take(summary_limit).collect();
    }

    if covered == 0 {
        return Ok(None);
    }
    Ok(Some((covered, summary)))
}

fn build_summary_prompt(old_summary: &str, chunk: &[ChainMsg]) -> String {
    let mut out = String::from(SUMMARY_INSTRUCTION);
    out.push_str("\n\n");
    if !old_summary.trim().is_empty() {
        out.push_str("【已有长期记忆，请在此基础上增量更新，不要丢失重要信息】\n");
        out.push_str(old_summary.trim());
        out.push_str("\n\n");
    }
    out.push_str("【需要压缩的群聊历史】\n");
    for message in chunk {
        match &message.openai {
            Some(value) => out.push_str(&serde_json::to_string(value).unwrap_or_default()),
            None => {
                out.push_str(match message.role {
                    "assistant" => "AI：",
                    "tool" => "工具结果：",
                    _ => "用户事件：",
                });
                out.push_str(&message.content);
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &'static str, content: &str) -> ChainMsg {
        ChainMsg {
            role,
            content: content.to_owned(),
            openai: None,
        }
    }

    fn config(max_tokens: usize) -> ChatConfig {
        ChatConfig {
            max_tokens,
            system_prompt: "system".to_owned(),
            ..ChatConfig::default()
        }
    }

    #[test]
    fn split_window_keeps_last_message_even_with_zero_budget() {
        let chain = vec![message("user", "a"), message("user", "b")];
        let (cold, recent) = split_window(&chain, 0);
        assert_eq!(cold.len(), 1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "b");
    }

    #[test]
    fn small_chain_does_not_need_compression() {
        let chain = vec![message("user", "hello")];
        assert!(!needs_compression(&config(8_000), &chain, None));
    }

    #[test]
    fn long_chain_needs_compression_and_keeps_recent_window() {
        let chain = (0..400)
            .map(|index| message("user", &format!("第 {index} 条消息，{}", "内容".repeat(20))))
            .collect::<Vec<_>>();
        assert!(needs_compression(&config(8_000), &chain, None));
        let messages = build_memory_messages(&config(8_000), &chain, None);
        assert!(messages.len() < chain.len());
        assert_eq!(messages.last().unwrap()["role"], "user");
    }

    #[test]
    fn summary_is_included_before_recent_messages() {
        let chain = vec![message("user", "最近一条")];
        let messages = build_memory_messages(&config(8_000), &chain, Some("旧记忆"));
        assert!(messages
            .iter()
            .any(|item| item["content"].as_str().unwrap_or("").contains("旧记忆")));
        assert_eq!(messages.last().unwrap()["content"], "最近一条");
    }

    #[test]
    fn reply_keeps_intermediate_tool_messages_and_reasoning() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut actor = Actor::new("test".to_owned(), tx);
        let generated = vec![
            json!({
                "role": "assistant",
                "content": Value::Null,
                "reasoning_content": "先算一下",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "run_command", "arguments": "{}"}
                }]
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "42"
            }),
        ];
        actor.on_reply_done(
            0,
            Ok(ReplyOutcome {
                reply: "答案是42".to_owned(),
                suppressed: false,
                generated,
                reasoning: "最终思考".to_owned(),
            }),
        );

        assert_eq!(actor.chain.len(), 3);
        assert_eq!(
            actor.chain[0].openai.as_ref().unwrap()["reasoning_content"],
            "先算一下"
        );
        assert_eq!(actor.chain[1].openai.as_ref().unwrap()["role"], "tool");
        assert_eq!(actor.chain[1].openai.as_ref().unwrap()["content"], "42");
        assert_eq!(
            actor.chain[2].openai.as_ref().unwrap()["reasoning_content"],
            "最终思考"
        );
    }

    #[tokio::test]
    async fn untriggered_messages_are_recorded_without_starting_reply() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut actor = Actor::new("test".to_owned(), tx);
        let event = Event::from_json(json!({
            "platform": "onebot11",
            "self_id": "10001",
            "post_type": "message",
            "message_type": "group",
            "group_id": "20002",
            "user_id": "30003",
            "sender": {"nickname": "路人"},
            "message_id": "m1",
            "message": [{"type": "text", "data": {"text": "随便聊聊"}}]
        }))
        .unwrap();
        actor.on_message(event, false);
        assert_eq!(actor.chain.len(), 1);
        assert!(actor.chain[0].content.contains("随便聊聊"));
        assert!(!actor.replying);
        assert!(actor.pending_reply.is_none());
    }

    #[test]
    fn raw_event_json_keeps_image_info() {
        let event = Event::from_json(json!({
            "platform": "onebot11",
            "self_id": "10001",
            "post_type": "message",
            "message_type": "group",
            "group_id": "20002",
            "user_id": "30003",
            "sender": {"nickname": "路人"},
            "message_id": "m-image",
            "message": [{"type": "image", "data": {"url": "https://example.com/a.png"}}]
        }))
        .unwrap();
        let content = format_context_message(&event);
        assert!(content.contains("\"url\":\"https://example.com/a.png\""));
    }

    #[test]
    fn context_message_is_raw_json_with_sender() {
        let event = Event::from_json(json!({
            "platform": "onebot11",
            "self_id": "10001",
            "post_type": "message",
            "message_type": "group",
            "user_id": "20002",
            "sender": {"nickname": "小明"},
            "message": [{"type": "text", "data": {"text": "你好"}}]
        }))
        .unwrap();
        let content = format_context_message(&event);
        assert!(content.contains("小明"));
        assert!(content.contains("20002"));
        assert!(content.contains("你好"));
    }
}
