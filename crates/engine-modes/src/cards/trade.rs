//! A card trade between two players, run by one authority.
//!
//! [`TradeOffer::execute`] swaps two collections the caller holds. Over a
//! network neither side holds both, so a [`TradeTable`] runs the trade on
//! the authority (the netplay host): the proposer states an offer, each side
//! confirms that exact revision, and the authority swaps once both have -
//! validating the offer against its own collection and the partner's last
//! reported holdings. Any new offer bumps the revision and clears both
//! confirmations, so a side never confirms terms it has not seen.
//!
//! The wire form is [`TradeMsg`]: a few message shapes with a byte codec,
//! carried as the payload of the netplay crate's host-authoritative
//! `Action` (client to host) and `Event` (host to room) messages. The netplay
//! layer needs nothing trade-specific.

use super::{CardBook, CardError, Collection, TradeOffer};

/// Which side of a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TradeSide {
    /// The side whose `give` lines leave its collection.
    Proposer = 0,
    /// The side whose `take` lines leave its collection.
    Partner = 1,
}

/// Why a table refused a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeError {
    /// A confirmation for an offer that has since changed.
    StaleRevision { have: u32, got: u32 },
    /// No offer is on the table.
    NoOffer,
    /// The table is closed (settled or cancelled).
    Closed,
    /// The offer does not hold against the collections.
    Invalid(CardError),
}

/// Where a table is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeState {
    /// Taking offers and confirmations.
    Open,
    /// The swap ran; the offer that moved.
    Settled(TradeOffer),
    /// One side walked away.
    Cancelled,
}

/// The authority's record of one trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeTable {
    offer: Option<TradeOffer>,
    revision: u32,
    confirmed: [bool; 2],
    state: TradeState,
}

impl Default for TradeTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeTable {
    pub fn new() -> Self {
        Self {
            offer: None,
            revision: 0,
            confirmed: [false; 2],
            state: TradeState::Open,
        }
    }

    pub fn offer(&self) -> Option<&TradeOffer> {
        self.offer.as_ref()
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn state(&self) -> &TradeState {
        &self.state
    }

    pub fn confirmed(&self, side: TradeSide) -> bool {
        self.confirmed[side as usize]
    }

    /// Put `offer` on the table; returns its revision. Both confirmations
    /// clear.
    pub fn propose(&mut self, offer: TradeOffer) -> Result<u32, TradeError> {
        if self.state != TradeState::Open {
            return Err(TradeError::Closed);
        }
        self.revision = self.revision.wrapping_add(1);
        self.offer = Some(offer);
        self.confirmed = [false; 2];
        Ok(self.revision)
    }

    /// `side` accepts revision `revision`. Returns whether both sides have
    /// now confirmed it.
    pub fn confirm(&mut self, side: TradeSide, revision: u32) -> Result<bool, TradeError> {
        if self.state != TradeState::Open {
            return Err(TradeError::Closed);
        }
        if self.offer.is_none() {
            return Err(TradeError::NoOffer);
        }
        if revision != self.revision {
            return Err(TradeError::StaleRevision {
                have: self.revision,
                got: revision,
            });
        }
        self.confirmed[side as usize] = true;
        Ok(self.confirmed == [true; 2])
    }

    pub fn cancel(&mut self) {
        if self.state == TradeState::Open {
            self.state = TradeState::Cancelled;
        }
    }

    /// Run the swap once both sides confirmed. Either every line moves or
    /// nothing does; a failed validation leaves the table open with the
    /// confirmations cleared, so the sides must agree again.
    pub fn settle(
        &mut self,
        proposer: &mut Collection,
        partner: &mut Collection,
        book: Option<&CardBook>,
    ) -> Result<TradeOffer, TradeError> {
        if self.state != TradeState::Open {
            return Err(TradeError::Closed);
        }
        let offer = self.offer.clone().ok_or(TradeError::NoOffer)?;
        if self.confirmed != [true; 2] {
            return Err(TradeError::NoOffer);
        }
        if let Err(e) = offer.execute(proposer, partner, book) {
            self.confirmed = [false; 2];
            return Err(TradeError::Invalid(e));
        }
        self.state = TradeState::Settled(offer.clone());
        Ok(offer)
    }
}

impl TradeOffer {
    /// Apply a settled trade to one side's own collection - what a remote
    /// side does on the authority's [`TradeMsg::Settled`]. Validated first;
    /// either every line moves or nothing does.
    pub fn apply_one_side(
        &self,
        side: TradeSide,
        own: &mut Collection,
        book: Option<&CardBook>,
    ) -> Result<(), CardError> {
        let (out, inn) = match side {
            TradeSide::Proposer => (&self.give, &self.take),
            TradeSide::Partner => (&self.take, &self.give),
        };
        let outgoing = super::merged(out);
        for (&id, &n) in &outgoing {
            if let Some(b) = book
                && b.card(id).is_none()
            {
                return Err(CardError::UnknownCard(id.to_string()));
            }
            let have = own.count(id);
            if have < n {
                return Err(CardError::NotEnough {
                    card: id.to_string(),
                    have,
                    need: n,
                });
            }
        }
        for (id, n) in outgoing {
            own.remove(id, n).expect("validated");
        }
        for (id, n) in super::merged(inn) {
            own.add(id, n);
        }
        Ok(())
    }
}

/// One trade message, the payload of a netplay `Action` / `Event`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeMsg {
    /// Client -> host: my holdings, for the host's validation.
    Holdings(Vec<(String, u32)>),
    /// Either way: this offer is on the table (the host stamps `revision`).
    Offer { revision: u32, offer: TradeOffer },
    /// Client -> host: I accept `revision`.
    Confirm { revision: u32 },
    /// Either way: walk away.
    Cancel,
    /// Host -> room: the swap ran; apply `offer` to your own side.
    Settled { revision: u32, offer: TradeOffer },
    /// Host -> room: the offer failed validation; the reason as text.
    Refused { revision: u32, reason: String },
}

const TAG_HOLDINGS: u8 = 1;
const TAG_OFFER: u8 = 2;
const TAG_CONFIRM: u8 = 3;
const TAG_CANCEL: u8 = 4;
const TAG_SETTLED: u8 = 5;
const TAG_REFUSED: u8 = 6;

/// The holdings line list of a collection, in id order.
pub fn holdings(c: &Collection) -> Vec<(String, u32)> {
    c.counts.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

/// A collection rebuilt from holdings lines (binder in line order).
pub fn from_holdings(lines: &[(String, u32)]) -> Collection {
    let mut c = Collection::new();
    for (id, n) in lines {
        c.add(id, *n);
    }
    c
}

impl TradeMsg {
    /// The payload bytes: `u8` tag, then the fields; strings and lists are a
    /// `u16` length first, integers little-endian.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::new();
        match self {
            Self::Holdings(lines) => {
                w.push(TAG_HOLDINGS);
                put_lines(&mut w, lines);
            }
            Self::Offer { revision, offer } => {
                w.push(TAG_OFFER);
                w.extend_from_slice(&revision.to_le_bytes());
                put_offer(&mut w, offer);
            }
            Self::Confirm { revision } => {
                w.push(TAG_CONFIRM);
                w.extend_from_slice(&revision.to_le_bytes());
            }
            Self::Cancel => w.push(TAG_CANCEL),
            Self::Settled { revision, offer } => {
                w.push(TAG_SETTLED);
                w.extend_from_slice(&revision.to_le_bytes());
                put_offer(&mut w, offer);
            }
            Self::Refused { revision, reason } => {
                w.push(TAG_REFUSED);
                w.extend_from_slice(&revision.to_le_bytes());
                put_str(&mut w, reason);
            }
        }
        w
    }

    /// Decode a payload; `None` for anything malformed or with trailing
    /// bytes.
    pub fn decode(b: &[u8]) -> Option<Self> {
        let mut r = Reader { b, p: 0 };
        let msg = match r.u8()? {
            TAG_HOLDINGS => Self::Holdings(r.lines()?),
            TAG_OFFER => Self::Offer {
                revision: r.u32()?,
                offer: r.offer()?,
            },
            TAG_CONFIRM => Self::Confirm { revision: r.u32()? },
            TAG_CANCEL => Self::Cancel,
            TAG_SETTLED => Self::Settled {
                revision: r.u32()?,
                offer: r.offer()?,
            },
            TAG_REFUSED => Self::Refused {
                revision: r.u32()?,
                reason: r.str()?,
            },
            _ => return None,
        };
        (r.p == b.len()).then_some(msg)
    }
}

const MAX: usize = u16::MAX as usize;

fn put_str(w: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(MAX);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    w.extend_from_slice(&(end as u16).to_le_bytes());
    w.extend_from_slice(&s.as_bytes()[..end]);
}

fn put_lines(w: &mut Vec<u8>, lines: &[(String, u32)]) {
    let n = lines.len().min(MAX);
    w.extend_from_slice(&(n as u16).to_le_bytes());
    for (id, c) in &lines[..n] {
        put_str(w, id);
        w.extend_from_slice(&c.to_le_bytes());
    }
}

fn put_offer(w: &mut Vec<u8>, o: &TradeOffer) {
    put_lines(w, &o.give);
    put_lines(w, &o.take);
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = usize::from(self.u16()?);
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn lines(&mut self) -> Option<Vec<(String, u32)>> {
        (0..self.u16()?)
            .map(|_| Some((self.str()?, self.u32()?)))
            .collect()
    }
    fn offer(&mut self) -> Option<TradeOffer> {
        Some(TradeOffer {
            give: self.lines()?,
            take: self.lines()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(lines: &[(&str, u32)]) -> Collection {
        let mut c = Collection::new();
        for (id, n) in lines {
            c.add(id, *n);
        }
        c
    }

    fn offer() -> TradeOffer {
        TradeOffer {
            give: vec![("a".into(), 1)],
            take: vec![("b".into(), 2)],
        }
    }

    #[test]
    fn both_sides_confirm_the_same_revision_before_the_swap() {
        let (mut me, mut you) = (col(&[("a", 1)]), col(&[("b", 2)]));
        let mut t = TradeTable::new();
        assert_eq!(t.settle(&mut me, &mut you, None), Err(TradeError::NoOffer));
        let r1 = t.propose(offer()).unwrap();
        assert_eq!(t.confirm(TradeSide::Proposer, r1), Ok(false));
        // A changed offer clears the earlier confirmation.
        let r2 = t.propose(offer()).unwrap();
        assert!(!t.confirmed(TradeSide::Proposer));
        assert_eq!(
            t.confirm(TradeSide::Partner, r1),
            Err(TradeError::StaleRevision { have: r2, got: r1 })
        );
        assert_eq!(t.confirm(TradeSide::Partner, r2), Ok(false));
        assert_eq!(t.confirm(TradeSide::Proposer, r2), Ok(true));
        assert_eq!(t.settle(&mut me, &mut you, None), Ok(offer()));
        assert_eq!((me.count("b"), you.count("a")), (2, 1));
        assert!(matches!(t.state(), TradeState::Settled(_)));
        assert_eq!(t.propose(offer()), Err(TradeError::Closed));
    }

    #[test]
    fn a_failed_validation_moves_nothing_and_reopens_confirmation() {
        let (mut me, mut you) = (col(&[("a", 1)]), col(&[("b", 1)]));
        let mut t = TradeTable::new();
        let r = t.propose(offer()).unwrap();
        t.confirm(TradeSide::Proposer, r).unwrap();
        t.confirm(TradeSide::Partner, r).unwrap();
        let before = (me.clone(), you.clone());
        assert!(matches!(
            t.settle(&mut me, &mut you, None),
            Err(TradeError::Invalid(CardError::NotEnough { .. }))
        ));
        assert_eq!((me, you), before);
        assert!(!t.confirmed(TradeSide::Proposer));
        assert_eq!(*t.state(), TradeState::Open);
    }

    #[test]
    fn each_side_applies_its_half_of_a_settled_trade() {
        let (mut me, mut you) = (col(&[("a", 1)]), col(&[("b", 2)]));
        let o = offer();
        o.apply_one_side(TradeSide::Proposer, &mut me, None)
            .unwrap();
        o.apply_one_side(TradeSide::Partner, &mut you, None)
            .unwrap();
        assert_eq!((me.count("a"), me.count("b")), (0, 2));
        assert_eq!((you.count("a"), you.count("b")), (1, 0));
        // Short on the outgoing side: nothing moves.
        let before = me.clone();
        assert!(
            o.apply_one_side(TradeSide::Proposer, &mut me, None)
                .is_err()
        );
        assert_eq!(me, before);
    }

    #[test]
    fn messages_round_trip_through_the_codec() {
        let msgs = [
            TradeMsg::Holdings(holdings(&col(&[("x", 3), ("y", 1)]))),
            TradeMsg::Offer {
                revision: 7,
                offer: offer(),
            },
            TradeMsg::Confirm { revision: 7 },
            TradeMsg::Cancel,
            TradeMsg::Settled {
                revision: 7,
                offer: offer(),
            },
            TradeMsg::Refused {
                revision: 7,
                reason: "short".into(),
            },
        ];
        for m in msgs {
            let b = m.encode();
            assert_eq!(TradeMsg::decode(&b), Some(m.clone()));
            assert_eq!(TradeMsg::decode(&b[..b.len() - 1]), None, "{m:?}");
        }
        assert_eq!(TradeMsg::decode(&[0]), None);
        assert_eq!(TradeMsg::decode(&[TAG_CANCEL, 0]), None);
        assert_eq!(from_holdings(&holdings(&col(&[("x", 3)]))).count("x"), 3);
    }
}
