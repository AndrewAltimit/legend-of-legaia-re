//! What a battle frame clears to.
//!
//! Retail's stage dome is a **front half**: the arena mesh covers the ground
//! and the horizon the camera looks at and leaves the rest of the frame open,
//! so whatever the framebuffer was cleared to is what the player reads as sky.
//! Clearing to the renderer's ordinary dark ground turns that open band into a
//! black ceiling.
//!
//! The value lives here rather than in either host because it is not a
//! renderer preference: it is part of what a battle looks like, and the two
//! hosts answered it differently - the native window selected it per frame
//! while the browser play page's WebGL path cleared every mode, battle
//! included, to one hard-coded near-black.

/// The sky the front-half stage dome leaves open, as linear RGBA.
pub const BATTLE_SKY_CLEAR: [f32; 4] = [0.32, 0.46, 0.66, 1.0];

/// The ordinary scene clear - the dark ground every non-battle 3D draw uses on
/// both hosts.
pub const SCENE_CLEAR: [f32; 4] = [0.04, 0.05, 0.08, 1.0];

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
        BATTLE_SKY_CLEAR
    } else {
        SCENE_CLEAR
    }
}
