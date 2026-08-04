//! Error Type

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum Error {
    #[cfg(feature = "blocking")]
    #[error("ureq error: {0}")]
    UreqError(#[from] ureq::Error),

    #[cfg(feature = "proxy")]
    #[error("proxy error: {0}")]
    ProxyError(#[from] ProxyError),

    #[error("tungstenite error: {0}")]
    TungsteniteError(#[from] tungstenite::Error),
    #[error("serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

#[cfg(feature = "proxy")]
mod proxy {
    use thiserror::Error;

    /// Proxy Error
    #[derive(Error, Debug)]
    pub enum ProxyError {
        #[error("invalid proxy URI: {0}")]
        InvalidProxyUri(#[from] http::uri::InvalidUri),
        #[error("not supported scheme: {0}")]
        NotSupportedScheme(http::Uri),
        #[error("http proxy error: {0}")]
        HttpProxyError(#[from] HttpProxyError),
        #[error("socks4 proxy error: {0}")]
        Socks4ProxyError(#[from] Socks4ProxyError),
        #[error("socks5 proxy error: {0}")]
        Socks5ProxyError(#[from] Socks5ProxyError),
    }

    /// Http Proxy Error
    #[derive(Error, Debug)]
    pub enum HttpProxyError {
        #[error("no proxy server host name: {0}")]
        NoProxyServerHostName(http::Uri),
        #[error("proxy host name is empty: {0}")]
        EmptyProxyServerHostName(http::Uri),
        #[error("io error: {0}")]
        IoError(#[from] std::io::Error),
        #[error("invalid response: {0}")]
        InvalidResponse(#[from] httparse::Error),
        #[error("bad response: {0} {1}")]
        BadResponse(u16, String),
        #[error("no status code")]
        NoStatusCode,
        #[error("not supported scheme: {0}")]
        NotSupportedScheme(http::Uri),
    }

    /// Socks4 Proxy Error
    #[derive(Error, Debug)]
    pub enum Socks4ProxyError {
        #[error("no proxy server host name: {0}")]
        NoProxyServerHostName(http::Uri),
        #[error("no proxy server port: {0}")]
        NoProxyServerPort(http::Uri),
        #[error("proxy host name is empty: {0}")]
        EmptyProxyServerHostName(http::Uri),
        #[error("io error: {0}")]
        IoError(#[from] std::io::Error),
        #[error("empty scheme: {0}")]
        NoScheme(http::Uri),
        #[error("not supported scheme: {0}")]
        NotSupportedScheme(http::Uri),
        #[error("lookup ip v4 addrs failed: {0}")]
        NoIpV4Addr(String),
        #[error("request rejected or failed")]
        RequestRejectedOrFailed(u8),
        #[error("no available identd service")]
        NoneAvailableIdentdService(u8),
        #[error("identd check failed: {0}")]
        IdentdCheckFailed(u8),
        #[error("unknown reply code: {0}")]
        UnknownReplyCode(u8),
    }

    /// Socks5 Proxy Error
    #[derive(Error, Debug)]
    pub enum Socks5ProxyError {
        #[error("no proxy server host name: {0}")]
        NoProxyServerHostName(http::Uri),
        #[error("no proxy server port: {0}")]
        NoProxyServerPort(http::Uri),
        #[error("proxy host name is empty: {0}")]
        EmptyProxyServerHostName(http::Uri),
        #[error("io error: {0}")]
        IoError(#[from] std::io::Error),
        #[error("empty scheme: {0}")]
        NoScheme(http::Uri),
        #[error("not supported scheme: {0}")]
        NotSupportedScheme(http::Uri),
        #[error("bad response version: {0}")]
        BadResponseVersion(u8),
        #[error("bad server choice: {0:?}")]
        BadServerChoice(u8),
        #[error("client authentication failed: {0:?}")]
        ClientAuthenticationFailed([u8; 2]),
        #[error("lookup ip addrs failed: {0}")]
        NoIpAddr(String),
        #[error("not supported server bind address type: {0}")]
        NotSupportedServerBindAddressType(u8),
        #[error("general failure: {0}")]
        GeneralFailure(u8),
        #[error("connection not allowed by rules: {0}")]
        ConnectionNotAllowedByRules(u8),
        #[error("network unreachable: {0}")]
        NetworkUnreachable(u8),
        #[error("host unreachable: {0}")]
        HostUnreachable(u8),
        #[error("connection refused: {0}")]
        ConnectionRefused(u8),
        #[error("ttl expired: {0}")]
        TtlExpired(u8),
        #[error("command not supported: {0}")]
        CommandNotSupported(u8),
        #[error("address type not supported: {0}")]
        AddressTypeNotSupported(u8),
        #[error("unknown reply code: {0}")]
        UnknownReplyCode(u8),
    }
}

#[cfg(feature = "proxy")]
pub use proxy::*;
