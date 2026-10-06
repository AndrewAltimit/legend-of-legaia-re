//! The volumetric ground-fog enhancement on the play page
//! (`legaia_engine_core::fog_volume`): the toggle and the per-frame bank,
//! the twin of the native window's `F9` and `stage_fog_volume`.
//!
//! The bank is the engine's - stepped inside `World::tick` while
//! `WorldToggles::volumetric_fog` is raised, which this page drives from the
//! persisted `OptionsState::volumetric_fog` through the same
//! `apply_to_world` push the native window makes. The page reads it back
//! through four exports and draws it in `site/js/webgl-fog-volume.js`
//! (the GLSL transcription of the native `fog_volume` WGSL).

use legaia_engine_core::fog_volume::{FogVolumeFrame, mesh_indices};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    fn fog_volume_frame(&self) -> Option<FogVolumeFrame<'_>> {
        self.scene_host
            .host()
            .and_then(|h| h.world.fog_volume_frame())
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Turn the volumetric ground-fog enhancement on / off - the page's
    /// "Ground fog" box, the native window's `F9`. Persisted with the other
    /// options; off resets the bank and the page draws the frame without it.
    pub fn set_volumetric_fog(&mut self, on: bool) {
        if self.options_state.volumetric_fog != on {
            self.options_state.volumetric_fog = on;
            self.persist_and_apply_options();
        } else {
            self.apply_options_side_effects();
        }
    }

    /// Whether the volumetric ground fog is on (the persisted option).
    pub fn volumetric_fog(&self) -> bool {
        self.options_state.volumetric_fog
    }

    /// This frame's bank scalars in `legaia_engine_core::fog_volume::header`
    /// order, or empty when there is no bank (toggle down, a mode without
    /// one, no style raised).
    pub fn play_fog_volume_header(&self) -> Vec<f32> {
        let mut h = self
            .fog_volume_frame()
            .map(|f| f.header())
            .unwrap_or_default();
        // Enhanced lighting: the mist sits in the scene's mood light (the
        // native window's `stage_fog_volume` rule).
        if self.options_state.enhanced_lighting && !h.is_empty() {
            use legaia_engine_core::fog_volume::header as hd;
            let c = legaia_engine_ui::scene_lighting::fog_tint(
                [h[hd::COLOR_R], h[hd::COLOR_G], h[hd::COLOR_B]],
                &self.lighting_mood(),
            );
            h[hd::COLOR_R] = c[0];
            h[hd::COLOR_G] = c[1];
            h[hd::COLOR_B] = c[2];
        }
        h
    }

    /// This frame's disturbance grid, `SIM_DIM`² bytes row-major `[z][x]`
    /// (`255` = undisturbed); empty when there is no bank.
    pub fn play_fog_volume_density(&self) -> Vec<u8> {
        self.fog_volume_frame()
            .map(|f| f.density)
            .unwrap_or_default()
    }

    /// The sheet mesh's `[x, floor_y, z, floor_weight]` vertices (retail
    /// Y-down), to
    /// re-upload whenever the header's ground generation changes; empty when
    /// there is no bank.
    pub fn play_fog_volume_mesh(&self) -> Vec<f32> {
        self.fog_volume_frame()
            .map(|f| f.mesh_positions())
            .unwrap_or_default()
    }

    /// The sheet mesh's triangle list (constant).
    pub fn play_fog_volume_indices(&self) -> Vec<u32> {
        mesh_indices()
    }
}
