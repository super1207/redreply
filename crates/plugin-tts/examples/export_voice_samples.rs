use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use plugin_tts::EdgeTtsService;
use redlang::plugin_ports::{TtsService, TTS_SAMPLE_TEXT};

fn main() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(std::env::args_os().nth(1).ok_or(
        "Usage: cargo run -p plugin-tts --example export_voice_samples -- <new-output-directory>",
    )?);
    let proxy = std::env::var("REDREPLY_TTS_PROXY").unwrap_or_default();
    let service = EdgeTtsService::default();
    let voices = service.voices(&proxy, 30)?;
    let mut locales = BTreeMap::<&str, Vec<_>>::new();
    let mut ids = HashSet::new();
    for voice in &voices {
        if voice.id.is_empty()
            || !ids.insert(voice.id.as_str())
            || !voice
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(format!("Unexpected voice ID: {}", voice.id).into());
        }
        locales.entry(&voice.locale).or_default().push(voice);
    }
    let total = voices.len();
    println!("{total} voices across {} locales", locales.len());

    // A private state directory lets a retry reuse successful syntheses and their transcripts.
    let samples = output.join("samples");
    let state = output.join(".export-state");
    if output.exists() && !state.is_dir() {
        return Err(io::Error::other("Use a new output directory").into());
    }
    fs::create_dir_all(&state)?;
    fs::create_dir_all(&samples)?;

    let next = AtomicUsize::new(0);
    let transcripts = Mutex::new(BTreeMap::new());
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(voice) = voices.get(index) else {
                    break;
                };
                let mp3 = samples.join(format!("{}.mp3", voice.id));
                let text = state.join(&voice.id);
                let result = (|| -> Result<String, String> {
                    if mp3.is_file() && text.is_file() {
                        return fs::read_to_string(&text).map_err(|error| error.to_string());
                    }
                    let mut preview = service.synthesize_sample(&voice.id, &proxy);
                    for delay in [300, 1000, 2500] {
                        if preview.is_ok() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(delay));
                        preview = service.synthesize_sample(&voice.id, &proxy);
                    }
                    let preview = preview?;
                    fs::write(&mp3, preview.audio).map_err(|error| error.to_string())?;
                    fs::write(&text, preview.transcript).map_err(|error| error.to_string())?;
                    Ok(preview.transcript.to_owned())
                })();
                match result {
                    Ok(transcript) => {
                        transcripts
                            .lock()
                            .unwrap()
                            .insert(voice.id.as_str(), transcript);
                    }
                    Err(error) => {
                        eprintln!("{}: {error}", voice.id);
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("{}: {error}", voice.id));
                    }
                }
                if (index + 1) % 25 == 0 {
                    println!("Processed voice {}/{total}", index + 1);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    if !failures.is_empty() {
        return Err(io::Error::other(format!(
            "{} requests failed: {}",
            failures.len(),
            failures.join("; ")
        ))
        .into());
    }
    let transcripts = transcripts.into_inner().unwrap();
    let original = transcripts
        .values()
        .filter(|text| **text == TTS_SAMPLE_TEXT)
        .count();
    let translated = total - original;

    let mut markdown = format!(
        "# 全部在线音色试听\n\n\
         这份目录覆盖导出时 TTS 插件返回的全部 {total} 个在线音色，共 {} 个语言或地区代码。\n\n\
         每条音频均由 RedReply 的 Rust TTS 插件合成。{original} 个音色朗读中文原句「{TTS_SAMPLE_TEXT}」，\
         {translated} 个音色在服务未返回中文音频后，改用对应语言的译文。\n\n\
         展开语言或地区可播放，GitHub 文件预览可点击「MP3」直接打开。\n\n\
         在线目录会变化。在脚本中用 \x60【TTS人声】\x60 查询最新音色，\
         用 \x60【语音@【TTS样本@完整音色ID】】\x60 生成试听。\n\n",
        locales.len(),
    );
    for (locale, voices) in locales {
        writeln!(
            markdown,
            "<details>\n<summary>{locale}（{}）</summary>\n\n| 人声 ID | 性别 | 朗读文本 | 试听 |\n| --- | --- | --- | --- |",
            voices.len()
        )?;
        for voice in voices {
            let id = &voice.id;
            let transcript = &transcripts[id.as_str()];
            let preview = format!("<audio controls preload=\"none\" aria-label=\"{id}\" src=\"tts/voices/samples/{id}.mp3\"></audio> [MP3](samples/{id}.mp3 ':ignore')");
            writeln!(
                markdown,
                "| `{id}` | {} | {transcript} | {preview} |",
                voice.gender
            )?;
        }
        markdown.push_str("\n</details>\n\n");
    }
    markdown.push_str("[返回文字转语音说明](/tts/)\n");
    fs::write(output.join("README.md"), markdown)?;
    fs::remove_dir_all(state)?;
    println!("Exported {total} MP3 files ({original} original, {translated} translated)");
    Ok(())
}
