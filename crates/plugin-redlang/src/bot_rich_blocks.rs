use super::*;
use serde_json::Map;

fn optional_text(value: Option<&Rc<RedValue>>) -> Result<String> {
    Ok(value
        .map(|v| v.expect_text_value())
        .transpose()?
        .unwrap_or_default())
}

fn opt_int(row: &[Rc<RedValue>], index: usize, min: i64, max: i64) -> Result<Option<i64>> {
    let value = row_text(row, index)?;
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        value
            .parse::<i64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("数值需在 {min} 至 {max} 之间"))?,
    ))
}

fn align(value: &str, vertical: bool) -> Result<&'static str> {
    match (vertical, value) {
        (false, "" | "左") => Ok("left"),
        (false, "居中") => Ok("center"),
        (false, "右") => Ok("right"),
        (true, "" | "上") => Ok("top"),
        (true, "居中") => Ok("middle"),
        (true, "下") => Ok("bottom"),
        _ => Err("横向对齐填左、居中或右，纵向对齐填上、居中或下".into()),
    }
}

fn caption(body: &mut Value, row: &[Rc<RedValue>], start: usize) -> Result<()> {
    let value = |index| -> Result<Value> {
        row.get(index)
            .map(|value: &Rc<RedValue>| inline_text(value, 8))
            .transpose()
            .map(|value| value.unwrap_or_else(|| "".into()))
    };
    let text = value(start)?;
    let credit = value(start + 1)?;
    if text != "" {
        body["caption"] = json!({"text":text});
        if credit != "" {
            body["caption"]["credit"] = credit;
        }
    } else if credit != "" {
        return Err("填写出处时也要填写说明".into());
    }
    Ok(())
}

pub(super) fn blocks(
    input: &RedValue,
    files: &mut Map<String, Value>,
    depth: u8,
) -> Result<Vec<Value>> {
    if depth == 0 {
        return Err("富文本内容块嵌套过深".into());
    }
    let items = array(input)?;
    if items.is_empty() {
        return Err("内容块数组至少有一项".into());
    }
    items.iter().map(|item| block(item, files, depth)).collect()
}

fn nested(
    row: &[Rc<RedValue>],
    index: usize,
    files: &mut Map<String, Value>,
    depth: u8,
) -> Result<Vec<Value>> {
    blocks(
        row.get(index).ok_or("请填写子内容块数组")?,
        files,
        depth - 1,
    )
}

fn block(value: &RedValue, files: &mut Map<String, Value>, depth: u8) -> Result<Value> {
    let row = array(value)?;
    let kind = row_text(row, 0)?;
    let mut body = match kind.as_str() {
        "段落" | "标题" | "代码" | "页脚" | "分隔线" | "公式" | "锚点" | "引文" | "草稿提示" =>
        {
            return super::basic_block(value);
        }
        "列表" => {
            let rows = array(row.get(1).ok_or("请填写列表项目")?)?;
            if rows.is_empty() {
                return Err("列表至少有一项".into());
            }
            let items = rows
                .iter()
                .map(|item| {
                    let item = array(item)?;
                    let mut body = json!({"blocks":nested(item,0,files,depth)?});
                    let mark = row_text(item, 1)?;
                    match mark.as_str() {
                        "" | "无" => (),
                        "待勾选" | "已勾选" => {
                            body["has_checkbox"] = true.into();
                            if mark == "已勾选" {
                                body["is_checked"] = true.into();
                            }
                        }
                        _ => return Err("勾选状态请填无、待勾选或已勾选".into()),
                    }
                    let style = row_text(item, 2)?;
                    match style.as_str() {
                        "" => (),
                        "1" | "a" | "A" | "i" | "I" => body["type"] = style.into(),
                        _ => return Err("编号样式请填 1、a、A、i 或 I".into()),
                    }
                    if let Some(n) = opt_int(item, 3, i64::MIN, i64::MAX)? {
                        if body["type"].is_null() {
                            return Err("自定义编号需填写编号样式".into());
                        }
                        body["value"] = n.into();
                    }
                    Ok(body)
                })
                .collect::<Result<Vec<_>>>()?;
            json!({"type":"list","items":items})
        }
        "引用块" => json!({"type":"blockquote","blocks":nested(row,1,files,depth)?}),
        "折叠引用" => {
            json!({"type":"expandable_blockquote","text":inline_text(row.get(1).ok_or("请填写引用内容")?,8)?})
        }
        "拼贴" | "幻灯片" => {
            let parts = nested(row, 1, files, depth)?;
            if parts
                .iter()
                .any(|part| !matches!(part["type"].as_str(), Some("photo" | "video" | "animation")))
            {
                return Err("拼贴和幻灯片请填图片、视频或动画内容块".into());
            }
            let mut body =
                json!({"type":if kind == "拼贴" { "collage" } else { "slideshow" },"blocks":parts});
            caption(&mut body, row, 2)?;
            body
        }
        "表格" => {
            let rows = array(row.get(1).ok_or("请填写表格行数组")?)?;
            if rows.is_empty() {
                return Err("表格至少有一行".into());
            }
            let cells = rows.iter().map(|line| {
                let line = array(line)?;
                if line.is_empty() { return Err("表格每行至少有一格".into()); }
                line.iter().map(|cell| {
                    if cell.as_array().is_none() {
                        let text = cell.expect_text_value()?;
                        return Ok(if text.is_empty() { json!({"align":"left","valign":"top"}) }
                            else { json!({"text":text,"align":"left","valign":"top"}) });
                    }
                    let cell = array(cell)?;
                    let mut body = json!({"align":align(&row_text(cell,4)?,false)?,"valign":align(&row_text(cell,5)?,true)?});
                    if let Some(value) = cell.first() {
                        let text = inline_text(value,8)?;
                        if text != "" { body["text"] = text; }
                    }
                    if boolean(&row_text(cell,1)?,false)? { body["is_header"] = true.into(); }
                    for (index,field) in [(2,"colspan"),(3,"rowspan")] {
                        if let Some(n) = opt_int(cell,index,1,i32::MAX.into())? { body[field] = n.into(); }
                    }
                    Ok(body)
                }).collect::<Result<Vec<_>>>()
            }).collect::<Result<Vec<_>>>()?;
            let mut body = json!({"type":"table","cells":cells});
            for (index, field) in [(2, "is_bordered"), (3, "is_striped"), (4, "is_compact")] {
                if boolean(&row_text(row, index)?, false)? {
                    body[field] = true.into();
                }
            }
            if let Some(title) = row.get(5) {
                let title = inline_text(title, 8)?;
                if title != "" {
                    body["caption"] = title;
                }
            }
            body
        }
        "折叠详情" => {
            let mut body = json!({"type":"details","summary":inline_text(row.get(1).ok_or("请填写折叠摘要")?,8)?,"blocks":nested(row,2,files,depth)?});
            if boolean(&row_text(row, 3)?, false)? {
                body["is_open"] = true.into();
            }
            body
        }
        "地图" => {
            let longitude = row_text(row, 1)?.parse::<f64>()?;
            let latitude = row_text(row, 2)?.parse::<f64>()?;
            if !(-180.0..=180.0).contains(&longitude) || !(-90.0..=90.0).contains(&latitude) {
                return Err("地图经纬度超出范围".into());
            }
            let mut body =
                json!({"type":"map","location":{"longitude":longitude,"latitude":latitude}});
            let w = opt_int(row, 4, 0, 10000)?;
            let h = opt_int(row, 5, 0, 10000)?;
            if let (Some(w), Some(h)) = (w, h) {
                if w + h > 10000 || (w > 0 && h > 0 && w.max(h) > 20 * w.min(h)) {
                    return Err("地图宽高之和不能超过 10000，宽高比例不能超过 20".into());
                }
            }
            for (key, value) in [
                ("zoom", opt_int(row, 3, 0, 24)?),
                ("width", w),
                ("height", h),
            ] {
                if let Some(value) = value {
                    body[key] = value.into();
                }
            }
            caption(&mut body, row, 6)?;
            body
        }
        "按钮" => {
            let rows = array(row.get(1).ok_or("请填写按钮数组")?)?;
            if !(1..=8).contains(&rows.len()) {
                return Err("富文本按钮每行 1 至 8 个".into());
            }
            json!({"type":"buttons","align":align(&row_text(row,2)?,false)?,
                   "buttons":rows.iter().map(|button| rich_button(button)).collect::<Result<Vec<_>>>()?})
        }
        kind => {
            let label = kind;
            let kind = super::media_kind(label)?;
            let source = row.get(1).ok_or("请填媒体来源")?;
            let name = row_text(row, 2)?;
            if source.as_text().is_some_and(|text| text.contains("://")) {
                super::source(source, &name, &mut Map::new())?;
            }
            let mut media = if let Some(options) = row
                .get(5)
                .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
            {
                crate::native_args::rich_block_media(source, &name, label, options, files)?
            } else {
                json!({"type":kind,"media":super::source(source,&name,files)?})
            };
            if let Some(above) = row
                .get(6)
                .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()))
            {
                if !matches!(kind, "photo" | "animation" | "video") {
                    return Err("说明位置只适用于图片、动画或视频".into());
                }
                media["show_caption_above_media"] =
                    boolean(&above.expect_text_value()?, false)?.into();
            }
            let mut body = json!({"type":kind,kind:media});
            caption(&mut body, row, 3)?;
            body
        }
    };
    if matches!(kind.as_str(), "引用块" | "折叠引用") {
        if let Some(credit) = row.get(2) {
            let credit = inline_text(credit, 8)?;
            if credit != "" {
                body["credit"] = credit;
            }
        }
    }
    Ok(body)
}

fn rich_button(value: &RedValue) -> Result<Value> {
    let row = array(value)?;
    let label = inline_text(row.first().ok_or("请填写按钮名称")?, 8)?;
    let kind = row_text(row, 1)?;
    let target = row_text(row, 2)?;
    if label == "" || row.len() > 5 {
        return Err("按钮请填名称、类型、值、样式及可选范围".into());
    }
    let mut body = json!({"text":label});
    match kind.as_str() {
        "" | "回调" if (1..=64).contains(&target.len()) => body["callback_data"] = target.into(),
        "链接" | "小程序" | "登录" => {
            let url = url::Url::parse(&target)?;
            if !matches!(
                (kind.as_str(), url.scheme()),
                ("链接", "http" | "https" | "tg") | ("小程序" | "登录", "https")
            ) {
                return Err("按钮链接协议不受支持".into());
            }
            match kind.as_str() {
                "登录" => {
                    let detail = row
                        .get(4)
                        .filter(|v| !matches!(v.as_ref(),RedValue::Text(t) if t.is_empty()));
                    let mut login = json!({"url":target});
                    if let Some(detail) = detail {
                        let fields = array(detail)?;
                        if fields.len() > 2 {
                            return Err("登录选项用【数组@转发文字@请求发消息权限】".into());
                        }
                        let text = row_text(fields, 0)?;
                        let write = row_text(fields, 1)?;
                        if !text.is_empty() {
                            login["forward_text"] = text.into();
                        }
                        if !write.is_empty() {
                            login["request_write_access"] = boolean(&write, false)?.into();
                        }
                    }
                    body["login_url"] = login;
                }
                "小程序" => body["web_app"] = json!({"url":target}),
                _ => body["url"] = target.into(),
            }
        }
        "复制" if !target.is_empty() && target.chars().count() <= 256 => {
            body["copy_text"] = json!({"text":target})
        }
        "禁用" if target.is_empty() => body["disabled"] = json!({}),
        "搜索" => body["switch_inline_query"] = target.into(),
        "当前搜索" => body["switch_inline_query_current_chat"] = target.into(),
        "指定搜索" => {
            let mut opts = json!({"query":target});
            if let Some(scope) = row.get(4).filter(|v| v.as_array().is_some()) {
                for value in array(scope)? {
                    let field = match value.expect_text_value()?.as_str() {
                        "用户" => "allow_user_chats",
                        "机器人" => "allow_bot_chats",
                        "群聊" => "allow_group_chats",
                        "频道" => "allow_channel_chats",
                        _ => return Err("搜索范围可填用户、机器人、群聊或频道".into()),
                    };
                    opts[field] = true.into();
                }
            }
            body["switch_inline_query_chosen_chat"] = opts;
        }
        _ => return Err("按钮类型或值无效".into()),
    }
    match optional_text(row.get(3))?.as_str() {
        "" => (),
        "强调" => body["style"] = "primary".into(),
        "成功" => body["style"] = "success".into(),
        "危险" => body["style"] = "danger".into(),
        "文字" if body["callback_data"].is_string() => body["style"] = "link".into(),
        _ => return Err("按钮样式请填强调、成功、危险或回调使用的文字".into()),
    }
    Ok(body)
}

pub(super) fn inline_text(value: &RedValue, depth: u8) -> Result<Value> {
    if depth == 0 {
        return Err("富文本样式嵌套过深".into());
    }
    let Some(items) = value.as_array() else {
        return Ok(value.expect_text_value()?.into());
    };
    Ok(items
        .iter()
        .map(|item| {
            let Some(row) = item.as_array() else {
                return Ok(item.expect_text_value()?.into());
            };
            let style = row_text(row, 0)?;
            let text = row.get(1).ok_or("请填写富文本样式文字")?;
            if row.len() > 4 {
                return Err("富文本样式参数过多".into());
            }
            let kind = match style.as_str() {
                "粗体" => "bold",
                "斜体" => "italic",
                "下划线" => "underline",
                "删除线" => "strikethrough",
                "剧透" => "spoiler",
                "下标" => "subscript",
                "上标" => "superscript",
                "高亮" => "marked",
                "代码" => "code",
                "链接" => "url",
                "日期时间" => "date_time",
                "自定义表情" => "custom_emoji",
                "公式" => "mathematical_expression",
                "邮箱" => "email_address",
                "电话" => "phone_number",
                "银行卡" => "bank_card_number",
                "TON地址" => "ton_address",
                "艾特" => "mention",
                "标签" => "hashtag",
                "货币标签" => "cashtag",
                "命令" => "bot_command",
                "锚点链接" => "anchor_link",
                "参考" => "reference",
                "参考链接" => "reference_link",
                _ => return Err("未知的富文本行内样式".into()),
            };
            let extra = row_text(row, 2)?;
            let field = match kind {
                "url" => "url",
                "email_address" => "email_address",
                "phone_number" => "phone_number",
                "bank_card_number" => "bank_card_number",
                "ton_address" => "address",
                "mention" => "username",
                "hashtag" => "hashtag",
                "cashtag" => "cashtag",
                "bot_command" => "bot_command",
                "anchor_link" => "anchor_name",
                "reference" => "name",
                "reference_link" => "reference_name",
                _ => "",
            };
            let mut body = json!({"type":kind});
            match kind {
                "mathematical_expression" => body["expression"] = text.expect_text_value()?.into(),
                "custom_emoji" => {
                    if extra.is_empty() {
                        return Err("请填写自定义表情ID".into());
                    }
                    body["alternative_text"] = text.expect_text_value()?.into();
                    body["custom_emoji_id"] = extra.as_str().into();
                }
                _ => body["text"] = inline_text(text, depth - 1)?,
            }
            if kind == "date_time" {
                body["unix_time"] = extra.parse::<i64>()?.into();
                let format = row_text(row, 3)?;
                if !format.is_empty() {
                    body["date_time_format"] = format.into();
                }
            } else if !field.is_empty() {
                if extra.is_empty() && kind != "anchor_link" {
                    return Err("此样式需要附加值".into());
                }
                body[field] = extra.into();
            }
            Ok(body)
        })
        .collect::<Result<Vec<Value>>>()?
        .into())
}
