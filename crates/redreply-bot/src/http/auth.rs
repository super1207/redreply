use std::net::IpAddr;

use hyper::header::{HeaderMap, COOKIE};
use serde::Serialize;

/// 框架鉴权配置。
///
/// 与旧版 `config_manager` 语义一致：
/// - `web_password` 为空时视为免密（开放访问）；
/// - 回环地址完全免鉴权；
/// - 同时存在读写密码时，只读密码只能访问 `Read` 级别的路由。
#[derive(Clone, Debug, Default)]
pub struct AuthConfig {
    pub web_password: String,
    pub readonly_web_password: String,
}

impl AuthConfig {
    pub fn new(web_password: impl Into<String>, readonly_web_password: impl Into<String>) -> Self {
        Self {
            web_password: web_password.into(),
            readonly_web_password: readonly_web_password.into(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.web_password.is_empty()
    }
}

/// 路由声明的访问要求。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteAccess {
    /// 任何人可访问（登录页等）。
    Public,
    /// 需要只读及以上权限。
    Read,
    /// 需要读写权限。
    Write,
    /// 仅限本机访问。
    LocalOnly,
}

/// 调用者实际获得的权限级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    None,
    Read,
    Write,
}

impl AccessLevel {
    pub fn can(self, required: RouteAccess, is_local: bool) -> bool {
        match required {
            RouteAccess::Public => true,
            RouteAccess::LocalOnly => is_local,
            RouteAccess::Read => matches!(self, AccessLevel::Read | AccessLevel::Write),
            RouteAccess::Write => self == AccessLevel::Write,
        }
    }
}

/// 根据请求头与来源地址解析调用者权限。
pub fn resolve_access(headers: &HeaderMap, remote: IpAddr, config: &AuthConfig) -> AccessLevel {
    if remote.is_loopback() {
        return AccessLevel::Write;
    }
    if config.is_open() {
        return AccessLevel::Write;
    }
    let Some(cookie_header) = headers.get(COOKIE).and_then(|value| value.to_str().ok()) else {
        return AccessLevel::None;
    };
    match cookie_value(cookie_header, "password") {
        Some(password) if password == config.web_password => AccessLevel::Write,
        Some(password)
            if !config.readonly_web_password.is_empty()
                && password == config.readonly_web_password =>
        {
            AccessLevel::Read
        }
        _ => AccessLevel::None,
    }
}

/// 从 Cookie 头里取指定名字的值（自动百分号解码）。
pub fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((key, value)) = part.split_once('=') {
            if key.trim() == name {
                return Some(percent_decode(value.trim()));
            }
        }
    }
    None
}

/// 生成写入 Cookie 的百分号编码值。
pub fn encode_cookie_value(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn percent_decode(raw: &str) -> String {
    if !raw.contains('%') && !raw.contains('+') {
        return raw.to_string();
    }
    url::form_urlencoded::parse(format!("v={raw}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers
    }

    #[test]
    fn loopback_always_write() {
        let config = AuthConfig::new("secret", "readonly");
        let access = resolve_access(&HeaderMap::new(), "127.0.0.1".parse().unwrap(), &config);
        assert_eq!(access, AccessLevel::Write);
    }

    #[test]
    fn open_when_no_password() {
        let config = AuthConfig::default();
        let access = resolve_access(
            &HeaderMap::new(),
            "192.168.1.2".parse::<IpAddr>().unwrap(),
            &config,
        );
        assert_eq!(access, AccessLevel::Write);
    }

    #[test]
    fn password_levels() {
        let config = AuthConfig::new("secret", "readonly");
        let remote: IpAddr = "192.168.1.2".parse().unwrap();
        assert_eq!(
            resolve_access(&headers("password=secret"), remote, &config),
            AccessLevel::Write
        );
        assert_eq!(
            resolve_access(&headers("password=readonly"), remote, &config),
            AccessLevel::Read
        );
        assert_eq!(
            resolve_access(&headers("password=wrong"), remote, &config),
            AccessLevel::None
        );
        assert_eq!(
            resolve_access(&HeaderMap::new(), remote, &config),
            AccessLevel::None
        );
    }

    #[test]
    fn percent_encoded_cookie_is_decoded() {
        let encoded = encode_cookie_value("p@ss word");
        let config = AuthConfig::new("p@ss word", "");
        let access = resolve_access(
            &headers(&format!("password={encoded}")),
            "192.168.1.2".parse().unwrap(),
            &config,
        );
        assert_eq!(access, AccessLevel::Write);
    }
}
