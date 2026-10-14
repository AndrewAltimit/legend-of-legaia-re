//! The field floor's vertex wave and the visible-tile ground crop: the
//! [`FieldFloorWave`] deformer the live floor ladder drives, its per-frame
//! sync, and the crop that follows the camera's tile window.

use super::*;

/// The live **floor-height ladder** patch for the field draw lists.
///
/// The scene's sixteen-rung elevation ladder is not static: field-VM op `0x4C`
/// nibble-9 sub-`0xE` rewrites all sixteen rungs and sub-`0..2` sets one rung
/// oscillating every frame through `FUN_801DDE34` -> `FUN_801DA930`, which is
/// the travelling wave under `jou`'s organic Seru interior (57 sites; `concnow`
/// carries 34, and `4C 90` occurs 180 times across 19 scenes). Retail's per-cell
/// terrain emitters re-read the scratchpad ladder at `0x1F80035C` every frame,
/// so the drawn ground moves with it.
///
/// The window bakes its `(mesh, model)` lists once per scene, so this keeps the
/// per-draw ladder rungs beside them and folds the difference into each
/// matrix's Y whenever the live ladder moves. Nothing happens - not even an
/// iteration - while the live ladder equals the one the lists were baked
/// against, which is every frame of every scene whose script leaves it alone.
/// The ladders a [`FieldFloorWave`] last folded in: (live, bound-spawn,
/// window-spawn), all scratchpad frame.
pub(super) type SeenLadders = ([i16; 16], [i16; 16], Option<[i16; 16]>);

#[derive(Default)]
pub(in crate::window) struct FieldFloorWave {
    /// MAN-frame ladder the four lists' `world_y` were resolved against.
    pub(super) base: Option<[i16; 16]>,
    /// The ladders last folded in: (live, bound-spawn, window-spawn), all
    /// scratchpad frame. Nothing is re-derived while they hold.
    pub(super) seen: Option<SeenLadders>,
    /// Per-draw Y offset currently folded into each list's matrices.
    pub(super) applied: [Vec<i32>; 4],
    /// Anchors parallel to `field_terrain_draws`.
    pub(in crate::window) terrain: Vec<FloorAnchor>,
    /// Anchors parallel to `field_terrain_color_draws`.
    pub(in crate::window) terrain_color: Vec<FloorAnchor>,
    /// Anchors parallel to `field_placement_draws`.
    pub(in crate::window) placement: Vec<FloorAnchor>,
    /// Anchors parallel to `field_placement_color_draws`.
    pub(in crate::window) placement_color: Vec<FloorAnchor>,
}

impl FieldFloorWave {
    /// Adopt one scene's baked ladder (`Scene::field_floor_height_lut`, MAN
    /// frame) and the four lists' per-draw rungs. Called once per scene load,
    /// after the four lists resolve.
    pub(in crate::window) fn install(
        base: Option<[i16; 16]>,
        terrain: Vec<FloorAnchor>,
        terrain_color: Vec<FloorAnchor>,
        placement: Vec<FloorAnchor>,
        placement_color: Vec<FloorAnchor>,
    ) -> Self {
        FieldFloorWave {
            base,
            seen: None,
            applied: [
                vec![0; terrain.len()],
                vec![0; terrain_color.len()],
                vec![0; placement.len()],
                vec![0; placement_color.len()],
            ],
            terrain,
            terrain_color,
            placement,
            placement_color,
        }
    }

    /// Fold the world's ladders into the four lists' Y translations: the two
    /// terrain lists follow the **live** rungs (`World::terrain.
    /// floor_height_lut`, scratchpad frame) as retail's per-cell sweep does;
    /// the two placed lists stand on the ladder their actors were spawned
    /// against (`World::placed_floor_offsets` - a placed object's Y is set
    /// once at spawn). `placement_keys` are the placed lists' window keys.
    /// Returns the number of draw matrices moved this frame.
    pub(in crate::window) fn apply(
        &mut self,
        world: &legaia_engine_core::world::World,
        lists: [&mut Vec<(usize, Mat4)>; 4],
        placement_keys: [&[Option<legaia_engine_scene::field_env::PlacedWindowKey>]; 2],
    ) -> usize {
        if self.base.is_none() {
            return 0;
        }
        let t = &world.terrain;
        let seen = (
            t.floor_height_lut,
            t.placed_spawn_lut,
            t.static_window.spawn_lut,
        );
        if self.seen == Some(seen) {
            return 0;
        }
        self.seen = Some(seen);
        let live = FloorWave::from_scene_and_world(self.base, &t.floor_height_lut);
        let terrain_offsets = |anchors: &[FloorAnchor]| -> Vec<i32> {
            anchors
                .iter()
                .map(|f| live.map_or(0, |w| w.offset(f)))
                .collect()
        };
        let targets = [
            terrain_offsets(&self.terrain),
            terrain_offsets(&self.terrain_color),
            world.placed_floor_offsets(self.base, &self.placement, placement_keys[0]),
            world.placed_floor_offsets(self.base, &self.placement_color, placement_keys[1]),
        ];
        let mut moved = 0;
        for ((list, applied), target) in lists.into_iter().zip(&mut self.applied).zip(targets) {
            for (((_, model), was), now) in list.iter_mut().zip(applied.iter_mut()).zip(target) {
                let dy = now - *was;
                if dy != 0 {
                    model.w_axis.y += dy as f32;
                    *was = now;
                    moved += 1;
                }
            }
        }
        moved
    }
}

impl PlayWindowApp {
    /// Keep the cropped ground in step with this frame's visible-tile cell
    /// rectangle: re-upload the ground with the index list
    /// `field_ground::crop_indices` keeps whenever
    /// [`legaia_engine_scene::field_view_window::ViewCells::stamp`] moves, and
    /// drop it when the crop is off. The browser play page re-uploads its
    /// ground indices through the same kernel on the same stamp.
    ///
    /// Re-resolve the walk ground through the world's **live** floor-height
    /// ladder and re-upload it when the ladder has moved. Retail's ground
    /// pass (PROT 0900 `FUN_801F6D48`) takes each cell's corner tiers through
    /// the ladder every frame, so a scene whose script animates it (op `0x4C`
    /// nibble 9: `jouina`'s pulsing path, `concnow`'s flesh pits) deforms the
    /// ground per vertex - the shape the floor sampler already walks the
    /// player on. Shared kernel `field_ground::live_render_positions`; the
    /// browser play page re-uploads through it too. Drops the cropped upload
    /// so [`Self::sync_ground_crop`] rebuilds it from the moved positions.
    pub(in crate::window) fn sync_ground_wave(&mut self) {
        if self.session.host.world.mode == SceneMode::WorldMap {
            return;
        }
        let live = self.session.host.world.terrain.floor_height_lut;
        let Some(src) = self.ground_src.as_mut() else {
            return;
        };
        if src.lut_applied == Some(live)
            || (src.lut_applied.is_none()
                && src.vmesh.positions
                    == legaia_engine_scene::field_ground::live_render_positions(&src.hf, &live))
        {
            src.lut_applied = Some(live);
            return;
        }
        src.lut_applied = Some(live);
        src.vmesh.positions =
            legaia_engine_scene::field_ground::live_render_positions(&src.hf, &live);
        src.flat_refs = legaia_engine_scene::field_ground::flat_refs(&src.hf, &src.vmesh.positions);
        let Some(r) = self.win.renderer.as_ref() else {
            return;
        };
        let v = &src.vmesh;
        match r.upload_vram_mesh_with_flat_refs(
            &v.positions,
            &v.uvs,
            &v.cba_tsb,
            &v.normals,
            &v.colors,
            &v.indices,
            &src.flat_refs,
        ) {
            Ok(m) => self.ground_heightfield = Some(m),
            Err(e) => log::warn!("live ground re-upload skipped: {e:#}"),
        }
        self.ground_crop = None;
    }
}

impl PlayWindowApp {
    pub(in crate::window) fn sync_ground_crop(
        &mut self,
        cells: Option<&legaia_engine_scene::field_view_window::ViewCells>,
    ) {
        let Some(cells) = cells else {
            self.ground_crop = None;
            return;
        };
        let stamp = cells.stamp();
        if self.ground_crop.as_ref().is_some_and(|(s, _)| *s == stamp) {
            return;
        }
        let (Some(src), Some(r)) = (self.ground_src.as_ref(), self.win.renderer.as_ref()) else {
            self.ground_crop = None;
            return;
        };
        let v = &src.vmesh;
        let indices =
            legaia_engine_scene::field_ground::crop_indices(&v.positions, &v.indices, Some(cells));
        if let Some(d) = self.draw_census.as_mut() {
            d.ground_crop = Some(legaia_engine_system::draw_census::MeshCensus::from_mesh(
                &v.positions,
                &v.cba_tsb,
                &v.colors,
                &indices,
            ));
        }
        // An empty crop is a real answer (no ground cell in the rectangle),
        // kept as `(stamp, None)` so the full ground does not stand in for it.
        if indices.is_empty() {
            self.ground_crop = Some((stamp, None));
            return;
        }
        self.ground_crop = r
            .upload_vram_mesh_with_flat_refs(
                &v.positions,
                &v.uvs,
                &v.cba_tsb,
                &v.normals,
                &v.colors,
                &indices,
                &src.flat_refs,
            )
            .map_err(|e| log::warn!("cropped ground upload skipped: {e:#}"))
            .ok()
            .map(|m| (stamp, Some(m)));
    }
}
