//! ureq adapter for the same bounded sockets used by synthesis.
//!
//! ureq 3.3's SOCKS connector waits for a scoped worker after its timeout; a
//! nonresponsive proxy can therefore exceed timeout_global. This connector keeps
//! DNS, proxy negotiation, TLS and response reads on our deadline-aware stream.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

use crate::deadline::DeadlineStream;

trait IoStream: Read + Write + Send + Sync {}
impl<T: Read + Write + Send + Sync> IoStream for T {}

pub(super) struct DeadlineConnector {
    pub proxy: String,
    pub deadline: Instant,
}

impl fmt::Debug for DeadlineConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TtsDeadlineConnector")
            .field("uses_proxy", &!self.proxy.is_empty())
            .finish()
    }
}

#[derive(Debug)]
pub(super) struct ConnectorResolver;

impl Resolver for ConnectorResolver {
    fn resolve(
        &self,
        _uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        // DeadlineConnector performs the actual bounded resolution. In
        // particular an HTTP/SOCKS5h proxy must resolve the target remotely.
        let mut addresses = self.empty();
        addresses.push(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 443)));
        Ok(addresses)
    }
}

impl Connector for DeadlineConnector {
    type Out = DeadlineTransport;

    fn connect(
        &self,
        details: &ConnectionDetails,
        _chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let host = details.uri.host().ok_or(ureq::Error::HostNotFound)?;
        if details.uri.scheme_str() != Some("https") || details.uri.port_u16().unwrap_or(443) != 443
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TTS catalog requires HTTPS on port 443",
            )
            .into());
        }
        let stream: Box<dyn IoStream> = if self.proxy.is_empty() {
            let tcp = DeadlineStream::connect(host, 443, self.deadline)?;
            Box::new(crate::tts::native_tls_connect(host, tcp).map_err(io::Error::other)?)
        } else {
            #[cfg(feature = "proxy")]
            {
                let proxy = crate::tts::proxy::blocking::connect_stream(
                    host,
                    &self.proxy,
                    Some(self.deadline),
                )
                .map_err(io::Error::other)?;
                Box::new(crate::tts::native_tls_connect(host, proxy).map_err(io::Error::other)?)
            }
            #[cfg(not(feature = "proxy"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "proxy feature is not enabled",
                )
                .into());
            }
        };
        Ok(Some(DeadlineTransport {
            stream,
            buffers: LazyBuffers::new(16 * 1024, 16 * 1024),
        }))
    }
}

pub(super) struct DeadlineTransport {
    stream: Box<dyn IoStream>,
    buffers: LazyBuffers,
}

impl fmt::Debug for DeadlineTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TtsDeadlineTransport")
    }
}

fn io_error(error: io::Error, timeout: NextTimeout) -> ureq::Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        ureq::Error::Timeout(timeout.reason)
    } else {
        error.into()
    }
}

impl Transport for DeadlineTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream
            .write_all(&self.buffers.output()[..amount])
            .map_err(|error| io_error(error, timeout))?;
        self.stream
            .flush()
            .map_err(|error| io_error(error, timeout))
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let count = self
            .stream
            .read(self.buffers.input_append_buf())
            .map_err(|error| io_error(error, timeout))?;
        self.buffers.input_appended(count);
        Ok(count > 0)
    }

    // Each catalog query owns a short-lived agent and one request deadline. Its
    // connection must not be pooled and later reused with an expired deadline.
    fn is_open(&mut self) -> bool {
        false
    }
    fn is_tls(&self) -> bool {
        true
    }
}
