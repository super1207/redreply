use super::*;
use crate::native_args;

fn block(value: &RedValue) -> Result<Value> {
    let row = array(value)?;
    let [kind, text, extra] = [row_text(row, 0)?, row_text(row, 1)?, row_text(row, 2)?];
    if row.len() > 3 {
        return Err("富文本内容块的参数过多".into());
    }
    let block = match kind.as_str() {
        "段落" => json!({"type":"paragraph","text":text}),
        "标题" => {
            let size = if extra.is_empty() {
                1
            } else {
                positive_id(&extra)?
            };
            if size > 6 {
                return Err("标题级别请填 1 至 6".into());
            }
            json!({"type":"heading","text":text,"size":size})
        }
        "代码" => {
            let mut code = json!({"type":"pre","text":text});
            if !extra.is_empty() {
                code["language"] = extra.into();
            }
            code
        }
        "页脚" => json!({"type":"footer","text":text}),
        "分隔线" => json!({"type":"divider"}),
        "公式" => json!({"type":"mathematical_expression","expression":text}),
        "锚点" => json!({"type":"anchor","name":text}),
        "引文" => {
            let mut quote = json!({"type":"pullquote","text":text});
            if !extra.is_empty() {
                quote["credit"] = extra.into();
            }
            quote
        }
        "草稿提示" => json!({"type":"thinking","text":text}),
        _ => {
            return Err(
                "内容块请填段落、标题、代码、页脚、分隔线、公式、锚点、引文或草稿提示".into(),
            )
        }
    };
    Ok(block)
}

fn content(rl: &mut RedLang, params: &[Ast]) -> Result<Value> {
    let input = rl.get_param(params, 0)?;
    let [mode, rtl, skip] = text_params::<3>(rl, params, 1)?;
    let mut files = serde_json::Map::new();
    let mut body = match mode.as_str() {
        "" | "Markdown" | "HTML" => {
            let text = input.expect_text_value()?;
            if text.is_empty() {
                return Err("请填写富文本内容".into());
            }
            let key = if mode == "HTML" { "html" } else { "markdown" };
            json!({key:text})
        }
        "内容块" => {
            let rows = array(&input)?;
            if rows.is_empty() {
                return Err("请填写至少一个富文本内容块".into());
            }
            json!({"blocks":rows.iter().map(|row| block(row)).collect::<Result<Vec<_>>>()?})
        }
        _ => return Err("富文本格式请填 Markdown、HTML 或内容块".into()),
    };
    for (key, value) in [("is_rtl", rtl), ("skip_entity_detection", skip)] {
        if !value.is_empty() {
            body[key] = boolean(&value, false)?.into();
        }
    }
    let input = rl.get_param(params, 4)?;
    if !matches!(input.as_ref(), RedValue::Text(value) if value.is_empty()) {
        if mode == "内容块" {
            return Err("资源表用于 Markdown 或 HTML 富文本".into());
        }
        let mut seen = HashSet::new();
        let mut media = Vec::new();
        for (index, value) in array(&input)?.iter().enumerate() {
            let row = array(value)?;
            let [id, kind, ..] = row.as_ref() else {
                return Err("富文本资源请填【数组@标识@类型@来源@文件名】".into());
            };
            let id = id.expect_text_value()?;
            if !(1..=64).contains(&id.len())
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                || !seen.insert(id.clone())
            {
                return Err("富文本资源标识须唯一，1 至 64 位字母、数字、下划线或连字符".into());
            }
            let kind = match kind.expect_text_value()?.as_str() {
                "图片" => "photo",
                "视频" => "video",
                "音频" => "audio",
                "文件" => "document",
                "动画" => "animation",
                "语音便笺" => "voice_note",
                _ => return Err("富文本资源类型请填图片、视频、音频、文件、动画或语音便笺".into()),
            };
            if !(3..=4).contains(&row.len()) {
                return Err("富文本资源请填【数组@标识@类型@来源@文件名】".into());
            }
            let source = native_args::media_source(
                &row[2],
                &row_text(row, 3)?,
                &format!("rich_{index}"),
                &mut files,
            )?;
            if let Some(link) = source
                .as_str()
                .filter(|link| link.contains("://") && !link.starts_with("attach://"))
            {
                let url = url::Url::parse(link)?;
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                    return Err("富文本资源链接请填 HTTP(S) 地址".into());
                }
            }
            media.push(json!({"id":id,"media":{"type":kind,"media":source}}));
        }
        if !media.is_empty() {
            body["media"] = media.into();
        }
    }
    Ok(json!({"body":body,"files":files}))
}

pub(crate) fn decode(value: &RedValue) -> Result<Value> {
    let text = value.expect_text_value()?;
    let segments = redlang::mytool::str_msg_to_arr(&json!(text))?;
    let [segment] = segments.as_array().ok_or("富文本格式错误")?.as_slice() else {
        return Err("请填单条【Telegram富文本】".into());
    };
    if segment["type"] != "telegram_rich" {
        return Err("请填【Telegram富文本】".into());
    }
    let value: Value = serde_json::from_str(
        segment["data"]["content"]
            .as_str()
            .ok_or("富文本缺少内容")?,
    )?;
    Ok(value)
}

pub(crate) fn requires_upload(body: &Value) -> bool {
    body["media"].as_array().into_iter().flatten().any(|item| {
        item["media"]["media"]
            .as_str()
            .is_some_and(|source| source.contains("://"))
    })
}

pub(super) fn register() {
    add_fun(vec!["Telegram富文本"], |rl, params| {
        if get_platform(rl) != "telegram" {
            return Err("富文本需要 Telegram 机器人来源".into());
        }
        let content = content(rl, params)?;
        let content = redlang::mytool::cq_params_encode(&content.to_string());
        Ok(Some(rv_text(format!(
            "[CQ:telegram_rich,content={content}]"
        ))))
    });
    add_fun(vec!["Telegram富文本草稿"], |rl, params| {
        let input = rl.get_param(params, 0)?;
        let data = decode(&input)?;
        if requires_upload(&data["body"])
            || data["files"]
                .as_object()
                .is_some_and(|files| !files.is_empty())
        {
            return Err("富文本草稿资源请使用已有 Telegram 文件ID".into());
        }
        let [id, can_stop, keep, chat, thread] = text_params::<5>(rl, params, 1)?;
        let id = id
            .parse::<i64>()
            .ok()
            .filter(|id| *id != 0)
            .ok_or("草稿ID请填非零整数")?;
        let target = if chat.is_empty() {
            if !rl.get_exmap("群ID").is_empty() {
                return Err("请指定接收草稿的机器人私聊ID".into());
            }
            positive_id(&rl.get_exmap("发送者ID"))?
        } else {
            positive_id(&chat)?
        };
        let mut body = json!({"chat_id":target,"draft_id":id,"rich_message":data["body"],
                              "can_stop":boolean(&can_stop,false)?,"keep_on_stop":boolean(&keep,false)?});
        if !thread.is_empty() {
            body["message_thread_id"] = positive_id(&thread)?.into();
        } else if let Ok(event) = serde_json::from_str(&rl.get_exmap("原始事件")) {
            let mut topic = json!({});
            apply_telegram_topic(
                &event,
                &rl.get_exmap("机器人ID"),
                &target.to_string(),
                &mut topic,
            );
            if let Some(thread) = topic.get("message_thread_id") {
                body["message_thread_id"] = thread.clone();
            }
        }
        telegram(rl, "sendRichMessageDraft", body)?;
        Ok(Some(rv_empty()))
    });
}

pub(super) fn event_fields(message: &Value) -> Value {
    fn text(value: &Value) -> String {
        match value {
            Value::String(value) => value.clone(),
            Value::Array(values) => values.iter().map(text).collect(),
            Value::Object(value) => value.get("text").map(text).unwrap_or_default(),
            _ => String::new(),
        }
    }
    fn block_text(block: &Value, result: &mut Vec<String>, depth: u8) {
        if depth == 0 {
            return;
        }
        for field in ["text", "expression", "summary", "credit", "caption"] {
            let part = text(&block[field]);
            if !part.is_empty() {
                result.push(part);
            }
        }
        for field in ["blocks", "items"] {
            for child in block[field].as_array().into_iter().flatten() {
                block_text(child, result, depth - 1);
            }
        }
    }
    let rich = &message["rich_message"];
    if !rich.is_object() {
        return Value::Null;
    }
    let mut parts = Vec::new();
    for block in rich["blocks"].as_array().into_iter().flatten() {
        block_text(block, &mut parts, 16);
    }
    json!({"正文":parts.join("\n"),"从右向左":rich["is_rtl"].as_bool().unwrap_or(false)})
}
