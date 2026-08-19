//! TTS Client and Stream, SpeechConfig, Response Type.

use crate::error::{Error, Result};

/// Synthesis Config
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SpeechConfig {
    pub voice_name: String,
    pub audio_format: String,
    pub pitch: i32,
    pub rate: i32,
    pub volume: i32,
}

impl From<&crate::voice::Voice> for SpeechConfig {
    fn from(voice: &crate::voice::Voice) -> Self {
        let audio_format = if let Some(ref audio_format) = voice.suggested_codec {
            audio_format.clone()
        } else {
            "audio-24khz-48kbitrate-mono-mp3".to_owned()
        };
        Self {
            voice_name: voice.name.clone(),
            audio_format,
            pitch: 0,
            rate: 0,
            volume: 0,
        }
    }
}

/// Audio Metadata
#[derive(Debug)]
pub struct AudioMetadata {
    pub metadata_type: Option<String>,
    pub offset: u64,
    pub duration: u64,
    pub text: Option<String>,
    pub length: u64,
    pub boundary_type: Option<String>,
}

impl AudioMetadata {
    fn from_str(text: &str) -> Result<Vec<Self>> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        if let Some(items) = value["Metadata"].as_array() {
            let mut audio_metadata = Vec::new();
            for item in items {
                let metadata_type = item["Type"].as_str().map(|x| x.to_owned());
                let offset = item["Data"]["Offset"].as_u64().unwrap_or(0);
                let duration = item["Data"]["Duration"].as_u64().unwrap_or(0);
                let text = item["Data"]["text"]["Text"].as_str().map(|x| x.to_owned());
                let length = item["Data"]["text"]["Length"].as_u64().unwrap_or(0);
                let boundary_type = item["Data"]["text"]["BoundaryType"]
                    .as_str()
                    .map(|x| x.to_owned());
                audio_metadata.push(AudioMetadata {
                    metadata_type,
                    offset,
                    duration,
                    text,
                    length,
                    boundary_type,
                });
            }
            Ok(audio_metadata)
        } else {
            Err(Error::UnexpectedMessage(format!(
                "unexpected json text: {}",
                text
            )))
        }
    }
}

pub mod client;

enum Payload {
    AudioBytes((tungstenite::Bytes, usize)),
    AudioMetadata(Vec<AudioMetadata>),
}

impl Payload {
    fn process(
        message: tungstenite::Message,
        turn_start: &mut bool,
        response: &mut bool,
        turn_end: &mut bool,
    ) -> Result<Option<Payload>> {
        match message {
            tungstenite::Message::Text(text) => {
                if text.contains("audio.metadata") {
                    if let Some(index) = text.find("\r\n\r\n") {
                        let metadata = AudioMetadata::from_str(&text[index + 4..])?;
                        Ok(Some(Payload::AudioMetadata(metadata)))
                    } else {
                        Ok(None)
                    }
                } else if text.contains("turn.start") {
                    *turn_start = true;
                    Ok(None)
                } else if text.contains("response") {
                    *response = true;
                    Ok(None)
                } else if text.contains("turn.end") {
                    *turn_end = true;
                    Ok(None)
                } else {
                    Err(Error::UnexpectedMessage(format!(
                        "unexpected text message: {}",
                        text
                    )))
                }
            }
            tungstenite::Message::Binary(bytes) => {
                if *turn_start || *response {
                    // 服务端帧不可信：不足2字节会下标越界 panic，
                    // 头声明长度超过帧实际长度会在调用方切片时 panic，必须先校验
                    if bytes.len() < 2 {
                        return Err(Error::UnexpectedMessage(
                            "binary audio frame too short".to_owned(),
                        ));
                    }
                    let header_len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                    let total = header_len.checked_add(2).ok_or_else(|| {
                        Error::UnexpectedMessage("binary audio header overflow".to_owned())
                    })?;
                    if total > bytes.len() {
                        return Err(Error::UnexpectedMessage(
                            "binary audio frame header exceeds frame length".to_owned(),
                        ));
                    }
                    Ok(Some(Payload::AudioBytes((bytes, total))))
                } else {
                    Ok(None)
                }
            }
            tungstenite::Message::Close(_) => {
                *turn_end = true;
                Ok(None)
            }
            _ => Err(Error::UnexpectedMessage(format!(
                "unexpected message: {}",
                message
            ))),
        }
    }
}

fn build_config_message(config: &SpeechConfig) -> tungstenite::Message {
    static SPEECH_CONFIG_HEAD: &str = r#"{"context":{"synthesis":{"audio":{"metadataoptions":{"sentenceBoundaryEnabled":"false","wordBoundaryEnabled":"true"},"outputFormat":""#;
    static SPEECH_CONFIG_TAIL: &str = r#""}}}}"#;
    let speech_config_message = format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{}{}{}",
        chrono::Local::now().to_rfc2822(),
        SPEECH_CONFIG_HEAD,
        config.audio_format,
        SPEECH_CONFIG_TAIL
    );
    tungstenite::Message::Text(speech_config_message.into())
}

fn build_ssml_message(text: &str, config: &SpeechConfig) -> tungstenite::Message {
    let ssml = format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'><voice name='{}'><prosody pitch='{:+}Hz' rate='{:+}%' volume='{:+}%'>{}</prosody></voice></speak>",
        config.voice_name, config.pitch, config.rate, config.volume, text,
    );
    let ssml_message = format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}\r\nPath:ssml\r\n\r\n{}",
        uuid::Uuid::new_v4().simple(),
        chrono::Local::now().to_rfc2822(),
        ssml,
    );
    tungstenite::Message::Text(ssml_message.into())
}

// try to fix china mainland 403 forbidden issue
// solution from:
// https://github.com/rany2/edge-tts/issues/290#issuecomment-2464956570
fn gen_sec_ms_gec() -> String {
    use sha2::Digest;

    // UTC time from 1601-01-01
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        + std::time::Duration::from_secs(11644473600);
    let ticks = duration.as_nanos() / 100;
    let ticks = ticks - ticks % 3_000_000_000;

    let mut hasher = sha2::Sha256::new();
    hasher.update(format!("{ticks}6A5AA1D4EAFF4E9FB37E23D68491D6F4"));
    let hash_code = hasher.finalize();
    let mut hex_str = String::new();
    for &byte in hash_code.iter() {
        hex_str.push_str(&format!("{:02X}", byte));
    }
    hex_str
}

fn build_websocket_request() -> tungstenite::Result<tungstenite::handshake::client::Request> {
    use crate::constants;
    use tungstenite::client::IntoClientRequest;
    use tungstenite::http::header;

    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let sec_ms_gec = gen_sec_ms_gec();
    let sec_ms_gec_version = "1-130.0.2849.68";
    let mut request = format!(
        "{}{}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",
        constants::WSS_URL,
        uuid,
        sec_ms_gec,
        sec_ms_gec_version
    )
    .into_client_request()?;
    let headers = request.headers_mut();
    headers.insert(header::PRAGMA, "no-cache".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    headers.insert(header::USER_AGENT, constants::USER_AGENT.parse().unwrap());
    headers.insert(header::ORIGIN, constants::ORIGIN.parse().unwrap());
    Ok(request)
}

#[cfg(feature = "blocking")]
type NativeTlsStream<T> = rustls::StreamOwned<rustls::ClientConnection, T>;

#[cfg(feature = "blocking")]
fn native_tls_connect<T: std::io::Read + std::io::Write>(
    host: &str,
    stream: T,
) -> Result<NativeTlsStream<T>> {
    use std::io::Write;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|e| {
        tungstenite::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    })?;
    let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), server_name)
        .map_err(|e| tungstenite::Error::Io(std::io::Error::other(e)))?;
    let mut tls_stream = rustls::StreamOwned::new(conn, stream);
    tls_stream.flush().map_err(|e| {
        tungstenite::Error::Io(std::io::Error::other(format!(
            "TLS handshake failed: {}",
            e
        )))
    })?;
    Ok(tls_stream)
}

#[cfg(feature = "blocking")]
fn websocket_connect() -> Result<tungstenite::WebSocket<NativeTlsStream<std::net::TcpStream>>> {
    use tungstenite::{ClientHandshake, Error, HandshakeError, error::*};

    let request = build_websocket_request()?;
    let host = request
        .uri()
        .host()
        .ok_or(Error::Url(UrlError::NoHostName))?
        .to_owned();

    let stream = std::net::TcpStream::connect((host.as_str(), 443)).map_err(Error::Io)?;
    stream.set_nodelay(true).map_err(Error::Io)?;

    let stream = native_tls_connect(&host, stream)?;
    let (websocket, _) = ClientHandshake::start(stream, request, None)?
        .handshake()
        .map_err(|e| match e {
            HandshakeError::Failure(e) => e,
            HandshakeError::Interrupted(_) => {
                panic!("Bug: blocking handshake not blocked")
            }
        })?;
    Ok(websocket)
}

#[cfg(feature = "proxy")]
mod proxy;
