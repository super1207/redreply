//! Functions to get all available voices.
//!
//! Use get_voices_list function to get all available voices.
//! Use get_voices_list_proxy function to get all available voices with proxy.

use super::transport::{ConnectorResolver, DeadlineConnector};
use crate::{constants, error::Result, voice::Voice};
use std::time::Instant;

fn voice_list_url() -> String {
    format!(
        "{}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",
        constants::VOICE_LIST_URL,
        crate::auth::sec_ms_gec(),
        constants::SEC_MS_GEC_VERSION
    )
}

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
        .get(voice_list_url())
        .header("User-Agent", constants::USER_AGENT)
        .call()?
        .body_mut()
        .read_json()?)
}

/// Query the live catalog within the caller's remaining total request budget.
/// An empty proxy explicitly means a direct connection, as in the TTS client.
pub fn get_voices_list_with_deadline(proxy: &str, deadline: Instant) -> Result<Vec<Voice>> {
    let tls_config = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .build();
    let config = ureq::config::Config::builder()
        .tls_config(tls_config)
        .timeout_global(Some(crate::deadline::remaining(deadline)?))
        // The custom connector handles the explicit proxy and its entire
        // handshake. Do not invoke ureq's unbounded SOCKS worker.
        .proxy(None)
        .build();
    let connector = DeadlineConnector {
        proxy: proxy.to_owned(),
        deadline,
    };
    let agent = ureq::Agent::with_parts(config, connector, ConnectorResolver);
    Ok(agent
        .get(voice_list_url())
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
        .get(voice_list_url())
        .header("User-Agent", constants::USER_AGENT)
        .call()?
        .body_mut()
        .read_json()?)
}

#[cfg(all(test, feature = "proxy"))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn stalled_socks_catalog_query_respects_total_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("socks5h://{}", listener.local_addr().unwrap());
        let (release, stop) = mpsc::channel::<()>();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            // Never answer the SOCKS greeting. The old ureq scoped worker
            // waited for this peer to close even after timeout_global expired.
            let _ = stop.recv_timeout(Duration::from_secs(3));
        });
        let started = Instant::now();
        let result = get_voices_list_with_deadline(&proxy, started + Duration::from_millis(500));
        let elapsed = started.elapsed();
        let _ = release.send(());
        peer.join().unwrap();
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_secs(2),
            "SOCKS query took {elapsed:?}"
        );
    }
}
