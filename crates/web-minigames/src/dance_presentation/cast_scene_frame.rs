//! The engine cast surface's per-frame scene: clip tick, frame build and the
//! combined scene buffers.

use super::*;

// ------------------------------------------------- the engine's cast surface

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Advance every floor body's clip `frames` frames - the run's own clip
    /// driver ([`DanceGame::advance_body_clips`]), which the play hosts'
    /// `World::tick_dance` steps every frame, count-in included. The judge
    /// binds each move clip into it ([`DanceGame::attach_clip_bank`]), so
    /// what a body plays is the run's, not this page's.
    ///
    /// The page used to animate the bodies in its own script - its own clip
    /// cursor and rate, its own pick of which move a press or a rival's beat
    /// triggers - beside the engine's driver.
    pub fn dance_body_clips_tick(&mut self, frames: u32) {
        if let Some(g) = self.dance.as_mut() {
            g.advance_body_clips(frames);
        }
    }

    /// Pose the floor's bodies for this frame through the engine's cast
    /// surface ([`legaia_engine_minigame_scenes::dance_cast_scene::DanceCastSurface::frame`]
    /// over this page's run - the call the native window and the play page
    /// make) and return its generation, or `-1` when no run is live or the
    /// cast did not decode. A generation the page has not seen means the
    /// static buffers changed (another mode's cast): re-read them.
    pub fn dance_scene_frame(&mut self) -> i32 {
        if !self.dance_surface.has_assets()
            && let Some((assets, _)) = self.dance_cast_assets()
        {
            self.dance_surface.set_assets(Some(assets));
        }
        match self.dance_surface.frame(self.dance.as_ref()) {
            Some(_) => self.dance_surface.generation() as i32,
            None => -1,
        }
    }

    /// This frame's posed positions, `[x, y, z]` per vertex, re-based on the
    /// hall origin - the frame [`Self::dance_env_positions`] is baked in.
    pub fn dance_scene_positions(&self) -> Vec<f32> {
        let (ox, oy, oz) = self.dance_cast_assets().map(|(_, o)| o).unwrap_or_default();
        self.dance_surface
            .scene()
            .map(|s| {
                s.positions
                    .iter()
                    .flat_map(|p| [p[0] - ox, p[1] - oy, p[2] - oz])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]`.
    pub fn dance_scene_uvs(&self) -> Vec<u8> {
        self.dance_surface
            .scene()
            .map(|s| s.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn dance_scene_cba_tsb(&self) -> Vec<u16> {
        self.dance_surface
            .scene()
            .map(|s| s.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, flag]` (the hybrid textured / fill layout).
    pub fn dance_scene_flat_rgba(&self) -> Vec<u8> {
        self.dance_surface
            .scene()
            .map(|s| s.flat_rgba.clone())
            .unwrap_or_default()
    }

    /// Triangle indices.
    pub fn dance_scene_indices(&self) -> Vec<u32> {
        self.dance_surface
            .scene()
            .map(|s| s.indices.clone())
            .unwrap_or_default()
    }
}
