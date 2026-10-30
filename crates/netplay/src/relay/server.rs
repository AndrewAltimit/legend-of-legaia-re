//! The socket relay: std threads over `std::net`, one port for raw TCP and
//! WebSocket clients alike.
//!
//! Threads: one listener, one manager (owns the [`RelayCore`]), and per
//! connection a reader and a writer. Readers decode frames and post them to
//! the manager; the manager runs the core and hands encoded bytes to each
//! connection's writer through a bounded channel - a client that stops
//! reading fills its channel and is dropped rather than growing a queue.
//! Every blocking call has a timeout and every thread polls the stop flag,
//! so [`RelayHandle::shutdown`] returns within a fraction of a second and
//! leaves no thread or listener behind.
//!
//! The relay speaks only the netplay protocol. An HTTP request that is not
//! a WebSocket upgrade gets `400` and the connection closes; there is no
//! file serving or any other path into the host.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::relay::{ConnId, RelayCore, RelayLimits, RelayOutput};
use crate::websocket;
use crate::wire::{RAW_PREAMBLE, WireDecoder, WireEncoder, WireEvent, WireKind};

/// How often blocked threads look at the stop flag.
const POLL: Duration = Duration::from_millis(50);
/// Time a new connection has to send its preamble or upgrade request.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// A write to a client that blocks this long drops the client.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Encoded messages queued per connection before it counts as stalled.
const OUTBOUND_QUEUE: usize = 1024;
/// How long a reader outlives the stop flag waiting for its socket to close.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
/// Connections one IP address may hold at once.
const MAX_CONNECTIONS_PER_IP: usize = 16;

/// Socket-level settings beside the core's [`RelayLimits`].
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub limits: RelayLimits,
    pub max_connections_per_ip: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: RelayLimits::default(),
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
        }
    }
}

enum Event {
    Connected {
        conn: ConnId,
        kind: WireKind,
        writer: SyncSender<Out>,
        socket: TcpStream,
    },
    Message(ConnId, crate::protocol::Message),
    Malformed(ConnId),
    Closed(ConnId),
}

/// The relay server entry point.
pub struct RelayServer;

impl RelayServer {
    /// Binds `addr` and starts relaying with default limits. Bind to
    /// `127.0.0.1:0` for an ephemeral port (see [`RelayHandle::local_addr`]).
    /// This is the in-process "host a world" server too: the hosting game
    /// spawns it and connects to it like any other client.
    pub fn spawn(addr: impl ToSocketAddrs) -> std::io::Result<RelayHandle> {
        Self::spawn_with(addr, ServerConfig::default())
    }

    pub fn spawn_with(
        addr: impl ToSocketAddrs,
        config: ServerConfig,
    ) -> std::io::Result<RelayHandle> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (events_tx, events_rx) = mpsc::channel();

        let seed = RandomState::new().hash_one(Instant::now());
        let core = RelayCore::new(config.limits.clone(), seed);
        let manager = {
            let stop = stop.clone();
            thread::Builder::new()
                .name("relay-manager".into())
                .spawn(move || manager_loop(core, events_rx, &stop))?
        };
        let listener_thread = {
            let stop = stop.clone();
            thread::Builder::new()
                .name("relay-listener".into())
                .spawn(move || listen_loop(listener, events_tx, &stop, &config))?
        };
        Ok(RelayHandle {
            local_addr,
            stop,
            threads: vec![listener_thread, manager],
        })
    }
}

/// A running relay. Dropping it shuts the relay down.
pub struct RelayHandle {
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl RelayHandle {
    /// The bound address (the real port when bound to port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Closes every room, drops every client, stops and joins every thread.
    pub fn shutdown(mut self) {
        self.stop_and_join();
    }

    /// Blocks until the relay stops (it only stops through another handle
    /// path - in the binary, process exit).
    pub fn wait(mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn listen_loop(
    listener: TcpListener,
    events: Sender<Event>,
    stop: &Arc<AtomicBool>,
    config: &ServerConfig,
) {
    let per_ip: Arc<Mutex<HashMap<IpAddr, usize>>> = Arc::default();
    let mut conn_threads: Vec<JoinHandle<()>> = Vec::new();
    let mut next_conn = 1u64;
    while !stop.load(Ordering::SeqCst) {
        let (stream, peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(_) => {
                // WouldBlock (nothing pending) or a transient accept error.
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        conn_threads.retain(|t| !t.is_finished());
        let ip = peer.ip();
        {
            let mut map = per_ip.lock().expect("per-ip map poisoned");
            let n = map.entry(ip).or_default();
            if *n >= config.max_connections_per_ip {
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            *n += 1;
        }
        let conn = ConnId(next_conn);
        next_conn += 1;
        let events = events.clone();
        let per_ip = per_ip.clone();
        let stop = stop.clone();
        let spawned = thread::Builder::new()
            .name(format!("relay-conn-{}", conn.0))
            .spawn(move || {
                connection(stream, conn, &events, &stop);
                let mut map = per_ip.lock().expect("per-ip map poisoned");
                if let Some(n) = map.get_mut(&ip) {
                    *n -= 1;
                    if *n == 0 {
                        map.remove(&ip);
                    }
                }
            });
        if let Ok(t) = spawned {
            conn_threads.push(t);
        }
    }
    drop(listener);
    for t in conn_threads {
        let _ = t.join();
    }
}

/// What the manager hands a connection's writer.
enum Out {
    Bytes(Vec<u8>),
    /// Write a close frame (WebSocket), then shut the socket.
    Close,
}

/// Reads the raw preamble or the WebSocket upgrade. Returns the framing
/// and any bytes that arrived after the handshake.
fn handshake(stream: &mut TcpStream, stop: &AtomicBool) -> Option<(WireKind, Vec<u8>)> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    stream.set_read_timeout(Some(POLL)).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut read_more = |buf: &mut Vec<u8>| -> Option<()> {
        loop {
            if stop.load(Ordering::SeqCst) || Instant::now() >= deadline {
                return None;
            }
            match stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    return Some(());
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) => {}
                Err(_) => return None,
            }
        }
    };
    while buf.len() < 4 {
        read_more(&mut buf)?;
    }
    if &buf[..4] == RAW_PREAMBLE {
        return Some((WireKind::Raw, buf[4..].to_vec()));
    }
    if &buf[..4] != b"GET " {
        return None;
    }
    let end = loop {
        if let Some(end) = websocket::head_end(&buf) {
            break end;
        }
        if buf.len() > websocket::MAX_HANDSHAKE_LEN {
            return None;
        }
        read_more(&mut buf)?;
    };
    match websocket::server_response(&buf[..end]) {
        Ok(response) => {
            stream.write_all(&response).ok()?;
            Some((WireKind::WsServer, buf[end..].to_vec()))
        }
        Err(_) => {
            let _ = stream.write_all(websocket::NOT_AN_UPGRADE);
            None
        }
    }
}

/// One connection's reader; spawns and joins its writer.
fn connection(mut stream: TcpStream, conn: ConnId, events: &Sender<Event>, stop: &Arc<AtomicBool>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let Some((kind, early)) = handshake(&mut stream, stop) else {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    };
    let (Ok(writer_stream), Ok(manager_socket)) = (stream.try_clone(), stream.try_clone()) else {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    };
    let (tx, rx) = mpsc::sync_channel(OUTBOUND_QUEUE);
    let writer = thread::Builder::new()
        .name(format!("relay-write-{}", conn.0))
        .spawn(move || writer_loop(writer_stream, rx, kind));
    let Ok(writer) = writer else {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    };
    let connected = events.send(Event::Connected {
        conn,
        kind,
        writer: tx.clone(),
        socket: manager_socket,
    });

    if connected.is_ok() {
        read_loop(&mut stream, conn, kind, early, events, &tx, stop);
    }
    let _ = events.send(Event::Closed(conn));
    drop(tx);
    let _ = writer.join();
    let _ = stream.shutdown(Shutdown::Both);
}

fn read_loop(
    stream: &mut TcpStream,
    conn: ConnId,
    kind: WireKind,
    early: Vec<u8>,
    events: &Sender<Event>,
    writer: &SyncSender<Out>,
    stop: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(POLL));
    let mut dec = WireDecoder::new(kind);
    // Server-side frames are unmasked, so a fresh encoder per pong is fine.
    let mut pong = WireEncoder::new(kind, 0);
    dec.push(&early);
    let mut buf = [0u8; 4096];
    let mut stop_seen: Option<Instant> = None;
    loop {
        loop {
            match dec.next_event() {
                Ok(Some(WireEvent::Message(msg))) => {
                    if events.send(Event::Message(conn, msg)).is_err() {
                        return;
                    }
                }
                Ok(Some(WireEvent::Ping(payload))) => {
                    if writer.try_send(Out::Bytes(pong.pong(&payload))).is_err() {
                        return;
                    }
                }
                Ok(Some(WireEvent::Close)) => return,
                Ok(None) => break,
                Err(_) => {
                    let _ = events.send(Event::Malformed(conn));
                    return;
                }
            }
        }
        // On shutdown the manager closes every registered connection
        // through its writer (so the room-closed notice is delivered first)
        // and the socket closing ends this loop. The grace period covers a
        // connection that registered too late for that.
        if stop.load(Ordering::SeqCst) {
            let seen = *stop_seen.get_or_insert_with(Instant::now);
            if seen.elapsed() > SHUTDOWN_GRACE {
                return;
            }
        }
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => dec.push(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => return,
        }
    }
}

/// Writes until told to close, the socket fails, or every sender (the
/// manager's and the reader's) is gone.
fn writer_loop(mut stream: TcpStream, rx: Receiver<Out>, kind: WireKind) {
    let mut enc = WireEncoder::new(kind, 0);
    loop {
        match rx.recv() {
            Ok(Out::Bytes(bytes)) => {
                if stream.write_all(&bytes).is_err() {
                    break;
                }
            }
            Ok(Out::Close) => {
                let _ = stream.write_all(&enc.close());
                break;
            }
            Err(_) => break,
        }
    }
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

struct ManagedConn {
    enc: WireEncoder,
    writer: SyncSender<Out>,
    socket: TcpStream,
}

fn manager_loop(mut core: RelayCore, events: Receiver<Event>, stop: &AtomicBool) {
    let start = Instant::now();
    let mut conns: HashMap<ConnId, ManagedConn> = HashMap::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            core.shutdown();
            apply(&mut core, &mut conns);
            break;
        }
        let now = start.elapsed().as_millis() as u64;
        match events.recv_timeout(POLL) {
            Ok(Event::Connected {
                conn,
                kind,
                writer,
                socket,
            }) => {
                conns.insert(
                    conn,
                    ManagedConn {
                        enc: WireEncoder::new(kind, 0),
                        writer,
                        socket,
                    },
                );
                core.connect(conn, now);
            }
            Ok(Event::Message(conn, msg)) => core.handle(conn, msg, now),
            Ok(Event::Malformed(conn)) => core.malformed(conn),
            Ok(Event::Closed(conn)) => {
                core.disconnect(conn);
                conns.remove(&conn);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        apply(&mut core, &mut conns);
    }
    for (_, c) in conns.drain() {
        let _ = c.socket.shutdown(Shutdown::Both);
    }
}

/// Carries out the core's decisions until it has nothing more to say (a
/// stalled client's removal produces more broadcasts).
fn apply(core: &mut RelayCore, conns: &mut HashMap<ConnId, ManagedConn>) {
    loop {
        let outputs = core.drain();
        if outputs.is_empty() {
            return;
        }
        for out in outputs {
            match out {
                RelayOutput::Send(conn, msg) => {
                    let Some(c) = conns.get_mut(&conn) else {
                        continue;
                    };
                    let bytes = c.enc.message(&msg);
                    if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
                        c.writer.try_send(Out::Bytes(bytes))
                    {
                        let c = conns.remove(&conn).expect("present above");
                        let _ = c.socket.shutdown(Shutdown::Both);
                        core.disconnect(conn);
                    }
                }
                RelayOutput::Close(conn) => {
                    if let Some(c) = conns.remove(&conn)
                        && c.writer.try_send(Out::Close).is_err()
                    {
                        let _ = c.socket.shutdown(Shutdown::Both);
                    }
                }
            }
        }
    }
}
