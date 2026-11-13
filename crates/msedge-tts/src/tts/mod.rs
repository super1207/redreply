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
            tungstenite::Message::Close(_) => Err(Error::UnexpectedMessage(
                "WebSocket closed before turn.end; incomplete audio was discarded".to_owned(),
            )),
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) => Ok(None),
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
        escape_xml(&config.voice_name),
        config.pitch,
        config.rate,
        config.volume,
        escape_xml(text),
    );
    let ssml_message = format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}\r\nPath:ssml\r\n\r\n{}",
        uuid::Uuid::new_v4().simple(),
        chrono::Local::now().to_rfc2822(),
        ssml,
    );
    tungstenite::Message::Text(ssml_message.into())
}

/// The public synthesis API accepts plain text, never caller-supplied SSML.
fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\'' => escaped.push_str("&apos;"),
            '"' => escaped.push_str("&quot;"),
            '\t' | '\n' | '\r' => escaped.push(character),
            character if character < '\u{20}' || matches!(character, '\u{fffe}' | '\u{ffff}') => {
                escaped.push(' ')
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn build_websocket_request() -> tungstenite::Result<tungstenite::handshake::client::Request> {
    use crate::constants;
    use tungstenite::client::IntoClientRequest;
    use tungstenite::http::header;

    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let sec_ms_gec = crate::auth::sec_ms_gec();
    let mut request = format!(
        "{}{}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",
        constants::WSS_URL,
        uuid,
        sec_ms_gec,
        constants::SEC_MS_GEC_VERSION
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
pub(crate) type NativeTlsStream<T> = rustls::StreamOwned<rustls::ClientConnection, T>;

#[cfg(feature = "blocking")]
pub(crate) fn native_tls_connect<T: std::io::Read + std::io::Write>(
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
    use tungstenite::{Error, error::*};

    let request = build_websocket_request()?;
    let host = request
        .uri()
        .host()
        .ok_or(Error::Url(UrlError::NoHostName))?
        .to_owned();

    let stream = std::net::TcpStream::connect((host.as_str(), 443)).map_err(Error::Io)?;
    stream.set_nodelay(true).map_err(Error::Io)?;

    websocket_with_stream(request, &host, stream)
}

#[cfg(feature = "blocking")]
fn websocket_connect_with_deadline(
    deadline: std::time::Instant,
) -> Result<tungstenite::WebSocket<NativeTlsStream<crate::deadline::DeadlineStream>>> {
    use tungstenite::{Error, error::*};
    let request = build_websocket_request()?;
    let host = request
        .uri()
        .host()
        .ok_or(Error::Url(UrlError::NoHostName))?
        .to_owned();
    let stream = crate::deadline::DeadlineStream::connect(&host, 443, deadline)?;
    websocket_with_stream(request, &host, stream)
}

#[cfg(feature = "blocking")]
fn websocket_with_stream<T: std::io::Read + std::io::Write>(
    request: tungstenite::handshake::client::Request,
    host: &str,
    stream: T,
) -> Result<tungstenite::WebSocket<NativeTlsStream<T>>> {
    use tungstenite::{ClientHandshake, Error, HandshakeError};
    let stream = native_tls_connect(host, stream)?;
    let (websocket, _) = ClientHandshake::start(stream, request, None)?
        .handshake()
        .map_err(|e| match e {
            HandshakeError::Failure(e) => e,
            HandshakeError::Interrupted(_) => Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "TTS WebSocket handshake timed out",
            )),
        })?;
    Ok(websocket)
}

#[cfg(feature = "proxy")]
pub(crate) mod proxy;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_and_voice_names_cannot_inject_ssml() {
        let config = SpeechConfig {
            voice_name: "voice'&<>\"".to_owned(),
            audio_format: "audio-24khz-48kbitrate-mono-mp3".to_owned(),
            pitch: -10,
            rate: 20,
            volume: -5,
        };
        let message = build_ssml_message("你好 <break/> & \"音色\" '\0'", &config);
        let text = message.to_text().unwrap();
        assert!(text.contains("name='voice&apos;&amp;&lt;&gt;&quot;'"));
        assert!(text.contains("你好 &lt;break/&gt; &amp; &quot;音色&quot; &apos; &apos;"));
        assert!(text.contains("pitch='-10Hz' rate='+20%' volume='-5%'"));
        assert!(!text.contains("<break/>"));
    }

    #[test]
    fn closing_before_turn_end_does_not_accept_partial_audio() {
        let mut started = true;
        let mut response = true;
        let mut ended = false;
        let result = Payload::process(
            tungstenite::Message::Close(None),
            &mut started,
            &mut response,
            &mut ended,
        );
        assert!(result.is_err());
        assert!(!ended);
    }
}
