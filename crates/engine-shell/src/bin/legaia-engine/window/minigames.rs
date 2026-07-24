//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    // The mode-24 minigame door warp (`World::arm_minigame_warp` /
    // `World::minigame_return_warp`, retail `FUN_80025980` / `FUN_80026018`) is
    // deliberately NOT called from these entry points, and the reason is a bug
    // one layer down rather than a missing prerequisite.
    //
    // `FUN_80026018` banks the mode-24 winnings accumulator `_DAT_80084440`
    // into the casino coin bank `_DAT_800845A4` (`0x80026050..0x80026078`,
    // clamped at 9,999,999). What fills that accumulator is the Baka Fighter
    // end-of-match tally: `FUN_801D239C` at `0x801d2894..0x801d28bc` adds each
    // drained step into `0x80084440`. This port instead pays that drain into
    // `World::money` - party gold, which retail keeps at the *different* word
    // `0x8008459C` - so `World::minigame_winnings` never fills and the warp's
    // commit would be an add of zero.
    //
    // Wiring the warp here without redirecting the duel tally first would
    // therefore close the audit row while leaving the round trip inert. The
    // redirect lives in `World::tick_baka_fighter` / `World::exit_baka_fighter`
    // (`engine-core`), so the two halves have to land together.
    // REF: FUN_80026018 (coin-bank commit), FUN_801d239c (the producer)

    /// Drive the fishing HUD's one-shot banner animations for this frame.
    ///
    /// Seeds a timer on each session phase edge (cast lock = strike + hook,
    /// resolve = reel-in or miss, recast = the auxiliary banner), then services
    /// every timer through the retail driver-tail loop
    /// ([`BannerTimer::service`](legaia_engine_render::BannerTimer::service))
    /// and caches this frame's draws for the HUD builder, which is `&self` and
    /// cannot advance them itself.
    ///
    /// The frame step is the engine's fixed one tick per frame (retail reads
    /// `DAT_1f800393`, its frame-rate compensation word).
    pub(super) fn tick_fishing_banners(&mut self) {
        use legaia_engine_core::fishing::{FightOutcome, FishingPhase};
        let Some(session) = self.session.host.world.fishing.as_ref() else {
            // Left the minigame: drop any half-run banner with the session.
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            self.fishing_prev_phase = None;
            return;
        };
        let phase = session.phase();
        let outcome = session.last_outcome();
        match (self.fishing_prev_phase, phase) {
            (Some(FishingPhase::Casting), FishingPhase::Fighting) => {
                self.fishing_banners.on_hook();
            }
            (Some(FishingPhase::Fighting), FishingPhase::Done) => match outcome {
                Some(FightOutcome::Landed { .. }) => self.fishing_banners.on_landed(),
                Some(FightOutcome::Snapped) => self.fishing_banners.on_snapped(),
                _ => {}
            },
            (Some(FishingPhase::Done), FishingPhase::Casting) => {
                self.fishing_banners.on_recast();
            }
            _ => {}
        }
        self.fishing_prev_phase = Some(phase);
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
    }

    /// Advance the Muscle Dome contest's round **time meter** one frame.
    ///
    /// Retail runs the meter from the arena's per-frame driver with the frame
    /// delta from scratchpad `0x1F800393`; the engine ticks a fixed one per
    /// frame. No-op outside a contest.
    pub(super) fn tick_muscle_time_meter(&mut self) {
        if let Some(s) = self.session.host.world.muscle_dome.as_mut() {
            s.tick_time_meter(1);
        }
    }

    /// Drain the Baka Fighter duel's queued SFX cues and enqueue them into the
    /// BGM director's SFX scheduler, so the punch/exchange hit (`BAKA_CUE_HIT`
    /// = `0x09`, written by the rules kernel's damage step) actually sounds in
    /// the live engine. Mirrors the battle strike-SFX path
    /// (`drain_and_log_battle_events` → `enqueue_sfx`); the director's
    /// per-frame `tick_sfx_frame` (driven from `drain_and_log_battle_events`)
    /// fires the enqueued cues against the resident class-2 SFX bank the same
    /// frame. The cues carry no gameplay state, so nothing here affects
    /// determinism. No-op outside the duel / when no audio is attached; the
    /// fight's cue buffer is drained regardless so it never accumulates.
    pub(super) fn drain_baka_sfx_cues(&mut self) {
        if self.session.host.world.mode != SceneMode::BakaFighter {
            return;
        }
        let cues: Vec<u8> = self
            .session
            .host
            .world
            .baka_fighter
            .as_mut()
            .map(|f| f.take_cues())
            .unwrap_or_default();
        if cues.is_empty() {
            return;
        }
        if let Some(bgm) = self.session.bgm.as_mut() {
            // Fire on the same frame (strike-relative delay 0); the duel has no
            // actor/target slots, so pass 0/0 for the HUD-context fields.
            for id in &cues {
                bgm.enqueue_sfx(*id as u16, 0, 0, 0);
            }
        } else {
            for id in &cues {
                log::debug!("baka SFX cue {id:#04x} (no audio)");
            }
        }
    }

    /// The monster stat archive (PROT 867) bytes, decoded + cached on first
    /// use. `None` if no disc is attached or the entry can't be read.
    pub(super) fn monster_archive_bytes(&mut self) -> Option<std::sync::Arc<Vec<u8>>> {
        if self.monster_archive.is_none() {
            const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;
            match self
                .session
                .host
                .index
                .entry_bytes_extended(MONSTER_ARCHIVE_PROT_ENTRY)
            {
                Ok(b) => self.monster_archive = Some(std::sync::Arc::new(b)),
                Err(e) => {
                    log::warn!("play-window: monster archive (PROT 867) load skipped: {e:#}");
                    return None;
                }
            }
        }
        self.monster_archive.clone()
    }

    /// Load the Noa dance overlay (PROT 0980), decode its baked step chart, and
    /// start a dance run in the world (suspending the current scene). Returns
    /// `false` (and logs) when no disc is attached or the chart can't decode.
    ///
    /// Mirrors the disc-gated `dance_minigame_real` test's overlay path: read
    /// the raw PROT entry, lift it to its statically-recovered loaded form via
    /// [`static_overlay::as_loaded`], then parse through
    /// [`DanceGame::from_overlay`].
    pub(super) fn start_dance_minigame(&mut self, long_song: bool) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("dance: overlay 0980 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("dance: as_loaded failed: {e:#}");
                return false;
            }
        };
        match legaia_engine_core::dance::DanceGame::from_overlay(&loaded, long_song) {
            Some(game) => {
                self.session.host.world.enter_dance(game);
                // The dance overlay loads one of two mode-selected chart loops
                // (global BGM 2058/2064 = extraction 1048/1054). The exact
                // mode->song arm is unpinned; approximate it by song length.
                self.session
                    .start_global_bgm(if long_song { 2064 } else { 2058 });
                true
            }
            None => {
                log::warn!("dance: step-chart parse failed");
                false
            }
        }
    }

    /// Load the fishing overlay (PROT 0972), decode its per-species table, and
    /// start a fishing session in the world (suspending the current scene).
    /// Returns `false` (and logs) when no disc is attached or the table can't
    /// decode. Mirrors [`Self::start_dance_minigame`]'s overlay path.
    ///
    /// The rod stat + persistent record start at defaults (the save-block
    /// fishing record isn't loaded into this dev entry point).
    pub(super) fn start_fishing_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("fishing: overlay 0972 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("fishing: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(species) = legaia_asset::fishing_species::parse(&loaded) else {
            log::warn!("fishing: species-table parse failed");
            return false;
        };
        // Decode the two point-exchange venue pages alongside the species
        // table, naming rows from the SCUS item table when it's readable
        // (P toggles the prize list while fishing).
        self.fishing_prize_venues = legaia_asset::fishing_exchange::parse(&loaded).map(|ex| {
            use legaia_engine_core::Vfs;
            let scus = if let Some(root) = self.extracted_root.as_deref() {
                legaia_engine_core::DirVfs::new(root)
                    .ok()
                    .and_then(|v| v.read("SCUS_942.54").ok())
            } else if let Some(disc) = self.disc_path.as_deref() {
                legaia_engine_core::DiscVfs::open(disc)
                    .ok()
                    .and_then(|v| v.read("SCUS_942.54").ok())
            } else {
                None
            };
            let names = scus
                .as_deref()
                .and_then(legaia_asset::item_names::ItemNameTable::from_scus);
            [0usize, 1].map(|venue| {
                legaia_engine_core::fishing::PrizeExchange::from_asset(
                    venue,
                    &ex.venues[venue],
                    names.as_ref(),
                )
            })
        });
        // Default rod stat for the dev entry point; the record resumes the
        // world's persistent point pool (banked back on exit).
        const DEV_ROD_STAT: i32 = 4;
        let record = legaia_engine_core::fishing::FishingRecord {
            points: self.session.host.world.fishing_points,
            ..Default::default()
        };
        let session =
            legaia_engine_core::fishing::FishingSession::new(species, DEV_ROD_STAT, record);
        self.session.host.world.enter_fishing(session);
        true
    }

    /// Load the slot-machine overlay (PROT 0975), decode its payout table, and
    /// start a slot session in the world (suspending the current scene).
    /// Returns `false` (and logs) when no disc is attached or the table can't
    /// decode. Mirrors [`Self::start_dance_minigame`]'s overlay path.
    ///
    /// The playing balance seeds from the world's casino coin bank
    /// (`World::casino_coins`, the retail `_DAT_800845A4`); a thin bank first
    /// goes through the casino's **coin-exchange counter**
    /// ([`Self::buy_casino_coins`]) and only falls back to a fronted dev stake
    /// when the party cannot pay. The final balance commits back to the bank on
    /// exit (`World::exit_slot_machine`).
    pub(super) fn start_slot_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("slots: overlay 0975 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("slots: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("slots: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(payouts) = legaia_asset::slot_payout::parse(&loaded) else {
            log::warn!("slots: payout-table parse failed");
            return false;
        };
        // The retail entry path arrives through the casino with coins already
        // banked; when the bank can't cover a spin, buy them at the exchange
        // counter first, and only front a dev stake if the party can't pay.
        const DEV_STAKE: i32 = 100;
        let bank = self.session.host.world.casino_coins as i32;
        let balance = if bank >= legaia_engine_core::slot_machine::MIN_SPIN_BALANCE {
            bank
        } else if let Some(bought) = self.buy_casino_coins(DEV_STAKE) {
            bought
        } else {
            log::info!("slots: coin bank {bank} too thin - fronting a {DEV_STAKE}-coin dev stake");
            DEV_STAKE
        };
        // Seed from the frame counter: deterministic across a replayed pad
        // stream (retail reseeds from BIOS rand at machine init).
        let seed = 0x5107_5EED ^ self.session.host.world.frame as u32;
        let machine = legaia_engine_core::slot_machine::SlotMachine::new(payouts, seed, balance);
        self.session.host.world.enter_slot_machine(machine);
        true
    }

    /// Buy `coins` at the casino's coin-exchange counter, debiting party gold
    /// and crediting the coin bank. Returns the new bank balance, or `None`
    /// when the counter refuses the sale (party can't pay, or the counter is
    /// out of coins) - in which case nothing is debited.
    ///
    /// The counter arithmetic is the ported one: the requested count is laid
    /// out least-significant-digit-first the way the screen's entry field
    /// stores it, and [`coin_exchange_quote`] resolves the total cost and both
    /// gates (`gold >= cost`, `stock >= coins`) exactly as `FUN_801E6F70`
    /// does before it recolours the total.
    ///
    /// [`coin_exchange_quote`]: legaia_engine_core::slot_machine::coin_exchange_quote
    ///
    /// The counter's remaining stock is retail's `_DAT_8007BB90`, a global the
    /// port has no producer for; this host stands in the full bank cap, so the
    /// stock gate only ever bites on an absurd request.
    fn buy_casino_coins(&mut self, coins: i32) -> Option<i32> {
        use legaia_engine_core::slot_machine::{
            BALANCE_CAP, COIN_ENTRY_DIGITS, coin_exchange_quote,
        };
        // The entry field is COIN_ENTRY_DIGITS single-digit cells, units first.
        let mut digits = [0u8; COIN_ENTRY_DIGITS];
        let mut n = coins.max(0);
        for d in digits.iter_mut() {
            *d = (n % 10) as u8;
            n /= 10;
        }
        let gold = self.session.host.world.money;
        let quote = coin_exchange_quote(&digits, gold, BALANCE_CAP);
        if !quote.is_valid() {
            log::info!(
                "slots: coin counter refused {} coins ({} gold, have {gold}; in stock: {})",
                quote.coins,
                quote.cost,
                quote.in_stock
            );
            return None;
        }
        self.session.host.world.money = gold - quote.cost;
        let bank = self.session.host.world.casino_coins as i32 + quote.coins;
        self.session.host.world.casino_coins = bank.max(0) as u32;
        log::info!(
            "slots: bought {} coins for {} gold at the exchange counter (bank {bank})",
            quote.coins,
            quote.cost
        );
        Some(bank)
    }

    /// Load the Baka Fighter overlay (PROT 0976), parse the roster + action
    /// tables, and enter a best-of-3 duel: the player fights as roster
    /// fighter 0 against a ladder opponent picked from the roster (rotating
    /// with the frame counter so repeat entries vary). Returns `false` (with
    /// a log line) when the overlay or tables don't resolve.
    pub(super) fn start_baka_minigame(&mut self) -> bool {
        use legaia_asset::static_overlay;
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("baka: overlay 0976 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("baka: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("baka: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(opponents) = legaia_asset::baka_opponents::parse(&loaded) else {
            log::warn!("baka: roster-table parse failed");
            return false;
        };
        let Some(actions) = legaia_asset::baka_opponents::parse_actions(&loaded) else {
            log::warn!("baka: action-table parse failed");
            return false;
        };
        // Rotate the ladder opponent with the frame counter (1..=16; roster 0
        // is the player-side default). Seed like the slot machine: frame-
        // derived, deterministic across a replayed pad stream.
        let frame = self.session.host.world.frame as u32;
        let opponent = 1 + (frame as usize % (opponents.len().saturating_sub(1).max(1)));
        let seed = 0xBA4A_F19A ^ frame;
        let Some(fight) = legaia_engine_core::baka_fighter::BakaFight::from_tables(
            &opponents, &actions, 0, opponent, seed,
        ) else {
            log::warn!("baka: fight construction failed (roster 0 vs {opponent})");
            return false;
        };
        log::info!(
            "baka: round 1 vs roster fighter {opponent} (gold prize {})",
            fight.gold_reward()
        );
        self.session.host.world.enter_baka_fighter(fight);
        // The duel overlay init (FUN_801CF00C) loads its own track: global
        // BGM 2053 = music_01 slot 53, the boss overture.
        self.session.start_global_bgm(2053);
        true
    }

    /// Load the Muscle Dome hand tables from the battle overlay (PROT 0898)
    /// and enter a contest. The player's card costs come from their own
    /// player battle file's equipped-section swing records (`+0x74`, the
    /// same bytes the Arts gauge reads); the opponent plays a flat
    /// favored-cost hand, and HP / budgets are dev constants (retail seeds
    /// them from the arena's battle-actor records). Returns `false` (with a
    /// log line) when the tables don't resolve.
    pub(super) fn start_muscle_minigame(&mut self) -> bool {
        use legaia_asset::muscle_dome as md;
        use legaia_asset::static_overlay;
        use legaia_engine_core::muscle_dome::{MuscleCard, MuscleDomeSession};
        let Some(rec) =
            static_overlay::overlay_map().by_prot_index(md::MUSCLE_OVERLAY_PROT_INDEX as u32)
        else {
            log::warn!("muscle: battle overlay 0898 absent from the static-overlay map");
            return false;
        };
        let raw = match self.session.host.index.entry_bytes_extended(rec.prot_index) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle: PROT {} read failed: {e:#}", rec.prot_index);
                return false;
            }
        };
        let loaded = match static_overlay::as_loaded(&raw, rec) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("muscle: as_loaded failed: {e:#}");
                return false;
            }
        };
        let Some(commands) = md::hand_command_ids(&loaded) else {
            log::warn!("muscle: hand command-id table failed its structural check");
            return false;
        };
        // Player card costs: the lead character's equipped-section swing
        // records, keyed by runtime slot = the command id.
        const FAVORED_COST: u16 = 0x1E;
        let mut player_costs = [FAVORED_COST; 4];
        if let Some(costs) = self.lead_swing_costs() {
            for (i, &cmd) in commands.iter().enumerate() {
                if let Some(&c) = costs.get((cmd - 0x0C) as usize)
                    && c > 0
                {
                    player_costs[i] = c as u16;
                }
            }
        } else {
            log::info!("muscle: lead swing costs unavailable - flat favored costs");
        }
        let card = |cmd: u8, cost: u16| MuscleCard {
            command_id: cmd,
            cost,
        };
        let player_hand = std::array::from_fn(|i| card(commands[i], player_costs[i]));
        let opp_hand = std::array::from_fn(|i| card(commands[i], FAVORED_COST));
        // Dev stand-ins for the arena actor records (budget pool +0x154, HP
        // +0x14c/+0x14e) and the awarded Seru (ctx+0x269 = 1 → spell 0x81).
        let session = MuscleDomeSession::new(player_hand, opp_hand, [120, 120], [500, 400], 1);
        log::info!(
            "muscle: contest started - hand commands {commands:02x?}, player costs {player_costs:?}"
        );
        self.session.host.world.enter_muscle_dome(session);
        // The arena loads no track of its own - it reuses the battle engine,
        // so it plays a battle theme. Use the standard random-battle theme
        // (global BGM 2026 = music_01 slot 26, M26B1); see
        // docs/subsystems/minigame-muscle-dome.md.
        self.session.start_global_bgm(2026);
        true
    }
}
