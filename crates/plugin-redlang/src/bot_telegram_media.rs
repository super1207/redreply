use super::*;
use crate::native_args;
use redlang::mytool::cq_params_encode;
use serde_json::Map;

fn number(
    body: &mut Value,
    field: &str,
    value: &str,
    min: i64,
    max: i64,
    label: &str,
) -> Result<()> {
    if !value.is_empty() {
        body[field] = value
            .parse::<i64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("{label}请填写 {min} 至 {max} 的整数"))?
            .into();
    }
    Ok(())
}

fn caption(body: &mut Value, text: &str, format: &str) -> Result<()> {
    if !text.is_empty() {
        body["caption"] = text.into();
    }
    let mode = match format {
        "" | "纯文本" => return Ok(()),
        "Markdown" => "MarkdownV2",
        "HTML" => "HTML",
        _ => return Err("说明格式请填纯文本、Markdown 或 HTML".into()),
    };
    body["parse_mode"] = mode.into();
    Ok(())
}

fn thumbnail(body: &mut Value, files: &mut Map<String, Value>, value: &RedValue) -> Result<()> {
    match value {
        RedValue::Text(text) if text.is_empty() => Ok(()),
        RedValue::Bin(bytes) if !files.is_empty() => {
            if bytes.len() >= 200_000 || !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return Err("缩略图需为小于 200 kB 的 JPEG 字节集".into());
            }
            body["thumbnail"] = "attach://preview".into();
            files.insert(
                "preview".into(),
                native_args::file_part("thumbnail.jpg", bytes, Some("image/jpeg")),
            );
            Ok(())
        }
        _ => Err("缩略图仅用于上传媒体，需使用【读文件】取得 JPEG 字节集".into()),
    }
}

pub(crate) fn segment(
    rl: &mut RedLang,
    params: &[Ast],
    source: &RedValue,
    kind: &str,
) -> RedCommandResult {
    if get_platform(rl) != "telegram" {
        return Err("便笺和动画需要 Telegram 机器人来源".into());
    }
    let position = if kind == "animation" { 1 } else { 2 };
    let filename = rl.get_param_text(params, position)?;
    let mut files = Map::new();
    let resource = native_args::media_source(source, &filename, "media", &mut files)?;
    if let Some(link) = resource
        .as_str()
        .filter(|value| files.is_empty() && value.contains("://"))
    {
        if kind == "video_note" {
            return Err("视频便笺来源需填写 Telegram 文件 ID 或字节集".into());
        }
        let url = url::Url::parse(link)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err("媒体直链请填写 HTTP(S) 地址".into());
        }
    }
    let mut body = json!({kind: resource});
    match kind {
        "voice" => {
            let [description, duration, format] = text_params::<3>(rl, params, 3)?;
            caption(&mut body, &description, &format)?;
            number(&mut body, "duration", &duration, 0, i64::MAX, "时长")?;
        }
        "video_note" => {
            let [duration, diameter] = text_params::<2>(rl, params, 3)?;
            number(&mut body, "duration", &duration, 0, 60, "时长")?;
            number(&mut body, "length", &diameter, 1, i64::MAX, "直径")?;
            thumbnail(&mut body, &mut files, rl.get_param(params, 5)?.as_ref())?;
        }
        "animation" => {
            let [description, duration, width, height, format, above, spoiler] =
                text_params::<7>(rl, params, 2)?;
            caption(&mut body, &description, &format)?;
            number(&mut body, "duration", &duration, 0, i64::MAX, "时长")?;
            number(&mut body, "width", &width, 1, i64::MAX, "宽度")?;
            number(&mut body, "height", &height, 1, i64::MAX, "高度")?;
            if !above.is_empty() {
                body["show_caption_above_media"] = boolean(&above, false)?.into();
            }
            if !spoiler.is_empty() {
                body["has_spoiler"] = boolean(&spoiler, false)?.into();
            }
            thumbnail(&mut body, &mut files, rl.get_param(params, 9)?.as_ref())?;
        }
        _ => return Err("未知 Telegram 媒体类型".into()),
    }
    let content = cq_params_encode(&json!({"kind":kind,"body":body,"files":files}).to_string());
    Ok(Some(rv_text(format!(
        "[CQ:telegram_media,content={content}]"
    ))))
}

pub(super) fn event_fields(message: &Value, kind: &str) -> Value {
    let media = &message[kind];
    if !media.is_object() {
        return Value::Null;
    }
    json!({"文件ID":media["file_id"],"文件唯一ID":media["file_unique_id"],"文件大小":media["file_size"],
           "时长":media["duration"],"宽度":media["width"],"高度":media["height"],"直径":media["length"],
           "文件名":media["file_name"],"媒体类型":media["mime_type"],"缩略图文件ID":media["thumbnail"]["file_id"],
           "说明":message["caption"],"说明在上方":message["show_caption_above_media"],"防剧透":message["has_media_spoiler"]})
}
