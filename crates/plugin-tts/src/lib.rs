//! TTS 插件：包装 msedge-tts，注册 `tts` 服务供其他插件（如 redlang）调用。

use std::sync::Arc;

use redlang::plugin_ports::{TtsService, TtsServiceHandle};
use redreply_bot::{Plugin, PluginContext, PluginMeta, Result};

/// 基于 Edge TTS 的实现（原 `TTS` 命令逻辑）。
pub struct EdgeTtsService;

impl TtsService for EdgeTtsService {
    fn synthesize(
        &self,
        text: &str,
        player: &str,
        proxy: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        let voices = if proxy.is_empty() {
            msedge_tts::voice::get_voices_list().map_err(|error| error.to_string())?
        } else {
            msedge_tts::voice::get_voices_list_proxy(proxy).map_err(|error| error.to_string())?
        };
        for voice in &voices {
            if voice.name.contains(player) {
                let config = msedge_tts::tts::SpeechConfig::from(voice);
                let audio = if proxy.is_empty() {
                    let mut tts = msedge_tts::tts::client::connect()
                        .map_err(|error| error.to_string())?;
                    tts.synthesize(text, &config)
                        .map_err(|error| error.to_string())?
                } else {
                    let mut tts = msedge_tts::tts::client::connect_proxy(proxy)
                        .map_err(|error| error.to_string())?;
                    tts.synthesize(text, &config)
                        .map_err(|error| error.to_string())?
                };
                return Ok(audio.audio_bytes);
            }
        }
        Err(format!("player `{player}` not found"))
    }
}

/// TTS 插件。
pub struct TtsPlugin;

impl Plugin for TtsPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("tts", env!("CARGO_PKG_VERSION"))
            .description("Edge TTS 语音合成服务")
            .provides("tts")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.register_service(
            "tts",
            Arc::new(TtsServiceHandle(Arc::new(EdgeTtsService))),
        )
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info("tts", "TTS 插件已就绪");
        Ok(())
    }
}
