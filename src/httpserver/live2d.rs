//! 看板娘 Live2D 对话的 HTTP 处理（从 mod.rs 拆分）
//! 包含：会话管理、上下文存取、工具结果规范化、对话步骤响应等。

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
use super::{json_api_response, BoxBody, Result};

pub(crate) const LIVE2D_CHAT_BODY_LIMIT: usize = 64 * 1024;
pub(crate) const LIVE2D_CONVERSATION_BODY_LIMIT: usize = 256 * 1024;
pub(crate) const LIVE2D_TOOL_RESULTS_TOTAL_CHARS: usize = 12_000;

#[derive(Deserialize)]
pub(crate) struct Live2dChatRequest {
    #[serde(default)]
    pub(crate) message: Option<String>,
    #[serde(default)]
    pub(crate) tool_results: Vec<Live2dToolResult>,
    #[serde(default)]
    pub(crate) generation: Option<String>,
    /// 前端「压缩」按钮显式携带的压缩意图标志：不预先压缩，
    /// 而是让看板娘在对话中可见地汇报压缩结果，完成后由后端用汇报替换旧历史。
    #[serde(default)]
    pub(crate) compress: bool,
}

#[derive(Deserialize)]
pub(crate) struct Live2dToolResult {
    pub(crate) id: String,
    pub(crate) result: String,
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
/// （原会话表已移除：工具调用状态可直接从数据库对话消息中恢复）
pub(crate) static LIVE2D_AGENT_FLOW_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
pub(crate) fn live2d_agent_flow_lock() -> &'static tokio::sync::Mutex<()> {
    LIVE2D_AGENT_FLOW_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) async fn get_live2d_agent_conversation(
) -> std::result::Result<Option<Vec<serde_json::Value>>, String> {
    // 每次调用都从数据库读取，保证记录与磁盘一致
    let loaded = spawn_blocking_io(
        "读取看板娘对话",
        crate::resource_manager::load_live2d_chat_conversation,
    )
    .await?;
    loaded
        .map(|raw| serde_json::from_str::<Vec<serde_json::Value>>(&raw).map_err(|e| e.to_string()))
        .transpose()
}
pub(crate) async fn save_live2d_agent_conversation(
    messages: Vec<serde_json::Value>,
) -> std::result::Result<(), String> {
    save_live2d_agent_conversation_state(messages, None).await
}

#[derive(Deserialize)]
pub(crate) struct Live2dCancelRequest {
    pub(crate) generation: String,
}

pub(crate) async fn save_live2d_agent_conversation_state(
    messages: Vec<serde_json::Value>,
    generation: Option<String>,
) -> std::result::Result<(), String> {
    let raw = serde_json::to_string(&messages).map_err(|error| error.to_string())?;
    // 每次调用结束都写入数据库
    spawn_blocking_io("保存看板娘对话", move || {
        crate::resource_manager::save_live2d_chat_state(&raw, generation.as_deref())
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
    let raw = serde_json::to_string(&messages).map_err(|error| error.to_string())?;
    spawn_blocking_io("保存对话", move || {
        crate::resource_manager::save_live2d_chat_conversation(&raw)
    })
    .await?;
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
            "can_write": can_write
        }
    }))
}

/// 仅当对话尾部正好是 assistant 工具调用时返回待执行 ID。
/// 已有 tool/final 消息的历史调用不会再次被识别为 pending。
pub(crate) fn live2d_agent_pending_call_ids(
    messages: &[serde_json::Value],
) -> Option<HashSet<String>> {
    let message = messages.last()?;
    if message.get("role").and_then(serde_json::Value::as_str) != Some("assistant") {
        return None;
    }
    let calls = message
        .get("tool_calls")
        .and_then(serde_json::Value::as_array)?;
    let ids: HashSet<String> = calls
        .iter()
        .filter_map(|call| {
            call.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    (!ids.is_empty()).then_some(ids)
}

pub(crate) async fn live2d_agent_has_pending_tools() -> std::result::Result<bool, String> {
    Ok(get_live2d_agent_conversation()
        .await?
        .as_ref()
        .and_then(|messages| live2d_agent_pending_call_ids(messages))
        .is_some())
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

pub(crate) fn validate_live2d_tool_results(
    pending_call_ids: &HashSet<String>,
    tool_results: Vec<Live2dToolResult>,
) -> Result<Vec<(String, String)>> {
    // 容错设计：工具执行失败/超时/结果缺失时不断开对话，
    // 而是为缺失的工具调用补占位失败说明，让模型看到后自行重试或调整方案。
    const FAILED_NOTE: &str = "[工具执行失败：未收到该工具调用的结果（可能执行超时或页面异常）。请根据情况重试、更换方案或直接告知用户。]";
    const TRUNCATED_NOTE: &str = "[返回内容过长，已截断。请缩小范围或分批读取后重新获取。]";
    let single_budget = crate::ai_client::MAX_TOOL_RESULT_CHARS;
    let total_budget = LIVE2D_TOOL_RESULTS_TOTAL_CHARS;

    let mut received_ids = HashSet::new();
    let mut results = Vec::with_capacity(pending_call_ids.len());
    let mut total_chars = 0;
    for result in tool_results {
        // 无效或重复 ID 直接丢弃（缺失的会在下方补全）
        if !pending_call_ids.contains(&result.id) || !received_ids.insert(result.id.clone()) {
            continue;
        }
        let mut text = result.result;
        if text.chars().count() > single_budget {
            text = text.chars().take(single_budget).collect::<String>();
            text.push_str("\n\n");
            text.push_str(TRUNCATED_NOTE);
        }
        if total_chars + text.chars().count() > total_budget {
            text = TRUNCATED_NOTE.to_owned();
        }
        total_chars += text.chars().count();
        results.push((result.id, text));
    }
    // 补全缺失的工具调用：以失败说明占位，保证模型拿到全部工具结果
    for id in pending_call_ids {
        if !received_ids.contains(id) {
            results.push((id.clone(), FAILED_NOTE.to_owned()));
        }
    }
    Ok(results)
}

/// 统一处理一个 Agent 步骤：保存对话、创建/清理会话、构造 data JSON。
/// 流式（SSE done 事件）与非流式（JSON 响应）共用，避免两套逻辑重复。
/// 统一处理一个 Agent 步骤：保存对话、构造 data JSON。
/// 流式（SSE done 事件）与非流式（JSON 响应）共用。
/// 单用户场景下不维护会话表：工具调用状态直接从数据库对话消息中恢复，
/// 这里只负责把每一步的完整消息持久化。
pub(crate) async fn live2d_agent_step_data(
    step: crate::ai_client::AgentStep,
    completed_steps: u64,
    context_length: u64,
    compress: bool,
) -> std::result::Result<serde_json::Value, String> {
    match step {
        crate::ai_client::AgentStep::Final {
            reply,
            reasoning_content,
            messages,
            context_tokens,
            context_tokens_estimated,
        } => {
            // 压缩请求：AI 已在对话中可见地汇报压缩结果。
            // 用该汇报替换旧历史（只保留一个 system 摘要消息），让上下文真正变小。
            if compress {
                let summary = reply.trim();
                let compressed_messages = vec![serde_json::json!({
                    "role": "system",
                    "content": format!(
                        "{}{}",
                        crate::ai_client::COMPRESSED_SUMMARY_PREFIX,
                        summary
                    ),
                    "timestamp": crate::ai_client::now_timestamp()
                })];
                save_live2d_agent_conversation(compressed_messages.clone()).await?;
                let context_tokens =
                    crate::ai_client::estimate_context_tokens(&compressed_messages);
                return Ok(json!({
                    "type": "final",
                    "reply": reply,
                    "reasoning_content": reasoning_content,
                    "context_tokens": context_tokens,
                    "context_tokens_estimated": true,
                    "context_length": context_length
                }));
            }
            save_live2d_agent_conversation(messages).await?;
            Ok(json!({
                "type": "final",
                "reply": reply,
                "reasoning_content": reasoning_content,
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
        } => {
            let next_step = completed_steps + 1;
            let generation = uuid::Uuid::new_v4().to_string();
            // 完整消息（含 assistant 的 tool_calls）已持久化，前端下次提交时
            // 通过 live2d_agent_pending_call_ids 从数据库恢复校验
            save_live2d_agent_conversation_state(messages, Some(generation.clone())).await?;
            Ok(json!({
                "type": "tool_calls",
                "step": next_step,
                "generation": generation,
                "reasoning_content": reasoning_content,
                "content": content,
                "calls": calls,
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
        live2d_agent_pending_call_ids, save_live2d_agent_conversation,
        validate_live2d_tool_results, Live2dConversationMutation, Live2dToolResult,
    };
    use std::collections::HashSet;

    #[test]
    fn validates_complete_tool_result_set() {
        let expected = HashSet::from(["call_1".to_owned(), "call_2".to_owned()]);
        let results = validate_live2d_tool_results(
            &expected,
            vec![
                Live2dToolResult {
                    id: "call_1".to_owned(),
                    result: "one".to_owned(),
                },
                Live2dToolResult {
                    id: "call_2".to_owned(),
                    result: "two".to_owned(),
                },
            ],
        )
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn pending_tool_calls_must_be_at_conversation_tail() {
        let pending = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{"id": "call_1", "type": "function"}]
        });
        assert_eq!(
            live2d_agent_pending_call_ids(&[pending.clone()])
                .unwrap()
                .len(),
            1
        );
        let completed = vec![
            pending,
            serde_json::json!({"role": "tool", "tool_call_id": "call_1", "content": "ok"}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        assert!(live2d_agent_pending_call_ids(&completed).is_none());
    }

    #[test]
    fn fills_missing_tool_result_with_failure_note() {
        let expected = HashSet::from(["call_1".to_owned(), "call_2".to_owned()]);
        let results = validate_live2d_tool_results(
            &expected,
            vec![Live2dToolResult {
                id: "call_1".to_owned(),
                result: "one".to_owned(),
            }],
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "call_1");
        assert_eq!(results[0].1, "one");
        assert_eq!(results[1].0, "call_2");
        assert!(results[1].1.contains("工具执行失败"));
    }

    #[test]
    fn truncates_oversized_tool_results_instead_of_rejecting() {
        let expected = HashSet::from(["call_1".to_owned(), "call_2".to_owned()]);
        let huge = "字".repeat(20_000);
        let results = validate_live2d_tool_results(
            &expected,
            vec![
                Live2dToolResult {
                    id: "call_1".to_owned(),
                    result: huge.clone(),
                },
                Live2dToolResult {
                    id: "call_2".to_owned(),
                    result: huge,
                },
            ],
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        let total_chars: usize = results.iter().map(|(_, text)| text.chars().count()).sum();
        assert!(
            total_chars <= 12_000,
            "截断后总长不应超过预算: {total_chars}"
        );
        assert!(results
            .iter()
            .all(|(_, text)| text.contains("返回内容过长")));
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
        let messages = vec![serde_json::json!({
            "role": "assistant",
            "reasoning_content": "reasoning",
            "content": "answer",
            "timestamp": crate::ai_client::now_timestamp()
        })];
        save_live2d_agent_conversation(messages.clone())
            .await
            .unwrap();
        assert_eq!(
            get_live2d_agent_conversation().await.unwrap(),
            Some(messages)
        );
        super::clear_live2d_agent_conversation().await.unwrap();
        assert_eq!(get_live2d_agent_conversation().await.unwrap(), None);
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

pub(crate) async fn cancel_live2d_pending_tools(
    generation: &str,
) -> std::result::Result<(), String> {
    let generation = generation.trim();
    if generation.is_empty() || generation.len() > 100 {
        return Err("取消请求缺少有效的对话代次".to_owned());
    }

    let stored_generation = get_live2d_agent_generation()
        .await?
        .ok_or_else(|| "当前没有待取消的工具调用".to_owned())?;
    if stored_generation != generation {
        return Err("待取消的工具调用已经失效".to_owned());
    }
    let mut messages = get_live2d_agent_conversation()
        .await?
        .ok_or_else(|| "当前没有待取消的对话".to_owned())?;
    let pending_ids = live2d_agent_pending_call_ids(&messages)
        .ok_or_else(|| "当前没有待取消的工具调用".to_owned())?;
    for id in pending_ids {
        messages.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": id,
            "content": "[用户已停止本轮工具执行，结果未提交]",
            "timestamp": crate::ai_client::now_timestamp()
        }));
    }
    messages.push(serde_json::json!({
        "role": "assistant",
        "content": "本轮工具执行已由用户停止。",
        "timestamp": crate::ai_client::now_timestamp()
    }));
    let raw = serde_json::to_string(&messages).map_err(|error| error.to_string())?;
    spawn_blocking_io("取消看板娘工具执行", move || {
        crate::resource_manager::save_live2d_chat_state(&raw, None)
    })
    .await
}
