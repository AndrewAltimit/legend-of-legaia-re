//! A relay driven over caller-supplied transports, polled from the caller's
//! loop: the test harness for multi-peer sessions, and a same-machine room
//! with no sockets at all.

use crate::relay::{ConnId, RelayCore, RelayLimits, RelayOutput};
use crate::transport::{NetError, Transport};

/// A [`RelayCore`] over a set of transports, one per connected client.
#[derive(Debug)]
pub struct LocalRelay<T> {
    core: RelayCore,
    conns: Vec<(ConnId, T)>,
    next_conn: u64,
}

impl<T: Transport> LocalRelay<T> {
    pub fn new(limits: RelayLimits, seed: u64) -> Self {
        Self {
            core: RelayCore::new(limits, seed),
            conns: Vec::new(),
            next_conn: 1,
        }
    }

    pub fn core(&self) -> &RelayCore {
        &self.core
    }

    /// Attaches the relay's end of a client's transport.
    pub fn add(&mut self, transport: T, now_ms: u64) -> ConnId {
        let conn = ConnId(self.next_conn);
        self.next_conn += 1;
        self.conns.push((conn, transport));
        self.core.connect(conn, now_ms);
        self.flush();
        conn
    }

    /// Reads every connection, runs the core, writes what it decided.
    pub fn pump(&mut self, now_ms: u64) {
        for i in 0..self.conns.len() {
            let conn = self.conns[i].0;
            loop {
                match self.conns[i].1.try_recv() {
                    Ok(Some(msg)) => self.core.handle(conn, msg, now_ms),
                    Ok(None) => break,
                    Err(NetError::Wire(_)) => {
                        self.core.malformed(conn);
                        break;
                    }
                    Err(_) => {
                        self.core.disconnect(conn);
                        break;
                    }
                }
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        for out in self.core.drain() {
            match out {
                RelayOutput::Send(conn, msg) => {
                    if let Some((_, t)) = self.conns.iter_mut().find(|(c, _)| *c == conn)
                        && t.send(&msg).is_err()
                    {
                        self.core.disconnect(conn);
                    }
                }
                RelayOutput::Close(conn) => {
                    if let Some(i) = self.conns.iter().position(|(c, _)| *c == conn) {
                        let (_, mut t) = self.conns.remove(i);
                        t.close();
                    }
                }
            }
        }
        // A send failure above may have queued more (the departure's
        // broadcasts); run until quiet.
        if !self.core.out.is_empty() {
            self.flush();
        }
    }
}
