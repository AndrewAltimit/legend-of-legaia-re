//! The three selector inputs the battle-intro style picker reads, resolved
//! from the live scene host.
//!
//! The resolution itself is `World::battle_intro_style_inputs`: the world
//! needs it too, because the chosen style fixes the transition's length
//! (`DAT_801D2458`, 252 frames for the swirl). Both hosts arm the emitter
//! through this delegate so the two cannot drift.

use super::SceneHost;
use legaia_render_kernels::battle_intro_styles::IntroStyleInputs;

impl SceneHost {
    /// The battle's **first monster id** - retail's `DAT_8007BD0C`
    /// (`World::battle_intro_slot0`).
    pub fn battle_intro_slot0(&self, formation_id: u16) -> u8 {
        self.world.battle_intro_slot0(formation_id)
    }

    /// All three selector inputs for `formation_id`
    /// (`World::battle_intro_style_inputs`).
    pub fn battle_intro_style_inputs(&self, formation_id: u16) -> IntroStyleInputs {
        self.world.battle_intro_style_inputs(formation_id)
    }
}
