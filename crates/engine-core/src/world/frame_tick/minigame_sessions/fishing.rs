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
        // The overlay's load zeroes the list cursor (`DAT_801D90A8`).
        self.minigames.fishing_exchange_cursor = 0;
        self.minigames.fishing_events.clear();
        self.minigames.fishing_cues.reset();
        self.minigames.fishing_hook_music = false;
        self.minigames.fishing_shore_reached = false;
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
        let lure_owned = legaia_engine_fishing::fishing::select_owned_rod(&mut lure, |id| {
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
        if !lure_owned {
            // Only a host launcher enters with no lure; the door refuses it.
            session.disarm_lure_gate();
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
        self.minigames.fishing_cues.reset();
        if let Some(s) = &session {
            self.bank_fishing_persist(s.persist());
        }
        session
    }

    /// Open the fishing point-exchange (prize shop) list on `exchange`.
    /// The host renders [`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_exchange`] and commits buys through
    /// [`World::fishing_exchange_buy`].
    ///
    /// The list opens on the cursor it was left on
    /// ([`legaia_engine_minigame_scenes::minigame_state::MinigameState::fishing_exchange_cursor`],
    /// retail's `DAT_801D90A8`), not on its first visible row: a cursor
    /// under the row-0 floor is the list's own entry clamp to resolve, which
    /// sends it to the last row (`0x801D0C88..0x801D0C9C`). Retail's list
    /// first opens there.
    pub fn open_fishing_exchange(
        &mut self,
        mut exchange: legaia_engine_fishing::fishing::PrizeExchange,
    ) {
        if let Some(open) = &self.minigames.fishing_exchange {
            self.minigames.fishing_exchange_cursor = open.cursor;
        }
        exchange.cursor = self.minigames.fishing_exchange_cursor;
        self.minigames.fishing_exchange = Some(exchange);
        self.minigames.fishing_exchange_dialog = Default::default();
    }

    /// Close the point-exchange list, keeping its cursor for the next visit.
    pub fn close_fishing_exchange(&mut self) {
        if let Some(open) = self.minigames.fishing_exchange.take() {
            self.minigames.fishing_exchange_cursor = open.cursor;
        }
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
    /// The session head's motor copy: it zeroes both motor words, then with
    /// the options screen's Events vibration row On (`_DAT_800845A8 == 0`)
    /// gives them the
    /// venue's rumble pair `DAT_801D90E0` / `DAT_801D90E4` every frame
    /// (`0x801CF3F4..0x801CF41C`), which the driver tail sets from the hooked
    /// fish's surge as `surge / 8` and `surge / 4`, each rounded toward zero
    /// (`0x801D0344..0x801D0380`). The head reads last frame's tail, which is
    /// the surge the session's last game tick left.
    ///
    /// PORT: FUN_801cf3bc (`0x801CF3F4..0x801CF41C`, `0x801D0344..0x801D0380`)
    fn fishing_motor_copy(&mut self) {
        let Some(surge) = self.minigames.fishing.as_ref().map(|s| s.surge()) else {
            return;
        };
        // The head zeroes both words first (`0x801CF3FC` and its delay
        // slot), so an Off row holds the motors still.
        let (pulse, level) = if self.event_vibration_off() {
            (0, 0)
        } else {
            (surge / 8, surge / 4)
        };
        self.rumble.motors.pulse = pulse;
        self.rumble.motors.level = level;
    }

    /// The hook's music swap. On the strike the lure tick stops the battle
    /// sound source `0x8007056C`, reinstalls it from `_DAT_8007B85C +
    /// 0x32000` and plays it, raising the latch `DAT_801D92A4`
    /// (`FUN_801D26CC`, `0x801D360C..0x801D3664`). That half of the buffer
    /// holds what the fishing init staged there, the hook theme
    /// ([`legaia_engine_mode::minigame_entry::FISHING_HOOK_BGM_ID`], raw TOC
    /// `0x3F9`, `0x801CF2F8..0x801CF30C`), not the last fight's bundle. The
    /// session puts the venue's own track back from the buffer's head when the
    /// catch showcase opens (state `0x20`, `0x801CFD84..0x801CFDD4`) or the
    /// shore comes back (state `0xA`, `0x801CF7E0..0x801CF82C`), dropping the
    /// latch.
    ///
    /// The port swaps back on [`legaia_engine_fishing::fishing::PondEvent::Recast`]
    /// or the frame after the venue camera reaches the showcase.
    ///
    /// PORT: FUN_801d26cc (`0x801D360C..0x801D3664`)
    /// PORT: FUN_801cf3bc (`0x801CF7E0..0x801CF82C`, `0x801CFD84..0x801CFDD4`)
    /// State `0xA`'s track restore on the session's first arrival at the
    /// shore (`0x801CF7E0..0x801CF830`): the hook latch `0x801D92A4` starts at
    /// `-1` (`0x801CF358`), so the arm's `beqz` on it falls through once,
    /// stops the battle source `0x8007056C` (`FUN_800266E0` / `FUN_80026520`),
    /// reinstalls category 5 from the staging buffer's head (`FUN_8001E54C`)
    /// and restarts the source (`FUN_80026478`). The head holds what the
    /// init staged there, the venue's own track
    /// ([`legaia_engine_mode::minigame_entry::FISHING_VENUE_BGM_ID`]), so
    /// the venue plays it whatever the departure scene played. The rod /
    /// type select ahead of it (state `0`) is not modelled, so the replay
    /// lands on the session's first shore tick.
    ///
    /// PORT: FUN_801cf3bc (state 0xa: the first arrival's track restore, 0x801CF7E0..0x801CF830)
    fn fishing_shore_replay(&mut self) {
        use legaia_engine_fishing::fishing::PondPhase;
        let at_shore = self
            .minigames
            .fishing
            .as_ref()
            .is_some_and(|s| matches!(s.phase(), PondPhase::Idle | PondPhase::NoLure));
        if !at_shore || self.minigames.fishing_shore_reached {
            return;
        }
        self.minigames.fishing_shore_reached = true;
        // The departure's track comes back on the return warp
        // ([`Self::restore_minigame_bgm`]); the restart word plays the
        // venue's from its head even where it already sounds.
        if !self.audio.minigame_bgm_active {
            self.audio.minigame_bgm_resume = self.audio.current_bgm;
            self.audio.minigame_bgm_active = true;
        }
        self.pending_field_events.push(FieldEvent::Bgm {
            text_id: legaia_engine_mode::minigame_entry::FISHING_VENUE_BGM_ID,
            sub_op: crate::world::BGM_SUB_OP_ENGINE_RESTART,
        });
    }

    fn fishing_hook_music(&mut self, events: &[legaia_engine_fishing::fishing::PondEvent]) {
        use legaia_engine_fishing::fishing::PondEvent;
        let hooked = events.iter().any(|e| matches!(e, PondEvent::Hooked(_)));
        // Retail puts the venue's track back in two places, each behind the
        // hook latch `0x801D92A4`: state `0x20`'s handler, the frame after
        // the driver reaches the catch's showcase (`0x801CFD84..0x801CFDD4`),
        // and state `0xA`'s, when the shore comes back with the latch still
        // up (`0x801CF7E0..0x801CF82C`; a lost fish, a snapped line). A
        // retail film of five catches drops the latch 272..290 vsyncs after
        // the catch opens: the close-up's lift and the rod's recover swing,
        // which the venue camera driver's showcase gate reads. Its state is
        // the venue frame's, which runs after this tick, so the restore lands
        // the frame after the step, as retail's handler does.
        let back = events.iter().any(|e| matches!(e, PondEvent::Recast))
            || self.minigames.fishing_venue.camera.showcase_opened();
        // Both are the init's staged tracks: the strike installs the second
        // half of the buffer, the restores its head.
        use legaia_engine_mode::minigame_entry::{FISHING_HOOK_BGM_ID, FISHING_VENUE_BGM_ID};
        /// The lift's decrescendo length (`li a2,0xc8` at `0x801D4B08`).
        const FISHING_LIFT_FADE_VSYNCS: u16 = 0xC8;
        // The landing is the line actor's sub-state `4` entry: on its first
        // frame (`+0x22` still zero) it decrescendos the battle source's
        // sequence, the hook theme, by `0xFF` over `0xC8` vsyncs
        // (`FUN_801D4948`, `0x801D4AF4..0x801D4B18`).
        if self.minigames.fishing_hook_music
            && events.iter().any(|e| matches!(e, PondEvent::Landed(_)))
        {
            self.pending_field_events.push(FieldEvent::Bgm {
                text_id: FISHING_LIFT_FADE_VSYNCS,
                sub_op: crate::world::BGM_SUB_OP_ENGINE_DECRESCENDO,
            });
        }
        if hooked && !self.minigames.fishing_hook_music {
            self.minigames.fishing_hook_music = true;
            self.start_minigame_track(FISHING_HOOK_BGM_ID);
        } else if back && self.minigames.fishing_hook_music {
            self.minigames.fishing_hook_music = false;
            self.start_minigame_track(FISHING_VENUE_BGM_ID);
        }
    }

    /// PORT: FUN_801cf3bc (the fishing overlay's driver, reached through its
    /// actor-template tick word). What this covers: the run-loop states
    /// `0xc` idle / `0xd` wind-up / `0x14` power oscillator / `0x1e..0x22`
    /// lure flight, then the pre-hook band roll and the hooked fight, which
    /// retail runs from the fish actor's handler `FUN_801d26cc` into
    /// `FUN_801d4004` rather than from the mode switch. Not covered here:
    /// the rod/type select (state `0`), the fade ramps and the exit fade
    /// (`200`) - the hosts' Start-to-leave affordance replaces the last, and
    /// the no-lure screen (`0x96`) leaves through the same exit - and the
    /// driver tail's HUD, banner
    /// timers and sub-screen, which the hosts compose. The point-exchange
    /// branch (`0x64..0x7a`) is [`World::fishing_exchange_input`].
    pub(in crate::world::frame_tick) fn tick_fishing(&mut self) {
        use legaia_engine_fishing::fishing::CAST_POWER_STEP as FISHING_CAST_STEP;
        use legaia_engine_fishing::fishing::{PondEvent, PondInput};
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
        self.fishing_motor_copy();
        // The venue's hub menu (Triangle / Select on the idle shore) owns the
        // frame while it is up, the exchange list it opens included.
        if self.tick_fishing_hub() || self.minigames.fishing_exchange.is_some() {
            // The point-exchange sub-screen owns the pad while it is open, as
            // retail's shop branch owns the mode switch. The venue's game
            // tick keeps its cadence under either.
            if let Some(s) = self.minigames.fishing.as_mut() {
                s.set_frame_cost(self.toggles.fishing_frame_cost);
                s.hold_vsync();
            }
            self.minigames.fishing_events.clear();
            return;
        }
        let pond_input = PondInput::from_engine_pad(self.input.pad(), self.input.pad_prev());
        let (events, lure_down, spent_lures) = match self.minigames.fishing.as_mut() {
            Some(s) => {
                let casts = s.persist().casts;
                // The venue's game frame, on the step its frame cost gives.
                s.set_frame_cost(self.toggles.fishing_frame_cost);
                s.vsync(pond_input, FISHING_CAST_STEP);
                for _ in 0..s.take_rod_creaks() {
                    // The hooked rod's creak, `0x201` into ring slot 1.
                    self.audio.sfx_ring_ops.push(
                        legaia_engine_system::sound_state::SfxRingOp::WriteSlot(1, 0x201),
                    );
                }
                // The reel's whirr, keyed and released on its own voice.
                let (stop, key) =
                    legaia_engine_fishing::fishing_actors::reel_voice_ops(&s.take_reel_voice());
                if stop {
                    self.audio
                        .sfx_voice_stops
                        .push(legaia_engine_fishing::fishing_actors::REEL_WHIRR_VOICE);
                }
                if key {
                    self.audio.sfx_voice_keys.push(reel_whirr_cue());
                }
                (
                    s.take_events(),
                    s.persist().casts != casts,
                    s.take_spent_lures(),
                )
            }
            None => (Vec::new(), false, Vec::new()),
        };
        self.fishing_hook_music(&events);
        self.fishing_shore_replay();
        // A broken line spends the equipped lure: the break arm's
        // `FUN_80042310(lure + 0x9D, 1)` (`0x801D3C20`), the bag's
        // window-bounded consume-by-id.
        for id in spent_lures {
            if let Ok(id) = u8::try_from(id) {
                let _ = self.party.inventory.consume_returning_slot(id, 1);
            }
        }
        // Back at the shore, state `0xB` runs the lure gate over the bag the
        // break just spent from: the equipped lure re-points at one still
        // held, or the session parks on the no-lure screen (state `0x96`).
        if events.iter().any(|e| matches!(e, PondEvent::Recast)) {
            let bag = &self.party.inventory;
            if let Some(s) = self.minigames.fishing.as_mut() {
                s.shore_lure_gate(|id| i32::from(bag.get(&(id as u8)).copied().unwrap_or(0)));
                self.minigames.fishing_lure = s.persist().lure;
            }
        }
        // State `200` re-runs the lure gate under its fade each frame.
        {
            let bag = &self.party.inventory;
            if let Some(s) = self.minigames.fishing.as_mut()
                && s.exit_fade_level().is_some()
            {
                s.exit_fade_lure_gate(|id| i32::from(bag.get(&(id as u8)).copied().unwrap_or(0)));
                self.minigames.fishing_lure = s.persist().lure;
            }
        }
        // The exit fade (state `200`, from the hub's row 4 or the no-lure
        // screen's edge) ran out: leave the venue.
        if self
            .minigames
            .fishing
            .as_mut()
            .is_some_and(|s| s.take_exit_request())
        {
            self.exit_fishing();
            self.close_minigame_round_trip();
            return;
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
        }
        // The hook and catch cues - one clock shared with the minigames
        // page, on the world's cue queue every host drains. The celebration's
        // cues land on the line actor's stage frames, not the landing frame.
        // The clock steps with the pond's game tick, and stands between them.
        let (strength, step) = self
            .minigames
            .fishing
            .as_ref()
            .map_or((0, 0), |s| (s.strength(), s.vsync_step()));
        if step > 0 {
            let cues = self
                .minigames
                .fishing_cues
                .frame(&events, strength, step as i16);
            self.minigames.pending_sfx.extend(cues);
        }
        self.minigames.fishing_events = events;
    }
}

/// The reel whirr's direct key-on (`FUN_80065034`, `0x801D2EB8`).
pub(crate) fn reel_whirr_cue() -> legaia_engine_minigames::other_game_overlay::VoiceAttrCue {
    use legaia_engine_fishing::fishing_actors::{REEL_WHIRR_ATTR, REEL_WHIRR_VOICE};
    legaia_engine_minigames::other_game_overlay::VoiceAttrCue {
        voice: u32::from(REEL_WHIRR_VOICE),
        vab_program_tone: REEL_WHIRR_ATTR.0,
        note_and_fine: REEL_WHIRR_ATTR.1,
        volume: REEL_WHIRR_ATTR.2,
    }
}

#[cfg(test)]
mod hook_audio_tests {
    use crate::world::{FieldEvent, World};
    use legaia_engine_fishing::fishing::{FishingRecord, PondEvent, PondSession};

    fn pond_world() -> World {
        let mut w = World::new();
        w.enter_fishing(PondSession::new(
            Vec::new(),
            vec![[0; 8]; 3],
            Vec::new(),
            0,
            0,
            0,
            0,
            FishingRecord::default(),
            0,
            1,
        ));
        w
    }

    fn bgm_starts(w: &mut World) -> Vec<u16> {
        std::mem::take(&mut w.pending_field_events)
            .into_iter()
            .filter_map(|e| match e {
                FieldEvent::Bgm { text_id, sub_op: 1 } => Some(text_id),
                _ => None,
            })
            .collect()
    }

    /// The strike swaps the init's hook theme in once, whatever the last
    /// fight played; the landing alone does not put the venue's track back
    /// (the showcase does, the tick after the venue camera reaches it:
    /// `fishing_catch_body_disc`), and the shore does, once.
    #[test]
    fn the_hook_plays_the_staged_hook_theme_until_the_shore() {
        use legaia_engine_mode::minigame_entry::{FISHING_HOOK_BGM_ID, FISHING_VENUE_BGM_ID};
        let mut w = pond_world();
        w.audio.current_bgm = Some(2000);
        w.audio.battle_bgm = Some(2026);
        w.fishing_hook_music(&[PondEvent::Hooked(3)]);
        assert_eq!(bgm_starts(&mut w), [FISHING_HOOK_BGM_ID]);
        w.fishing_hook_music(&[PondEvent::Hooked(3)]);
        assert!(bgm_starts(&mut w).is_empty(), "the latch holds");
        w.fishing_hook_music(&[PondEvent::Landed(40)]);
        let fades: Vec<_> = w
            .pending_field_events
            .iter()
            .filter_map(|e| match *e {
                FieldEvent::Bgm { text_id, sub_op }
                    if sub_op == crate::world::BGM_SUB_OP_ENGINE_DECRESCENDO =>
                {
                    Some(text_id)
                }
                _ => None,
            })
            .collect();
        assert_eq!(fades, [0xC8], "the lift fades the hook theme");
        assert!(
            bgm_starts(&mut w).is_empty(),
            "the catch keeps the hook theme"
        );
        w.fishing_hook_music(&[PondEvent::Recast]);
        assert_eq!(bgm_starts(&mut w), [FISHING_VENUE_BGM_ID]);
        w.fishing_hook_music(&[PondEvent::Recast]);
        assert!(bgm_starts(&mut w).is_empty(), "the latch is down");
        // The departure's track is what the return warp gives back.
        w.restore_minigame_bgm();
        assert_eq!(bgm_starts(&mut w), [2000]);
    }

    /// The first shore arrival restarts the venue's own track from its head,
    /// whatever the departure played, once per session (the latch's `-1`,
    /// `0x801CF358`): the restart word, not a plain start the host would play
    /// on through.
    #[test]
    fn the_first_shore_replays_the_venues_track_once() {
        use legaia_engine_mode::minigame_entry::FISHING_VENUE_BGM_ID;
        let mut w = pond_world();
        w.audio.current_bgm = Some(2000);
        w.pending_field_events.clear();
        w.fishing_shore_replay();
        let restarts = |w: &mut World| -> Vec<u16> {
            std::mem::take(&mut w.pending_field_events)
                .into_iter()
                .filter_map(|e| match e {
                    FieldEvent::Bgm { text_id, sub_op }
                        if sub_op == crate::world::BGM_SUB_OP_ENGINE_RESTART =>
                    {
                        Some(text_id)
                    }
                    _ => None,
                })
                .collect()
        };
        assert_eq!(restarts(&mut w), [FISHING_VENUE_BGM_ID]);
        w.fishing_shore_replay();
        assert!(restarts(&mut w).is_empty(), "only the first arrival");
        // A new session loads the overlay again: the latch is back at -1.
        w.exit_fishing();
        w.enter_fishing(pond_world().minigames.fishing.take().unwrap());
        w.fishing_shore_replay();
        assert_eq!(restarts(&mut w), [FISHING_VENUE_BGM_ID]);
    }

    /// The reel's whirr reaches both play hosts' voice queues: a reel press
    /// on a waiting line keys voice `0x13`, and the spent pull with the
    /// button let go releases it (`0x801D2EB8`, `0x801D2F30`).
    #[test]
    fn the_reel_whirr_keys_and_releases_its_voice() {
        use legaia_engine_fishing::fishing::{PondInput, PondPhase};
        let mut w = pond_world();
        let p = w.minigames.fishing.as_mut().unwrap();
        let cast = PondInput {
            cast_edge: true,
            ..Default::default()
        };
        p.tick(cast, 1, 0x80);
        for _ in 0..200 {
            if p.phase() == PondPhase::Power {
                break;
            }
            p.tick(PondInput::default(), 1, 0x80);
        }
        for _ in 0..24 {
            p.tick(PondInput::default(), 1, 0x80);
        }
        p.tick(cast, 1, 0x80);
        while p.phase() == PondPhase::Flight {
            p.tick(PondInput::default(), 1, 0x80);
        }
        assert_eq!(p.phase(), PondPhase::Waiting);
        let _ = p.take_reel_voice();
        let cross = legaia_engine_system::input::PadButton::Cross.mask();
        w.set_pad(cross);
        let _ = w.tick();
        let keys = w.take_sfx_voice_keys();
        assert!(
            keys.iter()
                .any(|k| k.voice == 0x13 && k.vab_program_tone == (2, 0, 0)),
            "a reel press keys the whirr: {keys:?}"
        );
        w.set_pad(0);
        let mut released = false;
        for _ in 0..40 {
            let _ = w.tick();
            released |= w.take_sfx_voice_stops().contains(&0x13);
        }
        assert!(released, "a spent pull with the button up releases it");
    }

    /// The head copies `surge / 8` and `surge / 4` into the motor words while
    /// the Events vibration row is On, and holds them at zero when it is Off.
    #[test]
    fn the_fight_drives_the_motors_behind_the_events_row() {
        let mut w = pond_world();
        w.minigames.fishing.as_mut().unwrap().seat_surge(0x3C0);
        w.fishing_motor_copy();
        assert_eq!((w.rumble.motors.pulse, w.rumble.motors.level), (0x78, 0xF0));
        w.minigames.fishing.as_mut().unwrap().seat_surge(-13);
        w.fishing_motor_copy();
        assert_eq!((w.rumble.motors.pulse, w.rumble.motors.level), (-1, -3));
        w.toggles.retail_options.vibration_events = 1;
        w.minigames.fishing.as_mut().unwrap().seat_surge(0x100);
        w.fishing_motor_copy();
        assert_eq!((w.rumble.motors.pulse, w.rumble.motors.level), (0, 0));
    }
}
