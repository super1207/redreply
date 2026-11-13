use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, watch, Mutex};
use tokio::time::{self, MissedTickBehavior};

use super::KookConnect;
use crate::{native_api, AdapterResult};

const VOICE_API_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Default)]
pub(super) struct Playback {
    active: Option<ActivePlayback>,
    last: Option<Value>,
}

#[derive(Debug)]
struct ActivePlayback {
    channel_id: String,
    cancel: Option<oneshot::Sender<()>>,
    done: watch::Receiver<Option<Value>>,
}

#[derive(Debug)]
struct VoiceEndpoint {
    target: String,
    bitrate: u32,
    ssrc: u32,
    payload_type: u8,
}

impl VoiceEndpoint {
    fn from_response(data: &Value) -> AdapterResult<Self> {
        fn number(data: &Value, key: &str) -> AdapterResult<u64> {
            data[key]
                .as_u64()
                .or_else(|| data[key].as_str().and_then(|value| value.parse().ok()))
                .ok_or_else(|| format!("KOOK 语音接口缺少有效的 {key}").into())
        }
        let address = data["ip"]
            .as_str()
            .ok_or("KOOK 语音接口未返回 IP")?
            .parse::<IpAddr>()?;
        let port = u16::try_from(number(data, "port")?)?;
        let rtcp_port = match data["rtcp_mux"].as_bool() {
            Some(true) => port,
            Some(false) => u16::try_from(number(data, "rtcp_port")?)?,
            None => return Err("KOOK 语音接口缺少 rtcp_mux".into()),
        };
        let bitrate = u32::try_from(number(data, "bitrate")?)?;
        let ssrc = u32::try_from(number(data, "audio_ssrc")?)?;
        let payload_type = u8::try_from(number(data, "audio_pt")?)?;
        if port == 0
            || rtcp_port == 0
            || !(6_000..=510_000).contains(&bitrate)
            || payload_type > 127
        {
            return Err("KOOK 返回的语音推流参数超出范围".into());
        }
        Ok(Self {
            target: format!(
                "rtp://{}?rtcpport={rtcp_port}",
                SocketAddr::new(address, port)
            ),
            bitrate,
            ssrc,
            payload_type,
        })
    }
}

fn source_protocols(source: &str) -> AdapterResult<&'static str> {
    if source.starts_with("https://") || source.starts_with("http://") {
        let url = reqwest::Url::parse(source)?;
        if url.host_str().is_some() && url.username().is_empty() && url.password().is_none() {
            return Ok("http,https,tcp,tls,crypto");
        }
        return Err("KOOK 播放 URL 必须有主机名且不能包含账号密码".into());
    }
    if Path::new(source).is_absolute() && Path::new(source).is_file() {
        return Ok("file,crypto");
    }
    Err("KOOK 播放音源必须为 HTTP(S) 地址或现有的本地绝对文件路径".into())
}

fn ffmpeg_command(source: &str, protocols: &str, volume: u64, endpoint: &VoiceEndpoint) -> Command {
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            protocols,
        ]);
    if protocols.starts_with("http") {
        command.args(["-rw_timeout", "15000000"]);
    }
    command
        .args([
            "-re", "-i", source, "-map", "0:a:0", "-vn", "-c:a", "libopus",
        ])
        .args([
            "-b:a",
            &endpoint.bitrate.to_string(),
            "-vbr",
            "off",
            "-ac",
            "2",
            "-ar",
            "48000",
        ])
        .args(["-af", &format!("volume={:.2}", volume as f64 / 100.0)])
        .args([
            "-f",
            "rtp",
            "-ssrc",
            &(endpoint.ssrc as i32).to_string(),
            "-payload_type",
            &endpoint.payload_type.to_string(),
            &endpoint.target,
        ]);
    command
}

fn required_text<'a>(params: &'a Value, key: &str) -> AdapterResult<&'a str> {
    params[key]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("KOOK 语音播放缺少 {key}").into())
}

async fn voice_request(
    bot: &KookConnect,
    action: &str,
    data: &Value,
    echo: &Value,
) -> AdapterResult<Value> {
    bot.native_request(
        "POST",
        &format!("voice/{action}"),
        data,
        None,
        VOICE_API_TIMEOUT,
        echo,
    )
    .await
}

async fn voice_call(bot: &KookConnect, action: &str, data: &Value) -> AdapterResult<Value> {
    let result = voice_request(bot, action, data, &Value::Null).await?;
    if result["retcode"] != 0 {
        return Err(format!(
            "KOOK voice/{action}：{} {}",
            result["retcode"], result["message"]
        )
        .into());
    }
    Ok(result["data"].clone())
}

pub(super) async fn native_voice_guard<'a>(
    playback: &'a Mutex<Playback>,
    endpoint: &str,
) -> AdapterResult<Option<tokio::sync::MutexGuard<'a, Playback>>> {
    let endpoint = endpoint.strip_prefix("/api/v3/").unwrap_or(endpoint);
    if !matches!(endpoint, "voice/join" | "voice/leave") {
        return Ok(None);
    }
    let state = playback.lock().await;
    if state.active.is_some() {
        return Err("KOOK 正在共享播放，请先调用【KOOK停止播放】".into());
    }
    Ok(Some(state))
}

pub(super) async fn play(bot: &KookConnect, params: &Value, echo: &Value) -> AdapterResult<Value> {
    let channel_id = required_text(params, "channel_id")?.to_owned();
    let source = required_text(params, "source")?;
    let protocols = source_protocols(source)?;
    let volume = params
        .get("volume")
        .map_or(Some(100), Value::as_u64)
        .filter(|value| *value <= 100)
        .ok_or("KOOK 音量必须是 0 到 100 的整数")?;
    let mut join = json!({"channel_id": channel_id});
    if let Some(password) = params.get("password") {
        join["password"] = password
            .as_str()
            .ok_or("KOOK 语音频道密码必须是文本")?
            .into();
    }
    let mut state = bot.playback.lock().await;
    if bot.is_stop.load(Ordering::Relaxed) {
        return Err("KOOK 连接已断开".into());
    }
    if state.active.is_some() {
        return Err("KOOK 正在共享播放，请先停止当前音频".into());
    }
    let response = voice_request(bot, "join", &join, echo).await?;
    if response["retcode"] != 0 {
        return Ok(response);
    }
    let data = &response["data"];
    let child = VoiceEndpoint::from_response(data).and_then(|endpoint| {
        ffmpeg_command(source, protocols, volume, &endpoint)
            .spawn()
            .map_err(|error| {
                format!("无法启动 FFmpeg（需要 PATH 中含有 libopus 的 ffmpeg）：{error}").into()
            })
    });
    let child = match child {
        Ok(child) => child,
        Err(error) => {
            if let Err(cleanup) = voice_call(bot, "leave", &json!({"channel_id": channel_id})).await
            {
                return Err(format!("{error}；{cleanup}").into());
            }
            return Err(error);
        }
    };
    let (cancel, cancelled) = oneshot::channel();
    let (done_tx, done) = watch::channel(None);
    state.active = Some(ActivePlayback {
        channel_id: channel_id.clone(),
        cancel: Some(cancel),
        done,
    });
    state.last = None;
    let worker = bot.clone();
    let worker_channel = channel_id.clone();
    tokio::spawn(async move {
        run(worker, worker_channel, child, cancelled, done_tx).await;
    });
    Ok(native_api::reply(
        0,
        &json!({"channel_id": channel_id, "state": "playing"}),
        "",
        echo,
    ))
}

async fn run(
    bot: KookConnect,
    channel_id: String,
    mut child: Child,
    mut cancelled: oneshot::Receiver<()>,
    done: watch::Sender<Option<Value>>,
) {
    let body = json!({"channel_id": channel_id});
    let mut keepalive = time::interval_at(
        time::Instant::now() + Duration::from_secs(40),
        Duration::from_secs(40),
    );
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut result = loop {
        tokio::select! {
            biased;
            _ = &mut cancelled => break json!({"state":"stopped"}),
            status = child.wait() => break match status {
                Ok(status) if status.success() => json!({"state":"finished"}),
                Ok(status) => json!({"state":"failed", "error": format!("FFmpeg 退出：{status}")}),
                Err(error) => json!({"state":"failed", "error": format!("无法等待 FFmpeg：{error}")}),
            },
            _ = keepalive.tick() => {
                // Let a stop request interrupt an in-flight keep-alive.
                tokio::select! {
                    biased;
                    _ = &mut cancelled => break json!({"state":"stopped"}),
                    result = voice_call(&bot, "keep-alive", &body) => if let Err(error) = result {
                        break json!({"state":"failed", "error": error.to_string()});
                    },
                }
            }
        }
    };
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill().await;
    }
    if let Err(error) = voice_call(&bot, "leave", &body).await {
        result["leave_error"] = error.to_string().into();
    }
    result["channel_id"] = channel_id.into();
    {
        let mut state = bot.playback.lock().await;
        state.active = None;
        state.last = Some(result.clone());
        done.send_replace(Some(result.clone()));
    }
    let event = json!({
        "time": chrono::Utc::now().timestamp(),
        "platform": "kook",
        "self_id": bot.self_id.read().unwrap_or_else(|e| e.into_inner()).clone(),
        "post_type": "notice",
        "notice_type": "kook",
        "sub_type": "voice_playback",
        "kook": result,
    });
    bot.send_to_onebot_client(&event).await;
}

pub(super) async fn stop(playback: &Mutex<Playback>) -> AdapterResult<Value> {
    let mut done = {
        let mut state = playback.lock().await;
        let Some(active) = state.active.as_mut() else {
            return Ok(json!({"state":"idle"}));
        };
        if let Some(cancel) = active.cancel.take() {
            let _ = cancel.send(());
        }
        active.done.clone()
    };
    let result = time::timeout(Duration::from_secs(12), done.wait_for(Option::is_some))
        .await
        .map_err(|_| "KOOK 停止播放超时")?
        .map_err(|_| "KOOK 播放任务已结束")?;
    Ok(result.clone().unwrap_or(Value::Null))
}

pub(super) async fn status(playback: &Mutex<Playback>) -> Value {
    let state = playback.lock().await;
    match &state.active {
        Some(active) => json!({
            "state": if active.cancel.is_some() {"playing"} else {"stopping"},
            "channel_id": active.channel_id,
        }),
        None => state
            .last
            .clone()
            .unwrap_or_else(|| json!({"state":"idle"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_endpoint_controls_rtp_and_rtcp_parameters() {
        let mut data = json!({
            "ip":"2001:db8::1", "port":"5004", "rtcp_mux":false, "rtcp_port":"5006",
            "bitrate":48000, "audio_ssrc":"4294967295", "audio_pt":"111"
        });
        let endpoint = VoiceEndpoint::from_response(&data).unwrap();
        assert_eq!(endpoint.target, "rtp://[2001:db8::1]:5004?rtcpport=5006");
        assert_eq!(endpoint.ssrc, u32::MAX);
        data["rtcp_mux"] = true.into();
        assert_eq!(
            VoiceEndpoint::from_response(&data).unwrap().target,
            "rtp://[2001:db8::1]:5004?rtcpport=5004"
        );
        data["audio_pt"] = 128.into();
        assert!(VoiceEndpoint::from_response(&data).is_err());
    }

    #[test]
    fn playback_source_limits_ffmpeg_input_protocols() {
        assert_eq!(
            source_protocols("https://example.org/song.mp3").unwrap(),
            "http,https,tcp,tls,crypto"
        );
        assert!(source_protocols("https://a:b@example.org/song.mp3").is_err());
        assert!(source_protocols("concat:https://example.org/a|https://example.org/b").is_err());
        assert!(source_protocols("-f").is_err());
    }
}
