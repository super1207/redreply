//! DeepSeek V4 tokenizer:本地精确统计/估算 prompt token 数。
//!
//! tokenizer.json 打包在插件内(`assets/deepseek_tokenizer.json`),进程内只加载一次。
//! 估算口径按 DeepSeek 对话模板:每条消息的内容 token + 模板特殊 token 开销,
//! 另加工具定义开销;用于历史压缩阈值判断与日志展示。

use std::sync::OnceLock;

use serde_json::Value;
use tokenizers::Tokenizer;

/// 打包的 DeepSeek V4 tokenizer
const TOKENIZER_JSON: &[u8] = include_bytes!("../assets/deepseek_tokenizer.json");

static TOKENIZER: OnceLock<Option<Tokenizer>> = OnceLock::new();

fn tokenizer() -> Option<&'static Tokenizer> {
    TOKENIZER
        .get_or_init(|| Tokenizer::from_bytes(TOKENIZER_JSON).ok())
        .as_ref()
}

/// 估算一次 /chat/completions 请求的 prompt token 数。
///
/// 口径 = DeepSeek 对话模板实际渲染的内容(特殊 token + 各段文本 token) + 工具定义。
/// 注意:opencode Zen 等代理会在转发时附加自己的隐藏上下文(实测约 27 token)并对
/// tools 做额外展开,这里只统计插件自己发出的 prompt。
pub(crate) fn estimate_request_tokens(body: &Value) -> Option<u64> {
    let tokenizer = tokenizer()?;
    // <|begin_of_sentence|> + 生成引导
    let mut total = 2u64;
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            match role {
                "system" => {
                    // 模板把 system 内容直接拼接在 BOS 之后,无额外特殊 token
                    total += content_tokens(tokenizer, message);
                }
                "user" => {
                    total += 1; // <|User|>
                    total += content_tokens(tokenizer, message);
                }
                "assistant" => {
                    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                        // <|Assistant|><|tool_calls_begin|><|tool_calls_end|><|end_of_sentence|>
                        total += 4;
                        for call in tool_calls {
                            total += 4; // <|tool_call_begin|><|tool_sep|><|tool_call_end|> + 代码块围栏
                            if let Some(name) =
                                call.pointer("/function/name").and_then(Value::as_str)
                            {
                                total += count_text(tokenizer, name);
                            }
                            if let Some(arguments) =
                                call.pointer("/function/arguments").and_then(Value::as_str)
                            {
                                total += count_text(tokenizer, arguments);
                            }
                        }
                    } else {
                        total += 2; // <|Assistant|> + <|end_of_sentence|>
                        total += content_tokens(tokenizer, message);
                    }
                }
                "tool" => {
                    // <|tool_outputs_begin|><|tool_output_begin|>...<|tool_output_end|>
                    total += 3;
                    total += content_tokens(tokenizer, message);
                }
                _ => total += content_tokens(tokenizer, message),
            }
        }
    }
    if let Some(tools) = body.get("tools") {
        total += count_value(tokenizer, tools);
    }
    Some(total)
}

fn content_tokens(tokenizer: &Tokenizer, message: &Value) -> u64 {
    message
        .get("content")
        .map(|content| count_value(tokenizer, content))
        .unwrap_or(0)
}

fn count_value(tokenizer: &Tokenizer, value: &Value) -> u64 {
    match value {
        Value::String(text) => count_text(tokenizer, text),
        Value::Array(_) | Value::Object(_) => count_text(tokenizer, &value.to_string()),
        _ => 0,
    }
}

fn count_text(tokenizer: &Tokenizer, text: &str) -> u64 {
    tokenizer
        .encode(text, false)
        .map(|encoding| encoding.get_ids().len() as u64)
        .unwrap_or(0)
}

/// 统计一段纯文本的 token 数；tokenizer 加载失败时按字符数粗略估计。
pub(crate) fn count_text_tokens(text: &str) -> usize {
    match tokenizer() {
        Some(tokenizer) => count_text(tokenizer, text) as usize,
        None => text.chars().count() / 3 + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn count(text: &str) -> u64 {
        let tokenizer = tokenizer().expect("tokenizer loads");
        count_text(tokenizer, text)
    }

    #[test]
    fn counts_text_tokens() {
        let hello = count("Hello!");
        assert!(hello > 0);
        let chinese = count("你好，世界");
        assert!(chinese > 0);
        let long = count(&"token ".repeat(100));
        assert!(long > 50, "{long}");
    }

    #[test]
    fn estimate_counts_messages_and_tools() {
        let body = json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "你好"}
            ]
        });
        let without_tools = estimate_request_tokens(&body).expect("tokenizer loads");
        let body = json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "你好"}
            ],
            "tools": [{"type": "function", "function": {"name": "run_command", "description": "执行命令"}}]
        });
        let with_tools = estimate_request_tokens(&body).expect("tokenizer loads");
        assert!(with_tools > without_tools);
    }

    #[test]
    fn estimate_matches_deepseek_template_shape() {
        // 实测(deepseek-v4.1-flash):本地估算与接口 prompt_tokens 只差代理的固定隐藏开销。
        // 本地口径:base 2 + system 内容 2 + <|User|> 1 + "Hello!" 2 = 7
        let body = json!({
            "messages": [
                {"role": "system", "content": "你是助手"},
                {"role": "user", "content": "Hello!"}
            ]
        });
        assert_eq!(estimate_request_tokens(&body), Some(7));

        // 单条 user "Hello!" = 2 + 1 + 2 = 5
        let body = json!({
            "messages": [{"role": "user", "content": "Hello!"}]
        });
        assert_eq!(estimate_request_tokens(&body), Some(5));
    }

    #[test]
    fn estimate_includes_tool_call_arguments() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "算一下"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {
"name": "run_command",
                        "arguments": "{\"code\":\"print(6*7)\"}"
                    }}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "退出码：0\nstdout:\n42"}
            ]
        });
        let estimate = estimate_request_tokens(&body).expect("tokenizer loads");
        let content_only =
            count("算一下") + count("{\"code\":\"print(6*7)\"}") + count("退出码：0\nstdout:\n42");
        assert!(
            estimate > content_only,
            "estimate={estimate} content={content_only}"
        );
    }
}
