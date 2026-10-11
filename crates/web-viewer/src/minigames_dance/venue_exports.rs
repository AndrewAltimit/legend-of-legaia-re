//! The dance hall's exports: the venue view-projection and the baked
//! environment mesh buffers.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The dance entry's own camera as a column-major view-projection over
    /// this page's baked frame (positions re-based on the human dancer's
    /// spawn, retail Y-down): the pose `FUN_801CEF54` stages - angles,
    /// `H`, the eye-space pair, focused on its beat-clock actor - through the
    /// shared kernel [`dance_venue::venue_camera`]. While a run is live the
    /// pose is the run's camera keyframe track instead
    /// (`DanceGame::camera_pose` through [`dance_venue::venue_camera_at`] -
    /// the track `dance_tick` advances). `aspect` is the canvas' width over
    /// height. Empty when the cast did not decode.
    pub fn dance_venue_vp(&self, aspect: f32) -> Vec<f32> {
        let tracked = self.dance.as_ref().and_then(|g| g.camera_pose()).map(|p| {
            dance_venue::venue_camera_at(&legaia_engine_minigames::dance::dance_scene_entry(), p)
        });
        match tracked {
            Some(camera) => self.dance_venue_vp_with(&camera, aspect),
            None => self
                .dance_bodies
                .as_ref()
                .map(|b| self.dance_venue_vp_with(&b.camera, aspect))
                .unwrap_or_default(),
        }
    }

    // ------------------------------------------------------ the dance hall
    //
    // Retail draws the dance minigame inside the host field scene: the hall -
    // the raised stage, the yellow/black checkered dance floor, the portrait
    // banners on the walls, the spotlight cones, the speaker and lamp
    // fixtures - is the `other7` scene's own environment pack instanced by
    // its `.MAP` placement / terrain layers, textured by the same scene VRAM
    // the dancer bodies already sample. These accessors hand the page that
    // map as ONE static baked mesh in the dancer frame (the human spawn at
    // the origin, retail Y-down coordinates), so the page appends it to the
    // combined vertex buffer as unposed geometry behind the cast.

    /// Baked hall vertex positions (`[x, y, z, ...]`, dancer frame). Empty
    /// when the scene's placement layers didn't resolve - the page then keeps
    /// the neutral ground and says so.
    pub fn dance_env_positions(&self) -> Vec<f32> {
        self.dance_bodies
            .as_ref()
            .filter(|b| !b.env.is_empty())
            .map(|b| b.env.positions.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` texel coords for the baked hall.
    pub fn dance_env_uvs(&self) -> Vec<i32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.uvs.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]` for the baked hall.
    pub fn dance_env_cba_tsb(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.cba_tsb.clone())
            .unwrap_or_default()
    }

    /// Triangle indices for the baked hall.
    pub fn dance_env_indices(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.indices.clone())
            .unwrap_or_default()
    }

    /// This frame's drawable subset of [`Self::dance_env_indices`] under the
    /// camera [`Self::dance_venue_vp`] frames with: the triangles the PSX GPU
    /// would not refuse for their screen span.
    pub fn dance_env_visible_indices(&self) -> Vec<u32> {
        let tracked = self.dance.as_ref().and_then(|g| g.camera_pose()).map(|p| {
            dance_venue::venue_camera_at(&legaia_engine_minigames::dance::dance_scene_entry(), p)
        });
        let camera = tracked.or_else(|| self.dance_bodies.as_ref().map(|b| b.camera));
        camera
            .map(|c| self.dance_env_visible_indices_with(&c))
            .unwrap_or_default()
    }

    pub fn dance_env_flat_rgba(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.env.flat.clone())
            .unwrap_or_default()
    }
}
