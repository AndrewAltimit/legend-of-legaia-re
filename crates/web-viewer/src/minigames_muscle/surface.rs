//! The minigames page's dome 3D through the engine's own surface
//! ([`legaia_engine_core::muscle_dome_scene::MuscleDomeSurface`]) - the
//! battle seats, the play-out choreography and the battle camera script the
//! native window and the browser play page draw a leg with - so the three
//! hosts frame a dome fight from one kernel.

use super::*;
use legaia_engine_core::muscle_dome_scene::{BeatKind, SelectFraming};
use std::sync::Arc;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Pose this page's dome leg for one frame through the engine surface and
    /// return its generation, or `-1` with no leg or when the scene does not
    /// decode. A generation the page has not seen means the static buffers
    /// and the VRAM changed: re-read them before the positions.
    ///
    /// `select` names the selection screen the page has up, because the page
    /// drives its own command flow rather than the session's menu: `0` the
    /// round prompt / `Begin | Reselect` confirm (the far framing), `1` the
    /// ring / direction entry / review / Ra-Seru list (the fighter's
    /// close-up), `2` the `Auto | Command` prompt (turned toward the
    /// opponent). `camera_option` is the Battle Camera option word.
    pub fn muscle_surface_frame(&mut self, select: u32, camera_option: u8) -> i32 {
        let Some(c) = self.muscle.as_ref() else {
            self.muscle_surface.frame(|_| None, None, None, 0);
            return -1;
        };
        let framing = match select {
            1 => SelectFraming::Member,
            2 => SelectFraming::Target,
            _ => SelectFraming::Far,
        };
        let (prot, entries) = (&self.prot, &self.entries);
        let read = |i: usize| entry_bytes(prot, entries, i as u32).map(|b| Arc::new(b.to_vec()));
        let surface = &mut self.muscle_surface;
        surface.set_select_framing(Some(framing));
        surface.set_camera_option(camera_option);
        match surface.frame_seated(read, &c.session, c.monster_id, c.char_slot as u32) {
            Some(_) => surface.generation() as i32,
            None => -1,
        }
    }

    /// This frame's posed positions, `[x, y, z]` per vertex, raw Y-down
    /// world coordinates: the fighter, the monster, then the arena.
    pub fn muscle_surface_positions(&self) -> Vec<f32> {
        self.muscle_surface
            .scene()
            .map(|s| s.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]`.
    pub fn muscle_surface_uvs(&self) -> Vec<u8> {
        self.muscle_surface
            .scene()
            .map(|s| s.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn muscle_surface_cba_tsb(&self) -> Vec<u16> {
        self.muscle_surface
            .scene()
            .map(|s| s.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured]`.
    pub fn muscle_surface_flat_rgba(&self) -> Vec<u8> {
        self.muscle_surface
            .scene()
            .map(|s| s.flat_rgba.clone())
            .unwrap_or_default()
    }

    /// Triangle indices.
    pub fn muscle_surface_indices(&self) -> Vec<u32> {
        self.muscle_surface
            .scene()
            .map(|s| s.indices.clone())
            .unwrap_or_default()
    }

    /// The seated dome's VRAM; empty with no scene.
    pub fn muscle_surface_vram(&self) -> Vec<u8> {
        self.muscle_surface
            .vram()
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// The battle camera's view-projection for a raw (Y-down) world vertex,
    /// column-major (`DomeCamera::vp_raw`, the matrix both play hosts draw a
    /// leg with). Empty with no scene.
    pub fn muscle_surface_vp(&self, aspect: f32) -> Vec<f32> {
        self.muscle_surface
            .scene()
            .map(|s| s.camera.vp_raw(aspect).to_vec())
            .unwrap_or_default()
    }

    /// The play-out beat on screen: `{"kind","play","attacker","at","len"}`
    /// (`kind` = `approach` / `swing` / `done`, `at` the ticks into it), or
    /// `null` outside a play-out - the schedule
    /// (`muscle_dome_scene::turn_timeline`) the page times its hit numerals
    /// and HP bars against, so they land on the surface's swings.
    pub fn muscle_surface_beat_json(&self) -> String {
        let Some((beat, at)) = self.muscle_surface.playback_beat() else {
            return "null".to_string();
        };
        let kind = match beat.kind {
            BeatKind::Approach => "approach",
            BeatKind::Swing => "swing",
            BeatKind::Done => "done",
        };
        serde_json::json!({
            "kind": kind,
            "play": beat.play,
            "attacker": beat.attacker,
            "at": at,
            "len": beat.len,
        })
        .to_string()
    }
}
