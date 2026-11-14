use super::*;

fn sticker(data: &Value) -> Value {
    json!({"文件ID":data["file_id"],"表情":data["emoji"],"自定义表情ID":data["custom_emoji_id"],
        "宽度":data["width"],"高度":data["height"],"贴纸包":data["set_name"]})
}

fn sticker_format(value: &str) -> Result<&str> {
    match value {
        "" | "静态" => Ok("static"),
        "动画" => Ok("animated"),
        "视频" => Ok("video"),
        _ => Err("贴纸格式请填静态、动画或视频".into()),
    }
}

fn sticker_input(row: &RedValue) -> Result<Value> {
    let row = array(row)?;
    let id = row_text(row, 0)?;
    let emoji = row.get(1).ok_or("请填写关联表情数组")?;
    let emoji = array(emoji)?;
    if !(1..=20).contains(&emoji.len()) || row.len() > 3 {
        return Err("每张贴纸须填 1 至 20 个关联表情，至多三项参数".into());
    }
    let emoji = emoji
        .iter()
        .map(|value| {
            let emoji = value.expect_text_value()?;
            required(&emoji, "关联表情")?;
            Ok(emoji)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"sticker":required(&id,"贴纸文件ID")?,"format":sticker_format(&row_text(row,2)?)?,
        "emoji_list":emoji}),
    )
}

fn sticker_set(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let op = rl.get_param_text(params, 0)?;
    let name = rl.get_param_text(params, 1)?;
    match op.as_str() {
        "查询" => {
            let result = telegram(
                rl,
                "getStickerSet",
                json!({"name":required(&name,"贴纸包名")?}),
            )?;
            let data = &result["data"];
            let stickers = data["stickers"].as_array().ok_or("平台未返回贴纸列表")?;
            output(&json!({"名称":data["name"],"标题":data["title"],
                "贴纸":stickers.iter().map(sticker).collect::<Vec<_>>()}))
        }
        "创建" => {
            let title = rl.get_param_text(params, 2)?;
            let rows = rl.get_param(params, 3)?;
            let [kind, owner] = text_params::<2>(rl, params, 4)?;
            let kind = match kind.as_str() {
                "" | "普通" => "regular",
                "自定义表情" => "custom_emoji",
                _ => return Err("贴纸包类型请填普通或自定义表情".into()),
            };
            let rows = array(&rows)?;
            if !(1..=50).contains(&rows.len()) {
                return Err("创建时需要 1 至 50 张贴纸".into());
            }
            let stickers = rows
                .iter()
                .map(|row| sticker_input(row))
                .collect::<Result<Vec<_>>>()?;
            let owner = positive_id(&context(rl, &owner, "发送者ID")?)?;
            telegram(
                rl,
                "createNewStickerSet",
                json!({"name":required(&name,"贴纸包名")?,
                "title":required(&title,"贴纸包标题")?,"stickers":stickers,"sticker_type":kind,"user_id":owner}),
            )?;
            empty()
        }
        "添加" | "替换" => {
            let row = rl.get_param(params, 2)?;
            let [owner, old] = text_params::<2>(rl, params, 3)?;
            let owner = positive_id(&context(rl, &owner, "发送者ID")?)?;
            let mut body = json!({"name":required(&name,"贴纸包名")?,"user_id":owner,"sticker":sticker_input(&row)?});
            let method = if op == "替换" {
                body["old_sticker"] = required(&old, "旧贴纸文件ID")?.into();
                "replaceStickerInSet"
            } else {
                "addStickerToSet"
            };
            telegram(rl, method, body)?;
            empty()
        }
        "改标题" => {
            let title = rl.get_param_text(params, 2)?;
            telegram(
                rl,
                "setStickerSetTitle",
                json!({"name":required(&name,"贴纸包名")?,"title":required(&title,"标题")?}),
            )?;
            empty()
        }
        "移除贴纸" | "删除" => {
            let (method, field, label) = if op == "删除" {
                ("deleteStickerSet", "name", "贴纸包名")
            } else {
                ("deleteStickerFromSet", "sticker", "贴纸文件ID")
            };
            telegram(rl, method, json!({field:required(&name,label)?}))?;
            empty()
        }
        _ => Err("贴纸包操作请填查询、创建、添加、替换、改标题、移除贴纸或删除".into()),
    }
}

fn kook_emoji(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let op = rl.get_param_text(params, 0)?;
    match op.as_str() {
        "上传" => {
            let file = rl.get_param(params, 1)?;
            let [name, server] = text_params::<2>(rl, params, 2)?;
            let RedValue::Bin(bytes) = file.as_ref() else {
                return Err("服务器表情需要 PNG 字节集".into());
            };
            if bytes.len() > 256 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err("服务器表情需要 256 KB 以内的 PNG".into());
            }
            let mut body = json!({"guild_id":guild(rl,&server)?});
            insert_text(&mut body, "name", &name);
            let result = upload_api(
                rl,
                "kook",
                "guild-emoji/create",
                body,
                json!({"emoji":crate::native_args::file_part("emoji.png",bytes,Some("image/png"))}),
            )?;
            scalar_text(&result["data"]["id"])
        }
        "列表" => {
            let [server, page] = text_params::<2>(rl, params, 1)?;
            let page = if page.is_empty() {
                1
            } else {
                positive_id(&page)?
            };
            let data = kook(
                rl,
                "GET",
                "guild-emoji/list",
                json!({"guild_id":guild(rl,&server)?,"page":page}),
            )?;
            mapped_list(
                &data["data"]["items"],
                |row| json!({"表情ID":row["id"],"名称":row["name"],"上传者ID":row["user_info"]["id"]}),
            )
        }
        "改名" | "删除" => {
            let [id, name] = text_params::<2>(rl, params, 1)?;
            let mut body = json!({"id":required(&id,"表情ID")?});
            let path = if op == "改名" {
                body["name"] = required(&name, "表情名")?.into();
                "guild-emoji/update"
            } else {
                "guild-emoji/delete"
            };
            kook(rl, "POST", path, body)?;
            empty()
        }
        _ => Err("服务器表情操作请填列表、上传、改名或删除".into()),
    }
}

fn kook_roles(rl: &mut RedLang, params: &[Ast]) -> RedCommandResult {
    let [op, first, second] = text_params::<3>(rl, params, 0)?;
    let server = match op.as_str() {
        "列表" | "创建" | "删除" => second.clone(),
        "授予" | "移除" => rl.get_param_text(params, 3)?,
        "编辑" => rl.get_param_text(params, 7)?,
        _ => return Err("身份组操作请填列表、创建、编辑、授予、移除或删除".into()),
    };
    let mut body = json!({"guild_id":guild(rl,&server)?});
    if op == "列表" {
        body["page"] = (if first.is_empty() {
            1
        } else {
            positive_id(&first)?
        })
        .into();
        let data = kook(rl, "GET", "guild-role/list", body)?;
        return mapped_list(&data["data"]["items"], |row| {
            json!({"身份组ID":row["role_id"],"名称":row["name"],
            "颜色":row["color"],"权限":row["permissions"],"权限名称":kook_right_names(&row["permissions"]),
            "可提及":row["mentionable"] == 1,"单独显示":row["hoist"] == 1})
        });
    }
    if op == "创建" {
        insert_text(&mut body, "name", &first);
        let data = kook(rl, "POST", "guild-role/create", body)?;
        let row = if data["data"].is_array() {
            &data["data"][0]
        } else {
            &data["data"]
        };
        return scalar_text(&row["role_id"]);
    }
    body["role_id"] = bounded(&first, 0, i64::MAX, "身份组ID")?.into();
    let path = match op.as_str() {
        "授予" | "移除" => {
            body["user_id"] = context(rl, &second, "发送者ID")?.into();
            if op == "授予" {
                "guild-role/grant"
            } else {
                "guild-role/revoke"
            }
        }
        "编辑" => {
            let permissions = rl.get_param(params, 3)?;
            let [color, hoist, mention] = text_params::<3>(rl, params, 4)?;
            insert_text(&mut body, "name", &second);
            if !matches!(permissions.as_ref(), RedValue::Text(value) if value.is_empty()) {
                body["permissions"] = kook_mask(&permissions)?.into();
            }
            if !color.is_empty() {
                body["color"] = bounded(&color, 0, 0xFF_FF_FF, "颜色")?.into();
            }
            if !hoist.is_empty() {
                body["hoist"] = u8::from(boolean(&hoist, false)?).into();
            }
            if !mention.is_empty() {
                body["mentionable"] = u8::from(boolean(&mention, false)?).into();
            }
            if body.as_object().is_some_and(|fields| fields.len() == 2) {
                return Err("请填写要修改的身份组资料".into());
            }
            "guild-role/update"
        }
        _ => "guild-role/delete",
    };
    kook(rl, "POST", path, body)?;
    empty()
}

pub(super) fn register() {
    add_fun(vec!["Telegram贴纸包"], sticker_set);
    add_fun(vec!["Telegram上传贴纸"], |rl, params| {
        let file = rl.get_param(params, 0)?;
        let [name, format, owner] = text_params::<3>(rl, params, 1)?;
        let owner = positive_id(&context(rl, &owner, "发送者ID")?)?;
        let data = upload_api(
            rl,
            "telegram",
            "uploadStickerFile",
            json!({"user_id":owner,"sticker_format":sticker_format(&format)?}),
            json!({"sticker":upload(&file,&name)?}),
        )?;
        scalar_text(&data["data"]["file_id"])
    });
    add_fun(vec!["Telegram自定义表情"], |rl, params| {
        let ids = rl.get_param(params, 0)?;
        let ids = array(&ids)?;
        if !(1..=200).contains(&ids.len()) {
            return Err("请提供 1 至 200 个自定义表情ID".into());
        }
        let ids = ids
            .iter()
            .map(|item| {
                let id = item.expect_text_value()?;
                required(&id, "自定义表情ID")?;
                Ok(id)
            })
            .collect::<Result<Vec<_>>>()?;
        let data = telegram(
            rl,
            "getCustomEmojiStickers",
            json!({"custom_emoji_ids":ids}),
        )?;
        mapped_list(&data["data"], sticker)
    });
    add_fun(vec!["KOOK服务器表情"], kook_emoji);
    add_fun(vec!["KOOK身份组"], kook_roles);
    add_fun(vec!["KOOK回应列表"], |rl, params| {
        let [message, emoji, scope] = text_params::<3>(rl, params, 0)?;
        let message = if message.is_empty() {
            let event: Value =
                serde_json::from_str(&rl.get_exmap("原始事件")).map_err(|_| "请填写平台消息ID")?;
            event["kook_message_id"]
                .as_str()
                .or_else(|| event["kook"]["msg_id"].as_str())
                .ok_or("请填写平台消息ID")?
                .to_owned()
        } else {
            message
        };
        let path = match scope.as_str() {
            "" if rl.get_exmap("群ID").is_empty() => "direct-message/reaction-list",
            "" | "频道" => "message/reaction-list",
            "私聊" => "direct-message/reaction-list",
            _ => return Err("回应查询范围请填频道或私聊".into()),
        };
        let data = kook(
            rl,
            "GET",
            path,
            json!({"msg_id":message,"emoji":required(&emoji,"表情")?}),
        )?;
        mapped_list(&data["data"], kook_user)
    });
}
