//! The RFC 6455 subset the relay and the native client speak: the opening
//! handshake and binary / control frames. No extensions, no text frames, no
//! subprotocol negotiation.
//!
//! The handshake's `Sec-WebSocket-Accept` is `base64(SHA-1(key + GUID))`;
//! SHA-1 and base64 live here rather than as dependencies because that one
//! digest is all the protocol needs them for. SHA-1 here authenticates
//! nothing - it only proves the server read the client's handshake.

use std::fmt;

/// The fixed GUID RFC 6455 appends to the client key.
const HANDSHAKE_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest HTTP request or response head the handshake reads.
pub const MAX_HANDSHAKE_LEN: usize = 8 * 1024;

pub const OP_CONTINUATION: u8 = 0x0;
pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;
pub const OP_PING: u8 = 0x9;
pub const OP_PONG: u8 = 0xA;

/// SHA-1 (FIPS 180-1) of `data`.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (hv, v) in h.iter_mut().zip([a, b, c, d, e]) {
            *hv = hv.wrapping_add(v);
        }
    }

    let mut out = [0u8; 20];
    for (dst, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        dst.copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.len();
        let v = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= n {
                out.push(ALPHABET[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `Sec-WebSocket-Accept` value for a client's `Sec-WebSocket-Key`.
pub fn accept_key(client_key: &str) -> String {
    let mut input = client_key.trim().as_bytes().to_vec();
    input.extend_from_slice(HANDSHAKE_GUID.as_bytes());
    base64(&sha1(&input))
}

/// Why a handshake or frame was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsError {
    /// The HTTP head is not a WebSocket upgrade (or not the expected answer).
    BadHandshake(&'static str),
    /// A frame violating the subset (unmasked client frame, text frame,
    /// fragmented control frame, reserved bits).
    Protocol(&'static str),
    /// A message over the size cap.
    TooLarge(usize),
}

impl fmt::Display for WsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHandshake(why) => write!(f, "websocket handshake: {why}"),
            Self::Protocol(why) => write!(f, "websocket protocol: {why}"),
            Self::TooLarge(n) => write!(f, "websocket message of {n} bytes over the cap"),
        }
    }
}

impl std::error::Error for WsError {}

/// Finds the end of an HTTP head (`\r\n\r\n`), returning its length.
pub fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Server side: validates a client's upgrade request head and returns the
/// `101 Switching Protocols` response to send.
pub fn server_response(request_head: &[u8]) -> Result<Vec<u8>, WsError> {
    let head = std::str::from_utf8(request_head).map_err(|_| WsError::BadHandshake("not UTF-8"))?;
    if !head.starts_with("GET ") {
        return Err(WsError::BadHandshake("not a GET"));
    }
    let upgrade = header(head, "Upgrade").ok_or(WsError::BadHandshake("no Upgrade"))?;
    if !upgrade.eq_ignore_ascii_case("websocket") {
        return Err(WsError::BadHandshake("Upgrade is not websocket"));
    }
    let key = header(head, "Sec-WebSocket-Key").ok_or(WsError::BadHandshake("no key"))?;
    Ok(format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key)
    )
    .into_bytes())
}

/// The response a server sends to an HTTP request that is not an upgrade.
pub const NOT_AN_UPGRADE: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Client side: the upgrade request for `host` / `path` with `key`.
pub fn client_request(host: &str, path: &str, key: &str) -> Vec<u8> {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .into_bytes()
}

/// Client side: checks the server's response head against `key`.
pub fn check_server_response(response_head: &[u8], key: &str) -> Result<(), WsError> {
    let head =
        std::str::from_utf8(response_head).map_err(|_| WsError::BadHandshake("not UTF-8"))?;
    let status = head.lines().next().unwrap_or("");
    if !status.starts_with("HTTP/1.1 101") {
        return Err(WsError::BadHandshake("server did not switch protocols"));
    }
    let accept = header(head, "Sec-WebSocket-Accept").ok_or(WsError::BadHandshake("no accept"))?;
    if accept != accept_key(key) {
        return Err(WsError::BadHandshake("accept key mismatch"));
    }
    Ok(())
}

/// Encodes one unfragmented frame. A client masks (`mask = Some`), a server
/// does not.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n @ 0..=125 => out.push(mask_bit | n as u8),
        n @ 126..=0xFFFF => {
            out.push(mask_bit | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(mask_bit | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match mask {
        None => out.extend_from_slice(payload),
        Some(key) => {
            out.extend_from_slice(&key);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        }
    }
    out
}

/// A decoded WebSocket message (fragments joined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// One parsed frame and the buffer bytes it took.
#[derive(Debug)]
struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
    used: usize,
}

/// Incremental frame decoder: feed bytes as they arrive, pull messages.
#[derive(Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    /// A fragmented binary message being assembled.
    partial: Option<Vec<u8>>,
    /// A server requires masked frames; a client requires unmasked ones.
    expect_masked: bool,
    max_message: usize,
}

impl FrameDecoder {
    /// A decoder for frames a client sent (the server's side).
    pub fn server(max_message: usize) -> Self {
        Self {
            buf: Vec::new(),
            partial: None,
            expect_masked: true,
            max_message,
        }
    }

    /// A decoder for frames a server sent (the client's side).
    pub fn client(max_message: usize) -> Self {
        Self {
            expect_masked: false,
            ..Self::server(max_message)
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete message, `Ok(None)` when more bytes are needed.
    pub fn next_message(&mut self) -> Result<Option<WsMessage>, WsError> {
        loop {
            let Some(Frame {
                fin,
                opcode,
                payload,
                used,
            }) = self.parse_frame()?
            else {
                return Ok(None);
            };
            self.buf.drain(..used);
            match opcode {
                OP_PING => return Ok(Some(WsMessage::Ping(payload))),
                OP_PONG => return Ok(Some(WsMessage::Pong(payload))),
                OP_CLOSE => return Ok(Some(WsMessage::Close)),
                OP_TEXT => return Err(WsError::Protocol("text frames are not used")),
                OP_BINARY => {
                    if self.partial.is_some() {
                        return Err(WsError::Protocol("new message inside a fragmented one"));
                    }
                    if fin {
                        return Ok(Some(WsMessage::Binary(payload)));
                    }
                    self.partial = Some(payload);
                }
                OP_CONTINUATION => {
                    let partial = self
                        .partial
                        .as_mut()
                        .ok_or(WsError::Protocol("continuation without a start"))?;
                    if partial.len() + payload.len() > self.max_message {
                        return Err(WsError::TooLarge(partial.len() + payload.len()));
                    }
                    partial.extend_from_slice(&payload);
                    if fin {
                        return Ok(Some(WsMessage::Binary(
                            self.partial.take().expect("checked above"),
                        )));
                    }
                }
                _ => return Err(WsError::Protocol("reserved opcode")),
            }
        }
    }

    /// One frame off the front of the buffer.
    fn parse_frame(&self) -> Result<Option<Frame>, WsError> {
        let b = &self.buf;
        if b.len() < 2 {
            return Ok(None);
        }
        if b[0] & 0x70 != 0 {
            return Err(WsError::Protocol("reserved bits set"));
        }
        let fin = b[0] & 0x80 != 0;
        let opcode = b[0] & 0x0F;
        let masked = b[1] & 0x80 != 0;
        if masked != self.expect_masked {
            return Err(WsError::Protocol("wrong masking for this direction"));
        }
        let (len, mut pos) = match b[1] & 0x7F {
            126 => {
                if b.len() < 4 {
                    return Ok(None);
                }
                (u64::from(u16::from_be_bytes([b[2], b[3]])), 4)
            }
            127 => {
                if b.len() < 10 {
                    return Ok(None);
                }
                (
                    u64::from_be_bytes(b[2..10].try_into().expect("8 bytes")),
                    10,
                )
            }
            n => (u64::from(n), 2),
        };
        if opcode >= OP_CLOSE && (!fin || len > 125) {
            return Err(WsError::Protocol("bad control frame"));
        }
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if len > self.max_message {
            return Err(WsError::TooLarge(len));
        }
        let key = if masked {
            if b.len() < pos + 4 {
                return Ok(None);
            }
            let k: [u8; 4] = b[pos..pos + 4].try_into().expect("4 bytes");
            pos += 4;
            Some(k)
        } else {
            None
        };
        if b.len() < pos + len {
            return Ok(None);
        }
        let mut payload = b[pos..pos + len].to_vec();
        if let Some(k) = key {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= k[i % 4];
            }
        }
        Ok(Some(Frame {
            fin,
            opcode,
            payload,
            used: pos + len,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_matches_fips_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn accept_key_matches_rfc6455_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frames_round_trip_both_directions_and_all_lengths() {
        for len in [0usize, 5, 125, 126, 300, 70_000] {
            let payload: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let masked = encode_frame(OP_BINARY, &payload, Some([1, 2, 3, 4]));
            let mut dec = FrameDecoder::server(1 << 20);
            // Byte-at-a-time for the short ones: the decoder must wait.
            if len < 200 {
                for b in &masked {
                    dec.push(std::slice::from_ref(b));
                }
            } else {
                dec.push(&masked);
            }
            assert_eq!(
                dec.next_message().unwrap(),
                Some(WsMessage::Binary(payload.clone()))
            );

            let plain = encode_frame(OP_BINARY, &payload, None);
            let mut dec = FrameDecoder::client(1 << 20);
            dec.push(&plain);
            assert_eq!(
                dec.next_message().unwrap(),
                Some(WsMessage::Binary(payload))
            );
        }
    }

    #[test]
    fn server_refuses_unmasked_and_oversized_frames() {
        let mut dec = FrameDecoder::server(16);
        dec.push(&encode_frame(OP_BINARY, b"hi", None));
        assert!(dec.next_message().is_err());
        let mut dec = FrameDecoder::server(16);
        dec.push(&encode_frame(OP_BINARY, &[0; 17], Some([0; 4])));
        assert_eq!(dec.next_message(), Err(WsError::TooLarge(17)));
    }

    #[test]
    fn fragments_are_joined() {
        let mut first = encode_frame(OP_BINARY, b"ab", Some([9; 4]));
        first[0] &= 0x7F; // clear FIN
        let second = encode_frame(OP_CONTINUATION, b"cd", Some([7; 4]));
        let mut dec = FrameDecoder::server(64);
        dec.push(&first);
        dec.push(&second);
        assert_eq!(
            dec.next_message().unwrap(),
            Some(WsMessage::Binary(b"abcd".to_vec()))
        );
    }

    #[test]
    fn handshake_round_trips() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let req = client_request("127.0.0.1:1", "/legaia", key);
        let end = head_end(&req).unwrap();
        let resp = server_response(&req[..end]).unwrap();
        check_server_response(&resp, key).unwrap();
        assert!(check_server_response(&resp, "other").is_err());
        assert!(server_response(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").is_err());
    }
}
