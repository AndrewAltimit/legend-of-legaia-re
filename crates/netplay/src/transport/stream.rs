//! Native client transports over a non-blocking `TcpStream`: raw TCP
//! (length-prefixed) and WebSocket. Connecting blocks, bounded by a
//! timeout; after that every call returns immediately.

use std::hash::{BuildHasher, RandomState};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::protocol::Message;
use crate::transport::{NetError, Transport};
use crate::websocket;
use crate::wire::{RAW_PREAMBLE, WireDecoder, WireEncoder, WireEvent, WireKind};

/// Unsent bytes a transport holds before it gives up on a peer that has
/// stopped reading.
const MAX_SEND_BUFFER: usize = 1 << 20;

/// A connected native client.
#[derive(Debug)]
pub struct StreamTransport {
    stream: TcpStream,
    enc: WireEncoder,
    dec: WireDecoder,
    out: Vec<u8>,
    /// The peer sent EOF or a close frame.
    peer_closed: bool,
    /// Closed locally or after an error.
    closed: bool,
}

fn random_u64() -> u64 {
    RandomState::new().hash_one(Instant::now())
}

fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> Result<TcpStream, NetError> {
    let mut last = NetError::Io("address resolved to nothing".into());
    for a in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                s.set_nodelay(true)?;
                return Ok(s);
            }
            Err(e) => last = e.into(),
        }
    }
    Err(last)
}

impl StreamTransport {
    /// Connects over raw TCP.
    pub fn connect_tcp(addr: impl ToSocketAddrs, timeout: Duration) -> Result<Self, NetError> {
        let mut stream = connect(addr, timeout)?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(RAW_PREAMBLE)?;
        Self::finish(stream, WireKind::Raw, &[])
    }

    /// Connects over WebSocket: `host` is the `Host` header (`ip:port`),
    /// `path` the request path (the relay accepts any).
    pub fn connect_ws(
        addr: impl ToSocketAddrs,
        host: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<Self, NetError> {
        let mut stream = connect(addr, timeout)?;
        let deadline = Instant::now() + timeout;
        stream.set_write_timeout(Some(timeout))?;
        let key =
            websocket::base64(&[random_u64().to_le_bytes(), random_u64().to_le_bytes()].concat());
        stream.write_all(&websocket::client_request(host, path, &key))?;

        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        let end = loop {
            if let Some(end) = websocket::head_end(&head) {
                break end;
            }
            if head.len() > websocket::MAX_HANDSHAKE_LEN {
                return Err(NetError::Handshake("response head too long".into()));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(NetError::Timeout);
            }
            stream.set_read_timeout(Some(left))?;
            match stream.read(&mut buf) {
                Ok(0) => return Err(NetError::Closed),
                Ok(n) => head.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        };
        websocket::check_server_response(&head[..end], &key)
            .map_err(|e| NetError::Handshake(e.to_string()))?;
        Self::finish(stream, WireKind::WsClient, &head[end..])
    }

    fn finish(stream: TcpStream, kind: WireKind, early: &[u8]) -> Result<Self, NetError> {
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        stream.set_nonblocking(true)?;
        let mut dec = WireDecoder::new(kind);
        dec.push(early);
        Ok(Self {
            stream,
            enc: WireEncoder::new(kind, random_u64()),
            dec,
            out: Vec::new(),
            peer_closed: false,
            closed: false,
        })
    }

    fn fail(&mut self, e: NetError) -> NetError {
        self.closed = true;
        let _ = self.stream.shutdown(Shutdown::Both);
        e
    }

    /// Writes as much of the send buffer as the socket takes.
    fn flush(&mut self) -> Result<(), NetError> {
        while !self.out.is_empty() {
            match self.stream.write(&self.out) {
                Ok(0) => return Err(self.fail(NetError::Closed)),
                Ok(n) => {
                    self.out.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(self.fail(e.into())),
            }
        }
        Ok(())
    }

    fn fill(&mut self) -> Result<(), NetError> {
        let mut buf = [0u8; 4096];
        while !self.peer_closed {
            match self.stream.read(&mut buf) {
                Ok(0) => self.peer_closed = true,
                Ok(n) => self.dec.push(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(self.fail(e.into())),
            }
        }
        Ok(())
    }
}

impl Transport for StreamTransport {
    fn send(&mut self, msg: &Message) -> Result<(), NetError> {
        if self.closed {
            return Err(NetError::Closed);
        }
        let bytes = self.enc.message(msg);
        if self.out.len() + bytes.len() > MAX_SEND_BUFFER {
            return Err(self.fail(NetError::Io("peer stopped reading".into())));
        }
        self.out.extend_from_slice(&bytes);
        self.flush()
    }

    fn try_recv(&mut self) -> Result<Option<Message>, NetError> {
        if self.closed {
            return Err(NetError::Closed);
        }
        self.flush()?;
        self.fill()?;
        loop {
            match self.dec.next_event() {
                Ok(Some(WireEvent::Message(m))) => return Ok(Some(m)),
                Ok(Some(WireEvent::Ping(p))) => {
                    let pong = self.enc.pong(&p);
                    self.out.extend_from_slice(&pong);
                    self.flush()?;
                }
                Ok(Some(WireEvent::Close)) => {
                    self.peer_closed = true;
                    return Err(self.fail(NetError::Closed));
                }
                Ok(None) if self.peer_closed => return Err(self.fail(NetError::Closed)),
                Ok(None) => return Ok(None),
                Err(e) => return Err(self.fail(e.into())),
            }
        }
    }

    fn close(&mut self) {
        if !self.closed {
            let bye = self.enc.close();
            self.out.extend_from_slice(&bye);
            let _ = self.flush();
            self.closed = true;
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for StreamTransport {
    fn drop(&mut self) {
        self.close();
    }
}
