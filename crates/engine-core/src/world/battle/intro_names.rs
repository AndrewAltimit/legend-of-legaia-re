//! The battle-**intro** enemy-name banner's lifetime: retail's flow state
//! `0x0A` (`FUN_801D0748`, `0x801D0DE0..0x801D0E38`) runs the composer
//! `FUN_801D9D3C` and seeds the intro timer `ctx[+0x6D6]`, and state `0x0B`
//! drains it by the frame step until it expires and `FUN_800355F0` sweeps
//! every text actor away. The labels themselves are built per frame by
//! `legaia_engine_battle_hud::battle_intro_names`; this module owns the timer.
//!
//! The one arm that skips the composer: a formation whose first monster id
//! byte (`0x8007BD0C`) is `0xB5` stores flow `0x0C` directly and draws no
//! names (`0x801D0DEC..0x801D0E0C`), still seeding the `0x5A` hold.
//!
//! The same span holds the fight's first round back: `0x0B` leaves - to `0x14`,
//! or straight to the execute arm `0xFE` when `ctx[+0x290]` reads `1` - only on
//! the pass its timer expires, and sweeps the labels on that same pass
//! (`jal 0x800355F0` at `0x801D0E7C` / `0x801D0EB4`), so no prompt or action
//! ever runs under them. The port parks the first round start on
//! [`BattleState::battle_open_held`].
//!
//! REF: FUN_801D0748 (flow states `0x0A` / `0x0B`), FUN_801D9D3C

use super::*;

/// `ctx[+0x6D6]` for an ordinary open (`li v0,0x5a`, `0x801D0E08`).
pub const INTRO_HOLD_FRAMES: u16 = legaia_engine_battle_menus::battle_open::PLAIN_OPEN_FRAMES;
/// `ctx[+0x6D6]` when the formation roll produced an advantage
/// (`li v0,0x78` behind the `ctx[+0x290]` test, `0x801D0E30`).
pub const INTRO_HOLD_FRAMES_ADVANTAGE: u16 = legaia_engine_battle_menus::battle_open::BANNER_FRAMES;
/// Ticks from the entry sweep's park to the composer: two battle passes,
/// flow `0x00` then `0x0A`, each a frame long. Retail's frame step there is
/// `3`: the loader's frames are still in the sixteen-entry cost ring
/// (`gaza2_card_battle_init` and a `karisto_sol_pre_encounter` encounter both
/// run `0x00 -> 0x0A -> 0x0B` at `+3` / `+6` vsyncs past the park, and drop to
/// step `2` about thirty vsyncs later).
pub const INTRO_LEAD_TICKS: u8 = 6;
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
        self.battle.intro_lead_ticks = 0;
        if first == Some(INTRO_NAMELESS_MONSTER) {
            self.battle.intro_names_frames = 0;
            return;
        }
        self.battle.intro_lead_ticks = INTRO_LEAD_TICKS;
        self.battle.intro_names_frames = if self.battle_ctx.formation_advantage != 0 {
            INTRO_HOLD_FRAMES_ADVANTAGE
        } else {
            INTRO_HOLD_FRAMES
        };
    }

    /// The passes between the entry sweep's park and the composer. The last
    /// one is flow `0x0A`, which raises the formation banner the names stand
    /// with (`FUN_801D9D3C`); from there the intro timer drains.
    ///
    /// REF: FUN_801D0748 (flow states `0x00` / `0x0A`)
    pub(in crate::world) fn step_battle_intro_lead(&mut self) {
        if self.battle.intro_lead_ticks == 0 || self.battle.battle_entry_sweeping() {
            return;
        }
        self.battle.intro_lead_ticks -= 1;
        if self.battle.intro_lead_ticks == 0 {
            self.raise_battle_open_banner();
        }
    }

    /// Tick with an idle pad until the battle open has let the first round
    /// start, for a script that drives a fight from its first prompt.
    pub fn run_out_battle_open(&mut self) {
        for _ in 0..0x400 {
            if !self.battle.round_open_pending {
                return;
            }
            self.set_pad(0);
            let _ = self.tick();
        }
    }
}
