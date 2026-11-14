use super::*;

pub(crate) async fn execute_tool_call(config: &ChatConfig, call: &ToolCall) -> String {
    if call.name != COMMAND_TOOL_NAME {
        return format!("未知工具：{}", call.name);
    }
    let Some(command_text) = command_from_arguments(&call.arguments) else {
        return "参数解析失败：需要 JSON 对象 {\"command\": \"...\"}".to_string();
    };
    log_info(format!(
        "ChatGPT 运行命令行（超时 {} 秒）：{}",
        config.command_timeout_secs,
        command_text.chars().take(120).collect::<String>()
    ));
    let timeout_secs = config.command_timeout_secs;
    let max_output = config.command_max_output;
    match redlang::runtime()
        .spawn_blocking(move || run_command(&command_text, timeout_secs, max_output))
        .await
    {
        Ok(result) => result,
        Err(error) => format!("执行线程失败：{error}"),
    }
}

pub(crate) fn command_from_arguments(arguments: &str) -> Option<String> {
    let value: Value = serde_json::from_str(arguments).ok()?;
    let command = value.get("command").and_then(Value::as_str)?;
    if command.trim().is_empty() {
        return None;
    }
    Some(command.to_owned())
}

pub(crate) fn shell_command(text: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/D", "/S", "/C", text]);
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c").arg(text);
        command
    }
}

pub(crate) fn bot_api_url() -> Option<String> {
    let addr = redlang::framework()?.http_addr()?;
    if addr.port() == 0 {
        return None;
    }
    let host = match addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    Some(format!("http://{host}:{}{BOT_API_PATH}", addr.port()))
}

/// 同步执行命令行（在 spawn_blocking 中调用），返回给模型的文本结果。
pub(crate) fn run_command(text: &str, timeout_secs: u64, max_output: usize) -> String {
    let mut command = shell_command(text);
    if let Some(url) = bot_api_url() {
        command.env("REDREPLY_API_URL", url);
    }
    match run_command_process(
        command,
        Duration::from_secs(timeout_secs.max(1)),
        max_output,
    ) {
        Ok((status, stdout, stderr, timed_out)) => format_command_result(
            status,
            timed_out,
            timeout_secs,
            &stdout,
            &stderr,
            max_output,
        ),
        Err(error) => format!("启动命令行失败：{error}"),
    }
}

/// (退出状态, stdout, stderr, 是否超时)
pub(crate) type CommandOutput = (Option<std::process::ExitStatus>, Vec<u8>, Vec<u8>, bool);

pub(crate) fn run_command_process(
    mut command: Command,
    timeout: Duration,
    max_output: usize,
) -> std::io::Result<CommandOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (line_tx, line_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let stdout_handle = stdout.map(|pipe| {
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            let mut reader = std::io::BufReader::new(pipe);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if line_tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
    });
    let stderr_handle = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut timed_out = false;
    let mut stdout_bytes = Vec::new();
    loop {
        drain_command_lines(&line_rx, &mut stdout_bytes, max_output);
        if let Some(current) = child.try_wait()? {
            status = Some(current);
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_tree(&mut child);
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }
    drain_command_lines(&line_rx, &mut stdout_bytes, max_output);
    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok((status, stdout_bytes, stderr, timed_out))
}

pub(crate) fn drain_command_lines(
    lines: &std::sync::mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    max_output: usize,
) {
    while let Ok(line) = lines.try_recv() {
        // 粗略限制累积量，避免失控输出吃内存；最终仍按字符数截断。
        if output.len() < max_output.saturating_mul(8).max(64 * 1024) {
            output.extend_from_slice(&line);
        }
    }
}

#[cfg(windows)]
pub(crate) fn kill_process_tree(child: &mut std::process::Child) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let pid = child.id().to_string();
    let _ = Command::new("taskkill")
        .args(["/PID", pid.as_str(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(windows))]
pub(crate) fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    let _ = Command::new("kill")
        .args(["-9", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) fn format_command_result(
    status: Option<std::process::ExitStatus>,
    timed_out: bool,
    timeout_secs: u64,
    stdout: &[u8],
    stderr: &[u8],
    max_output: usize,
) -> String {
    let mut out = String::new();
    if timed_out {
        out.push_str(&format!("执行超时（{timeout_secs} 秒），进程已终止\n"));
    } else if let Some(status) = status {
        out.push_str(&format!("退出码：{}\n", status.code().unwrap_or(-1)));
    }
    out.push_str("stdout:\n");
    out.push_str(decode_command_output(stdout).trim_end());
    out.push('\n');
    out.push_str("stderr:\n");
    out.push_str(decode_command_output(stderr).trim_end());
    truncate_chars(&mut out, max_output);
    out
}

pub(crate) fn decode_command_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    #[cfg(windows)]
    {
        let (text, _, _) = encoding_rs::GBK.decode(bytes);
        text.into_owned()
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

pub(crate) fn truncate_chars(text: &mut String, max_chars: usize) {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return;
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text.truncate(cut);
    text.push_str("\n…（输出已截断）");
}
