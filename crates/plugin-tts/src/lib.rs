//! TTS 插件：通过服务注册表提供 Edge 语音合成、实时人声目录和试听。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use msedge_tts::voice::Voice;
use redlang::plugin_ports::{
    TtsOptions, TtsService, TtsServiceHandle, TtsVoice, TtsVoicePreview, TTS_SAMPLE_TEXT,
};
use redreply_bot::{Plugin, PluginContext, PluginMeta, Result};

mod catalog;

/// Edge completed synthesis without an audio frame for the requested text and voice.
const NO_AUDIO_ERROR: &str = "TTS 服务未返回音频，请检查音色是否支持输入文本的语言";

const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_REQUESTS: usize = 4;
// Process-wide so repeatedly disabling/enabling the plugin cannot bypass the cap.
static ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

struct RequestPermit;

impl RequestPermit {
    fn acquire() -> std::result::Result<Self, String> {
        ACTIVE_REQUESTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_REQUESTS).then_some(active + 1)
            })
            .map_err(|_| "TTS 正在处理其他请求，请稍后重试（最多 4 个并发请求）".to_owned())?;
        Ok(Self)
    }
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        ACTIVE_REQUESTS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct VoiceCache {
    proxy: String,
    fetched_at: Instant,
    voices: Arc<Vec<Voice>>,
}

#[derive(Default)]
pub struct EdgeTtsService {
    cache: Mutex<Option<VoiceCache>>,
    stopped: AtomicBool,
}

struct AttemptError {
    message: String,
    retryable: bool,
}

impl AttemptError {
    fn retryable(message: String) -> Self {
        Self {
            message,
            retryable: true,
        }
    }
    fn permanent(message: String) -> Self {
        Self {
            message,
            retryable: false,
        }
    }
}

fn check_deadline(deadline: Instant) -> std::result::Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| "TTS 请求已超过总超时".to_owned())
}

/// The attempt closure creates a fresh connection and fresh audio buffer.
fn retry_until_deadline<T>(
    deadline: Instant,
    retries: u8,
    mut operation: impl FnMut() -> std::result::Result<T, AttemptError>,
) -> std::result::Result<T, String> {
    for attempt in 0..=retries {
        check_deadline(deadline)?;
        match operation() {
            Ok(value) => {
                check_deadline(deadline)?;
                return Ok(value);
            }
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(format!("TTS 请求已超过总超时：{}", error.message));
                }
                if !error.retryable || attempt == retries {
                    return Err(error.message);
                }
                let delay = Duration::from_millis(100 * (u64::from(attempt) + 1));
                std::thread::sleep(delay.min(check_deadline(deadline)?));
            }
        }
    }
    unreachable!("the inclusive attempt range always runs")
}

impl EdgeTtsService {
    fn check_running(&self) -> std::result::Result<(), AttemptError> {
        if self.stopped.load(Ordering::Acquire) {
            Err(AttemptError::permanent("TTS 插件已停用".to_owned()))
        } else {
            Ok(())
        }
    }

    fn load_voices(
        &self,
        proxy: &str,
        deadline: Instant,
        refresh: bool,
    ) -> std::result::Result<(Arc<Vec<Voice>>, bool), AttemptError> {
        self.check_running()?;
        check_deadline(deadline).map_err(AttemptError::permanent)?;
        if !refresh {
            let cache = self
                .cache
                .lock()
                .map_err(|_| AttemptError::permanent("TTS 人声缓存不可用".to_owned()))?;
            if let Some(cache) = cache.as_ref() {
                if cache.proxy == proxy && cache.fetched_at.elapsed() < CACHE_TTL {
                    return Ok((cache.voices.clone(), true));
                }
            }
        }
        // No mutex is held during network I/O or while a request is retried.
        let voices =
            msedge_tts::voice::get_voices_list_with_deadline(proxy, deadline).map_err(|error| {
                AttemptError::retryable(format!(
                    "获取 TTS 人声失败：{}",
                    redact_proxy(error.to_string(), proxy)
                ))
            })?;
        if voices.is_empty() {
            return Err(AttemptError::retryable(
                "TTS 服务返回了空人声目录".to_owned(),
            ));
        }
        self.check_running()?;
        let voices = Arc::new(voices);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AttemptError::permanent("TTS 人声缓存不可用".to_owned()))?;
        *cache = Some(VoiceCache {
            proxy: proxy.to_owned(),
            fetched_at: Instant::now(),
            voices: voices.clone(),
        });
        Ok((voices, false))
    }
}

fn redact_proxy(message: String, proxy: &str) -> String {
    if proxy.is_empty() {
        message
    } else {
        message.replace(proxy, "<proxy>")
    }
}

/// Full IDs and known aliases never fall back to a different voice. Legacy
/// fragments remain accepted only if they identify exactly one live voice.
fn select_voice(voices: &[Voice], player: &str) -> std::result::Result<Voice, String> {
    let player = if player.trim().is_empty() {
        "YunyangNeural"
    } else {
        player.trim()
    };
    let alias_id = catalog::id_for_alias(player);
    let query = alias_id.unwrap_or(player);
    if let Some(voice) = voices.iter().find(|voice| {
        voice
            .short_name
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(query))
            || voice.name.eq_ignore_ascii_case(query)
    }) {
        return Ok(voice.clone());
    }
    // A full ID or a known reference alias must match that exact voice.
    if alias_id.is_some() || query.contains('-') {
        return Err(format!(
            "当前 Edge 人声目录没有 `{query}`；试听样本不代表当前服务仍支持合成"
        ));
    }
    let query = query.to_ascii_lowercase();
    let matches: Vec<_> = voices
        .iter()
        .filter(|voice| {
            voice.name.to_ascii_lowercase().contains(&query)
                || voice
                    .short_name
                    .as_deref()
                    .is_some_and(|id| id.to_ascii_lowercase().contains(&query))
        })
        .collect();
    match matches.as_slice() {
        [voice] => Ok((*voice).clone()),
        [] => Err(format!("当前 Edge 人声目录没有 `{player}`")),
        _ => {
            let ids: Vec<_> = matches
                .iter()
                .take(8)
                .map(|voice| voice.short_name.as_deref().unwrap_or(&voice.name))
                .collect();
            Err(format!(
                "人声 `{player}` 不唯一，请填写完整 ID：{}",
                ids.join(", ")
            ))
        }
    }
}

fn sample_translation(locale: &str) -> Option<&'static str> {
    let language = locale.split('-').next()?;
    let mut language_match = None;
    for (code, text) in include_str!("sample_translations.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
    {
        if code == locale {
            return Some(text);
        }
        if code == language {
            language_match = Some(text);
        }
    }
    language_match
}

impl EdgeTtsService {
    /// Generates a preview with an explicit budget for the original and translated attempts.
    pub fn synthesize_sample_with_options(
        &self,
        voice: &str,
        proxy: &str,
        options: &TtsOptions,
    ) -> std::result::Result<TtsVoicePreview, String> {
        self.synthesize_request(TTS_SAMPLE_TEXT, voice, proxy, options, true)
            .map(|(audio, transcript)| TtsVoicePreview { audio, transcript })
    }

    fn synthesize_request<'a>(
        &self,
        text: &'a str,
        player: &str,
        proxy: &str,
        options: &TtsOptions,
        is_sample: bool,
    ) -> std::result::Result<(Vec<u8>, &'a str), String> {
        options.validate()?;
        if text.trim().is_empty() {
            return Err("TTS 文本不能为空".to_owned());
        }
        let _permit = RequestPermit::acquire()?;
        let deadline = Instant::now() + Duration::from_secs(options.timeout_secs);
        retry_until_deadline(deadline, options.retries, || {
            let (voices, cached) = self.load_voices(proxy, deadline, false)?;
            let voice = match select_voice(&voices, player) {
                Ok(voice) => voice,
                Err(_) if cached => {
                    let (fresh, _) = self.load_voices(proxy, deadline, true)?;
                    select_voice(&fresh, player).map_err(AttemptError::permanent)?
                }
                Err(error) => return Err(AttemptError::permanent(error)),
            };
            let mut config = msedge_tts::tts::SpeechConfig::from(&voice);
            config.rate = options.rate;
            config.pitch = options.pitch;
            config.volume = options.volume;
            config.audio_format = "audio-24khz-48kbitrate-mono-mp3".to_owned();
            self.check_running()?;
            let speak = |phrase: &str| {
                if proxy.is_empty() {
                    msedge_tts::tts::client::connect_with_deadline(deadline)
                        .and_then(|mut client| client.synthesize(phrase, &config))
                } else {
                    msedge_tts::tts::client::connect_proxy_with_deadline(proxy, deadline)
                        .and_then(|mut client| client.synthesize(phrase, &config))
                }
            };
            let mut transcript = text;
            let mut result = speak(transcript);
            if is_sample && matches!(result, Err(msedge_tts::error::Error::EmptyAudio)) {
                if let Some(translated) = voice.locale.as_deref().and_then(sample_translation) {
                    self.check_running()?;
                    transcript = translated;
                    result = speak(transcript);
                }
            }
            let audio = result.map_err(|error| match error {
                msedge_tts::error::Error::EmptyAudio if is_sample => {
                    AttemptError::permanent(NO_AUDIO_ERROR.to_owned())
                }
                msedge_tts::error::Error::EmptyAudio => {
                    AttemptError::retryable(NO_AUDIO_ERROR.to_owned())
                }
                error => AttemptError::retryable(format!(
                    "TTS 合成失败：{}",
                    redact_proxy(error.to_string(), proxy)
                )),
            })?;
            self.check_running()?;
            Ok((audio.audio_bytes, transcript))
        })
    }
}

impl TtsService for EdgeTtsService {
    fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.synthesize_with_options(text, player, proxy, &TtsOptions::default())
    }

    fn synthesize_with_options(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
        options: &TtsOptions,
    ) -> std::result::Result<Vec<u8>, String> {
        self.synthesize_request(text, player, proxy, options, false)
            .map(|(audio, _)| audio)
    }

    fn synthesize_sample(
        &self,
        voice: &str,
        proxy: &str,
    ) -> std::result::Result<TtsVoicePreview, String> {
        self.synthesize_sample_with_options(voice, proxy, &TtsOptions::default())
    }

    fn voices(&self, proxy: &str, timeout_secs: u64) -> std::result::Result<Vec<TtsVoice>, String> {
        let options = TtsOptions {
            timeout_secs,
            ..TtsOptions::default()
        };
        options.validate()?;
        let _permit = RequestPermit::acquire()?;
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let voices = retry_until_deadline(deadline, options.retries, || {
            self.load_voices(proxy, deadline, false)
                .map(|(voices, _)| voices)
        })?;
        let mut result: Vec<_> = voices
            .iter()
            .map(|voice| {
                let id = voice
                    .short_name
                    .clone()
                    .unwrap_or_else(|| voice.name.clone());
                let aliases = catalog::reference_for_id(&id)
                    .map(|reference| {
                        reference
                            .aliases
                            .iter()
                            .map(|alias| (*alias).to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                TtsVoice {
                    id,
                    name: voice.name.clone(),
                    locale: voice.locale.clone().unwrap_or_default(),
                    gender: voice.gender.clone().unwrap_or_default(),
                    aliases,
                }
            })
            .collect();
        result.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(result)
    }

    fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }
}

pub struct TtsPlugin;

impl Plugin for TtsPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("tts", env!("CARGO_PKG_VERSION"))
            .description("Edge TTS 语音合成、实时人声查询和试听")
            .provides("tts")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.register_service(
            "tts",
            Arc::new(TtsServiceHandle(Arc::new(EdgeTtsService::default()))),
        )
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info("tts", "TTS 插件已就绪");
        Ok(())
    }

    fn stop(&self, ctx: &PluginContext) -> Result<()> {
        if let Some(service) = ctx.services().get::<TtsServiceHandle>("tts") {
            service.shutdown();
        }
        // Existing calls own their handles and finish within their I/O deadline;
        // plugin disable never joins network workers while holding framework locks.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(id: &str) -> Voice {
        Voice {
            name: format!("Microsoft ({id})"),
            short_name: Some(id.to_owned()),
            gender: None,
            locale: None,
            suggested_codec: None,
            friendly_name: None,
            status: None,
            voice_tag: None,
        }
    }

    #[test]
    fn sample_translations_prefer_locale_and_fall_back_to_language() {
        let portuguese_brazil = sample_translation("pt-BR").unwrap();
        let portuguese_portugal = sample_translation("pt-PT").unwrap();
        assert_ne!(portuguese_brazil, portuguese_portugal);
        assert_eq!(sample_translation("en-AU"), sample_translation("en-US"));
        assert_ne!(
            sample_translation("iu-Cans-CA"),
            sample_translation("iu-Latn-CA")
        );
        assert_eq!(sample_translation("zh-CN"), None);
    }

    #[test]
    fn full_ids_support_voices_outside_the_reference_catalog() {
        let voices = vec![voice("ja-JP-NanamiNeural")];
        assert_eq!(
            select_voice(&voices, "ja-JP-NanamiNeural")
                .unwrap()
                .short_name
                .as_deref(),
            Some("ja-JP-NanamiNeural")
        );
    }

    #[test]
    fn chinese_aliases_preserve_dialect_identity() {
        let voices = vec![
            voice("zh-TW-YunJheNeural"),
            voice("wuu-CN-YunzheNeural"),
            voice("zh-CN-XiaoyiNeural"),
            voice("zh-HK-HiuMaanNeural"),
            voice("zh-TW-HsiaoChenNeural"),
        ];
        for (alias, expected) in [
            ("云哲", "zh-TW-YunJheNeural"),
            ("云哲(吴语)", "wuu-CN-YunzheNeural"),
            ("晓伊", "zh-CN-XiaoyiNeural"),
            ("晓依", "zh-CN-XiaoyiNeural"),
            ("晓曼", "zh-HK-HiuMaanNeural"),
            ("晓蔓", "zh-HK-HiuMaanNeural"),
            ("晓臻", "zh-TW-HsiaoChenNeural"),
        ] {
            assert_eq!(
                select_voice(&voices, alias).unwrap().short_name.as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn voice_aliases_have_unique_ids_and_unambiguous_mappings() {
        use std::collections::{HashMap, HashSet};
        let mut ids = HashSet::new();
        let mut aliases = HashMap::new();
        for reference in catalog::VOICES {
            assert!(
                ids.insert(reference.id),
                "duplicate voice ID: {}",
                reference.id
            );
            for alias in reference.aliases {
                let normalized = alias
                    .trim()
                    .replace('(', "（")
                    .replace(')', "）")
                    .to_ascii_lowercase();
                if let Some(previous) = aliases.insert(normalized, reference.id) {
                    assert_eq!(previous, reference.id, "conflicting alias: {alias}");
                }
                assert_eq!(catalog::id_for_alias(alias), Some(reference.id));
            }
        }
        assert_eq!(ids.len(), 28);
    }

    #[test]
    fn unavailable_or_ambiguous_voices_are_never_replaced() {
        let voices = vec![
            voice("zh-CN-XiaoxiaoNeural"),
            voice("zh-CN-YunxiNeural"),
            voice("zh-CN-sichuan-YunxiNeural"),
        ];
        assert!(select_voice(&voices, "晓涵").is_err());
        assert!(select_voice(&voices, "YunxiNeural").is_err());
        assert_eq!(
            select_voice(&voices, "云希").unwrap().short_name.as_deref(),
            Some("zh-CN-YunxiNeural")
        );
    }

    #[test]
    fn transient_errors_retry_but_permanent_errors_do_not() {
        let mut attempts = 0;
        let result = retry_until_deadline(Instant::now() + Duration::from_secs(2), 1, || {
            attempts += 1;
            if attempts == 1 {
                Err(AttemptError::retryable("temporary".to_owned()))
            } else {
                Ok(vec![1, 2])
            }
        });
        assert_eq!(result.unwrap(), vec![1, 2]);
        assert_eq!(attempts, 2);
        attempts = 0;
        let result: std::result::Result<(), String> =
            retry_until_deadline(Instant::now() + Duration::from_secs(2), 3, || {
                attempts += 1;
                Err(AttemptError::permanent("unknown voice".to_owned()))
            });
        assert_eq!(result.unwrap_err(), "unknown voice");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn expired_requests_and_stopped_services_do_not_start_network_work() {
        let result: std::result::Result<(), String> =
            retry_until_deadline(Instant::now(), 3, || panic!("expired request executed"));
        assert!(result.is_err());
        let service = EdgeTtsService::default();
        service.shutdown();
        assert!(service.voices("", 1).unwrap_err().contains("停用"));
    }
}
