use super::*;

pub(crate) fn parse(rl: &mut RedLang, params: &[Ast], index: usize) -> Result<Value> {
    let value = rl.get_param(params, index)?;
    if matches!(value.as_ref(), RedValue::Text(text) if text.is_empty()) {
        return Ok(json!({}));
    }
    let values = array(&value)?;
    if values.len() > 5 {
        return Err("发送选项最多五项".into());
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
    Ok(result)
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
