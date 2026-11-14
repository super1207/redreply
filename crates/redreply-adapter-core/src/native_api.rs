use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Map, Value};

use crate::AdapterResult;

pub fn object(value: &Value) -> AdapterResult<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| "原生 API 参数必须是 JSON 对象".into())
}

pub fn reply(code: i64, data: &Value, message: &str, echo: &Value) -> Value {
    json!({
        "status": if code == 0 { "ok" } else { "failed" },
        "retcode": code,
        "data": data,
        "message": message,
        "echo": echo,
    })
}

pub fn action_result(result: AdapterResult<Value>, echo: &Value) -> Value {
    result.unwrap_or_else(|error| reply(-1, &Value::Null, &error.to_string(), echo))
}

pub fn message_handles(params: &Value) -> AdapterResult<Vec<&str>> {
    let ids = params["message_ids"].as_array().ok_or("消息 ID 需要数组")?;
    ids.iter()
        .map(|id| id.as_str().filter(|id| !id.is_empty()).ok_or_else(|| "消息 ID 需要非空文本".into()))
        .collect()
}

#[derive(Default)]
pub struct DeleteBatch {
    results: Vec<Value>,
}

impl DeleteBatch {
    pub fn record(&mut self, target: Value, result: AdapterResult<Value>) {
        self.results.push(json!({"target": target, "response": action_result(result, &Value::Null)}));
    }

    pub fn finish(self, echo: &Value) -> Value {
        let failed = self.results.iter().filter(|item| item["response"]["retcode"] != 0).count();
        let (code, message) = if failed == 0 { (0, "") } else { (-1, "部分或全部删除请求失败") };
        reply(code, &json!({"results": self.results, "failed": failed}), message, echo)
    }
}

pub fn telegram_url(token: &str, method: &str) -> AdapterResult<reqwest::Url> {
    if method.is_empty()
        || !method.as_bytes()[0].is_ascii_alphabetic()
        || !method.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err("Telegram 原生 API 需要方法名，例如 setMyCommands".into());
    }
    let mut url = reqwest::Url::parse("https://api.telegram.org")?;
    url.path_segments_mut()
        .expect("Telegram API base URL supports path segments")
        .push(&format!("bot{token}"))
        .push(method);
    Ok(url)
}

pub fn kook_url(endpoint: &str) -> AdapterResult<reqwest::Url> {
    let endpoint = endpoint.strip_prefix("/api/v3/").unwrap_or(endpoint);
    if endpoint == "game" {
        return Ok(reqwest::Url::parse("https://www.kookapp.cn/api/v3/game")?);
    }
    let (resource, action) = endpoint
        .split_once('/')
        .ok_or("KOOK API 路径应为 resource/action")?;
    if ![resource, action].iter().all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }) {
        return Err("KOOK API 路径应为 resource/action".into());
    }
    let mut url = reqwest::Url::parse("https://www.kookapp.cn/api/v3/")?;
    url.path_segments_mut()
        .expect("KOOK API base URL supports path segments")
        .pop_if_empty()
        .push(resource)
        .push(action);
    Ok(url)
}

pub fn form_fields(params: &Map<String, Value>) -> impl Iterator<Item = (&str, String)> {
    params
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| {
            (
                name.as_str(),
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        })
}

pub fn post(
    request: reqwest::RequestBuilder,
    params: &Value,
    files: Option<&Value>,
) -> AdapterResult<reqwest::RequestBuilder> {
    let params_obj = object(params)?;
    let Some(files) = files else {
        return Ok(request
            .header("Content-Type", "application/json")
            .body(params.to_string()));
    };
    let files = object(files)?;
    if files.is_empty() {
        return Ok(request
            .header("Content-Type", "application/json")
            .body(params.to_string()));
    }
    let mut form = reqwest::multipart::Form::new();
    for (name, value) in form_fields(params_obj) {
        form = form.text(name.to_owned(), value);
    }
    for (field, file) in files {
        if params_obj.contains_key(field)
            || field.is_empty()
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(format!("无效或重复的 multipart 字段：{field}").into());
        }
        let filename = file["filename"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or("multipart 文件缺少 filename")?;
        let data = file["base64"].as_str().ok_or("multipart 文件缺少 base64")?;
        let mut part = reqwest::multipart::Part::bytes(STANDARD.decode(data)?)
            .file_name(filename.to_owned());
        if let Some(mime) = file["mime_type"].as_str() {
            part = part.mime_str(mime)?;
        }
        form = form.part(field.to_owned(), part);
    }
    Ok(request.multipart(form))
}

pub async fn response(
    request: reqwest::RequestBuilder,
    platform: &str,
) -> AdapterResult<(u16, Value)> {
    // Telegram tokens are part of request URLs. Keep transport errors free of URLs.
    let response = request
        .send()
        .await
        .map_err(|err| format!("{platform} API 请求失败：{}", err.without_url()))?;
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("{platform} API 读取失败：{}", err.without_url()))?;
    let body = serde_json::from_slice(&bytes)
        .map_err(|_| format!("{platform} API 返回了无效 JSON（HTTP {status}）"))?;
    Ok((status, body))
}

pub fn telegram_reply(status: u16, body: &Value, echo: &Value) -> Value {
    if (200..300).contains(&status) && body["ok"] == true {
        return reply(0, &body["result"], "", echo);
    }
    let fallback_code = if status >= 400 { i64::from(status) } else { -1 };
    let mut result = reply(
        body["error_code"].as_i64().unwrap_or(fallback_code),
        &Value::Null,
        body["description"]
            .as_str()
            .unwrap_or("Telegram API 调用失败"),
        echo,
    );
    if let Some(parameters) = body.get("parameters") {
        result["parameters"] = parameters.clone();
    }
    result
}

pub fn kook_reply(status: u16, body: &Value, echo: &Value) -> Value {
    let code = body["code"].as_i64().unwrap_or(-1);
    let code = if (200..300).contains(&status) || code > 0 {
        code
    } else {
        i64::from(status)
    };
    reply(
        code,
        &body["data"],
        body["message"].as_str().unwrap_or(""),
        echo,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_routes_stay_on_the_platform_origins() {
        assert_eq!(
            telegram_url("123:bot-token", "setMyCommands")
                .unwrap()
                .path(),
            "/bot123:bot-token/setMyCommands"
        );
        assert_eq!(
            kook_url("/api/v3/voice/join").unwrap().path(),
            "/api/v3/voice/join"
        );
        assert_eq!(kook_url("game").unwrap().path(), "/api/v3/game");
        assert!(kook_url("guild").is_err());
        for path in ["https://example.org", "x/../voice", "x/y?x=z", "x/%2e%2e"] {
            assert!(kook_url(path).is_err());
            assert!(telegram_url("123:bot-token", path).is_err());
        }
    }

    #[test]
    fn native_replies_keep_provider_errors_and_parameters() {
        let echo = json!("request-id");
        let result = telegram_reply(
            429,
            &json!({
                "ok": false, "error_code": 429, "description": "Too Many Requests", "parameters": {"retry_after": 3}
            }),
            &echo,
        );
        assert_eq!(result["retcode"], 429);
        assert_eq!(result["parameters"]["retry_after"], 3);
        assert_eq!(result["echo"], echo);
        assert_eq!(
            kook_reply(200, &json!({"code": 40000, "message": "failed"}), &echo)["status"],
            "failed"
        );
        assert_eq!(
            telegram_reply(200, &json!({"ok": true, "result": [1, 2]}), &echo)["data"],
            json!([1, 2])
        );
    }

    #[test]
    fn native_multipart_serializes_nested_parameters_and_files() {
        let request = post(
            reqwest::Client::new().post("https://api.telegram.org"),
            &json!({"photo":"attach://upload", "reply_markup":{"inline_keyboard":[]}}),
            Some(&json!({"upload":{"filename":"test.png","base64":"YWJj"}})),
        )
        .unwrap()
        .build()
        .unwrap();
        assert!(request.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("multipart/form-data;"));
        assert!(post(
            reqwest::Client::new().post("https://api.telegram.org"),
            &json!({"file":"conflicting"}),
            Some(&json!({"file":{"filename":"test.png","base64":"YWJj"}})),
        )
        .is_err());
    }
}
