//! The dance floor's step-marker tiles: the bake off the `.MAP` kind-1
//! records, the per-frame step, and the marker buffers the page uploads.

use super::*;

/// Load base of the dance overlay (slot A - `crates/asset/data/static-overlays.toml`).
const DANCE_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// `.MAP` primary trigger block, and the `+0x12000` fallback the same kind-1
/// scan falls through to.
const TRIGGER_BLOCK_OFFSET: usize = 0x10000;
const TRIGGER_FALLBACK_OFFSET: usize = 0x12000;

/// The dance floor's **step-marker tiles**: the per-cell actors retail's floor
/// pass `FUN_801D2A10` spawns for every cell whose kind-1 `.MAP` record
/// resolves to clip `6..=9`, each flipping its mesh through the class row of
/// the script table at `0x801D44CC` (`FUN_801D0640`).
///
/// The page's renderer owns ONE mesh with a static index buffer and re-uploads
/// only positions per frame, so a tile whose mesh changes cannot simply be
/// re-baked. Instead every tile contributes one span per **candidate** mesh of
/// its class row - all of them, with their real geometry and indices - and each
/// frame the spans whose mesh is not the tile's current one collapse to a
/// single point, which makes their triangles degenerate and covers no pixels.
/// Static topology, per-frame positions, and no allocation in the frame path.
#[derive(Default)]
pub(crate) struct DanceMarkers {
    floor: legaia_engine_field::minigame_floor::MarkerFloor,
    /// Base positions - every span at its real world position.
    positions: Vec<f32>,
    uvs: Vec<i32>,
    cba_tsb: Vec<u32>,
    flat: Vec<u8>,
    indices: Vec<u32>,
    /// `(tile index, this span's mesh value, first vertex, vertex count)`.
    spans: Vec<(usize, i16, usize, usize)>,
    /// Working copy handed to the page each frame.
    live: Vec<f32>,
}

impl DanceMarkers {
    fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Advance every tile's flipbook one frame.
    ///
    /// `pack_bias` is `0` deliberately: the page indexes the scene's own env
    /// pack, and retail's `_DAT_8007B6F8` bias turns the table value into a
    /// **global pool** index instead.
    fn step(&mut self, frame_delta: u8) {
        self.floor.step(frame_delta, 0);
    }

    /// This frame's positions, with the non-current spans collapsed.
    fn live_positions(&mut self) -> &[f32] {
        self.live.clear();
        self.live.extend_from_slice(&self.positions);
        let tiles = self.floor.tiles();
        for &(tile, mesh, start, len) in &self.spans {
            let current = tiles.get(tile).and_then(|t| t.actor.mesh);
            if current == Some(mesh) || len == 0 {
                continue;
            }
            let (x, y, z) = (
                self.live[start * 3],
                self.live[start * 3 + 1],
                self.live[start * 3 + 2],
            );
            for v in 0..len {
                self.live[(start + v) * 3] = x;
                self.live[(start + v) * 3 + 1] = y;
                self.live[(start + v) * 3 + 2] = z;
            }
        }
        &self.live
    }
}

/// Resolve the venue's marker tiles and bake every candidate mesh of every
/// tile, re-based on the human dancer's spawn like [`bake_dance_env`].
///
/// Returns an empty set - not a panic - when the venue carries no marker cell,
/// when the `.MAP` will not resolve, or when the overlay's script table will
/// not parse: a page that states the gap is better than one that fakes tiles.
pub(super) fn bake_dance_markers(
    index: &ProtIndex,
    scene: &Scene,
    res: &SceneResources,
    overlay: &[u8],
    origin: (f32, f32, f32),
) -> DanceMarkers {
    use legaia_engine_field::minigame_floor as mf;

    let Some(script) = legaia_engine_field::dance_marker::MarkerScript::from_overlay(
        overlay,
        DANCE_OVERLAY_BASE_VA,
    ) else {
        return DanceMarkers::default();
    };
    let Some(map_idx) = scene.field_map_index(index) else {
        return DanceMarkers::default();
    };
    let Ok(map) = index.entry_bytes_extended(map_idx) else {
        return DanceMarkers::default();
    };
    let primary =
        legaia_engine_field::field_regions::parse_tile_triggers(&map[TRIGGER_BLOCK_OFFSET..]);
    let fallback = map
        .get(TRIGGER_FALLBACK_OFFSET..)
        .map(legaia_engine_field::field_regions::parse_tile_triggers)
        .unwrap_or_default();
    let ramp = mf::height_ramp();
    let floor = mf::MarkerFloor::build(
        mf::FloorGrid::new(&map),
        &ramp,
        0,
        0,
        mf::GRID_EXTENT,
        mf::GRID_EXTENT,
        false,
        &primary,
        &fallback,
        script.clone(),
    );
    if floor.is_empty() {
        return DanceMarkers::default();
    }

    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let mut out = DanceMarkers {
        floor,
        ..Default::default()
    };
    let mut built: HashMap<usize, (VramMesh, Vec<u8>)> = HashMap::new();
    let tiles: Vec<_> = out.floor.tiles().to_vec();
    for (ti, tile) in tiles.iter().enumerate() {
        // Every distinct mesh the tile's class row can stage, in row order.
        let mut meshes: Vec<i16> = Vec::new();
        for step in 0..script.steps(tile.actor.class as usize) {
            if let Some((m, _)) = script.entry(tile.actor.class as usize, step)
                && !meshes.contains(&m)
            {
                meshes.push(m);
            }
        }
        for mesh in meshes {
            let Ok(slot) = usize::try_from(mesh) else {
                continue;
            };
            let Some(&res_tmd) = env_tmds.get(slot) else {
                continue;
            };
            let Some(rtmd) = res.tmds.get(res_tmd) else {
                continue;
            };
            let entry = built.entry(res_tmd).or_insert_with(|| {
                legaia_engine_scene::scene_assembly::build_hybrid_env_mesh(rtmd, &res.vram)
            });
            let (vm, flat) = (&entry.0, &entry.1);
            if vm.positions.is_empty() {
                continue;
            }
            let start = out.positions.len() / 3;
            let theta = (tile.rot[1] & 0xFFF) as f32 * (std::f32::consts::TAU / 4096.0);
            let (sin, cos) = theta.sin_cos();
            for p in &vm.positions {
                let (vx, vy, vz) = (p[0], p[1], p[2]);
                out.positions
                    .push(vx * cos + vz * sin + tile.pos[0] as f32 - origin.0);
                out.positions.push(vy + tile.pos[1] as f32 - origin.1);
                out.positions
                    .push(-vx * sin + vz * cos + tile.pos[2] as f32 - origin.2);
            }
            for uv in &vm.uvs {
                out.uvs.push(uv[0] as i32);
                out.uvs.push(uv[1] as i32);
            }
            for ct in &vm.cba_tsb {
                out.cba_tsb.push(ct[0] as u32);
                out.cba_tsb.push(ct[1] as u32);
            }
            if flat.is_empty() {
                let neutral = [
                    legaia_engine_field::packet_color::NEUTRAL,
                    legaia_engine_field::packet_color::NEUTRAL,
                    legaia_engine_field::packet_color::NEUTRAL,
                    255,
                ];
                out.flat
                    .extend(std::iter::repeat_n(neutral, vm.positions.len()).flatten());
            } else {
                out.flat.extend_from_slice(flat);
            }
            let base = start as u32;
            out.indices.extend(vm.indices.iter().map(|i| i + base));
            out.spans.push((ti, mesh, start, vm.positions.len()));
        }
    }
    out
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Per-vertex `[r, g, b, textured_flag]` for the baked hall's hybrid
    /// textured / vertex-colour render (same convention as the bodies).
    /// Number of step-marker tiles on the venue floor (`0` when its `.MAP`
    /// carries no clip-`6..=9` cell, which is what the page checks before
    /// appending the block at all).
    pub fn dance_marker_tiles(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| b.markers.floor.len() as u32)
            .unwrap_or(0)
    }

    /// Static marker geometry: every candidate mesh of every tile, in one
    /// block. Uploaded once; only [`Self::dance_marker_positions`] changes per
    /// frame (see [`DanceMarkers`] for why every candidate is baked).
    pub fn dance_marker_uvs(&self) -> Vec<i32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.markers.uvs.clone())
            .unwrap_or_default()
    }

    /// See [`Self::dance_marker_uvs`].
    pub fn dance_marker_cba_tsb(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.markers.cba_tsb.clone())
            .unwrap_or_default()
    }

    /// See [`Self::dance_marker_uvs`].
    pub fn dance_marker_indices(&self) -> Vec<u32> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.markers.indices.clone())
            .unwrap_or_default()
    }

    /// See [`Self::dance_marker_uvs`].
    pub fn dance_marker_flat_rgba(&self) -> Vec<u8> {
        self.dance_bodies
            .as_ref()
            .map(|b| b.markers.flat.clone())
            .unwrap_or_default()
    }

    /// Vertex count of the marker block - what the page reserves after the
    /// hall's.
    pub fn dance_marker_vertex_count(&self) -> u32 {
        self.dance_bodies
            .as_ref()
            .map(|b| (b.markers.positions.len() / 3) as u32)
            .unwrap_or(0)
    }

    /// Advance every marker tile's flipbook by `frame_delta` retail frames
    /// (`FUN_801D0640`), then hand back this frame's positions.
    ///
    /// The spans whose mesh is not the tile's current one come back collapsed
    /// onto a single point, so their triangles are degenerate and the static
    /// index buffer needs no edit.
    pub fn dance_marker_step(&mut self, frame_delta: u8) -> Vec<f32> {
        let Some(b) = self.dance_bodies.as_mut() else {
            return Vec::new();
        };
        if b.markers.is_empty() {
            return Vec::new();
        }
        b.markers.step(frame_delta);
        b.markers.live_positions().to_vec()
    }

    /// This frame's marker positions without advancing the flipbook - the
    /// initial upload.
    pub fn dance_marker_positions(&mut self) -> Vec<f32> {
        let Some(b) = self.dance_bodies.as_mut() else {
            return Vec::new();
        };
        if b.markers.is_empty() {
            return Vec::new();
        }
        b.markers.live_positions().to_vec()
    }
}
