//! The field map's meshes: scene VRAM, the per-slot env mesh (rest, posed,
//! lit, morphed) and the mesh buffers the page uploads.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    // ----------------------------------------------------------------- map

    /// Field VRAM (1 MB) - the image every mesh below samples. The engine's own
    /// scene VRAM, not a viewer-side rebuild.
    ///
    /// While a field-to-battle transition holds a landed capture, this is the
    /// emitter's **cloned** page instead - the scene VRAM with the captured
    /// field frame (and, for the curtain, its per-frame intermediate) blitted
    /// into the texture-page rects the style bodies sample. The pristine page
    /// comes back the moment the transition drops, via the same dirty flag.
    pub fn field_vram_bytes(&self) -> Vec<u8> {
        if let Some(page) = self.battle_intro.as_ref().and_then(|i| i.captured_vram()) {
            return page.as_bytes().to_vec();
        }
        self.res()
            .map(|r| r.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// True when a field VRAM effect (water CLUT-walk shimmer, jou's ambient
    /// palette cyclers / lightning, scripted CLUT fx) changed texels since
    /// the last call - the page then re-uploads [`Self::field_vram_bytes`].
    /// Reading clears the flag.
    pub fn field_vram_take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.field_vram_dirty)
    }

    /// `{"pack_count", "placements", "terrain", "ground_quads"}` for the status
    /// line; `null` before a scene is entered.
    pub fn field_status_json(&self) -> String {
        match self.field.as_ref() {
            Some(f) => format!(
                r#"{{"pack_count":{},"placements":{},"terrain":{},"ground_quads":{}}}"#,
                f.env_tmds.len(),
                f.placements.len(),
                f.terrain.len(),
                f.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0),
            ),
            None => "null".to_string(),
        }
    }

    /// Select + build environment-pack slot `slot`; subsequent `field_mesh_*`
    /// reads return that mesh.
    pub fn field_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        self.field_mesh_posed(slot, 0)
    }

    /// Select + build environment-pack slot `slot` **posed at frame 0** of
    /// scene ANM record `anim_id - 1` - the rest state of a placed prop whose
    /// object bind names a clip (cupboard doors closed on the cabinet's front
    /// face, the windmill's sails on their hub). Falls back to the raw
    /// object-local mesh when the pose can't resolve (no scene bundle, or the
    /// clip's bone count doesn't match the mesh's object count - retail's
    /// count-equality contract, `FUN_8001B964`), exactly as the native
    /// play-window falls back to its unposed instance. `anim_id == 0` is the
    /// plain unposed build ([`Self::field_mesh`]).
    pub fn field_mesh_posed(&mut self, slot: u32, anim_id: u32) -> Result<u32, JsValue> {
        let s = slot as usize;
        let anim = anim_id.min(u8::MAX as u32) as u8;
        let res_idx = {
            let f = self
                .field
                .as_ref()
                .ok_or_else(|| JsValue::from_str("field_mesh: no scene"))?;
            if f.cur.as_ref().map(|(key, _, _)| *key) == Some((s, anim)) {
                return Ok(slot);
            }
            *f.env_tmds
                .get(s)
                .ok_or_else(|| JsValue::from_str(&format!("field_mesh: slot {s} out of range")))?
        };
        let offsets: Option<Vec<([i16; 3], [i16; 3])>> = if anim == 0 {
            None
        } else {
            self.frame0_bone_offsets(anim, res_idx)
        };
        let built = {
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("field_mesh: no resources"))?;
            let rtmd = res
                .tmds
                .get(res_idx)
                .ok_or_else(|| JsValue::from_str("field_mesh: tmd missing"))?;
            let (mut mesh, flat) = match &offsets {
                Some(o) => {
                    legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_posed(rtmd, o)
                }
                None => legaia_engine_scene::scene_assembly::build_hybrid_env_mesh(rtmd, &res.vram),
            };
            // Enhanced lighting's emissive tags (TSB bit 13) - the native
            // window's env-mesh rule; the shaders read them only while the
            // enhancement is on.
            legaia_render_kernels::scene_lighting::tag_emissive_hybrid(
                &rtmd.raw, &mut mesh, &flat, &res.vram,
            );
            (mesh, flat)
        };
        if let Some(f) = self.field.as_mut() {
            f.cur = Some(((s, anim), built.0, built.1));
        }
        Ok(slot)
    }

    /// Select + build environment-pack slot `slot` (unposed) with its
    /// **light-source rows** shaded for a draw at the record angles
    /// `(rot_x, rot_y, rot_z)` under the world's live field light - the
    /// `legaia_engine_field::field_lit_mesh` kernel the native window shades
    /// its env draws with. Returns whether the mesh has lit rows at all; for a
    /// mesh without any, the build is the plain [`Self::field_mesh`] one and
    /// the page keeps sharing that upload across draws.
    pub fn field_mesh_lit(
        &mut self,
        slot: u32,
        rot_x: u32,
        rot_y: u32,
        rot_z: u32,
    ) -> Result<bool, JsValue> {
        self.field_mesh_posed_lit(slot, 0, rot_x, rot_y, rot_z)
    }

    /// [`Self::field_mesh_lit`] for a placed prop posed at frame 0 of scene
    /// ANM record `anim_id - 1` ([`Self::field_mesh_posed`]): the lit rows'
    /// normals turn with each bone before the draw's rotation folds in.
    /// `anim_id == 0` is the unposed build.
    pub fn field_mesh_posed_lit(
        &mut self,
        slot: u32,
        anim_id: u32,
        rot_x: u32,
        rot_y: u32,
        rot_z: u32,
    ) -> Result<bool, JsValue> {
        let s = slot as usize;
        let anim = anim_id.min(u8::MAX as u32) as u8;
        let light = self
            .scene_host
            .host()
            .map(|h| h.world.presentation.field_light)
            .ok_or_else(|| JsValue::from_str("field_mesh_lit: no scene"))?;
        let res_idx = *self
            .field
            .as_ref()
            .ok_or_else(|| JsValue::from_str("field_mesh_lit: no scene"))?
            .env_tmds
            .get(s)
            .ok_or_else(|| JsValue::from_str(&format!("field_mesh_lit: slot {s} out of range")))?;
        let offsets: Option<Vec<([i16; 3], [i16; 3])>> = if anim == 0 {
            None
        } else {
            self.frame0_bone_offsets(anim, res_idx)
        };
        let (mesh, flat, lit) = {
            let res = self
                .res()
                .ok_or_else(|| JsValue::from_str("field_mesh_lit: no resources"))?;
            let rtmd = res
                .tmds
                .get(res_idx)
                .ok_or_else(|| JsValue::from_str("field_mesh_lit: tmd missing"))?;
            let (mut mesh, mut flat, lit) = match &offsets {
                Some(o) => {
                    legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_posed_lit(rtmd, o)
                }
                None => {
                    legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_lit(rtmd, &res.vram)
                }
            };
            if legaia_engine_field::field_lit_mesh::has_lit_rows(&lit) {
                let rot = legaia_engine_field::field_lit_mesh::draw_rotation(
                    rot_x as u16,
                    rot_y as u16,
                    rot_z as u16,
                );
                legaia_engine_field::field_lit_mesh::shade_lit_rows_rgba(
                    &mut mesh.colors,
                    &mut flat,
                    &lit,
                    &light,
                    &rot,
                );
            }
            legaia_render_kernels::scene_lighting::tag_emissive_hybrid(
                &rtmd.raw, &mut mesh, &flat, &res.vram,
            );
            (mesh, flat, lit)
        };
        let has_lit = legaia_engine_field::field_lit_mesh::has_lit_rows(&lit);
        if let Some(f) = self.field.as_mut() {
            // Not the plain build's cache key: a later `field_mesh(slot)`
            // must rebuild the unshaded stream.
            f.cur = Some(((usize::MAX, 0), mesh, flat));
        }
        Ok(has_lit)
    }

    /// Whether env-pack slot `slot` carries light-source rows - the page's
    /// "needs a per-rotation shaded copy" test.
    pub fn field_mesh_has_lit_rows(&self, slot: u32) -> bool {
        let Some(res_idx) = self
            .field
            .as_ref()
            .and_then(|f| f.env_tmds.get(slot as usize).copied())
        else {
            return false;
        };
        let Some(res) = self.res() else {
            return false;
        };
        res.tmds.get(res_idx).is_some_and(|rtmd| {
            let (_, lit) = rtmd.build_filtered_vram_mesh_lit_vertices(&res.vram);
            legaia_engine_field::field_lit_mesh::has_lit_rows(&lit)
        })
    }

    /// The live field light as one comparable key (angles + back colour) -
    /// the page re-shades its lit env copies when it changes (op `4C 8A`).
    pub fn field_light_key(&self) -> String {
        self.scene_host
            .host()
            .map(|h| {
                let l = h.world.presentation.field_light;
                format!(
                    "{},{},{},{},{},{}",
                    l.angles[0], l.angles[1], l.angles[2], l.back[0], l.back[1], l.back[2]
                )
            })
            .unwrap_or_default()
    }

    pub fn field_mesh_positions(&self) -> Vec<f32> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.positions.iter().flatten().copied().collect()
    }

    /// Drain the env-pack slots whose VDF morph deltas changed since the
    /// last call (the engine world's ambient morph parts + the scene-entry
    /// pulse, ticked by the sim's ambient drain). The page re-uploads each
    /// slot's positions from [`Self::field_morph_positions`] - the play
    /// page's side of the `FUN_8001C604` render substitution.
    pub fn field_morph_slots(&mut self) -> Vec<u32> {
        let Some(host) = self.scene_host.host_mut() else {
            return Vec::new();
        };
        let mut slots: Vec<u32> = host
            .world
            .take_morph_dirty_slots()
            .into_iter()
            .map(|(s, _)| s as u32)
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }

    /// Morphed vertex positions for env-pack slot `slot`: the plain
    /// (`anim 0`) hybrid mesh build with the live VDF deltas staged onto
    /// the TMD group vertices. Stream order matches the uploaded rest-pose
    /// mesh (the prim walk is position-independent); empty when no morph
    /// targets the slot.
    pub fn field_morph_positions(&mut self, slot: u32) -> Vec<f32> {
        let s = slot as usize;
        let Some(res_idx) = self.field.as_ref().and_then(|f| f.env_tmds.get(s).copied()) else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let Some(res) = host.resources.as_ref() else {
            return Vec::new();
        };
        let Some(rtmd) = res.tmds.get(res_idx) else {
            return Vec::new();
        };
        let Some(m) = host.world.morphed_env_tmd(s, rtmd) else {
            return Vec::new();
        };
        let (mesh, _) = legaia_engine_scene::scene_assembly::build_hybrid_env_mesh(&m, &res.vram);
        mesh.positions.iter().flatten().copied().collect()
    }

    pub fn field_mesh_uvs(&self) -> Vec<u8> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.uvs.iter().flatten().copied().collect()
    }

    pub fn field_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some((_, m, _)) = self.field_cur() else {
            return Vec::new();
        };
        m.cba_tsb.iter().flatten().copied().collect()
    }

    pub fn field_mesh_indices(&self) -> Vec<u32> {
        self.field_cur()
            .map(|(_, m, _)| m.indices.clone())
            .unwrap_or_default()
    }

    pub fn field_mesh_flat_rgba(&self) -> Vec<u8> {
        self.field_cur()
            .map(|(_, _, f)| f.clone())
            .unwrap_or_default()
    }

    /// Positions of environment-pack slot `slot` **posed at pose key
    /// `frame`** ([`Self::field_placement_frames`]' value) of scene ANM record
    /// `anim_id - 1` - the per-frame re-pose the draw walker (`FUN_8001B964`)
    /// does off a placed prop's live cursor.
    /// Same vertex order as [`Self::field_mesh_posed`]'s frame-0 build (the two
    /// differ only in the per-object transform), so the page can upload the
    /// mesh once and rewrite just its positions each frame. Empty when the pose
    /// can't resolve (no bundle / bone-count mismatch) - the caller then leaves
    /// the prop at its rest pose.
    pub fn field_mesh_posed_frame_positions(
        &self,
        slot: u32,
        anim_id: u32,
        frame: u32,
    ) -> Vec<f32> {
        let s = slot as usize;
        let anim = anim_id.min(u8::MAX as u32) as u8;
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let Some(&res_idx) = f.env_tmds.get(s) else {
            return Vec::new();
        };
        let key = PropPoseKey::from_i32(frame as i32);
        let Some(offsets) = self.frame_bone_offsets(anim, res_idx, key) else {
            return Vec::new();
        };
        let Some(res) = self.res() else {
            return Vec::new();
        };
        let Some(rtmd) = res.tmds.get(res_idx) else {
            return Vec::new();
        };
        let (mesh, _flat) =
            legaia_engine_scene::scene_assembly::build_hybrid_env_mesh_posed(rtmd, &offsets);
        mesh.positions.iter().flatten().copied().collect()
    }
}
