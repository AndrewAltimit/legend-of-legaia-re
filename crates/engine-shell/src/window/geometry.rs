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

/// The four world-space corners of a camera-facing billboard, through the
/// shared corner kernel (`FxBillboard::corners`, the browser host's too).
/// Order: TL, TR, BL, BR. The half-extents were divided by the drawing
/// camera's view scale when the list was built
/// (`legaia_render_kernels::effect_billboard::world_half_extents`): retail
/// forms these corners in **view** space, after that scale.
fn effect_sprite_corners(
    b: &legaia_engine_world_views::effect_draws::FxBillboard,
    right: Vec3,
    up: Vec3,
) -> [Vec3; 4] {
    b.corners(right.to_array(), up.to_array()).map(Vec3::from)
}

/// Build a textured billboard mesh for the live effect sprites: one
/// camera-facing quad per child, sampling the scene VRAM at the sprite's
/// atlas `(u, v)` / `tpage` / `clut`. Mirrors the retail per-frame walker
/// (`FUN_801E0088` pass 2), which emits one GPU sprite primitive per child.
///
/// Texel residency: in battle the PROT 870 flame atlas + its CLUT rows are
/// blitted into the battle VRAM copy at battle entry
/// (`scene::upload_flame_atlas_into_vram`), and the field-resident
/// `player_data` §2 pages land at scene entry - the two effect-texel pools
/// `docs/subsystems/effect-vm.md` pins. A sprite whose atlas rect falls
/// outside the uploaded pools samples all-zero texels, which the VRAM-mesh
/// shader discards (clean, not garbage). Returns `None` when there is
/// nothing to draw.
///
/// # Every effect child is semi-transparent, and the TSB word has to say so
///
/// Retail's per-child GPU packet is prim code **`0x2E`** - a flat textured
/// quad with the semi-transparency bit set - so the ABE enable is
/// unconditional on this path, not a property of the atlas entry. The port's
/// blend model carries that enable in bit 15 of the TSB vertex attribute
/// ([`legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`]); every TMD path sets it
/// through `pack_tsb_semi`, and this builder used to push the raw atlas page
/// byte, which tops out at `0xFF` and can never reach bit 15.
///
/// The consequence was not a subtle one. `psx_blend::append_semi_tail` saw
/// no semi-transparent primitive, so no billboard triangle ever entered the
/// blend pass, and the shader's STP deferral never fired - so every texel
/// drew fully opaque. The flame atlas's CLUT row 474 is a fire ramp whose hot
/// end (`0xC73F` = `(248, 200, 136)`) is a pale tan: under retail's additive
/// `B + F` those texels are a glow over the dark arena, and drawn opaque they
/// are solid tan blobs sitting on the floor. Index `0` is `0x0000` and still
/// discards, which is why the blobs kept a puff-shaped silhouette inside the
/// `LEGAIA_DIAG_FX` outline rather than filling it.
pub(crate) fn effect_billboard_mesh(
    r: &legaia_engine_render::Renderer,
    sprites: &[legaia_engine_world_views::effect_draws::FxBillboard],
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
    for b in sprites {
        // The texel corners carry the child's random UV mirror (retail
        // pass 2), and the TSB carries the prim-ABE enable.
        let corners = effect_sprite_corners(b, right, up);
        let base = positions.len() as u32;
        for (corner, uv) in corners.iter().zip(b.uv) {
            positions.push(corner.to_array());
            uvs.push(uv);
            cba_tsb.push(b.cba_tsb);
            normals.push(face);
            colors.push([b.sprite.brightness; 3]);
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
/// age).
///
/// **Diagnostic only** (`LEGAIA_DIAG_FX=1`), because retail draws no such
/// rectangle. It makes a spawn readable when the textured quad samples texels
/// that are not resident - the billboard's geometry and animation are faithful
/// even when its pixels are not. In a battle the flame atlas + CLUT rows are
/// blitted at battle entry, so the quad itself draws and the outline is only
/// in the way.
pub(crate) fn effect_sprite_line_geometry(
    sprites: &[legaia_engine_world_views::effect_draws::FxBillboard],
    right: Vec3,
    up: Vec3,
) -> (Vec<[f32; 3]>, Vec<[u8; 4]>, Vec<u32>) {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(sprites.len() * 4);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(sprites.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(sprites.len() * 8);
    for b in sprites {
        let [tl, tr, bl, br] = effect_sprite_corners(b, right, up);
        // Warm spark colour, dimmed as the effect ages toward retirement.
        let fade = (1.0 - b.sprite.age01).clamp(0.0, 1.0);
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

/// Convert a [`WalkHeightfield`] into a renderer [`VramMesh`]. The heightfield
/// supplies per-vertex UVs (the `+0x14` atlas tile) **and** per-vertex
/// `[clut, tpage]` (the cell's terrain page + palette from `+0x15` /
/// `+0x16..+0x18`), so grass / mountain / water / forest cells each sample their
/// own VRAM page within the single ground mesh. Normals are left at the
/// `[0,0,0]` sentinel so the shader derives screen-space normals (flat-lit).
/// See docs/subsystems/world-map.md "Ground texturing".
pub(crate) fn heightfield_to_vram_mesh(
    hf: &legaia_scene_formats::field_objects::WalkHeightfield,
) -> legaia_tmd::mesh::VramMesh {
    let n = hf.positions.len();
    // Winding reversed onto the scene TMDs' parity + the GROUND_SINK: the
    // shared render kernel the browser play page's ground exports also run
    // (`legaia_engine_scene::field_ground`).
    let indices = legaia_engine_scene::field_ground::render_indices(hf);
    let positions = legaia_engine_scene::field_ground::render_positions(hf);
    legaia_tmd::mesh::VramMesh {
        positions,
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

/// Build a LineList plot of a kingdom's slot-4 **animation bank**
/// (`SceneResources::world_map_slot4`), as world-space `(positions, colors,
/// indices)`.
///
/// Slot 4 is not a mesh library: it is an asset-type-`0x05` ANM container of
/// world-map actor clips, and each 8-byte entry is a rigid transform - three
/// packed 12-bit translations plus three 8-bit angles
/// ([`world-map-overlay.md`](../../../../../../docs/formats/world-map-overlay.md)).
/// So what this draws is one polyline per (clip, part) through that part's
/// decoded translation across the clip's frames -
/// `legaia_scene_formats::world_map_overlay::translation_path_segments`, the only
/// geometric reading actually in the bytes. It used to call
/// `wireframe_segments_3d`, which plots raw `i16` slices straddling the
/// entry's nibble boundaries: a byte-diffing aid, not geometry.
///
/// The values are object-local model-space offsets, so a path is a motion
/// curve about the actor origin, not a world position; they are emitted at
/// raw retail Y-down coordinates (the world-map cameras compose the single
/// world negation). Colour is keyed by the body's sub-frame divisor
/// (`Segment3d::kind` = the header `+0x06` rate, `1` / `2` / `4`) so clips
/// that step at different cadences read apart. Returns empty geometry when no
/// clip has a moving part.
///
/// This is an env-gated inspection overlay (`LEGAIA_WORLDMAP_SLOT4=1`); it is
/// not a render path.
pub(crate) fn world_map_slot4_line_geometry(
    slot: &legaia_scene_formats::world_map_overlay::KingdomSlot4,
) -> LineGeometry {
    let segs = legaia_scene_formats::world_map_overlay::translation_path_segments(slot);
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(segs.len() * 2);
    let mut col: Vec<[u8; 4]> = Vec::with_capacity(segs.len() * 2);
    let mut idx: Vec<u32> = Vec::with_capacity(segs.len() * 2);
    for s in &segs {
        let c = match s.kind {
            1 => [120u8, 200, 255, 255], // every-frame clips (cyan)
            2 => [255u8, 160, 90, 255],  // half-rate clips (orange)
            4 => [200u8, 120, 255, 255], // quarter-rate clips (violet)
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

/// The 3D pass's viewport and projection aspect for a `surface_w` x
/// `surface_h` window: the 2D stage rect (`pause_menu::stage_transform`, the
/// integer-scaled, centred 320x240 stage every text and sprite draw uses) at
/// the stage's 4:3 aspect, so 3D that has to line up with the stage - the
/// naming screen's actor inside its windows, the field party HUD's
/// projected-player row - does at any window size. The browser play page's
/// canvas *is* a stage, so this is the one frame both hosts now draw 3D into.
///
/// `None` (whole surface, surface aspect) only when the window is smaller
/// than one stage and the rect would not fit.
pub(crate) fn scene_viewport_for(
    surface_w: u32,
    surface_h: u32,
) -> (Option<(u32, u32, u32, u32)>, f32) {
    let ((x0, y0), scale) = legaia_engine_ui::pause_menu::stage_transform(surface_w, surface_h);
    let (w, h) = (
        legaia_engine_ui::BOOT_UI_STAGE_W * scale,
        legaia_engine_ui::BOOT_UI_STAGE_H * scale,
    );
    if x0 < 0 || y0 < 0 || x0 as u32 + w > surface_w || y0 as u32 + h > surface_h {
        return (None, surface_w as f32 / surface_h.max(1) as f32);
    }
    (Some((x0 as u32, y0 as u32, w, h)), w as f32 / h as f32)
}

#[cfg(test)]
mod effect_billboard_tests {
    use legaia_engine_core::world::effect_sprite_tsb;
    use legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT;

    /// Retail's per-child packet is prim code `0x2E` - textured **and**
    /// semi-transparent - so every billboard corner has to carry the port's
    /// prim-ABE enable, whatever the atlas entry's page byte is.
    ///
    /// Pushing the page verbatim (which is what this used to do) can never
    /// set bit 15, because the atlas stores the page in a single byte. The
    /// blend pass then admits no billboard triangle at all and the shader's
    /// STP deferral never fires, so every flame / dust texel rasterises
    /// opaque: the CLUT-474 fire ramp's hot end is a pale tan, which is
    /// exactly the opaque tan domes that appeared on the battle floor.
    #[test]
    fn every_effect_billboard_corner_is_semi_transparent() {
        // The two pages the `efect.dat` inline atlas actually names, plus the
        // extremes of the byte the atlas can hold.
        for page in [0x25u16, 0x66, 0x00, 0xFF] {
            let tsb = effect_sprite_tsb(page);
            assert_ne!(
                tsb & TSB_SEMI_TRANSPARENT_BIT,
                0,
                "page {page:#04x} must keep the prim-ABE enable"
            );
            assert_eq!(
                tsb & !TSB_SEMI_TRANSPARENT_BIT,
                page,
                "page {page:#04x} must survive the packing unchanged"
            );
        }
    }
}
