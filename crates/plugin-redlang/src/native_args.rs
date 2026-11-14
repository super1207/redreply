use std::error::Error;
use std::rc::Rc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{Map, Number, Value};

use crate::core::{astparser::Ast, rv_object, rv_text, RedLang, RedValue};

// RedLang uses text for scalars. Mark explicitly typed values without changing text conversion
// for ordinary RedLang objects (numeric IDs, callback payloads and option labels can be text).
const SCALAR_KEY: &str = "__redreply_api_scalar";

pub(super) fn scalar(kind: &str, text: &str) -> Result<Rc<RedValue>, Box<dyn Error>> {
    let value = match kind {
        "整数" => text
            .parse::<Number>()
            .ok()
            .filter(|n| n.is_i64() || n.is_u64())
            .map(Value::Number),
        "小数" => text.parse::<Number>().ok().map(Value::Number),
        "逻辑" => match text {
            "真" => Some(Value::Bool(true)),
            "假" => Some(Value::Bool(false)),
            _ => None,
        },
        "空值" if text.is_empty() => Some(Value::Null),
        _ => None,
    }
    .ok_or_else(|| RedLang::make_err("【接口值】需要整数、小数、逻辑（真或假）或空值"))?;
    Ok(rv_object(
        [(SCALAR_KEY.to_owned(), rv_text(value.to_string()))].into(),
    ))
}

pub(super) fn value(input: &RedValue) -> Result<Value, Box<dyn Error>> {
    fn convert(input: &RedValue, depth: usize) -> Result<Value, Box<dyn Error>> {
        // Keep the generated request within the wire decoder's nesting limit.
        if depth > 120 {
            return Err(RedLang::make_err("原生接口参数嵌套过深"));
        }
        match input {
            RedValue::Text(text) => Ok(Value::String(text.as_ref().clone())),
            RedValue::Array(items) => items
                .iter()
                .map(|item| convert(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            RedValue::Object(fields) => {
                if let Some(encoded) = fields.get(SCALAR_KEY) {
                    if fields.len() != 1 {
                        return Err(RedLang::make_err("【接口值】不能与其他字段合并"));
                    }
                    let value: Value = serde_json::from_str(&encoded.expect_text_value()?)?;
                    return match value {
                        Value::Bool(_) | Value::Number(_) | Value::Null => Ok(value),
                        _ => Err(RedLang::make_err("无效的【接口值】")),
                    };
                }
                fields
                    .iter()
                    .map(|(key, item)| Ok((key.clone(), convert(item, depth + 1)?)))
                    .collect::<Result<Map<_, _>, _>>()
                    .map(Value::Object)
            }
            _ => Err(RedLang::make_err(
                "接口参数使用文本、数组或对象，字节集请放在上传文件对象中",
            )),
        }
    }
    convert(input, 0)
}

fn legacy_object(text: &str) -> Result<Value, Box<dyn Error>> {
    let value: Value = serde_json::from_str(text)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(RedLang::make_err(
            "原生接口参数需使用【对象】或旧版 JSON 对象",
        ))
    }
}

pub(super) fn body(
    rl: &mut RedLang,
    params: &[Ast],
    index: usize,
) -> Result<Value, Box<dyn Error>> {
    let input = rl.get_param(params, index)?;
    match input.as_ref() {
        RedValue::Text(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        RedValue::Text(text) => legacy_object(text),
        _ => object(&input),
    }
}

pub(super) fn object(input: &RedValue) -> Result<Value, Box<dyn Error>> {
    if matches!(input, RedValue::Object(_)) {
        let converted = value(input)?;
        if converted.is_object() {
            return Ok(converted);
        }
    }
    Err(RedLang::make_err("接口参数需使用【对象】"))
}

pub(super) fn files(
    rl: &mut RedLang,
    params: &[Ast],
    index: usize,
) -> Result<Option<Value>, Box<dyn Error>> {
    let input = rl.get_param(params, index)?;
    files_value(&input)
}

pub(super) fn files_value(input: &RedValue) -> Result<Option<Value>, Box<dyn Error>> {
    match input {
        RedValue::Text(text) if text.trim().is_empty() => Ok(None),
        RedValue::Text(text) => Ok(Some(legacy_object(text)?)),
        RedValue::Object(fields) => {
            let mut uploads = Map::new();
            for (field, file) in fields {
                uploads.insert(field.clone(), upload_file(file)?);
            }
            Ok(Some(Value::Object(uploads)))
        }
        _ => Err(RedLang::make_err("上传文件参数需使用【对象】")),
    }
}

pub(super) fn upload_file(input: &RedValue) -> Result<Value, Box<dyn Error>> {
    let RedValue::Object(file) = input else {
        return Err(RedLang::make_err(
            "上传文件需使用含 filename 和 内容 的【对象】",
        ));
    };
    let name = file
        .get("filename")
        .ok_or("上传文件缺少 filename")?
        .expect_text_value()?;
    if name.is_empty() {
        return Err(RedLang::make_err("上传文件的 filename 不能为空"));
    }
    let data = match file.get("内容").map(Rc::as_ref) {
        Some(RedValue::Bin(bytes)) => bytes,
        _ => return Err(RedLang::make_err("上传文件的 内容 需要字节集")),
    };
    Ok(file_part(
        &name,
        data,
        file.get("mime_type")
            .map(|value| value.expect_text_value())
            .transpose()?
            .as_deref(),
    ))
}

pub(super) fn file_part(name: &str, data: &[u8], mime: Option<&str>) -> Value {
    let mut upload = serde_json::json!({"filename":name,"base64":STANDARD.encode(data)});
    if let Some(mime) = mime {
        upload["mime_type"] = mime.into();
    }
    upload
}

pub(super) fn media_source(
    source: &RedValue,
    filename: &str,
    key: &str,
    files: &mut Map<String, Value>,
) -> Result<Value, Box<dyn Error>> {
    if let RedValue::Bin(bytes) = source {
        if filename.is_empty() {
            return Err(RedLang::make_err("媒体来源是字节集时需填写文件名"));
        }
        files.insert(key.to_owned(), file_part(filename, bytes, None));
        return Ok(format!("attach://{key}").into());
    }
    let text = source.expect_text_value()?;
    if text.is_empty() || !filename.is_empty() {
        return Err(RedLang::make_err(
            "请填写媒体链接或文件 ID，文件名仅用于字节集",
        ));
    }
    Ok(text.into())
}

pub(super) fn media_group(input: &RedValue) -> Result<Value, Box<dyn Error>> {
    let RedValue::Array(items) = input else {
        return Err(RedLang::make_err("【媒体组】需要媒体【数组】"));
    };
    if !(2..=10).contains(&items.len()) {
        return Err(RedLang::make_err("【媒体组】需要 2 至 10 项"));
    }
    let mut media = Vec::with_capacity(items.len());
    let mut files = Map::new();
    for (index, item) in items.iter().enumerate() {
        let (row, attachments) = media_item(item, index, false)?;
        media.push(row);
        files.extend(attachments);
    }
    Ok(serde_json::json!({"media":media,"files":files}))
}

pub(super) fn media_item(
    item: &RedValue,
    index: usize,
    allow_animation: bool,
) -> Result<(Value, Map<String, Value>), Box<dyn Error>> {
    let mut files = Map::new();
    let RedValue::Array(fields) = item else {
        return Err(RedLang::make_err(
            "每项媒体用【数组@类型@来源@说明@文件名】填写",
        ));
    };
    let [kind, source, ..] = fields.as_slice() else {
        return Err(RedLang::make_err("媒体项需要类型和来源"));
    };
    let text = |index: usize| -> Result<String, Box<dyn Error>> {
        fields
            .get(index)
            .map(|value| value.expect_text_value())
            .transpose()
            .map(Option::unwrap_or_default)
    };
    let kind = match kind.expect_text_value()?.as_str() {
        "图片" => "photo",
        "视频" => "video",
        "音频" => "audio",
        "文件" => "document",
        "实况照片" => "live_photo",
        "动画" if allow_animation => "animation",
        "动画" => return Err(RedLang::make_err("动画需单独发送，不适用于媒体组")),
        _ => {
            return Err(RedLang::make_err(
                "媒体类型可填图片、视频、音频、文件或实况照片",
            ))
        }
    };
    if fields.len() > if kind == "live_photo" { 6 } else { 4 } {
        return Err(RedLang::make_err("媒体项参数过多"));
    }
    let source = media_source(source, &text(3)?, &format!("media_{index}"), &mut files)?;
    let mut row = serde_json::json!({"type":kind,"media":source});
    let caption = text(2)?;
    if !caption.is_empty() {
        row["caption"] = caption.into();
    }
    if kind == "live_photo" {
        let photo = fields.get(4).ok_or("实况照片需要静态图来源")?;
        row["photo"] = media_source(photo, &text(5)?, &format!("photo_{index}"), &mut files)?;
    }
    Ok((row, files))
}

pub(super) fn edit_media(item: &RedValue) -> Result<(Value, Map<String, Value>), Box<dyn Error>> {
    let fields = item.as_array().ok_or("编辑媒体需要【数组】")?;
    let live = fields
        .first()
        .map(|v| v.expect_text_value())
        .transpose()?
        .as_deref()
        == Some("实况照片");
    let base = if live { 6 } else { 4 };
    if fields.len() <= base {
        return media_item(item, 0, true);
    }
    if fields.len() != base + 1 {
        return Err("媒体选项请放在媒体数组最后一项".into());
    }
    let (mut media, mut files) = media_item(&RedValue::Array(fields[..base].to_vec()), 0, true)?;
    let options = fields[base].as_array().ok_or("媒体选项需要【数组】")?;
    let kind = media["type"].as_str().unwrap_or_default().to_owned();
    let value = |index: usize| -> Result<String, Box<dyn Error>> {
        Ok(options
            .get(index)
            .map(|v| v.expect_text_value())
            .transpose()?
            .unwrap_or_default())
    };
    let layout: &[(&str, char)] = match kind.as_str() {
        "photo" => &[("has_spoiler", 'b')],
        "animation" => &[
            ("duration", 'n'),
            ("width", 'p'),
            ("height", 'p'),
            ("has_spoiler", 'b'),
            ("thumbnail", 'j'),
        ],
        "video" => &[
            ("duration", 'n'),
            ("width", 'p'),
            ("height", 'p'),
            ("has_spoiler", 'b'),
            ("thumbnail", 'j'),
            ("supports_streaming", 'b'),
            ("start_timestamp", 'n'),
            ("cover", 'm'),
        ],
        "audio" => &[
            ("duration", 'n'),
            ("performer", 't'),
            ("title", 't'),
            ("thumbnail", 'j'),
        ],
        "document" => &[("disable_content_type_detection", 'b'), ("thumbnail", 'j')],
        "live_photo" => &[("has_spoiler", 'b')],
        _ => return Err("未知媒体类型".into()),
    };
    if options.len() > layout.len() {
        return Err("媒体选项过多".into());
    }
    for (index, &(field, format)) in layout.iter().enumerate().take(options.len()) {
        let option = &options[index];
        if matches!(option.as_ref(),RedValue::Text(value) if value.is_empty()) {
            continue;
        }
        media[field] = match format {
            'b' => match value(index)?.as_str() {
                "真" => true.into(),
                "假" => false.into(),
                _ => return Err("媒体开关请填真或假".into()),
            },
            'n' | 'p' => {
                let n: u32 = value(index)?.parse()?;
                if format == 'p' && n == 0 {
                    return Err("媒体宽高请填正整数".into());
                }
                n.into()
            }
            'j' => {
                if !files.contains_key("media_0") {
                    return Err("上传缩略图时也需上传媒体文件".into());
                }
                let RedValue::Bin(bytes) = option.as_ref() else {
                    return Err("缩略图请用 JPEG 字节集".into());
                };
                if bytes.len() >= 200_000 || !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                    return Err("缩略图须为小于 200 kB 的 JPEG".into());
                }
                files.insert(
                    "preview".into(),
                    file_part("thumbnail.jpg", bytes, Some("image/jpeg")),
                );
                "attach://preview".into()
            }
            'm' => {
                if let Some(row) = option.as_array() {
                    let [source, name] = row.as_slice() else {
                        return Err("上传封面请填【数组@字节集@文件名】".into());
                    };
                    media_source(source, &name.expect_text_value()?, "cover", &mut files)?
                } else {
                    value(index)?.into()
                }
            }
            _ => value(index)?.into(),
        };
    }
    Ok((media, files))
}

pub(super) fn edit_attachments(input: &RedValue, files: &mut Value) -> Result<(), Box<dyn Error>> {
    let rows = input.as_array().ok_or("附件列表需要【数组】")?;
    let files = files.as_object_mut().ok_or("附件结构无效")?;
    for row in rows {
        let Some([name, bytes, filename]) = row.as_array().map(Vec::as_slice) else {
            return Err("附件用【数组@标识@字节集@文件名】填写".into());
        };
        let name = name.expect_text_value()?;
        let filename = filename.expect_text_value()?;
        let RedValue::Bin(bytes) = bytes.as_ref() else {
            return Err("附件内容请用字节集".into());
        };
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || filename.is_empty()
            || files.contains_key(&name)
        {
            return Err("附件标识或文件名无效，或标识已重复".into());
        }
        files.insert(name, file_part(&filename, bytes, None));
    }
    Ok(())
}
