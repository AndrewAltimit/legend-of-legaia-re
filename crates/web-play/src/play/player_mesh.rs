//! The lead's field mesh: buffers, posed positions, transform, draw gate,
//! occlusion probe and draw tint.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    // -------------------------------------------------------------- player

    /// `true` when the lead's field mesh resolved out of the global TMD pool.
    pub fn player_has_mesh(&self) -> bool {
        self.player.is_some()
    }

    /// Player mesh geometry (object-local; pair with
    /// [`Self::player_mesh_positions`], which poses it).
    pub fn player_mesh_indices(&self) -> Vec<u32> {
        self.player
            .as_ref()
            .map(|p| p.base.indices.clone())
            .unwrap_or_default()
    }

    pub fn player_mesh_uvs(&self) -> Vec<u8> {
        self.player
            .as_ref()
            .map(|p| p.base.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn player_mesh_cba_tsb(&self) -> Vec<u16> {
        self.player
            .as_ref()
            .map(|p| p.base.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn player_mesh_flat_rgba(&self) -> Vec<u8> {
        self.player
            .as_ref()
            .map(|p| p.flat.clone())
            .unwrap_or_default()
    }

    /// The player's vertices **posed at the current frame**: the world's live
    /// `pose_frame` (idle clip standing, walk clip moving), composed per bone.
    /// Falls back to the object-local rest geometry when no clip is installed -
    /// which is what a lead outside the Vahn / Noa / Gala trio gets, since the
    /// locomotion bundle only banks those three.
    ///
    /// The vertices come back at the player's render scale
    /// (`World::player_render_scale`, `+0x72` - `0xC00` on a kingdom map),
    /// the native window's actor-matrix scale: the page translates them to
    /// the player's position, so scaling about the local origin is the same
    /// composition.
    pub fn player_mesh_positions(&mut self) -> Vec<f32> {
        let scale = self
            .scene_host
            .host()
            .map_or(1.0, |h| h.world.player_render_scale());
        let mut out = self.player_mesh_positions_unscaled();
        if scale != 1.0 {
            for v in &mut out {
                *v *= scale;
            }
        }
        out
    }

    fn player_mesh_positions_unscaled(&mut self) -> Vec<f32> {
        let pose: Option<Vec<([i16; 3], [i16; 3])>> = self.scene_host.host().and_then(|h| {
            let slot = h.world.player_actor_slot? as usize;
            let mut bones = h
                .world
                .actors
                .get(slot)?
                .pose_frame
                .as_ref()?
                .bone_outputs
                .clone();
            // The script's look rotation on one object (`4C 45`), the kernel
            // the native window folds into the same rig.
            if let Some(l) = h
                .world
                .actor_look(legaia_engine_field::actor_look::LookKey::Player)
            {
                legaia_engine_field::actor_look::apply_look(&mut bones, l);
            }
            Some(bones)
        });
        let Some(p) = self.player.as_mut() else {
            return Vec::new();
        };
        match pose {
            Some(bones) if !bones.is_empty() => {
                // Disjoint field borrows: the scratch buffer and the source
                // geometry are different fields of the rig.
                let PlayerRig {
                    base,
                    object_ids,
                    posed,
                    ..
                } = p;
                pose_into(posed, &base.positions, object_ids, &bones);
                posed.clone()
            }
            _ => p.base.positions.iter().flatten().copied().collect(),
        }
    }

    /// Whether the page draws the lead's field body this frame: `false`
    /// where the scene hides it outright (`World::actor_hidden_by_scene` -
    /// the opening prologue's vignette shots have no lead), the native
    /// window's actor-pass skip.
    pub fn play_player_drawn(&self) -> bool {
        self.scene_host.host().is_none_or(|h| {
            let slot = h.world.player_actor_slot.map_or(0, usize::from);
            !h.world.actor_hidden_by_scene(slot)
        })
    }

    /// `[world_x, world_y, world_z, facing_units]` for the player actor.
    /// `facing_units` is the engine heading (`render_26`, PSX 12-bit; `0` =
    /// travelling `+Z`); the world coords are the raw retail frame (`+Y` down).
    pub fn player_transform(&self) -> Vec<f32> {
        let Some(a) = self.scene_host.host().and_then(|h| {
            let slot = h.world.player_actor_slot? as usize;
            h.world.actors.get(slot)
        }) else {
            return vec![0.0; 4];
        };
        vec![
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
            a.move_state.render_26 as f32,
        ]
    }

    /// Camera-occlusion fade **visibility gate**: is the player completely
    /// hidden from `eye` by the static scene geometry? Ray-casts the
    /// 5-point body cross of `engine_core::field_occlusion` (the same
    /// kernel the native play-window runs) against the scene's occluder
    /// set. `eye_*` are RAW retail Y-down world coordinates - the page's
    /// draw frame is Y-up, so `play-app.js` negates its `_eye()` Y on the
    /// way in. `false` when no scene / player / occluders exist, so a
    /// partially visible character (or an unprovable one) never fades.
    pub fn field_player_occluded(&self, eye_x: f32, eye_y: f32, eye_z: f32) -> bool {
        let Some(f) = self.field.as_ref() else {
            return false;
        };
        let Some(h) = self.scene_host.host() else {
            return false;
        };
        // Body centre: the one kernel the native gate samples
        // (`field_occlusion::player_body_centre` - the floor tier under the
        // actor lifted half a character height). This page used to rebuild
        // it here with its own copy of the half-height constant.
        let Some(centre) = legaia_engine_screens::field_occlusion::player_body_centre(&h.world)
        else {
            return false;
        };
        f.occluders.fully_occluded([eye_x, eye_y, eye_z], centre)
    }

    /// The player's op-`4C 81` draw tint, `[r, g, b, ir0]`, or empty while
    /// the player draws untinted (`World::player_draw_tint`, the native
    /// window's player cue).
    pub fn play_player_tint(&self) -> Vec<f32> {
        self.scene_host
            .host()
            .and_then(|h| h.world.player_draw_tint())
            .map(|(colour, blend)| {
                let (far, ir0) = legaia_engine_field::object_tint::tint_cue(colour, blend);
                vec![far[0], far[1], far[2], ir0]
            })
            .unwrap_or_default()
    }
}
