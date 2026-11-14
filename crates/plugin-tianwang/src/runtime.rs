use super::*;

pub(crate) fn handle_event(event: &Event) {
    if is_self_event(event) {
        return;
    }
    let session_key = session_key(event);
    if session_key.is_empty() {
        return;
    }

    let config = cached_config();
    if !group_allowed(event, &config) {
        return;
    }
    // 即使没有文本（例如纯图片），也要记录原始事件 JSON 作为上下文。
    let text = extract_text(event).unwrap_or_default();
    let triggered =
        config.is_ready() && !text.is_empty() && match_trigger(event, &text, &config).is_some();
    session::dispatch(session_key, event.clone(), triggered);
}

pub(crate) fn is_self_event(event: &Event) -> bool {
    let user_id = event
        .get("user_id")
        .map(value_to_string)
        .unwrap_or_default();
    !user_id.is_empty() && user_id == event.self_id
}

/// 群白名单为空时所有群都不启用；非空时仅白名单内的群会进入上下文并触发回复。
pub(crate) fn group_allowed(event: &Event, config: &ChatConfig) -> bool {
    if event.message_type.as_deref() != Some("group") {
        return true;
    }
    let group_id = event
        .get("group_id")
        .map(value_to_string)
        .unwrap_or_default();
    !group_id.is_empty() && config.group_whitelist.iter().any(|allowed| allowed == &group_id)
}

pub(crate) async fn call_api_json(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    remote_id: &str,
    action: Value,
) -> Value {
    let text = action.to_string();
    let platform = platform.to_owned();
    let self_id = self_id.to_owned();
    let passive_id = passive_id.to_owned();
    let remote_id = remote_id.to_owned();
    let send_handle = redlang::runtime().spawn_blocking(move || {
        redlang::cqapi::cq_call_api(&platform, &self_id, &passive_id, &text, &remote_id)
    });
    match send_handle.await {
        Ok(ret) => serde_json::from_str(&ret).unwrap_or(json!({"retcode": -1, "raw": ret})),
        Err(_) => json!({"retcode": -1}),
    }
}

pub(crate) fn api_ok(ret: &Value) -> bool {
    ret.get("retcode").and_then(Value::as_i64) == Some(0)
}

pub(crate) async fn bot_api_call(
    request: Request<Incoming>,
    _context: RouteContext,
) -> std::result::Result<HttpResponse, HttpError> {
    let body = collect_body(request.into_body(), usize::MAX).await?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|error| HttpError::bad_request(format!("JSON 解析失败：{error}")))?;
    let platform = required_api_text(&payload, "platform")?;
    let self_id = required_api_text(&payload, "self_id")?;
    let passive_id = optional_api_text(&payload, "passive_id");
    let remote_id = optional_api_text(&payload, "remote_id");
    let action = required_api_text(&payload, "action")?;
    let params = match payload.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => return Err(HttpError::bad_request("params 必须是 JSON 对象")),
    };
    let mut call = json!({"action": action});
    if params.as_object().is_some_and(|params| !params.is_empty()) {
        call["params"] = params;
    }
    log_info(format!("ChatGPT Bot API 调用：{action}"));
    let result = call_api_json(&platform, &self_id, &passive_id, &remote_id, call).await;
    Ok(json_response(StatusCode::OK, &result))
}

pub(crate) fn required_api_text(
    payload: &Value,
    key: &str,
) -> std::result::Result<String, HttpError> {
    let text = match payload.get(key) {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    if text.is_empty() {
        return Err(HttpError::bad_request(format!("缺少 {key}")));
    }
    Ok(text)
}

pub(crate) fn optional_api_text(payload: &Value, key: &str) -> String {
    match payload.get(key) {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

pub(crate) const STREAM_FLUSH_CHARS: usize = 120;

pub(crate) enum ReplySink {
    Whole,
    QqStream(QqStreamState),
}

pub(crate) struct QqStreamState {
    platform: String,
    self_id: String,
    passive_id: String,
    remote_id: String,
    user_id: String,
    session_id: Option<String>,
    index: u64,
    pending: String,
    failed: bool,
}

impl ReplySink {
    pub(crate) fn for_event(
        platform: &str,
        message_type: &str,
        self_id: &str,
        passive_id: &str,
        remote_id: &str,
        user_id: Option<&Value>,
    ) -> Self {
        let user_id = user_id.map(value_to_string).unwrap_or_default();
        if platform == "qqgroup_public" && message_type == "private" && !user_id.is_empty() {
            ReplySink::QqStream(QqStreamState {
                platform: platform.to_owned(),
                self_id: self_id.to_owned(),
                passive_id: passive_id.to_owned(),
                remote_id: remote_id.to_owned(),
                user_id,
                session_id: None,
                index: 1,
                pending: String::new(),
                failed: false,
            })
        } else {
            ReplySink::Whole
        }
    }

    pub(crate) async fn push(&mut self, piece: &str) {
        if let ReplySink::QqStream(state) = self {
            state.push(piece).await;
        }
    }

    pub(crate) async fn finish_ok(&mut self) -> bool {
        match self {
            ReplySink::Whole => false,
            ReplySink::QqStream(state) => state.finish_ok().await,
        }
    }

    pub(crate) async fn finish_err(&mut self, err_text: Option<&str>) -> Option<String> {
        match self {
            ReplySink::Whole => err_text.map(str::to_owned),
            ReplySink::QqStream(state) => {
                state.finish_err(err_text).await;
                None
            }
        }
    }
}

impl QqStreamState {
    pub(crate) async fn push(&mut self, piece: &str) {
        if self.failed || piece.is_empty() {
            return;
        }
        self.pending.push_str(piece);
        if self.pending.chars().count() >= STREAM_FLUSH_CHARS {
            self.flush().await;
        }
    }

    async fn flush(&mut self) {
        if self.failed || self.pending.is_empty() {
            return;
        }
        let content = std::mem::take(&mut self.pending);
        if self.session_id.is_none() {
            match self.call_start(&content).await {
                Ok(session_id) => {
                    self.session_id = Some(session_id);
                }
                Err(error) => {
                    self.failed = true;
                    log_warn(format!("ChatGPT 流式开始失败:{error}"));
                }
            }
            return;
        }
        let index = self.index;
        self.index += 1;
        if let Err(error) = self.call("append", &content, index).await {
            self.failed = true;
            log_warn(format!("ChatGPT 流式发送失败:{error}"));
        }
    }

    pub(crate) async fn finish_ok(&mut self) -> bool {
        self.flush().await;
        if self.failed || self.session_id.is_none() {
            return false;
        }
        let index = self.index;
        self.index += 1;
        if let Err(error) = self.call("end", "", index).await {
            log_warn(format!("ChatGPT 流式结束失败:{error}"));
        }
        true
    }

    async fn finish_err(&mut self, err_text: Option<&str>) {
        if self.session_id.is_none() {
            return;
        }
        let index = self.index;
        self.index += 1;
        let _ = self.call("end", err_text.unwrap_or(""), index).await;
    }

    async fn call_start(&self, content: &str) -> std::result::Result<String, String> {
        let ret = self.call("start", content, 0).await?;
        ret.get("data")
            .and_then(|data| data.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("流式开始未返回 session_id:{ret}"))
    }

    async fn call(
        &self,
        mode: &str,
        content: &str,
        index: u64,
    ) -> std::result::Result<Value, String> {
        let mut params = json!({
            "mode": mode,
            "user_id": self.user_id,
            "content": content,
        });
        if mode != "start" {
            params["session_id"] = json!(self.session_id.clone().unwrap_or_default());
            params["index"] = json!(index);
        }
        let ret = call_api_json(
            &self.platform,
            &self.self_id,
            &self.passive_id,
            &self.remote_id,
            json!({"action": "send_stream_msg", "params": params}),
        )
        .await;
        if !api_ok(&ret) {
            return Err(ret.to_string());
        }
        Ok(ret)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_reply_text(
    platform: &str,
    self_id: &str,
    passive_id: &str,
    remote_id: &str,
    message_type: &str,
    group_id: Option<&Value>,
    user_id: Option<&Value>,
    text: &str,
) {
    let Some(send_json) = build_reply(message_type, group_id, user_id, text) else {
        return;
    };
    let ret = call_api_json(platform, self_id, passive_id, remote_id, send_json).await;
    if !api_ok(&ret) {
        log_warn(format!("ChatGPT 回复失败:{ret}"));
    }
}

pub(crate) fn extract_text(event: &Event) -> Option<String> {
    let message = event.get("message")?;
    let text = match message {
        Value::String(text) => text.clone(),
        Value::Array(segments) => segments
            .iter()
            .filter(|segment| segment.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|segment| {
                segment
                    .get("data")
                    .and_then(|data| data.get("text"))
                    .and_then(Value::as_str)
            })
            .collect::<String>(),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

pub(crate) fn is_at_self(event: &Event) -> bool {
    let Some(Value::Array(segments)) = event.get("message") else {
        return false;
    };
    let self_id = event.self_id.trim();
    if self_id.is_empty() {
        return false;
    }
    segments
        .iter()
        .filter(|segment| segment.get("type").and_then(Value::as_str) == Some("at"))
        .filter_map(|segment| segment.get("data"))
        .any(|data| {
            ["qq", "id", "user_id"]
                .iter()
                .any(|key| data.get(key).map(value_to_string).as_deref() == Some(self_id))
        })
}

pub(crate) fn match_trigger(event: &Event, text: &str, config: &ChatConfig) -> Option<(String, String)> {
    let prompt = match event.message_type.as_deref() {
        Some("group") => {
            if is_at_self(event) {
                text.to_owned()
            } else {
                strip_prefix(text, &config.prefixes)?
            }
        }
        Some("private") => {
            if config.private_always {
                strip_prefix(text, &config.prefixes).unwrap_or_else(|| text.to_owned())
            } else {
                strip_prefix(text, &config.prefixes)?
            }
        }
        _ => return None,
    };
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    Some((prompt.to_owned(), session_key(event)))
}

pub(crate) fn strip_prefix(text: &str, prefixes: &[String]) -> Option<String> {
    prefixes
        .iter()
        .find(|prefix| text.starts_with(prefix.as_str()))
        .map(|prefix| text[prefix.len()..].to_owned())
}

pub(crate) fn session_key(event: &Event) -> String {
    let target = match event.message_type.as_deref() {
        Some("group") => event
            .get("group_id")
            .map(value_to_string)
            .unwrap_or_default(),
        _ => event
            .get("user_id")
            .map(value_to_string)
            .unwrap_or_default(),
    };
    format!("{}:{}:{target}", event.platform, event.self_id)
}

pub(crate) fn build_messages(
    config: &ChatConfig,
    history: &[ChatMsg],
    prompt: &str,
    event_json: &str,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(history.len() + 3);
    if !config.system_prompt.trim().is_empty() {
        messages.push(json!({"role": "system", "content": config.system_prompt}));
    }
    for item in history {
        messages.push(json!({"role": item.role, "content": item.content}));
    }
    let user_content = if event_json.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}\n\n[当前事件原始JSON]\n{event_json}")
    };
    messages.push(json!({"role": "user", "content": user_content}));
    messages
}

pub(crate) fn build_reply(
    message_type: &str,
    group_id: Option<&Value>,
    user_id: Option<&Value>,
    text: &str,
) -> Option<Value> {
    let message = json!([{"type": "text", "data": {"text": text}}]);
    match message_type {
        "group" => {
            let group_id = group_id?.clone();
            Some(json!({
                "action": "send_group_msg",
                "params": {"group_id": group_id, "message": message}
            }))
        }
        "private" => {
            let user_id = user_id?.clone();
            let mut params = json!({"user_id": user_id, "message": message});
            if let Some(group_id) = group_id {
                params["group_id"] = group_id.clone();
            }
            Some(json!({"action": "send_private_msg", "params": params}))
        }
        _ => None,
    }
}

pub(crate) fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

