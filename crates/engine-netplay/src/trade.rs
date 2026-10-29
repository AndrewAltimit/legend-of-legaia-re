//! Card trades between two players in a room, run by the room's host.
//!
//! The rules are the card economy's [`TradeTable`]: an offer on the table,
//! each side confirming that exact revision, one all-or-nothing swap. This
//! module carries it over the room: a player's trade message travels to the
//! host as an `Action` and the host's verdicts come back to the room as
//! `Event`s, each wrapped in a [`TradeEnvelope`] naming the two players.
//!
//! - Every player reports its holdings ([`TradeMsg::Holdings`]) to the host,
//!   which relays them so a proposer can see what a partner owns.
//! - The proposer's offer confirms for the proposer; the partner confirms
//!   or cancels.
//! - With both confirmed the host settles the table against the two
//!   holdings it was last told and broadcasts [`TradeMsg::Settled`]; each
//!   side applies the offer to its **own** collection
//!   ([`legaia_engine_modes::cards::TradeOffer::apply_one_side`], which
//!   validates again), so the cards move on both worlds or on neither, and
//!   the collection is saved with the rest of the world's card progress.
//!
//! The host is a player too: its own messages go through the same handler
//! without touching the network.

use std::collections::BTreeMap;

use legaia_engine_core::world::World;
use legaia_engine_modes::cards::TradeOffer;
use legaia_engine_modes::cards::trade::{
    TradeMsg, TradeSide, TradeState, TradeTable, from_holdings, holdings,
};
use legaia_netplay::protocol::{Message, PlayerId};

use crate::mode_net::action_tag;

/// Frames between holdings checks (a changed collection is re-reported).
const HOLDINGS_EVERY: u32 = 30;

/// One trade message between two players.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeEnvelope {
    pub proposer: PlayerId,
    pub partner: PlayerId,
    pub msg: TradeMsg,
}

impl TradeEnvelope {
    /// `[TRADE tag][proposer u32 LE][partner u32 LE][trade message]`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![action_tag::TRADE];
        out.extend_from_slice(&self.proposer.0.to_le_bytes());
        out.extend_from_slice(&self.partner.0.to_le_bytes());
        out.extend(self.msg.encode());
        out
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let [action_tag::TRADE, rest @ ..] = b else {
            return None;
        };
        if rest.len() < 8 {
            return None;
        }
        let id = |i: usize| {
            PlayerId(u32::from_le_bytes([
                rest[i],
                rest[i + 1],
                rest[i + 2],
                rest[i + 3],
            ]))
        };
        Some(Self {
            proposer: id(0),
            partner: id(4),
            msg: TradeMsg::decode(&rest[8..])?,
        })
    }
}

/// An offer between this player and another, as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTrade {
    pub proposer: PlayerId,
    pub partner: PlayerId,
    pub revision: u32,
    pub offer: TradeOffer,
}

/// The host's half: the tables and everyone's last holdings.
#[derive(Debug, Clone, Default)]
pub struct TradeHost {
    holdings: BTreeMap<PlayerId, Vec<(String, u32)>>,
    tables: BTreeMap<(PlayerId, PlayerId), TradeTable>,
}

impl TradeHost {
    /// One message from `from` (the relay's stamp, or the host itself).
    /// Returns the envelopes to broadcast; a message that speaks for a
    /// player other than its sender is dropped.
    pub fn handle(&mut self, from: PlayerId, env: TradeEnvelope) -> Vec<TradeEnvelope> {
        let TradeEnvelope {
            proposer,
            partner,
            msg,
        } = env;
        let reply = |msg| TradeEnvelope {
            proposer,
            partner,
            msg,
        };
        let key = (proposer, partner);
        match msg {
            TradeMsg::Holdings(lines) if from == proposer => {
                self.holdings.insert(from, lines.clone());
                vec![reply(TradeMsg::Holdings(lines))]
            }
            TradeMsg::Offer { offer, .. } if from == proposer && proposer != partner => {
                let table = self.tables.entry(key).or_default();
                if *table.state() != TradeState::Open {
                    *table = TradeTable::new();
                }
                let Ok(revision) = table.propose(offer.clone()) else {
                    return Vec::new();
                };
                // Proposing is accepting one's own terms.
                let _ = table.confirm(TradeSide::Proposer, revision);
                vec![reply(TradeMsg::Offer { revision, offer })]
            }
            TradeMsg::Confirm { revision } if from == partner || from == proposer => {
                let side = if from == proposer {
                    TradeSide::Proposer
                } else {
                    TradeSide::Partner
                };
                let Some(table) = self.tables.get_mut(&key) else {
                    return Vec::new();
                };
                match table.confirm(side, revision) {
                    Ok(true) => {}
                    Ok(false) => return Vec::new(),
                    Err(e) => {
                        return vec![reply(TradeMsg::Refused {
                            revision,
                            reason: format!("{e:?}"),
                        })];
                    }
                }
                let mut a =
                    from_holdings(self.holdings.get(&proposer).map_or(&[][..], Vec::as_slice));
                let mut b =
                    from_holdings(self.holdings.get(&partner).map_or(&[][..], Vec::as_slice));
                match table.settle(&mut a, &mut b, None) {
                    Ok(offer) => {
                        self.tables.remove(&key);
                        self.holdings.insert(proposer, holdings(&a));
                        self.holdings.insert(partner, holdings(&b));
                        vec![reply(TradeMsg::Settled { revision, offer })]
                    }
                    Err(e) => vec![reply(TradeMsg::Refused {
                        revision,
                        reason: format!("{e:?}"),
                    })],
                }
            }
            TradeMsg::Cancel if from == partner || from == proposer => {
                self.tables.remove(&key);
                vec![reply(TradeMsg::Cancel)]
            }
            _ => Vec::new(),
        }
    }

    pub fn forget(&mut self, player: PlayerId) {
        self.holdings.remove(&player);
        self.tables.retain(|(a, b), _| *a != player && *b != player);
    }
}

/// Every player's half: what the room has said about trades, and the
/// world-side effects of a settled one.
#[derive(Debug, Clone, Default)]
pub struct TradeDesk {
    /// Everyone's last relayed holdings, this player's included.
    pub holdings: BTreeMap<PlayerId, Vec<(String, u32)>>,
    /// The offer this player is part of, if any.
    pub open: Option<OpenTrade>,
    /// Lines for the room's chat ("Traded Stone Imp for Gale Finch").
    pub notes: Vec<String>,
    reported: Option<Vec<(String, u32)>>,
    frame: u32,
}

impl TradeDesk {
    /// This frame's holdings report, when the collection changed (or was
    /// never reported). `None` without a card economy.
    pub fn report(&mut self, world: &World, me: PlayerId) -> Option<TradeEnvelope> {
        self.frame = self.frame.wrapping_add(1);
        if !world.cards_available() || !self.frame.is_multiple_of(HOLDINGS_EVERY) {
            return None;
        }
        let now = holdings(&world.expansion.quests.progress.cards.collection);
        if self.reported.as_ref() == Some(&now) {
            return None;
        }
        self.reported = Some(now.clone());
        Some(TradeEnvelope {
            proposer: me,
            partner: me,
            msg: TradeMsg::Holdings(now),
        })
    }

    /// A host verdict. Applies a settled trade this player is part of to
    /// its world's collection.
    pub fn receive(&mut self, world: &mut World, me: PlayerId, env: &TradeEnvelope) {
        let mine = env.proposer == me || env.partner == me;
        match &env.msg {
            TradeMsg::Holdings(lines) => {
                self.holdings.insert(env.proposer, lines.clone());
            }
            TradeMsg::Offer { revision, offer } if mine => {
                self.open = Some(OpenTrade {
                    proposer: env.proposer,
                    partner: env.partner,
                    revision: *revision,
                    offer: offer.clone(),
                });
            }
            TradeMsg::Settled { offer, .. } if mine => {
                let side = if env.proposer == me {
                    TradeSide::Proposer
                } else {
                    TradeSide::Partner
                };
                let book = world.card_book();
                let own = &mut world.expansion.quests.progress.cards.collection;
                let line = match offer.apply_one_side(side, own, book.as_ref()) {
                    Ok(()) => {
                        // Re-report at once: the holdings changed.
                        self.reported = None;
                        self.frame = HOLDINGS_EVERY - 1;
                        format!("Trade done: {}", describe(offer, side))
                    }
                    Err(e) => format!("Trade failed here: {e:?}"),
                };
                self.notes.push(line);
                self.open = None;
            }
            TradeMsg::Refused { reason, .. } if mine => {
                self.notes.push(format!("Trade refused: {reason}"));
                self.open = None;
            }
            TradeMsg::Cancel if mine => {
                self.notes.push("Trade cancelled".to_string());
                self.open = None;
            }
            _ => {}
        }
    }
}

/// `offer` from `side`'s point of view: "Stone Imp x1 for Gale Finch x1".
pub fn describe(offer: &TradeOffer, side: TradeSide) -> String {
    let lines = |l: &[(String, u32)]| -> String {
        l.iter()
            .map(|(id, n)| format!("{id} x{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match side {
        TradeSide::Proposer => format!("{} for {}", lines(&offer.give), lines(&offer.take)),
        TradeSide::Partner => format!("{} for {}", lines(&offer.take), lines(&offer.give)),
    }
}

/// An envelope as the `Action` a guest sends.
pub fn action(env: &TradeEnvelope) -> Message {
    Message::Action {
        from: PlayerId(0),
        payload: env.encode(),
    }
}
