//! Battle arms the retail kernel differential (`psx-retail-exec`) holds to
//! the bytes one arm at a time.
//!
//! Each method is one `World` step a retail routine's arm ports, kept
//! `pub(in crate::world)` for every other caller and re-exposed here under the
//! `test-drivers` feature only, as [`super::test_drivers`] does for the
//! battle-screen steps. The caller seats the world the arm reads and asserts
//! what it wrote; the gate the arm's own caller applies stays with that caller.

use super::World;

pub trait BattleArmDrivers {
    /// The anim commit's strike-loop reach snap
    /// (`World::commit_reach_snap`, `FUN_8004AD80` `0x8004BF7C..0x8004C118`).
    fn reach_snap(&mut self, slot: usize);
    /// The presentation tick's defeat-fade bookkeeping
    /// (`World::tick_battle_defeat_sink`, `FUN_80050120`
    /// `0x80050384..0x800504E8`): the Jette count, the sink, the lone latch.
    fn defeat_sink(&mut self);
    /// The formation's second cell as the battle reads it (`gp+0x9F5`).
    fn formation_second_cell(&self) -> u8;
}

impl BattleArmDrivers for World {
    fn reach_snap(&mut self, slot: usize) {
        World::commit_reach_snap(self, slot)
    }
    fn defeat_sink(&mut self) {
        World::tick_battle_defeat_sink(self)
    }
    fn formation_second_cell(&self) -> u8 {
        World::battle_formation_cell(self, 1)
    }
}
