//! Host half of the **mode-24 minigame door-warp**: drain
//! [`World::pending_minigame_warp`], load the selected minigame's overlay off
//! the disc, and enter it.
//!
//! This is the missing link between the field VM and the five minigame rules
//! engines. The VM arm ([`legaia_engine_vm::field::host::FieldHost::minigame_door_warp`],
//! retail `FUN_801DE840` case `0x3e`) only publishes a `sub_id`; retail's
//! mode-24 init `FUN_80025980` is what turns that into a loaded overlay
//! (`FUN_8003EBE4(sub_id + 0x4D)`) and calls its init entry. This module is
//! that init: it resolves the same PROT entry through
//! [`crate::minigame_entry::MinigameSubId::prot_index`], parses the overlay's
//! own tables, and installs the session.
//!
//! ## Why the overlay is the data source
//!
//! Each minigame's tuning data lives *in* the overlay the warp loads - the
//! fishing species table in `0972`, the slot payout table in `0975`, the Baka
//! roster in `0976`, the dome course ladder in `0977`, the dance chart in
//! `0980`. That is the same load the native host's debug hotkeys perform, so
//! wiring the warp does not introduce a second data path; it introduces the
//! *first player-reachable* caller of the existing one.
//!
//! ## The two dev slots
//!
//! `sub_id` 1 and 2 (`OTHER2` / `OTHER3`) are dev modules with no shipped
//! gameplay. A warp naming one is still a genuine retail site, so the host
//! completes the round trip immediately (arm + return warp) rather than
//! parking the script on a mode it can never leave.
// REF: FUN_80025980 (mode-24 OTHER INIT - the overlay load this mirrors)

use super::*;
use crate::minigame_entry::MinigameSubId;

/// What draining a pending door-warp did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinigameWarpOutcome {
    /// The minigame was entered; the world is now in its scene mode.
    Entered(MinigameSubId),
    /// The slot is a dev module the engine does not implement - the round trip
    /// completed immediately and the world is back in the field.
    DevSlotSkipped(MinigameSubId),
    /// The overlay read or table parse failed (no disc attached, or the entry
    /// did not decode). The round trip completed so the script can continue.
    LoadFailed(MinigameSubId),
    /// `sub_id` was outside the 7-id warp space - a desynced walk, not a
    /// minigame. Nothing happened.
    UnknownSubId(u8),
}

impl SceneHost {
    /// Read the overlay for `slot` in its statically-recovered loaded form.
    fn minigame_overlay_bytes(&self, slot: MinigameSubId) -> Option<Vec<u8>> {
        use legaia_asset::static_overlay;
        let rec = static_overlay::overlay_map().by_prot_index(slot.prot_index())?;
        let raw = self.index.entry_bytes_extended(rec.prot_index).ok()?;
        static_overlay::as_loaded(&raw, rec).ok()
    }

    /// Drain a pending mode-24 door-warp, if one is armed.
    ///
    /// The winnings-accumulator zero and the departure-scene backup already ran
    /// in the VM host's arm ([`World::arm_minigame_warp`]), so this only has to
    /// load and install. Every failure arm completes the round trip through
    /// [`World::minigame_return_warp`] - a script that armed a warp must never
    /// be left in a mode with no exit.
    pub fn drain_minigame_warp(&mut self) -> Option<MinigameWarpOutcome> {
        let sub_id = self.world.pending_minigame_warp.take()?;
        let Some(slot) = MinigameSubId::from_sub_id(sub_id) else {
            return Some(MinigameWarpOutcome::UnknownSubId(sub_id));
        };
        if !slot.is_playable() {
            self.world.minigame_return_warp();
            return Some(MinigameWarpOutcome::DevSlotSkipped(slot));
        }
        let Some(loaded) = self.minigame_overlay_bytes(slot) else {
            self.world.minigame_return_warp();
            return Some(MinigameWarpOutcome::LoadFailed(slot));
        };
        let entered = match slot {
            MinigameSubId::Fishing => self.enter_fishing_from_overlay(&loaded),
            MinigameSubId::SlotMachine => self.enter_slot_from_overlay(&loaded),
            MinigameSubId::BakaFighter => self.enter_baka_from_overlay(&loaded),
            MinigameSubId::MuscleDome => self.enter_muscle_from_overlay(&loaded),
            MinigameSubId::Dance => self.enter_dance_from_overlay(&loaded),
            MinigameSubId::Other2 | MinigameSubId::Other3 => false,
        };
        if entered {
            Some(MinigameWarpOutcome::Entered(slot))
        } else {
            self.world.minigame_return_warp();
            Some(MinigameWarpOutcome::LoadFailed(slot))
        }
    }

    /// PROT 0972's per-species table -> a live [`crate::fishing::FishingSession`].
    ///
    /// The rod stat is the venue default; the record resumes the world's
    /// persistent point pool, which `exit_fishing` banks back.
    fn enter_fishing_from_overlay(&mut self, loaded: &[u8]) -> bool {
        /// Default rod stat until the save block's rod is wired.
        const DEFAULT_ROD_STAT: i32 = 4;
        let Some(species) = legaia_asset::fishing_species::parse(loaded) else {
            return false;
        };
        let record = crate::fishing::FishingRecord {
            points: self.world.fishing_points,
            ..Default::default()
        };
        self.world
            .enter_fishing(crate::fishing::FishingSession::new(
                species,
                DEFAULT_ROD_STAT,
                record,
            ));
        true
    }

    /// PROT 0975's payout table -> a live [`crate::slot_machine::SlotMachine`].
    ///
    /// The playing balance is assigned from the casino coin bank, which is the
    /// overlay init's own `DAT_801d4114 = _DAT_800845A4`; `exit_slot_machine`
    /// performs the symmetric state-100 commit back.
    fn enter_slot_from_overlay(&mut self, loaded: &[u8]) -> bool {
        /// The literal LCG seed the slot overlay's init writes to
        /// `DAT_801d3c80`.
        const SLOT_RNG_SEED: u32 = 0x6C0A_2AF0;
        let Some(payouts) = legaia_asset::slot_payout::parse(loaded) else {
            return false;
        };
        let balance = self.world.casino_coins as i32;
        self.world
            .enter_slot_machine(crate::slot_machine::SlotMachine::new(
                payouts,
                SLOT_RNG_SEED,
                balance,
            ));
        true
    }

    /// PROT 0976's roster + action tables -> a live
    /// [`crate::baka_fighter::BakaFight`].
    ///
    /// Roster `0` is the player-side default; the ladder opponent rotates with
    /// the frame counter so a repeat entry varies while a replayed pad stream
    /// stays deterministic.
    fn enter_baka_from_overlay(&mut self, loaded: &[u8]) -> bool {
        let Some(opponents) = legaia_asset::baka_opponents::parse(loaded) else {
            return false;
        };
        let Some(actions) = legaia_asset::baka_opponents::parse_actions(loaded) else {
            return false;
        };
        let frame = self.world.frame as u32;
        let opponent = 1 + (frame as usize % opponents.len().saturating_sub(1).max(1));
        let seed = 0xBA4A_F19A ^ frame;
        let Some(fight) =
            crate::baka_fighter::BakaFight::from_tables(&opponents, &actions, 0, opponent, seed)
        else {
            return false;
        };
        self.world.enter_baka_fighter(fight);
        true
    }

    /// The battle overlay's hand command-id table -> a live
    /// [`crate::muscle_dome::MuscleDomeSession`].
    ///
    /// `loaded` here is PROT **0977** (the arena door/init slot the warp
    /// selects), but the contest's own tables live in the battle overlay
    /// (PROT 0898) - the arena reuses the battle engine wholesale. Both are
    /// read; the damage model is installed when 0898 decodes, and the leg
    /// resolves without one when it does not (the same disclosed fallback the
    /// native launcher takes).
    fn enter_muscle_from_overlay(&mut self, _loaded: &[u8]) -> bool {
        use crate::muscle_dome::{DomeCombatant, DomeDamageModel, MuscleCard, MuscleDomeSession};
        use legaia_asset::muscle_dome as md;
        use legaia_asset::static_overlay;

        /// Flat favored-class swing cost, until the lead's equipped swing
        /// records are threaded through the warp.
        const FAVORED_COST: u16 = 0x1E;
        /// Stand-in turn budget / HP for both fighters - the arena stages its
        /// own `(course, round)` from story flags, which a door warp does not
        /// carry.
        const STANDIN_BUDGET: u16 = 120;
        const STANDIN_HP: i32 = 400;
        /// The victory caption's Seru index. It names a *string*, not a prize.
        const CAPTION_SERU_INDEX: u8 = 1;
        const STANDIN_COMBATANT: DomeCombatant = DomeCombatant {
            hp_max: STANDIN_HP as u16,
            int: 60,
            udf: 40,
            ldf: 40,
            element: 0,
        };

        let Some(rec) =
            static_overlay::overlay_map().by_prot_index(md::MUSCLE_OVERLAY_PROT_INDEX as u32)
        else {
            return false;
        };
        let Ok(raw) = self.index.entry_bytes_extended(rec.prot_index) else {
            return false;
        };
        let Ok(battle) = static_overlay::as_loaded(&raw, rec) else {
            return false;
        };
        let Some(commands) = md::hand_command_ids(&battle) else {
            return false;
        };
        let hand: [MuscleCard; crate::muscle_dome::HAND_SLOTS] =
            std::array::from_fn(|i| MuscleCard {
                command_id: commands[i],
                cost: FAVORED_COST,
            });
        let mut session = MuscleDomeSession::new(
            hand,
            hand,
            [STANDIN_BUDGET; 2],
            [STANDIN_HP; 2],
            CAPTION_SERU_INDEX,
        );
        let seed = 0x4D55_5343 ^ self.world.frame as u32;
        if let Some(model) = DomeDamageModel::from_battle_overlay(
            &raw,
            [STANDIN_COMBATANT; 2],
            [STANDIN_HP; 2],
            seed,
        ) {
            session.install_damage_model(model);
        }
        self.world.enter_muscle_dome(session);
        true
    }

    /// PROT 0980's baked step chart -> a live [`crate::dance::DanceGame`].
    ///
    /// The door warp opens the qualifier (`yosenn`) on the short song; which
    /// heat is staged is the hall's own story-flag state, not the warp's.
    fn enter_dance_from_overlay(&mut self, loaded: &[u8]) -> bool {
        let Some(game) = crate::dance::DanceGame::from_overlay_for_mode(
            loaded,
            crate::dance::DanceMode::Qualifier,
            false,
        ) else {
            return false;
        };
        self.world.enter_dance(game);
        true
    }
}
