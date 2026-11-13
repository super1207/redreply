pub use crate::deadline::DeadlineStream;
use crate::error::Result;
use crate::tts::{
    NativeTlsStream, Payload, SpeechConfig, build_config_message, build_ssml_message,
    client::SynthesizedAudio, websocket_connect, websocket_connect_with_deadline,
};
use std::io::{Read, Write};
use std::net::TcpStream;

/// Sync Client
pub struct MSEdgeTTSClient<T: Read + Write>(pub(crate) tungstenite::WebSocket<NativeTlsStream<T>>);

impl<T: Read + Write> MSEdgeTTSClient<T> {
    /// Synthesize text to speech with a [SpeechConfig] synchronously
    pub fn synthesize(&mut self, text: &str, config: &SpeechConfig) -> Result<SynthesizedAudio> {
        let config_message = build_config_message(config);
        let ssml_message = build_ssml_message(text, config);
        self.0.send(config_message)?;
        self.0.send(ssml_message)?;

        let mut audio_bytes = Vec::new();
        let mut audio_metadata = Vec::new();
        let mut turn_start = false;
        let mut response = false;
        let mut turn_end = false;
        loop {
            if turn_end {
                break;
            }

            let message = self.0.read()?;
            let payload = Payload::process(message, &mut turn_start, &mut response, &mut turn_end)?;
            if let Some(payload) = payload {
                match payload {
                    Payload::AudioBytes(payload) => {
                        audio_bytes.push(payload);
                    }
                    Payload::AudioMetadata(metadata) => {
                        audio_metadata.extend(metadata);
                    }
                }
            }
        }

        let audio_bytes: Vec<u8> = audio_bytes
            .iter()
            .flat_map(|(bytes, index)| &bytes[*index..])
            .copied()
            .collect();

        if audio_bytes.is_empty() {
            return Err(crate::error::Error::UnexpectedMessage(
                "TTS returned empty audio".to_owned(),
            ));
        }

        Ok(SynthesizedAudio {
            audio_format: config.audio_format.clone(),
            audio_bytes,
            audio_metadata,
        })
    }
}

/// Create Sync TTS [Client](MSEdgeTTSClient)
pub fn connect() -> Result<MSEdgeTTSClient<TcpStream>> {
    Ok(MSEdgeTTSClient(websocket_connect()?))
}

/// Connect and synthesize with one absolute deadline, including socket I/O.
pub fn connect_with_deadline(
    deadline: std::time::Instant,
) -> Result<MSEdgeTTSClient<DeadlineStream>> {
    Ok(MSEdgeTTSClient(websocket_connect_with_deadline(deadline)?))
}

#[cfg(feature = "proxy")]
pub use crate::tts::proxy::blocking::{connect_proxy, connect_proxy_with_deadline};
