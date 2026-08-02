use std::io::{Read, Write};

use crate::{
    error::{HttpProxyError, Result, Socks4ProxyError, Socks5ProxyError},
    tts::{
        NativeTlsStream, build_websocket_request,
        client::MSEdgeTTSClient,
        native_tls_connect,
        proxy::{
            build_http_proxy_request, build_socks4_connection_request,
            build_socks5_authentication_request, build_socks5_connection_request, parse_userinfo,
        },
    },
};

pub enum ProxyStream {
    TcpStream(std::net::TcpStream),
    TlsStream(Box<NativeTlsStream<std::net::TcpStream>>),
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
) -> Result<ProxyStream, Socks4ProxyError> {
    use std::net::ToSocketAddrs;

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

    let mut stream = std::net::TcpStream::connect((proxy_host, proxy_port))?;
    stream.set_nodelay(true)?;
    let request = match proxy.scheme_str().unwrap().to_lowercase().as_str() {
        "socks4" => {
            let mut socket_addrs = (target_host, 443).to_socket_addrs()?;
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
) -> Result<ProxyStream, Socks5ProxyError> {
    use std::net::ToSocketAddrs;

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

    let mut stream = std::net::TcpStream::connect((proxy_host, proxy_port))?;
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
        let request = build_socks5_authentication_request(username.unwrap(), password.unwrap());
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
            let socket_addr = (target_host, 443).to_socket_addrs()?.next();
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
    match buf[1] {
        0x00 => match buf[3] {
            0x01 | 0x04 => {
                let (ip, port) = {
                    if buf[3] == 0x01 {
                        // BNDADDR ADDR (4), BNDPORT (2)
                        let mut buf = [0u8; 6];
                        stream.read_exact(&mut buf)?;
                        let ip = std::net::IpAddr::from([buf[0], buf[1], buf[2], buf[3]]);
                        let port = u16::from_be_bytes([buf[4], buf[5]]);
                        (ip, port)
                    } else {
                        // BNDADDR ADDR (16), BNDPORT (2)
                        let mut buf = [0u8; 18];
                        stream.read_exact(&mut buf)?;
                        let ip = std::net::IpAddr::from([
                            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8],
                            buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
                        ]);
                        let port = u16::from_be_bytes([buf[16], buf[17]]);
                        (ip, port)
                    }
                };

                let socket_addr = stream.peer_addr().unwrap();
                if socket_addr.ip() == ip && socket_addr.port() == port {
                    Ok(ProxyStream::TcpStream(stream))
                } else {
                    let stream = std::net::TcpStream::connect((ip, port))?;
                    stream.set_nodelay(true)?;
                    Ok(ProxyStream::TcpStream(stream))
                }
            }
            addr_t => Err(Socks5ProxyError::NotSupportedServerBindAddressType(addr_t)),
        },
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
            let stream = std::net::TcpStream::connect((proxy_host, proxy_port))?;
            stream.set_nodelay(true)?;
            ProxyStream::TcpStream(stream)
        }
        Some(scheme) => match scheme.to_lowercase().as_str() {
            "http" => {
                let stream = std::net::TcpStream::connect((proxy_host, proxy_port))?;
                stream.set_nodelay(true)?;
                ProxyStream::TcpStream(stream)
            }
            "https" => {
                let stream = std::net::TcpStream::connect((proxy_host.as_str(), 443))?;
                stream.set_nodelay(true)?;

                let connector = native_tls::TlsConnector::new()?;
                let stream = match connector.connect(&proxy_host, stream) {
                    Ok(stream) => stream,
                    Err(native_tls::HandshakeError::Failure(err)) => return Err(err.into()),
                    Err(native_tls::HandshakeError::WouldBlock(_)) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "native TLS proxy handshake unexpectedly blocked",
                        )
                        .into());
                    }
                };
                ProxyStream::TlsStream(Box::new(stream))
            }
            _ => return Err(HttpProxyError::NotSupportedScheme(proxy)),
        },
    };
    stream.write_all(build_http_proxy_request(target_host, username, password).as_bytes())?;
    stream.flush()?;

    let mut buf = [0u8; 1024];
    let mut n = 0;
    loop {
        n += stream.read(&mut buf[n..])?;
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
) -> Result<tungstenite::WebSocket<NativeTlsStream<ProxyStream>>> {
    use tungstenite::ClientHandshake;
    use tungstenite::error::*;
    use tungstenite::handshake::HandshakeError;

    let proxy: http::Uri = uri.parse().map_err(ProxyError::InvalidProxyUri)?;
    let (username, password) = parse_userinfo(&proxy);

    let request = build_websocket_request()?;
    let target_host = request
        .uri()
        .host()
        .ok_or(Error::Url(UrlError::NoHostName))?
        .to_owned();
    let stream: std::result::Result<ProxyStream, ProxyError> = match proxy.scheme_str() {
        Some(scheme) => match scheme.to_lowercase().as_str() {
            "socks4" | "socks4a" => {
                socks4_proxy(target_host.as_str(), proxy, username.as_deref()).map_err(|e| e.into())
            }
            "socks" | "socks5" | "socks5h" => socks5_proxy(
                target_host.as_str(),
                proxy,
                username.as_deref(),
                password.as_deref(),
            )
            .map_err(|e| e.into()),
            "http" | "https" => http_proxy(
                target_host.as_str(),
                proxy,
                username.as_deref(),
                password.as_deref(),
            )
            .map_err(|e| e.into()),
            _ => Err(ProxyError::NotSupportedScheme(proxy)),
        },
        None => http_proxy(
            target_host.as_str(),
            proxy,
            username.as_deref(),
            password.as_deref(),
        )
        .map_err(|e| e.into()),
    };

    let stream = native_tls_connect(&target_host, stream?)?;
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
    Ok(MSEdgeTTSClient(websocket_connect_proxy(proxy)?))
}
