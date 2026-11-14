use super::*;
use crate::native_args;
#[path = "bot_rich_blocks.rs"]
mod block;

fn basic_block(value: &RedValue) -> Result<Value> {
    let row = array(value)?;
    let kind = row_text(row, 0)?;
    let text = row
        .get(1)
        .map(|value| block::inline_text(value, 8))
        .transpose()?
        .unwrap_or_else(|| "".into());
    if row.len() > 3 {
        return Err("富文本内容块的参数过多".into());
    }
    let block = match kind.as_str() {
        "段落" => json!({"type":"paragraph","text":text}),
        "标题" => {
            let extra = row_text(row, 2)?;
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
            let extra = row_text(row, 2)?;
            let mut code = json!({"type":"pre","text":text});
            if !extra.is_empty() {
                code["language"] = extra.into();
            }
            code
        }
        "页脚" => json!({"type":"footer","text":text}),
        "分隔线" => json!({"type":"divider"}),
        "公式" => json!({"type":"mathematical_expression","expression":row_text(row,1)?}),
        "锚点" => json!({"type":"anchor","name":row_text(row,1)?}),
        "引文" => {
            let mut quote = json!({"type":"pullquote","text":text});
            if let Some(credit) = row.get(2) {
                let credit = block::inline_text(credit, 8)?;
                if credit != "" {
                    quote["credit"] = credit;
                }
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

fn media_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "图片" => Ok("photo"),
        "视频" => Ok("video"),
        "音频" => Ok("audio"),
        "文件" => Ok("document"),
        "动画" => Ok("animation"),
        "语音便笺" => Ok("voice_note"),
        _ => Err("未知的富文本内容块或媒体类型".into()),
    }
}

fn source(
    input: &RedValue,
    filename: &str,
    files: &mut serde_json::Map<String, Value>,
) -> Result<Value> {
    if let RedValue::Text(link) = input {
        if link.contains("://") {
            let url = url::Url::parse(link)?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err("富文本资源链接请填 HTTP(S) 地址".into());
            }
        }
    }
    native_args::media_source(input, filename, &format!("rich_{}", files.len()), files)
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
            json!({"blocks":block::blocks(&input, &mut files, 16)?})
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
        for value in array(&input)? {
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
            let kind = media_kind(&kind.expect_text_value()?)?;
            if !(3..=4).contains(&row.len()) {
                return Err("富文本资源请填【数组@标识@类型@来源@文件名】".into());
            }
            let source = source(&row[2], &row_text(row, 3)?, &mut files)?;
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
    match body {
        Value::Array(items) => items.iter().any(requires_upload),
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            key == "media" && value.as_str().is_some_and(|source| source.contains("://"))
                || requires_upload(value)
        }),
        _ => false,
    }
}

pub(super) fn inline(value: &RedValue) -> Result<Value> {
    let data = decode(value)?;
    if requires_upload(&data["body"])
        || data["files"]
            .as_object()
            .is_some_and(|files| !files.is_empty())
    {
        return Err("行内富文本资源请使用已上传的 Telegram 文件ID".into());
    }
    Ok(json!({"rich_message":data["body"]}))
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
            Value::Object(value) => value
                .get("text")
                .or_else(|| value.get("alternative_text"))
                .or_else(|| value.get("expression"))
                .map(text)
                .unwrap_or_default(),
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
        for row in block["cells"].as_array().into_iter().flatten() {
            for cell in row.as_array().into_iter().flatten() {
                let part = text(&cell["text"]);
                if !part.is_empty() {
                    result.push(part);
                }
            }
        }
    }
    fn structure(block: &Value, depth: u8) -> Value {
        let kind = match block["type"].as_str() {
            Some("paragraph") => "段落",
            Some("heading") => "标题",
            Some("pre") => "代码",
            Some("footer") => "页脚",
            Some("divider") => "分隔线",
            Some("mathematical_expression") => "公式",
            Some("anchor") => "锚点",
            Some("pullquote") => "引文",
            Some("thinking") => "草稿提示",
            Some("list") => "列表",
            Some("blockquote") => "引用块",
            Some("expandable_blockquote") => "折叠引用",
            Some("collage") => "拼贴",
            Some("slideshow") => "幻灯片",
            Some("table") => "表格",
            Some("details") => "折叠详情",
            Some("map") => "地图",
            Some("buttons") => "按钮",
            Some("photo") => "图片",
            Some("video") => "视频",
            Some("animation") => "动画",
            Some("audio") => "音频",
            Some("document") => "文件",
            Some("voice_note") => "语音便笺",
            _ => "",
        };
        let children: Vec<_> = block["blocks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|_| depth > 0)
            .map(|child| structure(child, depth - 1))
            .collect();
        let items: Vec<_> = block["items"].as_array().into_iter().flatten().filter(|_| depth > 0).map(|item| {
            let parts: Vec<_> = item["blocks"].as_array().into_iter().flatten().map(|child| structure(child,depth - 1)).collect();
            json!({"内容块":parts,"编号":item["label"],"带复选框":item["has_checkbox"],"已勾选":item["is_checked"]})
        }).collect();
        let rows: Vec<_> = block["cells"].as_array().into_iter().flatten().map(|row| {
            row.as_array().into_iter().flatten().map(|cell| json!({
                "文字":text(&cell["text"]),"表头":cell["is_header"],"跨列":cell["colspan"],"跨行":cell["rowspan"],
                "横向":horizontal(&cell["align"]),"纵向":vertical(&cell["valign"])
            })).collect::<Vec<_>>()
        }).collect();
        let buttons: Vec<_> = block["buttons"].as_array().into_iter().flatten().map(|button| json!({
            "文字":text(&button["text"]),"链接":button["url"],"回调值":button["callback_data"],"复制文字":button["copy_text"]["text"],
            "小程序链接":button["web_app"]["url"],"登录链接":button["login_url"]["url"],
            "搜索词":button.get("switch_inline_query").or_else(||button.get("switch_inline_query_current_chat")).unwrap_or(&button["switch_inline_query_chosen_chat"]["query"]),
            "样式":match button["style"].as_str() { Some("primary")=>"强调",Some("danger")=>"危险",Some("success")=>"成功",Some("link")=>"文字",_=>"" },
            "禁用":button["disabled"].is_object()
        })).collect();
        let file = block["type"]
            .as_str()
            .map(|kind| &block[kind])
            .unwrap_or(&Value::Null);
        let file = file
            .as_array()
            .and_then(|photos| photos.last())
            .unwrap_or(file);
        json!({"类型":kind,"文字":text(&block["text"]),"说明":text(&block["caption"]),
               "出处":text(block.get("credit").unwrap_or(&block["caption"]["credit"])),
               "摘要":text(&block["summary"]),"公式":block["expression"],"名称":block["name"],"级别":block["size"],
               "语言":block["language"],"内容块":children,"项目":items,"表格行":rows,"按钮":buttons,"文件ID":file["file_id"],
               "经度":block["location"]["longitude"],"纬度":block["location"]["latitude"],"已展开":block["is_open"],
               "缩放":block["zoom"],"宽度":block.get("width").unwrap_or(&file["width"]),"高度":block.get("height").unwrap_or(&file["height"]),
               "时长":file["duration"],"文件名":file["file_name"],"媒体类型":file["mime_type"],"演出者":file["performer"],"媒体标题":file["title"],
               "封面ID":file["cover"].as_array().and_then(|items|items.last()).map(|item| &item["file_id"]),
               "缩略图ID":file["thumbnail"]["file_id"],"边框":block["is_bordered"],"条纹":block["is_striped"],
               "紧凑":block["is_compact"],"对齐":horizontal(&block["align"])})
    }
    fn horizontal(value: &Value) -> &str {
        match value.as_str() {
            Some("left") => "左",
            Some("center") => "居中",
            Some("right") => "右",
            _ => "",
        }
    }
    fn vertical(value: &Value) -> &str {
        match value.as_str() {
            Some("top") => "上",
            Some("middle") => "居中",
            Some("bottom") => "下",
            _ => "",
        }
    }
    let rich = &message["rich_message"];
    if !rich.is_object() {
        return Value::Null;
    }
    let mut parts = Vec::new();
    let blocks = rich["blocks"].as_array().into_iter().flatten();
    for block in blocks.clone() {
        block_text(block, &mut parts, 16);
    }
    let structure: Vec<_> = blocks.map(|block| structure(block, 16)).collect();
    json!({"正文":parts.join("\n"),"从右向左":rich["is_rtl"].as_bool().unwrap_or(false),"内容块":structure})
}
