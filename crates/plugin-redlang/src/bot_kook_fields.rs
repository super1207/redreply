use super::*;

pub(crate) fn media(data: &Value, only: Option<&str>) -> Value {
    let extra = &data["extra"];
    let kind = data["type"].as_i64().or_else(|| extra["type"].as_i64());
    let kind = match kind {
        Some(3) => "视频",
        Some(8) => "音频",
        _ => match data["attachments"]["type"]
            .as_str()
            .or_else(|| extra["attachments"]["type"].as_str())
        {
            Some("video") => "视频",
            Some("audio") => "音频",
            _ => return Value::Null,
        },
    };
    if only.is_some_and(|value| value != kind) {
        return Value::Null;
    }
    let attachment = [&data["attachments"], &extra["attachments"]]
        .into_iter()
        .find_map(|value| {
            if value.is_object() {
                return Some(value);
            }
            value.as_array()?.iter().find(|item| item.is_object())
        })
        .unwrap_or(&Value::Null);
    let url = attachment["url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .or_else(|| {
            data["content"]
                .as_str()
                .filter(|link| link.starts_with("https://") || link.starts_with("http://"))
        });
    json!({"类型":kind,"链接":url,"文件名":attachment["name"],"媒体类型":attachment["file_type"],
           "字节数":attachment["size"],"时长":attachment["duration"],"宽度":attachment["width"],
           "高度":attachment["height"]})
}

pub(crate) fn media_list(data: &Value) -> Value {
    if data["type"] != 10 {
        let media = media(data, None);
        return if media.is_null() {
            json!([])
        } else {
            json!([media])
        };
    }
    let parsed = data["content"]
        .as_str()
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let cards = parsed.as_ref().unwrap_or(&data["content"]);
    let rows: Vec<_> = cards.as_array().into_iter().flatten().flat_map(|card| {
        card["modules"].as_array().into_iter().flatten()
    }).filter_map(|module| {
        let kind = match module["type"].as_str() {
            Some("video") => "视频",
            Some("audio") => "音频",
            _ => return None,
        };
        Some(json!({"类型":kind,"链接":module["src"],"标题":module["title"],"封面":module["cover"]}))
    }).collect();
    rows.into()
}

pub(crate) fn previews(data: &Value) -> Value {
    let rows: Vec<_> = data
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row.is_object())
        .map(|row| {
            let kind = match row["type"].as_str() {
                Some("bili-video") => "哔哩哔哩视频".into(),
                Some("link") => "链接".into(),
                _ => row["type"].clone(),
            };
            json!({"类型":kind,"链接":row["url"],"原链接":row["origin_url"],"标题":row["title"],
            "简介":row.get("description").unwrap_or(&row["desc"]),
            "封面":row.get("pic").or_else(|| row.get("image")).unwrap_or(&row["thumbnail"]),
            "时长":row["duration"],"视频编号":row["av_no"],"播放器链接":row["iframe_path"],
            "站点":row["site_name"]})
        })
        .collect();
    rows.into()
}

pub(super) fn guild(event: &Value) -> Value {
    if event["platform"] != "kook" {
        return Value::Null;
    }
    let body = &event["kook"];
    let kind = event["sub_type"].as_str().unwrap_or_default();
    let id = body.get("guild_id").or_else(|| event.get("groups_id"));
    let operation = match kind {
        "updated_guild" => "服务器更新",
        "deleted_guild" => "服务器删除",
        "self_joined_guild" => "机器人加入",
        "self_exited_guild" => "机器人退出",
        "joined_guild" => "成员加入",
        "exited_guild" => "成员退出",
        "updated_guild_member" => "成员更新",
        "guild_member_online" => "成员上线",
        "guild_member_offline" => "成员下线",
        "added_block_list" => "封禁成员",
        "deleted_block_list" => "解除封禁",
        "added_emoji" => "添加表情",
        "updated_emoji" => "更新表情",
        "removed_emoji" => "删除表情",
        _ => return Value::Null,
    };
    let mut result = json!({"类型":operation,"服务器ID":id});
    let mut insert = |fields: &[(&str, &str)]| {
        for (label, field) in fields {
            if let Some(value) = body.get(*field) {
                result[*label] = value.clone();
            }
        }
    };
    match kind {
        "updated_guild" | "deleted_guild" => insert(&[
            ("服务器ID", "id"),
            ("名称", "name"),
            ("所有者ID", "user_id"),
            ("图标", "icon"),
            ("通知类型", "notify_type"),
            ("地区", "region"),
            ("公开", "enable_open"),
            ("公开ID", "open_id"),
            ("默认频道ID", "default_channel_id"),
            ("欢迎频道ID", "welcome_channel_id"),
        ]),
        "self_joined_guild" => insert(&[("授权状态", "state")]),
        "joined_guild" => insert(&[("用户ID", "user_id"), ("加入时间", "joined_at")]),
        "exited_guild" => insert(&[("用户ID", "user_id"), ("退出时间", "exited_at")]),
        "updated_guild_member" => insert(&[
            ("用户ID", "user_id"),
            ("用户ID", "id"),
            ("用户名", "username"),
            ("昵称", "nickname"),
            ("识别号", "identify_num"),
            ("在线", "online"),
            ("机器人", "bot"),
            ("状态", "status"),
            ("头像", "avatar"),
            ("会员头像", "vip_avatar"),
            ("手机已认证", "mobile_verified"),
            ("身份组ID列表", "roles"),
            ("助力开始时间", "boost_start_at"),
        ]),
        "guild_member_online" | "guild_member_offline" => {
            insert(&[
                ("用户ID", "user_id"),
                ("事件时间", "event_time"),
                ("共同服务器ID列表", "guilds"),
            ]);
            result["在线"] = (kind == "guild_member_online").into();
        }
        "added_block_list" | "deleted_block_list" => insert(&[
            ("用户ID列表", "user_id"),
            ("操作人ID", "operator_id"),
            ("原因", "remark"),
        ]),
        "added_emoji" | "updated_emoji" | "removed_emoji" => {
            insert(&[("表情ID", "id"), ("表情名称", "name")])
        }
        _ => (),
    }
    result
}
