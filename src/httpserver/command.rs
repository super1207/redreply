//! 服务端命令行 REST 接口：POST /run_command

use std::collections::{HashMap, HashSet};
use std::process::{Child, Command, Output};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::http::HeaderValue;
use hyper::{Method, Request, Response};
use serde::Deserialize;
use serde_json::json;

use super::{full, BoxBody, Result};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default, Deserialize)]
struct CommandRequest {
    #[serde(default)]
    content: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    shell: bool,
    #[serde(default)]
    encoding: String,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    command_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

struct CommandGeneration {
    pids: HashSet<u32>,
    cancel_flags: Vec<Arc<AtomicBool>>,
    cancelled_at: Option<Instant>,
}

static COMMAND_GENERATIONS: OnceLock<Mutex<HashMap<String, CommandGeneration>>> = OnceLock::new();
const CANCELLED_GENERATION_TTL: Duration = Duration::from_secs(300);

fn command_generations() -> &'static Mutex<HashMap<String, CommandGeneration>> {
    COMMAND_GENERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_command_generations(generations: &mut HashMap<String, CommandGeneration>) {
    generations.retain(|_, state| {
        !state.pids.is_empty()
            || !state.cancel_flags.is_empty()
            || state
                .cancelled_at
                .is_some_and(|cancelled_at| cancelled_at.elapsed() < CANCELLED_GENERATION_TTL)
    });
}

fn register_command(tokens: &[String], pid: u32) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let mut generations = command_generations().lock().unwrap();
    prune_command_generations(&mut generations);
    if tokens.iter().any(|token| {
        generations
            .get(token)
            .is_some_and(|state| state.cancelled_at.is_some())
    }) {
        return false;
    }
    for token in tokens {
        generations
            .entry(token.clone())
            .or_insert_with(|| CommandGeneration {
                pids: HashSet::new(),
                cancel_flags: Vec::new(),
                cancelled_at: None,
            })
            .pids
            .insert(pid);
    }
    true
}

fn register_c_program(tokens: &[String]) -> Option<Arc<AtomicBool>> {
    let cancel = Arc::new(AtomicBool::new(false));
    if tokens.is_empty() {
        return Some(cancel);
    }
    let mut generations = command_generations().lock().unwrap();
    prune_command_generations(&mut generations);
    if tokens.iter().any(|token| {
        generations
            .get(token)
            .is_some_and(|state| state.cancelled_at.is_some())
    }) {
        return None;
    }
    for token in tokens {
        generations
            .entry(token.clone())
            .or_insert_with(|| CommandGeneration {
                pids: HashSet::new(),
                cancel_flags: Vec::new(),
                cancelled_at: None,
            })
            .cancel_flags
            .push(cancel.clone());
    }
    Some(cancel)
}

fn unregister_c_program(tokens: &[String], cancel: &Arc<AtomicBool>) {
    let mut generations = command_generations().lock().unwrap();
    for token in tokens {
        let remove = if let Some(state) = generations.get_mut(token) {
            state.cancel_flags.retain(|item| !Arc::ptr_eq(item, cancel));
            state.pids.is_empty()
                && state.cancel_flags.is_empty()
                && state.cancelled_at.is_none()
        } else {
            false
        };
        if remove {
            generations.remove(token);
        }
    }
}

fn unregister_command(tokens: &[String], pid: u32) {
    let mut generations = command_generations().lock().unwrap();
    for token in tokens {
        let remove = if let Some(state) = generations.get_mut(token) {
            state.pids.remove(&pid);
            state.pids.is_empty() && state.cancelled_at.is_none()
        } else {
            false
        };
        if remove {
            generations.remove(token);
        }
    }
}

fn command_is_active(generation: &str, pid: u32) -> bool {
    command_generations()
        .lock()
        .unwrap()
        .get(generation)
        .is_some_and(|state| state.pids.contains(&pid))
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) -> std::result::Result<(), String> {
    use std::os::windows::process::CommandExt;

    let status = Command::new("taskkill")
        .creation_flags(0x08000000)
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("无法终止命令行进程 {pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("终止命令行进程 {pid} 失败: {status}"))
    }
}

#[cfg(not(windows))]
fn kill_process_tree(pid: u32) -> std::result::Result<(), String> {
    let status = Command::new("kill")
        .args(["-KILL", &format!("-{pid}")])
        .status()
        .map_err(|error| format!("无法终止命令行进程 {pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("终止命令行进程 {pid} 失败: {status}"))
    }
}

pub async fn cancel_generation(generation: &str) -> std::result::Result<(), String> {
    let generation = generation.trim();
    if generation.is_empty() || generation.len() > 100 {
        return Err("取消请求缺少有效的对话代次".to_owned());
    }
    let pids = {
        let mut generations = command_generations().lock().unwrap();
        prune_command_generations(&mut generations);
        let state = generations
            .entry(generation.to_owned())
            .or_insert_with(|| CommandGeneration {
                pids: HashSet::new(),
                cancel_flags: Vec::new(),
                cancelled_at: None,
            });
        state.cancelled_at = Some(Instant::now());
        for cancel in &state.cancel_flags {
            cancel.store(true, Ordering::Relaxed);
        }
        state.pids.iter().copied().collect::<Vec<_>>()
    };
    let generation = generation.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut failures = Vec::new();
        for pid in pids {
            if let Err(error) = kill_process_tree(pid) {
                if command_is_active(&generation, pid) {
                    failures.push(error);
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    })
    .await
    .map_err(|error| format!("停止命令行任务失败: {error}"))?
}

pub async fn run_c(
    source_code: String,
    generation: &str,
    command_id: &str,
) -> std::result::Result<serde_json::Value, String> {
    if source_code.trim().is_empty() {
        return Err("C 源代码不能为空".to_owned());
    }
    if source_code.chars().count() > 1_000_000 {
        return Err("C 源代码过大，最多支持 1000000 个字符".to_owned());
    }
    if generation.len() > 100 || command_id.len() > 100 {
        return Err("命令取消标识过长".to_owned());
    }
    let mut tokens = Vec::new();
    for token in [generation.trim(), command_id.trim()] {
        if !token.is_empty() && !tokens.iter().any(|item| item == token) {
            tokens.push(token.to_owned());
        }
    }
    let cancel = register_c_program(&tokens).ok_or_else(|| "C 程序执行已取消".to_owned())?;
    let task_cancel = cancel.clone();
    let result = tokio::task::spawn_blocking(move || {
        c89_runner::run_c89_with_cancel(source_code, task_cancel)
    })
    .await
    .map_err(|error| format!("C 程序任务失败: {error}"));
    unregister_c_program(&tokens, &cancel);
    let output = result?;
    Ok(json!({
        "stdout": output,
        "stderr": "",
        "exit_code": 0
    }))
}

fn wait_for_command(child: Child, tokens: &[String]) -> std::result::Result<Output, String> {
    let pid = child.id();
    if !register_command(tokens, pid) {
        let _ = kill_process_tree(pid);
        let _ = child.wait_with_output();
        return Err("命令执行已取消".to_owned());
    }
    let registration = scopeguard::guard((tokens.to_owned(), pid), |(tokens, pid)| {
        unregister_command(&tokens, pid);
    });
    let output = child
        .wait_with_output()
        .map_err(|error| format!("命令执行失败: {error}"));
    drop(registration);
    output
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

/// Match Node.js child_process output decoding: UTF-8 by default, with an
/// explicit GBK compatibility option for existing Windows scripts.
fn decode_windows_output(bytes: &[u8], encoding: &str, what: &str) -> std::result::Result<String, String> {
    use encoding::Encoding;

    if encoding.is_empty() || encoding == "utf8" || encoding == "utf-8" {
        return Ok(String::from_utf8_lossy(bytes).into_owned());
    }
    if encoding == "gbk" {
        return encoding::all::GBK
            .decode(bytes, encoding::DecoderTrap::Ignore)
            .map_err(|error| format!("GBK 解码{what}失败: {error}"));
    }
    if encoding == "buffer" {
        return Err("encoding=buffer 暂不支持，请使用 utf8 或 gbk".to_owned());
    }
    if encoding != "auto" {
        return Err(format!("不支持的编码: {encoding}"));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(_) => encoding::all::GBK
            .decode(bytes, encoding::DecoderTrap::Ignore)
            .map_err(|error| format!("GBK 解码{what}失败: {error}")),
    }
}

fn request_command(request: &CommandRequest) -> std::result::Result<String, String> {
    let kind = if request.kind.is_empty() {
        "exec"
    } else {
        request.kind.as_str()
    };
    match kind {
        "exec" => {
            let command = if request.command.is_empty() {
                request.content.as_str()
            } else {
                request.command.as_str()
            };
            if command.trim().is_empty() {
                Err("缺少 command 内容（请放在 content 字段）".into())
            } else {
                Ok(command.to_owned())
            }
        }
        "execFile" => {
            if request.program.trim().is_empty() {
                return Err("缺少 program 内容".to_owned());
            }
            Ok(request.program.to_owned())
        }
        _ => Err(format!("不支持的命令类型: {kind}")),
    }
}

fn configure_command(command: &mut Command, request: &CommandRequest) {
    if let Some(cwd) = request.cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
        command.current_dir(cwd);
    }
    if let Some(env) = request.env.as_ref() {
        command.env_clear().envs(env);
    }
}

fn run_command(request: &CommandRequest) -> std::result::Result<serde_json::Value, String> {
    let command_text = request_command(request)?;
    let encoding = request.encoding.trim().to_lowercase();
    let mut command_tokens = Vec::new();
    for token in [request.generation.trim(), request.command_id.trim()] {
        if !token.is_empty() && !command_tokens.iter().any(|item| item == token) {
            command_tokens.push(token.to_owned());
        }
    }

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    #[cfg(windows)]
    let output: Output = {
        let mut child_command = if request.kind == "execFile" && !request.shell {
            let mut command = Command::new(&command_text);
            command.args(&request.args);
            command
        } else {
            let shell_command = if request.kind == "execFile" {
                std::iter::once(command_text.as_str())
                    .chain(request.args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command_text.clone()
            };
            let mut command = Command::new("cmd");
            command.args(["/d", "/s", "/c", &shell_command]);
            command
        };
        configure_command(&mut child_command, request);
        let child = child_command
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("命令执行失败: {error}"))?;
        wait_for_command(child, &command_tokens)?
    };

    #[cfg(not(windows))]
    let output = {
        use std::os::unix::process::CommandExt;

        let mut child_command = if request.kind == "execFile" && !request.shell {
            let mut command = Command::new(&command_text);
            command.args(&request.args);
            command
        } else {
            let shell_command = if request.kind == "execFile" {
                std::iter::once(command_text.as_str())
                    .chain(request.args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                command_text.clone()
            };
            let mut command = Command::new("sh");
            command.args(["-c", &shell_command]);
            command
        };
        configure_command(&mut child_command, request);
        let child = child_command
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("命令执行失败: {error}"))?;
        wait_for_command(child, &command_tokens)?
    };

    let stdout = decode_output(&output.stdout, &encoding, "stdout")?;
    let stderr = decode_output(&output.stderr, &encoding, "stderr")?;
    Ok(json!({
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": output.status.code()
    }))
}

fn decode_output(bytes: &[u8], encoding: &str, what: &str) -> std::result::Result<String, String> {
    if cfg!(target_os = "windows") {
        decode_windows_output(bytes, encoding, what)
    } else if encoding == "gbk" {
        use encoding::Encoding;
        encoding::all::GBK
            .decode(bytes, encoding::DecoderTrap::Ignore)
            .map_err(|error| format!("GBK 解码{what}失败: {error}"))
    } else if encoding == "buffer" {
        Err("encoding=buffer 暂不支持，请使用 utf8 或 gbk".to_owned())
    } else if encoding.is_empty() || encoding == "utf8" || encoding == "utf-8" || encoding == "auto" {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    } else {
        Err(format!("不支持的编码: {encoding}"))
    }
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
    if request.generation.len() > 100 || request.command_id.len() > 100 {
        return Ok(err_response("命令取消标识过长"));
    }
    let result = tokio::task::spawn_blocking(move || run_command(&request)).await;
    match result {
        Err(error) => Ok(err_response(&format!("命令行任务失败: {error}"))),
        Ok(Ok(data)) => Ok(ok_response(data)),
        Ok(Err(error)) => Ok(err_response(&error)),
    }
}

#[derive(Deserialize)]
struct RunCRequest {
    #[serde(default)]
    source_code: String,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    command_id: String,
}

pub async fn handle_run_c(request: Request<Incoming>) -> Result<Response<BoxBody>> {
    if request.method() != Method::POST {
        let mut response = err_response("请求方法不支持");
        *response.status_mut() = hyper::StatusCode::METHOD_NOT_ALLOWED;
        return Ok(response);
    }
    let body = match read_body(request.into_body()).await {
        Ok(body) => body,
        Err(error) => return Ok(err_response(&format!("请求体过大: {error}"))),
    };
    let request: RunCRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return Ok(err_response(&format!("请求格式错误: {error}"))),
    };
    match run_c(
        request.source_code,
        &request.generation,
        &request.command_id,
    )
    .await
    {
        Ok(data) => Ok(ok_response(data)),
        Err(error) => Ok(err_response(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_generation, command_generations, register_c_program, run_c, run_command,
        unregister_c_program, CommandRequest,
    };
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn exec_returns_stdout_stderr_and_exit_code_separately() {
        let request = CommandRequest {
            kind: "exec".to_owned(),
            #[cfg(windows)]
            command: "echo stdout & echo stderr 1>&2".to_owned(),
            #[cfg(not(windows))]
            command: "printf stdout; printf stderr >&2".to_owned(),
            encoding: "utf8".to_owned(),
            ..CommandRequest::default()
        };
        let result = run_command(&request).unwrap();
        assert!(result["stdout"].as_str().unwrap().contains("stdout"));
        assert!(result["stderr"].as_str().unwrap().contains("stderr"));
        assert_eq!(result["exit_code"], 0);
    }

    #[test]
    fn exec_reports_nonzero_exit_code_without_merging_output() {
        let request = CommandRequest {
            kind: "exec".to_owned(),
            #[cfg(windows)]
            command: "echo failed 1>&2 & exit /b 7".to_owned(),
            #[cfg(not(windows))]
            command: "printf failed >&2; exit 7".to_owned(),
            encoding: "utf8".to_owned(),
            ..CommandRequest::default()
        };
        let result = run_command(&request).unwrap();
        assert_eq!(result["stdout"], "");
        assert!(result["stderr"].as_str().unwrap().contains("failed"));
        assert_eq!(result["exit_code"], 7);
    }

    #[test]
    fn exec_file_passes_arguments_without_shell_joining() {
        let request = CommandRequest {
            kind: "execFile".to_owned(),
            #[cfg(windows)]
            program: "cmd.exe".to_owned(),
            #[cfg(windows)]
            args: vec![
                "/d".to_owned(),
                "/s".to_owned(),
                "/c".to_owned(),
                "echo argument with spaces".to_owned(),
            ],
            #[cfg(not(windows))]
            program: "/usr/bin/printf".to_owned(),
            #[cfg(not(windows))]
            args: vec!["%s".to_owned(), "argument with spaces".to_owned()],
            encoding: "utf8".to_owned(),
            ..CommandRequest::default()
        };
        let result = run_command(&request).unwrap();
        assert!(result["stdout"]
            .as_str()
            .unwrap()
            .contains("argument with spaces"));
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn run_c_compiles_and_executes_on_riscv() {
        let result = run_c(
            "#include <stdio.h>\nint main(void) { printf(\"run_c ok\\n\"); return 0; }"
                .to_owned(),
            "test-run-c",
            "test-run-c-command",
        )
        .await
        .unwrap();
        assert_eq!(result["stdout"], "run_c ok\n");
        assert_eq!(result["stderr"], "");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_generation_stops_run_c() {
        let command_id = format!("run-c-command-{}", uuid::Uuid::new_v4());
        let tokens = vec![command_id.clone()];
        let cancel = register_c_program(&tokens).unwrap();
        let task_cancel = cancel.clone();
        let task = tokio::task::spawn_blocking(move || {
            c89_runner::run_c89_with_cancel(
                "int main(void) { for (;;) {} return 0; }".to_owned(),
                task_cancel,
            )
        });
        cancel_generation(&command_id).await.unwrap();
        let output = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("取消后 run_c 没有及时退出")
            .unwrap();
        unregister_c_program(&tokens, &cancel);
        assert!(output.contains("运行已取消"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_generation_terminates_running_command() {
        let generation = format!("test-{}", uuid::Uuid::new_v4());
        let command_id = format!("command-{}", uuid::Uuid::new_v4());
        let request = CommandRequest {
            #[cfg(windows)]
            content: "ping 127.0.0.1 -n 31 >nul".to_owned(),
            #[cfg(not(windows))]
            content: "sleep 30".to_owned(),
            generation: generation.clone(),
            command_id: command_id.clone(),
            ..CommandRequest::default()
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_command(&request));
        });

        let registration_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let registered = command_generations()
                .lock()
                .unwrap()
                .get(&command_id)
                .is_some_and(|state| !state.pids.is_empty());
            if registered {
                break;
            }
            assert!(
                Instant::now() < registration_deadline,
                "命令未在预期时间内登记"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        cancel_generation(&command_id).await.unwrap();
        let _ = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("取消后命令没有及时退出");
    }
}
