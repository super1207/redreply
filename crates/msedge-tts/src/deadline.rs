//! Blocking sockets whose DNS, connect, read and write phases share one deadline.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

static RESOLVERS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
const MAX_RESOLVERS: usize = 4;

pub(crate) fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "TTS total deadline exceeded"))
}

struct ResolverPermit;

impl Drop for ResolverPermit {
    fn drop(&mut self) {
        RESOLVERS_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// OS name resolution cannot be cancelled. A small process-wide cap prevents
/// timed-out DNS lookups from accumulating threads; none retains plugin state.
pub(crate) fn resolve(host: &str, port: u16, deadline: Instant) -> io::Result<Vec<SocketAddr>> {
    remaining(deadline)?;
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    RESOLVERS_IN_FLIGHT
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            (active < MAX_RESOLVERS).then_some(active + 1)
        })
        .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "TTS DNS resolver is busy"))?;
    let permit = ResolverPermit;
    let host = host.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("tts-dns".to_owned())
        .spawn(move || {
            let _permit = permit;
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.collect());
            let _ = sender.send(result);
        })?;
    receiver
        .recv_timeout(remaining(deadline)?)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                io::Error::new(io::ErrorKind::TimedOut, "TTS DNS lookup timed out")
            }
            mpsc::RecvTimeoutError::Disconnected => io::Error::other("TTS DNS resolver stopped"),
        })?
}

pub(crate) fn resolve_optional(
    host: &str,
    port: u16,
    deadline: Option<Instant>,
) -> io::Result<Vec<SocketAddr>> {
    match deadline {
        Some(deadline) => resolve(host, port, deadline),
        None => (host, port)
            .to_socket_addrs()
            .map(|addresses| addresses.collect()),
    }
}

/// Every operation applies the remaining request time to the actual OS socket.
/// Repeated partial reads/writes therefore cannot reset the total timeout.
#[derive(Debug)]
pub struct DeadlineStream {
    stream: TcpStream,
    deadline: Option<Instant>,
}

impl DeadlineStream {
    pub fn connect(host: &str, port: u16, deadline: Instant) -> io::Result<Self> {
        Self::connect_optional(host, port, Some(deadline))
    }

    /// Legacy clients keep their original unbounded connection lifetime.
    pub(crate) fn connect_optional(
        host: &str,
        port: u16,
        deadline: Option<Instant>,
    ) -> io::Result<Self> {
        let Some(limit) = deadline else {
            let stream = TcpStream::connect((host, port))?;
            stream.set_nodelay(true)?;
            return Ok(Self { stream, deadline });
        };
        let addresses = resolve(host, port, limit)?;
        let mut last_error = io::Error::new(
            io::ErrorKind::NotFound,
            "TTS DNS lookup returned no addresses",
        );
        for address in addresses {
            match TcpStream::connect_timeout(&address, remaining(limit)?) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    return Ok(Self { stream, deadline });
                }
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    pub fn set_nodelay(&self, enabled: bool) -> io::Result<()> {
        self.stream.set_nodelay(enabled)
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            self.stream.set_read_timeout(Some(remaining(deadline)?))?;
        }
        self.stream.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            self.stream.set_write_timeout(Some(remaining(deadline)?))?;
        }
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(deadline) = self.deadline {
            remaining(deadline)?;
        }
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn stalled_socket_has_a_real_read_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let started = Instant::now();
        let mut client = DeadlineStream::connect(
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
            started + Duration::from_millis(200),
        )
        .unwrap();
        let (_server, _) = listener.accept().unwrap();
        let error = client.read(&mut [0; 1]).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn reads_do_not_restart_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let started = Instant::now();
        let mut client = DeadlineStream::connect(
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
            started + Duration::from_millis(200),
        )
        .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.write_all(b"x").unwrap();
        client.read_exact(&mut [0; 1]).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        let error = client.read(&mut [0; 1]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
