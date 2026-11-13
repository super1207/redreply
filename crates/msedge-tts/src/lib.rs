//! This library is a wrapper of **MSEdge Read aloud** function API.
//! You can use it to synthesize text to speech with many voices MS provided.
//!
//! # Features
//!
//! | Feature | Description |
//! |---|---|
//! | `blocking` (default) | Synchronous TTS client and voice list using `ureq` and `tungstenite`. |
//! | `proxy` | SOCKS4/5 and HTTP CONNECT proxy support. |
//!
//! # How to use
//! 1. You need get a [tts::SpeechConfig] to configure the voice of text to speech.  
//!    You can convert [voice::Voice] to [tts::SpeechConfig] simply. Use [voice::get_voices_list] function to get all available voices.  
//!    [voice::Voice] and [tts::SpeechConfig] implemented [serde::Serialize] and [serde::Deserialize].  
//!    For example:
//!     ```rust
//!     use msedge_tts::voice::get_voices_list;
//!     use msedge_tts::tts::SpeechConfig;
//!     
//!     let voices = get_voices_list().unwrap();
//!     let speechConfig = SpeechConfig::from(&voices[0]);
//!     ```
//!     You can also create [tts::SpeechConfig] by yourself. Make sure you know the right **voice name** and **audio format**.
//!
//! 2. Create a TTS [tts::client] and synthesize text to speech.
//!    Call client function [synthesize](tts::client::MSEdgeTTSClient::synthesize) to synthesize text to speech. This function return Type [SynthesizedAudio](tts::client::SynthesizedAudio),
//!    you can get [audio_bytes](tts::client::SynthesizedAudio::audio_bytes) and [audio_metadata](tts::client::SynthesizedAudio::audio_metadata).
//!     ```rust
//!     use msedge_tts::{tts::client::connect, tts::SpeechConfig, voice::get_voices_list};
//!     
//!     let voices = get_voices_list().unwrap();
//!     for voice in &voices {
//!         if voice.name.contains("YunyangNeural") {
//!             let config = SpeechConfig::from(voice);
//!             let mut tts = connect().unwrap();
//!             let audio = tts
//!                 .synthesize("Hello, World! 你好，世界！", &config)
//!                 .unwrap();
//!             break;
//!         }
//!     }
//!     ```

mod auth;
mod constants;

#[cfg(feature = "blocking")]
mod deadline;
pub mod error;
pub mod tts;
pub mod voice;
