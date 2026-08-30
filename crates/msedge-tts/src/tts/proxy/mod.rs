/// VER (1), CMD (1), DSTPORT (2), DSTIP (4), ID (? + 1), socks4a? DOMAIN (? + 1)
fn build_socks4_connection_request(
    target_host: &str,
    dst_ip: Option<std::net::Ipv4Addr>,
    username: Option<&str>,
) -> Vec<u8> {
    // VER, CMD, DSTPORT
    let mut bytes = vec![0x04, 0x01, 0x01, 0xbb];

    // DSTIP (4)
    if let Some(ip) = dst_ip {
        bytes.extend(ip.octets()); // socks4
    } else {
        bytes.extend([0x00, 0x00, 0x00, 0x01]); // socks4a
    }

    // ID (? + 1)
    if let Some(username) = username {
        bytes.extend(username.as_bytes());
    }
    bytes.push(0x00);

    // socks4a DOMAIN (? + 1)
    if dst_ip.is_none() {
        bytes.extend(target_host.as_bytes());
        bytes.push(0x00);
    }

    bytes
}

/// VER (1), IDLEN (1), ID (IDLEN), PWLEN (1), PW (PWLEN)
fn build_socks5_authentication_request(username: &str, password: &str) -> Vec<u8> {
    let mut bytes = vec![0x01]; // VER
    if username.is_empty() {
        bytes.extend([0x01, 0x00]);
    } else {
        bytes.push(username.len() as u8); // IDLEN
        bytes.extend(username.as_bytes()); // ID
    }
    if password.is_empty() {
        bytes.extend([0x01, 0x00]);
    } else {
        bytes.push(password.len() as u8); // PWLEN
        bytes.extend(password.as_bytes()); // PW
    }
    bytes
}

/// VER (1), CMD (1), RSV (1), DSTADDR [TYPE (1), ADDR (?)], DSTPORT (2)
fn build_socks5_connection_request(target_host: &str, dst_ip: Option<std::net::IpAddr>) -> Vec<u8> {
    // VER, CMD, RSV
    let mut bytes = vec![0x05, 0x01, 0x00];

    // DSTADDR
    if let Some(ip) = dst_ip {
        match ip {
            std::net::IpAddr::V4(ip) => {
                bytes.push(0x01); // TYPE
                bytes.extend(ip.octets()); // ADDR
            }
            std::net::IpAddr::V6(ip) => {
                bytes.push(0x04); // TYPE
                bytes.extend(ip.octets()); // ADDR
            }
        }
    } else {
        bytes.push(0x03); // TYPE
        bytes.push(target_host.len() as u8); // ADDRLEN
        bytes.extend(target_host.as_bytes()); // ADDR
    }

    // DSTPORT
    bytes.extend([0x01, 0xbb]);

    bytes
}

fn build_http_proxy_request(
    target_host: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> String {
    use base64::*;

    if let Some(username) = username
        && let Some(password) = password
    {
        let credential =
            base64::prelude::BASE64_STANDARD.encode(format!("{}:{}", username, password));
        format!(
            "CONNECT {}:443 HTTP/1.1\r\nHost: {}:443\r\nProxy-Authorization: Basic {}\r\nProxy-Connection: Keep-Alive\r\n\r\n",
            target_host, target_host, credential
        )
    } else {
        format!(
            "CONNECT {}:443 HTTP/1.1\r\nHost: {}:443\r\nProxy-Connection: Keep-Alive\r\n\r\n",
            target_host, target_host
        )
    }
}

/// Extract `(username, password)` from the URI authority's userinfo.
///
/// Supports `user@host`, `user:pass@host`, and no-userinfo formats.
pub(crate) fn parse_userinfo(uri: &http::Uri) -> (Option<String>, Option<String>) {
    if let Some(authority) = uri.authority() {
        let s = authority.as_str();
        if let Some(at) = s.rfind('@') {
            let userinfo = &s[..at];
            if let Some(colon) = userinfo.find(':') {
                return (
                    Some(userinfo[..colon].to_owned()),
                    Some(userinfo[colon + 1..].to_owned()),
                );
            }
            return (Some(userinfo.to_owned()), None);
        }
    }
    (None, None)
}

#[cfg(feature = "blocking")]
pub(crate) mod blocking;

#[cfg(test)]
mod tests {
    use super::parse_userinfo;

    #[test]
    fn parse_userinfo_none() {
        let uri = "http://127.0.0.1:8080".parse().unwrap();
        assert_eq!(parse_userinfo(&uri), (None, None));
    }

    #[test]
    fn parse_userinfo_username_only() {
        let uri = "http://john@127.0.0.1:8080".parse().unwrap();
        assert_eq!(parse_userinfo(&uri), (Some("john".into()), None));
    }

    #[test]
    fn parse_userinfo_username_and_password() {
        let uri = "http://john:smith@127.0.0.1:8080".parse().unwrap();
        assert_eq!(
            parse_userinfo(&uri),
            (Some("john".into()), Some("smith".into()))
        );
    }

    #[test]
    fn parse_userinfo_password_with_at() {
        let uri = "http://user:p@ss@host:8000".parse().unwrap();
        assert_eq!(
            parse_userinfo(&uri),
            (Some("user".into()), Some("p@ss".into()))
        );
    }

    #[test]
    fn parse_userinfo_socks_scheme() {
        let uri = "socks5://alice:secret@proxy:1080".parse().unwrap();
        assert_eq!(
            parse_userinfo(&uri),
            (Some("alice".into()), Some("secret".into()))
        );
    }
}
