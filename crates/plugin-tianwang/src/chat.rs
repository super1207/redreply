use super::*;

pub(crate) fn command_tool_definition() -> Value {
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
pub(crate) fn no_reply_tool_definition() -> Value {
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

pub(crate) fn build_request_body(config: &ChatConfig, messages: &[Value]) -> Value {
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
pub(crate) fn assistant_tool_call_message(
    text: &str,
    calls: &[ToolCall],
    reasoning: &str,
) -> Value {
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

pub(crate) struct SseEvent {
    pub(crate) piece: String,
    pub(crate) reasoning: String,
    pub(crate) tool_calls: Vec<ToolCallDelta>,
    pub(crate) usage: Option<StreamUsage>,
    pub(crate) done: bool,
}

/// 解析 usage：prompt/completion 必须存在；缓存命中数兼容多种字段命名。
pub(crate) fn parse_usage(usage: &Value) -> Option<StreamUsage> {
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

pub(crate) fn parse_sse_line(line: &str) -> std::result::Result<SseEvent, String> {
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

pub(crate) fn extract_text_content(obj: &Value) -> Option<String> {
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
pub(crate) async fn chat_stream(
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
pub(crate) async fn chat_dialog(
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
pub(crate) async fn chat_dialog_with_messages(
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
