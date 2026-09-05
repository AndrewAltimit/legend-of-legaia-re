//! Shared full-scene **assembly kernel**: load a CDNAME field/town scene
//! through the engine's real scene loaders and resolve everything a static
//! full-map view needs - the environment mesh pack, the `.MAP` placement /
//! terrain-tile draws, the walk-ground heightfield, and the field VRAM.
//!
//! Three hosts consume this one kernel (see `docs/tooling/host-drift.md` for
//! why it must stay one kernel): the browser field-scene page
//! (`web-viewer::field_scene` wraps [`AssembledScene`] with its render
//! caches), the browser play page's mesh builders, and the native
//! `legaia-engine export-glb` world exporter ([`crate::glb_export`]).
//!
//! The hybrid mesh builders here merge the two primitive families every env
//! pack mixes - VRAM-textured prims and untextured `F*`/`G*` vertex-colour
//! prims - into one vertex stream with the [`crate::packet_color`] side
//! channel, the same convention the WebGL shader and the `.glb` baker read.

use crate::coplanar_draws;
use crate::field_env::{self, EnvDraw};
use crate::packet_color;
use crate::scene::{ProtIndex, Scene};
use crate::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, ResolvedTmd, SceneLoadKind, SceneResources,
};
use std::collections::HashMap;

/// A fully-assembled static field scene: the engine scene resources plus the
/// resolved draw lists. Everything positions in the **retail field frame**
/// (PSX +Y down); the render-frame flip is the consumer's
/// ([`draw_translation`] applies the standard one).
pub struct AssembledScene {
    /// CDNAME label the scene was loaded as.
    pub name: String,
    /// Engine scene resources: field-mode VRAM + every parsed scene TMD.
    pub res: SceneResources,
    /// Environment-pack subset of `res.tmds` (pack-index order) - the index
    /// space the placement records select from.
    pub env_tmds: Vec<usize>,
    /// Placed-object draws (`flags & 0x4`; buildings / props / landmarks).
    /// For world-map scenes this is the whole sparse mesh layer: the walk
    /// `.MAP`'s placed landmarks followed by its decoration cells, matching
    /// the native play-window's `resolve_world_map_terrain_draws`.
    pub placements: Vec<EnvDraw>,
    /// Bulk terrain-tile draws (`CELL_VISIBLE`; ground / decor tiles).
    /// Empty for world-map scenes (their ground is the heightfield).
    pub terrain: Vec<EnvDraw>,
    /// Walk-ground heightfield surface (`None` when the scene has no
    /// resolvable `.MAP` floor grid / floor LUT).
    pub ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Cross-draw coplanar lifts ([`crate::coplanar_draws`]) for the combined
    /// terrain + placement lists, applied by [`Self::draw_world_position`] so
    /// overlapping same-plane tiles resolve deterministically instead of
    /// z-fighting (mirrors the native play-window).
    pub coplanar_offsets: HashMap<EnvDraw, [f32; 3]>,
}

impl AssembledScene {
    /// A draw's world position with its coplanar lift folded in - the retail
    /// field frame (PSX +Y down), the same numbers the browser position
    /// accessors export.
    pub fn draw_world_position(&self, d: &EnvDraw) -> [f32; 3] {
        let off = self.coplanar_offsets.get(d).copied().unwrap_or([0.0; 3]);
        [
            d.world_x as f32 + off[0],
            d.world_y as f32 + off[1],
            d.world_z as f32 + off[2],
        ]
    }
}

/// A draw's placement transform in the **render frame** the site pages and
/// the `.glb` baker share: world Y negated (the mesh-local `(1, -1, 1)` flip
/// leaves translations untouched, so objects must be re-signed onto their
/// floor tiles) and the authored yaw converted to radians with the same
/// negation `placementModelScaledY` applies.
pub fn draw_translation(pos: [f32; 3]) -> [f32; 3] {
    [pos[0], -pos[1], pos[2]]
}

/// The authored PSX yaw (`4096` = full revolution) in render-frame radians.
pub fn draw_rot_y_radians(rot_y: u16) -> f32 {
    -((rot_y & 0xFFF) as f32) * std::f32::consts::PI / 2048.0
}

/// Assemble a CDNAME scene's full static map: field-mode [`SceneResources`]
/// (VRAM + env TMD pack) + the `.MAP` placement / terrain-tile draws resolved
/// through [`field_env`] + the walk-ground heightfield.
pub fn assemble_field_scene(index: &ProtIndex, name: &str) -> Result<AssembledScene, String> {
    let scene = Scene::load(index, name).map_err(|e| format!("{e:#}"))?;

    // The shared blocks the retail field engine keeps resident across
    // scene transitions (player TMD + shared UI atlas) - included so the
    // VRAM matches the engine's field build; the env-pack vote filters
    // them out of the mesh selection.
    let mut shared_scenes: Vec<Scene> = Vec::new();
    for n in FIELD_SHARED_BLOCKS {
        if let Ok(s) = Scene::load(index, n) {
            shared_scenes.push(s);
        }
    }
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    let is_world_map = crate::scene::is_world_map_scene(name);
    let kind = if is_world_map {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    };
    // Boot-resident system-UI bundle (raw PROT TOC entries 0/1): layers
    // under the scene build so the row-510 strip CLUT + (960,256)
    // menu-glyph atlas the env meshes sample (town01 slots 21/26/74,
    // rikuroa slots 50/51/63) are resident here too.
    let system_ui = index.system_ui_bundle().ok();
    let (res, _stats) = SceneResources::build_targeted_with_options(
        &scene,
        &shared_refs,
        BuildOptions {
            kind,
            // Retail's field loader DMA-uploads every scene TIM; the
            // render-targeted subset drops ~75% of the env pack's prims.
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )
    .map_err(|e| format!("{e:#}"))?;

    let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
    let floor_lut = scene.field_floor_height_lut(index).ok().flatten();
    // World-map scenes draw two sparse walk-frame layers over the continent
    // heightfield; field/town scenes draw the placed objects + the bulk
    // terrain-tile layer (mirrors the play-window's resolve_field_* /
    // resolve_world_map_* split in `engine-shell`).
    let (placement_records, terrain_records) = if is_world_map {
        // The walk `.MAP` carries the placed-flag landmarks (`FUN_8003A55C`,
        // flags & 0x4) AND a **decoration** layer - walk-visible cells with a
        // nonzero record `+0x10` and no placed flag: the crossed-quad trees,
        // mountain groups and props (~300 cells on Drake alone). Both are one
        // un-posed layer, concatenated landmarks-then-decorations exactly as
        // `resolve_world_map_terrain_draws` in `engine-shell` does. The
        // continent ground itself is NOT this layer - it is the heightfield.
        let mut tiles = scene
            .walk_object_placements(index)
            .ok()
            .flatten()
            .unwrap_or_default();
        if let Ok(Some(deco)) = scene.walk_decoration_placements(index) {
            tiles.extend(deco);
        }
        (tiles, Vec::new())
    } else {
        (
            scene
                .field_object_placements(index)
                .ok()
                .flatten()
                .unwrap_or_default(),
            // Retail's per-cell terrain emitters (`FUN_801F69D8` /
            // `FUN_801F7088`) skip records carrying the *placed* flag - those
            // are the placement sweep's actors, drawn (and posed) above. The
            // two layers also resolve Y differently (corner average vs single
            // nibble), so a duplicate here would land at a different height.
            scene
                .field_terrain_tiles(index)
                .ok()
                .flatten()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
                .collect(),
        )
    };
    // Object binds resolve BEFORE the placement pass so each placed draw
    // carries its bind's `anim_id` (the clip that poses a multi-object prop -
    // windmill sails, doors; the native play-window's frame-0 static bake and
    // the glb exporter's animated-prop split both key on it). A bind-less
    // resolve leaves every draw at `anim_id` 0 and multi-object props draw as
    // raw object-local parts.
    let binds = if is_world_map {
        None
    } else {
        scene.field_object_binds(index).ok().flatten()
    };
    let (mut placements, _) = field_env::resolve_placed_env_draws(
        &env_tmds,
        &placement_records,
        floor_lut,
        binds.as_ref(),
    );
    // Story-hidden placed objects: the `.MAP` table says where an object CAN
    // stand, its bind record's spawn prologue says whether it currently DOES
    // (town01's flag-gated gate rocks; town0c's parked intact doorway). The
    // full-map assembly stages each scene at its canonical free-roam visit,
    // same as the play page's world does.
    if let Some(binds) = binds.as_ref() {
        let hidden = field_env::story_hidden_records_for_scene(&scene, index);
        field_env::retain_visible_placed_draws(&mut placements, binds, &hidden);
    }
    let (terrain, _) = field_env::resolve_env_draws(&env_tmds, &terrain_records, floor_lut);
    let ground = scene
        .walk_heightfield(index)
        .ok()
        .flatten()
        .filter(|h| !h.indices.is_empty());

    // Cross-draw coplanar lifts over the combined layers (terrain first,
    // then placements - the same concatenation the native shell ranks).
    let mut combined: Vec<EnvDraw> = Vec::with_capacity(terrain.len() + placements.len());
    combined.extend_from_slice(&terrain);
    combined.extend_from_slice(&placements);
    let planes = coplanar_draws::draw_plane_summaries(&combined, &res);
    let coplanar_offsets = coplanar_draws::coplanar_draw_offsets(&combined, &planes);

    Ok(AssembledScene {
        name: name.to_string(),
        res,
        env_tmds,
        placements,
        terrain,
        ground,
        coplanar_offsets,
    })
}

/// Build one env-pack mesh the way the native play-window renders it: the
/// VRAM-filtered **textured** prims plus the untextured `F*`/`G*`
/// **vertex-colour** prims (`legaia_tmd::mesh::tmd_to_color_mesh` - the props
/// whose prims carry per-vertex RGB instead of UVs, which the textured
/// builder drops; the engine-shell draws them on its colour-mesh pipeline).
/// Both halves are merged into one vertex stream, with a parallel
/// `[r, g, b, flag]` byte array (`flag` 255 = textured, sample VRAM and
/// modulate by the RGB; 0 = untextured, fill with the RGB) matching the
/// `u_use_flat_colors` / `a_flat_rgba` convention the field-character hybrid
/// shader path already implements. Every vertex carries a colour - the
/// textured half's is the prim's packet word, retail's whole field lighting
/// model (`texel * colour / 128`).
pub fn build_hybrid_env_mesh(
    rtmd: &ResolvedTmd,
    vram: &legaia_tim::Vram,
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    let mut mesh = rtmd.build_filtered_vram_mesh(vram);
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh(&rtmd.tmd, &rtmd.raw);
    // Coplanar z-fight resolution over BOTH halves as one stream - the same
    // shared kernel the native play-window runs (`legaia_tmd::mesh::coplanar`):
    // flag double-sided pairs for the shader's facing discard, nudge distinct
    // coplanar decal layers toward their visible side. The merge below carries
    // the colour half's pair flag onto the merged CBA attribute.
    legaia_tmd::mesh::resolve_hybrid(&mut mesh, &mut cmesh);
    merge_hybrid_halves(mesh, &cmesh)
}

/// [`build_hybrid_env_mesh`] **posed** at one set of per-object rigid
/// transforms (frame 0 of a placed prop's object-bind clip): both halves go
/// through the `*_posed_rot` builders - the same `R . v + T` bake the native
/// play-window's posed-placement pass runs - and merge into the same hybrid
/// stream. Used for the `.MAP` placed objects whose bind names an animation;
/// their TMD objects are that clip's bones, and the raw object-local vertices
/// are nonsense without its transform.
pub fn build_hybrid_env_mesh_posed(
    rtmd: &ResolvedTmd,
    offsets: &[([i16; 3], [i16; 3])],
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    let mut mesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, offsets);
    let mut cmesh = legaia_tmd::mesh::tmd_to_color_mesh_posed_rot(&rtmd.tmd, &rtmd.raw, offsets);
    legaia_tmd::mesh::resolve_hybrid(&mut mesh, &mut cmesh);
    merge_hybrid_halves(mesh, &cmesh)
}

/// The **kingdom slot-1 landmark pack** hybrid build - the world-map sibling
/// of [`build_hybrid_env_mesh`] for the surfaces that stamp one pack mesh per
/// walk `.MAP` cell (the world-overview page's `pack_mesh_*` accessors and its
/// continent `.glb` export; the native play-window keeps the two halves on
/// their own pipelines through `resolve_world_map_terrain_draws`).
///
/// Same merge, same `[r, g, b, flag]` side channel, two deliberate
/// differences from the field kernel:
///
/// - the textured half is the **unfiltered** [`legaia_tmd::mesh::tmd_to_vram_mesh`]
///   build: the kingdom TIM_LIST packs ~50 TIMs into VRAM rows 479..510, so
///   the VRAM-targeted filter's depth-mismatch heuristic drops most of the
///   pack's prims (the page has always drawn the unfiltered half);
/// - no coplanar pass: the overview runs no cross-draw `coplanar_draws`
///   ranking, and the textured half stays byte-identical to what the page
///   drew before the colour half existed.
///
/// The colour half is what was missing: the landmark meshes mix textured
/// walls with untextured `F*`/`G*` prims - Rim Elm's four hut roofs are 24
/// gouraud triangles (Drake slot 29), the Uru Mais temple is colour prims only
/// (Karisto slot 8: nothing to draw without this half) -
/// and retail draws them through the same per-prim dispatch as the walls (the
/// group header's `flags >> 1` selects the renderer; the untextured slots
/// 12..=15 are populated in both the SCUS table `0x8007657C` and the
/// world-map overlay's `0x801F8968` row - see `docs/subsystems/world-map.md`).
/// A textured-only build drew the huts as open rings.
pub fn build_hybrid_pack_mesh(
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(tmd, raw);
    let cmesh = legaia_tmd::mesh::tmd_to_color_mesh(tmd, raw);
    merge_hybrid_halves(mesh, &cmesh)
}

/// Merge the untextured vertex-colour half into the textured half's vertex
/// stream, producing the parallel `[r, g, b, flag]` array. Both halves carry a
/// colour: the untextured half's is its **fill**, the textured half's is its
/// **modulation** (`texel * colour / 128`), and the `flag` byte is which.
///
/// The colour half's per-vertex PSX blend word (ABE enable in bit 15 + ABR
/// mode in bits 5..=6, see [`legaia_tmd::mesh::ColorMesh::blend`]) is carried
/// into the TSB half of the merged `cba_tsb` attribute - the same packing the
/// textured prims ride ([`legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`]).
/// Dropping it forced every untextured semi-transparent prim (water sheets,
/// window light shafts) to draw opaque in the WebGL viewers. The flat-colour
/// shader path never samples VRAM for these verts, so the nonzero TSB is
/// blend metadata only.
fn merge_hybrid_halves(
    mut mesh: legaia_tmd::mesh::VramMesh,
    cmesh: &legaia_tmd::mesh::ColorMesh,
) -> (legaia_tmd::mesh::VramMesh, Vec<u8>) {
    // The textured half's stream is its packet colours (the shader's
    // `texel * colour / 128` modulation), NOT white - a white stream would
    // brighten every textured surface by 255/128.
    if cmesh.is_empty() {
        let flat = packet_color::textured(&mesh);
        return (mesh, flat);
    }
    let mut flat = packet_color::textured(&mesh);
    flat.reserve(cmesh.positions.len() * 4);
    let base = mesh.positions.len() as u32;
    for ((p, c), blend) in cmesh
        .positions
        .iter()
        .zip(cmesh.colors.iter())
        .zip(cmesh.blend.iter())
    {
        mesh.positions.push(*p);
        mesh.uvs.push([0, 0]);
        // A colour vert flagged as one copy of a double-sided pair
        // (`resolve_hybrid`, blend bit 14) re-keys the flag onto the merged
        // CBA attribute's bit 15 - where the WebGL shader's facing discard
        // reads it for every vertex, textured or flat.
        let cba = if blend & legaia_tmd::mesh::BLEND_DOUBLE_SIDED_BIT != 0 {
            legaia_tmd::mesh::CBA_DOUBLE_SIDED_BIT
        } else {
            0
        };
        mesh.cba_tsb
            .push([cba, blend & !legaia_tmd::mesh::BLEND_DOUBLE_SIDED_BIT]);
        mesh.normals.push([0.0, 0.0, 0.0]);
        // Keep `VramMesh::colors` index-aligned with the positions: the
        // untextured half has no modulation word, so it takes the neutral one.
        mesh.colors.push([packet_color::NEUTRAL; 3]);
        flat.extend_from_slice(&[c[0], c[1], c[2], 0]);
    }
    mesh.indices.extend(cmesh.indices.iter().map(|i| i + base));
    (mesh, flat)
}

/// The site's sky-backdrop heuristic (`FieldSceneView.isSkyMesh`), applied to
/// a built mesh's AABB + uploaded corner count: a paper-thin plane spanning
/// the whole map, or a sparse >3400-unit dome shell. `vert_count` is the
/// uploaded per-primitive-corner count (`positions.len()`), because
/// `legaia_tmd` emits one position per corner with no dedup - the 512 cutoff
/// is calibrated against that space (sky shells run 48..480 uploaded corners;
/// the densest real geometry clearing the AABB arm starts at 582, kor5's
/// whole tiled plaza floor at 1714).
pub fn is_sky_mesh(mesh: &legaia_tmd::mesh::VramMesh) -> bool {
    if mesh.positions.is_empty() {
        return false;
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    let (sx, sy, sz) = (max[0] - min[0], max[1] - min[1], max[2] - min[2]);
    let sparse = mesh.positions.len() <= 512;
    let flat_plane = sx.min(sz) < 8.0 && sx.max(sz) > 3000.0 && sy > 600.0;
    let dome_shell = sx > 3400.0 && sz > 3400.0 && sy > 800.0 && sparse;
    flat_plane || dome_shell
}

#[cfg(test)]
mod tests {
    use super::{build_hybrid_pack_mesh, merge_hybrid_halves};
    use legaia_tmd::mesh::{ColorMesh, TSB_SEMI_TRANSPARENT_BIT, VramMesh};

    /// One object mixing a textured `FT3` group (flags `0x20`, the same prim
    /// layout as `legaia_tmd`'s synthetic pyramid) with an untextured gouraud
    /// `G3` group (flags `0x1D`: three colour words, then the vertex
    /// indices) - the shape of every landmark pack mesh that carries a roof.
    fn synth_mixed_tmd() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x8000_0002u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        let prim_top: u32 = 28;
        // Two groups of one 20-byte prim, each followed by its footer slot,
        // then the u32 terminator.
        let prim_size: u32 = 2 * (8 + 2 * 20) + 4;
        let vert_top: u32 = prim_top + prim_size;
        buf.extend_from_slice(&vert_top.to_le_bytes());
        buf.extend_from_slice(&4u32.to_le_bytes()); // n_vert
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&prim_top.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes()); // n_primitive
        buf.extend_from_slice(&0i32.to_le_bytes());
        // Group A: FT3 (count=1 flags=0x20 olen=7 ilen=5 flag=1 mode=0x25),
        // vertex indices at byte 14 (raw index * 8): verts 0,1,2.
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0x0020u16.to_le_bytes());
        buf.extend_from_slice(&[7, 5, 1, 0x25]);
        let mut prim = vec![0u8; 20];
        for (i, v) in [0u16, 1, 2].iter().enumerate() {
            prim[14 + i * 2..16 + i * 2].copy_from_slice(&(v * 8).to_le_bytes());
        }
        buf.extend_from_slice(&prim);
        buf.extend_from_slice(&[0u8; 20]);
        // Group B: G3 (count=1 flags=0x1D olen=5 ilen=5 flag=0 mode=0x31):
        // three colour words then verts 0,1,3.
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0x001Du16.to_le_bytes());
        buf.extend_from_slice(&[5, 5, 0, 0x31]);
        let mut prim = vec![0u8; 20];
        prim[0..4].copy_from_slice(&[0x60, 0x40, 0x20, 0x34]);
        prim[4..8].copy_from_slice(&[0x61, 0x41, 0x21, 0x34]);
        prim[8..12].copy_from_slice(&[0x62, 0x42, 0x22, 0x34]);
        for (i, v) in [0u16, 1, 3].iter().enumerate() {
            prim[12 + i * 2..14 + i * 2].copy_from_slice(&(v * 8).to_le_bytes());
        }
        buf.extend_from_slice(&prim);
        buf.extend_from_slice(&[0u8; 20]);
        buf.extend_from_slice(&0u32.to_le_bytes());
        for (x, y, z) in [(0i16, 0i16, 0i16), (64, 0, 0), (0, 0, 64), (32, -80, 32)] {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&z.to_le_bytes());
            buf.extend_from_slice(&0i16.to_le_bytes());
        }
        buf
    }

    /// The pack kernel keeps BOTH prim families: the textured triangle comes
    /// through flagged 255 exactly as the plain textured builder emits it,
    /// and the untextured gouraud triangle - the roof - is appended flagged
    /// 0 with its own colour words, so a consumer of the merged stream draws
    /// the whole landmark. A textured-only build has three vertices here.
    #[test]
    fn pack_hybrid_keeps_the_untextured_roof_prims() {
        let buf = synth_mixed_tmd();
        let tmd = legaia_tmd::parse(&buf).expect("synthetic TMD parses");
        let textured_only = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &buf);
        assert_eq!(
            textured_only.positions.len(),
            3,
            "textured half is one triangle"
        );

        let (mesh, flat) = build_hybrid_pack_mesh(&tmd, &buf);
        assert_eq!(mesh.positions.len(), 6);
        assert_eq!(mesh.indices, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(flat.len(), 24);
        // Textured prefix: byte-identical to the plain build, flagged 255.
        assert_eq!(&mesh.positions[..3], &textured_only.positions[..]);
        assert_eq!(&mesh.cba_tsb[..3], &textured_only.cba_tsb[..]);
        for v in 0..3 {
            assert_eq!(flat[v * 4 + 3], 255, "textured vert {v} flag");
        }
        // Colour tail: the roof's apex vertex and its gouraud colours,
        // flagged 0, no VRAM address (the shader fills, never samples).
        assert_eq!(mesh.positions[5], [32.0, -80.0, 32.0]);
        assert_eq!(
            &flat[12..24],
            &[
                0x60, 0x40, 0x20, 0, //
                0x61, 0x41, 0x21, 0, //
                0x62, 0x42, 0x22, 0,
            ]
        );
        for v in 3..6 {
            assert_eq!(
                mesh.cba_tsb[v],
                [0, 0],
                "colour vert {v} carries no VRAM address"
            );
            assert_eq!(mesh.uvs[v], [0, 0]);
        }
    }

    /// The colour half's blend words must survive the hybrid merge in the
    /// TSB half of `cba_tsb` - an untextured ABE prim (fountain water /
    /// window light shaft) that loses its blend word draws as an opaque
    /// grey blob in the WebGL viewers.
    #[test]
    fn hybrid_merge_keeps_colour_half_blend_words() {
        let mesh = VramMesh {
            positions: vec![[0.0; 3]; 3],
            uvs: vec![[0, 0]; 3],
            cba_tsb: vec![[7, 0x1234]; 3],
            indices: vec![0, 1, 2],
            normals: vec![[0.0; 3]; 3],
            colors: vec![[128; 3]; 3],
        };
        let semi = TSB_SEMI_TRANSPARENT_BIT; // ABE on, ABR 0
        let cmesh = ColorMesh {
            positions: vec![[1.0; 3]; 3],
            colors: vec![[10, 20, 30]; 3],
            indices: vec![0, 1, 2],
            blend: vec![semi; 3],
        };
        let (merged, flat) = merge_hybrid_halves(mesh, &cmesh);
        assert_eq!(merged.positions.len(), 6);
        assert_eq!(flat.len(), 24);
        // Textured prefix untouched, colour tail flagged 0 with its blend
        // word in the TSB half.
        assert_eq!(merged.cba_tsb[0], [7, 0x1234]);
        for v in 3..6 {
            assert_eq!(merged.cba_tsb[v], [0, semi], "vert {v}");
            assert_eq!(flat[v * 4 + 3], 0, "vert {v} flag");
        }
    }
}
