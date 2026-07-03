//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
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
    /// (`World::casino_coins`, the retail `_DAT_800845A4`); a thin bank is
    /// fronted a dev stake so the dev entry point is playable. The final
    /// balance commits back to the bank on exit (`World::exit_slot_machine`).
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
        // Dev stake when the bank can't cover a spin (the retail entry path
        // arrives through the casino with coins already banked).
        const DEV_STAKE: i32 = 100;
        let bank = self.session.host.world.casino_coins as i32;
        let balance = if bank >= legaia_engine_core::slot_machine::MIN_SPIN_BALANCE {
            bank
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
        true
    }
}
