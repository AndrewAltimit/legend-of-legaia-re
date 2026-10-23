//! WebAssembly bindings for browsing a Legend of Legaia disc image in the browser.
//!
//! Auto-detects: full Mode2/2352 .bin disc, raw PROT.DAT, or a single TIM.
//! After loading a disc, classifies every PROT entry via `legaia_disc_audit::categorize`
//! and pre-scans them for embedded TIMs so the UI shows a filtered, browsable
//! list of viewable entries instead of every raw entry.

pub(crate) mod art_clip_bank;
pub mod arts_view;
pub mod audio;
mod audio_api;
mod catalog;
mod character;
pub mod equipment_view;
pub mod field_npc;
pub mod field_scene;
mod inspect;
mod monster;
mod nav_disc;
pub mod npc_dialog;
mod packet_color;
pub mod player_anm;
mod prot_locate;
mod scene_export;
mod scene_geom;
pub mod sentinel_placements;
pub mod sfx_view;
pub mod shop_items;
pub mod shop_vendors;
pub mod summon_view;
pub mod tmd3d;
mod viewer_render;
mod world_mist;

// The ROM patcher's exports live in `legaia-web-patcher`. Nothing in this
// crate names one of its items, and rustc links only the crates a build
// references, so the bundle names it here: without this line its
// `#[wasm_bindgen]` exports would drop out of the cdylib.
extern crate legaia_web_patcher;

use legaia_container_formats::tim_scan;
use legaia_disc_audit::categorize::{Class, classify};
use legaia_disc_audit::tim_catalog;
use legaia_disc_audit::tim_deep_catalog;
use legaia_iso::image::{extract_prot_dat, extract_scus};
use legaia_prot::toc_index::{EntryMeta, parse_prot_toc};
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// Log a line to the browser console.
///
/// Natively this writes to stderr instead: `web_sys::console` is a
/// wasm-bindgen import and calling it off-wasm aborts the process, which is
/// what kept the whole of [`LegaiaViewer`] out of reach of the native
/// disc-gated tests (they had to re-implement each accessor against the
/// crate APIs instead of driving the page's real surface).
fn console_log(s: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&JsValue::from_str(s));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{s}");
}

/// Render a catalog TIM's curated label as a JSON value for the info panel: a
/// quoted label string, or `null` when the fingerprint isn't curated yet.
fn json_label(label: Option<&str>) -> String {
    match label {
        Some(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        None => "null".to_string(),
    }
}

/// Parse a 2-char axis pair string like `"xz"` / `"xy"` / `"zy"` into the
/// (horizontal, vertical) [`Axis`] pair the slot-4 emitter expects.
/// Unknown / malformed strings fall back to `(X, Z)` (historical top-down).
fn parse_axes(
    s: &str,
) -> (
    legaia_scene_formats::world_map_overlay::Axis,
    legaia_scene_formats::world_map_overlay::Axis,
) {
    use legaia_scene_formats::world_map_overlay::Axis;
    let pick = |c: char| -> Axis {
        match c {
            'x' | 'X' => Axis::X,
            'y' | 'Y' => Axis::Y,
            'z' | 'Z' => Axis::Z,
            _ => Axis::X,
        }
    };
    let mut it = s.chars();
    let a = it.next().map(pick).unwrap_or(Axis::X);
    let b = it.next().map(pick).unwrap_or(Axis::Z);
    (a, b)
}

/// Project a decoded slot-4 translation `[tx, ty, tz]` onto one [`Axis`].
///
/// [`legaia_scene_formats::world_map_overlay::Axis::pick`] selects out of the raw
/// four-`i16` byte view of an entry, which is not what the runtime reads; the
/// slot-4 emitters project the decoded transform instead, so they need this.
///
/// [`Axis`]: legaia_scene_formats::world_map_overlay::Axis
fn axis_of(t: [i16; 3], axis: legaia_scene_formats::world_map_overlay::Axis) -> i16 {
    use legaia_scene_formats::world_map_overlay::Axis;
    match axis {
        Axis::X => t[0],
        Axis::Y => t[1],
        Axis::Z => t[2],
    }
}

/// One entry's metadata + its first viewable TIM hit (if any).
#[derive(Clone)]
struct ViewerEntry {
    meta: EntryMeta,
    class: Class,
    first_tim: Option<TimHit>,
    /// Total number of TIM hits found by tim_scan (for the status line).
    tim_count: usize,
    /// Where the entry's leading TMD lives (if any). Used by the 3D viewer
    /// path. None ⇒ no TMD; render the TIM instead (or a "no TMD" message).
    tmd_source: Option<TmdSource>,
    /// Total Legaia TMDs found across the entry's LZS sections (the
    /// scene_asset_table environment-geometry mesh pack). `tmd_source` renders
    /// the first; this surfaces how many more the entry carries.
    tmd_pack_count: usize,
}

#[derive(Clone, Copy)]
enum TmdSource {
    /// Bare TMD at offset 0 of the entry.
    Direct { offset: usize },
    /// scene_tmd_stream wrapper: 4-byte chunk0 header + bare TMD.
    ///
    /// These are battle-stage backdrop shells, and every one is authored as
    /// **half** a bowl that retail closes by drawing the same mesh a second
    /// time under a per-stage transform (`legaia_scene_formats::battle_backdrop`).
    /// This variant is therefore both the one `LegaiaViewer::tmd_note`
    /// labels and the one whose mesh gets placed rather than drawn raw.
    SceneTmdStream { offset: usize, len: usize },
    /// TMD packed inside one of the entry's LZS-decompressed sections.
    /// Field/town scene_asset_table entries store their environment-geometry
    /// mesh pack this way (`town01` = entry 4, 121 meshes). `offset`/`len` are
    /// within `tmd_scan::scan_entry(buf).lzs_sections[section]`.
    Lzs {
        section: usize,
        offset: usize,
        len: usize,
    },
}

#[derive(Clone)]
struct TimHit {
    /// Source for the bytes: Raw is offset within the entry; Lzs(i, off) is
    /// section index + offset within that decompressed section.
    source: TimSource,
    width: u32,
    height: u32,
    bpp: u32,
}

#[derive(Clone)]
enum TimSource {
    Raw(usize),
    Lzs { section: usize, offset: usize },
}

use legaia_scene_formats::ocean::{OceanAssets, find_ocean_assets};

/// Loaded kingdom bundle (slot 0 = TIM_LIST -> VRAM, slot 1 = TMD pack ->
/// per-slot TMD bodies). Built by [`LegaiaViewer::set_scene_kingdom`] and
/// consumed by the assembled top-view world-map render path.
struct KingdomPack {
    /// PROT entry index this pack was loaded from (for status / dedup).
    prot_index: u32,
    /// 1 MB software PSX VRAM filled by uploading every TIM that decodes
    /// out of slot 0's LZS payload.
    vram: legaia_tim::Vram,
    /// Decompressed slot-1 payload: `[u32 count][u32 word_offsets[count]][TMDs]`.
    pack: Vec<u8>,
    /// Per-slot byte offset within `pack` (= `word_offsets[k] * 4`, the
    /// runtime pointer math from `FUN_8001F05C case 2`).
    byte_offsets: Vec<usize>,
    /// Per-slot body end (= next slot's start, or `pack.len()` for the last).
    byte_ends: Vec<usize>,
    /// Currently selected pack slot for the mesh accessors.
    cur_slot: Option<usize>,
    /// Ocean tile texture + base CLUT + animation table, extracted from
    /// slot 0's TIM_LIST. `None` when the kingdom is not a world-map
    /// kingdom (the assets are only present in PROT 0085 / 0244 / 0391).
    ocean: Option<OceanAssets>,
    /// The kingdom's slot-5 **CLUT-walk** shimmer (ocean head + the seven
    /// shoreline / river cells), installed into [`Self::vram`] by the engine
    /// kernel every play host runs
    /// (`legaia_engine_scene::clut_walk_anim::ClutWalkAnim::install`) and
    /// stepped by [`LegaiaViewer::kingdom_clut_tick`]. `None` until
    /// `set_scene_kingdom` resolves it (raw PROT.DAT loads have no CDNAME
    /// block to resolve against).
    clut_anim: Option<legaia_web_play::field_scene_anim::FieldSceneAnim>,
}

#[wasm_bindgen]
pub struct LegaiaViewer {
    /// Canvas DOM id. We re-resolve the actual `HtmlCanvasElement` and its
    /// 2D context on every render call: the JS UI swaps in a fresh canvas
    /// when transitioning between 2D and 3D modes (a HTMLCanvasElement
    /// can only ever hold one rendering-context type for its lifetime),
    /// and any cached references would still point at the *old*, detached
    /// element after the swap. The fallout was "2D entries don't render
    /// after viewing a 3D entry" - the put_image_data call landed on the
    /// orphan canvas and never touched the visible DOM node.
    canvas_id: String,
    disc: Vec<u8>,
    /// Filtered list of entries visible in the UI. Order matches PROT order.
    viewable: Vec<ViewerEntry>,
    current: usize,
    /// CLUT index to use when rendering paletted TIMs.
    clut_idx: usize,
    /// Currently-loaded kingdom bundle (Drake/Sebucus/Karisto). Populated by
    /// `set_scene_kingdom`; consumed by the `pack_mesh_*` accessors.
    kingdom: Option<KingdomPack>,
    /// Bulk continent terrain pack for the same kingdom. Each kingdom's PROT
    /// bundle has a SECOND 7-asset table at offset +8/+9 holding the world
    /// terrain TMDs (Drake +8 = 70 TMDs, Sebucus +9 = 43 TMDs). Loaded
    /// alongside the landmark pack by `set_scene_kingdom` when present;
    /// consumed by the `continent_pack_*` accessors.
    continent: Option<KingdomPack>,
    /// Item-name table (`PTR_DAT_8007436C`) decoded from `SCUS_942.54` at
    /// load time. Resolves a monster record's raw `drop_item` id into a
    /// readable name for the enemy table. `None` on raw PROT.DAT loads (no
    /// SCUS) - the page then falls back to the raw id.
    item_names: Option<legaia_game_tables::item_names::ItemNameTable>,
    /// Spell-name table (`DAT_800754C8` / `DAT_800754D0`) decoded from
    /// `SCUS_942.54` at load time. Resolves a monster record's global
    /// magic-attack ids into the on-screen spell names (`0x27` -> `Tail Fire`).
    /// `None` on raw PROT.DAT loads.
    spell_names: Option<legaia_game_tables::spell_names::SpellNameTable>,
    /// Per-monster steal table (`DAT_80077828`) decoded from `SCUS_942.54` at
    /// load time. Resolves what the Evil God Icon steals from each monster (item
    /// + chance) for the enemy table. `None` on raw PROT.DAT loads (no SCUS).
    steal_table: Option<legaia_game_tables::steal_table::StealTable>,
    /// Per-stage battle-backdrop mirror table (`DAT_80078B50`) decoded from
    /// `SCUS_942.54` at load time. Selects which of the two transforms a
    /// `scene_tmd_stream` backdrop's **second** copy gets. `None` on raw
    /// PROT.DAT loads - the preview then uses retail's default (half turn)
    /// and says the transform is unresolved.
    backdrop_mirror: Option<legaia_scene_formats::battle_backdrop::MirrorXTable>,
    /// Flat catalog of every standard PSX TIM in the loaded PROT.DAT image,
    /// built at load time from the TOC (see [`tim_catalog`]). Drives the
    /// "TIM Catalog" browse mode: page through every TIM by id with its CLUT
    /// variants, regardless of which PROT entry (or the unindexed gap) hosts
    /// it. Empty on the single-TIM load path.
    tim_catalog: Vec<tim_catalog::CatalogTim>,
    /// Deep catalog: standard PSX TIMs recovered from inside LZS-compressed
    /// PROT sections (the compressed character / scene textures the flat
    /// catalog can't see). Built at load time. Drives the "compressed
    /// textures" grid, a tier separate from the raw catalog above.
    tim_deep_catalog: Vec<tim_deep_catalog::DeepCatalogTim>,
    /// One-entry decompression cache for rendering deep-catalog thumbnails.
    /// The deep catalog is grouped by entry, so caching the last entry's
    /// decompressed sections lets a run of same-entry thumbnails reuse one
    /// decode instead of re-decompressing per thumbnail.
    deep_section_cache: std::cell::RefCell<Option<(u32, Vec<Vec<u8>>)>>,
    /// VRAM after the boot-resident system-UI bundle upload (retail
    /// `FUN_800198E0`'s flat-strip CLUT law), built at load time. Lets the
    /// catalog say when a TIM's own palette never reaches VRAM and offer the
    /// palettes VRAM really holds on its row. `None` when the bundle does not
    /// parse (single-TIM loads, foreign PROT.DAT).
    boot_cluts: Option<legaia_ui_formats::tim_palette_context::BootClutVram>,
    /// Per-rectangle palette map of the system-UI texture page, from the SCUS
    /// widget-class table (full-disc loads only). Drives the catalog's
    /// "as the game draws it" composite.
    sheet_regions: Option<Vec<legaia_ui_formats::tim_palette_context::SheetRegion>>,
    /// Walk-view continent ground for the currently-loaded kingdom: the
    /// procedural heightfield surface built from the walk `.MAP` floor grid,
    /// per-cell-textured from the terrain-type-keyed multi-page atlas. Built by
    /// [`LegaiaViewer::set_scene_kingdom`] alongside the landmark pack; consumed
    /// by the `walk_ground_*` accessors so the world-overview viewer draws the
    /// same continent terrain the native engine does. `None` until a kingdom is
    /// loaded, or when the walk `.MAP` / floor LUT can't be resolved.
    walk_ground: Option<legaia_scene_formats::field_objects::WalkHeightfield>,
    /// Walk-frame placed landmarks for the currently-loaded kingdom: the
    /// `flags & 0x4` slot-1 pack objects (`FUN_8003A55C`), resolved into the
    /// same `col*128` world frame as [`LegaiaViewer::walk_ground`]. Built by
    /// [`LegaiaViewer::set_scene_kingdom`]; consumed by the `walk_placement_*`
    /// accessors so the viewer draws the landmark meshes on top of the
    /// continent terrain instead of the misaligned overview-frame JSON layer.
    walk_placements: Option<Vec<WalkPlacement>>,
    /// World-map labels of the currently-loaded kingdom (the location table
    /// trailing the kingdom MAN, filtered to the kingdom's region). Built by
    /// [`LegaiaViewer::set_scene_kingdom`]; consumed by
    /// `world_map_labels_json` for the page's top-down map.
    world_map_labels: Option<Vec<WorldMapLabel>>,
    /// Assembled full field/town scene (env mesh pack + `.MAP` placement /
    /// terrain draws + walk-ground heightfield), loaded through the engine's
    /// real scene loaders. Populated by [`LegaiaViewer::set_scene_field`];
    /// consumed by the `field_scene_*` accessors.
    field_scene: Option<field_scene::FieldScenePack>,
    /// NPC catalog for the loaded field scene (the MAN's partition-1 actor
    /// placements, resolved against `field_scene`'s TMD pool). Populated by
    /// [`LegaiaViewer::set_scene_npcs`]; consumed by the `field_npc_*`
    /// accessors.
    field_npcs: Option<legaia_web_play::field_npc::FieldNpcPack>,
    /// Cached engine-core PROT index over the loaded disc (built on the
    /// first `set_scene_field`; cleared on `load_disc`).
    prot_index: Option<std::sync::Arc<legaia_engine_scene::scene::ProtIndex>>,
    /// CDNAME.TXT contents captured at `load_disc` time (the stored `disc`
    /// buffer only retains PROT.DAT, but the full-scene assembler needs the
    /// scene-name -> block map). `None` on raw PROT.DAT / single-TIM loads.
    cdname_text: Option<String>,
    /// In-progress scene `.glb` export session (see [`scene_export`]). The
    /// JS pages feed the exact mesh buffers + per-draw transforms they
    /// render, then `scene_export_finish` bakes the textured glTF. `None`
    /// when no export is being assembled.
    scene_export: Option<scene_export::SceneExportState>,
    /// Equipment stat-bonus table (`DAT_80074F68`) decoded from
    /// `SCUS_942.54` at load time. The equipment viewer reads each record's
    /// slot byte to label a player file's five sections, which are **not** in
    /// the same order for every character. `None` on raw PROT.DAT loads.
    equip_stats: Option<legaia_game_tables::equip_stats::EquipStatTable>,
    /// Every named item's stat record as JSON ([`shop_items`]), joined from
    /// the SCUS item / equipment / effect / passive tables at load time for
    /// the shops page. `None` on raw PROT.DAT loads.
    shop_item_records: Option<String>,
    /// The battle model the equipment viewer currently shows: one character
    /// assembled at one loadout, with its band VRAM, clip bank and diff
    /// against the unequipped default. See [`equipment_view`].
    equipped: Option<equipment_view::EquippedCharacter>,
    /// One item **card** of the equipment panel: a single-item loadout built
    /// for its thumbnail / metadata / download, cached under its
    /// `(character, section, id)` so the three calls share one assembly.
    item_card: Option<equipment_view::ItemCard>,
    /// Each character's Tactical Arts as clips, decoded once per disc load
    /// and re-expanded per loadout (see [`art_clip_bank`]).
    art_clips: std::collections::HashMap<usize, Vec<art_clip_bank::ArtClip>>,
}

/// VRAM rectangles a single primitive's CBA / TSB lookup will touch.
/// Used by the targeted VRAM upload to skip TIMs that have no bearing on
/// the current entry's mesh.
#[derive(Clone, Copy)]
struct PrimTarget {
    clut: (u16, u16, u16, u16),
    page: (u16, u16, u16, u16),
}

/// For a given TIM and the mesh's target rectangles, decide
/// independently whether to upload its image and CLUT blocks. Returns
/// `(upload_image, upload_clut)`. A block is uploaded iff it's useful
/// (overlaps a same-kind target) AND doesn't clobber a different-kind
/// target. The "doesn't clobber" half is what kills the rainbow-noise
/// symptom that comes from a TIM's image bytes landing on a VRAM row
/// another primitive uses as its CLUT.
fn tim_block_targeting(tim: &legaia_tim::Tim, needs: &[PrimTarget]) -> (bool, bool) {
    let img = &tim.image;
    let img_rect = (img.fb_x, img.fb_y, img.fb_w, img.h);
    let clut_rect = tim.clut.as_ref().map(|c| (c.fb_x, c.fb_y, c.w, c.h));
    let img_useful = needs.iter().any(|t| rects_overlap(img_rect, t.page));
    let img_collides_clut = needs.iter().any(|t| rects_overlap(img_rect, t.clut));
    let clut_useful = clut_rect.is_some_and(|r| needs.iter().any(|t| rects_overlap(r, t.clut)));
    let clut_collides_page =
        clut_rect.is_some_and(|r| needs.iter().any(|t| rects_overlap(r, t.page)));
    (
        img_useful && !img_collides_clut,
        clut_useful && !clut_collides_page,
    )
}

fn rects_overlap(a: (u16, u16, u16, u16), b: (u16, u16, u16, u16)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Vertex-centroid bounding sphere - `[cx, cy, cz, r]`. The center is the
/// mean of every vertex (mass-weighted by vertex count, since every vertex
/// contributes equally), so a model whose AABB is asymmetric (an extended
/// weapon, an arm thrown out for a strike pose) anchors the camera target on
/// the bulk of the geometry instead of halfway between the body and the
/// outlier. Radius is the maximum distance from the centroid to any vertex,
/// which guarantees every vertex is visible at the default camera distance.
fn centroid_bounds(positions: &[[f32; 3]]) -> Vec<f32> {
    if positions.is_empty() {
        return vec![0.0; 4];
    }
    let n = positions.len() as f32;
    let mut sx = 0f32;
    let mut sy = 0f32;
    let mut sz = 0f32;
    for p in positions {
        sx += p[0];
        sy += p[1];
        sz += p[2];
    }
    let c = [sx / n, sy / n, sz / n];
    let mut r2max = 0f32;
    for p in positions {
        let dx = p[0] - c[0];
        let dy = p[1] - c[1];
        let dz = p[2] - c[2];
        let r2 = dx * dx + dy * dy + dz * dz;
        if r2 > r2max {
            r2max = r2;
        }
    }
    let r = r2max.sqrt().max(1.0);
    vec![c[0], c[1], c[2], r]
}

/// Expand a fixed-size PSX sprite (8x8 or 16x16) into the same 6-vertex
/// quad layout we use for poly prims. Sprites are top-left + (u, v) =
/// top-left of the texture rect with implicit width/height.
fn sprite_to_quad(
    verts: &mut Vec<u8>,
    cmd: u8,
    color: [u8; 3],
    pos: (i16, i16),
    uv: (u8, u8),
    clut: u16,
    size: (i16, i16),
) {
    let mut flags = 0u8;
    if cmd & 0x02 != 0 {
        flags |= 0x01;
    }
    if cmd & 0x01 != 0 {
        flags |= 0x02;
    }
    // Sprites don't carry their own tpage word - they inherit from the
    // global GP0 `DrawMode` state (cmd 0xE1). The save state doesn't
    // expose a tracked tpage at prim-time, so we leave tsb=0 here; the
    // shader treats tsb=0 as "use sprite UV directly". CLUT is honoured
    // via the cba field.
    let tsb = 0u16;
    let (x, y) = pos;
    let (u, v) = uv;
    let (sw, sh) = size;
    let x1 = x.saturating_add(sw);
    let y1 = y.saturating_add(sh);
    let u1 = u.saturating_add(sw as u8);
    let v1 = v.saturating_add(sh as u8);
    // Two tris: (x0,y0)(x1,y0)(x0,y1) + (x1,y0)(x1,y1)(x0,y1)
    let push = |verts: &mut Vec<u8>, x: i16, y: i16, u: u8, v: u8| {
        verts.extend_from_slice(&x.to_le_bytes());
        verts.extend_from_slice(&y.to_le_bytes());
        verts.push(u);
        verts.push(v);
        verts.extend_from_slice(&clut.to_le_bytes());
        verts.extend_from_slice(&tsb.to_le_bytes());
        verts.extend_from_slice(&color);
        verts.push(flags);
    };
    push(verts, x, y, u, v);
    push(verts, x1, y, u1, v);
    push(verts, x, y1, u, v1);
    push(verts, x1, y, u1, v);
    push(verts, x1, y1, u1, v1);
    push(verts, x, y1, u, v1);
}

/// Locate the 7-asset table inside a kingdom PROT buffer. Scans
/// 0x800-aligned offsets for `u32_le[0] == 7` and
/// `descriptor[0].data_offset == 0x40` (the structural signature shared by
/// the `scene_scripted_asset_table` and bare `scene_asset_table` variants).
pub fn find_asset_table_offset(buf: &[u8]) -> Option<usize> {
    let mut off = 0usize;
    while off + 64 <= buf.len() {
        let count = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if count == 7 {
            let d0 = u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap());
            if d0 == 0x40 {
                return Some(off);
            }
        }
        off += 0x800;
    }
    None
}

/// Extended on-disc footprint of a field `.MAP` file (object-record table +
/// collision/floor grid at `+0x4000` + object-index grid at `+0x8000`). The
/// walk-view `.MAP` entry is identified by this exact footprint, matching
/// `legaia_engine_scene::scene::FIELD_MAP_LEN`.
const WALK_FIELD_MAP_LEN: usize = 0x12000;

/// Resolve + build the walk-view continent ground heightfield for the kingdom
/// whose bundle leads at PROT entry `prot_base`, from raw PROT.DAT bytes.
///
/// Mirrors the native `Scene::walk_heightfield` resolution without the full
/// `ProtIndex`/CDNAME machinery (the world-overview viewer already has the raw
/// PROT.DAT bytes in hand):
///
/// - **Walk `.MAP`** is the entry two slots before the kingdom block start
///   (`prot_base - 2`), whose extended on-disc footprint is exactly
///   `WALK_FIELD_MAP_LEN` (`0x12000`): the `.MAP` is slot 0 of the scene
///   block and the bundle `prot_base` names is slot 2 (pinned 14/14 against
///   a live `map01` walk capture). Falls back to scanning the block when that
///   slot isn't a `0x12000` entry.
/// - **Floor-height LUT** is `man[+0x02..+0x22]` (16 `s16` LE) from the kingdom
///   bundle's MAN slot (slot 2), the same bytes `Scene::field_floor_height_lut`
///   reads.
///
/// Reuses [`legaia_scene_formats::field_objects::build_ground_heightfield`] for the grid
/// math, with the overworld's cell gate. Returns `None` when either source can't be resolved.
pub fn build_walk_ground(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<legaia_scene_formats::field_objects::WalkHeightfield> {
    let (map_bytes, lut) = resolve_walk_map_and_lut(disc, entries, prot_base)?;
    // A kingdom's ground is `FUN_801F89B8`'s, which tests no `0x1000` bit.
    let hf = legaia_scene_formats::field_objects::build_ground_heightfield(
        map_bytes,
        &lut,
        legaia_scene_formats::field_objects::GroundCellGate::TerrainRecord,
    );
    (!hf.indices.is_empty()).then_some(hf)
}

/// One walk-frame landmark placement resolved for the world-overview viewer: a
/// slot-1 pack mesh index plus its world position in the **same `col*128`
/// world frame** the walk heightfield is built in.
///
/// Mirrors the native engine's `resolve_placement_draws` world transform: the
/// placement anchor sits at the floor height the shared
/// [`legaia_scene_formats::field_objects::Placement::world_y`] kernel resolves (the
/// corner-block average for decoration cells, the single placement-tile
/// nibble for placed objects, plus `y_off`; the runtime stores the floor LUT
/// negated) and the JS renderer applies the shared
/// `(1, -1, 1)` model flip at scale `1` - the slot-1 pack meshes are already in
/// true world units, unlike the legacy overview-frame icons that needed an
/// arbitrary presentation scale. This is why these placements line up on top of
/// [`build_walk_ground`]'s terrain while the old `world-overview.json`
/// overview-frame placements do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkPlacement {
    /// Slot-1 pack mesh index (record `+0x10`); index into the kingdom pack the
    /// `pack_mesh_*` accessors expose.
    pub pack_index: u32,
    /// World X in the `col*128` walk frame.
    pub world_x: i32,
    /// World Y from [`legaia_scene_formats::field_objects::Placement::world_y`]
    /// (pre-Y-flip, same frame as the heightfield's stored positions).
    pub world_y: i32,
    /// World Z in the `row*128` walk frame.
    pub world_z: i32,
    /// Authored yaw from the object record's `+0x0A`, PSX angle units
    /// (`4096` = full revolution) - the Sebucus bridges' quarter-turns and
    /// the decoration layer's per-tree variety. Retail's pure-Y matrix
    /// (`FUN_80026988`) maps local `+Z` to `(sin, 0, cos)`; the JS
    /// renderer's `placementModelScaled*` yaw is the opposite sense, so the
    /// page applies `rotY = -(rot_y & 0xFFF) * PI / 2048`. The record's
    /// X/Z tilts are zero on all three retail walk maps and aren't carried.
    pub rot_y: u16,
    /// `true` for a **placed** object (record flag `0x4`, the set
    /// `FUN_8003A55C` spawns as an actor - towns, towers, gates, bridges),
    /// `false` for a decoration-layer stamp (trees, mountain groups, props).
    /// The page's top-down map draws the two as separate dot families.
    pub placed: bool,
}

/// One world-map label of the loaded kingdom: a record of the 29-entry
/// location table trailing the kingdom MAN
/// ([`legaia_scene_formats::place_names::world_map_table`]), already filtered
/// to the kingdom's own `region` and lifted into world coordinates.
///
/// The coordinates are the ones the game's own map display projects the name
/// from (`map << 7`, the label pass at `0x801CF368` in PROT 0981), which is
/// the same `col*128` walk frame [`build_walk_ground`] and
/// [`build_walk_placements`] are built in - so a label lands on its town on
/// the page's top-down map with no further transform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapLabel {
    /// Record index in the 29-entry table.
    pub index: usize,
    /// World X (`map_x << 7`).
    pub world_x: i32,
    /// World Z (`map_y << 7`).
    pub world_z: i32,
    /// Discovery-flag index the retail label pass gates the name on.
    pub discovery_flag: u16,
    /// The in-game place name, read from the caller's disc.
    pub name: String,
}

/// The region byte the location table files a kingdom's records under
/// (`0` Drake, `1` Sebucus, `2` Karisto), from the kingdom bundle's PROT base.
pub fn kingdom_region(prot_base: u32) -> Option<u8> {
    match prot_base {
        85 => Some(0),
        244 => Some(1),
        391 => Some(2),
        _ => None,
    }
}

/// Resolve the world-map labels of the kingdom whose bundle sits at
/// `prot_base`: decode the kingdom MAN (slot 2 of the bundle at `prot_base`,
/// else `prot_base + 1` - the probe order [`resolve_walk_map_and_lut`] uses),
/// read its trailing location table and keep the records of this kingdom's
/// region. `None` when the base is not a kingdom's or no table decodes.
pub fn build_world_map_labels(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<Vec<WorldMapLabel>> {
    let region = kingdom_region(prot_base)?;
    let table = [prot_base, prot_base + 1].into_iter().find_map(|idx| {
        let meta = entries.iter().find(|e| e.index == idx)?;
        let off = meta.byte_offset as usize;
        let end = off.checked_add(meta.size_bytes as usize)?;
        let man = legaia_scene_formats::kingdom_bundle::decode_slot(disc.get(off..end)?, 2).ok()?;
        legaia_scene_formats::place_names::world_map_table(&man)
    })?;
    Some(
        table
            .locations
            .into_iter()
            .filter(|l| l.region == region)
            .map(|l| WorldMapLabel {
                index: l.index,
                world_x: (l.map_x as i32) << 7,
                world_z: (l.map_y as i32) << 7,
                discovery_flag: l.discovery_flag,
                name: l.name,
            })
            .collect(),
    )
}

/// Resolve the kingdom's walk-frame pack-mesh stamps in the same world frame
/// as [`build_walk_ground`], so the world-overview viewer can overlay them on
/// the continent terrain. Two disjoint layers, concatenated:
///
/// - the **placed landmarks** (the `flags & 0x4` slot-1 pack objects
///   `FUN_8003A55C` draws; [`legaia_scene_formats::field_objects::parse_placements`]),
/// - the **decoration layer** (draw-gated `0x2000` cells stamping the record
///   `+0x10` mesh without the placed flag - the crossed-quad billboard trees,
///   mountain groups, props, and the big enterable mountains whose cells carry
///   no walkable ground at all;
///   [`legaia_scene_formats::field_objects::parse_walk_decorations`]).
///
/// Reads the same walk `.MAP` + floor-height LUT [`build_walk_ground`] does
/// (see `resolve_walk_map_and_lut`) and resolves each placement's world Y from
/// the floor nibble exactly like the native `resolve_placement_draws`.
/// Placements whose mesh isn't in the scene pack (protagonist / NPC ids,
/// `pack_index == None`) are dropped. Returns `None` when the walk `.MAP` /
/// floor LUT can't be resolved.
pub fn build_walk_placements(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<Vec<WalkPlacement>> {
    let (map_bytes, lut) = resolve_walk_map_and_lut(disc, entries, prot_base)?;
    let mut placements = legaia_scene_formats::field_objects::parse_placements(map_bytes);
    placements.extend(legaia_scene_formats::field_objects::parse_walk_decorations(
        map_bytes,
    ));
    let resolved = placements
        .iter()
        .filter_map(|p| {
            let pack_index = p.pack_index?;
            // World Y through the one floor-height kernel the native engine's
            // `resolve_placed_env_draws` uses (corner average for decoration
            // cells, single nibble for placed objects).
            Some(WalkPlacement {
                pack_index: pack_index as u32,
                world_x: p.world_x,
                world_y: p.world_y(&lut),
                world_z: p.world_z,
                rot_y: p.rot_y,
                placed: p.flags & legaia_scene_formats::field_objects::FLAG_PLACED != 0,
            })
        })
        .collect();
    Some(resolved)
}

/// Resolve the kingdom's walk `.MAP` bytes + 16-entry floor-height LUT from raw
/// PROT.DAT, the shared source both [`build_walk_ground`] and
/// [`build_walk_placements`] read. Mirrors `Scene::walk_field_map_index` +
/// `Scene::field_floor_height_lut`:
///
/// - **Walk `.MAP`** is the entry two slots before the kingdom block start
///   (`prot_base - 2`), whose extended on-disc footprint is exactly
///   [`WALK_FIELD_MAP_LEN`] (`0x12000`): the `.MAP` is slot 0 of the scene
///   block and the bundle `prot_base` names is slot 2 (pinned 14/14 against
///   a live `map01` walk capture). Falls back to scanning the block when that
///   slot isn't a `0x12000` entry.
/// - **Floor-height LUT** is `man[+0x02..+0x22]` (16 `s16` LE) from the kingdom
///   bundle's MAN slot (slot 2), the same bytes `Scene::field_floor_height_lut`
///   reads.
fn resolve_walk_map_and_lut<'a>(
    disc: &'a [u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<(&'a [u8], [i16; 16])> {
    // Floor-height LUT from the kingdom MAN (slot 2). Try the bundle at
    // `prot_base`, then `prot_base + 1` (the bare scene_asset_table variant
    // carries the same MAN), matching `try_load_kingdom_at`'s probe order.
    let lut = [prot_base, prot_base + 1]
        .into_iter()
        .find_map(|idx| kingdom_floor_lut(disc, entries, idx))?;

    // Walk `.MAP` entry: `prot_base - 2` when it's a 0x12000 entry, else the
    // first 0x12000 entry in the kingdom block.
    let is_field_map = |idx: u32| -> bool {
        entries
            .iter()
            .find(|e| e.index == idx)
            .is_some_and(|e| e.size_bytes as usize == WALK_FIELD_MAP_LEN)
    };
    let walk_idx = prot_base
        .checked_sub(2)
        .filter(|&i| is_field_map(i))
        .or_else(|| (prot_base..prot_base + 8).find(|&i| is_field_map(i)))?;

    let meta = entries.iter().find(|e| e.index == walk_idx)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    let map_bytes = disc.get(off..end)?;
    Some((map_bytes, lut))
}

/// Decode the kingdom bundle at PROT entry `idx` and read its 16-entry
/// floor-height LUT (`man[+0x02..+0x22]`, 16 `s16` LE) out of the MAN slot
/// (slot 2). `None` when the entry isn't a kingdom bundle or the MAN is short.
fn kingdom_floor_lut(disc: &[u8], entries: &[EntryMeta], idx: u32) -> Option<[i16; 16]> {
    let meta = entries.iter().find(|e| e.index == idx)?;
    let off = meta.byte_offset as usize;
    let end = (meta.byte_offset + meta.size_bytes) as usize;
    let buf = disc.get(off..end)?;
    let man = legaia_scene_formats::kingdom_bundle::decode_slot(buf, 2).ok()?;
    let lut_bytes = man.get(0x02..0x22)?;
    let mut lut = [0i16; 16];
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = i16::from_le_bytes([lut_bytes[i * 2], lut_bytes[i * 2 + 1]]);
    }
    Some(lut)
}

fn read_u32_le_slice(buf: &[u8], at: usize) -> Result<u32, String> {
    let bytes = buf
        .get(at..at + 4)
        .ok_or_else(|| format!("read_u32 at {at} oob (len {})", buf.len()))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn resolve_canvas(canvas_id: &str) -> Result<HtmlCanvasElement, JsValue> {
    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let el = doc
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas not found"))?;
    el.dyn_into::<HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element with that id is not a <canvas>"))
}

/// Detect a parseable Legaia TMD inside a PROT entry buffer. Two layouts:
///   - SceneTmdStream entries: `[u32 chunk0][bare TMD][streaming chunks]`.
///     The asset crate's detector returns the exact TMD byte range.
///   - Bare TMD at offset 0 (rare; caught by raw TMD magic check).
///
/// Returns None if no TMD is present.
/// Returns the renderable TMD source plus the total count of LZS-packed TMDs
/// in the entry (0 unless the geometry lives in LZS sections).
fn detect_tmd_in_entry(buf: &[u8], class: Class) -> (Option<TmdSource>, usize) {
    if class == Class::SceneTmdStream
        && let Some(s) = legaia_scene_formats::scene_tmd_stream::detect(buf)
    {
        let r = s.tmd_range();
        // Validate the TMD actually parses; the detector is structural only.
        if legaia_tmd::parse(&buf[r.start..r.end]).is_ok() {
            return (
                Some(TmdSource::SceneTmdStream {
                    offset: r.start,
                    len: r.end - r.start,
                }),
                0,
            );
        }
    }
    // Bare TMD at offset 0?
    if buf.len() >= legaia_tmd::HEADER_SIZE
        && let Ok(_) = legaia_tmd::parse(buf)
    {
        return (Some(TmdSource::Direct { offset: 0 }), 0);
    }
    // Field/town environment geometry: Legaia TMDs packed inside the entry's
    // LZS-decompressed sections (the scene_asset_table mesh pack). The raw
    // scanners above can't see these, so the viewer reported "no TMD" for
    // whole towns. `scan_entry` walks the LZS sections; render the first and
    // surface the total count.
    let scan = legaia_container_formats::tmd_scan::scan_entry(buf);
    let lzs_hits: Vec<_> = scan
        .hits
        .iter()
        .filter_map(|(src, hit)| match src {
            legaia_container_formats::tmd_scan::Source::Lzs(idx) => Some((*idx, hit)),
            legaia_container_formats::tmd_scan::Source::Raw => None,
        })
        .collect();
    if let Some((section, hit)) = lzs_hits.first() {
        return (
            Some(TmdSource::Lzs {
                section: *section,
                offset: hit.offset,
                len: hit.byte_len,
            }),
            lzs_hits.len(),
        );
    }
    (None, 0)
}

// ---------------------------------------------------------------------------
// LegaiaAudio: WASM bindings for site/media.html
// ---------------------------------------------------------------------------

/// In-browser audio extraction surface. Owns the loaded Mode2/2352 disc plus
/// its extracted PROT.DAT bytes; exposes JSON enumerators for the three
/// audio families (VAB / BGM / XA) and PCM-returning decoders for each.
///
/// BGM playback uses [`legaia_engine_audio::WebAudioOut`] under the hood -
/// constructed lazily on the first `start_bgm` call so the autoplay policy
/// is satisfied (must happen inside a user-gesture handler on the JS side).
#[wasm_bindgen]
pub struct LegaiaAudio {
    /// Full disc bytes (kept resident so XA demux can read raw sectors).
    disc: Vec<u8>,
    /// Extracted PROT.DAT bytes. TOC parses against this slice.
    prot: Vec<u8>,
    /// Parsed PROT TOC.
    entries: Vec<legaia_prot::toc_index::EntryMeta>,
    /// WebAudio output, constructed on the first `start_bgm` call so the
    /// `AudioContext::new` happens inside a user-gesture handler. Once
    /// created, retained across BGM switches via `attach_sequencer`.
    #[cfg(target_arch = "wasm32")]
    audio_out: Option<legaia_engine_audio::WebAudioOut>,
    /// The STR movie currently opened for video playback, keyed by start LBA.
    /// Holds every frame's assembled bitstream so `str_decode_frame` can decode
    /// one frame at a time off the audio clock without re-walking the disc.
    str_video: Option<(u32, audio::StrVideo)>,
}
