//! The NPC layer's exports over `crate::field_actors::FieldActors`, plus the
//! effect-clip read-back.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    // ---------------------------------------------------------------- NPCs
    //
    // Every answer below is `crate::field_actors::FieldActors`'s, the one
    // actor layer the map viewer runs too.

    fn actor_banks(&self) -> crate::field_actors::ActorBanks<'_> {
        crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        }
    }

    /// The scene's NPC / actor catalog. Shape:
    /// `{"anm_prot": 4, "npcs": [{"i", "slot", "model", "anim", "nobj",
    /// "kind", "target_map", "dialog", "conditional", "special", "x", "z"},
    /// ...]}`. `null` before a scene is entered.
    pub fn play_npc_catalog_json(&self) -> String {
        self.actors.catalog_json()
    }

    /// The object count catalog entry `i`'s mesh is cut to right now (the
    /// live clip's bone count), or `-1` for an uncut mesh. The page re-builds
    /// an NPC's mesh when this moves.
    pub fn play_npc_mesh_cut(&self, i: u32) -> i32 {
        let Some(h) = self.scene_host.host() else {
            return -1;
        };
        self.actors.mesh_cut(h, self.actor_banks(), i)
    }

    /// Per catalog entry, the generation of its op-`0x4B` VDF morph (`-1` =
    /// never armed); the page re-reads [`Self::play_npc_morph_base`] when it
    /// moves.
    pub fn play_npc_morph_states(&self) -> Vec<i32> {
        self.actors.morph_states()
    }

    /// Catalog entry `i`'s object-local base positions with its live morph
    /// staged (`World::npc_morphed_tmd`), in [`Self::play_npc_mesh_positions`]'
    /// vertex order.
    pub fn play_npc_morph_base(&self, i: u32) -> Vec<f32> {
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        self.actors.morph_base(h, self.actor_banks(), i)
    }

    /// Build catalog entry `i`'s mesh (hybrid: textured + vertex-colour prims,
    /// with per-vertex bone ids). Returns `i`. A special (`model >= 0xF0`)
    /// resolves out of the world's global TMD pool, and a clip-bound mesh is
    /// cut to the clip's bone count, as the native window's field-NPC bind.
    pub fn play_npc_mesh(&mut self, i: u32) -> Result<u32, JsValue> {
        let Some(h) = self.scene_host.host() else {
            return Err(JsValue::from_str("play_npc_mesh: no scene"));
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors
            .build_mesh(h, banks, i)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(i)
    }

    /// Per catalogued NPC, `1` while its op-`43 07` deformation breaks it up
    /// (`FieldActors::shatter_states`); empty while none does.
    pub fn play_npc_shatter_states(&self) -> Vec<u8> {
        self.scene_host
            .host()
            .map(|h| self.actors.shatter_states(h))
            .unwrap_or_default()
    }

    /// Catalog entry `i`'s shattered object-local corners, one per index of
    /// its mesh (`FieldActors::shatter`); empty while it is whole. Selects
    /// the entry's mesh as [`Self::play_npc_mesh`] does, so the `mesh_*`
    /// readers then describe it.
    pub fn play_npc_shatter(&mut self, i: u32) -> Vec<f32> {
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        let banks = crate::field_actors::ActorBanks {
            scene_anm: self.scene_anm.as_ref(),
            locomotion_anm: self.locomotion_anm.as_ref(),
        };
        self.actors.shatter(h, banks, i)
    }

    /// The live model id the scripted-motion VM's op `0x0E` re-bound catalog
    /// entry `i`'s actor to, or `-1` while it still draws its spawn mesh. The
    /// page re-uploads the mesh when the answer moves.
    pub fn play_npc_live_model(&self, i: u32) -> i32 {
        self.scene_host
            .host()
            .map_or(-1, |h| self.actors.live_model(h, i))
    }

    pub fn play_npc_mesh_positions(&self) -> Vec<f32> {
        self.actors.mesh_positions()
    }

    pub fn play_npc_mesh_uvs(&self) -> Vec<u8> {
        self.actors.mesh_uvs()
    }

    pub fn play_npc_mesh_cba_tsb(&self) -> Vec<u16> {
        self.actors.mesh_cba_tsb()
    }

    pub fn play_npc_mesh_indices(&self) -> Vec<u32> {
        self.actors.mesh_indices()
    }

    /// Per-vertex TMD object index for the built NPC mesh - the bone each
    /// vertex hangs from.
    pub fn play_npc_mesh_object_ids(&self) -> Vec<u32> {
        self.actors.mesh_object_ids()
    }

    pub fn play_npc_mesh_flat_rgba(&self) -> Vec<u8> {
        self.actors.mesh_flat_rgba()
    }

    /// Catalog entry `i`'s spawn clip, 6 `i32` per bone per frame
    /// (`[tx, ty, tz, rx, ry, rz]`, absolute); empty with no clip.
    pub fn play_npc_pose_frames(&self, i: u32) -> Vec<i32> {
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        self.actors.pose_frames(h, self.actor_banks(), i)
    }

    /// `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
    /// it has none.
    pub fn play_npc_pose_dims(&self, i: u32) -> Vec<u32> {
        let Some(h) = self.scene_host.host() else {
            return vec![0, 0];
        };
        self.actors.pose_dims(h, self.actor_banks(), i)
    }

    /// The off-map hide-box coordinate (`FIELD_OFFMAP_HIDE_XZ`): the page
    /// skips drawing any NPC whose live position is this tile on both axes,
    /// exactly as the native play-window's draw pass does.
    pub fn field_offmap_hide_xz(&self) -> i32 {
        legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ as i32
    }

    /// Live clip-playback state of every catalogued NPC, `[pose, generation,
    /// look_lo, look_hi, ...]` (`FieldActors::clip_states`; `[-1, -1, 0, 0]`
    /// with no live clip player).
    pub fn play_npc_clip_states(&self) -> Vec<i32> {
        self.actors.clip_states(self.scene_host.host())
    }

    /// Current pose of catalog entry `i`'s live clip, 6 `i32` per bone, read
    /// without advancing the playhead (it moves only in
    /// [`LegaiaRuntime::tick_frame`]).
    pub fn play_npc_live_bones(&self, i: u32) -> Vec<i32> {
        self.actors.live_bones(i, self.scene_host.host())
    }

    /// Live world state of every catalogued NPC, `[x, y, z, facing_units,
    /// ...]` in catalog order (world positions, the render-scale-zero hide,
    /// the floor / scripted-arc height).
    pub fn play_npc_transforms(&self) -> Vec<f32> {
        self.scene_host
            .host()
            .map(|h| self.actors.transforms(h))
            .unwrap_or_default()
    }

    /// Per catalogued NPC: `1` = retail's actor cull (`FUN_801D79E8`) hides
    /// it under this frame's visible-tile crop, so the page skips its draw
    /// (`FieldActors::culled`). Empty while no crop applies.
    pub fn play_npc_culled(&self, debug_camera: bool) -> Vec<u8> {
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        let cells = self.field_view_cells_now(debug_camera);
        self.actors.culled(h, cells.as_ref())
    }

    /// Every catalogued NPC's draw pose (`FieldActors::draw_poses`, the
    /// engine's `World::field_npc_draw_pose` - the native window's NPC
    /// transform), 8 floats per entry.
    pub fn play_npc_draw_poses(&self) -> Vec<f32> {
        self.scene_host
            .host()
            .map(|h| self.actors.draw_poses(h))
            .unwrap_or_default()
    }

    /// Live `(pitch, roll)` of every catalogued NPC, `[pitch, roll, ...]`
    /// (`World::field_npc_tilt`; `(0, 0)` for an untilted actor).
    pub fn play_npc_tilts(&self) -> Vec<f32> {
        self.scene_host
            .host()
            .map(|h| self.actors.tilts(h))
            .unwrap_or_default()
    }

    /// Per catalogued NPC, the op-`4C 81` **draw tint** as a constant
    /// per-draw cue, `[r, g, b, ir0, ...]` (far colour in display `0..1`,
    /// `ir0` in `1.0 = 0x1000` units; `ir0 == 0` = untinted). **Empty** while
    /// no NPC is tinted. The same `World::field_npc_draw_tint` the native
    /// play-window stages on its NPC draws.
    pub fn play_npc_tints(&self) -> Vec<f32> {
        self.scene_host
            .host()
            .map(|h| self.actors.tints(h))
            .unwrap_or_default()
    }

    /// Whether any field context has `+0x42` raised this frame (field-VM
    /// `4C C2`) - the page asks [`Self::play_effect_clip`] per draw only
    /// while this is `true`.
    pub fn play_effect_clip_live(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| !h.world.object_effect_clips().is_empty())
    }

    /// One draw's **object-effect clip** in mesh space,
    /// `[m0, m1, m2, lo, hi, 1]` (keep `lo <= m . p <= hi`), or empty while
    /// the draw's actor has no raised `+0x42`. `kind` `0` = a placed
    /// object, `key` its placement draw index (the [`Self::field_placement_tints`]
    /// index); `kind` `1` = a catalogued NPC, `key` its placement slot;
    /// `kind` `2` = the player (`key` ignored).
    /// `model` is the draw's page model matrix (16 floats, column-major), in
    /// the page's Y-flipped frame: its row 1 is negated back to retail's
    /// before the shared kernel `World::object_effect_mesh_clip` - the one
    /// the native window asks per placed / NPC draw.
    pub fn play_effect_clip(&self, kind: u8, key: u32, model: Vec<f32>) -> Vec<f32> {
        use legaia_engine_field::object_tint::ActorTintKey;
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        if model.len() < 16 {
            return Vec::new();
        }
        let tint_key = match kind {
            0 => {
                let Some(r) = self
                    .field
                    .as_ref()
                    .and_then(|f| f.placement_records.get(key as usize).copied().flatten())
                else {
                    return Vec::new();
                };
                ActorTintKey::Object(r as u16)
            }
            1 => ActorTintKey::Npc(key as usize),
            2 => ActorTintKey::Player,
            _ => return Vec::new(),
        };
        // Column-major `model[c * 4 + r]`; retail = diag(1, -1, 1) * page.
        let row = |r: usize, sign: f32| {
            [
                sign * model[r],
                sign * model[4 + r],
                sign * model[8 + r],
                sign * model[12 + r],
            ]
        };
        let rows = [row(0, 1.0), row(1, -1.0), row(2, 1.0)];
        h.world
            .object_effect_mesh_clip(tint_key, rows)
            .map(|c| c.shader_floats().to_vec())
            .unwrap_or_default()
    }
}
