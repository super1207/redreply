use crate::deadline::{DeadlineStream, resolve_optional};
use std::io::{Read, Write};
use std::time::Instant;

use crate::{
    error::{HttpProxyError, Result, Socks4ProxyError, Socks5ProxyError},
    tts::{
        NativeTlsStream, build_websocket_request,
        client::MSEdgeTTSClient,
        proxy::{
            build_http_proxy_request, build_socks4_connection_request,
            build_socks5_authentication_request, build_socks5_connection_request, parse_userinfo,
        },
        websocket_with_stream,
    },
};

pub enum ProxyStream {
    TcpStream(DeadlineStream),
    TlsStream(Box<NativeTlsStream<DeadlineStream>>),
}

impl std::io::Read for ProxyStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::TcpStream(stream) => stream.read(buf),
            Self::TlsStream(stream) => stream.read(buf),
        }
    }
}

impl std::io::Write for ProxyStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::TcpStream(stream) => stream.write(buf),
            Self::TlsStream(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::TcpStream(stream) => stream.flush(),
            Self::TlsStream(stream) => stream.flush(),
        }
    }
}

fn socks4_proxy(
    target_host: &str,
    proxy: http::Uri,
    username: Option<&str>,
    deadline: Option<Instant>,
) -> Result<ProxyStream, Socks4ProxyError> {
    if proxy.scheme_str().is_none() {
        return Err(Socks4ProxyError::NoScheme(proxy));
    }
    if proxy.host().is_none() {
        return Err(Socks4ProxyError::NoProxyServerHostName(proxy));
    }
    let proxy_host = proxy.host().unwrap();
    if proxy_host.is_empty() {
        return Err(Socks4ProxyError::EmptyProxyServerHostName(proxy));
    }
    if proxy.port_u16().is_none() {
        return Err(Socks4ProxyError::NoProxyServerPort(proxy));
    }
    let proxy_port = proxy.port_u16().unwrap();

    let mut stream = DeadlineStream::connect_optional(proxy_host, proxy_port, deadline)?;
    stream.set_nodelay(true)?;
    let request = match proxy.scheme_str().unwrap().to_lowercase().as_str() {
        "socks4" => {
            let mut socket_addrs = resolve_optional(target_host, 443, deadline)?.into_iter();
            let ipv4 = loop {
                match socket_addrs.next() {
                    Some(socket_addr) => match socket_addr.ip() {
                        std::net::IpAddr::V4(ip) => break Some(ip),
                        std::net::IpAddr::V6(_) => {}
                    },
                    None => break None,
                }
            };

            if ipv4.is_none() {
                return Err(Socks4ProxyError::NoIpV4Addr(format!("{}:443", target_host)));
            }
            build_socks4_connection_request(target_host, ipv4, username)
        }
        "socks4a" => build_socks4_connection_request(target_host, None, username),
        _ => return Err(Socks4ProxyError::NotSupportedScheme(proxy)),
    };
    stream.write_all(&request)?;
    stream.flush()?;

    let mut buf = [0u8; 8];
    stream.read_exact(&mut buf)?;
    match buf[1] {
        0x5a => Ok(ProxyStream::TcpStream(stream)),
        0x5b => Err(Socks4ProxyError::RequestRejectedOrFailed(0x5b)),
        0x5c => Err(Socks4ProxyError::NoneAvailableIdentdService(0x5c)),
        0x5d => Err(Socks4ProxyError::IdentdCheckFailed(0x5d)),
        code => Err(Socks4ProxyError::UnknownReplyCode(code)),
    }
}

fn socks5_proxy(
    target_host: &str,
    proxy: http::Uri,
    username: Option<&str>,
    password: Option<&str>,
    deadline: Option<Instant>,
) -> Result<ProxyStream, Socks5ProxyError> {
    if proxy.scheme_str().is_none() {
        return Err(Socks5ProxyError::NoScheme(proxy));
    }
    if proxy.host().is_none() {
        return Err(Socks5ProxyError::NoProxyServerHostName(proxy));
    }
    let proxy_host = proxy.host().unwrap();
    if proxy_host.is_empty() {
        return Err(Socks5ProxyError::EmptyProxyServerHostName(proxy));
    }
    if proxy.port_u16().is_none() {
        return Err(Socks5ProxyError::NoProxyServerPort(proxy));
    }
    let proxy_port = proxy.port_u16().unwrap();

    let mut stream = DeadlineStream::connect_optional(proxy_host, proxy_port, deadline)?;
    stream.set_nodelay(true)?;

    // Client greeting: VER (1), NAUTH (1), AUTH (NAUTH)
    let mut bytes = vec![0x05];
    if username.is_some() && password.is_some() {
        bytes.extend([0x02, 0x00, 0x02]); // NAUTH, No authentication (0x00), Username/Password (0x02)
    } else {
        bytes.extend([0x01, 0x00]); // NAUTH, No authentication (0x00)
    }
    stream.write_all(&bytes)?;
    stream.flush()?;

    // Server choice: VER (1), CAUTH (1)
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf)?;
    if buf[0] != 0x05 {
        return Err(Socks5ProxyError::BadResponseVersion(buf[0]));
    }
    if buf[1] != 0x00 && buf[1] != 0x02 {
        return Err(Socks5ProxyError::BadServerChoice(buf[1]));
    }

    // Client authentication
    if buf[1] == 0x02 {
        // 客户端未声明凭据时服务器不应选择 0x02；若仍被选择（不合规/被劫持的
        // 服务器），必须报错而不是 unwrap panic（远程可控数据）
        let username = username.ok_or(Socks5ProxyError::BadServerChoice(0x02))?;
        let password = password.ok_or(Socks5ProxyError::BadServerChoice(0x02))?;
        let request = build_socks5_authentication_request(username, password);
        stream.write_all(&request)?;
        stream.flush()?;

        // Server response: VER (1), STATUS (1)
        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf)?;
        if buf[1] != 0x00 {
            return Err(Socks5ProxyError::ClientAuthenticationFailed(buf));
        }
    }

    // Client connection
    let request = match proxy.scheme_str().unwrap().to_lowercase().as_str() {
        "socks5" => {
            let socket_addr = resolve_optional(target_host, 443, deadline)?
                .into_iter()
                .next();
            if let Some(ip_addr) = socket_addr {
                build_socks5_connection_request(target_host, Some(ip_addr.ip()))
            } else {
                return Err(Socks5ProxyError::NoIpAddr(format!("{}:443", target_host)));
            }
        }
        "socks" | "socks5h" => build_socks5_connection_request(target_host, None),
        _ => return Err(Socks5ProxyError::NotSupportedScheme(proxy)),
    };
    stream.write_all(&request)?;
    stream.flush()?;

    // Server response: VER (1), STATUS (1), RSV (1), BNDADDR [TYPE (1), ADDR (4/16)], BNDPORT (2)
    let mut buf = [0u8; 4]; // VER (1), STATUS (1), RSV (1), BNDADDR TYPE (1)
    stream.read_exact(&mut buf)?;
    if buf[0] != 0x05 {
        return Err(Socks5ProxyError::BadResponseVersion(buf[0]));
    }
    match buf[1] {
        0x00 => {
            // BND.ADDR describes the proxy's outbound endpoint. The tunnel stays
            // on this socket; reconnecting to BND.ADDR would discard it.
            let address_bytes = match buf[3] {
                0x01 => 4,
                0x04 => 16,
                0x03 => {
                    let mut length = [0; 1];
                    stream.read_exact(&mut length)?;
                    usize::from(length[0])
                }
                addr_t => return Err(Socks5ProxyError::NotSupportedServerBindAddressType(addr_t)),
            };
            let mut bound_address = vec![0; address_bytes + 2];
            stream.read_exact(&mut bound_address)?;
            Ok(ProxyStream::TcpStream(stream))
        }
        0x01 => Err(Socks5ProxyError::GeneralFailure(0x01)),
        0x02 => Err(Socks5ProxyError::ConnectionNotAllowedByRules(0x02)),
        0x03 => Err(Socks5ProxyError::NetworkUnreachable(0x03)),
        0x04 => Err(Socks5ProxyError::HostUnreachable(0x04)),
        0x05 => Err(Socks5ProxyError::ConnectionRefused(0x05)),
        0x06 => Err(Socks5ProxyError::TtlExpired(0x06)),
        0x07 => Err(Socks5ProxyError::CommandNotSupported(0x07)),
        0x08 => Err(Socks5ProxyError::AddressTypeNotSupported(0x08)),
        code => Err(Socks5ProxyError::UnknownReplyCode(code)),
    }
}

fn http_proxy(
    target_host: &str,
    proxy: http::Uri,
    username: Option<&str>,
    password: Option<&str>,
    deadline: Option<Instant>,
) -> std::result::Result<ProxyStream, HttpProxyError> {
    if let Some(proxy_host) = proxy.host() {
        if proxy_host.is_empty() {
            return Err(HttpProxyError::EmptyProxyServerHostName(proxy));
        }
    } else {
        return Err(HttpProxyError::NoProxyServerHostName(proxy));
    }

    let proxy_host = proxy.host().unwrap().to_owned();

    let proxy_port = proxy.port_u16().unwrap_or(match proxy.scheme_str() {
        None => 80,
        Some(scheme) => match scheme.to_lowercase().as_str() {
            "https" => 443,
            "http" => 80,
            _ => 80,
        },
    });

    let mut stream = match proxy.scheme_str() {
        None => {
            let stream = DeadlineStream::connect_optional(&proxy_host, proxy_port, deadline)?;
            stream.set_nodelay(true)?;
            ProxyStream::TcpStream(stream)
        }
        Some(scheme) => match scheme.to_lowercase().as_str() {
            "http" => {
                let stream = DeadlineStream::connect_optional(&proxy_host, proxy_port, deadline)?;
                stream.set_nodelay(true)?;
                ProxyStream::TcpStream(stream)
            }
            "https" => {
                let stream = DeadlineStream::connect_optional(&proxy_host, proxy_port, deadline)?;
                stream.set_nodelay(true)?;

                let _ = rustls::crypto::ring::default_provider().install_default();
                let mut root_store = rustls::RootCertStore::empty();
                root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let config = rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth();
                let server_name = rustls::pki_types::ServerName::try_from(proxy_host.clone())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
                let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), server_name)
                    .map_err(std::io::Error::other)?;
                let mut tls_stream = rustls::StreamOwned::new(conn, stream);
                tls_stream.flush()?;
                ProxyStream::TlsStream(Box::new(tls_stream))
            }
            _ => return Err(HttpProxyError::NotSupportedScheme(proxy)),
        },
    };
    stream.write_all(build_http_proxy_request(target_host, username, password).as_bytes())?;
    stream.flush()?;

    let mut buf = [0u8; 1024];
    let mut n = 0;
    loop {
        // 缓冲写满仍未见头部结束符：头部过长；read 返回 0：代理提前断开。
        // 两种情况都必须报错，否则 read(&mut buf[n..]) 永远读到空切片
        // 返回 Ok(0)，循环变成 100% CPU 死转
        if n == buf.len() {
            return Err(HttpProxyError::BadResponse(0, "代理响应头过长".to_string()));
        }
        let got = stream.read(&mut buf[n..])?;
        if got == 0 {
            return Err(HttpProxyError::BadResponse(
                0,
                "代理提前断开连接".to_string(),
            ));
        }
        n += got;
        if n >= 4 && &buf[n - 4..n] == b"\r\n\r\n" {
            break;
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 5];
    let mut response = httparse::Response::new(&mut headers);
    response.parse(&buf)?;

    match response.code {
        None => Err(HttpProxyError::NoStatusCode),
        Some(200) => Ok(stream),
        Some(code) => Err(HttpProxyError::BadResponse(
            code,
            response.reason.unwrap_or("").to_owned(),
        )),
    }
}

use crate::error::ProxyError;

fn websocket_connect_proxy(
    uri: &str,
    deadline: Option<Instant>,
) -> Result<tungstenite::WebSocket<NativeTlsStream<ProxyStream>>> {
    use tungstenite::error::*;
    let request = build_websocket_request()?;
    let target_host = request
        .uri()
        .host()
        .ok_or(Error::Url(UrlError::NoHostName))?
        .to_owned();
    let stream = connect_stream(&target_host, uri, deadline)?;
    websocket_with_stream(request, &target_host, stream)
}

/// Shared by bounded WebSocket and voice-list connections, including proxy I/O.
pub(crate) fn connect_stream(
    target_host: &str,
    uri: &str,
    deadline: Option<Instant>,
) -> Result<ProxyStream> {
    let proxy: http::Uri = uri.parse().map_err(ProxyError::InvalidProxyUri)?;
    let (username, password) = parse_userinfo(&proxy);
    let stream: std::result::Result<ProxyStream, ProxyError> = match proxy.scheme_str() {
        Some(scheme) => match scheme.to_lowercase().as_str() {
            "socks4" | "socks4a" => socks4_proxy(target_host, proxy, username.as_deref(), deadline)
                .map_err(|e| e.into()),
            "socks" | "socks5" | "socks5h" => socks5_proxy(
                target_host,
                proxy,
                username.as_deref(),
                password.as_deref(),
                deadline,
            )
            .map_err(|e| e.into()),
            "http" | "https" => http_proxy(
                target_host,
                proxy,
                username.as_deref(),
                password.as_deref(),
                deadline,
            )
            .map_err(|e| e.into()),
            _ => Err(ProxyError::NotSupportedScheme(proxy)),
        },
        None => http_proxy(
            target_host,
            proxy,
            username.as_deref(),
            password.as_deref(),
            deadline,
        )
        .map_err(|e| e.into()),
    };

    Ok(stream?)
}

/// Create Sync TTS [Client](MSEdgeTTSClient) with proxy
///
/// # Arguments:
///
/// * `proxy` - a str of format `<protocol>://<user>:<password>@<host>:port`.
///
/// The proxy protocol is specified by the URI scheme.
///
/// * `http`: Proxy. Default when no scheme is specified.  
/// * `https`: HTTPS Proxy.  
/// * `socks4`: SOCKS4 Proxy.  
/// * `socks4a`: SOCKS4a Proxy. Proxy resolves URL hostname.  
/// * `socks5`: SOCKS5 Proxy.  
/// * `socks` | `socks5h`: SOCKS5 Proxy. Proxy resolves URL hostname.  
pub fn connect_proxy(proxy: &str) -> Result<MSEdgeTTSClient<ProxyStream>> {
    Ok(MSEdgeTTSClient(websocket_connect_proxy(proxy, None)?))
}

/// The same absolute deadline covers proxy negotiation, TLS and synthesis I/O.
pub fn connect_proxy_with_deadline(
    proxy: &str,
    deadline: Instant,
) -> Result<MSEdgeTTSClient<ProxyStream>> {
    Ok(MSEdgeTTSClient(websocket_connect_proxy(
        proxy,
        Some(deadline),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn socks_connect_keeps_the_established_tunnel() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("socks5h://{}", listener.local_addr().unwrap());
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            stream.write_all(&[5, 0]).unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request[..4], &[5, 1, 0, 3]);
            let mut target = vec![0; usize::from(request[4]) + 2];
            stream.read_exact(&mut target).unwrap();
            // A successful proxy can return 0.0.0.0:0 as its bound address;
            // this is not a new destination for the client to connect to.
            stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            stream.write_all(b"tunnel").unwrap();
        });
        let mut stream = connect_stream(
            "example.test",
            &proxy,
            Some(Instant::now() + Duration::from_secs(2)),
        )
        .unwrap();
        let mut response = [0; 6];
        stream.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"tunnel");
        peer.join().unwrap();
    }
}
