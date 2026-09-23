//! The field fog pool's scene-entry install and per-frame render step, as
//! `World` methods - the seam both hosts draw the fog through.
//!
//! Retail splits the system across three sites: MAIN INIT allocates the pool
//! and the MAN installer (`FUN_8003AEB0`) hands the scene's section-4 table
//! to `DAT_80073ED8` and clears the gate; the ambient emitter spawns into
//! the pool from the actor pass; and the field render pass (`0x80026EBC` in
//! `FUN_80026CE4`) ages, moves and draws the pool when the game mode is `3`
//! and the gate word is set. The first two land here and in
//! `World::tick_cutscene_elements`; the third is [`World::fog_render_step`],
//! which each host calls from its draw path with the camera it is about to
//! draw the scene through, so the sheets project through the same matrix
//! as the geometry.

use super::*;
use crate::fog_particles::{FOG_CAP_DEFAULT, FOG_DPAD_MASK, FogFrameEnv, FogQuad, FogView};
use legaia_engine_vm::psx_camera::FieldCameraView;

/// MAN sibling-section index (`sibling_sections()[3]`) that installs into
/// `DAT_80073ED8` - section 4 of the six.
const FOG_REGION_SIBLING_SECTION: usize = 3;

impl World {
    /// The seated player's world position (`+0x14 / +0x16 / +0x18` of the
    /// player actor) as the fog code reads it: the player actor's move
    /// state, which is where scene entry seats the player and the field
    /// loop moves it; the VM ctx position is the fallback for a world with no
    /// player actor.
    pub fn fog_player_world_pos(&self) -> [i32; 3] {
        let slot = usize::from(self.player_actor_slot.unwrap_or(0));
        match self.actors.get(slot) {
            Some(a) => [
                i32::from(a.move_state.world_x),
                i32::from(a.move_state.world_y),
                i32::from(a.move_state.world_z),
            ],
            None => [
                i32::from(self.field_ctx.world_x as i16),
                i32::from(self.field_ctx.world_y as i16),
                i32::from(self.field_ctx.world_z as i16),
            ],
        }
    }

    /// Scene-entry reset: a fresh pool, the gate clear, no regions, the
    /// default cap - what MAIN INIT's allocation (`0x801D7364`) and the field
    /// reset's `sw zero,-0x47ac` + `sw a0,-0x4350` (`0x8003B690` /
    /// `0x8003B6E8`) leave behind on every field entry.
    ///
    /// REF: FUN_801D6704, FUN_8003AEB0
    pub fn reset_fog_for_scene_entry(&mut self) {
        self.fog.reset();
        self.fog.gate = false;
        self.fog.regions.clear();
        self.fog.cap = FOG_CAP_DEFAULT;
    }

    /// Install the scene MAN's section-4 fog-region table - the
    /// `DAT_80073ED8` / `DAT_80073EDC` pair `FUN_8003AEB0` sets from the
    /// section (`+3` = count, records from `+4`) - and the pool cap the same
    /// installer derives from the MAN header
    /// ([`crate::fog_particles::fog_cap_for_man`]).
    ///
    /// REF: FUN_8003AEB0
    pub fn install_fog_regions(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        self.fog.regions = man_file
            .sibling_sections()
            .get(FOG_REGION_SIBLING_SECTION)
            .and_then(|s| s.body(man))
            .map(crate::fog_particles::parse_fog_regions)
            .unwrap_or_default();
        // `_DAT_8007BCB0`, seated by the same installer from the MAN's
        // header byte `+1` (`0x8003B6BC..0x8003B6E8`).
        self.fog.cap = crate::fog_particles::fog_cap_for_man(man);
    }

    /// One rendered frame of the fog pool through `view` - the field render
    /// pass's `0x80026EA4..0x80026F28`: nothing outside game mode `3`
    /// ([`SceneMode::Field`]) or while the gate word is clear; otherwise the
    /// pool walk with this frame's `dt`, player position, held pad, camera
    /// vertical offset and global tint.
    ///
    /// Returns the quads to composite this frame, also kept in
    /// [`World::fog_quads`]. The `dt` is every tick since the previous call
    /// (retail's `DAT_1F800393` as the render pass sees it); a call with no
    /// tick in between re-emits the same frame.
    ///
    /// REF: FUN_80026CE4 (the pass; its fog stage at `0x80026EA4`)
    pub fn fog_render_step(&mut self, view: &FieldCameraView) -> &[FogQuad] {
        let dt = self.fog.pending_dt.min(u32::from(u8::MAX)) as u8;
        self.fog.pending_dt = 0;
        if self.mode != SceneMode::Field || !self.fog.gate {
            self.fog.quads.clear();
            return &self.fog.quads;
        }
        // `_DAT_8007BCB8..BA`: the op `0x4C 0x12` global multiply tint,
        // `0x80` neutral - the same ramp the scene grade reads.
        let tint = self
            .presentation
            .tint
            .as_ref()
            .map(|t| {
                t.factor()
                    .map(|f| (f * 128.0).round().clamp(0.0, 255.0) as u8)
            })
            .unwrap_or([0x80; 3]);
        let env = FogFrameEnv {
            dt,
            player: self.fog_player_world_pos(),
            dpad_held: self.input.retail_pad().held & FOG_DPAD_MASK != 0,
            y_offset: self.camera.offset_ease,
            tint,
            window: self.terrain.region_attributes.box_bytes,
        };
        self.fog.render_step(&FogView::from_field_view(view), &env)
    }

    /// The quads the last [`World::fog_render_step`] produced.
    pub fn fog_quads(&self) -> &[FogQuad] {
        &self.fog.quads
    }
}
