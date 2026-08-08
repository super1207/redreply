//! 服务端命令行 REST 接口：POST /run_command

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::{Method, Request, Response};
use serde::Deserialize;
use serde_json::json;

use super::{full, BoxBody, Result};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct CommandRequest {
    #[serde(default)]
    content: String,
    #[serde(default)]
    encoding: String,
}

fn ok_response(data: serde_json::Value) -> Response<BoxBody> {
    let mut response = Response::new(full(json!({"retcode": 0, "data": data}).to_string()));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn err_response(message: &str) -> Response<BoxBody> {
    let mut response = Response::new(full(
        json!({"retcode": -1, "data": {"message": message}}).to_string(),
    ));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

async fn read_body(mut body: Incoming) -> Result<Bytes> {
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame?;
        if let Some(chunk) = frame.data_ref() {
            if data.len() + chunk.len() > MAX_BODY_BYTES {
                return Err("请求体过大".into());
            }
            data.extend_from_slice(chunk);
        }
    }
    Ok(Bytes::from(data))
}

fn run_command(request: &CommandRequest) -> std::result::Result<serde_json::Value, String> {
    use encoding::Encoding;

    let command = request.content.trim();
    if command.is_empty() {
        return Err("缺少 command 内容（请放在 content 字段）".into());
    }
    let encoding = request.encoding.trim().to_lowercase();

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    #[cfg(windows)]
    let output: std::process::Output = {
        let tmp_dir = crate::get_tmp_dir().map_err(|error| format!("获取临时目录失败: {error}"))?;
        let tmp_file = format!("{}.cmd", uuid::Uuid::new_v4());
        let tmp_path = std::path::Path::new(&tmp_dir).join(&tmp_file);
        let mut gbk_cmd = b"@echo off\r\n".to_vec();
        let encoded =
            encoding::Encoding::encode(encoding::all::GBK, command, encoding::EncoderTrap::Ignore)
                .map_err(|error| format!("GBK 编码命令失败: {error}"))?;
        gbk_cmd.extend(encoded);
        std::fs::write(&tmp_path, &gbk_cmd)
            .map_err(|error| format!("写入临时命令文件失败: {error}"))?;
        let _guard = scopeguard::guard(tmp_path.clone(), |path| {
            let _ = std::fs::remove_file(path);
        });
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .current_dir(current_dir)
            .arg("/c")
            .arg(&tmp_path)
            .output()
            .map_err(|error| format!("命令执行失败: {error}"))?
    };

    #[cfg(not(windows))]
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|error| format!("命令执行失败: {error}"))?;

    let stdout = if cfg!(target_os = "windows") {
        if encoding == "utf8" || encoding == "utf-8" {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else if encoding.is_empty() || encoding == "gbk" {
            encoding::all::GBK
                .decode(&output.stdout, encoding::DecoderTrap::Ignore)
                .map_err(|error| format!("GBK 解码输出失败: {error}"))?
        } else {
            return Err(format!("不支持的编码: {encoding}"));
        }
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let stderr = if cfg!(target_os = "windows") {
        encoding::all::GBK
            .decode(&output.stderr, encoding::DecoderTrap::Ignore)
            .map_err(|error| format!("GBK 解码 stderr 失败: {error}"))?
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    Ok(json!({"output": format!("{stdout}{stderr}")}))
}

pub async fn handle(request: Request<Incoming>) -> Result<Response<BoxBody>> {
    if request.method() != Method::POST {
        let mut response = err_response("请求方法不支持");
        *response.status_mut() = hyper::StatusCode::METHOD_NOT_ALLOWED;
        return Ok(response);
    }
    let body = match read_body(request.into_body()).await {
        Ok(body) => body,
        Err(error) => return Ok(err_response(&format!("请求体过大: {error}"))),
    };
    let request: CommandRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return Ok(err_response(&format!("请求格式错误: {error}"))),
    };
    match run_command(&request) {
        Ok(data) => Ok(ok_response(data)),
        Err(error) => Ok(err_response(&error)),
    }
}
