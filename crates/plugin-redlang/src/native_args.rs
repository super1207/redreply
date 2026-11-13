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
    match input.as_ref() {
        RedValue::Text(text) if text.trim().is_empty() => Ok(None),
        RedValue::Text(text) => Ok(Some(legacy_object(text)?)),
        RedValue::Object(fields) => {
            let mut uploads = Map::new();
            for (field, file) in fields {
                let RedValue::Object(file) = file.as_ref() else {
                    return Err(RedLang::make_err(
                        "每个上传字段需使用含 filename 和 内容 的【对象】",
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
                uploads.insert(
                    field.clone(),
                    serde_json::json!({
                        "filename": name, "base64": STANDARD.encode(data.as_slice())
                    }),
                );
            }
            Ok(Some(Value::Object(uploads)))
        }
        _ => Err(RedLang::make_err("上传文件参数需使用【对象】")),
    }
}
