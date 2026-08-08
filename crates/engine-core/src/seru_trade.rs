//! Runtime seru trading: the engine side of the randomizer's `--seru-trade`
//! toggle, mirroring the retail overlay's **want-a-type / offer-a-partner**
//! model. Each play-time bucket the vendor has one standing preference - it
//! wants every party-held instance of one seru type and hands back a different
//! seru at a fixed level - and the preference reseeds every bucket.
//!
//! The offer isn't stored - it's recomputed on demand from the shared kernel
//! [`legaia_asset::seru_trade`] using `(master seed, play-time bucket)`, the
//! same [`legaia_asset::seru_trade::bucket_offer`] stream the randomizer bakes
//! into the on-disc schedule the retail handler reads, so the engine and a
//! patched disc always show the same trade for the same seed + bucket. The
//! randomizer embeds only the master seed (+ enabled flag) in the disc;
//! [`crate::World::install_seru_trade_config`] reads it at boot, and this
//! module turns it into the live trade UI's state ([`SeruTradeSession`]) and
//! performs the swap on the character spell lists.
//!
//! "Owning" a seru here means the spell id sits in a character record's spell
//! list (`+0x13D`, [`legaia_save::SpellList`]); the tradeable id space is the
//! player Seru-magic block ([`legaia_asset::seru_trade::SERU_POOL_START`]
//! `..=`[`legaia_asset::seru_trade::SERU_POOL_END`]), the same ids
//! [`legaia_asset::spell_names`] names. A trade rewrites the owner's spell list
//! in place, so the new seru is castable the next time a battle loads the party.

use legaia_asset::seru_trade::{self, BucketOffer, OwnedSeru, OwnerTrade, SeruTradeConfig};
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

/// Apply `trade` to `party`: remove the given seru from the owner's spell list
/// and add the received seru **at the offer's level** - the level shown in the
/// UI before confirming, and the byte the retail handler writes into the level
/// array (`+0x161`).
///
/// [`legaia_asset::seru_trade::expand_offers`] already filters out owners who
/// hold the received seru, so the normal path is the in-place replace; if a
/// stale trade slips through with the received seru already owned, the given
/// slot is compact-removed instead (no duplicate is created). Returns what
/// happened; on anything but [`TradeResult::Swapped`] the party is untouched.
pub fn apply_trade(party: &mut [CharacterRecord], trade: &OwnerTrade) -> TradeResult {
    let owner = trade.owner_slot as usize;
    let Some(ch) = party.get_mut(owner) else {
        return TradeResult::BadOwner;
    };
    let mut list = ch.spell_list();
    let count = list.count as usize;
    let Some(pos) = list.ids[..count]
        .iter()
        .position(|&id| id == trade.given_id)
    else {
        return TradeResult::GiveNotOwned;
    };

    let already_has_receive = list.ids[..count].contains(&trade.received_id);
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
        // Replace in place with the received seru at the offered level.
        list.ids[pos] = trade.received_id;
        list.levels[pos] = trade.received_level;
    }

    ch.set_spell_list(list);
    TradeResult::Swapped {
        owner_slot: trade.owner_slot,
        given: trade.given_id,
        received: trade.received_id,
    }
}

/// Live state of an open trade menu at one vendor.
///
/// The host drives it: move the cursor over [`offers`](Self::offers), open the
/// yes/no confirm, and on a confirmed "yes" call [`take_confirmed`](Self::take_confirmed)
/// to get the trade to apply (via [`apply_trade`]). After a successful trade the
/// host calls [`refresh`](Self::refresh) so the line list reflects the new
/// owned set; [`refresh`] also reseeds the offer when the play-time bucket has
/// advanced. The bucket's standing [`offer`](Self::offer) is always present -
/// even with no qualifying owner - so the UI can name both sides of the trade
/// and say "No `<want>` available to trade for `<give>`" when
/// [`offers`](Self::offers) is empty.
#[derive(Debug, Clone)]
pub struct SeruTradeSession {
    /// The disc-embedded config (master seed + offer cap).
    pub config: SeruTradeConfig,
    /// This vendor's phase offset into the bucket schedule
    /// ([`legaia_asset::seru_trade::vendor_bucket_offset`], summed from the
    /// shop's stock + name exactly as the retail handler sums the armed
    /// op-`0x49` record) - so each trader shows its own offer at any given
    /// play time while the disc stores one schedule.
    pub vendor_offset: u8,
    /// The play-time bucket the current offer was generated for.
    pub time_bucket: u32,
    /// The bucket's standing preference: wanted seru, give-back seru, and the
    /// level the give-back comes at.
    pub offer: BucketOffer,
    /// One selectable line per party member who owns the wanted seru (and does
    /// not already own the give-back).
    pub offers: Vec<OwnerTrade>,
    /// Highlighted line index (clamped to `offers`).
    pub cursor: usize,
    /// Whether the yes/no confirm overlay is open over the highlighted line.
    pub confirming: bool,
    /// Cursor within the yes/no overlay (`true` = "Yes").
    pub confirm_yes: bool,
}

impl SeruTradeSession {
    /// Open a trade session at a vendor (identified by its schedule phase
    /// `vendor_offset`) for the current `party` and `play_time_seconds`.
    pub fn open(
        config: SeruTradeConfig,
        vendor_offset: u8,
        play_time_seconds: u32,
        party: &[CharacterRecord],
    ) -> Self {
        let (offer, offers) = Self::compute(&config, vendor_offset, play_time_seconds, party);
        Self {
            config,
            vendor_offset,
            time_bucket: seru_trade::time_bucket(play_time_seconds),
            offer,
            offers,
            cursor: 0,
            confirming: false,
            confirm_yes: false,
        }
    }

    /// The vendor's bucket offer + its per-owner expansion for a time + party.
    fn compute(
        config: &SeruTradeConfig,
        vendor_offset: u8,
        play_time_seconds: u32,
        party: &[CharacterRecord],
    ) -> (BucketOffer, Vec<OwnerTrade>) {
        let offer = seru_trade::bucket_offer(
            config.seed,
            seru_trade::bucket_index_for(play_time_seconds, vendor_offset) as u32,
            &seru_trade::default_pool(),
        );
        let owned = party_owned_seru(party);
        (offer, seru_trade::expand_offers(offer, &owned))
    }

    /// Recompute the offer for the current `party` + `play_time_seconds`. The
    /// line list changes when the party's owned seru change (after a trade) or
    /// when the play-time crosses a bucket boundary (the reseed). Closes any
    /// open confirm and clamps the cursor.
    pub fn refresh(&mut self, play_time_seconds: u32, party: &[CharacterRecord]) {
        let (offer, offers) =
            Self::compute(&self.config, self.vendor_offset, play_time_seconds, party);
        self.time_bucket = seru_trade::time_bucket(play_time_seconds);
        self.offer = offer;
        self.offers = offers;
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

    /// `true` when no party member qualifies for this bucket's trade (the UI
    /// then reports "No `<want>` available to trade for `<give>`").
    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    /// Move the highlight by `delta`, wrapping around the line list. No-op while
    /// the confirm overlay is open or when there are no lines.
    pub fn move_cursor(&mut self, delta: i32) {
        if self.confirming || self.offers.is_empty() {
            return;
        }
        let n = self.offers.len() as i32;
        self.cursor = (((self.cursor as i32 + delta) % n + n) % n) as usize;
    }

    /// The currently-highlighted trade line, if any.
    pub fn selected(&self) -> Option<&OwnerTrade> {
        self.offers.get(self.cursor)
    }

    /// Open the yes/no confirm over the highlighted line (defaulting to "No",
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

    /// If the confirm overlay is open on "Yes", close it and return the trade to
    /// apply (the host then calls [`apply_trade`] and [`refresh`]). Returns
    /// `None` otherwise (still picking, or sitting on "No").
    pub fn take_confirmed(&mut self) -> Option<OwnerTrade> {
        if self.confirming && self.confirm_yes {
            let trade = self.selected().copied();
            self.confirming = false;
            self.confirm_yes = false;
            return trade;
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

    fn config(seed: u64) -> SeruTradeConfig {
        SeruTradeConfig {
            enabled: true,
            seed,
            max_offers: 4,
        }
    }

    /// The offer a vendor at `offset` shows at play-time 0.
    fn offer_at(seed: u64, offset: u8) -> BucketOffer {
        seru_trade::bucket_offer(seed, offset as u32, &seru_trade::default_pool())
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
    fn trade_replaces_in_place_at_offer_level() {
        let mut party = vec![ch_with_spells(&[0x81, 0x85])];
        let trade = OwnerTrade {
            owner_slot: 0,
            given_id: 0x81,
            received_id: 0x90,
            given_level: 1,
            received_level: 7,
        };
        assert_eq!(
            apply_trade(&mut party, &trade),
            TradeResult::Swapped {
                owner_slot: 0,
                given: 0x81,
                received: 0x90
            }
        );
        let list = party[0].spell_list();
        assert_eq!(list.count, 2);
        assert_eq!(&list.ids[..2], &[0x90, 0x85]);
        assert_eq!(
            list.levels[0], 7,
            "received seru arrives at the offered level"
        );
    }

    #[test]
    fn trade_compacts_when_receive_already_owned() {
        let mut party = vec![ch_with_spells(&[0x81, 0x90, 0x85])];
        let trade = OwnerTrade {
            owner_slot: 0,
            given_id: 0x81,
            received_id: 0x90, // already owned (stale line)
            given_level: 1,
            received_level: 5,
        };
        assert!(matches!(
            apply_trade(&mut party, &trade),
            TradeResult::Swapped { .. }
        ));
        let list = party[0].spell_list();
        assert_eq!(list.count, 2, "no duplicate created");
        assert_eq!(&list.ids[..2], &[0x90, 0x85]);
    }

    #[test]
    fn trade_rejects_stale_or_bad_owner() {
        let mut party = vec![ch_with_spells(&[0x85])];
        let stale = OwnerTrade {
            owner_slot: 0,
            given_id: 0x81,
            received_id: 0x90,
            given_level: 1,
            received_level: 3,
        };
        assert_eq!(apply_trade(&mut party, &stale), TradeResult::GiveNotOwned);
        let bad = OwnerTrade {
            owner_slot: 9,
            given_id: 0x85,
            received_id: 0x90,
            given_level: 1,
            received_level: 3,
        };
        assert_eq!(apply_trade(&mut party, &bad), TradeResult::BadOwner);
        assert_eq!(party[0].spell_list().count, 1, "party untouched on failure");
    }

    #[test]
    fn session_names_both_sides_even_when_no_owner_qualifies() {
        // A party owning nothing tradeable still gets the bucket offer, so the
        // host can render "No <want> available to trade for <give>".
        let party = vec![ch_with_spells(&[0x05])];
        let s = SeruTradeSession::open(config(0xABCD), 1, 0, &party);
        assert!(s.is_empty(), "no owner of the want -> no lines");
        let expect = offer_at(0xABCD, 1);
        assert_eq!(
            s.offer, expect,
            "the vendor's phased offer present regardless"
        );
        assert_ne!(s.offer.want_id, 0);
        assert_ne!(s.offer.want_id, s.offer.give_id);
    }

    #[test]
    fn vendors_at_different_offsets_show_different_offers() {
        // Same play time, same seed: two traders with different phase offsets
        // sit on different schedule slots, so their offers generally differ.
        let party = vec![ch_with_spells(&[0x05])];
        let base = SeruTradeSession::open(config(0xFEED), 0, 0, &party).offer;
        let differs = (1..8u8)
            .any(|off| SeruTradeSession::open(config(0xFEED), off, 0, &party).offer != base);
        assert!(differs, "some nearby offset shows a different offer");
    }

    #[test]
    fn session_lists_owners_of_the_want_and_confirm_applies_at_level() {
        let seed = 0xABCD;
        let offer = offer_at(seed, 1);
        // Two members own the wanted seru; one of them also owns the give-back
        // and must be filtered out.
        let mut party = vec![
            ch_with_spells(&[offer.want_id]),
            ch_with_spells(&[offer.want_id, offer.give_id]),
        ];
        let mut s = SeruTradeSession::open(config(seed), 1, 0, &party);
        assert_eq!(s.offers.len(), 1, "give-back owner filtered");
        assert_eq!(s.offers[0].owner_slot, 0);
        assert_eq!(s.offers[0].received_level, offer.give_level);

        // Sitting on "No" yields nothing; "Yes" yields the highlighted line.
        s.begin_confirm();
        assert!(s.take_confirmed().is_none());
        s.toggle_confirm();
        let trade = s.take_confirmed().expect("confirmed yes");

        assert!(matches!(
            apply_trade(&mut party, &trade),
            TradeResult::Swapped { .. }
        ));
        let list = party[0].spell_list();
        assert_eq!(&list.ids[..1], &[offer.give_id]);
        assert_eq!(
            list.levels[0], offer.give_level,
            "swap lands at the bucket's give level"
        );
        s.refresh(0, &party);
        assert!(
            s.is_empty(),
            "owner now holds the give-back -> line filtered on refresh"
        );
    }

    #[test]
    fn refresh_reseeds_across_bucket_boundary() {
        let party = vec![ch_with_spells(&[0x81, 0x85, 0x88, 0x8C, 0x90])];
        let mut s = SeruTradeSession::open(config(7), 3, 0, &party);
        let b0 = s.offer;
        assert_eq!(s.time_bucket, 0);
        // Advance several buckets; the standing offer should change at some point.
        let mut changed = false;
        for bucket in 1..12u32 {
            s.refresh(bucket * seru_trade::SECONDS_PER_RESEED, &party);
            assert_eq!(s.time_bucket, bucket);
            if s.offer != b0 {
                changed = true;
            }
        }
        assert!(changed, "the offer should reseed across buckets");
    }
}
