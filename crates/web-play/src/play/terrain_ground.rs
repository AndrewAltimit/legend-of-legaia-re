//! Terrain draws and the walk ground: terrain transforms and facing, the
//! visible-tile window, the floor wave, and the ground heightfield buffers.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Per-terrain-draw authored pitch (object record `+0x08`), parallel to
    /// [`Self::field_terrain_slots`]. Composed with yaw and roll in retail's
    /// `Rx * Ry * Rz` order, as the native window composes every terrain
    /// cell (`battle_intro::placement_rotation`).
    pub fn field_terrain_rot_x(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_x).collect())
            .unwrap_or_default()
    }

    /// Per-terrain-draw authored roll (object record `+0x0C`). See
    /// [`Self::field_terrain_rot_x`].
    pub fn field_terrain_rot_z(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_z).collect())
            .unwrap_or_default()
    }

    /// The **camera-facing** terrain / decoration cells' linear model parts
    /// for this frame, flattened `[terrain index, 9 column-major floats]` per
    /// cell, in the page's Y-up frame (`F * K * R`, `F = scale(1,-1,1)`): the
    /// record's own `Rx * Ry * Rz` behind the basis
    /// `gte::decoration_cell_basis` resolves against the engine camera -
    /// retail's decoration pass `FUN_801F7088` drops the flagged axes
    /// (`+0x12 & 0x380`) from the camera rotation before the cell's own, so
    /// `rugi`'s candle glows and the `vell` forest trees face the lens. The
    /// page keeps each cell's translation and swaps in this linear part.
    /// **Empty** with no flagged cell, or when `vp` is not the engine
    /// camera's matrix (the page drew through its own orbit). The native
    /// window rebuilds the same draws per frame.
    pub fn field_terrain_facing(&self, vp: &[f32]) -> Vec<f32> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        if f.terrain.iter().all(|d| d.view_skip == 0) {
            return Vec::new();
        }
        let Some(cam) = self
            .engine_camera
            .filter(|(m, _)| m.as_slice() == vp)
            .and_then(|(_, frame)| frame.field_view())
            .map(|v| legaia_render_kernels::gte::PartCameraPose::from_field_view(&v))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, d) in f.terrain.iter().enumerate() {
            let Some(k) =
                legaia_render_kernels::gte::decoration_cell_basis(d.view_skip, Some(&cam))
            else {
                continue;
            };
            let r =
                legaia_render_kernels::battle_intro::placement_rotation(d.rot_x, d.rot_y, d.rot_z);
            let kr = glam::Mat3::from_cols_array_2d(&k).transpose() * glam::Mat3::from_mat4(r);
            let page = glam::Mat3::from_diagonal(glam::Vec3::new(1.0, -1.0, 1.0)) * kr;
            out.push(i as f32);
            out.extend_from_slice(&page.to_cols_array());
        }
        out
    }

    /// This frame's visible-tile cell rectangle through the shared
    /// `field_view_window::field_view_cells` kernel the native play-window
    /// asks: `None` = draw the map whole. `debug_camera` is the page's `F3`
    /// vantage, which (like the native window's) lifts the crop.
    pub(super) fn field_view_cells_now(
        &self,
        debug_camera: bool,
    ) -> Option<legaia_engine_scene::field_view_window::ViewCells> {
        let h = self.scene_host.host()?;
        legaia_engine_world_views::field_view_window::field_view_cells(
            &h.world,
            legaia_engine_scene::field_view_window::framing_is_retail(self.scene_host.camera())
                && !debug_camera,
        )
    }

    /// A stamp that moves whenever the visible-tile crop can change what
    /// [`Self::field_terrain_live`] and [`Self::field_ground_indices_cropped`]
    /// return; `0` while no crop applies. The page re-reads both only when it
    /// moves - the native window re-uploads its ground on the same stamp.
    pub fn field_view_window_stamp(&self, debug_camera: bool) -> u32 {
        self.field_view_cells_now(debug_camera)
            .map_or(0, |c| c.stamp())
    }

    /// Per-terrain-draw **live** mask (parallel to [`Self::field_terrain_slots`]):
    /// `1` = inside this frame's visible-tile crop
    /// (`field_view_window::terrain_draw_visible`, the gate the native
    /// window's terrain pass asks per draw). All `1` while no crop applies.
    pub fn field_terrain_live(&self, debug_camera: bool) -> Vec<u8> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let cells = self.field_view_cells_now(debug_camera);
        f.terrain
            .iter()
            .map(|d| {
                u8::from(
                    legaia_engine_scene::field_view_window::terrain_draw_visible(
                        cells.as_ref(),
                        legaia_engine_scene::field_view_window::CellKey::of_draw(d),
                    ),
                )
            })
            .collect()
    }

    /// [`Self::field_ground_indices`] cropped to this frame's visible cells
    /// (`field_ground::crop_indices`, the kernel the native window re-uploads
    /// its ground through). The whole list while no crop applies.
    pub fn field_ground_indices_cropped(&self, debug_camera: bool) -> Vec<u32> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        let cells = self.field_view_cells_now(debug_camera);
        legaia_engine_scene::field_ground::crop_indices(
            &hf.positions,
            &legaia_engine_scene::field_ground::render_indices(hf),
            cells.as_ref(),
        )
    }

    /// Turn retail's visible-tile crop on / off (the `retail_view_window`
    /// option, persisted like the native config). It only applies at retail
    /// framing either way.
    pub fn set_retail_view_window(&mut self, on: bool) {
        if self.options_state.retail_view_window != on {
            self.options_state.retail_view_window = on;
            self.persist_and_apply_options();
        }
    }

    /// Whether retail's visible-tile crop is on.
    pub fn retail_view_window(&self) -> bool {
        self.options_state.retail_view_window
    }

    pub fn field_terrain_slots(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.env_slot as u32).collect())
            .unwrap_or_default()
    }

    pub fn field_terrain_positions(&self) -> Vec<f32> {
        self.field
            .as_ref()
            .map(|f| env_positions(&f.terrain, &f.coplanar_offsets))
            .unwrap_or_default()
    }

    /// Per-draw **floor-wave** Y offsets for the static map, terrain draws
    /// first then placement draws - the two lists `field_terrain_positions()`
    /// and `field_placement_positions()` return, concatenated in that order.
    ///
    /// The scene's sixteen-rung floor-height ladder is script-animated: op
    /// `0x4C` nibble-9 sub-`0..2` sets a rung oscillating every frame
    /// (`FUN_801DDE34` -> `FUN_801DA930`), which is the travelling wave under
    /// `jou`'s organic interior floor. The page bakes its draw positions once
    /// per scene, so this is what moves the drawn ground with the walk
    /// heightfield instead of leaving it at the disc-static tier.
    ///
    /// **Empty** whenever the live ladder equals the baked one, which is every
    /// frame of every scene whose script leaves it alone - the page skips the
    /// whole pass on an empty return.
    pub fn field_floor_wave_offsets(&self) -> Vec<f32> {
        let Some(f) = self.field.as_ref() else {
            return Vec::new();
        };
        let Some(h) = self.scene_host.host() else {
            return Vec::new();
        };
        // The terrain / decoration cells follow the live rungs; a placed
        // object stands on the ladder its actor was spawned against
        // (`World::placed_floor_offsets`).
        let wave = field_env::FloorWave::from_scene_and_world(
            f.floor_lut,
            &h.world.terrain.floor_height_lut,
        );
        let placed = h.world.placed_floor_offsets(
            f.floor_lut,
            f.placements.iter().map(|d| &d.floor),
            &f.window_keys,
        );
        if wave.is_none() && placed.iter().all(|&o| o == 0) {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(f.terrain.len() + f.placements.len());
        out.extend(
            f.terrain
                .iter()
                .map(|d| wave.map_or(0, |w| w.offset(&d.floor)) as f32),
        );
        out.extend(placed.into_iter().map(|o| o as f32));
        out
    }

    pub fn field_terrain_rot_y(&self) -> Vec<u16> {
        self.field
            .as_ref()
            .map(|f| f.terrain.iter().map(|d| d.rot_y).collect())
            .unwrap_or_default()
    }

    /// The walk-ground heightfield's drawn positions, flattened - through the
    /// shared [`legaia_engine_scene::field_ground::render_positions`] kernel
    /// (the `GROUND_SINK`) the native window's ground mesh also runs.
    pub fn field_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        legaia_engine_scene::field_ground::render_positions(hf)
            .into_iter()
            .flatten()
            .collect()
    }

    /// The walk-ground heightfield's drawn positions re-resolved through the
    /// world's **live** floor-height ladder
    /// ([`legaia_engine_scene::field_ground::live_render_positions`]), when it
    /// has moved since the last call - empty when nothing changed, so the
    /// page re-uploads only on a frame the ladder actually moved. Field VM op
    /// `0x4C` nibble 9 animates the ladder (`jouina`'s pulsing path,
    /// `concnow`'s flesh pits), and the floor sampler the player stands on
    /// reads the same ladder, so the drawn ground and the collision surface
    /// stay one shape. The native window re-uploads through the same kernel.
    pub fn field_ground_live_positions(&mut self) -> Vec<f32> {
        let Some(live) = self
            .scene_host
            .host()
            .map(|h| h.world.terrain.floor_height_lut)
        else {
            return Vec::new();
        };
        let Some(f) = self.field.as_mut() else {
            return Vec::new();
        };
        let Some(hf) = f.ground.as_ref() else {
            return Vec::new();
        };
        if f.is_world_map || f.ground_lut_applied == Some(live) {
            return Vec::new();
        }
        f.ground_lut_applied = Some(live);
        legaia_engine_scene::field_ground::live_render_positions(hf, &live)
            .into_iter()
            .flatten()
            .collect()
    }

    pub fn field_ground_uvs(&self) -> Vec<u8> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        hf.uvs.iter().flatten().copied().collect()
    }

    pub fn field_ground_cba_tsb(&self) -> Vec<u16> {
        let Some(hf) = self.field.as_ref().and_then(|f| f.ground.as_ref()) else {
            return Vec::new();
        };
        hf.cba_tsb.iter().flatten().copied().collect()
    }

    /// The walk-ground heightfield's drawn triangles, through the shared
    /// [`legaia_engine_scene::field_ground::render_indices`] kernel: reversed
    /// onto the scene TMDs' winding, so the cutscene camera's NCLIP pass keeps
    /// the ground on this page as it does in the native window.
    pub fn field_ground_indices(&self) -> Vec<u32> {
        self.field
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(legaia_engine_scene::field_ground::render_indices)
            .unwrap_or_default()
    }

    pub fn field_ground_quad_count(&self) -> u32 {
        self.field
            .as_ref()
            .and_then(|f| f.ground.as_ref())
            .map(|hf| hf.quad_count() as u32)
            .unwrap_or(0)
    }
}
