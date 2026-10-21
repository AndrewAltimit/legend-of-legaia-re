//! The global mode-cell setter.
//!
//! PORT: FUN_8003C110
//! REF: FUN_800353E0, FUN_801DD35C
//!
//! `FUN_8003C110` is a one-instruction setter: a *byte* store (`sb`) of its
//! argument into the global `DAT_80073F20`. Observed writers: the
//! field-subsystem state reset `FUN_800353E0` sets it to `0x0C`, the menu
//! dispatcher `FUN_801DD35C` to `0x10`.

use super::*;

impl SceneHost {
    /// Set the global mode cell (`DAT_80073F20`, a byte) - the
    /// `FUN_8003C110` setter. Retail-observed values: `0x0C` from the
    /// field-subsystem reset, `0x10` from the menu dispatcher.
    pub fn set_mode_cell(&mut self, value: u8) {
        self.mode_cell = value;
    }

    /// Current value of the global mode cell (`DAT_80073F20`). Zero until a
    /// setter runs (retail BSS default).
    pub fn mode_cell(&self) -> u8 {
        self.mode_cell
    }
}
