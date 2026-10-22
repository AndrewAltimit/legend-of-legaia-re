//! Per-placement streams parallel to the placement list: slots, transforms,
//! anim ids and pose frames, moves and turns, parked / tint / model / live
//! masks, and the actor cull.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Per-placement env-pack slot (parallel to
    /// [`Self::field_placement_positions`] / [`Self::field_placement_rot_y`]).
    pub fn field_placement_slots(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    pub fn field_placement_positions(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| env_positions(&f.placements, &f.coplanar_offsets))
            .unwrap_or_default()
    }

    pub fn field_placement_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// Per-placement uniform render scale (parallel to
    /// [`Self::field_placement_slots`]): the bind record's `actor[+0x72]` as a
    /// factor, applied after the rotation (`T * R * S`). `1.0` for nearly
    /// every placement; town01's horizon backdrop draws at `0.25`.
    /// Index of the first overworld **decoration** draw in the placement
    /// list (the landmarks come first): the page stages retail's per-object
    /// decoration depth cue on the draws from here on. Equal to the list's
    /// length off the overworld.
    pub fn field_decoration_start(&self) -> u32 {
        self.field
            .as_ref()
            .map(|f| f.decoration_start as u32)
            .unwrap_or(0)
    }

    pub fn field_placement_scales(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| f.placement_scales.clone())
            .unwrap_or_default()
    }

    /// Per-placement authored pitch (object record `+0x08`), parallel to
    /// [`Self::field_placement_rot_y`]. Composed with yaw and roll in
    /// retail's `Rx * Ry * Rz` order (`FUN_80026988`).
    pub fn field_placement_rot_x(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_x).collect())
            .unwrap_or_default()
    }

    /// Per-placement authored roll (object record `+0x0C`). See
    /// [`Self::field_placement_rot_x`].
    pub fn field_placement_rot_z(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.rot_z).collect())
            .unwrap_or_default()
    }

    /// Per-placement object-bind animation id (parallel to
    /// [`Self::field_placement_slots`]). `0` = unposed; nonzero = draw the
    /// slot's mesh through [`Self::field_mesh_posed`] with this id, or the
    /// prop's multi-object parts heap on the origin.
    pub fn field_placement_anim_ids(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.placements.iter().map(|d| d.anim_id as u32).collect())
            .unwrap_or_default()
    }

    /// Per-placement **live model** (parallel to
    /// [`Self::field_placement_slots`]), 16 column-major floats per placement
    /// in the page's Y-up frame: where a placed object whose actor a script
    /// has moved (an `A3` seat, an op-`4C 42` lift under the `0x20000000`
    /// height law - `chitei2`'s falling boulder) or turned (op `0x38`,
    /// `4C 48` - `town0c`'s exit rocks) draws this frame. Retail's case-5
    /// draw reads the actor, not the `.MAP` record.
    ///
    /// A placement nothing has moved or turned carries sixteen zeros (its
    /// `[15]` is `0`, which no model's is) and keeps the draw the page baked;
    /// the array is **empty** while no placement is live, which is nearly
    /// every frame of nearly every scene.
    ///
    /// The composition is the native play-window's, not a second one: the
    /// spawn model [`legaia_render_kernels::scene_lighting::placement_model`] over the coplanar lift
    /// and the placed floor offset, through
    /// [`field_env::live_placed_model`] (turn about the draw's own origin,
    /// then displace), and only then into the page's frame
    /// ([`y_up_model`]).
    pub fn field_placement_live_models(&self) -> Vec<f32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let moves = h.world.object_draw_displacements();
        let turns = h.world.object_draw_turn_matrices();
        let live =
            |r: &Option<usize>| r.is_some_and(|r| turns.contains_key(&r) || moves.contains_key(&r));
        if !f.placement_records.iter().any(live) {
            return Vec::new();
        }
        let floor = h.world.placed_floor_offsets(
            f.floor_lut,
            f.placements.iter().map(|d| &d.floor),
            &f.window_keys,
        );
        let mut out = Vec::with_capacity(f.placements.len() * 16);
        for (i, (d, record)) in f.placements.iter().zip(&f.placement_records).enumerate() {
            if !live(record) {
                out.extend_from_slice(&[0.0; 16]);
                continue;
            }
            let lift = f.coplanar_offsets.get(d).copied().unwrap_or([0.0; 3]);
            let spawn = legaia_render_kernels::scene_lighting::placement_model(
                [
                    d.world_x as f32 + lift[0],
                    d.world_y as f32 + lift[1] + floor.get(i).copied().unwrap_or(0) as f32,
                    d.world_z as f32 + lift[2],
                ],
                [d.rot_x, d.rot_y, d.rot_z],
                f.placement_scales.get(i).copied().unwrap_or(1.0),
            )
            .to_cols_array();
            let model = field_env::live_placed_model(&spawn, *record, &turns, &moves);
            out.extend_from_slice(&y_up_model(&model));
        }
        out
    }

    /// Per-placement **parked** mask (parallel to
    /// [`Self::field_placement_slots`]): `1` where the placed object's actor
    /// now stands at the off-map hide box (or carries a zero render scale),
    /// so the draw is skipped. The page builds its placed draws once, at
    /// scene load, from the records parked by then; a script that parks one
    /// later (`rugi`'s entry script runs `A3 06 7F 7F` on the stone block the
    /// opened wall leaves behind) hides it through this mask. **Empty** while
    /// no placement is parked. The same `World::hidden_object_records` set
    /// the native play-window reads per frame.
    pub fn field_placement_parked(&self) -> Vec<u8> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let parked = h.world.hidden_object_records();
        if parked.is_empty() {
            return Vec::new();
        }
        let mask: Vec<u8> = f
            .placement_records
            .iter()
            .map(|r| u8::from(r.is_some_and(|r| parked.contains(&r))))
            .collect();
        if mask.iter().all(|&m| m == 0) {
            return Vec::new();
        }
        mask
    }

    /// Per-placement **draw tint** (parallel to
    /// [`Self::field_placement_slots`]), flattened `[r, g, b, ir0]`: the far
    /// colour (display `0..1`) and `IR0` (`1.0 = 0x1000`) of a constant
    /// per-draw depth cue, from the placed object's actor `+0x74` / `+0x78`
    /// (op `4C 81` - chitei2's hologram panels go black once the generator
    /// is down). `ir0 == 0` = untinted. **Empty** while no placement is
    /// tinted. The same `World::object_draw_tints` table the native
    /// play-window stages per placed draw.
    pub fn field_placement_tints(&self) -> Vec<f32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let tints = h.world.object_draw_tints();
        if tints.is_empty() {
            return Vec::new();
        }
        let mut any = false;
        let mut out = Vec::with_capacity(f.placement_records.len() * 4);
        for r in &f.placement_records {
            match r.and_then(|r| tints.get(&r)) {
                Some(&(colour, blend)) => {
                    let (far, ir0) = legaia_engine_field::object_tint::tint_cue(colour, blend);
                    out.extend_from_slice(&[far[0], far[1], far[2], ir0]);
                    any = true;
                }
                None => out.extend_from_slice(&[0.0; 4]),
            }
        }
        if any { out } else { Vec::new() }
    }

    /// Per-placement **live model** (parallel to
    /// [`Self::field_placement_slots`]): the env-pack slot a placed object's
    /// scripted-motion stream swapped in with op `0x0E` (koin3's video
    /// wall), or `-1` for a placement drawing its own slot. **Empty** while
    /// no stream has swapped a model. The same `World::object_live_models`
    /// table the native play-window draws its placed objects through.
    pub fn field_placement_models(&self) -> Vec<i32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let models = h.world.object_live_models();
        if models.is_empty() {
            return Vec::new();
        }
        field_env::placed_model_swaps(&f.placement_records, models)
            .into_iter()
            .map(|id| id.map_or(-1, |id| id as i32))
            .collect()
    }

    /// Per-placement **live** mask (parallel to [`Self::field_placement_slots`]):
    /// `1` = draw it this frame, `0` = the placement is a sub-area window
    /// sweep's whose actor is not on the world's windowed static-object list.
    /// Every entry is `1` unless retail windowing is on
    /// ([`Self::set_retail_static_window`]). The same
    /// [`legaia_engine_world_views::scene_staging::placed_draw_live`] kernel the native play-window's
    /// placed-object pass asks per draw.
    pub fn field_placement_live(&self) -> Vec<u8> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let window = &h.world.terrain.static_window;
        f.window_keys
            .iter()
            .map(|k| {
                u8::from(legaia_engine_world_views::scene_staging::placed_draw_live(
                    k.as_ref(),
                    window,
                ))
            })
            .collect()
    }

    /// Per-placement **actor-cull** mask (parallel to
    /// [`Self::field_placement_slots`]): `1` = the placed object's actor is
    /// culled this frame (outside the region box or the visible tile window
    /// widened by its record's cull radius - retail's `FUN_801D79E8`, which
    /// the actor draw walk honours), so the page skips it. **Empty** while
    /// the visible-tile crop does not apply (no crop = draw every placement).
    /// The same `field_view_window::placed_actor_visible` kernel the native
    /// play-window's placed-object pass asks per draw.
    pub fn field_placement_culled(&self, debug_camera: bool) -> Vec<u8> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        let Some(cells) = self.field_view_cells_now(debug_camera) else {
            return Vec::new();
        };
        let moves = h.world.object_draw_displacements();
        let per = field_env::placed_draw_displacements(&f.placement_records, &moves);
        f.placements
            .iter()
            .zip(per)
            .map(|(d, m)| {
                u8::from(
                    !legaia_engine_world_views::field_view_window::placed_actor_visible(
                        &h.world,
                        Some(&cells),
                        d.world_x + m[0],
                        d.world_z + m[2],
                        d.cull_radius,
                    ),
                )
            })
            .collect()
    }

    /// A stamp that moves whenever [`Self::field_placement_culled`] can: the
    /// published cull view (focus, region box, window) and the crop's own
    /// stamp; `0` while no crop applies. The page re-reads the mask only when
    /// it moves.
    pub fn field_placement_cull_stamp(&self, debug_camera: bool) -> u32 {
        let Some(cells) = self.field_view_cells_now(debug_camera) else {
            return 0;
        };
        let Some(view) = self.scene_host.host().and_then(|h| h.world.npcs.cull_view) else {
            return 0;
        };
        let mut h: u32 = cells.stamp();
        for v in [
            view.focus_stored[0],
            view.focus_stored[1],
            i32::from_le_bytes(view.attr_box),
            i32::from_le_bytes(view.window.map(|b| b as u8)),
        ] {
            for b in v.to_le_bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
            }
        }
        h.max(1)
    }

    /// Whether retail's placed-object near reject drops a placed draw whose
    /// origin sits at clip `w` = `origin_view_depth` under this frame's
    /// retail camera - the same `field_env::placed_origin_near_culled` kernel
    /// the native play-window's placed-object pass asks per draw. It runs
    /// only from retail's vantage (`field_view_window::retail_vantage`, the
    /// predicate the native pass asks); the page skips the call under its
    /// own `F3` camera.
    pub fn field_placed_near_culled(&self, origin_view_depth: f32) -> bool {
        self.scene_host.host().is_some_and(|h| {
            legaia_engine_world_views::field_view_window::retail_vantage(
                &h.world,
                self.scene_host.camera(),
                false,
            )
        }) && field_env::placed_origin_near_culled(origin_view_depth)
    }

    /// A stamp that changes whenever [`Self::field_placement_live`] can: the
    /// windowed list's rebuild generation, with the retail-windowing flag in
    /// bit 0. The page re-reads the mask only when this moves.
    pub fn field_static_window_stamp(&self) -> u32 {
        self.scene_host.host().map_or(0, |h| {
            let w = &h.world.terrain.static_window;
            (w.generation << 1) | u32::from(w.retail_windowing)
        })
    }

    /// Turn retail static-object windowing on / off (the
    /// `retail_static_window` option, persisted like the native config).
    pub fn set_retail_static_window(&mut self, on: bool) {
        if self.options_state.retail_static_window != on {
            self.options_state.retail_static_window = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether retail static-object windowing is on.
    pub fn retail_static_window(&self) -> bool {
        self.options_state.retail_static_window
    }

    /// Live pose key of each placement (parallel to
    /// [`Self::field_placement_slots`]): `-1` for a static prop (no anim, or
    /// no live prop-bank entry), else the prop's
    /// [`PropPoseKey::to_i32`] (`PropAnimBank::pose_key` - the `actor+0x68`
    /// cursor the frame blender poses from, plus the clamp bit; `0` is the
    /// rest pose). The world advances every prop's cursor each field tick
    /// (`tick_prop_interactions` -> `PropAnimBank::tick_anims`, retail's
    /// `FUN_800204F8`), so an animated prop - the windmill sails, a swinging
    /// door mid-swing - reports a changing key, and the page re-poses it by
    /// handing the key back to [`Self::field_mesh_posed_frame_positions`].
    pub fn field_placement_frames(&self) -> Vec<i32> {
        let (Some(f), Some(h)) = (self.field.as_ref(), self.scene_host.host()) else {
            return Vec::new();
        };
        f.placements
            .iter()
            .map(|d| {
                if d.anim_id == 0 {
                    return -1;
                }
                h.world
                    .props
                    .bank
                    .pose_key(d.anchor)
                    .map(PropPoseKey::to_i32)
                    .unwrap_or(-1)
            })
            .collect()
    }
}

/// A retail-frame (Y-down) column-major model in the page's Y-up world:
/// `diag(1, -1, 1) * model`, which negates the Y row of every column. The
/// page draws retail-frame vertices through it, so the flip sits on the left
/// of the whole composition - the one place the placed draws' frame change
/// is written.
fn y_up_model(model: &[f32; 16]) -> [f32; 16] {
    let mut out = *model;
    for c in 0..4 {
        out[c * 4 + 1] = -out[c * 4 + 1];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page-frame model of a turned, moved placement is the conjugated
    /// turn on the flipped spawn linear part and the flipped displacement on
    /// the flipped origin - the arithmetic the page used to spell itself, now
    /// one flip over the native composition.
    #[test]
    fn y_up_of_the_native_live_model_is_the_page_frame_composition() {
        let spawn = legaia_render_kernels::scene_lighting::placement_model(
            [120.0, -48.0, 900.0],
            [0x155, 0x300, 0xE20],
            0.5,
        )
        .to_cols_array();
        let turn = field_env::object_turn_matrix([0x155, 0x300, 0xE20], [0x400, 0x0C0, 0x010]);
        let turns = std::collections::HashMap::from([(7usize, turn)]);
        let moves = std::collections::HashMap::from([(7usize, [30, -700, -12])]);
        let got = y_up_model(&field_env::live_placed_model(
            &spawn,
            Some(7),
            &turns,
            &moves,
        ));

        let flip = |r: usize, c: usize| if (r == 1) != (c == 1) { -1.0f32 } else { 1.0 };
        let base = y_up_model(&spawn);
        let mut want = base;
        for c in 0..3 {
            for row in 0..3 {
                want[c * 4 + row] = (0..3)
                    .map(|j| flip(row, j) * turn[j * 4 + row] * base[c * 4 + j])
                    .sum();
            }
        }
        want[12] += 30.0;
        want[13] -= -700.0;
        want[14] += -12.0;
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-3, "{got:?} vs {want:?}");
        }
        // An untouched record keeps the spawn model, flipped.
        assert_eq!(
            y_up_model(&field_env::live_placed_model(
                &spawn,
                Some(8),
                &turns,
                &moves
            )),
            base
        );
    }
}
