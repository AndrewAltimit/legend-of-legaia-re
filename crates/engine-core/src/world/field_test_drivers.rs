//! The field-side state a test above this crate seeds and reads directly.
//!
//! `psx-retail-exec`'s field-VM differentials step the real [`World`] on
//! state they generate and compare what a field op leaves against the
//! retail kernel. Some of that state is `pub(crate)` for every other caller;
//! this trait re-exposes the few words those tests need under their own
//! names, and only when the `test-drivers` feature is on, which only test
//! crates' `[dev-dependencies]` enable (the battle side is
//! [`super::test_drivers::BattleTestDrivers`]).

use super::World;

pub trait FieldTestDrivers {
    /// The screen-effect colour tween's latched target RGB, retail's
    /// `_DAT_8007BCCD..CF` (op `34` sub `0`).
    fn effect_target_rgb(&self) -> [i16; 3];
    /// Seed that latch, as a previous `34 0x` left it.
    fn seed_effect_target_rgb(&mut self, rgb: [i16; 3]);
    /// One tick of the tile board walker's state 2 on actor `slot`
    /// (`World::tile_board_walk_step`, `FUN_801EF2B0` `0x801EFA88`) toward
    /// the committed target, `game_frame` whether the tick is a game frame;
    /// a no-op with none.
    fn tile_board_walk(&mut self, slot: usize, game_frame: bool);
}

impl FieldTestDrivers for World {
    fn effect_target_rgb(&self) -> [i16; 3] {
        self.presentation.effect_target_rgb
    }
    fn seed_effect_target_rgb(&mut self, rgb: [i16; 3]) {
        self.presentation.effect_target_rgb = rgb;
    }
    fn tile_board_walk(&mut self, slot: usize, game_frame: bool) {
        if let Some((tx, tz)) = self.board.target {
            self.tile_board_walk_step(slot, tx, tz, game_frame);
        }
    }
}
