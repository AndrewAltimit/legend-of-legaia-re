//! The battle **stage-id** byte `_DAT_8007B64A`: which stage overlay slot the
//! current fight reads, and so which arm the side-band tick
//! ([`crate::battle_sideband`]) takes.
//!
//! Retail treats the byte as last-writer-wins across five writers, each
//! ported at its own seat and writing [`crate::world::BattleState::stage_id`]:
//!
//! * the entity SM's battle-entry tail (`0` default / `1` tutorial,
//!   `FUN_801DA51C`) - `World::enter_battle` / `World::arm_battle_tutorial`;
//! * the battle initializer's per-formation override (`2` for formation
//!   monster `0xB5`, `FUN_80055B6C`) - `World::enter_battle_from_formation`;
//! * the arrival module's hand-back (`0`, PROT 0968 phase 6) -
//!   `World::tick_battle_sideband`;
//! * the mid-battle boss-transition arm (`3`, the tail of the Final Heal
//!   sweep `FUN_801E6968`) - [`World::run_boss_transition_arm`].
//!
//! Stage `1`'s behaviour is the sparring-prompt machine
//! (`world/battle/tutorial.rs`); stages `2` / `3` - the two phases of the
//! Cort fight - are the two stage modules
//! ([`crate::battle_stage_module`]), driven from the side-band.

use super::*;

impl World {
    /// First monster id of the active formation - the engine's view of the
    /// battle formation cell `_DAT_8007BD0C` both retail stage-override arms
    /// test.
    /// Retail's cell is one byte; an engine formation id above `0xFF` (a
    /// modded table) can never match the byte compare, so it resolves as
    /// "no override" rather than truncating onto an accidental match.
    pub(in crate::world) fn formation_slot0_monster_id(&self) -> Option<u8> {
        self.battle
            .active_formation
            .as_ref()
            .and_then(|f| f.slots.first())
            .and_then(|s| u8::try_from(s.monster_id).ok())
    }

    /// The stage id the current battle reads (`_DAT_8007B64A`).
    pub fn battle_stage_id(&self) -> u8 {
        self.battle.stage_id
    }

    /// The boss-transition tail of the Final Heal sweep (`FUN_801E6968`,
    /// `0x801E6CE4..0x801E6D64`), run at the head of cleanup state `0x50` -
    /// the one place retail calls the sweep (`jal 0x801E6968` at
    /// `0x801E5C6C`). When the formation cell reads `0xB5` and the first
    /// monster seat's live HP `+0x14C` is `0`, it pages entry 969 in, writes
    /// stage id `3`, bumps `ctx[+0x26]` and zeroes the seat's `+0x21C` /
    /// `+0x225`. Its `ctx[+0x07] = 0xFD` park is the caller's to apply after
    /// the rest of the `0x50` body has run (`World::live_battle_tick`): the
    /// state's advance to `0x51` is guarded on the byte still reading `0x50`,
    /// so the park stands, the end-of-action gate `0x5A` never runs, and
    /// nothing but the form-transition module ever closes this battle.
    /// Returns `true` when the arm fired.
    ///
    /// REF: FUN_801E6968 (the arm's kernel is
    /// [`crate::battle_stage_module::boss_transition_stage_id`])
    pub(in crate::world) fn run_boss_transition_arm(&mut self) -> bool {
        let Some(id) = self.formation_slot0_monster_id() else {
            return false;
        };
        let first_seat = usize::from(self.party.party_count.max(1));
        let Some(liveness) = self.actors.get(first_seat).map(|a| a.battle.liveness) else {
            return false;
        };
        let Some(stage) = crate::battle_stage_module::boss_transition_stage_id(id, liveness) else {
            return false;
        };
        self.battle.stage_id = stage;
        self.battle_ctx.levelup_banner_element =
            self.battle_ctx.levelup_banner_element.wrapping_add(1);
        let seat = &mut self.actors[first_seat].battle;
        seat.render_flag = 0;
        seat.capture_state = 0;
        true
    }
}
