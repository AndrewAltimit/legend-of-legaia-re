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

/// World-unit size of one texel when drawing an effect billboard (the atlas
/// stores sprite extents in texels; the renderer scales them to world units).
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
    for s in sprites {
        let [u0, v0] = s.uv;
        let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1)).min(255) as u8;
        let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1)).min(255) as u8;
        let u0 = (u0 & 0xFF) as u8;
        let v0 = (v0 & 0xFF) as u8;
        let corners = effect_sprite_corners(s, right, up);
        let corner_uv = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]];
        let base = positions.len() as u32;
        for (corner, uv) in corners.iter().zip(corner_uv) {
            positions.push(corner.to_array());
            uvs.push(uv);
            cba_tsb.push([s.clut, s.page]);
            normals.push(face);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    match r.upload_vram_mesh(&positions, &uvs, &cba_tsb, &normals, &indices) {
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
    legaia_tmd::mesh::VramMesh {
        positions: hf.positions.clone(),
        uvs: hf.uvs.clone(),
        // Per-cell terrain page + palette (multi-page terrain atlas).
        cba_tsb: hf.cba_tsb.clone(),
        normals: vec![[0.0, 0.0, 0.0]; n],
        indices: hf.indices.clone(),
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

/// Build the flat tiled battle ground grid (retail's `func_0x801d02c0` grass
/// grid): a `(N+1)x(N+1)` vertex grid of quads on the PSX `y=0` plane centred at
/// the world origin, every vertex sampling the stage dome's **grass texel** so
/// it reads as real grass from the battle VRAM instead of the bare clear colour.
/// Returns `None` if the dome has no ground-plane (`|y|` small) textured vertex
/// to borrow the texel from. Drawn with the actor camera so the party stands on
/// it; coarse cells are fine because every vertex samples the same texel.
pub(crate) fn build_battle_ground_grid(
    dome: &legaia_tmd::mesh::VramMesh,
) -> Option<legaia_tmd::mesh::VramMesh> {
    // Borrow the dome's GRASS texture, targeting the exact tile retail's grid
    // (func_0x801d02c0) uses. `mednafen-state prim-trace` on the real map01
    // battle shows those ground tiles at uv ~ (132..140, 2..13) with their own
    // CBA/TSB. PROT 88 is the same TMD, so the dome's grass vertices carry the
    // same UVs - find the flat ground vertex nearest that tile centre, take its
    // CBA/TSB, and tile that small window. (Earlier picks - "first textured
    // vertex", "largest XZ area + bbox centre" - landed on the ground-mist
    // object or a 2-tone checker region of the texture, hence the checkerboard.)
    const GU0: u8 = 132;
    const GU1: u8 = 140;
    const GV0: u8 = 2;
    const GV1: u8 = 13;
    let tcu = ((GU0 as u16 + GU1 as u16) / 2) as i32;
    let tcv = ((GV0 as u16 + GV1 as u16) / 2) as i32;
    let best = (0..dome.positions.len())
        .filter(|&i| dome.positions[i][1].abs() < 5.0 && dome.cba_tsb[i] != [0, 0])
        .min_by_key(|&i| {
            let [u, v] = dome.uvs[i];
            (u as i32 - tcu).pow(2) + (v as i32 - tcv).pow(2)
        })?;
    let cba_tsb = dome.cba_tsb[best];
    let [bu, bv] = dome.uvs[best];
    log::info!(
        "battle ground grid: grass tile uv [{GU0}..{GU1}]x[{GV0}..{GV1}] cba_tsb={cba_tsb:?} (nearest dome vert uv=({bu},{bv}))"
    );
    let (u0, u1, v0, v1) = (GU0, GU1, GV0, GV1);

    const N: i32 = 28; // cells per side (retail func_0x801d02c0 grid)
    const P: f32 = 512.0; // retail func_0x801d02c0 cell pitch (0x200) -> ~+/-16384 extent
    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
    };
    // Per-cell quads (own 4 vertices each) so EVERY cell maps to the same full
    // grass UV tile `[u0..u1]x[v0..v1]`. Shared-vertex grids forced a single UV
    // per vertex, which (alternating box corners by parity) made adjacent cells
    // sample different texture columns -> green-vs-dirt whole-cell jumps. With
    // each cell carrying the whole tile, the grass repeats uniformly.
    let half = N / 2;
    for iz in 0..N {
        for ix in 0..N {
            let (x0, z0) = ((ix - half) as f32 * P, (iz - half) as f32 * P);
            let (x1, z1) = (x0 + P, z0 + P);
            let base = m.positions.len() as u32;
            for (x, z, u, v) in [
                (x0, z0, u0, v0),
                (x1, z0, u1, v0),
                (x0, z1, u0, v1),
                (x1, z1, u1, v1),
            ] {
                m.positions.push([x, 0.0, z]);
                m.uvs.push([u, v]);
                m.cba_tsb.push(cba_tsb);
                m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y (flat ground faces up)
            }
            m.indices
                .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    Some(m)
}
