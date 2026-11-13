//! OneBot11 事件/请求/响应的 ID 映射与格式转换。
//!
//! 映射表见 [`crate::store`]；本模块只做纯转换（外加写入映射）。
//! 旧位置为宿主 `onebot11s`，随 OneBot 能力整体迁入插件。

use redlang::mytool::{read_json_str, str_msg_to_arr};

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn red_id_to_ob(red_id: &str) -> i32 {
    crate::store::store().red_id_to_ob(red_id, now_secs())
}

fn change_msg_id_to_i32(root: &mut serde_json::Value) {
    if let Some(object) = root.as_object_mut() {
        for (key, value) in object {
            if key == "message_id" {
                if let Some(text) = value.as_str() {
                    *value = serde_json::json!(red_id_to_ob(text));
                }
            } else if value.is_array() || value.is_object() {
                change_msg_id_to_i32(value);
            }
        }
    } else if let Some(array) = root.as_array_mut() {
        for value in array {
            change_msg_id_to_i32(value);
        }
    }
}

fn mycrc64(data: &str) -> i64 {
    // 不取 abs：i64::MIN.abs() 会 panic(debug)/保持负数(release)，
    // 且负值键在 remember_uid/red_uid 两侧一致使用，无需取正
    crc64::crc64(0, data.as_bytes()) as i64
}

fn change_uid_to_i64(root: &mut serde_json::Value) {
    if let Some(object) = root.as_object_mut() {
        for (key, value) in object {
            if matches!(
                key.as_str(),
                "target_id" | "user_id" | "group_id" | "self_id" | "message_id" | "operator_id"
            ) {
                if let Some(text) = value.as_str() {
                    if let Ok(id) = text.parse::<i64>() {
                        *value = serde_json::json!(id);
                    } else {
                        let checksum = mycrc64(text);
                        crate::store::store().remember_uid(checksum, text.to_owned());
                        *value = serde_json::json!(checksum);
                    }
                }
            } else if value.is_array() || value.is_object() {
                change_uid_to_i64(value);
            }
        }
    } else if let Some(array) = root.as_array_mut() {
        for value in array {
            change_uid_to_i64(value);
        }
    }
}

fn ob_id_to_red(ob_id: i32) -> Option<String> {
    crate::store::store().ob_id_to_red(ob_id, now_secs())
}

fn change_event_at_and_reply(
    root: &mut serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(msg) = root.get_mut("message") else {
        return Ok(());
    };
    if msg.is_string() {
        *msg = str_msg_to_arr(msg).map_err(|error| error.to_string())?;
    }
    // message 字段可能为 null/数字等非法类型，不能假定必为数组
    let msg_arr = msg
        .as_array_mut()
        .ok_or("message field is not string or array")?;
    for node in msg_arr {
        let tp = node.get_mut("type").ok_or("type not in node")?;
        if tp == "at" {
            let data = node.get_mut("data").ok_or("no data in at node")?;
            let qq = data.get_mut("qq").ok_or("no qq in data node")?;
            let qq_str = qq.as_str().ok_or("qq in node not str")?;
            if let Ok(qq_int) = qq_str.parse::<i64>() {
                *qq = serde_json::json!(qq_int.to_string());
            } else {
                let checksum = mycrc64(qq_str);
                crate::store::store().remember_uid(checksum, qq_str.to_owned());
                *qq = serde_json::json!(checksum.to_string());
            }
        } else if tp == "reply" {
            let data = node.get_mut("data").ok_or("no data in reply node")?;
            let id_str = read_json_str(data, "id");
            if id_str.is_empty() {
                return Err("id in node not str".into());
            }
            let id = data.get_mut("id").ok_or("no id in data node")?;
            *id = serde_json::json!(red_id_to_ob(&id_str).to_string());
        }
    }
    Ok(())
}

fn change_obid_to_str(
    root: &mut serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(object) = root.as_object_mut() {
        for (key, value) in object {
            if matches!(
                key.as_str(),
                "target_id" | "user_id" | "group_id" | "self_id"
            ) {
                let ob_uid = if let Some(ob_uid) = value.as_i64() {
                    Some(ob_uid)
                } else if let Some(text) = value.as_str() {
                    Some(text.parse::<i64>()?)
                } else {
                    None
                };
                if let Some(ob_uid) = ob_uid {
                    if let Some(red_uid) = crate::store::store().red_uid(ob_uid) {
                        *value = serde_json::json!(red_uid);
                    }
                }
            } else if value.is_array() || value.is_object() {
                change_obid_to_str(value)?;
            }
        }
    } else if let Some(array) = root.as_array_mut() {
        for value in array {
            change_obid_to_str(value)?;
        }
    }
    Ok(())
}

pub fn deal_event_groups(
    root: &mut serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let groups_id = read_json_str(root, "groups_id");
    if groups_id.is_empty() {
        return Ok(());
    }
    root.as_object_mut()
        .ok_or("event not object")?
        .remove("groups_id");
    let platform = read_json_str(root, "platform");
    let self_id = read_json_str(root, "self_id");
    let Some(group_id) = root.get_mut("group_id") else {
        return Ok(());
    };
    let group_id_int = group_id.as_i64().ok_or("group_id not i64")?;
    let hash_key = format!("{groups_id}|{group_id_int}");
    let new_group_id = mycrc64(&hash_key);
    (*group_id) = serde_json::json!(new_group_id);
    // 按 (platform, self_id) 隔离映射，避免不同平台/账号的数字群号碰撞
    crate::store::store().remember_group(
        &platform,
        &self_id,
        new_group_id,
        group_id_int,
        groups_id,
    );
    Ok(())
}

fn change_params_at_and_reply(
    root: &mut serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(msg) = root.get_mut("message") else {
        return Ok(());
    };
    if msg.is_string() {
        *msg = str_msg_to_arr(msg).map_err(|error| error.to_string())?;
    }
    // message 字段可能为 null/数字等非法类型，不能假定必为数组
    let msg_arr = msg
        .as_array_mut()
        .ok_or("message field is not string or array")?;
    for node in msg_arr {
        let tp = node.get_mut("type").ok_or("type not in node")?;
        if tp == "at" {
            let data = node.get_mut("data").ok_or("no data in at node")?;
            let qq_str = read_json_str(data, "qq");
            // qq 可能是非数字的 red_uid(跨平台用户ID)，解析失败时保持原样，
            // 不能因 `?` 让整条 API 请求失败
            let qq_int_opt = qq_str.parse::<i64>().ok();
            let red_id_opt = qq_int_opt
                .as_ref()
                .and_then(|qq_int| crate::store::store().red_uid(*qq_int));
            let qq = data.get_mut("qq").ok_or("no qq in at data node")?;
            if let Some(red_id) = red_id_opt.as_deref() {
                (*qq) = serde_json::json!(red_id);
            } else if let Some(qq_int) = qq_int_opt {
                (*qq) = serde_json::json!(qq_int.to_string());
            } else {
                (*qq) = serde_json::json!(qq_str);
            }
        } else if tp == "reply" {
            let data = node.get_mut("data").ok_or("no data in reply node")?;
            let id_str = read_json_str(data, "id");
            // id 可能是非数字的 red 消息ID，解析失败或查不到映射时保持原样
            if let Ok(id_int) = id_str.parse::<i32>() {
                if let Some(red_id) = ob_id_to_red(id_int) {
                    let id = data.get_mut("id").ok_or("no id in data reply node")?;
                    (*id) = serde_json::json!(red_id);
                }
            }
        }
    }
    Ok(())
}

fn deal_request_groups(
    root: &mut serde_json::Value,
    platform: &str,
    self_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let group_id = read_json_str(root, "group_id");
    if group_id.is_empty() {
        return Ok(());
    }
    // 两级群（QQ频道/KOOK/Satori 的 guild 频道）在事件转换时会把原生群号
    // 替换成 CRC64 哈希并写入 groups_map；普通群（onebot11/telegram/...）
    // 没有映射，群号本身就是原生 ID。
    //
    // 这里不能按平台名判断：Satori 的 platform 字段是底层平台（qq/kook/...），
    // 并不是 "satori"；同一个连接也可能同时承载 guild 与私聊。映射是否存在
    // 才是唯一可靠的信号；映射本身按 (platform, self_id) 隔离，普通平台不会
    // 命中其它平台的哈希群号。
    let Ok(group_id_int) = group_id.parse::<i64>() else {
        // 非数字群号不可能是两级群哈希，原样透传
        return Ok(());
    };
    let Some((real_group_id, groups_id)) =
        crate::store::store().group(platform, self_id, group_id_int)
    else {
        // 普通群没有映射；两级群映射过期/进程重启后也保持原值，
        // 由适配器/协议端返回明确错误（不会像旧版那样静默丢请求）。
        return Ok(());
    };
    let Some(group_id) = root.get_mut("group_id") else {
        return Ok(());
    };
    *group_id = serde_json::json!(real_group_id);
    if let Some(object) = root.as_object_mut() {
        object.insert("groups_id".to_owned(), serde_json::json!(groups_id));
    }
    Ok(())
}

pub fn event_to_onebot(
    root: &serde_json::Value,
) -> Result<(serde_json::Value, String, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut root = root.clone();
    let passive_id = read_json_str(&root, "message_id");

    change_msg_id_to_i32(&mut root); // 处理message_id
    change_uid_to_i64(&mut root); // 处理处理uid
    change_event_at_and_reply(&mut root)?; // 处理at和回复
    deal_event_groups(&mut root)?; // 处理groups
                                   // 处理platform和self_id
    let platform = read_json_str(&root, "platform");
    let self_id = read_json_str(&root, "self_id");
    // 处理passive_id
    root.as_object_mut()
        .ok_or("event root is not object")?
        .remove("platform");
    if !passive_id.is_empty() {
        let group_id = read_json_str(&root, "group_id");
        let user_id = read_json_str(&root, "user_id");
        let mut key = String::new();
        if !group_id.is_empty() {
            key = group_id;
        } else if !user_id.is_empty() {
            key = user_id;
        }
        if !key.is_empty() {
            // key 可能是非数字的跨平台ID，解析失败时跳过记忆而非让整条事件转换失败
            if let Ok(key_int) = key.parse::<i64>() {
                crate::store::store().remember_passive_id(key_int, passive_id);
            }
        }
    }
    Ok((root, platform, self_id))
}

fn change_params_msg_id(
    root: &mut serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ob_msg_id = read_json_str(root, "message_id");
    if ob_msg_id.is_empty() {
        return Ok(());
    }
    let ob_msg_id_int = ob_msg_id.parse::<i32>()?;
    let red_msg_id = ob_id_to_red(ob_msg_id_int).ok_or("message_id not found")?;
    let Some(object) = root.as_object_mut() else {
        return Err("params not object".into());
    };
    object.insert("message_id".to_owned(), serde_json::json!(red_msg_id));
    Ok(())
}

pub fn request_to_red(
    root: &serde_json::Value,
    platform: &str,
    self_id: &str,
) -> Result<(serde_json::Value, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut root = root.clone();
    let mut passive_id = "".to_string();
    let Some(params) = root.get_mut("params") else {
        return Ok((root, passive_id));
    };

    // 处理 passive_id；非数字 ID 没有被动回复映射，跳过而不是让整条请求失败
    let user_id = read_json_str(params, "user_id");
    let group_id = read_json_str(params, "group_id");
    if let Ok(group_id_int) = group_id.parse::<i64>() {
        if let Some(v) = crate::store::store().passive_id(group_id_int) {
            passive_id = v.to_owned();
        }
    }
    if !user_id.is_empty() && passive_id.is_empty() {
        if let Ok(user_id_int) = user_id.parse::<i64>() {
            if let Some(v) = crate::store::store().passive_id(user_id_int) {
                passive_id = v.to_owned();
            }
        }
    }

    deal_request_groups(params, platform, self_id)?;
    change_obid_to_str(params)?;
    change_params_at_and_reply(params)?;
    change_params_msg_id(params)?;
    Ok((root, passive_id))
}

/// 构造 OneBot11 错误响应，保证请求转换/调用失败时客户端仍能收到回包。
pub fn error_ret(message: &str, echo: Option<serde_json::Value>) -> serde_json::Value {
    let mut root = serde_json::json!({
        "status": "failed",
        "retcode": -1,
        "data": null,
        "wording": message,
    });
    if let Some(echo) = echo {
        root["echo"] = echo;
    }
    root
}

pub fn red_ret_to_ob(
    root: serde_json::Value,
    echo: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut root = root.clone();
    if !root.is_object() {
        return serde_json::json!({
            "retcode":-1,
            "status":"failed",
            "data":root.to_string(),
            "echo":echo
        });
    }
    if let Some(data) = root.get_mut("data") {
        change_msg_id_to_i32(data); // 处理message_id
        change_uid_to_i64(data); // 处理处理uid
    }

    if let Some(object) = root.as_object_mut() {
        match echo {
            // 回显客户端自己的 echo；请求没带 echo 时移除适配器内部 UUID，
            // 避免内部实现细节泄漏给反向接口客户端
            Some(echo) => {
                object.insert("echo".to_owned(), echo);
            }
            None => {
                object.remove("echo");
            }
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_group_mapping_passes_native_id_through() {
        let mut params = json!({"group_id": 9000000000001i64});
        deal_request_groups(&mut params, "onebot11", "10001").unwrap();
        assert_eq!(params["group_id"], json!(9000000000001i64));
        assert!(params.get("groups_id").is_none());
    }

    #[test]
    fn non_numeric_group_id_passes_through() {
        let mut params = json!({"group_id": "channel-abc"});
        deal_request_groups(&mut params, "satori", "10001").unwrap();
        assert_eq!(params["group_id"], json!("channel-abc"));
        assert!(params.get("groups_id").is_none());
    }

    #[test]
    fn mapped_group_is_restored_with_groups_id() {
        let hashed = 9000000000002i64;
        crate::store::store().remember_group(
            "qqguild_public",
            "10001",
            hashed,
            12345,
            "guild-xx".to_owned(),
        );
        let mut params = json!({"group_id": hashed});
        deal_request_groups(&mut params, "qqguild_public", "10001").unwrap();
        assert_eq!(params["group_id"], json!(12345));
        assert_eq!(params["groups_id"], json!("guild-xx"));
    }

    #[test]
    fn mapping_is_isolated_by_platform_and_account() {
        let hashed = 9000000000003i64;
        crate::store::store().remember_group("kook", "20002", hashed, 54321, "guild-yy".to_owned());
        // 其它平台/账号即使数字相同也必须透传
        let mut params = json!({"group_id": hashed});
        deal_request_groups(&mut params, "onebot11", "20002").unwrap();
        assert_eq!(params["group_id"], json!(hashed));
        assert!(params.get("groups_id").is_none());

        let mut params = json!({"group_id": hashed});
        deal_request_groups(&mut params, "kook", "99999").unwrap();
        assert_eq!(params["group_id"], json!(hashed));
        assert!(params.get("groups_id").is_none());
    }

    #[test]
    fn error_response_keeps_echo() {
        let response = error_ret("bad request", Some(json!(42)));
        assert_eq!(response["retcode"], -1);
        assert_eq!(response["status"], "failed");
        assert_eq!(response["echo"], 42);
        assert!(response["wording"]
            .as_str()
            .unwrap_or_default()
            .contains("bad request"));
    }

    #[test]
    fn internal_echo_is_not_leaked_when_client_omits_it() {
        let response = red_ret_to_ob(
            json!({
                "status": "ok",
                "retcode": 0,
                "data": {"message_id": 123},
                "echo": "internal-adapter-uuid"
            }),
            None,
        );
        assert!(response.get("echo").is_none());
    }
}
