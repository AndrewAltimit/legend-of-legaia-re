//! The battle stage's exports: the clear colour, the battle gate and VRAM,
//! the backdrop shell (static and spun), the ground grid and their depth
//! cues.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// What this frame's 3D pass should clear to: four linear RGBA floats
    /// from the shared
    /// [`legaia_engine_ui::battle_stage_clear`] selector, the one the native
    /// window renders with.
    ///
    /// The stage dome is a front half, so the open band above it is read as
    /// sky and the clear colour *is* that sky. This page's WebGL path cleared
    /// every mode to one hard-coded near-black, which put a black ceiling
    /// over every browser battle while the native window showed sky.
    pub fn play_scene_clear_color(&self) -> Vec<f32> {
        let stage_battle = self.play_battle_active()
            && self
                .battle_render
                .as_ref()
                .is_some_and(|b| b.stage_present());
        // The page draws no boot UI through the engine; the menu-overlay
        // screen term is the shared kernel's (`MenuRuntime::covers_field`).
        match self.scene_host.host() {
            Some(h) => legaia_engine_screens::field_frame::frame_clear_color(
                &h.world,
                false,
                &self.menu,
                stage_battle,
            )
            .to_vec(),
            None => legaia_engine_ui::battle_stage_clear::scene_clear(false, stage_battle, [0; 3])
                .to_vec(),
        }
    }

    /// `true` while a battle 3D render is built and the world is in
    /// [`SceneMode::Battle`] - the page's per-frame branch gate.
    pub fn play_battle_active(&self) -> bool {
        self.battle_render.is_some()
            && self
                .scene_host
                .host()
                .is_some_and(|h| h.world.mode == SceneMode::Battle)
    }

    /// Bumps once per battle entry; the page re-uploads the battle scene
    /// when it changes.
    pub fn play_battle_generation(&self) -> u32 {
        self.battle_render
            .as_ref()
            .map(|b| b.generation)
            .unwrap_or(0)
    }

    /// The retail 4x battle world scale the page composes onto actor draws.
    pub fn play_battle_world_scale(&self) -> f32 {
        BATTLE_WORLD_SCALE
    }

    /// The battle VRAM (1 MB): stage + flame atlas + monster/party bands.
    pub fn play_battle_vram_bytes(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .map(|b| b.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    fn battle_spun(&self) -> Option<&BattleMesh> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop_spun.as_ref())
    }

    pub fn play_battle_backdrop_positions(&self) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.stage_positions())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_uvs(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_cba_tsb(&self) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_indices(&self) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_flat_rgba(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.backdrop.as_ref())
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    /// The backdrop's drawn slot 1, one copy, in the stage's scaled space -
    /// empty when the draw list has no slot 1. The page draws it once per
    /// model of [`Self::play_battle_backdrop_spun_models`].
    pub fn play_battle_backdrop_spun_positions(&self) -> Vec<f32> {
        self.battle_spun()
            .map(|m| m.stage_positions())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_spun_uvs(&self) -> Vec<u8> {
        self.battle_spun().map(|m| m.uvs()).unwrap_or_default()
    }

    pub fn play_battle_backdrop_spun_cba_tsb(&self) -> Vec<u16> {
        self.battle_spun().map(|m| m.cba_tsb()).unwrap_or_default()
    }

    pub fn play_battle_backdrop_spun_indices(&self) -> Vec<u32> {
        self.battle_spun()
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_battle_backdrop_spun_flat_rgba(&self) -> Vec<u8> {
        self.battle_spun()
            .map(|m| m.flat.clone())
            .unwrap_or_default()
    }

    /// This frame's two model matrices for the spun backdrop slot, copy A
    /// then copy B, 16 column-major floats each: the slot's Y angle
    /// (`World::battle_backdrop_slot_1_yaw`, wound by `FUN_80046A20`) under
    /// each copy's transform
    /// (`legaia_engine_scene_host::scene_host::backdrop_slot_1_basis`, the per-object
    /// turn of `FUN_8001ADA4`'s backdrop arm) - the native window's two
    /// draws, with the page's stage Y flip composed on. Empty outside a fight
    /// with a spun slot.
    pub fn play_battle_backdrop_spun_models(&self) -> Vec<f32> {
        let (Some(host), Some(br)) = (self.scene_host.host(), self.battle_render.as_ref()) else {
            return Vec::new();
        };
        let (Some(_), Some(sh)) = (br.backdrop_spun.as_ref(), br.shell.as_ref()) else {
            return Vec::new();
        };
        let yaw = host.world.battle_backdrop_slot_1_yaw();
        let mut out = Vec::with_capacity(32);
        for copy_b in [false, true] {
            let r =
                legaia_engine_scene_host::scene_host::backdrop_slot_1_basis(sh.second, yaw, copy_b);
            // The page's stage model is the Y flip (`placementModelScaledY`
            // at unit scale - the shell's own placement), so each model is
            // `flip * basis`: row 1 negated.
            out.extend_from_slice(&[
                r[0][0], -r[1][0], r[2][0], 0.0, r[0][1], -r[1][1], r[2][1], 0.0, r[0][2],
                -r[1][2], r[2][2], 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
        }
        out
    }

    /// The NCLIP word of each [`Self::play_battle_backdrop_spun_models`]
    /// draw, in the same order: copy A culls its back faces, a mirrored
    /// copy B draws both sides
    /// (`legaia_engine_scene_host::scene_host::backdrop_copy_nclip`, the
    /// native window's per-draw mark). Empty when the models are.
    pub fn play_battle_backdrop_spun_nclip(&self) -> Vec<u32> {
        let Some(br) = self.battle_render.as_ref() else {
            return Vec::new();
        };
        let (Some(_), Some(sh)) = (br.backdrop_spun.as_ref(), br.shell.as_ref()) else {
            return Vec::new();
        };
        [false, true]
            .map(|copy_b| {
                legaia_engine_scene_host::scene_host::backdrop_copy_nclip(sh.second, copy_b)
            })
            .to_vec()
    }

    pub fn play_battle_ground_positions(&self) -> Vec<f32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.stage_positions())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_uvs(&self) -> Vec<u8> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.uvs())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_cba_tsb(&self) -> Vec<u16> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_battle_ground_indices(&self) -> Vec<u32> {
        self.battle_render
            .as_ref()
            .and_then(|b| b.ground.as_ref())
            .map(|m| m.mesh.indices.clone())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, 255]` packet colours of the ground grid - the
    /// modulation half of retail's `texel * colour / 128`. The colour is the
    /// **live** battle ambient `0x8007B7B0` (`World::battle_ambient_base`
    /// `+ 0x404040`), which a summon close-up ramps down; the page re-sends
    /// this whenever [`Self::play_battle_ground_ambient_key`] moves, the way
    /// the native window re-uploads its grid.
    pub fn play_battle_ground_flat_rgba(&self) -> Vec<u8> {
        let Some(ground) = self.battle_render.as_ref().and_then(|b| b.ground.as_ref()) else {
            return Vec::new();
        };
        let near = legaia_engine_battle_vm::battle_ground_grid::battle_ambient_colour(
            self.battle_ambient_base(),
        );
        if near == legaia_engine_battle_vm::battle_ground_grid::GRID_RGBC_SETTLED {
            return ground.flat.clone();
        }
        crate::packet_color::textured(
            &legaia_scene_formats::battle_backdrop::build_ground_grid_rgbc(near),
        )
    }

    /// The battle ambient base (`World::battle_ambient_base`, 8 bits a
    /// channel) packed `0x00BBGGRR` - a change key for the ground grid's
    /// colour: when it moves, the page re-reads
    /// [`Self::play_battle_ground_flat_rgba`] and
    /// [`Self::play_battle_ground_cue_json`].
    pub fn play_battle_ground_ambient_key(&self) -> u32 {
        let [r, g, b] = self.battle_ambient_base();
        u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
    }

    /// The live battle ambient base, or the settled neutral with no scene
    /// host.
    fn battle_ambient_base(&self) -> [u8; 3] {
        self.scene_host
            .host()
            .map(|h| h.world.battle_ambient_base())
            .unwrap_or(legaia_engine_battle_vm::battle_ground_grid::GRID_FAR_BASE_NEUTRAL)
    }

    /// The backdrop's per-draw depth cue this frame
    /// (`World::battle_backdrop_cue`, `FUN_80050120`'s `+0x78` ramp on the
    /// backdrop pair): `null` for an uncued draw, `{"hidden":true}` when the
    /// ramp has taken the pair off the draw (full weight - PROT 0903's fire
    /// tunnel), otherwise a flat pull toward black
    /// `{"far":[0,0,0],"near_z":-1,"far_z":0,"max_ir0":W}` - the same cue
    /// the native window attaches to its stage draw.
    pub fn play_battle_backdrop_cue_json(&self) -> String {
        let Some(host) = self.scene_host.host() else {
            return "null".to_string();
        };
        match host.world.battle_backdrop_cue() {
            None => r#"{"hidden":true}"#.to_string(),
            Some(w) if w > 0.0 => {
                format!(r#"{{"far":[0,0,0],"near_z":-1.0,"far_z":0.0,"max_ir0":{w}}}"#)
            }
            Some(_) => "null".to_string(),
        }
    }

    /// Whether the ground grid is on the screen this frame
    /// (`World::battle_ground_grid_drawn`: retail skips the grid's draw
    /// while a cast holds `ctx[+0x278]` bit 0 with the ambient on its
    /// floor). `true` outside a running host.
    pub fn play_battle_ground_drawn(&self) -> bool {
        self.scene_host
            .host()
            .is_none_or(|h| h.world.battle_ground_grid_drawn())
    }

    /// Ground-grid depth-cue parameters:
    /// `{"far":[r,g,b],"near_z":0,"far_z":Z,"max_ir0":M}` (display 0..1
    /// colour), or `null` when no grid is up. The page attaches this to the
    /// grid placement as a **per-draw** cue, so the browser grid fogs into
    /// the stage's far colour exactly as the native `DrawCue` seam does.
    ///
    /// `far_z` is a VIEW-depth window in GTE units, and is **not** scaled:
    /// the grid's vertices ride [`BATTLE_WORLD_SCALE`] as a model transform,
    /// but the camera's translation trio is already in view units, so the
    /// fragment's view depth is retail's `SZ` as-is (see
    /// `docs/subsystems/battle.md`, the grid's depth cue).
    pub fn play_battle_ground_cue_json(&self) -> String {
        let Some(br) = self.battle_render.as_ref().filter(|b| b.grid_far.is_some()) else {
            return "null".to_string();
        };
        use legaia_engine_battle_vm::battle_ground_grid as grid;
        // The far colour `0x8007BB48` follows the same live base as the
        // near colour (both ramp through a summon close-up).
        let far = grid::grid_far_colour(self.battle_ambient_base(), br.outdoor)
            .map(|c| f32::from(c) / 255.0);
        format!(
            r#"{{"far":[{},{},{}],"near_z":0.0,"far_z":{},"max_ir0":{}}}"#,
            far[0],
            far[1],
            far[2],
            grid::grid_cue_far_z(),
            grid::grid_cue_max_ir0()
        )
    }
}
