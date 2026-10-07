//! The battle-**intro** enemy-name banner's lifetime: retail's flow state
//! `0x0A` (`FUN_801D0748`, `0x801D0DE0..0x801D0E38`) runs the composer
//! `FUN_801D9D3C` and seeds the intro timer `ctx[+0x6D6]`, and state `0x0B`
//! drains it by the frame step until it expires and `FUN_800355F0` sweeps
//! every text actor away. The labels themselves are built per frame by
//! `battle_hud::battle_intro_names`; this module owns the timer.
//!
//! The one arm that skips the composer: a formation whose first monster id
//! byte (`0x8007BD0C`) is `0xB5` stores flow `0x0C` directly and draws no
//! names (`0x801D0DEC..0x801D0E0C`), still seeding the `0x5A` hold.
//!
//! Retail also holds the round prompt back for the whole span; the port does
//! not park the command flow on it (the round prompt opens with the labels
//! still up), because every recorded replay and scripted battle ladder paces
//! its first input off the round prompt's current frame.
//!
//! REF: FUN_801D0748 (flow states `0x0A` / `0x0B`), FUN_801D9D3C

use super::*;

/// `ctx[+0x6D6]` for an ordinary open (`li v0,0x5a`, `0x801D0E08`).
pub const INTRO_HOLD_FRAMES: u16 = crate::battle_open::PLAIN_OPEN_FRAMES;
/// `ctx[+0x6D6]` when the formation roll produced an advantage
/// (`li v0,0x78` behind the `ctx[+0x290]` test, `0x801D0E30`).
pub const INTRO_HOLD_FRAMES_ADVANTAGE: u16 = crate::battle_open::BANNER_FRAMES;
/// The monster id that suppresses the names (`li v0,0xb5`, `0x801D0DF0`).
pub const INTRO_NAMELESS_MONSTER: u16 = 0xB5;

impl World {
    /// Seed the intro banner's timer at battle open. Must run before the
    /// formation arm clears `ctx[+0x290]`, which it reads.
    pub(in crate::world) fn arm_battle_intro_names(&mut self) {
        let first = self
            .actors
            .get(usize::from(self.party.party_count.clamp(1, 3)))
            .and_then(|a| a.battle_monster_id);
        if first == Some(INTRO_NAMELESS_MONSTER) {
            self.battle.intro_names_frames = 0;
            return;
        }
        self.battle.intro_names_frames = if self.battle_ctx.formation_advantage != 0 {
            INTRO_HOLD_FRAMES_ADVANTAGE
        } else {
            INTRO_HOLD_FRAMES
        };
    }

    /// Whether the battle camera's entry sweep still owns the opening
    /// (`BattleCamera::start_entry_sweep`). Retail's frame driver
    /// `FUN_80046A20` calls the battle tick `FUN_801D0748` - whose flow
    /// states `0x0A` / `0x0B` compose and drain these labels - only once its
    /// entry counter `gp+0x330` reads `0xFF` (`beq a0,v0,0x80047014` at
    /// `0x80046EF8`, `jal 0x801D0748` at `0x80047014`), so the names come up
    /// after the sweep, never under it.
    pub fn battle_entry_sweeping(&self) -> bool {
        self.battle
            .camera
            .as_ref()
            .is_some_and(|c| c.entry_sweep_counter().is_some())
    }

    /// One tick of the intro timer: `ctx[+0x6D6] -= 0x1F800393` per battle
    /// pass, expiring at or below zero (`0x801D0E3C..0x801D0E58`) - one a
    /// vsync tick ([`super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK`]).
    /// The timer holds while the entry sweep runs
    /// ([`Self::battle_entry_sweeping`]).
    pub(in crate::world) fn step_battle_intro_names(&mut self) {
        if self.battle_entry_sweeping() {
            return;
        }
        let step = u16::from(super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK);
        self.battle.intro_names_frames = self.battle.intro_names_frames.saturating_sub(step);
    }
}
