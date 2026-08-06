//! The battle **stage-id** resolver: which stage overlay slot
//! (`_DAT_8007B64A`) the current fight reads.
//!
//! Retail treats the byte as last-writer-wins across four writers: the
//! entity SM's battle-entry tail (`0` default / `1` tutorial,
//! `FUN_801DA51C`), the battle initializer's per-formation override (`2`,
//! `FUN_80055B6C`), and the mid-battle boss-transition arm (`3`,
//! `FUN_801FD150`'s epilogue - overlay-band code the SCUS-only census
//! misses). The engine keeps no mutable byte; [`World::battle_stage_id`]
//! derives the same value from live battle state, which reproduces the
//! last-writer order because each later writer's guard implies the earlier
//! one's.
//!
//! The stage overlays themselves are MIPS *code* (extraction entries
//! `stage_id + 966`); the engine does not execute them. Stage `1`'s
//! behaviour is ported natively (the sparring-prompt machine,
//! `world/battle/tutorial.rs`); stages `2` / `3` - the two phases of the
//! `0xB5` boss fight - have no native behaviour port yet, so this resolver
//! is the pinned selection kernel, not a claim that a host pages 968/969.

use super::*;

impl World {
    /// First monster id of the active formation - the engine's view of the
    /// battle formation cell `_DAT_8007BD0C` both retail stage-override arms
    /// test.
    /// Retail's cell is one byte; an engine formation id above `0xFF` (a
    /// modded table) can never match the byte compare, so it resolves as
    /// "no override" rather than truncating onto an accidental match.
    fn formation_slot0_monster_id(&self) -> Option<u8> {
        self.active_formation
            .as_ref()
            .and_then(|f| f.slots.first())
            .and_then(|s| u8::try_from(s.monster_id).ok())
    }

    /// The stage id the current battle reads, derived from live state.
    ///
    /// * `3` - the `0xB5` formation's first monster seat is dead: the
    ///   mid-battle transition arm has fired
    ///   ([`crate::overlay_loader::boss_transition_stage_id`],
    ///   `FUN_801FD150` `0x801FD4D4..0x801FD548`). Retail reads the seat's
    ///   `+0x14C`, mirrored here as
    ///   [`legaia_engine_vm::battle_action::BattleActor::liveness`].
    /// * `2` - the `0xB5` formation, phase 1 still alive: the battle-init
    ///   override ([`crate::overlay_loader::battle_init_stage_override`],
    ///   `FUN_80055B6C` `0x80055D2C..0x80055D44`).
    /// * `1` - the armed sparring tutorial (`FUN_801DA51C`'s entry tail,
    ///   consumed by [`World::take_battle_tutorial_arm`]).
    /// * `0` - every other fight: no stage overlay.
    ///
    /// REF: FUN_801FD150, FUN_80055B6C, FUN_801DA51C
    pub fn battle_stage_id(&self) -> u8 {
        if let Some(id) = self.formation_slot0_monster_id() {
            let first_seat = self.party_count.max(1) as usize;
            let seat_liveness = self
                .actors
                .get(first_seat)
                .map(|a| a.battle.liveness)
                .unwrap_or(0);
            if let Some(stage) = crate::overlay_loader::boss_transition_stage_id(id, seat_liveness)
            {
                return stage;
            }
            if let Some(stage) = crate::overlay_loader::battle_init_stage_override(id) {
                return stage;
            }
        }
        if self.battle_tutorial.is_some() {
            return crate::battle_tutorial::TUTORIAL_STAGE_ID;
        }
        0
    }
}
