//! 插件端口类型：宿主与插件之间共享的服务接口。
//!
//! 插件 crate 实现这些 trait 并注册为服务；宿主（脚本命令等）按服务名取用，
//! 从而与具体插件实现解耦。

use std::sync::Arc;

/// 一次 TTS 请求的参数。超时覆盖音色查询、连接、接收和全部重试。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsOptions {
    /// 相对语速，单位为百分比，0 表示服务默认值。
    pub rate: i32,
    /// 相对音调，单位为 Hz。
    pub pitch: i32,
    /// 相对音量，单位为百分比。
    pub volume: i32,
    pub timeout_secs: u64,
    /// 首次尝试失败后，最多额外尝试多少次。
    pub retries: u8,
}

impl Default for TtsOptions {
    fn default() -> Self {
        Self {
            rate: 0,
            pitch: 0,
            volume: 0,
            timeout_secs: 12,
            retries: 1,
        }
    }
}

impl TtsOptions {
    /// 解析 RedLang 的可选位置参数；空字符串使用默认值。
    /// rate/volume 接受整数或 % 后缀，pitch 接受整数或 Hz 后缀。
    pub fn from_parameters(
        rate: &str,
        pitch: &str,
        volume: &str,
        timeout_secs: &str,
        retries: &str,
    ) -> std::result::Result<Self, String> {
        fn adjustment(value: &str, suffix: &str, label: &str) -> std::result::Result<i32, String> {
            let value = value.trim();
            if value.is_empty() {
                return Ok(0);
            }
            let number = value
                .strip_suffix(suffix)
                .or_else(|| value.strip_suffix(&suffix.to_ascii_lowercase()))
                .unwrap_or(value);
            number
                .parse()
                .map_err(|_| format!("TTS {label} 必须为整数，可带 {suffix} 后缀"))
        }
        let defaults = Self::default();
        let options = Self {
            rate: adjustment(rate, "%", "语速")?,
            pitch: adjustment(pitch, "Hz", "音调")?,
            volume: adjustment(volume, "%", "音量")?,
            timeout_secs: if timeout_secs.trim().is_empty() {
                defaults.timeout_secs
            } else {
                timeout_secs
                    .trim()
                    .parse()
                    .map_err(|_| "TTS 超时必须为整数秒".to_owned())?
            },
            retries: if retries.trim().is_empty() {
                defaults.retries
            } else {
                retries
                    .trim()
                    .parse()
                    .map_err(|_| "TTS 重试次数必须为非负整数".to_owned())?
            },
        };
        options.validate()?;
        Ok(options)
    }

    /// 本插件的输入边界，不代表微软服务对所有值均承诺支持。
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(-99..=1000).contains(&self.rate) {
            return Err("TTS 语速须在 -99..1000% 之间".to_owned());
        }
        if !(-1000..=1000).contains(&self.pitch) {
            return Err("TTS 音调须在 -1000..1000Hz 之间".to_owned());
        }
        if !(-100..=100).contains(&self.volume) {
            return Err("TTS 音量须在 -100..100% 之间".to_owned());
        }
        if !(1..=120).contains(&self.timeout_secs) {
            return Err("TTS 总超时须在 1..120 秒之间".to_owned());
        }
        if self.retries > 3 {
            return Err("TTS 额外重试次数须在 0..3 之间".to_owned());
        }
        Ok(())
    }
}

/// 当前服务目录中的人声；id 是可用于合成的完整 ShortName。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsVoice {
    pub id: String,
    pub name: String,
    pub locale: String,
    pub gender: String,
    pub aliases: Vec<String>,
}

/// 已录制的参考试听。存在样本不表示当前后端仍支持合成该人声。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsVoiceSample {
    pub id: String,
    pub name: String,
    pub category: String,
    pub gender: String,
    pub sample_url: String,
    pub source_url: String,
}

/// TTS 服务端口。
pub trait TtsService: Send + Sync {
    fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String>;

    /// 兼容只实现旧接口的第三方服务，但不静默忽略新的非默认参数。
    fn synthesize_with_options(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
        options: &TtsOptions,
    ) -> std::result::Result<Vec<u8>, String> {
        options.validate()?;
        if options == &TtsOptions::default() {
            self.synthesize(text, player, proxy)
        } else {
            Err("当前 TTS 服务不支持扩展参数".to_owned())
        }
    }

    fn voices(
        &self,
        _proxy: &str,
        _timeout_secs: u64,
    ) -> std::result::Result<Vec<TtsVoice>, String> {
        Err("当前 TTS 服务不支持查询人声".to_owned())
    }

    fn samples(&self) -> Vec<TtsVoiceSample> {
        Vec::new()
    }

    /// 插件停用时拒绝后续调用；默认实现保持第三方旧服务兼容。
    fn shutdown(&self) {}
}

/// TTS 服务句柄（以具体类型注册，内部持有动态实现）。
pub struct TtsServiceHandle(pub Arc<dyn TtsService>);

impl TtsServiceHandle {
    pub fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.0.synthesize(text, player, proxy)
    }

    pub fn synthesize_with_options(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
        options: &TtsOptions,
    ) -> std::result::Result<Vec<u8>, String> {
        self.0.synthesize_with_options(text, player, proxy, options)
    }

    pub fn voices(
        &self,
        proxy: &str,
        timeout_secs: u64,
    ) -> std::result::Result<Vec<TtsVoice>, String> {
        self.0.voices(proxy, timeout_secs)
    }

    pub fn samples(&self) -> Vec<TtsVoiceSample> {
        self.0.samples()
    }

    pub fn shutdown(&self) {
        self.0.shutdown();
    }
}

/// 浏览器服务端口。
pub trait BrowserBridgeService: Send + Sync {
    /// 执行浏览器请求（JSON 协议见 `redreply_browser` crate）。
    fn execute_json(&self, request: &str) -> std::result::Result<Vec<u8>, String>;
}

/// 浏览器服务句柄。
pub struct BrowserServiceHandle(pub Arc<dyn BrowserBridgeService>);

impl BrowserServiceHandle {
    pub fn execute_json(&self, request: &str) -> std::result::Result<Vec<u8>, String> {
        self.0.execute_json(request)
    }
}

/// MQTT 服务端口：跨实例事件发布与远程 API 调用。
pub trait MqttService: Send + Sync {
    /// 是否已连上 broker。
    fn connected(&self) -> bool;

    /// 把本实例事件发布到 MQTT（来自 MQTT 的事件应自行忽略）。
    fn publish_event(&self, event: &serde_json::Value) -> std::result::Result<(), String>;

    /// 调用远端实例 API，返回 OneBot 原始 JSON 字符串。
    fn call_remote(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        payload: serde_json::Value,
        remote_id: &str,
    ) -> std::result::Result<String, String>;
}

/// MQTT 服务句柄。
pub struct MqttServiceHandle(pub Arc<dyn MqttService>);

impl MqttServiceHandle {
    pub fn connected(&self) -> bool {
        self.0.connected()
    }

    pub fn publish_event(
        &self,
        event: &serde_json::Value,
    ) -> std::result::Result<(), String> {
        self.0.publish_event(event)
    }

    pub fn call_remote(
        &self,
        platform: &str,
        self_id: &str,
        passive_id: &str,
        payload: serde_json::Value,
        remote_id: &str,
    ) -> std::result::Result<String, String> {
        self.0.call_remote(platform, self_id, passive_id, payload, remote_id)
    }
}

/// PostgreSQL 服务端口：消息落库。
pub trait PgService: Send + Sync {
    /// 已启用并连接 PostgreSQL 时写入一条消息事件。
    fn insert_message(&self, root: &serde_json::Value) -> std::result::Result<(), String>;
}

/// PostgreSQL 服务句柄。
pub struct PgServiceHandle(pub Arc<dyn PgService>);

impl PgServiceHandle {
    pub fn insert_message(
        &self,
        root: &serde_json::Value,
    ) -> std::result::Result<(), String> {
        self.0.insert_message(root)
    }
}


#[cfg(test)]
mod tts_tests {
    use super::*;

    #[test]
    fn parses_defaults_signed_values_and_units() {
        assert_eq!(
            TtsOptions::from_parameters("", "", "", "", "").unwrap(),
            TtsOptions::default()
        );
        assert_eq!(
            TtsOptions::from_parameters("+20%", "-5Hz", "-10", "30", "0").unwrap(),
            TtsOptions {
                rate: 20,
                pitch: -5,
                volume: -10,
                timeout_secs: 30,
                retries: 0
            }
        );
        for values in [
            ("1.5", "", "", "", ""),
            ("-100", "", "", "", ""),
            ("", "1001", "", "", ""),
            ("", "", "101", "", ""),
            ("", "", "", "0", ""),
            ("", "", "", "121", ""),
            ("", "", "", "", "4"),
            ("", "", "", "", "-1"),
        ] {
            assert!(
                TtsOptions::from_parameters(values.0, values.1, values.2, values.3, values.4)
                    .is_err()
            );
        }
    }

    struct LegacyService;
    impl TtsService for LegacyService {
        fn synthesize(&self, text: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, String> {
            Ok(text.as_bytes().to_vec())
        }
    }

    #[test]
    fn legacy_implementations_do_not_silently_drop_new_options() {
        let service = TtsServiceHandle(Arc::new(LegacyService));
        assert_eq!(
            service
                .synthesize_with_options("hello", "", "", &TtsOptions::default())
                .unwrap(),
            b"hello"
        );
        let options = TtsOptions {
            rate: 20,
            ..TtsOptions::default()
        };
        assert!(service
            .synthesize_with_options("hello", "", "", &options)
            .is_err());
        assert!(service.voices("", 12).is_err());
    }
}
