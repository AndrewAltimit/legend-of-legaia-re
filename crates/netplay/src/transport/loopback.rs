//! An in-memory link with simulated network conditions, on a virtual clock
//! the test (or a local-multiplayer host) advances.
//!
//! Every message goes through the real codec, so a loopback session
//! exercises the same bytes a socket carries. Each direction delays a
//! message by `latency` plus a seeded random `0..=jitter` ticks and delivers
//! in arrival-time order, so jitter reorders. `input_drop_per_mille` drops
//! [`Message::Input`] packets only: it models a future unreliable input
//! channel, which the lockstep core's input redundancy and stall resend are
//! built to survive. Lobby and session control stay reliable.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::protocol::Message;
use crate::transport::{NetError, Transport};

/// A shared virtual clock, in ticks (a test typically uses one per frame).
#[derive(Debug, Clone, Default)]
pub struct SimClock(Arc<AtomicU64>);

impl SimClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn now(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn advance(&self, ticks: u64) {
        self.0.fetch_add(ticks, Ordering::Relaxed);
    }
}

/// One direction's network conditions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkConditions {
    /// Fixed one-way delay, ticks.
    pub latency: u64,
    /// Extra random delay `0..=jitter`, ticks. Non-zero reorders.
    pub jitter: u64,
    /// Chance in 1000 that an input packet is dropped.
    pub input_drop_per_mille: u16,
}

#[derive(Debug)]
struct Pipe {
    /// `(deliver_at, sequence, encoded message)`.
    queue: Vec<(u64, u64, Vec<u8>)>,
    seq: u64,
    rng: u64,
    conditions: LinkConditions,
    closed: bool,
}

impl Pipe {
    fn new(conditions: LinkConditions, seed: u64) -> Self {
        Self {
            queue: Vec::new(),
            seq: 0,
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            conditions,
            closed: false,
        }
    }

    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

/// One end of a loopback link.
#[derive(Debug)]
pub struct Loopback {
    clock: SimClock,
    tx: Arc<Mutex<Pipe>>,
    rx: Arc<Mutex<Pipe>>,
}

/// A linked pair of ends: what `a` sends `b` receives under `a_to_b`, and
/// the reverse under `b_to_a`.
pub fn link(
    clock: &SimClock,
    a_to_b: LinkConditions,
    b_to_a: LinkConditions,
    seed: u64,
) -> (Loopback, Loopback) {
    let ab = Arc::new(Mutex::new(Pipe::new(a_to_b, seed)));
    let ba = Arc::new(Mutex::new(Pipe::new(b_to_a, seed ^ 0x5555_5555)));
    (
        Loopback {
            clock: clock.clone(),
            tx: ab.clone(),
            rx: ba.clone(),
        },
        Loopback {
            clock: clock.clone(),
            tx: ba,
            rx: ab,
        },
    )
}

impl Transport for Loopback {
    fn send(&mut self, msg: &Message) -> Result<(), NetError> {
        let mut pipe = self.tx.lock().expect("loopback pipe poisoned");
        if pipe.closed {
            return Err(NetError::Closed);
        }
        let c = pipe.conditions;
        if matches!(msg, Message::Input(_))
            && c.input_drop_per_mille > 0
            && pipe.next_rand() % 1000 < u64::from(c.input_drop_per_mille)
        {
            return Ok(());
        }
        let jitter = if c.jitter > 0 {
            pipe.next_rand() % (c.jitter + 1)
        } else {
            0
        };
        let at = self.clock.now() + c.latency + jitter;
        let seq = pipe.seq;
        pipe.seq += 1;
        pipe.queue.push((at, seq, msg.encode()));
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<Message>, NetError> {
        let mut pipe = self.rx.lock().expect("loopback pipe poisoned");
        let now = self.clock.now();
        let next = pipe
            .queue
            .iter()
            .enumerate()
            .filter(|(_, (at, _, _))| *at <= now)
            .min_by_key(|(_, (at, seq, _))| (*at, *seq))
            .map(|(i, _)| i);
        match next {
            Some(i) => {
                let (_, _, bytes) = pipe.queue.swap_remove(i);
                Message::decode(&bytes)
                    .map(Some)
                    .map_err(|e| NetError::Wire(e.into()))
            }
            None if pipe.closed && pipe.queue.is_empty() => Err(NetError::Closed),
            None => Ok(None),
        }
    }

    fn close(&mut self) {
        self.tx.lock().expect("loopback pipe poisoned").closed = true;
        self.rx.lock().expect("loopback pipe poisoned").closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_holds_messages_until_due() {
        let clock = SimClock::new();
        let c = LinkConditions {
            latency: 3,
            ..Default::default()
        };
        let (mut a, mut b) = link(&clock, c, c, 1);
        a.send(&Message::Leave).unwrap();
        clock.advance(2);
        assert_eq!(b.try_recv().unwrap(), None);
        clock.advance(1);
        assert_eq!(b.try_recv().unwrap(), Some(Message::Leave));
    }

    #[test]
    fn jitter_reorders() {
        let clock = SimClock::new();
        let c = LinkConditions {
            latency: 0,
            jitter: 20,
            input_drop_per_mille: 0,
        };
        let (mut a, mut b) = link(&clock, c, c, 7);
        for nonce in 0..64 {
            a.send(&Message::Ping { nonce }).unwrap();
        }
        clock.advance(21);
        let mut got = Vec::new();
        while let Some(Message::Ping { nonce }) = b.try_recv().unwrap() {
            got.push(nonce);
        }
        assert_eq!(got.len(), 64);
        assert!(got.windows(2).any(|w| w[0] > w[1]), "jitter should reorder");
    }

    #[test]
    fn close_is_seen_after_the_backlog() {
        let clock = SimClock::new();
        let (mut a, mut b) = link(&clock, Default::default(), Default::default(), 1);
        a.send(&Message::Leave).unwrap();
        a.close();
        assert_eq!(b.try_recv().unwrap(), Some(Message::Leave));
        assert_eq!(b.try_recv(), Err(NetError::Closed));
        assert_eq!(a.send(&Message::Leave), Err(NetError::Closed));
    }
}
