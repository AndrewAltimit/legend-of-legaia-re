//! The trading-card economy on the `World`: collecting every card grant
//! into the active pack's collection, the battle drops, the duel deck and
//! stakes, and the card screen (binder, boosters, dealer, deck builder)
//! while it owns the frame under [`SceneMode::Expansion`].
//!
//! Every entry point is the identity with no pack active: no grant is
//! collected, no drop is rolled, no screen opens. The drops roll on the
//! collection's own card stream
//! ([`legaia_engine_modes::cards::economy::CardProgress::rng`]), seeded once
//! from a read of the world's frame counter and RNG word - a read, so the
//! game's own `rand()` sequence is the same with or without a pack.
//!
//! The card screen opens from three places: the pause menu's card row
//! ([`World::open_card_menu`]), a pack map's `card_dealer` trigger
//! ([`World::open_card_dealer`]), and a host's debug key.

use legaia_engine_battle::monster_catalog::FormationDef;
use legaia_engine_expansion::cards::{self, CardNotice, CardSeat};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::BuildOptions;
use legaia_engine_modes::cards::CardBook;
use legaia_engine_modes::cards::screen::{CardScreen, ScreenStep};
use legaia_engine_modes::session::Reward;

use super::World;
use super::expansion_modes::GOLD_CEILING;

impl World {
    /// The active pack's card book; `None` with no pack.
    pub fn card_book(&self) -> Option<CardBook> {
        let pack = self.expansion.pack.as_deref()?;
        Some(CardBook::new(pack.cards.clone()))
    }

    /// Whether the active pack has a card economy (any card defined).
    pub fn cards_available(&self) -> bool {
        self.expansion
            .pack
            .as_deref()
            .is_some_and(|p| !p.cards.cards.is_empty())
    }

    /// Seed the card stream on first use: the frame counter and the RNG
    /// word, read and mixed with the pack id - never advanced.
    fn seed_card_stream(&mut self) {
        let Some(pack) = self.expansion.pack.as_deref() else {
            return;
        };
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for b in pack.manifest.id.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
        }
        let seed = h ^ self.frame.rotate_left(23) ^ (u64::from(self.rng_state) << 7);
        self.expansion.quests.progress.cards.seed_once(seed);
    }

    /// Drain the quest card inbox and the mode-win queue into the active
    /// pack's collection, queueing a notice per card. No-op with no pack
    /// (the queues are left for a pack that defines the cards).
    pub fn collect_card_grants(&mut self) {
        let Some(book) = self.card_book() else {
            return;
        };
        let x = &mut self.expansion;
        if x.quests.card_inbox.is_empty() && x.pending_card_rewards.is_empty() {
            return;
        }
        cards::collect(
            &mut x.quests.progress.cards,
            &book,
            &mut x.quests.card_inbox,
            &mut x.pending_card_rewards,
            &mut x.cards.notices,
        );
    }

    /// Take the card notices ("Got card: X") queued since the last call,
    /// for the host's toast.
    pub fn take_card_notices(&mut self) -> Vec<CardNotice> {
        self.expansion.cards.take_notices()
    }

    /// Roll the active pack's card rewards for a won `formation`: each
    /// seat's retail monster id, the pack formation and the pack boss. Called
    /// once from the victory spoils ([`World::apply_battle_loot`]); a
    /// no-reward fight rolls nothing, and with no pack nothing is read.
    pub(in crate::world) fn expansion_card_drops(&mut self, formation: &FormationDef) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        if pack.cards.rewards.is_empty() || self.special_battle_word() != 0 {
            return;
        }
        let compiled = self.expansion.battle.compiled.clone();
        let fid = formation.formation_id;
        let pack_formation = compiled
            .as_deref()
            .and_then(|c| c.formation(fid))
            .map(|f| cards::local_key(&f.key).to_string());
        let boss = compiled.as_deref().and_then(|c| {
            c.bosses
                .iter()
                .find(|(_, b)| b.formation_id == fid)
                .map(|(k, _)| k.clone())
        });
        let ids: Vec<u16> = formation.slots.iter().map(|s| s.monster_id).collect();
        let sources = cards::battle_sources(&ids, pack_formation.as_deref(), boss.as_deref());
        self.seed_card_stream();
        let book = CardBook::new(pack.cards.clone());
        let x = &mut self.expansion;
        let progress = &mut x.quests.progress.cards;
        let grants = progress.with_rng(|rng| {
            sources
                .iter()
                .flat_map(|s| book.roll_rewards(s, rng))
                .collect::<Vec<_>>()
        });
        cards::credit(progress, &book, &grants, &mut x.cards.notices);
    }

    /// The duel deck a pack Grid Duel deals the player: the collection's
    /// ([`legaia_engine_modes::cards::economy::CardProgress::duel_deck`]),
    /// or the instance's lent starter deck while the collection is short of
    /// five. Default options with no pack.
    pub(in crate::world) fn pack_mode_build_options(&mut self) -> BuildOptions {
        let mut opts = BuildOptions::default();
        if let Some(book) = self.card_book() {
            let x = &mut self.expansion;
            cards::grant_starter(&mut x.quests.progress.cards, &book, &mut x.cards.notices);
            opts.player_deck = x.quests.progress.cards.duel_deck(&book);
        }
        opts
    }

    /// A finished duel's stake: a card the opponent took from the player's
    /// deck leaves the collection. The player's own winnings arrive through
    /// the pending card rewards.
    pub(in crate::world) fn settle_card_stakes(&mut self, rewards: &[Reward]) {
        let Some(book) = self.card_book() else {
            return;
        };
        for r in rewards {
            if let Reward::Card { seat, card } = r
                && *seat != 0
            {
                let x = &mut self.expansion;
                cards::forfeit(
                    &mut x.quests.progress.cards,
                    &book,
                    card,
                    &mut x.cards.notices,
                );
            }
        }
    }

    fn open_card_screen(&mut self, screen: CardScreen) -> bool {
        if !self.cards_available() || self.mode == SceneMode::Expansion {
            return false;
        }
        if let Some(book) = self.card_book() {
            let x = &mut self.expansion;
            cards::grant_starter(&mut x.quests.progress.cards, &book, &mut x.cards.notices);
        }
        self.seed_card_stream();
        self.collect_card_grants();
        self.expansion.cards.seat = Some(Box::new(CardSeat {
            return_mode: self.mode,
            screen,
        }));
        self.mode = SceneMode::Expansion;
        true
    }

    /// Open the card menu (binder, boosters, sell, deck) over the running
    /// mode - the pause menu's card row. `false` with no pack cards, or
    /// while another expansion screen owns the frame.
    pub fn open_card_menu(&mut self) -> bool {
        self.open_card_screen(CardScreen::menu())
    }

    /// Open the binder directly.
    pub fn open_card_binder(&mut self) -> bool {
        self.open_card_screen(CardScreen::binder())
    }

    /// Open the counter of the pack dealer `dealer` (`""` = the first) -
    /// a map's `card_dealer` trigger. `false` for an unknown dealer.
    pub fn open_card_dealer(&mut self, dealer: &str) -> bool {
        let Some(d) = self
            .expansion
            .pack
            .as_deref()
            .and_then(|p| p.cards.dealer(dealer))
        else {
            return false;
        };
        self.open_card_screen(CardScreen::at_dealer(d))
    }

    /// Whether the card screen owns the frame.
    pub fn card_screen_open(&self) -> bool {
        self.mode == SceneMode::Expansion && self.expansion.cards.seat.is_some()
    }

    /// Close the card screen, restoring the mode it opened over.
    pub fn close_card_screen(&mut self) {
        if let Some(seat) = self.expansion.cards.seat.take() {
            self.expansion.quests.progress.cards.mark_all_seen();
            if self.mode == SceneMode::Expansion {
                self.mode = seat.return_mode;
            }
        }
    }

    /// One frame of the card screen, off this frame's pad.
    pub(in crate::world) fn tick_card_seat(&mut self) {
        let Some(mut seat) = self.expansion.cards.seat.take() else {
            return;
        };
        let Some(book) = self.card_book() else {
            // The pack went away under the screen.
            self.mode = seat.return_mode;
            return;
        };
        self.collect_card_grants();
        let (pad, prev) = (self.input.pad(), self.input.pad_prev());
        let mut gold = self.party.money;
        let step = seat.screen.step(
            pad,
            prev,
            &book,
            &mut self.expansion.quests.progress.cards,
            &mut gold,
            GOLD_CEILING as i32,
        );
        self.party.money = gold;
        match step {
            ScreenStep::Stay => self.expansion.cards.seat = Some(seat),
            ScreenStep::Leave => self.mode = seat.return_mode,
        }
    }
}
