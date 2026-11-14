use redlang::mytool::cq_params_encode;

use super::*;

fn decimal(value: &str, min: f64, max: f64, name: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|number| (min..=max).contains(number))
        .ok_or_else(|| format!("{name}请填 {min} 至 {max} 的数字").into())
}

fn coordinate(lng: &str, lat: &str) -> Result<Value> {
    Ok(
        json!({"longitude":decimal(lng,-180.0,180.0,"经度")?,"latitude":decimal(lat,-90.0,90.0,"纬度")?}),
    )
}

fn segment(rl: &RedLang, kind: &str, body: Value) -> RedCommandResult {
    if get_platform(rl) != "telegram" {
        return Err("此消息类型需要 Telegram 机器人来源".into());
    }
    let content = cq_params_encode(&body.to_string());
    Ok(Some(rv_text(format!("[CQ:{kind},content={content}]"))))
}

pub(crate) fn event_location(data: &Value) -> Value {
    if !data.is_object() {
        return Value::Null;
    }
    json!({"经度":data["longitude"],"纬度":data["latitude"],"精度米数":data["horizontal_accuracy"],
        "有效秒数":data["live_period"],"方向角":data["heading"],"接近提醒米数":data["proximity_alert_radius"]})
}

pub(crate) fn event_venue(data: &Value) -> Value {
    if !data.is_object() {
        return Value::Null;
    }
    json!({"位置":event_location(&data["location"]),"场所名":data["title"],"地址":data["address"],
        "Foursquare场所ID":data["foursquare_id"],"Foursquare场所类型":data["foursquare_type"],
        "Google场所ID":data["google_place_id"],"Google场所类型":data["google_place_type"]})
}

pub(crate) fn event_contact(data: &Value) -> Value {
    if !data.is_object() {
        return Value::Null;
    }
    json!({"电话":data["phone_number"],"名字":data["first_name"],"姓氏":data["last_name"],"用户ID":data["user_id"],"名片文本":data["vcard"]})
}

fn live_fields(
    [lng, lat, accuracy, seconds, heading, radius]: [String; 6],
    edit: bool,
) -> Result<Value> {
    let mut body = coordinate(&lng, &lat)?;
    if !accuracy.is_empty() {
        body["horizontal_accuracy"] = decimal(&accuracy, 0.0, 1500.0, "精度米数")?.into();
    }
    if !seconds.is_empty() {
        let duration = if seconds == "永久" {
            i64::from(i32::MAX)
        } else {
            bounded(
                &seconds,
                60,
                if edit { i64::from(i32::MAX) - 1 } else { 86400 },
                "有效秒数",
            )?
        };
        body["live_period"] = duration.into();
    }
    if !heading.is_empty() {
        body["heading"] = bounded(&heading, 1, 360, "方向角")?.into();
    }
    if !radius.is_empty() {
        body["proximity_alert_radius"] = bounded(&radius, 1, 100000, "接近提醒米数")?.into();
    }
    if !edit && seconds.is_empty() && (!radius.is_empty() || !heading.is_empty()) {
        return Err("方向角和接近提醒需要实时位置的有效秒数".into());
    }
    Ok(body)
}

pub(crate) fn live_edit_fields(values: &[Rc<RedValue>]) -> Result<Value> {
    if !(2..=6).contains(&values.len()) {
        return Err("新位置需要经纬度和至多四项选填参数".into());
    }
    live_fields(
        [
            row_text(values, 0)?,
            row_text(values, 1)?,
            row_text(values, 2)?,
            row_text(values, 3)?,
            row_text(values, 4)?,
            row_text(values, 5)?,
        ],
        true,
    )
}

pub(super) fn register() {
    add_fun(vec!["Telegram位置"], |rl, params| {
        let data = live_fields(text_params::<6>(rl, params, 0)?, false)?;
        segment(rl, "telegram_location", data)
    });
    add_fun(vec!["Telegram场所"], |rl, params| {
        let [lng, lat, title, address, fs_id, fs_type, g_id, g_type] =
            text_params::<8>(rl, params, 0)?;
        let mut body = coordinate(&lng, &lat)?;
        body["title"] = required(&title, "场所名")?.into();
        body["address"] = required(&address, "地址")?.into();
        for (key, value) in [
            ("foursquare_id", fs_id),
            ("foursquare_type", fs_type),
            ("google_place_id", g_id),
            ("google_place_type", g_type),
        ] {
            insert_text(&mut body, key, &value);
        }
        segment(rl, "telegram_venue", body)
    });
    add_fun(vec!["Telegram联系人"], |rl, params| {
        let [phone, first, last, card] = text_params::<4>(rl, params, 0)?;
        if card.len() > 2048 {
            return Err("vCard 名片最多 2048 字节".into());
        }
        let mut body = json!({"phone_number":required(&phone,"电话号码")?,"first_name":required(&first,"名字")?});
        insert_text(&mut body, "last_name", &last);
        insert_text(&mut body, "vcard", &card);
        segment(rl, "telegram_contact", body)
    });
}
