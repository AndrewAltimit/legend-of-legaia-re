//! Extracted from `window.rs` (mechanical split; behavior-preserving).
//!
//! Render-side geometry helpers: effect billboards, world-map marker /
//! slot-4 wireframe line geometry, the terrain heightfield-to-mesh bridge,
//! and the battle ground grid builder.

use super::*;

/// Raw `LineList` geometry: `(positions, per-vertex colours, line indices)`.
/// The geometry helpers (`world_map_*_line_geometry`) emit this shape; it is
/// uploaded via `Renderer::upload_lines`.
pub(crate) type LineGeometry = (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>);

/// Extra world-unit scale on an effect billboard. The sprite's `size` is
/// already the retail pass-2 world size (`atlas w/h * sprite_scale >> 8`),
/// so the identity scale draws it faithfully.
const EFFECT_TEXEL_WORLD: f32 = 1.0;

/// The four world-space corners of a camera-facing billboard for `sprite`,
/// using the camera's world `right`/`up` basis. Order: TL, TR, BL, BR.
fn effect_sprite_corners(
    sprite: &legaia_engine_core::world::EffectSprite,
    right: Vec3,
    up: Vec3,
) -> [Vec3; 4] {
    let c = Vec3::from(sprite.world_pos);
    let hw = sprite.size[0] * 0.5 * EFFECT_TEXEL_WORLD;
    let hh = sprite.size[1] * 0.5 * EFFECT_TEXEL_WORLD;
    let rx = right * hw;
    let uy = up * hh;
    [c - rx + uy, c + rx + uy, c - rx - uy, c + rx - uy]
}

/// Build a textured billboard mesh for the live effect sprites: one
/// camera-facing quad per child, sampling the scene VRAM at the sprite's
/// atlas `(u, v)` / `tpage` / `clut`. Mirrors the retail per-frame walker
/// (`FUN_801E0088` pass 2), which emits one GPU sprite primitive per child.
///
/// The texel-source upload for battle effects is not yet pinned, so a quad
/// over empty VRAM samples all-zero texels which the VRAM-mesh shader
/// discards (clean, not garbage); real pixels appear once that upload lands.
/// Returns `None` when there is nothing to draw.
pub(crate) fn effect_billboard_mesh(
    r: &legaia_engine_render::Renderer,
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> Option<UploadedVramMesh> {
    if sprites.is_empty() {
        return None;
    }
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut uvs: Vec<[u8; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut cba_tsb: Vec<[u16; 2]> = Vec::with_capacity(sprites.len() * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(sprites.len() * 6);
    // Quad faces the camera; a single normal toward the viewer keeps the
    // lambert term stable rather than relying on the derivative fallback.
    let face = right.cross(up).normalize_or_zero().to_array();
    // Per-sprite modulation: the retail pass-2 brightness envelope writes
    // `r = g = b = brightness` on the GPU packet (`0x80` = neutral, the
    // same value as `legaia_prims::MODULATION_NEUTRAL`), so the ramp-in /
    // ramp-out fade is faithful.
    let mut colors: Vec<[u8; 3]> = Vec::with_capacity(sprites.len() * 4);
    for s in sprites {
        let [u0, v0] = s.uv;
        let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1)).min(255) as u8;
        let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1)).min(255) as u8;
        let (mut u0, mut u1) = ((u0 & 0xFF) as u8, u1);
        let (mut v0, mut v1) = ((v0 & 0xFF) as u8, v1);
        // Random UV-mirror corner order (retail pass 2): a set flip swaps
        // which side samples the base texel column/row.
        if s.flip_h {
            std::mem::swap(&mut u0, &mut u1);
        }
        if s.flip_v {
            std::mem::swap(&mut v0, &mut v1);
        }
        let corners = effect_sprite_corners(s, right, up);
        let corner_uv = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]];
        let base = positions.len() as u32;
        for (corner, uv) in corners.iter().zip(corner_uv) {
            positions.push(corner.to_array());
            uvs.push(uv);
            cba_tsb.push([s.clut, s.page]);
            normals.push(face);
            colors.push([s.brightness; 3]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    match r.upload_vram_mesh(&positions, &uvs, &cba_tsb, &normals, &colors, &indices) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("effect billboard mesh upload: {e:#}");
            None
        }
    }
}

/// Build a tinted outline for each effect billboard through the Lines
/// pipeline (a camera-facing rectangle, sized from the sprite atlas, faded by
/// age). This keeps spawned effects visible while the textured-quad's VRAM
/// source is unpinned - the billboard's geometry and animation are faithful
/// even when its texels are not yet resident.
pub(crate) fn effect_sprite_line_geometry(
    sprites: &[legaia_engine_core::world::EffectSprite],
    right: Vec3,
    up: Vec3,
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(sprites.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(sprites.len() * 8);
    for s in sprites {
        let [tl, tr, bl, br] = effect_sprite_corners(s, right, up);
        // Warm spark colour, dimmed as the effect ages toward retirement.
        let fade = (1.0 - s.age01).clamp(0.0, 1.0);
        let c = [
            (80.0 + 175.0 * fade) as u8,
            (200.0 * fade) as u8,
            (255.0 * fade) as u8,
            255,
        ];
        let base = pos.len() as u32;
        for corner in [tl, tr, br, bl] {
            pos.push(corner.to_array());
            col.push(c);
        }
        // Four edges of the rectangle (LineList).
        for &(a, b) in &[(0u32, 1u32), (1, 2), (2, 3), (3, 0)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// RGBA colour of a world-map entity marker, keyed by its kind: portals
/// (town/dungeon entrances) cyan, NPCs green, encounter zones warm red.
fn world_map_entity_marker_color(kind: legaia_engine_core::world::WorldMapEntityKind) -> [u8; 4] {
    use legaia_engine_core::world::WorldMapEntityKind as K;
    match kind {
        K::Portal => [0, 200, 255, 255],
        K::Npc => [80, 220, 80, 255],
        K::EncounterZone => [230, 80, 40, 255],
    }
}

/// Convert a [`WalkHeightfield`] into a renderer [`VramMesh`]. The heightfield
/// supplies per-vertex UVs (the `+0x14` atlas tile) **and** per-vertex
/// `[clut, tpage]` (the cell's terrain page + palette from `+0x15` /
/// `+0x16..+0x18`), so grass / mountain / water / forest cells each sample their
/// own VRAM page within the single ground mesh. Normals are left at the
/// `[0,0,0]` sentinel so the shader derives screen-space normals (flat-lit).
/// See docs/subsystems/world-map.md "Ground texturing".
pub(crate) fn heightfield_to_vram_mesh(
    hf: &legaia_asset::field_objects::WalkHeightfield,
) -> legaia_tmd::mesh::VramMesh {
    let n = hf.positions.len();
    // The heightfield is ENGINE-synthesised geometry (no retail winding to
    // preserve), and its builder happens to wind opposite to the scene TMDs
    // under the field frame. Reverse each triangle so the ground survives
    // the cutscene-camera NCLIP pass (`Renderer::set_backface_cull`) with
    // the same parity as the disc meshes. A no-op for every both-sided pass
    // (the default `cull_mode: None` pipelines draw either winding).
    let mut indices = hf.indices.clone();
    for tri in indices.chunks_exact_mut(3) {
        tri.swap(1, 2);
    }
    legaia_tmd::mesh::VramMesh {
        positions: hf.positions.clone(),
        uvs: hf.uvs.clone(),
        // Per-cell terrain page + palette (multi-page terrain atlas).
        cba_tsb: hf.cba_tsb.clone(),
        normals: vec![[0.0, 0.0, 0.0]; n],
        // The heightfield carries the ground's baked prim colour
        // (`GROUND_PRIM_COLOR`): retail's ground quads are neutral `0x808080`
        // on every cell, so the modulation is the identity and the tile draws
        // at its raw texel. Sourced from the heightfield rather than assumed
        // here, so the one disc-derived fact has one home.
        colors: hf.colors.clone(),
        indices,
    }
}

/// MAN), so they sit correctly relative to the player even while the kingdom
/// terrain mesh renders at its own pack-local coordinates.
pub(crate) fn world_map_entity_line_geometry(
    markers: &[legaia_engine_core::world::WorldMapEntityMarker],
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.06;
    let arm = diag * 0.02;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(markers.len() * 6);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(markers.len() * 6);
    let mut idx: Vec<u32> = Vec::with_capacity(markers.len() * 6);
    for m in markers {
        let [x, y, z] = m.world_pos;
        let c = world_map_entity_marker_color(m.kind);
        let base = pos.len() as u32;
        // 0: base, 1: top (up = world -Y under the geometry convention),
        // 2..=5: base-cross arm ends along +/-X and +/-Z.
        let verts = [
            [x, y, z],
            [x, y - post_h, z],
            [x - arm, y, z],
            [x + arm, y, z],
            [x, y, z - arm],
            [x, y, z + arm],
        ];
        for v in verts {
            pos.push(v);
            col.push(c);
        }
        // Vertical post + the two base-cross segments.
        for &(a, b) in &[(0u32, 1u32), (2, 3), (4, 5)] {
            idx.push(base + a);
            idx.push(base + b);
        }
    }
    (pos, col, idx)
}

/// Build a LineList for the overworld player marker: a taller upright post (so
/// the player reads above the kind-coded entity markers), a base cross, and a
/// facing tick pointing in the player's heading. White-yellow, sized relative
/// to the scene AABB. Same Y-flip convention as the entity markers.
pub(crate) fn world_map_player_line_geometry(
    marker: &legaia_engine_core::world::WorldMapPlayerMarker,
    aabb_lo: [f32; 3],
    aabb_hi: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let diag = (Vec3::from(aabb_hi) - Vec3::from(aabb_lo))
        .length()
        .max(1.0);
    let post_h = diag * 0.09;
    let arm = diag * 0.025;
    let tick = diag * 0.05;
    let [x, y, z] = marker.world_pos;
    let c = [255u8, 230, 60, 255];
    // Heading: PSX 12-bit angle, 0 = +Z, quarter turn (1024) = +X.
    let angle = (marker.facing as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let verts = [
        [x, y, z],                           // 0 base
        [x, y - post_h, z],                  // 1 top
        [x - arm, y, z],                     // 2 -X arm
        [x + arm, y, z],                     // 3 +X arm
        [x, y, z - arm],                     // 4 -Z arm
        [x, y, z + arm],                     // 5 +Z arm
        [x + sin * tick, y, z + cos * tick], // 6 facing tick end
    ];
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(7);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(7);
    for v in verts {
        pos.push(v);
        col.push(c);
    }
    // Post + base-cross (X/Z arms) + facing tick.
    let idx = vec![0, 1, 2, 3, 4, 5, 0, 6];
    (pos, col, idx)
}

/// Build a LineList wireframe of a kingdom's decoded slot-4 vertex pool
/// (`SceneResources::world_map_slot4`), as world-space `(positions, colors,
/// indices)`. Each body's records are emitted at their raw object-local
/// coordinates (no per-object placement transform - the cluster-A command
/// stream that supplies those is unpinned), at raw retail Y-down
/// coordinates (the world-map cameras compose the single world negation). Colour is keyed by body `kind`
/// (`1` = the shared universal mesh set, `2` = kingdom-specific objects,
/// `4` = wide-extent bodies) so the per-kingdom assembly structure reads
/// at a glance. Returns empty geometry when no body yields a segment.
///
/// This is an env-gated inspection overlay (`LEGAIA_WORLDMAP_SLOT4=1`); the
/// group-polyline segment topology is the documented inspection convention,
/// not the faithful triangle topology (see
/// `legaia_asset::world_map_overlay::wireframe_segments_3d`).
pub(crate) fn world_map_slot4_line_geometry(
    slot: &legaia_asset::world_map_overlay::KingdomSlot4,
) -> LineGeometry {
    let opts = legaia_asset::world_map_overlay::WireframeOptions::default();
    let segs = legaia_asset::world_map_overlay::wireframe_segments_3d(slot, &opts);
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(segs.len() * 2);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(segs.len() * 2);
    let mut idx: Vec<u32> = Vec::with_capacity(segs.len() * 2);
    for s in &segs {
        let c = match s.kind {
            1 => [120u8, 200, 255, 255], // shared universal bodies (cyan)
            2 => [255u8, 160, 90, 255],  // kingdom-specific objects (orange)
            4 => [200u8, 120, 255, 255], // wide-extent bodies (violet)
            _ => [180u8, 180, 180, 255],
        };
        let base = pos.len() as u32;
        for v in [s.a, s.b] {
            // Raw retail Y-down coordinates: the world-map cameras compose
            // FIELD_WORLD_FLIP, so no per-vertex negation.
            pos.push([v[0] as f32, v[1] as f32, v[2] as f32]);
            col.push(c);
        }
        idx.push(base);
        idx.push(base + 1);
    }
    (pos, col, idx)
}

/// The battle ground grid's texture address, constant in the retail overlay
/// (`func_0x801d02c0` scratch literals, confirmed against the GT4 packets in
/// the live prim pool of the Tetsu battle savestates): 4bpp texture page at
/// framebuffer `(832, 0)` = tpage attr `0x000D`, CLUT at `(0, 479)` = CBA
/// `0x77C0`. The ADDRESS is scene-independent - the scene's battle VRAM
/// build is what places that scene's own ground tile there (town01 = warm
/// sandy pebbles; the old "borrow the dome's nearest grass vertex" pick
/// sampled a blue texel region in town01 and painted the floor sky-blue).
pub(crate) const BATTLE_GROUND_TSB: u16 = legaia_engine_core::battle_backdrop::GROUND_TSB;
pub(crate) const BATTLE_GROUND_CBA: u16 = legaia_engine_core::battle_backdrop::GROUND_CBA;

/// Cells per side of the live battle grid (`_DAT_1f8003f8` / `_DAT_1f8003fa`).
pub(crate) const BATTLE_GROUND_CELLS: i32 = 28;

/// Build the flat tiled battle ground grid - the port of `func_0x801d02c0`'s
/// pass-2 emit.
///
/// A `28 x 28` field of `0x200`-pitch cells on the PSX `y = 0` plane, textured
/// from the constant [`BATTLE_GROUND_TSB`] / [`BATTLE_GROUND_CBA`] page. Each
/// cell is **four** quads, not one: retail projects a `3 x 3` corner lattice
/// per cell (`RTPT` three times over rows spaced `0x100` apart,
/// `0x801d04e4..0x801d0528`) and then emits `2 x 2` POLY_GT4s from it, taking
/// sub-tile `sub_row * 2 + sub_col` of the `(192..=255)^2` window for each.
/// The sub-tile choice is a table read, not a roll - see
/// [`legaia_engine_core::battle_backdrop::GROUND_SUB_TILE_UVS`].
///
/// The grid origin comes from
/// [`legaia_engine_core::battle_backdrop::grid_origin`], which carries retail's
/// extra `-0x200` bias on `z`.
///
/// # What this build leaves out, and why
///
/// Retail culls cells twice, and does so **every frame**, because the grid is
/// world-fixed and the battle camera orbits over it: a view-space `z` bracket
/// per cell ([`legaia_engine_core::battle_backdrop::classify_cell`]), then a
/// screen-rect bounding-box reject
/// ([`legaia_engine_core::battle_backdrop::cell_offscreen`]). Both kernels are
/// ported and tested; neither is applied here.
///
/// That is deliberate. Retail's culls exist to keep the ordering table and the
/// per-frame primitive budget inside a PSX's means: they remove only cells that
/// are already off-screen or behind the camera, so under a depth-buffered
/// projection with a real near plane they are **visually neutral**. Applying
/// them at build time would be worse than not applying them - the grid is
/// uploaded once and the camera then orbits, so cells culled against the entry
/// pose would stay culled once they swung into view. Applying them per frame
/// would mean rebuilding and re-uploading the whole mesh every frame to delete
/// geometry the GPU discards anyway.
///
/// So the port draws the full grid and the two kernels stand as the tested
/// record of retail's rule, for a host that ever needs the retail primitive
/// counts (a prim-pool parity oracle would).
pub(crate) fn build_battle_ground_grid() -> legaia_tmd::mesh::VramMesh {
    use legaia_engine_core::battle_backdrop as bb;

    const N: i32 = BATTLE_GROUND_CELLS;
    let (x0, z0) = bb::grid_origin(N, N);
    let sub = bb::GRID_SUB_STEP as f32;

    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    for iz in 0..N {
        for ix in 0..N {
            let cx = (x0 + ix * bb::GRID_CELL_PITCH) as f32;
            let cz = (z0 + iz * bb::GRID_CELL_PITCH) as f32;
            // The 2x2 sub-quads of this cell, each a `0x100` step of the
            // cell's own 3x3 corner lattice.
            for sr in 0..bb::GRID_SUB_TILES_PER_SIDE {
                for sc in 0..bb::GRID_SUB_TILES_PER_SIDE {
                    let (qx0, qz0) = (cx + sc as f32 * sub, cz + sr as f32 * sub);
                    let (qx1, qz1) = (qx0 + sub, qz0 + sub);
                    let (ua, va, ub, vb) = bb::ground_sub_tile_uv(sr, sc);
                    let base = m.positions.len() as u32;
                    // Retail's POLY_GT4 corner order: v0 top-left, v1
                    // top-right, v2 bottom-left, v3 bottom-right - the same
                    // order the UV table's four words are written in.
                    for (x, z, u, v) in [
                        (qx0, qz0, ua, va),
                        (qx1, qz0, ub, va),
                        (qx0, qz1, ua, vb),
                        (qx1, qz1, ub, vb),
                    ] {
                        m.positions.push([x, 0.0, z]);
                        m.uvs.push([u, v]);
                        m.cba_tsb.push([BATTLE_GROUND_CBA, BATTLE_GROUND_TSB]);
                        m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y
                        // Neutral modulation: the grid quads draw the raw
                        // tile texel. (NB the old builder pushed NO colours
                        // at all, so its upload failed the attribute-length
                        // check and the grid never drew - the "sky-blue
                        // floor" was the bare battle clear colour showing
                        // through.)
                        m.colors
                            .push([legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]);
                    }
                    m.indices
                        .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod battle_ground_grid_tests {
    use super::{
        BATTLE_GROUND_CBA, BATTLE_GROUND_CELLS, BATTLE_GROUND_TSB, build_battle_ground_grid,
    };
    use legaia_engine_core::battle_backdrop as bb;

    /// Retail emits `2 x 2` quads per cell, so a 28x28 grid is 3136 quads -
    /// four times what the old one-quad-per-cell builder produced.
    #[test]
    fn emits_four_quads_per_cell() {
        let m = build_battle_ground_grid();
        let cells = (BATTLE_GROUND_CELLS * BATTLE_GROUND_CELLS) as usize;
        let quads = cells * 4;
        assert_eq!(m.positions.len(), quads * 4);
        assert_eq!(m.indices.len(), quads * 6);
    }

    /// Every vertex attribute stream has to stay the same length or the
    /// upload's attribute-length check rejects the mesh and the grid silently
    /// does not draw.
    #[test]
    fn attribute_streams_stay_parallel() {
        let m = build_battle_ground_grid();
        let n = m.positions.len();
        assert_eq!(m.uvs.len(), n);
        assert_eq!(m.cba_tsb.len(), n);
        assert_eq!(m.normals.len(), n);
        assert_eq!(m.colors.len(), n);
        assert!(n > 0);
    }

    /// The grid is flat at `y = 0` and spans retail's biased extent: `x` over
    /// `[-7168, +7168]`, `z` pulled one cell back to `[-7680, +6656]`.
    #[test]
    fn lies_flat_over_the_retail_extent() {
        let m = build_battle_ground_grid();
        assert!(m.positions.iter().all(|p| p[1] == 0.0));
        let (x0, z0) = bb::grid_origin(BATTLE_GROUND_CELLS, BATTLE_GROUND_CELLS);
        let span = (BATTLE_GROUND_CELLS * bb::GRID_CELL_PITCH) as f32;
        let min_x = m.positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        let max_x = m.positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        let min_z = m.positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let max_z = m.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert_eq!((min_x, max_x), (x0 as f32, x0 as f32 + span));
        assert_eq!((min_z, max_z), (z0 as f32, z0 as f32 + span));
    }

    /// Every quad samples one of the four table sub-tiles with its corners in
    /// retail's order - no mirrored corner ever reaches the UVs, because the
    /// emitter has no mirror and no roll.
    #[test]
    fn uvs_are_unmirrored_table_sub_tiles() {
        let m = build_battle_ground_grid();
        for q in m.uvs.chunks_exact(4) {
            let (u0, v0) = (q[0][0], q[0][1]);
            let (u1, v1) = (q[3][0], q[3][1]);
            assert!(
                bb::GROUND_SUB_TILE_UVS.contains(&(u0, v0, u1, v1)),
                "quad UV block {:?} is not one of the four retail sub-tiles",
                (u0, v0, u1, v1)
            );
            // Corner order: v0 = (lo, lo), v1 = (hi, lo), v2 = (lo, hi).
            assert_eq!((q[1][0], q[1][1]), (u1, v0));
            assert_eq!((q[2][0], q[2][1]), (u0, v1));
        }
    }

    /// All four sub-tiles are actually used, and each exactly a quarter of the
    /// time - the deterministic `sub_row * 2 + sub_col` walk, not a hash.
    #[test]
    fn all_four_sub_tiles_appear_equally() {
        let m = build_battle_ground_grid();
        let mut hits = [0usize; 4];
        for q in m.uvs.chunks_exact(4) {
            let key = (q[0][0], q[0][1], q[3][0], q[3][1]);
            let i = bb::GROUND_SUB_TILE_UVS
                .iter()
                .position(|t| *t == key)
                .unwrap();
            hits[i] += 1;
        }
        let cells = (BATTLE_GROUND_CELLS * BATTLE_GROUND_CELLS) as usize;
        assert_eq!(hits, [cells; 4]);
    }

    /// The whole grid draws through the one pinned page/CLUT pair.
    #[test]
    fn every_quad_uses_the_pinned_page_and_clut() {
        let m = build_battle_ground_grid();
        assert!(
            m.cba_tsb
                .iter()
                .all(|c| *c == [BATTLE_GROUND_CBA, BATTLE_GROUND_TSB])
        );
    }
}
