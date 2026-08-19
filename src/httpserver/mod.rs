use crate::cqapi::cq_add_log_w;
use crate::cqapi::{cq_add_log, cq_call_api, cq_get_app_directory1_async, get_history_log};
use crate::cqevent::do_script;
use crate::httpevent::do_http_event;
use crate::mytool::read_json_str;
use crate::onebot11s::event_to_onebot;
use crate::redlang::RedLang;
use crate::status::get_status;
use crate::{
    config_manager::{
        get_ban_pkgs, read_readonly_web_password, read_web_password, set_ban_pkg,
        set_disabled_ws_urls, set_ws_urls,
    },
    initevent,
    lifecycle::wait_for_quit,
    resource_manager::get_version,
    runtime, runtime_context,
    script_manager::{set_gobal_filter_code, set_gobal_init_code},
};
use bytes::Bytes;
use futures_util::TryStreamExt;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::service::service_fn;
use hyper::{Method, Response, StatusCode};
use serde::de::DeserializeOwned;

use serde_json::json;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime};
use tokio_util::bytes::Buf;
use tokio_util::sync::CancellationToken;

pub(crate) type GenericError = Box<dyn std::error::Error + Send + Sync>;
pub(crate) type BoxBody = http_body_util::combinators::BoxBody<Bytes, GenericError>;
pub(crate) type Result<T> = std::result::Result<T, GenericError>;

mod command;
mod live2d;
use live2d::*;

pub fn add_ws_log(log_msg: String) {
    runtime().spawn(async move {
        runtime_context().session.broadcast_log(log_msg).await;
    });
}

async fn deal_api(
    request: hyper::Request<hyper::body::Incoming>,
    can_write: bool,
    can_read: bool,
) -> Result<hyper::Response<BoxBody>> {
    let url_path = request.uri().path();

    if url_path == "/source_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let mut path = String::new();
        if let Some(query) = request.uri().query() {
            for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
                if k == "path" {
                    path = v.into_owned();
                }
            }
        }
        let ret = match crate::embedded_source::get_source(&path) {
            Some(content) => json!({"retcode": 0, "data": content}),
            None => json!({"retcode": -1, "data": "文件不存在或未嵌入"}),
        };
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        return Ok(res);
    } else if url_path == "/source_code_list" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        // 按目录查看已嵌入源码：?dir=src/httpserver 返回该目录下的子目录与文件，
        // 逐层进入即可浏览全部文件，避免一次性返回全部文件列表。
        let mut dir = String::new();
        if let Some(query) = request.uri().query() {
            for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
                if k == "dir" {
                    dir = v.into_owned();
                }
            }
        }
        let (subdirs, files, parent) = crate::embedded_source::source_list_dir(&dir);
        let ret = json!({
            "retcode": 0,
            "data": {
                "dir": dir,
                "parent": parent,
                "subdirs": subdirs,
                "files": files
            }
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        return Ok(res);
    }

    if url_path == "/live2d_chat_config" {
        if request.method() == Method::GET {
            if !can_write {
                return Ok(hyper::Response::new(full("api not found")));
            }
            match crate::config_manager::live2d_chat_config() {
                Ok(config) => {
                    let (context_used, context_used_estimated) = live2d_agent_context_usage().await;
                    Ok(live2d_chat_config_response(
                        &config,
                        can_write,
                        context_used,
                        context_used_estimated,
                    ))
                }
                Err(error) => Ok(json_api_error(&format!("读取对话设置失败: {error}"))),
            }
        } else if request.method() == Method::POST {
            if !can_write {
                return Ok(hyper::Response::new(full("api not found")));
            }
            let update: Live2dChatConfigUpdate =
                match read_json_body(request, LIVE2D_CHAT_BODY_LIMIT).await {
                    Ok(update) => update,
                    Err(error) => return Ok(json_api_error(&format!("对话设置格式错误: {error}"))),
                };
            let _config_guard = live2d_agent_flow_lock().lock().await;
            match live2d_agent_has_pending_tools().await {
                Ok(true) => {
                    return Ok(json_api_error("工具执行期间不能修改对话设置，请稍后重试"));
                }
                Ok(false) => {}
                Err(error) => return Ok(json_api_error(&format!("读取对话状态失败: {error}"))),
            }
            let mut config = match crate::config_manager::live2d_chat_config() {
                Ok(config) => config,
                Err(error) => return Ok(json_api_error(&format!("读取对话设置失败: {error}"))),
            };
            config.config = update.config.trim().to_owned();
            config.configs = update.configs;
            {
                let mut seen = std::collections::HashSet::new();
                config.configs.retain(|item| seen.insert(item.name.clone()));
            }
            if config.configs.is_empty() {
                return Ok(json_api_error("配置列表不能为空"));
            }
            if !config.configs.iter().any(|item| item.name == config.config) {
                return Ok(json_api_error("当前选中的配置必须存在于配置列表中"));
            }
            let current = config
                .configs
                .iter()
                .find(|item| item.name == config.config)
                .expect("当前选中配置已确认存在");
            config.base_url = current.base_url.clone();
            config.api_key = current.api_key.clone();
            config.model = current.model.clone();
            config.thinking_level = current.thinking_level.clone();
            config.context_length = current.context_length;
            config.proxy = current.proxy.clone();
            config.system_prompt = update.system_prompt.trim().to_owned();
            let config_to_save = config.clone();
            match tokio::task::spawn_blocking(move || {
                crate::config_manager::save_live2d_chat_config(&config_to_save)
            })
            .await?
            {
                Ok(()) => {
                    if let Some(messages) = match get_live2d_agent_conversation().await {
                        Ok(messages) => messages,
                        Err(error) => {
                            return Ok(json_api_error(&format!("读取对话状态失败: {error}")));
                        }
                    } {
                        let used = crate::ai_client::estimate_context_tokens(&messages);
                        if used > config.context_length {
                            if let Err(error) = clear_live2d_agent_conversation().await {
                                return Ok(json_api_error(&format!("清除超限对话失败: {error}")));
                            }
                        }
                    }
                    let (context_used, context_used_estimated) = live2d_agent_context_usage().await;
                    Ok(live2d_chat_config_response(
                        &config,
                        true,
                        context_used,
                        context_used_estimated,
                    ))
                }
                Err(error) => Ok(json_api_error(&format!("保存对话设置失败: {error}"))),
            }
        } else {
            Ok(method_not_allowed())
        }
    } else if url_path == "/live2d_chat_clear" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        let _clear_guard = match live2d_agent_flow_lock().try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                return Ok(json_api_error("当前正在对话中，请先停止对话再清除上下文"));
            }
        };
        match live2d_agent_has_pending_tools().await {
            Ok(true) => return Ok(json_api_error("工具执行期间不能清除对话，请稍后重试")),
            Ok(false) => {}
            Err(error) => return Ok(json_api_error(&format!("读取对话状态失败: {error}"))),
        }
        return match clear_live2d_agent_conversation().await {
            Ok(()) => Ok(json_api_response(json!({
                "retcode": 0,
                "data": {
                    "cleared": true,
                    "context_tokens": 0
                }
            }))),
            Err(error) => Ok(json_api_error(&format!("清除对话失败: {error}"))),
        };
    } else if url_path == "/live2d_chat_stream" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        return deal_live2d_chat_stream(request).await;
    } else if url_path == "/live2d_chat_cancel" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        let cancel_request: Live2dCancelRequest =
            match read_json_body(request, LIVE2D_CHAT_BODY_LIMIT).await {
                Ok(cancel_request) => cancel_request,
                Err(error) => return Ok(json_api_error(&format!("取消请求格式错误: {error}"))),
            };
        // 仅终止本次对话期间由前端发起的 run_c 命令进程（command_id == 对话 generation）
        return match command::cancel_generation(&cancel_request.generation).await {
            Ok(()) => Ok(json_api_response(json!({"retcode": 0, "data": true}))),
            Err(error) => Ok(json_api_error(&error.to_string())),
        };
    } else if url_path == "/live2d_chat_append" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        let append_request: Live2dAppendRequest =
            match read_json_body(request, LIVE2D_APPEND_BODY_LIMIT).await {
                Ok(append_request) => append_request,
                Err(error) => return Ok(json_api_error(&format!("落库请求格式错误: {error}"))),
            };
        return match deal_live2d_chat_append(append_request).await {
            Ok(()) => Ok(json_api_response(json!({"retcode": 0, "data": true}))),
            Err(error) => Ok(json_api_error(&error)),
        };
    } else if url_path == "/live2d_chat_state" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::GET {
            return Ok(method_not_allowed());
        }
        return deal_live2d_chat_state().await;
    } else if url_path == "/run_command" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        return command::handle(request).await;
    } else if url_path == "/run_c" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        return command::handle_run_c(request).await;
    } else if url_path == "/run_command_cancel" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        let cancel_request: Live2dCancelRequest =
            match read_json_body(request, LIVE2D_CHAT_BODY_LIMIT).await {
                Ok(cancel_request) => cancel_request,
                Err(error) => return Ok(json_api_error(&format!("取消命令格式错误: {error}"))),
            };
        return match command::cancel_generation(&cancel_request.generation).await {
            Ok(()) => Ok(json_api_response(json!({"retcode": 0, "data": true}))),
            Err(error) => Ok(json_api_error(&error)),
        };
    } else if url_path == "/live2d_conversation" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() == Method::POST {
            let mutation: Live2dConversationMutation =
                match read_json_body(request, LIVE2D_CONVERSATION_BODY_LIMIT).await {
                    Ok(mutation) => mutation,
                    Err(error) => return Ok(json_api_error(&format!("修改对话格式错误: {error}"))),
                };
            let _mutate_guard = live2d_agent_flow_lock().lock().await;
            match live2d_agent_has_pending_tools().await {
                Ok(true) => return Ok(json_api_error("工具执行期间不能编辑对话，请稍后重试")),
                Ok(false) => {}
                Err(error) => return Ok(json_api_error(&format!("读取对话状态失败: {error}"))),
            }
            return match mutate_live2d_agent_conversation(mutation).await {
                Ok(messages) => Ok(json_api_response(json!({
                    "retcode": 0,
                    "data": {"count": messages.len()}
                }))),
                Err(error) => Ok(json_api_error(&error)),
            };
        }
        if request.method() != Method::GET {
            return Ok(method_not_allowed());
        }
        let messages = match get_live2d_agent_conversation().await {
            Ok(messages) => messages.unwrap_or_default(),
            Err(error) => return Ok(json_api_error(&format!("读取对话状态失败: {error}"))),
        };
        let count = messages.len();
        let config = match crate::config_manager::live2d_chat_config() {
            Ok(config) => config,
            Err(error) => return Ok(json_api_error(&format!("读取对话设置失败: {error}"))),
        };
        let diaries = tokio::task::spawn_blocking(crate::resource_manager::list_live2d_diaries)
            .await
            .ok()
            .and_then(|result| result.ok())
            .unwrap_or_default();
        let next_payload =
            crate::ai_client::build_next_request_payload(&config, &messages, &diaries)
                .unwrap_or_else(|error| serde_json::json!({"error": error.to_string()}));
        let payload = json!({
            "retcode": 0,
            "data": {
                "messages": messages,
                "count": count,
                "next_payload": next_payload
            }
        });
        let mut res = hyper::Response::new(full(payload.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/live2d_diary" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::GET {
            return Ok(method_not_allowed());
        }
        let items =
            match tokio::task::spawn_blocking(crate::resource_manager::list_live2d_diaries).await {
                Ok(Ok(items)) => items,
                Ok(Err(error)) => return Ok(json_api_error(&format!("读取日记本失败: {error}"))),
                Err(error) => return Ok(json_api_error(&format!("读取日记本失败: {error}"))),
            };
        let payload = json!({
            "retcode": 0,
            "data": {
                "items": items
                    .into_iter()
                    .map(|entry| json!({
                        "key": entry.key,
                        "title": entry.title,
                        "content": entry.content,
                        "create_time": entry.create_time,
                        "update_time": entry.update_time
                    }))
                    .collect::<Vec<_>>(),
                "total": crate::resource_manager::LIVE2D_DIARY_LIMIT
            }
        });
        let mut res = hyper::Response::new(full(payload.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/live2d_diary_query" {
        if !can_write {
            return Ok(hyper::Response::new(full("api not found")));
        }
        if request.method() != Method::POST {
            return Ok(method_not_allowed());
        }
        let body = match read_json_body::<serde_json::Value>(request, LIVE2D_CHAT_BODY_LIMIT).await
        {
            Ok(body) => body,
            Err(error) => return Ok(json_api_error(&format!("查询格式错误: {error}"))),
        };
        let sql = match body.get("sql").and_then(serde_json::Value::as_str) {
            Some(sql) => sql.trim(),
            None => return Ok(json_api_error("缺少 sql 字段")),
        };
        if sql.is_empty() {
            return Ok(json_api_error("sql 不能为空"));
        }
        if sql.chars().count() > 2000 {
            return Ok(json_api_error("sql 过长"));
        }
        let sql = sql.to_owned();
        let query_result =
            tokio::task::spawn_blocking(move || crate::resource_manager::query_live2d_db(&sql))
                .await;
        let (columns, rows) = match query_result {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                return Ok(json_api_error(&format!("查询失败: {error}")));
            }
            Err(error) => {
                return Ok(json_api_error(&format!("查询失败: {error}")));
            }
        };
        let payload = json!({
            "retcode": 0,
            "data": {"columns": columns, "rows": rows}
        });
        let mut res = hyper::Response::new(full(payload.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/get_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match Ok::<_, GenericError>(runtime_context().scripts.read_scripts()) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/backup_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::backup_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/test_cron" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let def_str = String::new();
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let key = params.get("key").unwrap_or(&def_str);
        let schedule_rst = <cron::Schedule as std::str::FromStr>::from_str(key);
        let ret: serde_json::Value;
        let mut timestamp_vec = vec![];
        if let Ok(schedule) = schedule_rst {
            let now_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;
            let datetime_rst =
                chrono::TimeZone::timestamp_opt(&chrono::prelude::Local, now_time, 0);

            if let chrono::LocalResult::Single(data) = datetime_rst {
                for datetime in schedule.after(&data).take(10) {
                    timestamp_vec.push(datetime.to_rfc3339());
                }
            }
            ret = json!({
                "retcode":0,
                "data":timestamp_vec
            });
        } else {
            ret = json!({
                "retcode":-1,
                "data":timestamp_vec
            });
        }
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path.starts_with("/5350b16b-b5e2-425a-bba1-d33d92813ab4/") {
        // github代理
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let sp_ret = url_path
            .split("5350b16b-b5e2-425a-bba1-d33d92813ab4")
            .collect::<Vec<&str>>();
        let github_url = sp_ret.get(1).ok_or("url error")?;
        let git_proxy = crate::mytool::get_github_proxy().await?;
        // 不再禁用证书校验：代理地址是动态获取的外部服务，关闭校验等于
        // 允许中间人替换下载内容；同时加请求超时
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        let uri =
            <reqwest::Url as std::str::FromStr>::from_str(&(git_proxy.to_owned() + github_url))?;
        let req = client.get(uri).build()?;
        if let Ok(ret) = client.execute(req).await {
            if ret.status() == reqwest::StatusCode::OK {
                let ct = ret
                    .headers()
                    .get("Content-Type")
                    .ok_or("content type error")?
                    .to_owned();
                // 流式接收并限制总量，避免大文件把内存吃爆
                use futures_util::StreamExt;
                const MAX_PROXY_BODY: usize = 64 * 1024 * 1024;
                let mut buf: Vec<u8> = Vec::new();
                let mut intact = true;
                let mut stream = ret.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            if buf.len() + bytes.len() > MAX_PROXY_BODY {
                                intact = false;
                                break;
                            }
                            buf.extend_from_slice(&bytes);
                        }
                        Err(_) => {
                            intact = false;
                            break;
                        }
                    }
                }
                if intact {
                    let mut res = hyper::Response::new(crate::httpserver::full(buf));
                    res.headers_mut().insert("Content-Type", ct);
                    return Ok(res);
                }
            }
        }
        let res = hyper::Response::new(full("access github error"));
        Ok(res)
    } else if url_path == "/read_one_pkg" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name;
        if let Some(name) = params.get("pkg_name") {
            pkg_name = name.to_owned();
        } else {
            pkg_name = "".to_owned();
        }
        match crate::script_manager::read_one_pkg(&pkg_name) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_all_pkg_name" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match Ok::<_, GenericError>(runtime_context().scripts.package_names()) {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_ban_pkgs" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match get_ban_pkgs() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/set_ban_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let pkg_name = read_json_str(&js, "pkg_name");
        let banned = js.get("banned").and_then(|v| v.as_bool()).unwrap_or(false);
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = set_ban_pkg(&pkg_name, banned);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/get_config" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::config_manager::read_config() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/set_ws_urls" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        match set_ws_urls(js) {
            Ok(_) => {
                let ret = json!({
                    "retcode":0,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(_) => {
                let ret = json!({
                    "retcode":-1,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
        }
    } else if url_path == "/set_disabled_ws_urls" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        match set_disabled_ws_urls(js) {
            Ok(_) => {
                let ret = json!({
                    "retcode":0,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(_) => {
                let ret = json!({
                    "retcode":-1,
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
        }
    } else if url_path == "/set_gobal_filter_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if code.is_string() {
                let rst = set_gobal_filter_code(code.as_str().unwrap());
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                } else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                    }
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/set_gobal_init_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let js = js;
            let code = &js["data"];
            if code.is_string() {
                let rst = set_gobal_init_code(code.as_str().unwrap());
                if rst.is_err() {
                    let ret = json!({
                        "retcode":-1,
                    });
                    let rst = tx.send(ret);
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                } else {
                    let ret = json!({
                        "retcode":0,
                    });
                    let rst = tx.send(ret);
                    if rst.is_err() {
                        cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                    }
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/get_gobal_filter_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::get_gobal_filter_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/get_gobal_init_code" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match crate::script_manager::get_gobal_init_code() {
            Ok(code) => {
                let ret = json!({
                    "retcode":0,
                    "data":code
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path == "/set_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::save_code(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            } else {
                cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/save_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::save_one_pkg(&js.to_string());
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/rename_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let old_pkg_name = read_json_str(&js, "old_pkg_name");
        let new_pkg_name = read_json_str(&js, "new_pkg_name");
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::rename_one_pkg(&old_pkg_name, &new_pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/del_one_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let js: serde_json::Value = serde_json::from_reader(body)?;
        let pkg_name = read_json_str(&js, "pkg_name");
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let rst = crate::script_manager::del_one_pkg(&pkg_name);
            if rst.is_ok() {
                let ret = json!({
                    "retcode":0,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            } else {
                let ret = json!({
                    "retcode":-1,
                });
                let rst = tx.send(ret);
                if rst.is_err() {
                    cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/run_code" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root: serde_json::Value = serde_json::from_reader(body)?;

        let bot_id = read_json_str(&root, "bot_id");
        let platform = read_json_str(&root, "platform");
        let user_id = read_json_str(&root, "user_id");
        let group_id = read_json_str(&root, "group_id");
        let code = read_json_str(&root, "content");
        let mut pkg_name = read_json_str(&root, "pkg_name");
        if pkg_name == "默认包" {
            pkg_name = "".to_owned();
        }
        let groups_id = read_json_str(&root, "groups_id");
        thread::spawn(move || {
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id);
            rl.set_exmap("群ID", &group_id);
            rl.set_exmap("群组ID", &groups_id);
            rl.set_exmap("发送者ID", &user_id);
            rl.set_exmap("机器人平台", &platform);
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            if let Err(err) = do_script(&mut rl, &code, "normal", false) {
                cq_add_log_w(&format!("{}", err)).unwrap();
            }
        });
        let mut res = hyper::Response::new(full(
            serde_json::json!({
                "retcode":0,
            })
            .to_string(),
        ));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/run_code_and_ret" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let body = request.collect().await?.aggregate().reader();
        let root: serde_json::Value = serde_json::from_reader(body)?;

        let bot_id = read_json_str(&root, "bot_id");
        let platform = read_json_str(&root, "platform");
        let user_id = read_json_str(&root, "user_id");
        let group_id = read_json_str(&root, "group_id");
        let code = read_json_str(&root, "content");
        let mut pkg_name = read_json_str(&root, "pkg_name");
        if pkg_name == "默认包" {
            pkg_name = "".to_owned();
        }
        let groups_id = read_json_str(&root, "groups_id");

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let mut rl = RedLang::new();
            rl.set_exmap("机器人ID", &bot_id);
            rl.set_exmap("群ID", &group_id);
            rl.set_exmap("群组ID", &groups_id);
            rl.set_exmap("发送者ID", &user_id);
            rl.set_exmap("机器人平台", &platform);
            rl.pkg_name = pkg_name;
            rl.script_name = "网页调试".to_owned();
            rl.can_wrong = true;
            let ret_rst = rl.parse_to_string(&code);
            let res;
            let ret;
            if let Ok(ret_str) = ret_rst {
                ret = ret_str;
                res = serde_json::json!({
                    "retcode":0,
                    "data":ret
                })
                .to_string();
            } else {
                ret = format!("{}", ret_rst.err().unwrap());
                res = serde_json::json!({
                    "retcode":-1,
                    "data":ret
                })
                .to_string();
            }
            let rst = tx.send(res);
            if rst.is_err() {
                cq_add_log_w(&format!("Error:{:?}", rst.err())).unwrap();
            }
        });
        let ret_str = rx.await?;
        let mut res = hyper::Response::new(full(ret_str));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/close" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        cq_add_log_w("收到退出指令，正在退出").unwrap();
        wait_for_quit();
    } else if url_path == "/get_version" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let ret = json!({
            "retcode":0,
            "data":get_version()
        });
        let mut res = hyper::Response::new(full(ret.to_string()));
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
        Ok(res)
    } else if url_path == "/login" {
        let body_len = request
            .headers()
            .get("content-length")
            .ok_or("/login 中没有content-length")?
            .to_str()?
            .parse::<usize>()?;
        let mut res = hyper::Response::new(full(vec![]));
        if body_len < 256 {
            let mut body_reader = request.collect().await?.aggregate().reader();
            let mut body = Vec::new();
            body_reader.read_to_end(&mut body)?;
            //let body = hyper::body::to_bytes(request.into_body()).await?;
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            let body_str = String::from_utf8(body.to_vec())?;
            // 从表单体中取出 password 字段并解码（表单编码中 + 表示空格），
            // 再按认证侧 http_auth 相同的 byte_serialize 编码写入 Cookie：
            // 之前直接回写原始 body，含空格/符号/中文的密码永远无法通过比对
            if let Some((_, pass)) = url::form_urlencoded::parse(body_str.as_bytes())
                .into_iter()
                .find(|(k, _)| k == "password")
            {
                let encoded: String =
                    url::form_urlencoded::byte_serialize(pass.as_bytes()).collect();
                let pass_cookie = format!("password={encoded};Max-Age=31536000");
                res.headers_mut().append(
                    hyper::header::SET_COOKIE,
                    HeaderValue::from_str(&pass_cookie)?,
                );
            }
        }
        res.headers_mut()
            .insert("Location", HeaderValue::from_static("/index.html"));
        Ok(res)
    } else if url_path == "/get_send_recv_status" {
        if !can_read {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        match get_status() {
            Ok(status) => {
                let ret = json!({
                    "retcode": 0,
                    "data": status
                });
                let mut res = hyper::Response::new(full(ret.to_string()));
                res.headers_mut()
                    .insert("Content-Type", HeaderValue::from_static("application/json"));
                Ok(res)
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
                Ok(res)
            }
        }
    } else if url_path.starts_with("/user") {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || match do_http_event(request, can_write, can_read) {
            Ok(response) => {
                if let Err(err) = tx.send(response) {
                    cq_add_log_w(&format!("Error:{err:?}")).unwrap();
                }
            }
            Err(err) => {
                let mut res = hyper::Response::new(full(err.to_string()));
                res.headers_mut().insert(
                    "Content-Type",
                    HeaderValue::from_static("text/html; charset=utf-8"),
                );
                if let Err(send_err) = tx.send(res) {
                    cq_add_log_w(&format!("Error:{send_err:?}")).unwrap();
                }
            }
        })
        .await?;
        let ret = rx.await?;
        // let t = http_body_util::combinators::BoxBody::new(ret);
        // let k = ret.map_err(|never| match never {});
        Ok(ret)
    } else if url_path == "/upload_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }

        let content_len = request
            .headers()
            .get("content-length")
            .ok_or("/upload_pkg 中没有content-length")?;
        let content_len = content_len.to_str()?.parse::<usize>()?;

        // 如果 content_len > 10MB，就拒绝上传
        if content_len > 11 * 1024 * 1024 {
            // 读取前1KB数据（避免完全预读）
            let mut body_stream = request.into_body();
            let mut received = 0;
            while received < 1024 {
                if let Some(frame_result) = body_stream.frame().await {
                    let frame = frame_result?;
                    if let Some(data) = frame.data_ref() {
                        received += data.len();
                    }
                }
            }
            cq_add_log_w("文件太大,请上传小于10MB的文件".to_string().as_str()).unwrap();
            let res = hyper::Response::new(full("文件太大,请上传小于10MB的文件"));
            return Ok(res);
        }

        // 读取body
        let mut body_stream = request.into_body();
        let mut body = Vec::with_capacity(content_len);
        while let Some(frame_result) = body_stream.frame().await {
            let frame = frame_result?;
            if let Some(data) = frame.data_ref() {
                body.extend_from_slice(data);

                if body.len() > content_len {
                    cq_add_log_w("Client sent more data than content-length header specified.")
                        .unwrap();
                    let res =
                        hyper::Response::new(full("Bad request: body size exceeds content-length"));
                    return Ok(res);
                }
            }
        }
        if body.len() != content_len {
            let res =
                hyper::Response::new(full("Bad request: body size does not match content-length"));
            return Ok(res);
        }

        // 将body解析为json,得到filename,file_size,file_content
        let json: serde_json::Value = tokio::task::spawn_blocking(
            move || -> std::result::Result<serde_json::Value, serde_json::Error> {
                serde_json::from_slice(&body)
            },
        )
        .await??;
        let filename = read_json_str(&json, "filename");

        // 文件名可能是1.a.b.z.d.red.7z，我们应该取.red.7z前面的部分
        let name = filename
            .strip_suffix(".red.7z")
            // 如果 strip_suffix 返回 None，ok_or 会将其转换为 Err
            .ok_or("文件名格式错误：必须以 .red.7z 结尾")?
            // 将 &str 转换为拥有的 String
            .to_owned();

        let file_size = read_json_str(&json, "file_size");
        let file_content = read_json_str(&json, "file_content");

        // 将file_content解码
        // 将base64解码操作移到spawn_blocking中执行
        let file_content = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            base64::Engine::decode(
                &base64::engine::GeneralPurpose::new(
                    &base64::alphabet::STANDARD,
                    base64::engine::general_purpose::PAD,
                ),
                file_content,
            )
            .map_err(|e| e.into())
        })
        .await??;

        // 验证file_content与file_size是否一致
        if file_content.len() != file_size.parse::<usize>()? {
            let res = hyper::Response::new(full("文件大小与文件内容不符"));
            return Ok(res);
        }

        // 如果文件目录不存在，就创建
        let tmp_dir = crate::cqapi::get_tmp_dir_async().await?;
        let file_dir = PathBuf::from(&tmp_dir).join("pkg");

        if !file_dir.exists() {
            tokio::fs::create_dir_all(&file_dir).await?;
        }
        // 生成一个随机文件名
        let uid = uuid::Uuid::new_v4().to_string();
        let tmp_file_name = format!("{}.red.7z", uid);
        let tmp_file_path = file_dir.join(&tmp_file_name);

        // 删除临时文件
        let _guard = scopeguard::guard(tmp_file_path.clone(), |path| {
            runtime().spawn(async {
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        // 将文件写入文件
        tokio::fs::write(&tmp_file_path, &file_content).await?;

        // 解压文件
        let decompress_path = file_dir.join(&uid);

        // 删除临时目录
        let _guard = scopeguard::guard(decompress_path.clone(), |path| {
            runtime().spawn(async {
                let _ = tokio::fs::remove_dir_all(path).await;
            });
        });

        // 将解压操作移到spawn_blocking中执行
        let tmp_file_path_clone = tmp_file_path.clone();
        let decompress_path_clone = decompress_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::decompress_file(&tmp_file_path_clone, &decompress_path_clone)?;
            Ok(())
        })
        .await??;

        // 判断其中有无 script.json 文件
        let script_json_path = decompress_path.join("script.json");
        if !script_json_path.exists() {
            let res = hyper::Response::new(full("文件中不存在script.json"));
            return Ok(res);
        }

        let plus_dir_str = cq_get_app_directory1_async().await?;
        let pkg_dir = PathBuf::from_str(&plus_dir_str)?.join("pkg_dir");
        let real_pkg_dir = pkg_dir.join(&name);

        // 与保存/删除/改名共用同一把保存锁：上传对脚本缓存做读-改-写，
        // 不加锁会与并发保存互相丢数据。整个临界区放进阻塞线程执行，
        // 避免在异步上下文持有非 Send 的 std MutexGuard
        let real_pkg_dir_for_install = real_pkg_dir.clone();
        let name_for_install = name.to_owned();
        let installed = tokio::task::spawn_blocking(
            move || -> Result<bool> {
                let _save_guard = crate::script_manager::lock_save()?;
                if real_pkg_dir_for_install.exists() {
                    return Ok(false);
                }
                sevenz_rust2::decompress_file(&tmp_file_path, &real_pkg_dir_for_install)?;
                let mut new_script = vec![];
                let current_scripts = runtime_context().scripts.read_scripts();
                for it in current_scripts.as_array().ok_or("read script cache err")? {
                    let it_name = read_json_str(it, "pkg_name");
                    if it_name != name_for_install {
                        new_script.push(it.to_owned());
                    }
                }
                let scripts_str =
                    std::fs::read_to_string(real_pkg_dir_for_install.join("script.json"))?;
                let mut scripts: serde_json::Value = serde_json::from_str(&scripts_str)?;
                for it in scripts.as_array_mut().ok_or("script.json not array")? {
                    let obj_mut = it.as_object_mut().ok_or("script obj not object")?;
                    obj_mut.insert(
                        "pkg_name".to_owned(),
                        serde_json::json!(name_for_install.as_str()),
                    );
                    new_script.push(it.to_owned());
                }
                runtime_context()
                    .scripts
                    .replace_scripts(serde_json::Value::Array(new_script));
                runtime_context().scripts.add_package_name(name_for_install);
                Ok(true)
            },
        )
        .await??;
        if !installed {
            let res = hyper::Response::new(full("插件已经存在，无法重复安装"));
            return Ok(res);
        }

        // 执行初始化脚本，不用等待
        let name_t = name.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Err(err) = initevent::do_init_event(Some(&name_t)) {
                cq_add_log_w(&err.to_string()).unwrap();
            }
        });

        cq_add_log_w(&format!("上传文件大小为{}", content_len)).unwrap();

        let res = hyper::Response::new(full("ok"));
        Ok(res)
    } else if url_path == "/download_pkg" {
        if !can_write {
            let res = hyper::Response::new(full("api not found"));
            return Ok(res);
        }
        let params = crate::httpevent::get_params_from_uri(request.uri());
        let pkg_name = params.get("pkg_name").cloned().unwrap_or_default();
        let plus_dir = cq_get_app_directory1_async().await?;
        let pkg_dir = if pkg_name.is_empty() {
            PathBuf::from_str(&plus_dir)?.join("default_pkg_dir")
        } else {
            PathBuf::from_str(&plus_dir)?
                .join("pkg_dir")
                .join(&pkg_name)
        };

        // 递归计算pkg_dir大小
        async fn get_size(path: &PathBuf) -> Result<usize> {
            let mut size = 0;
            let mut entries = tokio::fs::read_dir(path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    size += Box::pin(get_size(&path)).await?;
                } else {
                    size += entry.metadata().await?.len() as usize;
                }
            }
            Ok(size)
        }
        let size = get_size(&pkg_dir).await?;

        // 大于20MB，报错
        if size > 20 * 1024 * 1024 {
            let res = hyper::Response::new(full("文件大小超过10MB"));
            return Ok(res);
        }

        let tmp_dir = crate::cqapi::get_tmp_dir_async().await?;
        let file_dir = PathBuf::from(&tmp_dir).join("pkg");

        if !file_dir.exists() {
            tokio::fs::create_dir_all(&file_dir).await?;
        }

        let uid = uuid::Uuid::new_v4().to_string();
        let tmp_file_name = format!("{}.red.7z", uid);
        let tmp_file_path = file_dir.join(&tmp_file_name);

        let _guard = scopeguard::guard(tmp_file_path.clone(), |path| {
            runtime().spawn(async move {
                let _ = tokio::fs::remove_file(path).await;
            });
        });

        let tmp_file_path_clone = tmp_file_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            sevenz_rust2::compress_to_path(&pkg_dir, tmp_file_path_clone)?;
            Ok(())
        })
        .await??;

        let file_bin = tokio::fs::read(tmp_file_path).await?;

        let mut res = hyper::Response::new(full(file_bin));
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("application/octet-stream"),
        );
        // 设置文件名为pkg_name.red.7z，如果pkg_name为空，则为default.red.7z
        if pkg_name.is_empty() {
            res.headers_mut().insert(
                "Content-Disposition",
                HeaderValue::from_str("attachment; filename=default.red.7z")?,
            );
        } else {
            res.headers_mut().insert(
                "Content-Disposition",
                HeaderValue::from_str(&format!("attachment; filename={}.red.7z", pkg_name))?,
            );
        }
        Ok(res)
    } else {
        let res = hyper::Response::new(full("api not found"));
        Ok(res)
    }
}

async fn deal_file(
    request: hyper::Request<hyper::body::Incoming>,
) -> Result<hyper::Response<BoxBody>> {
    let url_path = request.uri().path();
    let app_dir = cq_get_app_directory1_async().await?;
    let path = PathBuf::from(&app_dir);
    let path = path.join("webui");
    let url_path_t = url_path.replace("/", std::path::MAIN_SEPARATOR_STR);
    let file_path = path.join(url_path_t.get(1..).unwrap());
    let file = tokio::fs::File::open(file_path).await?;
    let reader_stream = tokio_util::io::ReaderStream::new(file);
    let stream_body =
        http_body_util::StreamBody::new(reader_stream.map_ok(hyper::body::Frame::data));
    let boxed_body: http_body_util::combinators::BoxBody<tokio_util::bytes::Bytes, std::io::Error> =
        BodyExt::boxed(stream_body);
    let kk = boxed_body
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .boxed();
    let mut res = hyper::Response::new(kk);
    *res.status_mut() = hyper::StatusCode::OK;
    let file_name = url_path.rsplit('/').next().unwrap_or("");
    if matches!(
        file_name,
        "vue.js"
            | "vue3-sfc-loader.min.js"
            | "favicon.ico"
            | "l2d.js"
            | "model.moc"
            | "default-costume.png"
            | "pajamas-costume.png"
            | "school-costume.png"
    ) {
        res.headers_mut()
            .insert("Cache-Control", HeaderValue::from_static("max-age=300"));
    }
    if url_path.ends_with(".html") {
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
    } else if url_path.ends_with(".js") {
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        );
    } else if url_path.ends_with(".css") {
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("text/css; charset=utf-8"),
        );
    } else if url_path.ends_with(".png") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("image/png"));
    } else if url_path.ends_with(".ico") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("image/x-icon"));
    } else if url_path.ends_with(".txt") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("text/plain"));
    } else if url_path.ends_with(".md") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("text/markdown"));
    } else if url_path.ends_with(".woff2") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("font/woff2"));
    } else if url_path.ends_with(".json") {
        res.headers_mut()
            .insert("Content-Type", HeaderValue::from_static("application/json"));
    } else if url_path.ends_with(".moc") || url_path.ends_with(".mtn") {
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("application/octet-stream"),
        );
    } else if url_path.ends_with(".vue") {
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
    } else {
        *res.status_mut() = hyper::StatusCode::NOT_FOUND;
    }
    // cq_add_log_w(&format!("{:?}",res));
    Ok(res)
}
/// 处理ws协议
async fn serve_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<()> {
    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, mut _read_half) = futures_util::StreamExt::split(ws_stream);

    while let Some(msg) = rx.recv().await {
        // 当所有tx被释放时,tx.recv会返回None
        // log::info!("recive:{}",msg);
        write_half
            .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
            .await?;
    }
    Ok(())
}

/// 处理ws协议
async fn serve_py_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
) -> Result<()> {
    cq_add_log("connect to pyserver").unwrap();

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half) = futures_util::StreamExt::split(ws_stream);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // 当所有tx被释放时,tx.recv会返回None
            // log::info!("recive:{}",msg);
            let rst = write_half
                .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
                .await;
            if rst.is_err() {
                runtime_context().session.clear_python_handler().await;
                cq_add_log_w(&format!(
                    "serve send1 of python err:{:?}",
                    rst.err().unwrap()
                ))
                .unwrap();
                break;
            }
        }
        cq_add_log_w("serve send2 of python err").unwrap();
    });

    async fn deal_py_msg(
        mut read_half: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>,
        >,
    ) -> Result<()> {
        while let Some(msg_t) = read_half.next().await {
            {
                if !runtime_context().session.has_python_handler().await {
                    break;
                }
            }
            let msg_text = msg_t?.to_text()?.to_owned();

            if msg_text == "opened" {
                cq_add_log("python环境已经连接！").unwrap();
                continue;
            }

            let js: serde_json::Value = serde_json::from_str(&msg_text)?;
            let echo = read_json_str(&js, "echo");
            let data = js
                .get("data")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new()));
            runtime_context()
                .session
                .send_python_echo(&echo, data)
                .await?;
        }
        Ok(())
    }
    let ret = deal_py_msg(read_half).await;
    if ret.is_err() {
        cq_add_log_w(&format!(
            "serve recv of python err:{:?}",
            ret.err().unwrap()
        ))
        .unwrap();
    }
    runtime_context().session.clear_python_handler().await;

    Ok(())
}

pub async fn send_onebot_event(root: serde_json::Value) {
    let rst = event_to_onebot(&root);
    if rst.is_err() {
        cq_add_log_w(&format!(
            "convert to onebot event err:{:?}",
            rst.err().unwrap()
        ))
        .unwrap();
        return;
    }
    let (root, platform, self_id) = rst.unwrap();
    runtime_context()
        .session
        .send_onebot_event(root.to_string(), &platform, &self_id)
        .await;
}

pub async fn send_onebot_api_ret(root: serde_json::Value, platform: String, self_id: String) {
    runtime_context()
        .session
        .send_onebot_event(root.to_string(), &platform, &self_id)
        .await;
}

/// 处理ws协议
async fn serve_onebot_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    mut rx: tokio::sync::mpsc::Receiver<String>,
    platform: String,
    self_id: String,
) -> Result<()> {
    cq_add_log("connect to onebot").unwrap();

    // 获得升级后的ws流
    let ws_stream = websocket.await?;
    let (mut write_half, read_half) = futures_util::StreamExt::split(ws_stream);

    // 发送生命周期事件
    let tm = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let connect_event = serde_json::json!({
        "time":tm,
        "self_id":self_id,
        "post_type":"meta_event",
        "meta_event_type":"lifecycle",
        "sub_type":"connect"
    })
    .to_string();

    write_half
        .send(hyper_tungstenite::tungstenite::Message::Text(
            connect_event.into(),
        ))
        .await?;

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // 当所有tx被释放时,tx.recv会返回None
            // log::info!("recive:{}",msg);
            let rst = write_half
                .send(hyper_tungstenite::tungstenite::Message::Text(msg.into()))
                .await;
            if rst.is_err() {
                cq_add_log_w("serve send1 of onebot err").unwrap();
                break;
            }
        }
        cq_add_log_w("serve send2 of onebot err").unwrap();
    });

    async fn deal_msg(
        mut read_half: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>,
        >,
        platform: String,
        self_id: String,
    ) -> Result<()> {
        while let Some(msg_t) = read_half.next().await {
            // 转json
            let msg_text = msg_t?.to_text()?.to_owned();
            let req_json_rst = serde_json::from_str(&msg_text);
            if req_json_rst.is_err() {
                cq_add_log_w(&format!("req json err:{:?}", req_json_rst.err().unwrap())).unwrap();
                continue;
            }
            // 转red请求
            let req_json = req_json_rst.unwrap();
            let root_rst = crate::onebot11s::request_to_red(&req_json);
            if root_rst.is_err() {
                cq_add_log_w(&format!(
                    "can't convert to red req:{:?}",
                    root_rst.err().unwrap()
                ))
                .unwrap();
                continue;
            }
            let (root, passive_id) = root_rst.unwrap();

            let self_id_t = self_id.clone();
            let platform_t = platform.clone();

            tokio::spawn(async move {
                let self_id = self_id_t.clone();
                let platform = platform_t.clone();

                let root_str = root.to_string();

                let echo = root.get("echo").cloned();
                let ret = tokio::task::spawn_blocking(move || {
                    let ret = cq_call_api(
                        &platform.clone(),
                        &self_id.clone(),
                        &passive_id,
                        &root_str,
                        "",
                    );
                    let ret_red_json: serde_json::Value = serde_json::from_str(&ret).unwrap();
                    crate::onebot11s::red_ret_to_ob(ret_red_json, echo)
                })
                .await;
                match ret {
                    Ok(response) => {
                        send_onebot_api_ret(response, platform_t, self_id_t).await;
                    }
                    Err(err) => {
                        cq_add_log_w(&format!("err:{err:?}")).unwrap();
                    }
                }
            });
        }
        Ok(())
    }
    let ret = deal_msg(read_half, platform, self_id).await;
    if ret.is_err() {
        cq_add_log_w(&format!(
            "serve recv of onebot err:{:?}",
            ret.err().unwrap()
        ))
        .unwrap();
    }

    Ok(())
}

async fn http_auth(request: &hyper::Request<Incoming>) -> Result<i32> {
    let web_pass_raw =
        tokio::task::spawn_blocking(move || -> Result<String> { read_web_password() }).await??;

    if web_pass_raw.is_empty() {
        return Ok(2);
    }
    let headers = request.headers();
    let cookie_str = headers
        .get("Cookie")
        .ok_or("can not found Cookie")?
        .to_str()?;
    {
        let web_pass: String =
            url::form_urlencoded::byte_serialize(web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}", web_pass)) {
            return Ok(2);
        }
    }
    let read_only_web_pass_raw =
        tokio::task::spawn_blocking(move || -> Result<String> { read_readonly_web_password() })
            .await??;
    if read_only_web_pass_raw.is_empty() {
        return Ok(1);
    }
    {
        let web_pass: String =
            url::form_urlencoded::byte_serialize(read_only_web_pass_raw.as_bytes()).collect();
        if cookie_str.contains(&format!("password={}", web_pass)) {
            return Ok(1);
        }
    }
    Err("password invaild".into())
}

fn is_users_api(url_path: &str) -> bool {
    if url_path.starts_with("/user") {
        return true;
    }
    false
}

fn rout_to_login() -> hyper::Response<BoxBody> {
    let mut res = hyper::Response::new(full(vec![]));
    *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
    res.headers_mut()
        .insert("Location", HeaderValue::from_static("/login.html"));
    res
}

pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    http_body_util::Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

async fn read_json_body<T: DeserializeOwned>(
    request: hyper::Request<Incoming>,
    limit: usize,
) -> Result<T> {
    if request
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > limit)
    {
        return Err("请求内容过大".into());
    }

    let mut body = request.into_body();
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame?;
        if let Some(chunk) = frame.data_ref() {
            if data.len() + chunk.len() > limit {
                return Err("请求内容过大".into());
            }
            data.extend_from_slice(chunk);
        }
    }
    Ok(serde_json::from_slice(&data)?)
}

pub(crate) fn json_api_response(value: serde_json::Value) -> hyper::Response<BoxBody> {
    let mut response = hyper::Response::new(full(value.to_string()));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

pub(crate) fn json_api_error(message: &str) -> hyper::Response<BoxBody> {
    json_api_response(json!({
        "retcode": -1,
        "data": {"message": message}
    }))
}

fn method_not_allowed() -> hyper::Response<BoxBody> {
    let mut response = json_api_error("请求方法不支持");
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    response
}

async fn connect_handle(
    request: hyper::Request<hyper::body::Incoming>,
    is_local: bool,
) -> Result<Response<BoxBody>> {
    let url_path = request.uri().path();
    // cq_add_log_w(&format!("url:{url_path}")).unwrap();
    // 登录页面不进行身份验证
    if url_path == "/login.html" {
        return deal_file(request).await;
    }

    // 登录API不进行身份验证
    if url_path == "/login" {
        return deal_api(request, false, false).await;
    }

    // 心跳接口无须身份验证
    if url_path == "/heartbeat" {
        runtime_context().services.set_auto_close(false);

        let res = hyper::Response::new(full("ok"));
        return Ok(res);
    }

    // onebot 接口走专有身份验证
    if url_path.starts_with("/onebot") {
        if hyper_tungstenite::is_upgrade_request(&request) {
            let mut access_token = String::new();
            let params = crate::httpevent::get_params_from_uri(request.uri());
            if params.contains_key("access_token") {
                let ac = params.get("access_token").unwrap();
                access_token = ac.to_owned();
            } else if request.headers().contains_key("Authorization") {
                let au = request.headers().get("Authorization").unwrap();
                access_token = au.to_str()?.to_owned();
            }

            let web_pass_raw = read_web_password()?;

            // 需要鉴权
            if !web_pass_raw.is_empty() && !is_local {
                if access_token.contains(&web_pass_raw) {
                    // 鉴权通过
                } else {
                    // 鉴权不通过
                    return Ok(hyper::Response::new(full("broken http")));
                }
            }

            let parts = url_path.split("/").collect::<Vec<&str>>();
            let platform = parts.get(2).map(ToString::to_string).unwrap_or_default();
            let self_id = parts.get(3).map(ToString::to_string).unwrap_or_default();

            let (response, websocket) = hyper_tungstenite::upgrade(request, None)?;
            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);
                let ws_uid = uuid::Uuid::new_v4().to_string();
                {
                    runtime_context()
                        .session
                        .add_onebot_session(
                            ws_uid.to_owned(),
                            tx,
                            platform.clone(),
                            self_id.clone(),
                        )
                        .await;
                }
                if let Err(e) = serve_onebot_websocket(websocket, rx, platform, self_id).await {
                    cq_add_log_w(&format!("onebots Error in websocket connection: {}", e)).unwrap();
                }
                {
                    runtime_context()
                        .session
                        .remove_onebot_session(&ws_uid)
                        .await;
                }
            });

            let headers = response.headers();
            let mut rrr = Response::builder().body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            return Ok(rrr);
        } else {
            return Ok(hyper::Response::new(full("broken http")));
        }
    }

    // 获取身份信息
    let (can_read, can_write) = if !is_local {
        // if !addr.ip().is_loopback() {
        match http_auth(&request).await {
            Ok(auth_code) => (true, auth_code == 2),
            Err(_) => (false, false),
        }
    } else {
        // 本机地址不进行身份验证
        (true, true)
    };

    // 认证失败,且不是用户API,跳转登录页面
    if !can_read && !is_users_api(url_path) {
        return Ok(rout_to_login());
    }
    // 升级ws协议
    if hyper_tungstenite::is_upgrade_request(&request) {
        if url_path == "/watch_log" {
            // 没有写权限不允许访问log
            if !can_write {
                let res = hyper::Response::new(full("api not found"));
                return Ok(res);
            }

            // ws协议升级返回
            let (response, websocket) = hyper_tungstenite::upgrade(request, None)?;
            let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);

            let history_log = get_history_log();
            for it in history_log {
                tx.send(it).await?;
            }

            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let uid = uuid::Uuid::new_v4().to_string();
                runtime_context()
                    .session
                    .add_log_session(uid.clone(), tx)
                    .await;
                if let Err(e) = serve_websocket(websocket, rx).await {
                    cq_add_log_w(&format!("watchlog Error in websocket connection: {}", e))
                        .unwrap();
                }
                // 线程结束，删除对应的entry
                runtime_context().session.remove_log_session(&uid).await;
            });
            let headers = response.headers();
            let mut rrr = Response::builder().body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            Ok(rrr)
        } else if url_path == "/pyserver" {
            // 没有写权限不允许访问pyserver
            if !can_write {
                let res = hyper::Response::new(full("api not found"));
                return Ok(res);
            }
            // ws协议升级返回
            let mut conf = tungstenite::protocol::WebSocketConfig::default();
            conf.max_frame_size = Some(1024 * 1024 * 100); // 100MB

            let (response, websocket) = hyper_tungstenite::upgrade(request, Some(conf))?;

            // 开启一个线程来处理ws
            tokio::spawn(async move {
                let (tx, rx) = tokio::sync::mpsc::channel::<String>(60);
                {
                    runtime_context().session.set_python_handler(tx).await;
                }
                if let Err(e) = serve_py_websocket(websocket, rx).await {
                    cq_add_log_w(&format!("pyserver Error in websocket connection: {}", e))
                        .unwrap();
                }
            });

            let headers = response.headers();
            let mut rrr = Response::builder().body(full("switching to websocket protocol"))?;
            (*rrr.status_mut()) = response.status();
            (*rrr.headers_mut()) = headers.to_owned();
            Ok(rrr)
        } else {
            Ok(hyper::Response::new(full("broken http")))
        }
    } else {
        // 处理HTTP协议
        if url_path == "/" {
            // 没有读权限不允许访问主页
            if !can_read {
                return Ok(rout_to_login());
            }
            let mut res = hyper::Response::new(full(vec![]));
            *res.status_mut() = hyper::StatusCode::MOVED_PERMANENTLY;
            res.headers_mut()
                .insert("Location", HeaderValue::from_static("/index.html"));
            Ok(res)
        } else if url_path == "/favicon.ico" {
            // 没有读权限不允许访问图标
            if !can_read {
                return Ok(rout_to_login());
            }
            let app_dir = cq_get_app_directory1_async().await?;
            let path = PathBuf::from(&app_dir);
            let path = path.join("webui");
            let url_path_t = "favicon.ico".to_owned();
            let file_path = path.join(url_path_t);
            let file_buf = tokio::fs::read(&file_path).await?;
            let mut res = hyper::Response::new(full(file_buf));
            res.headers_mut()
                .insert("Content-Type", HeaderValue::from_static("image/x-icon"));
            Ok(res)
        } else if !url_path.contains(".")
            || url_path.starts_with("/user")
            || url_path.contains("5350b16b-b5e2-425a-bba1-d33d92813ab4")
        {
            deal_api(request, can_write, can_read).await
        } else {
            // 没有读权限不允许访问文件
            if !can_read {
                return Ok(rout_to_login());
            }
            deal_file(request).await
        }
    }
}

#[cfg(target_os = "windows")]
fn bind_port_with_retry(addr: &str) {
    let mut retries = 0;
    loop {
        match std::net::TcpListener::bind(addr) {
            Ok(_listener) => return, // 成功绑定
            Err(e) => {
                if retries > 20 {
                    panic!("端口一直被占用，放弃启动: {}", e);
                }
                cq_add_log_w(&format!(
                    "端口被占用，等待旧进程退出... (尝试 {}/20)",
                    retries + 1
                ))
                .unwrap();
                thread::sleep(std::time::Duration::from_millis(500)); // 等待 0.5 秒
                retries += 1;
            }
        }
    }
}

pub fn init_http_server() -> Result<()> {
    let config = crate::config_manager::startup_config()?;
    let mut host = config.web_host.as_str();
    let port = config.web_port;
    if host == "localhost" {
        host = "127.0.0.1";
    }
    let web_uri = format!("{host}:{port}");
    cq_add_log(&format!("webui访问地址：http://{web_uri}")).unwrap();
    let addr1 = web_uri.parse::<std::net::SocketAddr>().unwrap();

    // 这里等待端口释放
    #[cfg(target_os = "windows")]
    {
        let lock_name = std::env::var("REDREPLY_LOCK_NAME").unwrap_or_else(|_| "".to_string());
        if !lock_name.is_empty() {
            cq_add_log("重启中....").unwrap();
            bind_port_with_retry(&web_uri);
        }
    }

    runtime().spawn(async move {
        let socket = if addr1.is_ipv4() {
            tokio::net::TcpSocket::new_v4().unwrap()
        } else {
            tokio::net::TcpSocket::new_v6().unwrap()
        };

        // win 下不需要设置这个
        #[cfg(not(windows))]
        socket.set_reuseaddr(true).unwrap();

        socket.bind(addr1).unwrap();
        let listener = socket.listen(20).unwrap();

        loop {
            let (stream, remote_address) = match listener.accept().await {
                Ok(ok) => ok,
                Err(err) => {
                    cq_add_log_w(&format!("HTTP accept 失败:{err}")).unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            let io = hyper_util::rt::TokioIo::new(stream);

            let service =
                service_fn(move |req| connect_handle(req, remote_address.ip().is_loopback()));

            tokio::task::spawn(async move {
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await
                {
                    cq_add_log_w(&format!("Error serving connection: {:?}", err)).unwrap();
                }
            });
        }
    });

    if !config.not_open_browser {
        crate::resource_manager::show_ctrl_web()?;
    }

    Ok(())
}
/// 流式对话接口：返回 text/event-stream 的 SSE 响应。
/// 事件：
///   event: reasoning  data: {"delta":"..."}
///   event: content    data: {"delta":"..."}
///   event: tool_call  data: {"slot":0,"name":"...","delta":"..."}
///   event: done       data: {"type":"final"|"tool_calls", ...}
///   event: error      data: {"message":"..."}
const LIVE2D_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(30);

struct Live2dResponseStreamState {
    rx: tokio::sync::mpsc::Receiver<Bytes>,
    cancellation: CancellationToken,
    done_delivered: bool,
}

impl Drop for Live2dResponseStreamState {
    fn drop(&mut self) {
        // 前端断开连接：后台任务通过 CancellationToken 感知并取消 AI 流式调用。
        // 中断落库由前端驱动的 append 协议负责（前端存活时点「停止」落库，
        // 意外刷新则无法落库，界面恢复时按尾消息判定为 interrupted_stream）。
        self.cancellation.cancel();
    }
}

async fn deal_live2d_chat_stream(
    request: hyper::Request<hyper::body::Incoming>,
) -> Result<hyper::Response<BoxBody>> {
    let chat_request: Live2dChatRequest =
        match read_json_body(request, LIVE2D_CHAT_BODY_LIMIT).await {
            Ok(chat_request) => chat_request,
            Err(error) => return Ok(json_api_error(&format!("消息格式错误: {error}"))),
        };
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(64);
    let cancellation = CancellationToken::new();
    let agent_cancellation = cancellation.clone();
    runtime().spawn(async move {
        run_live2d_chat_stream(chat_request, tx, agent_cancellation).await;
    });
    let state = Live2dResponseStreamState {
        rx,
        cancellation,
        done_delivered: false,
    };
    let stream = futures_util::stream::unfold(state, |mut state| async move {
        match state.rx.recv().await {
            Some(bytes) => {
                if bytes.starts_with(b"event: done\n") {
                    state.done_delivered = true;
                }
                Some((
                    Ok::<_, GenericError>(hyper::body::Frame::data(bytes)),
                    state,
                ))
            }
            None => None,
        }
    });
    let body: BoxBody = http_body_util::BodyExt::boxed(http_body_util::StreamBody::new(stream));
    let mut res = hyper::Response::new(body);
    res.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    res.headers_mut().insert(
        hyper::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    Ok(res)
}

/// 发送一条 SSE error 事件。
fn live2d_stream_send_error(tx: &tokio::sync::mpsc::Sender<Bytes>, text: String) {
    let _ = tx.try_send(live2d_sse_event("error", &json!({ "message": text })));
}

/// 处理前端落库请求（POST /live2d_chat_append）。
/// 对话持久化完全由前端驱动，保证「用户看到的」与「数据库里的」一致。
async fn deal_live2d_chat_append(append_request: Live2dAppendRequest) -> std::result::Result<(), String> {
    let generation = append_request
        .generation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 100);
    let mut save_generation: Option<&str> = None;
    match append_request.op.as_str() {
        "append" => {
            let message = append_request
                .message
                .ok_or_else(|| "追加消息缺少 message".to_owned())?;
            validate_live2d_conversation_message(&message)?;
            append_live2d_agent_message(message).await?;
            save_generation = generation;
            Ok(())
        }
        "replace_last" => {
            let message = append_request
                .message
                .ok_or_else(|| "替换消息缺少 message".to_owned())?;
            validate_live2d_conversation_message(&message)?;
            replace_last_live2d_agent_message(message).await?;
            save_generation = generation;
            Ok(())
        }
        "replace_all" => {
            let messages = append_request
                .messages
                .ok_or_else(|| "整体替换缺少 messages".to_owned())?;
            for message in &messages {
                validate_live2d_conversation_message(message)?;
            }
            replace_all_live2d_agent_messages(messages).await?;
            save_generation = generation;
            Ok(())
        }
        "compress_summary" => {
            let reply = append_request
                .reply
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "压缩请求缺少有效的回复内容".to_owned())?;
            let summary = format!(
                "{}{}",
                crate::ai_client::COMPRESSED_SUMMARY_PREFIX,
                reply
            );
            let estimated_tokens = crate::ai_client::estimate_context_tokens(&[serde_json::json!({
                "role": "system",
                "content": summary.clone()
            })]);
            let compressed_messages = vec![serde_json::json!({
                "role": "system",
                "content": summary,
                "timestamp": crate::ai_client::now_timestamp(),
                "context_tokens": estimated_tokens,
                "context_tokens_estimated": true
            })];
            replace_all_live2d_agent_messages(compressed_messages).await?;
            save_generation = generation;
            Ok(())
        }
        "clear" => {
            clear_live2d_agent_conversation().await?;
            Ok(())
        }
        other => Err(format!("未知的落库操作: {other}")),
    }?;
    if append_request.clear_generation {
        save_live2d_agent_generation(None).await?;
    } else if save_generation.is_some() {
        save_live2d_agent_generation(save_generation.map(str::to_owned)).await?;
    }
    Ok(())
}

/// 返回看板娘对话状态，供前端刷新后恢复界面。
async fn deal_live2d_chat_state() -> Result<hyper::Response<BoxBody>> {
    let messages = get_live2d_agent_conversation().await?.unwrap_or_default();
    let phase = live2d_agent_phase(&messages);
    let generation = get_live2d_agent_generation().await?;
    let pending_calls = live2d_agent_pending_calls(&messages);
    let (context_used, context_used_estimated) = live2d_agent_context_usage().await;
    let config = crate::config_manager::live2d_chat_config()
        .map_err(|error| format!("读取对话设置失败: {error}"))?;
    let payload = json!({
        "retcode": 0,
        "data": {
            "phase": phase,
            "messages": messages,
            "generation": generation,
            "pending_calls": pending_calls,
            "context_used": context_used,
            "context_used_estimated": context_used_estimated,
            "context_length": config.context_length
        }
    });
    let mut res = hyper::Response::new(full(payload.to_string()));
    res.headers_mut()
        .insert("Content-Type", HeaderValue::from_static("application/json"));
    Ok(res)
}

/// 一次看板娘 Agent 调用（两种入口：新提问 / 继续工具调用）。
enum Live2dAgentCall {
    Ask {
        messages: Vec<serde_json::Value>,
        message: String,
    },
    ContinueAgent {
        messages: Vec<serde_json::Value>,
    },
}

/// 一次 Agent 调用的流程上下文：已完成步数 + 全局流程锁。
struct Live2dAgentContext {
    completed_steps: u64,
    _flow_guard: tokio::sync::MutexGuard<'static, ()>,
}

/// 校验并准备一次看板娘 Agent 调用（流式接口专用）。
/// 单用户场景：不维护会话表，每次从数据库读取对话消息；
/// 以「是否携带 generation」区分继续工具调用与初始提问。
/// （工具结果已由前端通过 append 协议落库，这里直接读取完整对话）
async fn prepare_live2d_agent_call(
    config: &crate::config_manager::Live2dChatConfig,
    chat_request: Live2dChatRequest,
    flow_guard: tokio::sync::MutexGuard<'static, ()>,
) -> std::result::Result<(Live2dAgentCall, Live2dAgentContext), String> {
    let Live2dChatRequest {
        message,
        generation,
        compress: _compress,
    } = chat_request;

    // 继续工具调用：携带 generation 提交，从数据库恢复待执行的调用 ID
    if generation.is_some() {
        if message.is_some() {
            return Err("继续工具调用时不能同时携带消息".to_owned());
        }
        let generation = generation
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 100)
            .ok_or_else(|| "继续工具调用时缺少有效的对话代次".to_owned())?;
        let messages = get_live2d_agent_conversation()
            .await?
            .ok_or_else(|| "没有待继续的对话，请重新提问".to_owned())?;
        let pending_call_ids = live2d_agent_pending_call_ids(&messages)
            .ok_or_else(|| "没有待执行的工具调用，请重新提问".to_owned())?;
        let stored_generation = get_live2d_agent_generation()
            .await?
            .ok_or_else(|| "待执行工具调用已经失效，请重新提问".to_owned())?;
        if stored_generation != generation {
            return Err("工具结果所属对话已失效，请刷新后重新提问".to_owned());
        }
        // 工具结果消息已由前端落库，这里仅需确认已全部就绪
        if !pending_call_ids.is_empty() {
            return Err("仍有工具结果未提交，请完成工具执行后再继续".to_owned());
        }
        let completed_steps = live2d_agent_completed_steps(&messages);
        return Ok((
            Live2dAgentCall::ContinueAgent { messages },
            Live2dAgentContext {
                completed_steps,
                _flow_guard: flow_guard,
            },
        ));
    }

    // 初始提问
    let message = message
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .ok_or_else(|| "消息不能为空".to_owned())?
        .to_owned();
    if config.base_url.trim().is_empty() || config.api_key.trim().is_empty() {
        return Err("看板娘对话尚未配置，请先设置接口地址和 API Key".to_owned());
    }
    let empty_ctx = Live2dAgentContext {
        completed_steps: 0,
        _flow_guard: flow_guard,
    };
    let messages = get_live2d_agent_conversation().await?.unwrap_or_default();
    if live2d_agent_pending_call_ids(&messages).is_some_and(|pending| !pending.is_empty()) {
        return Err("上一轮工具仍在执行，请完成或放弃执行后重新提问".to_owned());
    }
    Ok((Live2dAgentCall::Ask { messages, message }, empty_ctx))
}

/// 调用一次流式 Agent 调用，并把推理/回答增量实时转发为 SSE 事件；
/// 返回完整的 AgentStep，供调用方发送 done 事件。
async fn run_live2d_stream_agent(
    config: &crate::config_manager::Live2dChatConfig,
    call: Live2dAgentCall,
    diaries: &[crate::resource_manager::Live2dDiaryEntry],
    tx: &tokio::sync::mpsc::Sender<Bytes>,
    cancellation: &CancellationToken,
) -> std::result::Result<crate::ai_client::AgentStep, String> {
    let (step_tx, mut step_rx) = tokio::sync::mpsc::channel::<crate::ai_client::StreamEvent>(64);
    let forward_tx = tx.clone();
    let forward_cancellation = cancellation.clone();
    let forwarder = runtime().spawn(async move {
        while let Some(event) = step_rx.recv().await {
            let (name, data) = match event {
                crate::ai_client::StreamEvent::Reasoning(delta) => {
                    ("reasoning", json!({ "delta": delta }))
                }
                crate::ai_client::StreamEvent::Content(delta) => {
                    ("content", json!({ "delta": delta }))
                }
                crate::ai_client::StreamEvent::ContextEstimate(tokens) => (
                    "context_estimate",
                    json!({
                        "context_tokens": tokens,
                        "context_tokens_estimated": true
                    }),
                ),
                crate::ai_client::StreamEvent::ToolCallDelta {
                    slot,
                    name,
                    arguments,
                } => (
                    "tool_call",
                    json!({
                        "slot": slot,
                        "name": name,
                        "delta": arguments
                    }),
                ),
            };
            if forward_cancellation.is_cancelled() {
                return Err("客户端已断开，对话已取消".to_owned());
            }
            let send_result = tokio::select! {
                biased;
                _ = forward_cancellation.cancelled() => {
                    return Err("客户端已断开，对话已取消".to_owned());
                }
                result = tokio::time::timeout(
                    LIVE2D_STREAM_SEND_TIMEOUT,
                    forward_tx.send(live2d_sse_event(name, &data)),
                ) => result,
            };
            match send_result {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    forward_cancellation.cancel();
                    return Err("客户端已断开，对话已取消".to_owned());
                }
                Err(_) => {
                    forward_cancellation.cancel();
                    return Err("向客户端发送流式内容超时，对话已取消".to_owned());
                }
            }
        }
        Ok::<(), String>(())
    });
    let result = match call {
        Live2dAgentCall::Ask { messages, message } => {
            crate::ai_client::stream_ask(config, &messages, &message, diaries, step_tx, cancellation)
                .await
        }
        Live2dAgentCall::ContinueAgent { messages, .. } => {
            crate::ai_client::stream_continue_agent(config, &messages, step_tx, cancellation).await
        }
    };
    let forward_result = forwarder
        .await
        .map_err(|error| format!("流式转发任务失败: {error}"))?;
    forward_result?;
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消".to_owned());
    }
    result.map_err(|error| error.to_string())
}

/// 把 AgentStep 转成 SSE 的 done 事件。落库由前端基于 done 数据中的
/// assistant_message 发起（见前端 runAgent 的落库协议）。
async fn emit_live2d_stream_step(
    step: crate::ai_client::AgentStep,
    completed_steps: u64,
    context_length: u64,
    tx: &tokio::sync::mpsc::Sender<Bytes>,
    cancellation: &CancellationToken,
) -> std::result::Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消".to_owned());
    }
    let data = live2d_agent_step_data(step, completed_steps, context_length).await?;
    if cancellation.is_cancelled() {
        return Err("客户端已断开，对话已取消".to_owned());
    }
    let send_result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            return Err("客户端已断开，对话已取消".to_owned());
        }
        result = tokio::time::timeout(
            LIVE2D_STREAM_SEND_TIMEOUT,
            tx.send(live2d_sse_event("done", &data)),
        ) => result,
    };
    match send_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err("客户端已断开，对话已取消".to_owned()),
        Err(_) => Err("向客户端发送完成事件超时".to_owned()),
    }
}

/// 在后台执行一次流式 Agent 流程（初始对话或继续工具调用），
/// 把增量事件与最终结果通过 `tx` 以 SSE 形式发送。
async fn run_live2d_chat_stream(
    chat_request: Live2dChatRequest,
    tx: tokio::sync::mpsc::Sender<Bytes>,
    cancellation: CancellationToken,
) {
    let flow_guard = live2d_agent_flow_lock().lock().await;
    if cancellation.is_cancelled() {
        return;
    }
    let config = match crate::config_manager::live2d_chat_config() {
        Ok(config) => config,
        Err(error) => {
            live2d_stream_send_error(&tx, format!("读取对话设置失败: {error}"));
            return;
        }
    };
    let diaries = tokio::task::spawn_blocking(crate::resource_manager::list_live2d_diaries)
        .await
        .ok()
        .and_then(|result| result.ok())
        .unwrap_or_default();
    let (call, ctx) = match prepare_live2d_agent_call(&config, chat_request, flow_guard).await {
        Ok(prepared) => prepared,
        Err(error) => {
            live2d_stream_send_error(&tx, error);
            return;
        }
    };
    let step = match run_live2d_stream_agent(&config, call, &diaries, &tx, &cancellation).await {
        Ok(step) => step,
        Err(error) => {
            cq_add_log_w(&format!("Live2D对话失败: {error}")).unwrap();
            live2d_stream_send_error(&tx, error);
            return;
        }
    };
    if let Err(error) = emit_live2d_stream_step(step, ctx.completed_steps, config.context_length, &tx, &cancellation)
        .await
    {
        cq_add_log_w(&format!("Live2D完成事件发送失败: {error}")).unwrap();
    }
}
