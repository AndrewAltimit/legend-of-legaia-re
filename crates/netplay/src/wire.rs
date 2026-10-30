//! Message framing on a byte stream, for both stream flavours one relay
//! port serves.
//!
//! - **Raw TCP**: the client opens with the four-byte [`RAW_PREAMBLE`], then
//!   both sides send `u32 LE length` + message bytes.
//! - **WebSocket**: an RFC 6455 handshake, then one binary frame per message
//!   ([`crate::websocket`]).
//!
//! A relay tells them apart from the first four bytes: `LGNP` or `GET `.
//! Pure byte shuffling - the socket code lives in [`crate::transport`] and
//! [`crate::relay`].

use std::fmt;

use crate::codec::CodecError;
use crate::protocol::{MAX_MESSAGE_LEN, Message};
use crate::websocket::{self, FrameDecoder, WsError, WsMessage};

/// What a raw-TCP client sends before its first frame.
pub const RAW_PREAMBLE: &[u8; 4] = b"LGNP";

/// Which framing a stream uses, and which end of it this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    Raw,
    /// The relay's end of a WebSocket (sends unmasked, expects masked).
    WsServer,
    /// A client's end of a WebSocket (sends masked, expects unmasked).
    WsClient,
}

/// Why a stream was given up on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    Codec(CodecError),
    Ws(WsError),
    /// A raw frame whose length prefix exceeds [`MAX_MESSAGE_LEN`].
    TooLarge(usize),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(e) => write!(f, "{e}"),
            Self::Ws(e) => write!(f, "{e}"),
            Self::TooLarge(n) => write!(f, "frame of {n} bytes over the cap"),
        }
    }
}

impl std::error::Error for WireError {}

impl From<CodecError> for WireError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}

impl From<WsError> for WireError {
    fn from(e: WsError) -> Self {
        Self::Ws(e)
    }
}

/// Encodes outgoing messages for one stream.
#[derive(Debug)]
pub struct WireEncoder {
    kind: WireKind,
    /// xorshift state for client mask keys. RFC 6455 masking guards
    /// intermediaries against cache poisoning, not secrecy; a seeded
    /// generator is enough for that.
    mask_state: u64,
}

impl WireEncoder {
    pub fn new(kind: WireKind, mask_seed: u64) -> Self {
        Self {
            kind,
            mask_state: mask_seed | 1,
        }
    }

    fn mask(&mut self) -> Option<[u8; 4]> {
        (self.kind == WireKind::WsClient).then(|| {
            let mut x = self.mask_state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.mask_state = x;
            (x as u32).to_le_bytes()
        })
    }

    pub fn message(&mut self, msg: &Message) -> Vec<u8> {
        let body = msg.encode();
        match self.kind {
            WireKind::Raw => {
                let mut out = Vec::with_capacity(body.len() + 4);
                out.extend_from_slice(&(body.len() as u32).to_le_bytes());
                out.extend_from_slice(&body);
                out
            }
            WireKind::WsServer | WireKind::WsClient => {
                let mask = self.mask();
                websocket::encode_frame(websocket::OP_BINARY, &body, mask)
            }
        }
    }

    /// The answer to a WebSocket ping; nothing on raw TCP.
    pub fn pong(&mut self, payload: &[u8]) -> Vec<u8> {
        match self.kind {
            WireKind::Raw => Vec::new(),
            _ => {
                let mask = self.mask();
                websocket::encode_frame(websocket::OP_PONG, payload, mask)
            }
        }
    }

    /// A WebSocket close frame; nothing on raw TCP (closing the socket is
    /// the signal).
    pub fn close(&mut self) -> Vec<u8> {
        match self.kind {
            WireKind::Raw => Vec::new(),
            _ => {
                let mask = self.mask();
                websocket::encode_frame(websocket::OP_CLOSE, &[], mask)
            }
        }
    }
}

/// One thing read off a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireEvent {
    Message(Message),
    /// A WebSocket ping, to be answered with [`WireEncoder::pong`].
    Ping(Vec<u8>),
    /// The peer closed the WebSocket.
    Close,
}

/// Decodes incoming bytes for one stream.
#[derive(Debug)]
pub enum WireDecoder {
    Raw(Vec<u8>),
    Ws(FrameDecoder),
}

impl WireDecoder {
    pub fn new(kind: WireKind) -> Self {
        match kind {
            WireKind::Raw => Self::Raw(Vec::new()),
            WireKind::WsServer => Self::Ws(FrameDecoder::server(MAX_MESSAGE_LEN)),
            WireKind::WsClient => Self::Ws(FrameDecoder::client(MAX_MESSAGE_LEN)),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        match self {
            Self::Raw(buf) => buf.extend_from_slice(bytes),
            Self::Ws(dec) => dec.push(bytes),
        }
    }

    /// The next event, `Ok(None)` when more bytes are needed.
    pub fn next_event(&mut self) -> Result<Option<WireEvent>, WireError> {
        match self {
            Self::Raw(buf) => {
                if buf.len() < 4 {
                    return Ok(None);
                }
                let len = u32::from_le_bytes(buf[..4].try_into().expect("4 bytes")) as usize;
                if len > MAX_MESSAGE_LEN {
                    return Err(WireError::TooLarge(len));
                }
                if buf.len() < 4 + len {
                    return Ok(None);
                }
                let msg = Message::decode(&buf[4..4 + len])?;
                buf.drain(..4 + len);
                Ok(Some(WireEvent::Message(msg)))
            }
            Self::Ws(dec) => Ok(match dec.next_message()? {
                None => None,
                Some(WsMessage::Binary(body)) => Some(WireEvent::Message(Message::decode(&body)?)),
                Some(WsMessage::Ping(p)) => Some(WireEvent::Ping(p)),
                // An unsolicited pong is allowed and means nothing; read on.
                Some(WsMessage::Pong(_)) => return self.next_event(),
                Some(WsMessage::Close) => Some(WireEvent::Close),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_pairs_with_its_peer() {
        let msg = Message::Chat {
            from: crate::protocol::PlayerId(3),
            text: "hello".into(),
        };
        for (send, recv) in [
            (WireKind::Raw, WireKind::Raw),
            (WireKind::WsClient, WireKind::WsServer),
            (WireKind::WsServer, WireKind::WsClient),
        ] {
            let mut enc = WireEncoder::new(send, 42);
            let mut dec = WireDecoder::new(recv);
            let a = enc.message(&msg);
            let b = enc.message(&Message::Leave);
            // Split across pushes at an awkward point.
            let mut all = a;
            all.extend(b);
            dec.push(&all[..3]);
            assert_eq!(dec.next_event().unwrap(), None);
            dec.push(&all[3..]);
            assert_eq!(
                dec.next_event().unwrap(),
                Some(WireEvent::Message(msg.clone()))
            );
            assert_eq!(
                dec.next_event().unwrap(),
                Some(WireEvent::Message(Message::Leave))
            );
            assert_eq!(dec.next_event().unwrap(), None);
        }
    }

    #[test]
    fn raw_length_over_cap_is_refused_before_buffering() {
        let mut dec = WireDecoder::new(WireKind::Raw);
        dec.push(&u32::MAX.to_le_bytes());
        assert_eq!(
            dec.next_event(),
            Err(WireError::TooLarge(u32::MAX as usize))
        );
    }
}
