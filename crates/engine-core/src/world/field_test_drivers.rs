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
}

impl FieldTestDrivers for World {
    fn effect_target_rgb(&self) -> [i16; 3] {
        self.presentation.effect_target_rgb
    }
    fn seed_effect_target_rgb(&mut self, rgb: [i16; 3]) {
        self.presentation.effect_target_rgb = rgb;
    }
}
