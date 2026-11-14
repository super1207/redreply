use super::*;

pub(crate) fn parse(rl: &mut RedLang, params: &[Ast], index: usize) -> Result<Value> {
    let value = rl.get_param(params, index)?;
    if matches!(value.as_ref(), RedValue::Text(text) if text.is_empty()) {
        return Ok(json!({}));
    }
    let values = array(&value)?;
    if values.len() > 6 {
        return Err("发送选项最多六项".into());
    }
    let mut result = json!({});
    for (index, field) in [(0, "disable_notification"), (1, "protect_content")] {
        let value = row_text(values, index)?;
        if !value.is_empty() {
            result[field] = boolean(&value, false)?.into();
        }
    }
    let effect = row_text(values, 2)?;
    if !effect.is_empty() {
        if effect.parse::<u64>().ok().filter(|id| *id > 0).is_none() {
            return Err("消息效果ID请填正整数".into());
        }
        result["message_effect_id"] = effect.into();
    }
    if let Some(post) = values.get(3).filter(|value| !absent(value)) {
        let fields = array(post)?;
        if fields.len() > 3 {
            return Err("建议帖选项为【数组@币种@价格@发布时间】".into());
        }
        let currency = row_text(fields, 0)?;
        let price = row_text(fields, 1)?;
        let date = row_text(fields, 2)?;
        let mut post = json!({});
        if !currency.is_empty() || !price.is_empty() {
            let (currency, min, max) = match currency.as_str() {
                "" | "星币" => ("XTR", 5, 100_000),
                "TON" => ("TON", 10_000_000, 10_000_000_000_000),
                _ => return Err("建议帖币种请填星币或 TON".into()),
            };
            let price = price
                .parse::<i64>()
                .ok()
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| format!("建议帖价格请填 {min} 至 {max} 的整数"))?;
            post["price"] = json!({"currency":currency,"amount":price});
        }
        if !date.is_empty() {
            post["send_date"] = publish_date(&date, 300)?.into();
        }
        result["suggested_post_parameters"] = post;
    }
    if let Some(ephemeral) = values.get(4).filter(|value| !absent(value)) {
        let fields = array(ephemeral)?;
        if fields.len() > 3 || result.get("suggested_post_parameters").is_some() {
            return Err("临时消息使用【数组@接收者ID@回调ID@替换原消息】，请省略建议帖".into());
        }
        let user = row_text(fields, 0)?;
        let callback = row_text(fields, 1)?;
        let replace = boolean(&row_text(fields, 2)?, false)?;
        let event: Value = serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
        let mut ephemeral = json!({});
        let callback = if callback == "当前"
            || (callback.is_empty() && user.is_empty() && event["sub_type"] == "callback_query")
        {
            event["telegram"]["id"]
                .as_str()
                .filter(|id| event["sub_type"] == "callback_query" && !id.is_empty())
                .ok_or("当前没有 Telegram 按钮回调")?
        } else {
            &callback
        };
        if !callback.is_empty() {
            ephemeral["callback_query_id"] = callback.into();
        }
        if !user.is_empty() || callback.is_empty() {
            let sender = rl.get_exmap("发送者ID");
            ephemeral["receiver_user_id"] =
                positive_id(if user.is_empty() { &sender } else { &user })?.into();
        }
        if replace {
            if callback.is_empty()
                || event["telegram"]["message"]["ephemeral_message_id"].is_number()
            {
                return Err("替换原消息需要普通按钮回调".into());
            }
            ephemeral["replace_callback_query_message"] = true.into();
        }
        if callback.is_empty()
            && matches!(
                event["sub_type"].as_str(),
                Some("ephemeral_message" | "edited_ephemeral_message")
            )
            && event["self_id"].as_str() == Some(&rl.get_exmap("机器人ID"))
            && event["chat_id"].as_str() == Some(&rl.get_exmap("群ID"))
            && event["telegram"]["from"]["id"] == ephemeral["receiver_user_id"]
        {
            result["ephemeral_reply_id"] = event["telegram"]["ephemeral_message_id"].clone();
        }
        result["ephemeral_message_parameters"] = ephemeral;
    }
    if let Some(reply) = values.get(5).filter(|value| !absent(value)) {
        result["reply_parameters"] = reply_fields(array(reply)?, rl)?;
    }
    Ok(result)
}

fn reply_fields(row: &[Rc<RedValue>], rl: &RedLang) -> Result<Value> {
    if row.len() > 10 {
        return Err("引用选项最多十项".into());
    }
    let target = row_text(row, 0)?;
    let chat = row_text(row, 1)?;
    let text = row_text(row, 2)?;
    let position = row_text(row, 3)?;
    let fallback = row_text(row, 4)?;
    let task = row_text(row, 5)?;
    let poll = row_text(row, 6)?;
    let format = row_text(row, 7)?;
    if !task.is_empty() && !poll.is_empty() {
        return Err("清单任务和投票选项只能指定一项".into());
    }
    let event: Value = serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
    let current = management::business::event_message(&event);
    let mut body = if target.is_empty() || target == "当前" {
        if !chat.is_empty() {
            return Err("跨会话引用请明确填写平台消息ID".into());
        }
        if let Some(id) = current["ephemeral_message_id"]
            .as_i64()
            .filter(|id| *id > 0)
        {
            json!({"ephemeral_message_id":id})
        } else {
            let id = current["message_id"]
                .as_i64()
                .filter(|id| *id > 0)
                .ok_or("请填写要引用的平台消息ID")?;
            json!({"message_id":id})
        }
    } else {
        json!({"message_id":positive_id(&target)?})
    };
    if !chat.is_empty() {
        body["chat_id"] = chat_id(&chat)?;
    }
    if !text.is_empty() {
        if text.chars().count() > 1024 && format.is_empty() {
            return Err("引用片段最多 1024 字符".into());
        }
        body["quote"] = text.into();
    }
    if !position.is_empty() {
        body["quote_position"] = position.parse::<u32>()?.into();
    }
    if !fallback.is_empty() {
        body["allow_sending_without_reply"] = boolean(&fallback, false)?.into();
    }
    if !task.is_empty() {
        body["checklist_task_id"] = positive_id(&task)?.into();
    }
    if !poll.is_empty() {
        body["poll_option_id"] = poll.into();
    }
    if !format.is_empty() {
        if body.get("quote").is_none() {
            return Err("引用格式需要填写引用片段".into());
        }
        body["quote_parse_mode"] = match format.as_str() {
            "Markdown" => "MarkdownV2",
            "HTML" => "HTML",
            _ => return Err("引用格式请填 Markdown 或 HTML".into()),
        }
        .into();
    }
    if let Some(parts) = row.get(8).filter(|value| !absent(value)) {
        if body.get("quote").is_some() {
            return Err("富文本引文和普通引用片段请二选一".into());
        }
        let mut texts = Vec::new();
        for part in array(parts)? {
            if let Some(row) = part.as_array() {
                if !(2..=4).contains(&row.len()) {
                    return Err("富文本引文可填文本或【数组@样式@文本】".into());
                }
                let label = row_text(row, 0)?;
                let text = row_text(row, 1)?;
                let detail = row_text(row, 2)?;
                let kind = match label.as_str() {
                    "粗体" => "bold",
                    "斜体" => "italic",
                    "下划线" => "underline",
                    "删除线" => "strikethrough",
                    "剧透" => "spoiler",
                    "下标" => "subscript",
                    "上标" => "superscript",
                    "自定义表情" if !detail.is_empty() => {
                        texts.push(json!({"type":"custom_emoji","alternative_text":text,"custom_emoji_id":detail}));
                        continue;
                    }
                    "公式" => {
                        texts.push(json!({"type":"mathematical_expression","expression":text}));
                        continue;
                    }
                    "日期时间" => {
                        let mut entity = json!({"type":"date_time","text":text,"unix_time":detail.parse::<i64>()?});
                        let format = row_text(row, 3)?;
                        if !format.is_empty() {
                            entity["date_time_format"] = format.into();
                        }
                        texts.push(entity);
                        continue;
                    }
                    _ => return Err("未知的富文本引文样式".into()),
                };
                texts.push(json!({"type":kind,"text":text}));
            } else {
                texts.push(part.expect_text_value()?.into());
            }
        }
        if !texts.is_empty() {
            body["quote_rich_texts"] = texts.into();
        }
    }
    if let Some(entities) = row.get(9).filter(|value| !absent(value)) {
        let quote = body["quote"].as_str().ok_or("引用样式需要普通引用片段")?;
        if body.get("quote_parse_mode").is_some() {
            return Err("引用格式与引用样式请二选一".into());
        }
        let boundaries: HashSet<_> = std::iter::once(0)
            .chain(quote.chars().scan(0, |offset, ch| {
                *offset += ch.len_utf16();
                Some(*offset)
            }))
            .collect();
        let entities = array(entities)?
            .iter()
            .map(|entity| {
                let row = array(entity)?;
                if !(3..=5).contains(&row.len()) {
                    return Err("引用样式请填【数组@样式@位置@长度@附加值@日期格式】".into());
                }
                let offset = row_text(row, 1)?.parse::<usize>()?;
                let length = row_text(row, 2)?.parse::<usize>()?;
                if length == 0
                    || !boundaries.contains(&offset)
                    || offset
                        .checked_add(length)
                        .is_none_or(|end| !boundaries.contains(&end))
                {
                    return Err("引用样式范围超出引用片段的 UTF-16 长度".into());
                }
                let kind = match row_text(row, 0)?.as_str() {
                    "粗体" => "bold",
                    "斜体" => "italic",
                    "下划线" => "underline",
                    "删除线" => "strikethrough",
                    "剧透" => "spoiler",
                    "自定义表情" => "custom_emoji",
                    "日期时间" => "date_time",
                    _ => return Err("未知的普通引用样式".into()),
                };
                let mut item = json!({"type":kind,"offset":offset,"length":length});
                if kind == "custom_emoji" {
                    let id = row_text(row, 3)?;
                    if id.is_empty() {
                        return Err("请填写自定义表情ID".into());
                    }
                    item["custom_emoji_id"] = id.into();
                } else if kind == "date_time" {
                    item["unix_time"] = row_text(row, 3)?.parse::<i64>()?.into();
                    let format = row_text(row, 4)?;
                    if !format.is_empty() {
                        item["date_time_format"] = format.into();
                    }
                }
                Ok(item)
            })
            .collect::<Result<Vec<_>>>()?;
        body["quote_entities"] = entities.into();
    }
    Ok(body)
}

pub(crate) fn edit_fields(value: &RedValue, kind: &str) -> Result<Value> {
    let fields = array(value)?;
    if fields.len() > 3 {
        return Err("编辑选项最多三项".into());
    }
    let mut body = json!({});
    let format = row_text(fields, 0)?;
    if !format.is_empty() {
        if !matches!(kind, "" | "文本" | "说明" | "媒体") {
            return Err("格式仅适用于文本、说明或媒体".into());
        }
        body["parse_mode"] = match format.as_str() {
            "Markdown" => "MarkdownV2",
            "HTML" => "HTML",
            _ => return Err("格式请填 Markdown、HTML 或留空".into()),
        }
        .into();
    }
    if let Some(preview) = fields.get(1).filter(|value| !absent(value)) {
        if !matches!(kind, "" | "文本") {
            return Err("链接预览仅适用于 Telegram 文本".into());
        }
        let preview = array(preview)?;
        if preview.len() > 4 {
            return Err("预览请填【数组@关闭@链接@尺寸@放在上方】".into());
        }
        let mut data = json!({});
        for (index, key) in [(0, "is_disabled"), (3, "show_above_text")] {
            let value = row_text(preview, index)?;
            if !value.is_empty() {
                data[key] = boolean(&value, false)?.into();
            }
        }
        let url = row_text(preview, 1)?;
        if !url.is_empty() {
            let parsed = url::Url::parse(&url)?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err("预览链接请填 HTTP(S) 地址".into());
            }
            data["url"] = url.into();
        }
        match row_text(preview, 2)?.as_str() {
            "" => (),
            "小" => data["prefer_small_media"] = true.into(),
            "大" => data["prefer_large_media"] = true.into(),
            _ => return Err("预览尺寸请填大、小或留空".into()),
        }
        body["link_preview_options"] = data;
    }
    let above = row_text(fields, 2)?;
    if !above.is_empty() {
        if !matches!(kind, "说明" | "媒体") {
            return Err("说明置顶仅适用于说明或媒体".into());
        }
        body["show_caption_above_media"] = boolean(&above, false)?.into();
    }
    Ok(body)
}

pub(crate) fn forward_fields(input: &RedValue) -> Result<Value> {
    if absent(input) {
        return Ok(json!({}));
    }
    let row = array(input)?;
    if row.len() > 7 {
        return Err("转发附加选项最多七项".into());
    }
    let mut body = json!({});
    let start = row_text(row, 0)?;
    if !start.is_empty() {
        body["video_start_timestamp"] = start.parse::<u32>()?.into();
    }
    if let Some(value) = row.get(1).filter(|value| !absent(value)) {
        let value = value.expect_text_value()?;
        body["caption"] = (if value == "清空" { "" } else { &value }).into();
    }
    let format = row_text(row, 2)?;
    if !format.is_empty() {
        body["parse_mode"] = match format.as_str() {
            "Markdown" => "MarkdownV2",
            "HTML" => "HTML",
            _ => return Err("说明格式请填 Markdown、HTML 或留空".into()),
        }
        .into();
    }
    for (index, field) in [(3, "show_caption_above_media"), (4, "remove_caption")] {
        let value = row_text(row, index)?;
        if !value.is_empty() {
            body[field] = boolean(&value, false)?.into();
        }
    }
    if let Some(markup) = row.get(5).filter(|value| !absent(value)) {
        body["reply_markup"] = super::telegram_keyboard_value(markup)?;
    }
    let source = row_text(row, 6)?;
    if !source.is_empty() {
        body["from_chat_id"] = chat_id(&source)?;
    }
    Ok(body)
}

fn absent(value: &RedValue) -> bool {
    matches!(value, RedValue::Text(text) if text.is_empty())
}

fn publish_date(date: &str, minimum: i64) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();
    date.parse::<i64>()
        .ok()
        .filter(|date| (now + minimum..=now + 2_678_400).contains(date))
        .ok_or_else(|| "发布时间请填官方时间范围内的 Unix 秒".into())
}

pub(crate) fn approve(
    rl: &mut RedLang,
    params: &[Ast],
    event: &Value,
    approve: bool,
) -> RedCommandResult {
    let [value, id, target] = text_params::<3>(rl, params, 0)?;
    let message = &event["telegram"];
    let id = if id.is_empty() {
        message["message_id"]
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or("请填写建议帖平台消息ID")?
    } else {
        positive_id(&id)?
    };
    let target = if target.is_empty() {
        message["chat"]["id"]
            .as_i64()
            .ok_or("请填写频道私信会话ID")?
    } else {
        target.parse::<i64>()?
    };
    let mut body = json!({"chat_id":target,"message_id":id});
    let method = if approve {
        if !value.is_empty() {
            body["send_date"] = publish_date(&value, 0)?.into();
        }
        "approveSuggestedPost"
    } else {
        if value.chars().count() > 128 {
            return Err("拒绝建议帖的留言最多 128 字".into());
        }
        if !value.is_empty() {
            body["comment"] = value.into();
        }
        "declineSuggestedPost"
    };
    telegram(rl, method, body)?;
    Ok(Some(rv_empty()))
}

pub(crate) fn simple_options(
    options: &Value,
    body: &mut Value,
    private: bool,
    suggested: bool,
) -> Result<()> {
    if options.get("ephemeral_message_parameters").is_some() {
        return Err("请用【输出流】发送临时消息".into());
    }
    if options.get("suggested_post_parameters").is_some()
        && (!suggested || !body["direct_messages_topic_id"].is_number())
    {
        return Err("建议帖需要支持建议帖的消息类型和频道私信话题".into());
    }
    if options.get("message_effect_id").is_some() && !private {
        return Err("消息效果只适用于私聊".into());
    }
    let reply = &options["reply_parameters"];
    if reply.get("ephemeral_message_id").is_some()
        || reply.get("chat_id").is_some()
            && (body.get("business_connection_id").is_some()
                || body.get("direct_messages_topic_id").is_some()
                || reply["allow_sending_without_reply"] == true)
    {
        return Err("当前发送方式不支持此引用目标或失败后的继续发送选项".into());
    }
    for (field, value) in options.as_object().ok_or("Telegram 发送选项格式无效")? {
        body[field] = value.clone();
    }
    Ok(())
}

pub(crate) fn ephemeral_target(
    rl: &RedLang,
    id: &str,
    receiver: &str,
    target: &str,
) -> Result<Value> {
    let evt: Value = serde_json::from_str(&rl.get_exmap("原始事件")).unwrap_or_default();
    let msg = management::business::event_message(&evt);
    let number = |value: &str, field: &Value, label: &str| {
        if value.is_empty() {
            field
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("请填写{label}").into())
        } else {
            positive_id(value)
        }
    };
    let id = number(id, &msg["ephemeral_message_id"], "临时消息ID")?;
    let receiver = number(receiver, &msg["receiver_user"]["id"], "接收者ID")?;
    let chat = if target.is_empty() {
        msg["chat"]["id"]
            .as_i64()
            .map(|id| id.to_string())
            .unwrap_or_else(|| rl.get_exmap("群ID").to_string())
    } else {
        target.to_owned()
    };
    Ok(json!({"chat_id":chat_id(&chat)?,"receiver_user_id":receiver,"ephemeral_message_id":id}))
}

pub(super) fn suggestion_event(message: &Value, kind: &str) -> Value {
    let service = &message[kind];
    let (status, post_id) = match kind {
        "suggested_post_info" => (
            match service["state"].as_str() {
                Some("pending") => "待审核",
                Some("approved") => "已同意",
                Some("declined") => "已拒绝",
                _ => "",
            },
            &message["message_id"],
        ),
        "suggested_post_approved" => ("已同意", &service["suggested_post_message"]["message_id"]),
        "suggested_post_approval_failed" => {
            ("审核失败", &service["suggested_post_message"]["message_id"])
        }
        "suggested_post_declined" => ("已拒绝", &service["suggested_post_message"]["message_id"]),
        "suggested_post_paid" => ("已支付", &service["suggested_post_message"]["message_id"]),
        "suggested_post_refunded" => ("已退款", &service["suggested_post_message"]["message_id"]),
        _ => return Value::Null,
    };
    let currency = service["price"]["currency"]
        .as_str()
        .or_else(|| service["currency"].as_str());
    let currency = if currency == Some("XTR") {
        Some("星币")
    } else {
        currency
    };
    let refund = match service["reason"].as_str() {
        Some("post_deleted") => Some("帖子已删除"),
        Some("payment_refunded") => Some("付款方退款"),
        value => value,
    };
    json!({"状态":status,"消息ID":post_id,"会话ID":message["chat"]["id"],"币种":currency,
           "价格":service["price"]["amount"],"发布时间":service["send_date"],"留言":service["comment"],
           "退款原因":refund,"已付纳克":service["amount"],"已付星币":service["star_amount"]["amount"],
           "已付十亿分之一星币":service["star_amount"]["nanostar_amount"]})
}
