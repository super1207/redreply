//! 服务端文件系统与命令行 REST 接口：POST /fs
//! 请求体 JSON：{"op": "list|read|write|append|delete|mkdir|rmtree|exists|is_dir|is_file|size|stat|walk|cmd",
//!              "path": "...", "content": "...", "content_base64": "...", "encoding": "gbk|utf-8"}
//! 成功返回 {"retcode": 0, "data": <结果>}；失败返回 {"retcode": -1, "data": {"message": "..."}}

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::{Method, Request, Response};
use serde::Deserialize;
use serde_json::json;

use super::{full, BoxBody, Result};

/// 请求体上限（64MB，与 /proxy 对齐，足够放下 write 内容与命令）
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// fs_walk 条目上限，防止死循环撑爆内存
const WALK_LIMIT: usize = 20_000;

#[derive(Deserialize)]
struct FsRequest {
    op: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    content_base64: String,
    #[serde(default)]
    encoding: String,
}

fn ok_response(data: serde_json::Value) -> Response<BoxBody> {
    let mut resp = Response::new(full(json!({"retcode": 0, "data": data}).to_string()));
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp
}

fn err_response(message: &str) -> Response<BoxBody> {
    let mut resp = Response::new(
        full(json!({"retcode": -1, "data": {"message": message}}).to_string()),
    );
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp
}

async fn read_body(mut body: Incoming, limit: usize) -> Result<Bytes> {
    let mut data = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame?;
        if let Some(chunk) = frame.data_ref() {
            if data.len() + chunk.len() > limit {
                return Err("请求体过大".into());
            }
            data.extend_from_slice(chunk);
        }
    }
    Ok(Bytes::from(data))
}

fn path_param(req: &FsRequest) -> std::result::Result<&str, String> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err("缺少 path 参数".into());
    }
    Ok(path)
}

/// op=list：列出目录，返回 [{name, is_dir}]
fn op_list(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let entries = std::fs::read_dir(path).map_err(|e| format!("fs_list 失败: {e}"))?;
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("fs_list 失败: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        items.push(json!({"name": name, "is_dir": is_dir}));
    }
    Ok(serde_json::Value::Array(items))
}

/// op=read：读文件；文本返回 {"text"}，二进制返回 {"base64", "encoding":"base64"}
fn op_read(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let bytes = std::fs::read(path).map_err(|e| format!("fs_read 失败: {e}"))?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(json!({"text": text})),
        Err(error) => {
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, error.as_bytes());
            Ok(json!({"base64": b64, "encoding": "base64"}))
        }
    }
}

fn body_bytes(req: &FsRequest) -> std::result::Result<Vec<u8>, String> {
    if !req.content_base64.trim().is_empty() {
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            req.content_base64.trim(),
        )
        .map_err(|e| format!("content_base64 解码失败: {e}"))
    } else {
        Ok(req.content.as_bytes().to_vec())
    }
}

/// op=write：写入文件（content 或 content_base64）
fn op_write(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let bytes = body_bytes(req)?;
    std::fs::write(path, &bytes).map_err(|e| format!("fs_write 失败: {e}"))?;
    Ok(json!(true))
}

/// op=append：追加写入
fn op_append(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let bytes = body_bytes(req)?;
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&bytes)
        });
    result.map_err(|e| format!("fs_append 失败: {e}"))?;
    Ok(json!(true))
}

/// op=delete：删除文件或空目录
fn op_delete(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let result = match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::remove_dir(path),
    };
    result.map_err(|e| format!("fs_delete 失败: {e}"))?;
    Ok(json!(true))
}

/// op=mkdir：创建目录（含父目录）
fn op_mkdir(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    std::fs::create_dir_all(path).map_err(|e| format!("fs_mkdir 失败: {e}"))?;
    Ok(json!(true))
}

/// op=rmtree：递归删除目录
fn op_rmtree(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    std::fs::remove_dir_all(path).map_err(|e| format!("fs_rmtree 失败: {e}"))?;
    Ok(json!(true))
}

fn op_exists(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    Ok(json!(std::path::Path::new(path_param(req)?).exists()))
}

fn op_is_dir(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    Ok(json!(std::path::Path::new(path_param(req)?).is_dir()))
}

fn op_is_file(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    Ok(json!(std::path::Path::new(path_param(req)?).is_file()))
}

fn op_size(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let metadata = std::fs::metadata(path).map_err(|e| format!("fs_size 失败: {e}"))?;
    if !metadata.is_file() {
        return Err("fs_size: 目标不是文件".into());
    }
    Ok(json!(metadata.len()))
}

/// op=stat：返回 {name, is_dir, is_file, size, modified_secs?}
fn op_stat(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let path = path_param(req)?;
    let metadata = std::fs::metadata(path).map_err(|e| format!("fs_stat 失败: {e}"))?;
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let mut value = json!({
        "name": name,
        "is_dir": metadata.is_dir(),
        "is_file": metadata.is_file(),
        "size": metadata.len(),
    });
    if let Some(secs) = modified_secs {
        value["modified_secs"] = json!(secs);
    }
    Ok(value)
}

/// op=walk：递归遍历，返回 [{path, is_dir}]（相对路径，20000 条上限）
fn op_walk(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    let root = path_param(req)?;
    let mut items = Vec::new();
    let mut stack = vec![(root.to_owned(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("fs_walk 失败: {e}"))?;
        let mut subdirs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("fs_walk 失败: {e}"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel_path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            items.push(json!({"path": rel_path, "is_dir": is_dir}));
            if items.len() > WALK_LIMIT {
                return Err(format!("fs_walk: 条目超过 {WALK_LIMIT} 上限"));
            }
            if is_dir {
                subdirs.push((entry.path().to_string_lossy().into_owned(), rel_path));
            }
        }
        for subdir in subdirs {
            stack.push(subdir);
        }
    }
    Ok(serde_json::Value::Array(items))
}

/// op=cmd：执行系统命令（Windows: cmd /c 写临时 GBK .cmd 文件执行，不弹黑框；Linux/macOS: sh -c）
fn op_cmd(req: &FsRequest) -> std::result::Result<serde_json::Value, String> {
    use encoding::Encoding;
    let command = req.content.trim();
    if command.is_empty() {
        return Err("cmd: 缺少 command 内容（请放在 content 字段）".into());
    }
    let encoding = req.encoding.trim().to_lowercase();

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    #[cfg(windows)]
    let output: std::process::Output = {
        let tmp_dir = crate::get_tmp_dir().map_err(|e| format!("获取临时目录失败: {e}"))?;
        let tmp_file = format!("{}.cmd", uuid::Uuid::new_v4());
        let tmp_path = std::path::Path::new(&tmp_dir).join(&tmp_file);
        let mut gbk_cmd = b"@echo off\r\n".to_vec();
        let encoded = encoding::Encoding::encode(encoding::all::GBK, command, encoding::EncoderTrap::Ignore)
            .map_err(|e| format!("GBK 编码命令失败: {e}"))?;
        gbk_cmd.extend(encoded);
        std::fs::write(&tmp_path, &gbk_cmd).map_err(|e| format!("写入临时命令文件失败: {e}"))?;
        let _guard = scopeguard::guard(tmp_path.clone(), |path| {
            let _ = std::fs::remove_file(path);
        });
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        match std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .current_dir(current_dir)
            .arg("/c")
            .arg(&tmp_path)
            .output()
        {
            Ok(output) => output,
            Err(e) => return Err(format!("命令执行失败: {e}")),
        }
    };

    #[cfg(not(windows))]
    let output: std::process::Output = {
        match std::process::Command::new("sh").arg("-c").arg(command).output() {
            Ok(output) => output,
            Err(e) => return Err(format!("命令执行失败: {e}")),
        }
    };

    // 解码输出（encoding 仅 Windows 生效，默认 gbk）
    let stdout_str = if cfg!(target_os = "windows") {
        if encoding == "utf8" || encoding == "utf-8" {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else if encoding.is_empty() || encoding == "gbk" {
            match encoding::all::GBK.decode(&output.stdout, encoding::DecoderTrap::Ignore) {
                Ok(text) => text,
                Err(e) => return Err(format!("GBK 解码输出失败: {e}")),
            }
        } else {
            return Err(format!("不支持的编码: {encoding}"));
        }
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let stderr_str = if cfg!(target_os = "windows") {
        match encoding::all::GBK.decode(&output.stderr, encoding::DecoderTrap::Ignore) {
            Ok(text) => text,
            Err(e) => return Err(format!("GBK 解码 stderr 失败: {e}")),
        }
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    Ok(json!({"output": format!("{stdout_str}{stderr_str}")}))
}

/// /fs 主处理：按 op 分发
pub async fn handle(request: Request<Incoming>) -> Result<Response<BoxBody>> {
    if request.method() != Method::POST {
        let mut resp = err_response("请求方法不支持");
        *resp.status_mut() = hyper::StatusCode::METHOD_NOT_ALLOWED;
        return Ok(resp);
    }
    let body = match read_body(request.into_body(), MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(e) => return Ok(err_response(&format!("请求体过大: {e}"))),
    };
    let req: FsRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => return Ok(err_response(&format!("请求格式错误: {e}"))),
    };
    let result = match req.op.as_str() {
        "list" => op_list(&req),
        "read" => op_read(&req),
        "write" => op_write(&req),
        "append" => op_append(&req),
        "delete" => op_delete(&req),
        "mkdir" => op_mkdir(&req),
        "rmtree" => op_rmtree(&req),
        "exists" => op_exists(&req),
        "is_dir" => op_is_dir(&req),
        "is_file" => op_is_file(&req),
        "size" => op_size(&req),
        "stat" => op_stat(&req),
        "walk" => op_walk(&req),
        "cmd" => op_cmd(&req),
        other => return Ok(err_response(&format!("不支持的 op: {other}"))),
    };
    match result {
        Ok(data) => Ok(ok_response(data)),
        Err(message) => Ok(err_response(&message)),
    }
}
