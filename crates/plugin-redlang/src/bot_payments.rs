use redlang::mytool::cq_params_encode;

use super::*;

fn message_segment(kind: &str, data: Value) -> RedCommandResult {
    let content = cq_params_encode(&data.to_string());
    Ok(Some(rv_text(format!("[CQ:{kind},content={content}]"))))
}

fn prices(value: &RedValue) -> Result<(Vec<Value>, i64)> {
    let mut total = 0_i64;
    let rows = array(value)?;
    if rows.is_empty() {
        return Err("请至少填写一项价格明细".into());
    }
    let items = rows
        .iter()
        .map(|item| {
            let row = array(item)?;
            if row.len() != 2 {
                return Err("价格明细每项用【数组@名称@金额】".into());
            }
            let label = row_text(row, 0)?;
            required(&label, "价格项名称")?;
            let amount = row_text(row, 1)?
                .parse::<i64>()
                .map_err(|_| "价格项金额请填整数")?;
            total = total.checked_add(amount).ok_or("价格合计超出整数范围")?;
            Ok(json!({"label":label,"amount":amount}))
        })
        .collect::<Result<_>>()?;
    Ok((items, total))
}

fn invoice(rl: &mut RedLang, params: &[Ast], link: bool) -> Result<Value> {
    let [title, description, order] = text_params::<3>(rl, params, 0)?;
    if !(1..=32).contains(&title.chars().count())
        || !(1..=255).contains(&description.chars().count())
    {
        return Err("商品名需为 1 至 32 字，说明需为 1 至 255 字".into());
    }
    if !(1..=128).contains(&order.len()) {
        return Err("订单识别值需要 1 至 128 字节".into());
    }
    let amount = rl.get_param(params, 3)?;
    let [currency, provider] = text_params::<2>(rl, params, 4)?;
    let currency = match currency.as_str() {
        "" | "星币" => "XTR",
        value => value,
    };
    if currency.len() != 3 || !currency.bytes().all(|letter| letter.is_ascii_uppercase()) {
        return Err("币种填星币或三字母大写货币代码".into());
    }
    let star = currency == "XTR";
    if star != provider.is_empty() {
        return Err("星币省略支付服务商令牌，其他币种须填写令牌".into());
    }
    let (rows, total) = if let RedValue::Text(amount) = amount.as_ref() {
        let amount = positive_id(amount)?;
        (vec![json!({"label":title,"amount":amount})], amount)
    } else {
        prices(&amount)?
    };
    if total <= 0 || (star && rows.len() != 1) {
        return Err("发票合计必须大于零，星币发票只填一项价格".into());
    }
    let mut body = json!({"title":title,"description":description,"payload":order,
        "currency":currency,"provider_token":provider,"prices":rows});
    let options = rl.get_param(params, 6)?;
    if let RedValue::Text(text) = options.as_ref() {
        if text.is_empty() {
            return Ok(body);
        }
    }
    let options = array(&options)?;
    if options.len() > if link { 7 } else { 6 } {
        return Err("发票附加选项数量超出范围".into());
    }
    if let Some(flags) = options
        .first()
        .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
    {
        if star {
            return Err("星币发票不收集物流及联系资料".into());
        }
        let settings = rights(
            flags,
            &[
                ("姓名", "need_name"),
                ("电话", "need_phone_number"),
                ("邮箱", "need_email"),
                ("地址", "need_shipping_address"),
                ("转交电话", "send_phone_number_to_provider"),
                ("转交邮箱", "send_email_to_provider"),
                ("运费询价", "is_flexible"),
            ],
        )?;
        for (key, value) in settings.as_object().ok_or("订单资料格式错误")? {
            if value == true {
                body[key] = true.into();
            }
        }
        for (flag, required) in [
            ("is_flexible", "need_shipping_address"),
            ("send_phone_number_to_provider", "need_phone_number"),
            ("send_email_to_provider", "need_email"),
        ] {
            if body[flag] == true {
                body[required] = true.into();
            }
        }
    }
    let image = row_text(options, 1)?;
    if !image.is_empty() {
        let uri = url::Url::parse(&image)?;
        if !matches!(uri.scheme(), "https" | "http") || uri.host().is_none() {
            return Err("商品图片请填写 HTTP(S) 链接".into());
        }
        body["photo_url"] = image.into();
    }
    if let Some(tips) = options
        .get(2)
        .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
    {
        if star {
            return Err("星币发票没有小费选项".into());
        }
        let values = array(tips)?;
        if !(1..=5).contains(&values.len()) {
            return Err("小费请填上限和至多四档建议金额".into());
        }
        let max = positive_id(&row_text(values, 0)?)?;
        let suggestions = values[1..]
            .iter()
            .map(|value| positive_id(&value.expect_text_value()?))
            .collect::<Result<Vec<_>>>()?;
        if suggestions.windows(2).any(|pair| pair[0] >= pair[1])
            || suggestions.last().is_some_and(|v| *v > max)
        {
            return Err("小费建议须按金额递增且不超过上限".into());
        }
        body["max_tip_amount"] = max.into();
        if !suggestions.is_empty() {
            body["suggested_tip_amounts"] = json!(suggestions);
        }
    }
    if let Some(data) = options
        .get(3)
        .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
    {
        if star {
            return Err("星币发票无支付服务商扩展数据".into());
        }
        body["provider_data"] = crate::native_args::object(data)?.to_string().into();
    }
    if let Some(dimensions) = options
        .get(4)
        .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
    {
        if body["photo_url"].is_null() {
            return Err("填写图片尺寸时也要填写图片链接".into());
        }
        let dimensions = array(dimensions)?;
        if dimensions.len() > 3 {
            return Err("图片按宽度、高度、字节数三项填写".into());
        }
        for (index, key) in ["photo_width", "photo_height", "photo_size"]
            .iter()
            .enumerate()
        {
            let value = row_text(dimensions, index)?;
            if !value.is_empty() {
                body[*key] = positive_id(&value)?.into();
            }
        }
    }
    if link {
        let recurring = boolean(&row_text(options, 5)?, false)?;
        let connection = row_text(options, 6)?;
        if (recurring || !connection.is_empty()) && !star {
            return Err("仅星币发票链接支持订阅与商务连接".into());
        }
        if recurring {
            if total > 10000 {
                return Err("每期星币订阅金额不得超过 10000".into());
            }
            body["subscription_period"] = 2_592_000.into();
        }
        insert_text(&mut body, "business_connection_id", &connection);
    } else {
        insert_text(&mut body, "start_parameter", &row_text(options, 5)?);
    }
    Ok(body)
}

fn address(data: &Value) -> Value {
    if !data.is_object() {
        return Value::Null;
    }
    json!({"国家代码":data["country_code"],"省州":data["state"],"城市":data["city"],
        "地址一":data["street_line1"],"地址二":data["street_line2"],"邮政编码":data["post_code"]})
}

pub(crate) fn event_payment(event: &Value) -> Value {
    let msg = &event["telegram"];
    let (kind, data) = match event["sub_type"].as_str() {
        Some("shipping_query") => ("运费询价", msg),
        Some("pre_checkout_query") => ("结账审核", msg),
        Some("successful_payment") => ("支付成功", &msg["successful_payment"]),
        Some("refunded_payment") => ("退款完成", &msg["refunded_payment"]),
        Some("purchased_paid_media") => ("购买付费媒体", msg),
        _ => return Value::Null,
    };
    let user = msg.get("from").unwrap_or(&Value::Null);
    let order = &data["order_info"];
    let delivery = data
        .get("shipping_address")
        .unwrap_or(&order["shipping_address"]);
    let identity = data
        .get("invoice_payload")
        .unwrap_or(&data["paid_media_payload"]);
    json!({"类型":kind,"查询ID":data["id"],"用户ID":user["id"],"订单识别值":identity,
        "币种":data["currency"],"合计":data["total_amount"],"配送方式ID":data["shipping_option_id"],
        "地址":address(delivery),"姓名":order["name"],"电话":order["phone_number"],"邮箱":order["email"],
        "支付ID":data["telegram_payment_charge_id"],"服务商支付ID":data["provider_payment_charge_id"],
        "订阅到期时间":data["subscription_expiration_date"],"续订付款":data["is_recurring"] == true,
        "首期付款":data["is_first_recurring"] == true})
}

pub(crate) fn answer(
    rl: &mut RedLang,
    params: &[Ast],
    event: &Value,
    approve: bool,
) -> RedCommandResult {
    if business::event_context(rl).is_none() {
        return Err("请用收到支付查询的原机器人应答".into());
    }
    let native = &event["telegram"];
    let query = native["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("支付事件缺少查询ID")?;
    let shipping = event["sub_type"] == "shipping_query";
    let (method, key) = if shipping {
        ("answerShippingQuery", "shipping_query_id")
    } else {
        ("answerPreCheckoutQuery", "pre_checkout_query_id")
    };
    let mut body = json!({key:query,"ok":approve});
    if !approve {
        let reason = rl.get_param_text(params, 0)?;
        body["error_message"] = required(&reason, "显示给用户的拒绝原因")?.into();
    } else if shipping {
        let input = rl.get_param(params, 0)?;
        let rows = array(&input)?;
        if rows.is_empty() {
            return Err("同意配送需要至少一种配送方式".into());
        }
        let mut ids = HashSet::new();
        let options = rows
            .iter()
            .map(|row| {
                let row = array(row)?;
                if row.len() != 3 {
                    return Err("配送方式用【数组@选项ID@名称@金额或明细】".into());
                }
                let id = row_text(row, 0)?;
                if id.is_empty() || !ids.insert(id.clone()) {
                    return Err("配送方式ID必须非空且唯一".into());
                }
                let title = row_text(row, 1)?;
                required(&title, "配送方式名称")?;
                let (prices, total) = if let RedValue::Text(text) = row[2].as_ref() {
                    let amount = bounded(text, 0, i64::MAX, "配送费")?;
                    (vec![json!({"label":title,"amount":amount})], amount)
                } else {
                    prices(&row[2])?
                };
                if total < 0 {
                    return Err("配送费合计不能为负".into());
                }
                Ok(json!({"id":id,"title":title,"prices":prices}))
            })
            .collect::<Result<Vec<_>>>()?;
        body["shipping_options"] = json!(options);
    }
    telegram(rl, method, body)?;
    empty()
}

fn star_transaction(data: &Value) -> Value {
    let (direction, partner) = if data["source"].is_object() {
        ("收入", &data["source"])
    } else {
        ("支出", &data["receiver"])
    };
    let kind = match partner["type"].as_str() {
        Some("user") => "用户",
        Some("chat") => "聊天",
        Some("affiliate_program") => "分成计划",
        Some("fragment") => "提现平台",
        Some("telegram_ads") => "广告",
        Some("telegram_api") => "付费广播",
        _ => "其他",
    };
    json!({"交易ID":data["id"],"星币":data["amount"],"十亿分之一星币":data["nanostar_amount"].as_i64().unwrap_or(0),
        "时间":data["date"],"方向":direction,"对方类型":kind,"用户ID":partner["user"]["id"],"聊天ID":partner["chat"]["id"],
        "订单识别值":partner["invoice_payload"],"订阅周期秒数":partner["subscription_period"]})
}

fn stars(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let [op, first, second, third] = text_params::<4>(rl, params, 0)?;
    match op.as_str() {
        "余额" => {
            let data = telegram(rl, "getMyStarBalance", json!({}))?;
            output(
                &json!({"星币":data["data"]["amount"],"十亿分之一星币":data["data"]["nanostar_amount"].as_i64().unwrap_or(0)}),
            )
        }
        "流水" => {
            let mut body = json!({});
            if !first.is_empty() {
                body["offset"] = bounded(&first, 0, i64::MAX, "跳过笔数")?.into();
            }
            if !second.is_empty() {
                body["limit"] = bounded(&second, 1, 100, "返回笔数")?.into();
            }
            let data = telegram(rl, "getStarTransactions", body)?;
            mapped_list(&data["data"]["transactions"], star_transaction)
        }
        "退款" | "停续订" | "恢复续订" => {
            let event = business::event_context(rl).filter(|event| {
                event["sub_type"] == "successful_payment"
                    && event["telegram"]["successful_payment"]["currency"] == "XTR"
            });
            let details = event.as_ref().map(event_payment).unwrap_or_default();
            let (user, charge) = match (first.is_empty(), second.is_empty()) {
                (true, true) => (
                    details["用户ID"]
                        .as_i64()
                        .ok_or("请填写付款用户ID和支付ID")?,
                    details["支付ID"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .ok_or("支付事件缺少支付ID")?
                        .to_owned(),
                ),
                (false, false) => (positive_id(&first)?, second),
                _ => return Err("付款用户ID与支付ID须同时填写或同时省略".into()),
            };
            if !third.is_empty() {
                return Err("该支付操作只有付款用户ID与支付ID两个参数".into());
            }
            let mut body = json!({"user_id":user,"telegram_payment_charge_id":charge});
            let method = if op == "退款" {
                "refundStarPayment"
            } else {
                body["is_canceled"] = (op == "停续订").into();
                "editUserStarSubscription"
            };
            telegram(rl, method, body)?;
            empty()
        }
        _ => Err("星币操作请填余额、流水、退款、停续订或恢复续订".into()),
    }
}

pub(super) fn register() {
    add_fun(vec!["Telegram付费媒体"], |rl, params| {
        if get_platform(rl) != "telegram" {
            return Err("付费媒体需要 Telegram 机器人来源".into());
        }
        let [amount] = text_params::<1>(rl, params, 0)?;
        let amount = bounded(&amount, 1, 25000, "星币价格")?;
        let items = rl.get_param(params, 1)?;
        let items = array(&items)?;
        if !(1..=10).contains(&items.len()) {
            return Err("付费媒体需要 1 至 10 项".into());
        }
        let [order, caption, above] = text_params::<3>(rl, params, 2)?;
        if order.len() > 128 || caption.chars().count() > 1024 {
            return Err("订单识别值最多 128 字节，说明最多 1024 字".into());
        }
        let mut media = Vec::with_capacity(items.len());
        let mut files = serde_json::Map::new();
        for (index, item) in items.iter().enumerate() {
            let (mut row, uploads) = crate::native_args::media_item(item, index, false)?;
            if !matches!(row["type"].as_str(), Some("photo" | "video" | "live_photo")) {
                return Err("付费媒体类型请填图片、视频或实况照片".into());
            }
            if row.get("caption").is_some() {
                return Err("付费媒体的说明统一填写在命令第 4 项".into());
            }
            if row["type"] == "live_photo"
                && ["media", "photo"].iter().any(|key| {
                    row[*key].as_str().is_some_and(|source| {
                        source.starts_with("https://") || source.starts_with("http://")
                    })
                })
            {
                return Err("付费实况照片请使用 Telegram 文件ID或字节集".into());
            }
            // Keep only fields allowed by InputPaidMedia.
            row.as_object_mut()
                .ok_or("媒体格式错误")?
                .retain(|key, _| matches!(key.as_str(), "type" | "media" | "photo"));
            media.push(row);
            files.extend(uploads);
        }
        let mut body = json!({"star_count":amount,"media":media,"show_caption_above_media":boolean(&above,false)?});
        insert_text(&mut body, "payload", &order);
        insert_text(&mut body, "caption", &caption);
        let styles = rl.get_param(params, 5)?;
        super::super::send_options::apply_styles(
            &mut body,
            "caption",
            "caption_entities",
            &styles,
        )?;
        let data = json!({"body":body,"files":files});
        message_segment("telegram_paid_media", data)
    });
    add_fun(vec!["Telegram发票"], |rl, params| {
        if get_platform(rl) != "telegram" {
            return Err("付款发票需要 Telegram 机器人来源".into());
        }
        let data = invoice(rl, params, false)?;
        message_segment("telegram_invoice", data)
    });
    add_fun(vec!["Telegram发票链接"], |rl, params| {
        let body = invoice(rl, params, true)?;
        let data = telegram(rl, "createInvoiceLink", body)?;
        scalar_text(&data["data"])
    });
    add_fun(vec!["Telegram星币"], stars);
}
