//! What a battle frame clears to.
//!
//! **Black**, the same as every field frame. The draw-environment pair the
//! frame-begin driver `FUN_80016B6C` swaps (`0x8007BF30 + i * 0x74`; its
//! `dtd` byte is `+0x2A`, so the libgpu `DRAWENV` starts at `+0x14`) holds a
//! background colour `+0x2D..+0x2F` of `(0, 0, 0)` in every catalogued
//! battle state, where field states carry scene tints there. Wherever a
//! stage shell leaves the frame open - a cave or castle stage whose shell
//! has no sky panel, the Gaza fight's `korb3` - the retail display crop is
//! black, and the shell's own sky panels are the only sky a battle has.
//!
//! The port once cleared a stage battle to a sky blue, on the reading that
//! the shell is a front half whose open side is meant to read as sky. The
//! shell is completed by its second copy
//! (`docs/subsystems/battle.md`, backdrop shell - two copies of one mesh), so
//! nothing was owed there, and the blue painted every roofless stage's
//! ceiling a colour retail never draws.
//!
//! The value lives here rather than in either host because both hosts must
//! answer it the same way - the native window selects it per frame, the
//! browser play page reads it through `play_scene_clear_color`.

/// The ordinary scene clear - what every non-battle 3D frame shows wherever no
/// geometry covers it: **black**, retail's field and cutscene background.
/// Measured on retail display crops, the uncovered pixels read `(0, 0, 0)` in
/// every field-mode capture checked (an interior, a ravine, the prologue
/// tableau, the naming screen, a town dialogue beat).
///
/// It was a dark navy `[0.04, 0.05, 0.08]`, and only one host used it: the
/// native window passed no clear outside the boot UI and a stage battle, so
/// its renderer's own fallback `[0.05, 0.05, 0.07]` stood - two different
/// non-retail colours for the one background, visible behind the prologue
/// narration, the naming prompt and every gap in a scene's geometry.
pub const SCENE_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// A stage battle's clear: retail black ([module docs](self)).
pub const BATTLE_STAGE_CLEAR: [f32; 4] = SCENE_CLEAR;

/// Pure black, for the boot UI: the logos / title / save-select panels read on
/// PSX-style black rather than on a dark-blue clear.
pub const BOOT_UI_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The clear colour for one frame, given the two things that select it.
///
/// `boot_ui` wins over `stage_battle`; with neither, the scene clear stands.
pub fn scene_clear(boot_ui: bool, stage_battle: bool) -> [f32; 4] {
    if boot_ui {
        BOOT_UI_CLEAR
    } else if stage_battle {
        BATTLE_STAGE_CLEAR
    } else {
        SCENE_CLEAR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_clears_to_retail_black() {
        assert_eq!(scene_clear(false, false), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(scene_clear(true, false), BOOT_UI_CLEAR);
        assert_eq!(scene_clear(true, true), BOOT_UI_CLEAR, "boot UI wins");
        assert_eq!(scene_clear(false, true), [0.0, 0.0, 0.0, 1.0]);
    }
}
