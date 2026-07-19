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
pub(crate) const BATTLE_GROUND_TSB: u16 = 0x000D;
pub(crate) const BATTLE_GROUND_CBA: u16 = 0x77C0;

/// Build the flat tiled battle ground grid (retail's `func_0x801d02c0`
/// ground grid): a 28x28-cell field of `0x200`-pitch quads on the PSX `y=0`
/// plane centred at the world origin, textured from the constant
/// [`BATTLE_GROUND_TSB`] / [`BATTLE_GROUND_CBA`] page. The UV window is the
/// fixed `(192..255)^2` block of that page, which holds **four 32x32
/// sub-tiles** (two distinct variants, each duplicated across the row -
/// verified on the decoded town01 tile); each cell samples one sub-tile
/// picked by a per-cell deterministic hash standing in for retail's per-cell
/// `rand()` corner pick, with a random UV corner mirror on top (the GT4
/// packets' mirrored corner orders). Drawn with the battle camera so the
/// party stands on it.
pub(crate) fn build_battle_ground_grid() -> legaia_tmd::mesh::VramMesh {
    const N: i32 = 28; // cells per side (retail func_0x801d02c0 grid)
    const P: f32 = 512.0; // cell pitch (0x200) -> ~+/-7168 extent
    // The fixed sub-tile window inside the (192..255)^2 block.
    const UV_BASE: u16 = 192;
    const SUB: u16 = 32;
    let mut m = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    let half = N / 2;
    for iz in 0..N {
        for ix in 0..N {
            let (x0, z0) = ((ix - half) as f32 * P, (iz - half) as f32 * P);
            let (x1, z1) = (x0 + P, z0 + P);
            // Deterministic per-cell "random": quadrant pick (2 bits) +
            // mirror pick (2 bits). Retail rolls rand() per cell at grid
            // build; a coordinate hash keeps the engine build reproducible.
            let h = (ix as u32)
                .wrapping_mul(0x9E37_79B9)
                .wrapping_add((iz as u32).wrapping_mul(0x85EB_CA6B));
            let h = (h ^ (h >> 13)).wrapping_mul(0xC2B2_AE35);
            let (qu, qv) = ((h & 1) as u16, ((h >> 1) & 1) as u16);
            let (mir_h, mir_v) = (h & 4 != 0, h & 8 != 0);
            let (mut ua, mut ub) = (UV_BASE + qu * SUB, UV_BASE + qu * SUB + (SUB - 1));
            let (mut va, mut vb) = (UV_BASE + qv * SUB, UV_BASE + qv * SUB + (SUB - 1));
            if mir_h {
                std::mem::swap(&mut ua, &mut ub);
            }
            if mir_v {
                std::mem::swap(&mut va, &mut vb);
            }
            let (ua, ub, va, vb) = (ua as u8, ub as u8, va as u8, vb as u8);
            let base = m.positions.len() as u32;
            for (x, z, u, v) in [
                (x0, z0, ua, va),
                (x1, z0, ub, va),
                (x0, z1, ua, vb),
                (x1, z1, ub, vb),
            ] {
                m.positions.push([x, 0.0, z]);
                m.uvs.push([u, v]);
                m.cba_tsb.push([BATTLE_GROUND_CBA, BATTLE_GROUND_TSB]);
                m.normals.push([0.0, -1.0, 0.0]); // PSX up = -y (flat ground faces up)
                // Neutral modulation: the grid quads draw the raw tile
                // texel. (NB the old builder pushed NO colours at all, so
                // its upload failed the attribute-length check and the
                // grid never drew - the "sky-blue floor" was the bare
                // battle clear colour showing through.)
                m.colors
                    .push([legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]);
            }
            m.indices
                .extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    m
}
