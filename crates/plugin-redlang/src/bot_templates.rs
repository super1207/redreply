use redlang::mytool::{cq_params_encode, str_msg_to_arr};

use super::*;

// Positional data is addressable as data['参数1'], data['参数2'], and so on.
fn variables(input: &RedValue) -> Result<Value> {
    let value = match input {
        RedValue::Text(text) if text.is_empty() => json!({}),
        RedValue::Array(items) => Value::Object(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    Ok((
                        format!("参数{}", index + 1),
                        crate::native_args::value(item)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        RedValue::Object(_) => crate::native_args::object(input)?,
        _ => return Err("模板参数请填【数组】、自定义变量【对象】或留空".into()),
    };
    Ok(value)
}

fn template_id(id: &str) -> Result<&str> {
    if id.is_empty() || id.chars().count() > 16 {
        return Err("模板ID请填 1 至 16 字".into());
    }
    Ok(id)
}

fn template_mode(value: &str) -> Result<u8> {
    match value {
        "" | "文字" => Ok(1),
        "卡片" => Ok(3),
        _ => Err("模板类型请填文字或卡片".into()),
    }
}

fn stored_template_source(kind: &str, value: &RedValue) -> Result<(u8, String)> {
    let mode = template_mode(kind)?;
    if mode == 3 {
        if let RedValue::Text(text) = value {
            if text.starts_with("[CQ:kook_card,") {
                let cards = card_for_edit(value)?;
                return Ok((2, cards.to_string()));
            }
        }
    }
    Ok((mode, value.expect_text_value()?))
}

fn template_record(data: &Value) -> Value {
    let kind = match data["msgtype"].as_i64() {
        Some(1) => "文字",
        Some(2 | 3) => "卡片",
        _ => "",
    };
    let test = data["test_data"]
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    json!({"模板ID":data["id"],"标题":data["title"],"正文":data["content"],"类型":kind,
        "测试参数":test,"测试频道ID":data["test_channel"]})
}

fn manage(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let op = rl.get_param_text(params, 0)?;
    match op.as_str() {
        "列表" => {
            let [page, size] = text_params::<2>(rl, params, 1)?;
            let mut body = json!({});
            if !page.is_empty() {
                body["page"] = positive_id(&page)?.into();
            }
            if !size.is_empty() {
                body["page_size"] = bounded(&size, 1, 50, "每页数量")?.into();
            }
            let response = kook(rl, "GET", "template/list", body)?;
            mapped_list(&response["data"]["items"], template_record)
        }
        "创建" | "更新" => {
            let update = op == "更新";
            let start = if update { 2 } else { 1 };
            let title = rl.get_param_text(params, start)?;
            let source = rl.get_param(params, start + 1)?;
            let kind = rl.get_param_text(params, start + 2)?;
            let (mode, source) = stored_template_source(&kind, &source)?;
            let sample = rl.get_param(params, start + 3)?;
            let channel = rl.get_param_text(params, start + 4)?;
            let mut body = json!({});
            if update {
                body["id"] = template_id(&rl.get_param_text(params, 1)?)?.into();
            } else {
                required(&title, "模板标题")?;
                required(&source, "模板正文")?;
                body["type"] = 0.into();
            }
            if title.chars().count() > 64 {
                return Err("模板标题最多 64 字".into());
            }
            if channel.chars().count() > 64 {
                return Err("测试频道ID最多 64 字".into());
            }
            insert_text(&mut body, "title", &title);
            insert_text(&mut body, "content", &source);
            if !update || !kind.is_empty() {
                body["msgtype"] = mode.into();
            }
            if !matches!(sample.as_ref(),RedValue::Text(text) if text.is_empty()) {
                body["test_data"] = variables(&sample)?.to_string().into();
            }
            if update && channel == "清空" {
                body["test_channel"] = "".into();
            } else {
                insert_text(&mut body, "test_channel", &channel);
            }
            if update && body.as_object().is_some_and(|fields| fields.len() == 1) {
                return Err("更新模板请至少填写一项要修改的内容".into());
            }
            let path = if update {
                "template/update"
            } else {
                "template/create"
            };
            let response = kook(rl, "POST", path, body)?;
            scalar_text(&response["data"]["model"]["id"])
        }
        "删除" => {
            let id = rl.get_param_text(params, 1)?;
            kook(
                rl,
                "POST",
                "template/delete",
                json!({"id":template_id(&id)?}),
            )?;
            empty()
        }
        _ => Err("模板管理请填列表、创建、更新或删除".into()),
    }
}

pub(crate) fn template_message(value: &RedValue) -> Result<Value> {
    let content = value.expect_text_value()?;
    let parsed = str_msg_to_arr(&json!(content))?;
    let [message] = parsed.as_array().map(Vec::as_slice).unwrap_or_default() else {
        return Err("模板编辑需要一条【KOOK模板消息】".into());
    };
    if message["type"] != "kook_template" {
        return Err("新内容请用【KOOK模板消息】".into());
    }
    Ok(serde_json::from_str(
        message["data"]["content"]
            .as_str()
            .ok_or("模板消息缺少内容")?,
    )?)
}

pub(super) fn register() {
    add_fun(vec!["KOOK模板管理"], manage);
    add_fun(vec!["KOOK模板消息"], |rl, params| {
        if get_platform(rl) != "kook" {
            return Err("模板消息需要 KOOK 机器人来源".into());
        }
        let id = rl.get_param_text(params, 0)?;
        let input = rl.get_param(params, 1)?;
        let kind = rl.get_param_text(params, 2)?;
        let kind = if template_mode(&kind)? == 1 { 9 } else { 10 };
        let data = json!({"template_id":template_id(&id)?,"content":variables(&input)?.to_string(),"type":kind});
        Ok(Some(rv_text(format!(
            "[CQ:kook_template,content={}]",
            cq_params_encode(&data.to_string())
        ))))
    });
    add_fun(vec!["KOOK管道消息"], |rl, params| {
        let token = rl.get_param_text(params, 0)?;
        required(&token, "入管道令牌")?;
        let content = rl.get_param(params, 1)?;
        let [kind, target] = text_params::<2>(rl, params, 2)?;
        let (body, ty) = match kind.as_str() {
            "模板" => (variables(&content)?, None),
            "卡片" => (
                json!({"content":card_for_edit(&content)?.to_string()}),
                Some(10),
            ),
            "" | "文字" => {
                let value = content.expect_text_value()?;
                (json!({"content":required(&value,"管道消息正文")?}), Some(9))
            }
            _ => return Err("管道类型请填文字、卡片或模板".into()),
        };
        let mut args = json!({"token":token,"body":body});
        if let Some(ty) = ty {
            args["type"] = ty.into();
        }
        insert_text(&mut args, "target_id", &target);
        let response = success(call_platform_api_value(
            rl,
            "kook",
            "kook_pipe_message",
            args,
        )?)?;
        scalar_text(&response["data"]["msg_id"])
    });
}
