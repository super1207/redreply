//! Functions to get all available voices.
//!
//! Use get_voices_list function to get all available voices.
//! Use get_voices_list_proxy function to get all available voices with proxy.

use crate::{constants, error::Result, voice::Voice};

/// Get all available voices
pub fn get_voices_list() -> Result<Vec<Voice>> {
    let tls_config = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .build();
    let config = ureq::config::Config::builder()
        .tls_config(tls_config)
        .build();
    Ok(config
        .new_agent()
        .get(constants::VOICE_LIST_URL)
        .header("User-Agent", constants::USER_AGENT)
        .call()?
        .body_mut()
        .read_json()?)
}

/// Get all available voices with proxy.
///
/// **doc copy from ureq**
///
/// Create a proxy from a uri.
///
/// # Arguments:
///
/// * `proxy` - a str of format `<protocol>://<user>:<password>@<host>:port` . All parts
///   except host are optional.
///
/// ###  Protocols
///
/// * `http`: HTTP CONNECT proxy
/// * `https`: HTTPS CONNECT proxy (requires a TLS provider)
/// * `socks4`: SOCKS4 (requires **socks-proxy** feature)
/// * `socks4a`: SOCKS4A (requires **socks-proxy** feature)
/// * `socks5` and `socks`: SOCKS5 (requires **socks-proxy** feature)
///
/// # Examples proxy formats
///
/// * `http://127.0.0.1:8080`
/// * `socks5://john:smith@socks.google.com`
/// * `john:smith@socks.google.com:8000`
/// * `localhost`
#[cfg(feature = "proxy")]
pub fn get_voices_list_proxy(proxy: &str) -> Result<Vec<Voice>> {
    let tls_config = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .build();
    let proxy = ureq::Proxy::new(proxy)?;
    let config = ureq::config::Config::builder()
        .tls_config(tls_config)
        .proxy(Some(proxy))
        .build();
    Ok(config
        .new_agent()
        .get(constants::VOICE_LIST_URL)
        .header("User-Agent", constants::USER_AGENT)
        .call()?
        .body_mut()
        .read_json()?)
}
