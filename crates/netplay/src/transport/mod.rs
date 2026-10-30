//! Message transports: one non-blocking interface over an in-memory link,
//! native TCP / WebSocket sockets and the browser's `WebSocket`.
//!
//! Everything polls. A game host calls [`Transport::send`] and drains
//! [`Transport::try_recv`] once per frame; nothing blocks the frame and no
//! transport owns a thread, so the same driving code runs natively and in a
//! browser event loop.

use std::fmt;

use crate::protocol::Message;
use crate::wire::WireError;

pub mod loopback;
#[cfg(not(target_arch = "wasm32"))]
pub mod stream;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;

/// Why a transport failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The peer closed the connection, or it was closed locally.
    Closed,
    /// An OS-level socket error (its text).
    Io(String),
    /// The peer sent bytes that are not a protocol message.
    Wire(WireError),
    /// Connecting or the handshake took too long.
    Timeout,
    /// The WebSocket opening handshake failed.
    Handshake(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "connection closed"),
            Self::Io(e) => write!(f, "socket error: {e}"),
            Self::Wire(e) => write!(f, "bad data from peer: {e}"),
            Self::Timeout => write!(f, "timed out"),
            Self::Handshake(e) => write!(f, "handshake failed: {e}"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<WireError> for NetError {
    fn from(e: WireError) -> Self {
        Self::Wire(e)
    }
}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => Self::Timeout,
            _ => Self::Io(e.to_string()),
        }
    }
}

/// A bidirectional, ordered-or-not message pipe to one peer (a client's to
/// the relay, or the relay's to one client).
pub trait Transport {
    /// Queues `msg` for the peer. May buffer; never blocks the caller.
    fn send(&mut self, msg: &Message) -> Result<(), NetError>;
    /// The next message that has arrived, if any. `Err(Closed)` once the
    /// peer is gone and everything it sent has been read.
    fn try_recv(&mut self) -> Result<Option<Message>, NetError>;
    /// Closes the connection; later sends fail with [`NetError::Closed`].
    fn close(&mut self);
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, msg: &Message) -> Result<(), NetError> {
        (**self).send(msg)
    }
    fn try_recv(&mut self) -> Result<Option<Message>, NetError> {
        (**self).try_recv()
    }
    fn close(&mut self) {
        (**self).close()
    }
}

/// One [`Transport::try_recv`] for a pump loop that has already collected
/// messages: an error after some messages ends the loop with `Ok(None)` so
/// the caller still sees them (the room-closed notice before the close);
/// the transport reports the error again on the next call.
pub(crate) fn recv_or_hold<T: Transport + ?Sized>(
    t: &mut T,
    nothing_collected: bool,
) -> Result<Option<Message>, NetError> {
    match t.try_recv() {
        Err(e) if nothing_collected => Err(e),
        Err(_) => Ok(None),
        ok => ok,
    }
}
