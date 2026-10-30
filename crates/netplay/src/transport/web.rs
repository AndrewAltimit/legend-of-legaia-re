//! The browser transport: a `web_sys::WebSocket` in `arraybuffer` mode.
//!
//! The browser frames, masks and answers pings itself, so one WebSocket
//! message is one encoded [`Message`]. Its callbacks push into a queue that
//! [`Transport::try_recv`] drains from the page's frame loop; messages sent
//! before the socket opens wait in a bounded local queue.
//!
//! An `https://` page may only open `wss://` sockets (loopback excepted), so
//! a relay reached from the published site sits behind a TLS-terminating
//! proxy; see `docs/subsystems/netplay.md`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{BinaryType, CloseEvent, Event, MessageEvent, WebSocket};

use crate::protocol::{MAX_MESSAGE_LEN, Message};
use crate::transport::{NetError, Transport};

/// Messages held while the socket is still connecting.
const MAX_PENDING: usize = 256;

#[derive(Debug, Default)]
struct Shared {
    inbox: VecDeque<Vec<u8>>,
    open: bool,
    closed: bool,
    error: Option<String>,
}

/// A browser WebSocket connection to a relay.
pub struct WebSocketTransport {
    ws: WebSocket,
    shared: Rc<RefCell<Shared>>,
    pending: Vec<Vec<u8>>,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
}

impl WebSocketTransport {
    /// Starts connecting to `url` (`ws://host:port/` or `wss://...`).
    /// Returns at once; [`Self::is_open`] turns true when the socket opens.
    pub fn connect(url: &str) -> Result<Self, NetError> {
        let ws = WebSocket::new(url).map_err(|e| NetError::Io(format!("{e:?}")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let shared = Rc::new(RefCell::new(Shared::default()));

        let s = shared.clone();
        let on_open = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            s.borrow_mut().open = true;
        });
        let s = shared.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                let mut s = s.borrow_mut();
                if bytes.len() > MAX_MESSAGE_LEN {
                    s.error = Some(format!("message of {} bytes over the cap", bytes.len()));
                    s.closed = true;
                } else {
                    s.inbox.push_back(bytes);
                }
            }
        });
        let s = shared.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_: CloseEvent| {
            s.borrow_mut().closed = true;
        });
        let s = shared.clone();
        let on_error = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            let mut s = s.borrow_mut();
            s.error.get_or_insert_with(|| "websocket error".into());
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            shared,
            pending: Vec::new(),
            _on_open: on_open,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }

    /// Whether the socket has finished opening.
    pub fn is_open(&self) -> bool {
        let s = self.shared.borrow();
        s.open && !s.closed
    }

    fn flush_pending(&mut self) -> Result<(), NetError> {
        if !self.is_open() {
            return Ok(());
        }
        for bytes in self.pending.drain(..) {
            self.ws
                .send_with_u8_array(&bytes)
                .map_err(|e| NetError::Io(format!("{e:?}")))?;
        }
        Ok(())
    }
}

impl Transport for WebSocketTransport {
    fn send(&mut self, msg: &Message) -> Result<(), NetError> {
        if self.shared.borrow().closed {
            return Err(NetError::Closed);
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(NetError::Io("socket never opened".into()));
        }
        self.pending.push(msg.encode());
        self.flush_pending()
    }

    fn try_recv(&mut self) -> Result<Option<Message>, NetError> {
        self.flush_pending()?;
        let mut s = self.shared.borrow_mut();
        match s.inbox.pop_front() {
            Some(bytes) => Message::decode(&bytes)
                .map(Some)
                .map_err(|e| NetError::Wire(e.into())),
            None if s.closed => Err(s.error.take().map_or(NetError::Closed, NetError::Io)),
            None => Ok(None),
        }
    }

    fn close(&mut self) {
        let _ = self.ws.close();
        self.shared.borrow_mut().closed = true;
    }
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}
