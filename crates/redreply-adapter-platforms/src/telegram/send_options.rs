use super::*;

pub(super) fn validate(
    options: &Value,
    parts: &[(&str, String, String)],
    topic: &Topic,
    private: bool,
    business_id: &str,
    markup: Option<&Value>,
) -> AdapterResult<()> {
    let Some(options) = options.as_object() else {
        return if options.is_null() {
            Ok(())
        } else {
            Err("Telegram 发送选项需要对象".into())
        };
    };
    let effect = options.contains_key("message_effect_id");
    if let Some(reply) = options.get("reply_parameters") {
        let ephemeral = options.get("ephemeral_message_parameters");
        let cross_chat = reply.get("chat_id").is_some();
        let fallback = reply["allow_sending_without_reply"] == true;
        if !reply.is_object()
            || reply.get("ephemeral_message_id").is_some() && ephemeral.is_none()
            || cross_chat
                && (topic.direct.is_some()
                    || !business_id.is_empty()
                    || ephemeral.is_some()
                    || fallback)
            || fallback && ephemeral.is_some()
            || ephemeral.is_some_and(|value| value.get("receiver_user_id").is_none())
            || !business_id.is_empty() && reply["allow_sending_without_reply"] == false
        {
            return Err("此会话或临时发送方式不支持所选的引用目标或继续发送选项".into());
        }
    }
    if effect && (!private || parts.iter().any(|part| part.0 == "telegram_paid_media")) {
        return Err("消息效果只适用于支持该选项的私聊消息".into());
    }
    if options.contains_key("suggested_post_parameters") {
        if topic.direct.is_none()
            || parts.len() != 1
            || !business_id.is_empty()
            || parts
                .iter()
                .any(|part| matches!(part.0, "media_group" | "telegram_checklist"))
        {
            return Err("建议帖需要频道私信话题及单条支持建议帖的消息".into());
        }
    }
    if let Some(ephemeral) = options.get("ephemeral_message_parameters") {
        let callback = ephemeral["callback_query_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty());
        let receiver = ephemeral["receiver_user_id"]
            .as_i64()
            .is_some_and(|id| id > 0);
        if !ephemeral.is_object()
            || ephemeral.get("receiver_user_id").is_some() && !receiver
            || ephemeral.get("callback_query_id").is_some() && !callback
            || ephemeral
                .get("replace_callback_query_message")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err("Telegram 临时消息的接收者、回调或替换选项无效".into());
        }
        if (!receiver && !callback)
            || (private && !callback)
            || topic.direct.is_some()
            || options.contains_key("suggested_post_parameters")
            || parts.len() != 1
            || !business_id.is_empty()
            || parts.iter().any(|part| {
                matches!(
                    part.0,
                    "media_group"
                        | "telegram_checklist"
                        | "telegram_invoice"
                        | "telegram_paid_media"
                )
            })
            || markup.is_some_and(|markup| !markup["inline_keyboard"].is_array())
        {
            return Err(
                "临时消息需要单条支持的消息、群内指定用户或普通按钮回调，键盘须为行内键盘".into(),
            );
        }
        if options
            .get("ephemeral_reply_id")
            .is_some_and(|id| !id.as_i64().is_some_and(|id| id > 0))
            || ephemeral["replace_callback_query_message"] == true && !callback
        {
            return Err("Telegram 临时回复缺少有效的回调或消息ID".into());
        }
        if parts.iter().any(|part| {
            part.0 == "telegram_location"
                && serde_json::from_str::<Value>(&part.1)
                    .is_ok_and(|value| value["live_period"].as_i64().is_some_and(|n| n != 0))
        }) {
            return Err("临时消息只支持静态位置".into());
        }
    } else if options.contains_key("ephemeral_reply_id") {
        return Err("必须使用临时消息回复临时消息".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_only_removes_public_target_and_reply() {
        let options = json!({"ephemeral_message_parameters":{"callback_query_id":"123","replace_callback_query_message":true}});
        let parts = vec![("text", "Only the user sees this".to_owned(), String::new())];
        assert!(validate(&options, &parts, &Topic::default(), false, "", None).is_ok());
        let mut body = json!({"chat_id":-123,"reply_parameters":{"message_id":5},"message_thread_id":9,"text":"hello"});
        apply(&options, &mut body);
        assert_eq!(
            body,
            json!({"text":"hello","ephemeral_message_parameters":options["ephemeral_message_parameters"]})
        );
        assert!(validate(
            &options,
            &parts,
            &Topic {
                thread: None,
                direct: Some(42)
            },
            false,
            "",
            None
        )
        .is_err());
        assert!(validate(&options, &parts, &Topic::default(), false, "business", None).is_err());
        let malformed = json!({"ephemeral_message_parameters":{"callback_query_id":"123","receiver_user_id":null}});
        assert!(validate(&malformed, &parts, &Topic::default(), false, "", None).is_err());
    }

    #[test]
    fn suggested_posts_require_one_supported_part_in_direct_topic() {
        let options = json!({"suggested_post_parameters":{}});
        let parts = [("text", "suggestion".to_owned(), String::new())];
        let album = [("media_group", String::new(), String::new())];
        let direct = Topic {
            thread: None,
            direct: Some(42),
        };
        assert!(validate(&options, &parts, &direct, false, "", None).is_ok());
        assert!(validate(&options, &parts, &Topic::default(), false, "", None).is_err());
        assert!(validate(&options, &album, &direct, false, "", None).is_err());
    }

    #[test]
    fn reply_options_follow_chat_and_ephemeral_restrictions() {
        let parts = [("text", "reply".to_owned(), String::new())];
        let topic = Topic::default();
        let cross_chat = json!({"reply_parameters":{"chat_id":-200,"message_id":8}});
        assert!(validate(&cross_chat, &parts, &topic, false, "", None).is_ok());
        assert!(validate(&cross_chat, &parts, &topic, true, "business", None).is_err());
        let ephemeral = json!({"reply_parameters":{"ephemeral_message_id":42}});
        assert!(validate(&ephemeral, &parts, &topic, false, "", None).is_err());
        let directed = json!({"reply_parameters":{"ephemeral_message_id":42},"ephemeral_message_parameters":{"receiver_user_id":21}});
        assert!(validate(&directed, &parts, &topic, false, "", None).is_ok());
    }
}

pub(super) fn apply(options: &Value, payload: &mut Value) {
    for field in [
        "disable_notification",
        "protect_content",
        "message_effect_id",
        "suggested_post_parameters",
        "ephemeral_message_parameters",
    ] {
        if let Some(value) = options.get(field) {
            payload[field] = value.clone();
        }
    }
    if let Some(ephemeral) = options.get("ephemeral_message_parameters") {
        if ephemeral.get("receiver_user_id").is_none() {
            let object = payload
                .as_object_mut()
                .expect("Telegram message payload is an object");
            object.remove("chat_id");
            object.remove("message_thread_id");
            object.remove("reply_parameters");
        } else if let Some(id) = options.get("ephemeral_reply_id") {
            payload["reply_parameters"] = json!({"ephemeral_message_id":id});
        }
    }
}
