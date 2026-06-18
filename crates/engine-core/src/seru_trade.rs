//! Runtime seru trading: the engine side of the randomizer's `--seru-trade`
//! toggle. A vendor offers to swap one of a character's seru for a different
//! one; the offers reseed every two in-game hours.
//!
//! The offer table itself isn't stored — it's recomputed on demand from the
//! shared kernel [`legaia_asset::seru_trade`] using `(master seed, vendor id,
//! play-time bucket, the party's currently-owned seru)`. The randomizer embeds
//! only the master seed (+ enabled flag + offer cap) in the disc;
//! [`crate::World::install_seru_trade_config`] reads it at boot, and this module
//! turns it into the live trade UI's state ([`SeruTradeSession`]) and performs
//! the swap on the character spell lists.
//!
//! "Owning" a seru here means the spell id sits in a character record's spell
//! list (`+0x13D`, [`legaia_save::SpellList`]); the tradeable id space is the
//! player Seru-magic block ([`legaia_asset::seru_trade::SERU_POOL_START`]
//! `..=`[`legaia_asset::seru_trade::SERU_POOL_END`]), the same ids
//! [`legaia_asset::spell_names`] names. A trade rewrites the owner's spell list
//! in place, so the new seru is castable the next time a battle loads the party.

use legaia_asset::seru_trade::{self, OwnedSeru, SeruTradeConfig, TradeOffer};
use legaia_save::CharacterRecord;

/// Whether `id` is a tradeable player seru (the Seru-magic block).
pub fn is_tradeable_seru(id: u8) -> bool {
    (seru_trade::SERU_POOL_START..=seru_trade::SERU_POOL_END).contains(&id)
}

/// Enumerate every tradeable seru currently owned across `party`, tagged with
/// the roster slot of the character who holds it. Order is party slot, then the
/// character's own spell-list order.
pub fn party_owned_seru(party: &[CharacterRecord]) -> Vec<OwnedSeru> {
    let mut out = Vec::new();
    for (slot, ch) in party.iter().enumerate() {
        let list = ch.spell_list();
        for i in 0..list.count as usize {
            let id = list.ids[i];
            if is_tradeable_seru(id) {
                out.push(OwnedSeru {
                    seru_id: id,
                    owner_slot: slot as u8,
                    level: list.levels[i],
                });
            }
        }
    }
    out
}

/// Outcome of attempting a trade against the live party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeResult {
    /// The owner gave `given` and now owns `received` instead.
    Swapped {
        /// Roster slot whose spell list changed.
        owner_slot: u8,
        /// Seru id removed.
        given: u8,
        /// Seru id added.
        received: u8,
    },
    /// The owner no longer holds the seru the offer wanted (stale offer / the
    /// party changed since the offer was generated). Nothing was modified.
    GiveNotOwned,
    /// The offer's owner slot is out of range for this party.
    BadOwner,
}

/// Apply `offer` to `party`: remove one instance of the given seru from the
/// owner's spell list and add the received seru.
///
/// If the owner already owns the received seru, the given slot is simply removed
/// (no duplicate is created); otherwise the given slot is rewritten to the
/// received id with a fresh level byte. Returns what happened; on anything but
/// [`TradeResult::Swapped`] the party is left untouched.
pub fn apply_trade(party: &mut [CharacterRecord], offer: &TradeOffer) -> TradeResult {
    let owner = offer.give.owner_slot as usize;
    let Some(ch) = party.get_mut(owner) else {
        return TradeResult::BadOwner;
    };
    let mut list = ch.spell_list();
    let count = list.count as usize;
    let Some(pos) = list.ids[..count]
        .iter()
        .position(|&id| id == offer.give.seru_id)
    else {
        return TradeResult::GiveNotOwned;
    };

    let already_has_receive = list.ids[..count].contains(&offer.receive_seru_id);
    if already_has_receive {
        // Remove the given slot (compact left), preserving the parallel level
        // array, and drop the count by one.
        for i in pos..count - 1 {
            list.ids[i] = list.ids[i + 1];
            list.levels[i] = list.levels[i + 1];
        }
        list.ids[count - 1] = 0;
        list.levels[count - 1] = 0;
        list.count -= 1;
    } else {
        // Replace in place with the received seru at a fresh level.
        list.ids[pos] = offer.receive_seru_id;
        list.levels[pos] = 0;
    }

    ch.set_spell_list(list);
    TradeResult::Swapped {
        owner_slot: offer.give.owner_slot,
        given: offer.give.seru_id,
        received: offer.receive_seru_id,
    }
}

/// Live state of an open trade menu at one vendor.
///
/// The host drives it: move the cursor over [`offers`](Self::offers), open the
/// yes/no confirm, and on a confirmed "yes" call [`take_confirmed`](Self::take_confirmed)
/// to get the offer to apply (via [`apply_trade`]). After a successful trade the
/// host calls [`refresh`](Self::refresh) so the offer list reflects the new
/// owned set; [`refresh`] also reseeds the offers when the play-time bucket has
/// advanced (every two in-game hours).
#[derive(Debug, Clone)]
pub struct SeruTradeSession {
    /// The disc-embedded config (master seed + offer cap).
    pub config: SeruTradeConfig,
    /// Which vendor this session belongs to (seeds the offer generator).
    pub vendor_id: u16,
    /// The play-time bucket the current offers were generated for.
    pub time_bucket: u32,
    /// The trades offered this bucket.
    pub offers: Vec<TradeOffer>,
    /// Highlighted offer index (clamped to `offers`).
    pub cursor: usize,
    /// Whether the yes/no confirm overlay is open over the highlighted offer.
    pub confirming: bool,
    /// Cursor within the yes/no overlay (`true` = "Yes").
    pub confirm_yes: bool,
}

impl SeruTradeSession {
    /// Open a trade session at `vendor_id` for the current `party` and
    /// `play_time_seconds`.
    pub fn open(
        config: SeruTradeConfig,
        vendor_id: u16,
        play_time_seconds: u32,
        party: &[CharacterRecord],
    ) -> Self {
        let owned = party_owned_seru(party);
        let offers = seru_trade::offers_at(&config, vendor_id, play_time_seconds, &owned);
        Self {
            config,
            vendor_id,
            time_bucket: seru_trade::time_bucket(play_time_seconds),
            offers,
            cursor: 0,
            confirming: false,
            confirm_yes: false,
        }
    }

    /// Recompute the offers for the current `party` + `play_time_seconds`. The
    /// offer set changes when the party's owned seru change (after a trade) or
    /// when the play-time crosses a two-hour boundary (the reseed). Closes any
    /// open confirm and clamps the cursor.
    pub fn refresh(&mut self, play_time_seconds: u32, party: &[CharacterRecord]) {
        let owned = party_owned_seru(party);
        self.time_bucket = seru_trade::time_bucket(play_time_seconds);
        self.offers =
            seru_trade::offers_at(&self.config, self.vendor_id, play_time_seconds, &owned);
        self.confirming = false;
        self.confirm_yes = false;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.offers.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.offers.len() {
            self.cursor = self.offers.len() - 1;
        }
    }

    /// `true` when the vendor has no trades to offer (empty list).
    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    /// Move the highlight by `delta`, wrapping around the offer list. No-op while
    /// the confirm overlay is open or when there are no offers.
    pub fn move_cursor(&mut self, delta: i32) {
        if self.confirming || self.offers.is_empty() {
            return;
        }
        let n = self.offers.len() as i32;
        self.cursor = (((self.cursor as i32 + delta) % n + n) % n) as usize;
    }

    /// The currently-highlighted offer, if any.
    pub fn selected(&self) -> Option<&TradeOffer> {
        self.offers.get(self.cursor)
    }

    /// Open the yes/no confirm over the highlighted offer (defaulting to "No",
    /// matching the retail shop confirm). No-op when there's nothing to confirm.
    pub fn begin_confirm(&mut self) {
        if self.selected().is_some() {
            self.confirming = true;
            self.confirm_yes = false;
        }
    }

    /// Toggle the yes/no cursor (no-op unless confirming).
    pub fn toggle_confirm(&mut self) {
        if self.confirming {
            self.confirm_yes = !self.confirm_yes;
        }
    }

    /// Close the confirm overlay without trading.
    pub fn cancel_confirm(&mut self) {
        self.confirming = false;
        self.confirm_yes = false;
    }

    /// If the confirm overlay is open on "Yes", close it and return the offer to
    /// apply (the host then calls [`apply_trade`] and [`refresh`]). Returns
    /// `None` otherwise (still picking, or sitting on "No").
    pub fn take_confirmed(&mut self) -> Option<TradeOffer> {
        if self.confirming && self.confirm_yes {
            let offer = self.selected().copied();
            self.confirming = false;
            self.confirm_yes = false;
            return offer;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::SpellList;

    fn ch_with_spells(ids: &[u8]) -> CharacterRecord {
        let mut r = CharacterRecord::zeroed();
        let mut list = SpellList::default();
        for (i, &id) in ids.iter().enumerate() {
            list.ids[i] = id;
            list.levels[i] = 1;
        }
        list.count = ids.len() as u8;
        r.set_spell_list(list);
        r
    }

    #[test]
    fn owned_enumeration_tags_owner_and_filters_pool() {
        let party = vec![
            ch_with_spells(&[0x81, 0x05, 0x88]), // 0x05 is not a tradeable seru
            ch_with_spells(&[0x90]),
        ];
        let owned = party_owned_seru(&party);
        assert_eq!(
            owned,
            vec![
                OwnedSeru {
                    seru_id: 0x81,
                    owner_slot: 0,
                    level: 1
                },
                OwnedSeru {
                    seru_id: 0x88,
                    owner_slot: 0,
                    level: 1
                },
                OwnedSeru {
                    seru_id: 0x90,
                    owner_slot: 1,
                    level: 1
                },
            ]
        );
    }

    #[test]
    fn trade_replaces_in_place_when_receive_is_new() {
        let mut party = vec![ch_with_spells(&[0x81, 0x85])];
        let offer = TradeOffer {
            give: OwnedSeru {
                seru_id: 0x81,
                owner_slot: 0,
                level: 0,
            },
            receive_seru_id: 0x90,
        };
        assert_eq!(
            apply_trade(&mut party, &offer),
            TradeResult::Swapped {
                owner_slot: 0,
                given: 0x81,
                received: 0x90
            }
        );
        let list = party[0].spell_list();
        assert_eq!(list.count, 2);
        assert_eq!(&list.ids[..2], &[0x90, 0x85]);
        assert_eq!(list.levels[0], 0, "new seru starts at level 0");
    }

    #[test]
    fn trade_compacts_when_receive_already_owned() {
        let mut party = vec![ch_with_spells(&[0x81, 0x90, 0x85])];
        let offer = TradeOffer {
            give: OwnedSeru {
                seru_id: 0x81,
                owner_slot: 0,
                level: 0,
            },
            receive_seru_id: 0x90, // already owned
        };
        assert!(matches!(
            apply_trade(&mut party, &offer),
            TradeResult::Swapped { .. }
        ));
        let list = party[0].spell_list();
        assert_eq!(list.count, 2, "no duplicate created");
        assert_eq!(&list.ids[..2], &[0x90, 0x85]);
    }

    #[test]
    fn trade_rejects_stale_or_bad_owner() {
        let mut party = vec![ch_with_spells(&[0x85])];
        let stale = TradeOffer {
            give: OwnedSeru {
                seru_id: 0x81,
                owner_slot: 0,
                level: 0,
            },
            receive_seru_id: 0x90,
        };
        assert_eq!(apply_trade(&mut party, &stale), TradeResult::GiveNotOwned);
        let bad = TradeOffer {
            give: OwnedSeru {
                seru_id: 0x85,
                owner_slot: 9,
                level: 0,
            },
            receive_seru_id: 0x90,
        };
        assert_eq!(apply_trade(&mut party, &bad), TradeResult::BadOwner);
        assert_eq!(party[0].spell_list().count, 1, "party untouched on failure");
    }

    #[test]
    fn session_confirm_flow_yields_offer_then_applies() {
        let config = SeruTradeConfig {
            enabled: true,
            seed: 0xABCD,
            max_offers: 4,
        };
        let mut party = vec![
            ch_with_spells(&[0x81, 0x82, 0x83]),
            ch_with_spells(&[0x90, 0x91]),
        ];
        let mut s = SeruTradeSession::open(config, 1, 0, &party);
        assert!(!s.is_empty(), "party owns seru, so offers exist");

        // Sitting on "No" yields nothing; "Yes" yields the highlighted offer.
        s.begin_confirm();
        assert!(s.take_confirmed().is_none());
        s.toggle_confirm();
        let offer = s.take_confirmed().expect("confirmed yes");

        let before: usize = party.iter().map(|c| c.spell_list().count as usize).sum();
        assert!(matches!(
            apply_trade(&mut party, &offer),
            TradeResult::Swapped { .. }
        ));
        s.refresh(0, &party);
        // The swapped-out seru is gone from its owner (count same or -1).
        let after: usize = party.iter().map(|c| c.spell_list().count as usize).sum();
        assert!(after <= before);
    }

    #[test]
    fn refresh_reseeds_across_two_hour_boundary() {
        let config = SeruTradeConfig {
            enabled: true,
            seed: 7,
            max_offers: 4,
        };
        let party = vec![ch_with_spells(&[0x81, 0x85, 0x88, 0x8C, 0x90])];
        let mut s = SeruTradeSession::open(config, 3, 0, &party);
        let b0 = s.offers.clone();
        assert_eq!(s.time_bucket, 0);
        // Advance several buckets; the offers should change at some point.
        let mut changed = false;
        for bucket in 1..12u32 {
            s.refresh(bucket * seru_trade::SECONDS_PER_RESEED, &party);
            assert_eq!(s.time_bucket, bucket);
            if s.offers != b0 {
                changed = true;
            }
        }
        assert!(changed, "offers should reseed across two-hour buckets");
    }
}
