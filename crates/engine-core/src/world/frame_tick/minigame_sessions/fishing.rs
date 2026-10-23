//! The fishing session: enter and exit, the persisted record, the point
//! exchange and the per-frame tick.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Enter the fishing minigame on `session`, suspending the current scene
    /// mode (restored by [`World::exit_fishing`]). Like the dance / pause-menu
    /// suspend contract, the interrupted field state stays intact underneath.
    ///
    /// Takes a ready session; [`World::enter_fishing_session`] is the entry
    /// that builds one from the persistent save-block words, which is what
    /// every player-facing and debug path uses.
    pub fn enter_fishing(&mut self, session: legaia_engine_fishing::fishing::PondSession) {
        if self.mode != SceneMode::Fishing {
            self.minigames.fishing_return_mode = self.mode;
        }
        self.minigames.fishing = Some(session);
        self.minigames.fishing_events.clear();
        self.mode = SceneMode::Fishing;
    }

    /// The persistent fishing words (`_DAT_8008444C..0x8008446C`) as the
    /// world holds them between sessions.
    pub fn fishing_persist(&self) -> legaia_engine_fishing::fishing::FishingPersist {
        let m = &self.minigames;
        legaia_engine_fishing::fishing::FishingPersist {
            lure: m.fishing_lure,
            rod: m.fishing_rod as i32,
            casts: m.fishing_casts,
            record: legaia_engine_fishing::fishing::FishingRecord {
                points: m.fishing_points,
                best_points: m.fishing_best_points,
                best_fish: m.fishing_best_fish as usize,
            },
            purchased_mask: m.fishing_prizes_purchased,
        }
    }

    /// Bank a session's persistent words back into the world - retail keeps
    /// them in the live save window, so they outlast the overlay.
    pub(in crate::world::frame_tick) fn bank_fishing_persist(
        &mut self,
        p: legaia_engine_fishing::fishing::FishingPersist,
    ) {
        let m = &mut self.minigames;
        m.fishing_lure = p.lure;
        m.fishing_rod = p.rod.max(0) as u32;
        m.fishing_casts = p.casts;
        m.fishing_points = p.record.points;
        m.fishing_best_points = p.record.best_points;
        m.fishing_best_fish = p.record.best_fish as u32;
        m.fishing_prizes_purchased = p.purchased_mask;
    }

    /// Open a fishing session over the decoded overlay `tables` at `venue`
    /// (`0` Buma, `1` Vidna - `DAT_801d90d0`), seeded from the persistent
    /// save-block words, and enter it.
    ///
    /// The bring-up's two ownership scans run first, as in retail: the rod
    /// scan ([`World::resolve_fishing_entry_rod`], `FUN_801CF070`) and the
    /// lure gate ([`legaia_engine_fishing::fishing::select_owned_rod`] over the lure family,
    /// `FUN_801d712c`), each writing its corrected index back. `venue_map`
    /// gives the cast lure a world to land in (the venue scene's `.MAP`).
    ///
    /// The BIOS-rand stream is seeded off the frame counter, so a repeat
    /// entry varies while a replayed pad stream stays deterministic.
    pub fn enter_fishing_session(
        &mut self,
        tables: &legaia_engine_fishing::fishing::FishingTables,
        venue: usize,
        venue_map: Option<legaia_engine_fishing::fishing::PondVenue>,
    ) {
        use legaia_engine_mode::minigame_entry::FISHING_SEED_SALT;
        self.resolve_fishing_entry_rod();
        let bag = &self.party.inventory;
        let mut lure = self.minigames.fishing_lure;
        legaia_engine_fishing::fishing::select_owned_rod(&mut lure, |id| {
            i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
        });
        self.minigames.fishing_lure = lure;
        let seed = FISHING_SEED_SALT ^ self.frame as u32;
        let mut session = legaia_engine_fishing::fishing::PondSession::from_tables(
            tables,
            venue,
            self.fishing_persist(),
            seed,
        );
        if let Some(v) = venue_map {
            session.attach_venue(v);
        }
        self.enter_fishing(session);
    }

    /// Re-point the persistent rod cell at a rod the party actually holds, and
    /// return it as a session's `rod_stat`.
    ///
    /// This is the fishing bring-up's own scan
    /// ([`legaia_engine_fishing::fishing::entry_rod_index`], retail `FUN_801CF070` at
    /// `0x801cf35c..0x801cf39c`) run against the live bag: it keeps
    /// [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_rod`] when that rod is held,
    /// otherwise steps forward with wrap, and lands on `0` for a party holding
    /// no rod at all - so a stale save index never divides the tension gauge.
    /// The result is written back, exactly as retail leaves it in
    /// `_DAT_80084454`, which is the same cell the persistent HUD's rod row
    /// reads.
    ///
    /// Every session entry runs it, through [`World::enter_fishing_session`]:
    /// the mode-24 door warp and both play hosts' debug launchers alike.
    pub fn resolve_fishing_entry_rod(&mut self) -> i32 {
        let bag = &self.party.inventory;
        let rod =
            legaia_engine_fishing::fishing::entry_rod_index(self.minigames.fishing_rod, |id| {
                i32::from(bag.get(&(id as u8)).copied().unwrap_or(0))
            });
        self.minigames.fishing_rod = rod;
        rod as i32
    }

    /// Leave the fishing minigame and restore the interrupted mode, returning
    /// the session so the host can read the final [`FishingRecord`]. Every
    /// persistent word the session carries - the point record, the cast
    /// counter, lure, rod and the one-time prize mask - is banked back into
    /// [`legaia_engine_minigame_scenes::minigame_state::MinigameState`] (retail writes those cells in place;
    /// the next session seeds from them). No-op when fishing isn't active.
    ///
    /// [`FishingRecord`]: legaia_engine_fishing::fishing::FishingRecord
    pub fn exit_fishing(&mut self) -> Option<legaia_engine_fishing::fishing::PondSession> {
        if self.mode == SceneMode::Fishing {
            self.mode = self.minigames.fishing_return_mode;
        }
        let session = self.minigames.fishing.take();
        self.minigames.fishing_exchange = None;
        self.minigames.fishing_events.clear();
        if let Some(s) = &session {
            self.bank_fishing_persist(s.persist());
        }
        session
    }

    /// Open the fishing point-exchange (prize shop) list on `exchange`.
    /// The host renders [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_exchange`] and commits buys through
    /// [`World::fishing_exchange_buy`].
    pub fn open_fishing_exchange(
        &mut self,
        mut exchange: legaia_engine_fishing::fishing::PrizeExchange,
    ) {
        // Row 0 hides until strictly affordable - floor the cursor to the
        // first visible row for the current point pool.
        exchange.cursor = exchange
            .cursor
            .max(exchange.first_visible(self.minigames.fishing_points));
        self.minigames.fishing_exchange = Some(exchange);
    }

    /// Close the point-exchange list.
    pub fn close_fishing_exchange(&mut self) {
        self.minigames.fishing_exchange = None;
    }

    /// Commit a point-exchange purchase of `qty` units of `row`
    /// (`FUN_801d06c8`'s Yes arm): validates through
    /// [`legaia_engine_fishing::fishing::PrizeExchange::buy`] against the persistent pool /
    /// purchased mask / live inventory count, then deducts
    /// [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_points`], latches the one-time bit, and grants the
    /// item into [`legaia_engine_menus::party_state::PartyState::inventory`]. While a fishing session is live its
    /// record is synced to the reduced pool so the on-screen point total
    /// matches. `None` when no exchange is open or the buy doesn't validate.
    pub fn fishing_exchange_buy(
        &mut self,
        row: usize,
        qty: u32,
    ) -> Option<legaia_engine_fishing::fishing::PrizePurchase> {
        let ex = self.minigames.fishing_exchange.as_ref()?;
        let item_id = ex.rows.get(row)?.item_id;
        let owned = *self.party.inventory.get(&item_id).unwrap_or(&0) as u32;
        let purchase = ex.buy(
            row,
            qty,
            self.minigames.fishing_points,
            owned,
            self.minigames.fishing_prizes_purchased,
        )?;
        self.minigames.fishing_points -= purchase.cost as i32;
        if let Some(bit) = purchase.latched_bit {
            self.minigames.fishing_prizes_purchased |= 1 << bit;
        }
        let _ = self
            .party
            .inventory
            .add(purchase.item_id, purchase.qty.min(255) as u8);
        if let Some(s) = &mut self.minigames.fishing {
            s.record.points = self.minigames.fishing_points;
            s.purchased_mask = self.minigames.fishing_prizes_purchased;
        }
        Some(purchase)
    }

    /// Advance the fishing minigame one frame, reading this frame's pad into
    /// the session's [`PondInput`](legaia_engine_fishing::fishing::PondInput):
    ///
    /// - **Cast / confirm** is the [`Circle`] edge (retail packed bit
    ///   `0x20`): it starts the wind-up at the idle shore, locks the power
    ///   meter, and dismisses a resolved fight.
    /// - **Reel** is the held [`Cross`] (`0x40`, reel A) / [`Square`]
    ///   (`0x80`, reel B) pair, rebuilt into the retail held word
    ///   `_DAT_8007b850` and classified by the ported decoder inside the
    ///   session, so holding both resolves to reel A as retail does.
    /// - **Strike credit**: the pre-hook band check's pad nudge, counted by
    ///   [`legaia_engine_fishing::fishing_actors::bite_pad_nudge`] - one per fresh D-pad
    ///   left, D-pad right, and reel press, the reel pair counting once
    ///   ([`PondInput::from_engine_pad`](legaia_engine_fishing::fishing::PondInput::from_engine_pad)).
    ///
    /// The frame's [`PondEvent`](legaia_engine_fishing::fishing::PondEvent)s land in
    /// [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_events`] for the hosts' banner
    /// one-shots, and the two sound-bearing ones queue their cues here.
    ///
    /// [`Circle`]: input::PadButton::Circle
    /// [`Cross`]: input::PadButton::Cross
    /// [`Square`]: input::PadButton::Square
    ///
    /// PORT: FUN_801cf3bc (the fishing overlay's driver, reached through its
    /// actor-template tick word). What this covers: the run-loop states
    /// `0xc` idle / `0xd` wind-up / `0x14` power oscillator / `0x1e..0x22`
    /// lure flight, then the pre-hook band roll and the hooked fight, which
    /// retail runs from the fish actor's handler `FUN_801d26cc` into
    /// `FUN_801d4004` rather than from the mode switch. Not covered here:
    /// the rod/type select (state `0`), the fade ramps, the no-lure end
    /// screen (`0x96`) and the exit fade (`200`) - the hosts' Start-to-leave
    /// affordance replaces the last - and the driver tail's HUD, banner
    /// timers and sub-screen, which the hosts compose. The point-exchange
    /// branch (`0x64..0x7a`) is [`World::fishing_exchange_input`]. The
    /// casting-meter step is not byte-pinned; `FISHING_CAST_STEP` is the
    /// host rate.
    pub(in crate::world::frame_tick) fn tick_fishing(&mut self) {
        use legaia_engine_fishing::fishing::{PondEvent, PondInput};
        /// Per-frame casting-meter step (see the method note - not byte-pinned).
        const FISHING_CAST_STEP: i32 = 0x80;
        /// The lure-landing cue and the ring slot it is stored into.
        const LURE_DOWN_CUE: i16 = 0x204;
        const LURE_DOWN_CUE_SLOT: u8 = 2;
        /// Packed spread argument the strike splash fans its three parts by.
        /// Direct form (bit [`legaia_engine_fishing::fishing_chrome::SPLASH_SUB_BLOCK_BIT`]
        /// clear); the value is the play window's, carried over unchanged.
        const SPLASH_SPREAD: i32 = 0x40;
        if self.minigames.fishing.is_none() {
            // Mode is Fishing but no session installed - drop back to a sane mode.
            self.mode = self.minigames.fishing_return_mode;
            return;
        }
        // The venue's hub menu (Triangle / Select on the idle shore) owns the
        // frame while it is up, the exchange list it opens included.
        if self.tick_fishing_hub() {
            self.minigames.fishing_events.clear();
            return;
        }
        // The point-exchange sub-screen owns the pad while it is open, as
        // retail's shop branch owns the mode switch.
        if self.minigames.fishing_exchange.is_some() {
            self.minigames.fishing_events.clear();
            return;
        }
        let pond_input = PondInput::from_engine_pad(self.input.pad(), self.input.pad_prev());
        let (events, lure_down, spent_lures) = match self.minigames.fishing.as_mut() {
            Some(s) => {
                let casts = s.persist().casts;
                s.tick(pond_input, 1, FISHING_CAST_STEP);
                for _ in 0..s.take_rod_creaks() {
                    // The hooked rod's creak, `0x201` into ring slot 1.
                    self.audio.sfx_ring_ops.push(
                        legaia_engine_system::sound_state::SfxRingOp::WriteSlot(1, 0x201),
                    );
                }
                (
                    s.take_events(),
                    s.persist().casts != casts,
                    s.take_spent_lures(),
                )
            }
            None => (Vec::new(), false, Vec::new()),
        };
        // A broken line spends the equipped lure: the break arm's
        // `FUN_80042310(lure + 0x9D, 1)` (`0x801D3C20`), the bag's
        // window-bounded consume-by-id.
        for id in spent_lures {
            if let Ok(id) = u8::try_from(id) {
                let _ = self.party.inventory.consume_returning_slot(id, 1);
            }
        }
        // The lure landing: the arm of the lure tick that bumps the
        // persistent cast counter stores cue `0x204` straight into ring
        // slot 2 (`sh v0,-0x4924(v1)` at `0x801D2950`, `FUN_801d26cc`). A
        // runtime-bank id, resolved through the venue scene's own bundle -
        // the fishing init loads no `efect.dat` of its own.
        if lure_down {
            self.audio
                .sfx_ring_ops
                .push(legaia_engine_system::sound_state::SfxRingOp::WriteSlot(
                    LURE_DOWN_CUE_SLOT,
                    LURE_DOWN_CUE,
                ));
        }
        for e in &events {
            // The cadence-match strike splash spawns its three parts into
            // the shared effect pool. The producer is the session's own
            // event, not a venue actor, so every host that ticks the world
            // gets the burst.
            if matches!(e, PondEvent::Splash) {
                let parts = legaia_engine_fishing::fishing_chrome::splash_burst(
                    legaia_engine_fishing::fishing_actors::SCREEN_CENTRE.0,
                    legaia_engine_fishing::fishing_actors::SCREEN_CENTRE.1,
                    legaia_engine_minigames::minigame_fx::SPLASH_SPRITE_ID,
                    SPLASH_SPREAD,
                );
                for p in &parts {
                    let (x, y) = p.fx_seat();
                    self.minigames.fx.spawn(x, y, p.sprite_id);
                }
            }
            // The hook and catch cues - one kernel shared with the minigames
            // page. The hook cue used to live on the native window's line
            // actor, which made the strike audible on one surface; the cue
            // queue is drained by every host.
            self.minigames
                .pending_sfx
                .extend(legaia_engine_minigame_scenes::minigame_state::pond_event_cues(e));
        }
        self.minigames.fishing_events = events;
    }
}
