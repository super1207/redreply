//! 看板娘 Live2D 对话的 HTTP 处理（从 mod.rs 拆分）
//! 包含：会话管理、上下文存取、对话步骤响应等。
//!
//! 落库协议：对话的持久化全部由前端显式发起（POST /live2d_chat_append），
//! 后端 AI 流式调用本身不写库，保证「用户看到的」与「数据库里的」始终一致。

use crate::cqapi::cq_add_log_w;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::sync::OnceLock;
/// 统一执行一次阻塞 I/O 任务（消除 spawn_blocking 错误处理样板）。
async fn spawn_blocking_io<T, F>(label: &str, f: F) -> std::result::Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => {
            cq_add_log_w(&format!("{label}失败: {error}")).unwrap();
            Err(error.to_string())
        }
        Err(error) => {
            cq_add_log_w(&format!("{label}失败: {error}")).unwrap();
            Err(error.to_string())
        }
    }
}
use super::{json_api_response, BoxBody};

pub(crate) const LIVE2D_CHAT_BODY_LIMIT: usize = 64 * 1024;
pub(crate) const LIVE2D_CONVERSATION_BODY_LIMIT: usize = 256 * 1024;
/// 落库接口的请求体上限：工具调用参数不再限制长度后，
/// assistant(tool_calls) 消息随落库可达 MB 级（run_c 源码上限为 1MB）。
pub(crate) const LIVE2D_APPEND_BODY_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct Live2dChatRequest {
    #[serde(default)]
    pub(crate) message: Option<String>,
    #[serde(default)]
    pub(crate) generation: Option<String>,
    /// 前端「压缩」按钮显式携带的压缩意图标志：不预先压缩，
    /// 而是让看板娘在对话中可见地汇报压缩结果，完成后由前端用汇报替换旧历史。
    #[serde(default)]
    pub(crate) compress: bool,
}

/// 前端落库请求（POST /live2d_chat_append）。
/// op 取值：
/// - "append"：追加一条消息，可携带 generation 同写、clear_generation 同清
/// - "replace_last"：用新消息替换末尾消息（思考快照 → 完整回复的演进）
/// - "replace_all"：用 messages 整体替换对话
/// - "compress_summary"：用 reply 作为压缩摘要替换整个对话（保留一条 system 摘要）
/// - "clear"：清空对话与代次
#[derive(Deserialize)]
pub(crate) struct Live2dAppendRequest {
    pub(crate) op: String,
    #[serde(default)]
    pub(crate) message: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) messages: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub(crate) reply: Option<String>,
    #[serde(default)]
    pub(crate) generation: Option<String>,
    #[serde(default)]
    pub(crate) clear_generation: bool,
}

#[derive(Deserialize)]
pub(crate) struct Live2dCancelRequest {
    pub(crate) generation: String,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum Live2dConversationMutation {
    Update {
        index: usize,
        expected: serde_json::Value,
        message: serde_json::Value,
    },
    Delete {
        index: usize,
        expected: serde_json::Value,
    },
}

/// 单用户场景下的对话流程互斥锁：一次只处理一个对话请求，
/// 防止初始提问与工具结果提交并发交错导致状态错乱。
/// （落库由前端 append 接口独立发起，不经过此锁）
pub(crate) static LIVE2D_AGENT_FLOW_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
pub(crate) fn live2d_agent_flow_lock() -> &'static tokio::sync::Mutex<()> {
    LIVE2D_AGENT_FLOW_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) async fn get_live2d_agent_conversation(
) -> std::result::Result<Option<Vec<serde_json::Value>>, String> {
    // 每次调用都从数据库读取，保证记录与磁盘一致
    spawn_blocking_io(
        "读取看板娘对话",
        crate::resource_manager::load_live2d_chat_messages,
    )
    .await
}

pub(crate) async fn append_live2d_agent_message(
    message: serde_json::Value,
) -> std::result::Result<(), String> {
    spawn_blocking_io("追加看板娘对话", move || {
        crate::resource_manager::append_live2d_chat_message(&message)
    })
    .await
}

pub(crate) async fn replace_last_live2d_agent_message(
    message: serde_json::Value,
) -> std::result::Result<(), String> {
    spawn_blocking_io("更新看板娘对话", move || {
        crate::resource_manager::replace_live2d_chat_last_message(&message)
    })
    .await
}

pub(crate) async fn replace_all_live2d_agent_messages(
    messages: Vec<serde_json::Value>,
) -> std::result::Result<(), String> {
    spawn_blocking_io("替换看板娘对话", move || {
        crate::resource_manager::replace_all_live2d_chat_messages(&messages)
    })
    .await
}

pub(crate) async fn get_live2d_agent_generation() -> std::result::Result<Option<String>, String> {
    spawn_blocking_io(
        "读取看板娘对话代次",
        crate::resource_manager::load_live2d_chat_generation,
    )
    .await
}

pub(crate) async fn save_live2d_agent_generation(
    generation: Option<String>,
) -> std::result::Result<(), String> {
    spawn_blocking_io("保存看板娘对话代次", move || {
        crate::resource_manager::save_live2d_chat_generation(generation.as_deref())
    })
    .await
}

pub(crate) async fn clear_live2d_agent_conversation() -> std::result::Result<(), String> {
    spawn_blocking_io(
        "清除看板娘对话",
        crate::resource_manager::clear_live2d_chat_conversation,
    )
    .await
}

pub(crate) fn validate_live2d_conversation_message(
    message: &serde_json::Value,
) -> std::result::Result<(), String> {
    let object = message
        .as_object()
        .ok_or_else(|| "消息必须是 JSON 对象".to_owned())?;
    let role = object
        .get("role")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "消息缺少 role".to_owned())?;
    if !matches!(role, "system" | "user" | "assistant" | "tool") {
        return Err("role 只能是 system、user、assistant 或 tool".to_owned());
    }
    match object.get("content") {
        Some(serde_json::Value::String(_)) => {}
        Some(serde_json::Value::Null) if role == "assistant" => {}
        _ => return Err("content 必须是字符串，只有 assistant 消息可以为 null".to_owned()),
    }
    Ok(())
}

pub(crate) fn apply_live2d_conversation_mutation(
    messages: &mut Vec<serde_json::Value>,
    mutation: Live2dConversationMutation,
) -> std::result::Result<(), String> {
    match mutation {
        Live2dConversationMutation::Update {
            index,
            expected,
            message,
        } => {
            if index >= messages.len() {
                return Err("消息索引已失效，请刷新后重试".to_owned());
            }
            if messages[index] != expected {
                return Err("消息已发生变化，请刷新后重试".to_owned());
            }
            validate_live2d_conversation_message(&message)?;
            messages[index] = message;
        }
        Live2dConversationMutation::Delete { index, expected } => {
            if index >= messages.len() {
                return Err("消息索引已失效，请刷新后重试".to_owned());
            }
            if messages[index] != expected {
                return Err("消息已发生变化，请刷新后重试".to_owned());
            }
            messages.remove(index);
        }
    }
    Ok(())
}

pub(crate) async fn mutate_live2d_agent_conversation(
    mutation: Live2dConversationMutation,
) -> std::result::Result<Vec<serde_json::Value>, String> {
    let mut messages = get_live2d_agent_conversation().await?.unwrap_or_default();
    apply_live2d_conversation_mutation(&mut messages, mutation)?;
    replace_all_live2d_agent_messages(messages.clone()).await?;
    Ok(messages)
}

pub(crate) async fn live2d_agent_context_usage() -> (u64, bool) {
    match get_live2d_agent_conversation().await {
        Ok(Some(messages)) => crate::ai_client::stored_context_usage(&messages)
            .unwrap_or_else(|| (crate::ai_client::estimate_context_tokens(&messages), true)),
        _ => (0, true),
    }
}

#[derive(Deserialize)]
pub(crate) struct Live2dChatConfigUpdate {
    pub(crate) config: String,
    pub(crate) configs: Vec<crate::config_manager::Live2dProviderConfig>,
    pub(crate) system_prompt: String,
}

pub(crate) fn live2d_chat_config_response(
    config: &crate::config_manager::Live2dChatConfig,
    can_write: bool,
    context_used: u64,
    context_used_estimated: bool,
) -> hyper::Response<BoxBody> {
    json_api_response(json!({
        "retcode": 0,
        "data": {
            "config": config.config,
            "configs": config.configs,
            "system_prompt": config.system_prompt,
            "context_used": context_used,
            "context_used_estimated": context_used_estimated,
            "has_api_key": !config.api_key.is_empty(),
            "proxy": config.proxy,
            "can_write": can_write
        }
    }))
}

/// 判断对话当前阶段，用于刷新后恢复界面。
/// - "empty"：从未对话
/// - "completed"：最后一条是带内容的 assistant（一轮完整对话已完成）
/// - "interrupted"：最后一条是带 interrupted 标记的 assistant（主动停止 / AI 报错已落库）
/// - "tool_pending"：存在未执行完的工具调用（含"工具结果已落库但未继续生成"）
/// - "interrupted_stream"：最后一条是 user 或思考快照，说明提问/思考/回复中意外中断
pub(crate) fn live2d_agent_phase(messages: &[serde_json::Value]) -> &'static str {
    let Some(last) = messages.last() else {
        return "empty";
    };
    let role = last.get("role").and_then(serde_json::Value::as_str).unwrap_or("");
    match role {
        "assistant" => {
            if last.get("interrupted").is_some() {
                return "interrupted";
            }
            if last
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
            {
                return "tool_pending";
            }
            if last.get("content").and_then(serde_json::Value::as_str).is_some() {
                return "completed";
            }
            // 思考快照（content:null 且带 reasoning）说明停在思考完成后、回复未完成
            "interrupted_stream"
        }
        "tool" => "tool_pending",
        "user" => "interrupted_stream",
        _ => "interrupted_stream",
    }
}

/// 从尾向前找到最后一条 assistant(tool_calls)，返回其结果缺失的调用 ID 集合。
/// - Some(非空)：仍有工具结果未提交（悬挂的工具执行）
/// - Some(空集)：工具调用全部有结果（对话停在工具阶段，可继续生成）
/// - None：对话末尾没有 assistant(tool_calls)（无工具流程）
pub(crate) fn live2d_agent_pending_call_ids(
    messages: &[serde_json::Value],
) -> Option<HashSet<String>> {
    let mut done_ids: HashSet<String> = HashSet::new();
    for message in messages.iter().rev() {
        let role = message.get("role").and_then(serde_json::Value::as_str).unwrap_or("");
        if role == "assistant" {
            let calls = message.get("tool_calls").and_then(serde_json::Value::as_array);
            if calls.is_some_and(|calls| !calls.is_empty()) {
                let call_ids: HashSet<String> = calls
                    .unwrap()
                    .iter()
                    .filter_map(|call| {
                        call.get("id").and_then(serde_json::Value::as_str).map(str::to_owned)
                    })
                    .collect();
                return Some(call_ids.difference(&done_ids).cloned().collect());
            }
            // 最后一条 assistant 不带 tool_calls：对话已进入后续阶段，无工具流程
            return None;
        } else if role == "tool" {
            if let Some(id) = message.get("tool_call_id").and_then(serde_json::Value::as_str) {
                done_ids.insert(id.to_owned());
            }
        } else {
            return None;
        }
    }
    None
}

/// 返回待重跑工具的可执行调用列表（仅缺失结果的那些），供前端「继续执行」。
/// 每个元素为 { id, name, javascript | source_code }。
pub(crate) fn live2d_agent_pending_calls(
    messages: &[serde_json::Value],
) -> Option<Vec<serde_json::Value>> {
    let pending_ids = live2d_agent_pending_call_ids(messages)?;
    for message in messages.iter().rev() {
        let role = message.get("role").and_then(serde_json::Value::as_str).unwrap_or("");
        if role == "assistant" {
            let Some(calls) = message.get("tool_calls").and_then(serde_json::Value::as_array)
            else {
                return None;
            };
            if calls.is_empty() {
                return None;
            }
            let mut result = Vec::new();
            for call in calls {
                let Some(id) = call.get("id").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if !pending_ids.contains(id) {
                    continue;
                }
                let name = call
                    .pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let mut item = json!({ "id": id, "name": name });
                if let Some(args_raw) =
                    call.pointer("/function/arguments").and_then(serde_json::Value::as_str)
                {
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_raw) {
                        if let Some(js) = args.get("javascript").and_then(serde_json::Value::as_str)
                        {
                            item["javascript"] = js.to_owned().into();
                        }
                        if let Some(src) =
                            args.get("source_code").and_then(serde_json::Value::as_str)
                        {
                            item["source_code"] = src.to_owned().into();
                        }
                    }
                }
                result.push(item);
            }
            return Some(result);
        } else if role != "tool" {
            return None;
        }
    }
    None
}

/// 统计对话中已完成（含进行中）的 Agent 工具调用轮数，用于步骤序号显示。
pub(crate) fn live2d_agent_completed_steps(messages: &[serde_json::Value]) -> u64 {
    messages
        .iter()
        .filter(|message| {
            message.get("role").and_then(serde_json::Value::as_str) == Some("assistant")
                && message.get("tool_calls").is_some()
        })
        .count() as u64
}

pub(crate) async fn live2d_agent_has_pending_tools() -> std::result::Result<bool, String> {
    Ok(get_live2d_agent_conversation()
        .await?
        .as_ref()
        .and_then(|messages| live2d_agent_pending_call_ids(messages))
        .is_some_and(|pending| !pending.is_empty()))
}

/// 统一构造一个 Agent 步骤的 data JSON（done 事件体）。
/// 落库由前端发起，这里不再写库；assistant_message 携带完整消息供前端落库。
pub(crate) async fn live2d_agent_step_data(
    step: crate::ai_client::AgentStep,
    completed_steps: u64,
    context_length: u64,
) -> std::result::Result<serde_json::Value, String> {
    match step {
        crate::ai_client::AgentStep::Final {
            reply,
            reasoning_content,
            messages,
            context_tokens,
            context_tokens_estimated,
            ..
        } => {
            let assistant_message = messages.last().cloned().unwrap_or_else(|| {
                json!({ "role": "assistant", "content": reply })
            });
            Ok(json!({
                "type": "final",
                "reply": reply,
                "reasoning_content": reasoning_content,
                "assistant_message": assistant_message,
                "context_tokens": context_tokens,
                "context_tokens_estimated": context_tokens_estimated,
                "context_length": context_length
            }))
        }
        crate::ai_client::AgentStep::ToolCalls {
            calls,
            messages,
            reasoning_content,
            content,
            context_tokens,
            context_tokens_estimated,
            ..
        } => {
            let next_step = completed_steps + 1;
            let generation = uuid::Uuid::new_v4().to_string();
            let assistant_message =
                messages.last().cloned().unwrap_or_else(|| json!({ "role": "assistant" }));
            Ok(json!({
                "type": "tool_calls",
                "step": next_step,
                "generation": generation,
                "reasoning_content": reasoning_content,
                "content": content,
                "calls": calls,
                "assistant_message": assistant_message,
                "context_tokens": context_tokens,
                "context_tokens_estimated": context_tokens_estimated,
                "context_length": context_length
            }))
        }
    }
}

#[cfg(test)]
mod live2d_agent_tests {
    use super::{
        apply_live2d_conversation_mutation, get_live2d_agent_conversation,
        live2d_agent_pending_call_ids, live2d_agent_pending_calls, live2d_agent_phase,
        replace_all_live2d_agent_messages, Live2dConversationMutation,
    };
    use std::collections::HashSet;
    /// 涉及真实 reddat.db 的测试共享同一数据库文件，串行执行避免相互污染。
    static DB_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tool_message(id: &str, done: bool) -> serde_json::Value {
        let mut message = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {
                    "id": id,
                    "type": "function",
                    "function": { "name": "execute_javascript", "arguments": "{\"javascript\":\"return 1;\"}" }
                }
            ]
        });
        if done {
            message["content"] = serde_json::json!("已完成");
        }
        message
    }

    #[test]
    fn pending_tool_calls_must_be_missing_results() {
        let pending = tool_message("call_1", false);
        assert_eq!(
            live2d_agent_pending_call_ids(&[pending.clone()])
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>(),
            HashSet::from(["call_1".to_owned()])
        );
        // 结果已提交、停在工具阶段：返回空集（可继续生成，非悬挂）
        let all_done = vec![
            pending.clone(),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "ok"}),
        ];
        assert_eq!(live2d_agent_pending_call_ids(&all_done).unwrap(), HashSet::new());
        // 对话已进入最终回答阶段：无工具流程
        let completed = vec![
            pending,
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "ok"}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        assert!(live2d_agent_pending_call_ids(&completed).is_none());
    }

    #[test]
    fn pending_calls_reexpose_executable_arguments() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "execute_javascript", "arguments": "{\"javascript\":\"return 1;\"}" }
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": { "name": "run_c", "arguments": "{\"source_code\":\"int main(){return 0;}\"}" }
                    }
                ]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "ok"}),
        ];
        let calls = live2d_agent_pending_calls(&messages).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["id"], "call_2");
        assert!(calls[0].get("source_code").is_some());
    }

    #[test]
    fn phase_reflects_conversation_stage() {
        assert_eq!(live2d_agent_phase(&[]), "empty");
        assert_eq!(
            live2d_agent_phase(&[serde_json::json!({"role": "user", "content": "hi"})]),
            "interrupted_stream"
        );
        assert_eq!(
            live2d_agent_phase(&[serde_json::json!({"role": "assistant", "content": "hi"})]),
            "completed"
        );
        assert_eq!(
            live2d_agent_phase(&[tool_message("call_1", false)]),
            "tool_pending"
        );
        assert_eq!(
            live2d_agent_phase(&[serde_json::json!({
                "role": "assistant",
                "content": "已停止",
                "interrupted": true
            })]),
            "interrupted"
        );
        assert_eq!(
            live2d_agent_phase(&[serde_json::json!({
                "role": "assistant",
                "content": null,
                "reasoning_content": "思考"
            })]),
            "interrupted_stream"
        );
    }

    #[test]
    fn updates_and_deletes_conversation_messages() {
        let original = serde_json::json!({"role": "user", "content": "原问题"});
        let replacement = serde_json::json!({"role": "user", "content": "修改后的问题"});
        let mut messages = vec![
            original.clone(),
            serde_json::json!({"role": "assistant", "content": "回答"}),
        ];
        apply_live2d_conversation_mutation(
            &mut messages,
            Live2dConversationMutation::Update {
                index: 0,
                expected: original,
                message: replacement.clone(),
            },
        )
        .unwrap();
        assert_eq!(messages[0], replacement);

        let expected = messages[1].clone();
        apply_live2d_conversation_mutation(
            &mut messages,
            Live2dConversationMutation::Delete { index: 1, expected },
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn rejects_stale_or_invalid_conversation_edits() {
        let original = serde_json::json!({"role": "user", "content": "原问题"});
        let mut messages = vec![original.clone()];
        let stale = apply_live2d_conversation_mutation(
            &mut messages,
            Live2dConversationMutation::Delete {
                index: 0,
                expected: serde_json::json!({"role": "user", "content": "旧副本"}),
            },
        );
        assert!(stale.is_err());

        let invalid = apply_live2d_conversation_mutation(
            &mut messages,
            Live2dConversationMutation::Update {
                index: 0,
                expected: original,
                message: serde_json::json!({"role": "unknown", "content": "内容"}),
            },
        );
        assert!(invalid.is_err());
    }

    #[tokio::test]
    async fn stores_single_conversation_for_next_user_turn() {
        let _db_guard = DB_TEST_LOCK.lock().unwrap();
        // 测试间共享同一个 reddat.db，先清理隔离
        super::clear_live2d_agent_conversation().await.unwrap();
        let messages = vec![serde_json::json!({
            "role": "assistant",
            "reasoning_content": "reasoning",
            "content": "answer",
            "timestamp": crate::ai_client::now_timestamp()
        })];
        replace_all_live2d_agent_messages(messages.clone())
            .await
            .unwrap();
        assert_eq!(
            get_live2d_agent_conversation().await.unwrap(),
            Some(messages)
        );
        super::clear_live2d_agent_conversation().await.unwrap();
        assert_eq!(get_live2d_agent_conversation().await.unwrap(), None);
    }

    #[tokio::test]
    async fn replace_last_persists_generation_for_tool_continue() {
        let _db_guard = DB_TEST_LOCK.lock().unwrap();
        // 测试间共享同一个 reddat.db，先清理隔离
        super::clear_live2d_agent_conversation().await.unwrap();
        // 模拟前端工具调用落库协议：replace_last + generation（④ 工具调用落库）
        let assistant = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "execute_javascript", "arguments": "{\"javascript\":\"return 1;\"}" }
            }]
        });
        super::super::deal_live2d_chat_append(super::super::Live2dAppendRequest {
            op: "replace_last".to_owned(),
            message: Some(assistant.clone()),
            messages: None,
            reply: None,
            generation: Some("gen-123".to_owned()),
            clear_generation: false,
        })
        .await
        .unwrap();
        // 代次必须已写入，否则继续工具调用会被拒绝
        assert_eq!(
            super::get_live2d_agent_generation().await.unwrap(),
            Some("gen-123".to_owned())
        );
        // 尾部消息是 assistant(tool_calls) 且全部结果缺失 → pending 非空
        let messages = super::get_live2d_agent_conversation().await.unwrap().unwrap();
        let pending = live2d_agent_pending_call_ids(&messages).unwrap();
        assert_eq!(pending, HashSet::from(["call_1".to_owned()]));
        // 继续流程全部落库后：结果为全，但代次仍应在（直到最终回复时才清除）
        let tool_result = serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "ok",
            "timestamp": crate::ai_client::now_timestamp()
        });
        super::super::deal_live2d_chat_append(super::super::Live2dAppendRequest {
            op: "append".to_owned(),
            message: Some(tool_result),
            messages: None,
            reply: None,
            generation: None,
            clear_generation: false,
        })
        .await
        .unwrap();
        let messages = super::get_live2d_agent_conversation().await.unwrap().unwrap();
        assert!(live2d_agent_pending_call_ids(&messages).unwrap().is_empty());
        assert_eq!(
            super::get_live2d_agent_generation().await.unwrap(),
            Some("gen-123".to_owned())
        );
        // 清理
        super::clear_live2d_agent_conversation().await.unwrap();
    }
}
/// 构造一条 SSE 事件文本（event + data 两行，空行结尾）。
pub(crate) fn live2d_sse_event(event: &str, data: &serde_json::Value) -> bytes::Bytes {
    let mut out = String::with_capacity(64 + data.to_string().len());
    out.push_str("event: ");
    out.push_str(event);
    out.push_str("\ndata: ");
    out.push_str(&data.to_string());
    out.push_str("\n\n");
    bytes::Bytes::from(out)
}
