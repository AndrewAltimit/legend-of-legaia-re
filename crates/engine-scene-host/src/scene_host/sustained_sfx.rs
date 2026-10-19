//! The scene host's side of the sustained-SFX teardown, and the global
//! mode-cell setter.
//!
//! PORT: FUN_8003C110
//! REF: FUN_80017910 (the teardown itself is
//! `legaia_engine_system::sustained_sfx::SustainedSfx::release`),
//!      FUN_800353E0, FUN_801DD35C
//!
//! `FUN_8003C110` is the one-instruction companion setter: a *byte* store
//! (`sb`) of its argument into the global `DAT_80073F20`. Observed writers:
//! the field-subsystem state reset `FUN_800353E0` sets it to `0x0C`, the menu
//! dispatcher `FUN_801DD35C` to `0x10`.

use super::*;

impl SceneHost {
    /// Release every held sustained-SFX voice - the `FUN_80017910` teardown
    /// over [`SceneHost::sustained_sfx`]. Returns the stopped voice ids in
    /// call order. Called by [`SceneHost::load_scene`] (mirroring the
    /// mode-init site `FUN_8001DCF8`); engines driving battle anim
    /// transitions call it directly (the `FUN_8004AD80` site).
    pub fn release_sustained_sfx(&mut self) -> Vec<u16> {
        self.sustained_sfx.release()
    }

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
