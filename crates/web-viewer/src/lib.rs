//! WebAssembly bindings for browsing a Legend of Legaia disc image in the browser.
//!
//! Auto-detects: full Mode2/2352 .bin disc, raw PROT.DAT, or a single TIM.
//! After loading a disc, classifies every PROT entry via `legaia_asset::categorize`
//! and pre-scans them for embedded TIMs so the UI shows a filtered, browsable
//! list of viewable entries instead of every raw entry.

pub mod audio;
pub mod disc;
pub mod fog_lut;
pub mod ocean;
pub mod runtime;
pub mod sentinel_placements;
pub mod tmd3d;

use disc::{EntryMeta, extract_prot_dat, extract_scus, parse_prot_toc};
use legaia_asset::categorize::{Class, classify};
use legaia_asset::tim_catalog;
use legaia_asset::tim_deep_catalog;
use legaia_asset::tim_scan;
use legaia_asset::worldmap_menu;
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

fn console_log(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
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
    legaia_asset::world_map_overlay::Axis,
    legaia_asset::world_map_overlay::Axis,
) {
    use legaia_asset::world_map_overlay::Axis;
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

use crate::ocean::{OceanAssets, find_ocean_assets};

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
    /// World-map landmark menu (16 names + ~20 placement records) parsed from
    /// `SCUS_942.54` at load time. Only populated when a full disc image is
    /// loaded (raw PROT.DAT / single TIM paths leave this `None`).
    worldmap_menu: Option<worldmap_menu::WorldmapMenu>,
    /// Fog LUT bytes extracted from SCUS at load time. 4 KiB (2048 u16
    /// entries) — the shared depth-cue ramp the world-map overlay leaves
    /// at `0x801F7644..0x801F8690` consult on every vertex. None when
    /// the SCUS extract or LUT scan didn't surface a match (raw
    /// PROT.DAT load, regional variant, modded disc).
    fog_lut: Option<Vec<u8>>,
    /// Item-name table (`PTR_DAT_8007436C`) decoded from `SCUS_942.54` at
    /// load time. Resolves a monster record's raw `drop_item` id into a
    /// readable name for the enemy table. `None` on raw PROT.DAT loads (no
    /// SCUS) - the page then falls back to the raw id.
    item_names: Option<legaia_asset::item_names::ItemNameTable>,
    /// Spell-name table (`DAT_800754C8` / `DAT_800754D0`) decoded from
    /// `SCUS_942.54` at load time. Resolves a monster record's global
    /// magic-attack ids into the on-screen spell names (`0x27` -> `Tail Fire`).
    /// `None` on raw PROT.DAT loads.
    spell_names: Option<legaia_asset::spell_names::SpellNameTable>,
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
    /// Walk-view continent ground for the currently-loaded kingdom: the
    /// procedural heightfield surface built from the walk `.MAP` floor grid,
    /// per-cell-textured from the terrain-type-keyed multi-page atlas. Built by
    /// [`LegaiaViewer::set_scene_kingdom`] alongside the landmark pack; consumed
    /// by the `walk_ground_*` accessors so the world-overview viewer draws the
    /// same continent terrain the native engine does. `None` until a kingdom is
    /// loaded, or when the walk `.MAP` / floor LUT can't be resolved.
    walk_ground: Option<legaia_asset::field_objects::WalkHeightfield>,
    /// Walk-frame placed landmarks for the currently-loaded kingdom: the
    /// `flags & 0x4` slot-1 pack objects (`FUN_8003A55C`), resolved into the
    /// same `col*128` world frame as [`LegaiaViewer::walk_ground`]. Built by
    /// [`LegaiaViewer::set_scene_kingdom`]; consumed by the `walk_placement_*`
    /// accessors so the viewer draws the landmark meshes on top of the
    /// continent terrain instead of the misaligned overview-frame JSON layer.
    walk_placements: Option<Vec<WalkPlacement>>,
}

#[wasm_bindgen]
impl LegaiaViewer {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str) -> Result<LegaiaViewer, JsValue> {
        console_error_panic_hook::set_once();
        // Validate that the id resolves to a canvas at construction, even
        // though we re-resolve on every render. This catches the common
        // typo case immediately instead of silently no-oping later.
        let _ = resolve_canvas(canvas_id)?;
        Ok(Self {
            canvas_id: canvas_id.to_string(),
            disc: Vec::new(),
            viewable: Vec::new(),
            current: 0,
            clut_idx: 0,
            kingdom: None,
            continent: None,
            worldmap_menu: None,
            fog_lut: None,
            item_names: None,
            spell_names: None,
            tim_catalog: Vec::new(),
            tim_deep_catalog: Vec::new(),
            deep_section_cache: std::cell::RefCell::new(None),
            walk_ground: None,
            walk_placements: None,
        })
    }

    /// Load a disc image. Auto-detects: full Mode2/2352 .bin, raw PROT.DAT,
    /// or single TIM. Returns the count of viewable entries (entries with at
    /// least one decodable TIM) for the JS UI.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        self.viewable.clear();
        self.current = 0;

        self.worldmap_menu = None;
        self.fog_lut = None;
        self.item_names = None;
        self.spell_names = None;
        self.tim_catalog = Vec::new();
        self.tim_deep_catalog = Vec::new();
        *self.deep_section_cache.borrow_mut() = None;
        self.walk_ground = None;
        self.walk_placements = None;
        let prot_bytes = if let Some(extracted) = extract_prot_dat(&bytes) {
            console_log(&format!(
                "Detected Mode2/2352 disc image ({} MB); extracted PROT.DAT ({} MB)",
                bytes.len() / 1024 / 1024,
                extracted.len() / 1024 / 1024
            ));
            // Best-effort parse of the world-map menu out of SCUS_942.54.
            // Failures are silent: the user might be loading a region build
            // that ships a differently-named executable, and the rest of the
            // viewer still works without the menu overlay.
            if let Some(scus) = extract_scus(&bytes) {
                match worldmap_menu::parse_scus(&scus) {
                    Ok(menu) => {
                        console_log(&format!(
                            "Parsed world-map menu: {} names, {} placements",
                            menu.names.len(),
                            menu.placements.len()
                        ));
                        self.worldmap_menu = Some(menu);
                    }
                    Err(e) => console_log(&format!("worldmap_menu::parse_scus skipped: {e}")),
                }
                // Locate the world-map fog LUT - same bytes the runtime
                // consults on every vertex. The viewer auto-uploads
                // these to the GL renderer so the user doesn't need to
                // drop a separate fog_probe.lut.bin file.
                if let Some(lut) = fog_lut::find(&scus) {
                    self.fog_lut = Some(lut.to_vec());
                    console_log(&format!(
                        "Extracted fog LUT from SCUS ({} bytes, {} entries)",
                        lut.len(),
                        lut.len() / 2
                    ));
                } else {
                    console_log("fog_lut::find skipped: no LUT signature in SCUS");
                }
                // Decode the item-name table so the enemy table can show drop
                // names instead of raw ids. Silent on failure (regional build
                // with a different table address) - the page falls back to ids.
                if let Some(table) = legaia_asset::item_names::ItemNameTable::from_scus(&scus) {
                    console_log(&format!(
                        "Decoded item-name table from SCUS ({} named ids)",
                        table.named_count()
                    ));
                    self.item_names = Some(table);
                } else {
                    console_log("item_names::from_scus skipped: table not found in SCUS");
                }
                // Decode the spell-name table so the enemy table can show the
                // monster's magic attacks by name instead of raw ids.
                if let Some(table) = legaia_asset::spell_names::SpellNameTable::from_scus(&scus) {
                    self.spell_names = Some(table);
                } else {
                    console_log("spell_names::from_scus skipped: table not found in SCUS");
                }
            }
            extracted
        } else if parse_prot_toc(&bytes).is_some() {
            console_log("Loading bytes as raw PROT.DAT");
            bytes
        } else if let Ok(tim) = legaia_tim::parse(&bytes) {
            console_log(&format!(
                "Loading standalone TIM ({:?}, {}x{})",
                tim.mode,
                tim.pixel_width(),
                tim.image.h
            ));
            self.disc = bytes;
            self.render_tim_at(&self.disc.clone(), 0, "Standalone TIM")?;
            return Ok(0);
        } else {
            return Err(JsValue::from_str(
                "Unrecognised buffer: not a Mode2/2352 disc, not a PROT.DAT, not a TIM",
            ));
        };

        let entries = parse_prot_toc(&prot_bytes)
            .ok_or_else(|| JsValue::from_str("PROT TOC parse failed"))?;

        // Build the flat TIM catalog from the whole image + TOC spans. This
        // catches TIMs in the unindexed system-UI gap that the per-entry
        // tim-scan below never sees, and gives the UI a stable per-TIM id.
        let spans: Vec<(u64, u64, u32)> = entries
            .iter()
            .map(|e| (e.byte_offset, e.size_bytes, e.index))
            .collect();
        self.tim_catalog = tim_catalog::build_from_spans(&prot_bytes, &spans);
        console_log(&format!(
            "Cataloged {} TIMs in PROT.DAT",
            self.tim_catalog.len()
        ));

        // Deep tier: LZS-decompress every entry and catalog the compressed
        // TIMs the flat (raw-bytes) catalog can't see.
        self.tim_deep_catalog = tim_deep_catalog::build_from_spans(&prot_bytes, &spans);
        console_log(&format!(
            "Cataloged {} TIMs inside LZS-compressed sections",
            self.tim_deep_catalog.len()
        ));

        console_log(&format!(
            "Found {} PROT entries - classifying…",
            entries.len()
        ));
        self.disc = prot_bytes;

        // Index the strict TIM catalog by owning entry so the entry browser
        // shows the SAME TIM the catalog does (strict-validated, jPSXdec
        // parity) instead of whatever the lenient per-entry scan picked first.
        // Entries whose TIMs only exist inside LZS-compressed sections aren't
        // in the catalog (the flat scan doesn't decompress), so those still
        // fall back to the lenient scan below.
        // entry index -> its catalog TIM ids, ascending (the catalog is built
        // in ascending-offset order, so push order is already correct).
        let mut catalog_by_entry: std::collections::HashMap<u32, Vec<u32>> =
            std::collections::HashMap::new();
        for t in &self.tim_catalog {
            if let Some(idx) = t.entry_index {
                catalog_by_entry.entry(idx).or_default().push(t.id);
            }
        }

        // Classify + tim-scan each entry. Skip non-viewable classes early to
        // keep this fast on the user's main thread. The expensive step is
        // tim_scan::scan_entry which LZS-decompresses + walks magic offsets;
        // we only run it on classes that can plausibly contain TIMs.
        let mut viewable = Vec::new();
        for e in entries {
            let off = e.byte_offset as usize;
            let end = (e.byte_offset + e.size_bytes) as usize;
            if end > self.disc.len() {
                continue;
            }
            let buf = &self.disc[off..end];
            let report = classify(buf);

            let cat_ids: &[u32] = catalog_by_entry
                .get(&e.index)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let has_cat = !cat_ids.is_empty();

            // Skip classes that never carry TIMs - unless the catalog already
            // found a (strict) TIM here, in which case show it regardless.
            if !has_cat
                && matches!(
                    report.class,
                    Class::Empty
                        | Class::Tiny
                        | Class::AllZeros
                        | Class::MostlyZeros
                        | Class::ConstantByte
                        | Class::PochiFiller
                        | Class::MipsOverlay
                        | Class::OverlayPtrTable
                        | Class::SceneVabStream
                )
            {
                continue;
            }

            // Prefer the strict catalog TIM for this entry; fall back to the
            // lenient scan only when the catalog has none (LZS-only entries).
            let (first_tim, tim_count) = if has_cat {
                let t = &self.tim_catalog[cat_ids[0] as usize];
                let ft = TimHit {
                    source: TimSource::Raw(t.offset_in_entry as usize),
                    width: t.width,
                    height: t.height,
                    bpp: t.bpp,
                };
                (Some(ft), cat_ids.len())
            } else {
                let scan = tim_scan::scan_entry(buf);
                let tim_count = scan.hits.len();
                // First hit whose bytes actually decode (not just magic match).
                let mut first_tim = None;
                for (source, hit) in &scan.hits {
                    let bytes_for_parse: Option<&[u8]> = match source {
                        tim_scan::Source::Raw => Some(&buf[hit.offset..]),
                        tim_scan::Source::Lzs(idx) => {
                            scan.lzs_sections.get(*idx).map(|s| &s[hit.offset..])
                        }
                    };
                    if let Some(b) = bytes_for_parse
                        && legaia_tim::parse(b).is_ok()
                    {
                        let ts = match source {
                            tim_scan::Source::Raw => TimSource::Raw(hit.offset),
                            tim_scan::Source::Lzs(idx) => TimSource::Lzs {
                                section: *idx,
                                offset: hit.offset,
                            },
                        };
                        first_tim = Some(TimHit {
                            source: ts,
                            width: hit.width,
                            height: hit.height,
                            bpp: hit.bpp,
                        });
                        break;
                    }
                }
                (first_tim, tim_count)
            };

            // Detect a leading TMD for the 3D viewer path (raw, scene_tmd_stream,
            // or the first of an LZS-packed environment-geometry mesh pack).
            let (tmd_source, tmd_pack_count) = detect_tmd_in_entry(buf, report.class);

            // Skip entries that have neither a viewable TIM nor a parseable TMD.
            if first_tim.is_none() && tmd_source.is_none() {
                continue;
            }

            viewable.push(ViewerEntry {
                meta: e,
                class: report.class,
                first_tim,
                tim_count,
                tmd_source,
                tmd_pack_count,
            });
        }

        console_log(&format!(
            "Filtered to {} viewable entries (any embedded TIM)",
            viewable.len()
        ));
        self.viewable = viewable;
        // Don't render here - the JS side decides which canvas surface
        // (2D blit vs WebGL2 mesh viewer vs assembled world map) is
        // active and calls the matching accessor. Rendering at load time
        // unconditionally requested a 2D context, which failed when the
        // canvas had been WebGL2-bound by a prior session in the same
        // page lifetime ("no 2d context (canvas was already bound to
        // webgl...)" - the world-overview page hits this any time the
        // user reloads a disc after entering the top-down or mesh view).
        Ok(self.viewable.len() as u32)
    }

    pub fn entry_count(&self) -> u32 {
        self.viewable.len() as u32
    }

    pub fn current_index(&self) -> u32 {
        self.viewable
            .get(self.current)
            .map(|e| e.meta.index)
            .unwrap_or(0)
    }

    pub fn next_entry(&mut self) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        self.current = (self.current + 1) % self.viewable.len();
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn prev_entry(&mut self) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        self.current = if self.current == 0 {
            self.viewable.len() - 1
        } else {
            self.current - 1
        };
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    /// Jump to the slot in the filtered list (NOT the PROT index). Used by
    /// the dropdown / list-click UI.
    pub fn set_slot(&mut self, slot: u32) -> Result<u32, JsValue> {
        if self.viewable.is_empty() {
            return Ok(0);
        }
        let s = (slot as usize).min(self.viewable.len() - 1);
        self.current = s;
        self.clut_idx = 0;
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn set_clut(&mut self, idx: u32) -> Result<(), JsValue> {
        self.clut_idx = idx as usize;
        self.render_current()
    }

    // --- TIM Catalog browse mode -----------------------------------------
    //
    // The catalog is a flat, jPSXdec-parity inventory of every standard TIM
    // in the loaded PROT.DAT, keyed by a stable id. These accessors let the
    // page page through all of them by id and switch CLUT variants, even for
    // TIMs that live in the unindexed system-UI gap (no owning PROT entry).

    /// Number of cataloged TIMs in the loaded PROT.DAT.
    pub fn catalog_len(&self) -> u32 {
        self.tim_catalog.len() as u32
    }

    /// Number of CLUT palettes available for cataloged TIM `id` (0 for
    /// 16/24bpp TIMs, which carry no palette).
    pub fn catalog_clut_count(&self, id: u32) -> u32 {
        self.tim_catalog
            .get(id as usize)
            .map(|t| t.clut_count as u32)
            .unwrap_or(0)
    }

    /// JSON describing cataloged TIM `id` (offset, owning entry, dimensions,
    /// CLUT count, byte length, fingerprint) for the info panel.
    pub fn catalog_info_json(&self, id: u32) -> String {
        match self.tim_catalog.get(id as usize) {
            Some(t) => {
                let entry = match t.entry_index {
                    Some(i) => i.to_string(),
                    None => "gap".to_string(),
                };
                format!(
                    "{{\"id\":{},\"abs_offset\":{},\"sector\":{},\"entry\":\"{}\",\
                     \"offset_in_entry\":{},\"width\":{},\"height\":{},\"bpp\":{},\
                     \"clut_count\":{},\"byte_len\":{},\"fnv1a\":\"{:016x}\",\"label\":{}}}",
                    t.id,
                    t.abs_offset,
                    t.sector,
                    entry,
                    t.offset_in_entry,
                    t.width,
                    t.height,
                    t.bpp,
                    t.clut_count,
                    t.byte_len,
                    t.fnv1a,
                    json_label(t.label),
                )
            }
            None => "{}".to_string(),
        }
    }

    /// Render cataloged TIM `id` with CLUT `clut` into the 2D canvas named
    /// `canvas_id`. The catalog browser uses its own canvas (separate from
    /// the PROT-entry browser's, which switches between 2D and WebGL), so it
    /// takes the target id explicitly rather than the viewer's bound canvas.
    pub fn render_catalog_tim(&self, id: u32, clut: u32, canvas_id: &str) -> Result<(), JsValue> {
        let t = self
            .tim_catalog
            .get(id as usize)
            .ok_or_else(|| JsValue::from_str(&format!("catalog id {id} out of range")))?;
        let off = t.abs_offset as usize;
        let tim = legaia_tim::parse(&self.disc[off..])
            .map_err(|e| JsValue::from_str(&format!("catalog[{id}] TIM parse: {e}")))?;
        let clut_idx = if t.clut_count > 0 {
            (clut as usize).min(t.clut_count - 1)
        } else {
            0
        };
        let rgba = legaia_tim::decode_rgba8(&tim, clut_idx)
            .map_err(|e| JsValue::from_str(&format!("catalog[{id}] decode: {e}")))?;
        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        let canvas = resolve_canvas(canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("catalog canvas has no 2D context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        if w == 0 || h == 0 {
            return Err(JsValue::from_str(&format!(
                "catalog[{id}]: empty TIM ({w}x{h})"
            )));
        }
        canvas.set_width(w);
        canvas.set_height(h);
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    // --- Deep TIM Catalog (compressed textures) --------------------------
    //
    // The deep catalog is the LZS-embedded tier: standard TIMs recovered from
    // inside compressed PROT sections, which the flat (raw-bytes) catalog
    // above can't reach. Keyed by (entry, lzs-section, offset-in-section).
    // These accessors mirror the flat-catalog ones so the page can drive a
    // second, clearly-labeled grid from the same UI code.

    /// Number of cataloged compressed TIMs in the loaded PROT.DAT.
    pub fn deep_catalog_len(&self) -> u32 {
        self.tim_deep_catalog.len() as u32
    }

    /// Number of CLUT palettes available for deep-catalog TIM `id`.
    pub fn deep_catalog_clut_count(&self, id: u32) -> u32 {
        self.tim_deep_catalog
            .get(id as usize)
            .map(|t| t.clut_count as u32)
            .unwrap_or(0)
    }

    /// JSON describing deep-catalog TIM `id` (owning entry, LZS section,
    /// offset within the decoded section, dimensions, CLUT count, byte
    /// length, fingerprint) for the info panel.
    pub fn deep_catalog_info_json(&self, id: u32) -> String {
        match self.tim_deep_catalog.get(id as usize) {
            Some(t) => format!(
                "{{\"id\":{},\"entry\":{},\"lzs_section\":{},\"offset_in_section\":{},\
                 \"width\":{},\"height\":{},\"bpp\":{},\"clut_count\":{},\
                 \"byte_len\":{},\"fnv1a\":\"{:016x}\",\"label\":{}}}",
                t.id,
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                json_label(t.label),
            ),
            None => "{}".to_string(),
        }
    }

    /// Decompress deep-catalog TIM `id`'s owning entry (via a one-entry cache)
    /// and return the decoded section bytes it lives in, plus the offset.
    fn deep_section_bytes(&self, id: u32) -> Result<(Vec<u8>, usize), JsValue> {
        let t = self
            .tim_deep_catalog
            .get(id as usize)
            .ok_or_else(|| JsValue::from_str(&format!("deep catalog id {id} out of range")))?;
        // Reuse the cached sections if this is the same entry as last time.
        {
            let cache = self.deep_section_cache.borrow();
            if let Some((cached_entry, sections)) = cache.as_ref()
                && *cached_entry == t.entry_index
            {
                let section = sections.get(t.lzs_section as usize).ok_or_else(|| {
                    JsValue::from_str(&format!("deep[{id}]: section {} gone", t.lzs_section))
                })?;
                return Ok((section.clone(), t.offset_in_section as usize));
            }
        }
        // Cache miss: find the entry span, slice, decompress, and cache.
        let entries = parse_prot_toc(&self.disc)
            .ok_or_else(|| JsValue::from_str("deep: PROT TOC parse failed"))?;
        let entry = entries
            .iter()
            .find(|e| e.index == t.entry_index)
            .ok_or_else(|| JsValue::from_str(&format!("deep[{id}]: entry gone")))?;
        let start = entry.byte_offset as usize;
        let end = start.saturating_add(entry.size_bytes as usize);
        if end > self.disc.len() {
            return Err(JsValue::from_str(&format!("deep[{id}]: entry span OOB")));
        }
        let sections = legaia_lzs::decompress_container(&self.disc[start..end])
            .map_err(|e| JsValue::from_str(&format!("deep[{id}]: LZS decode: {e}")))?;
        let section = sections
            .get(t.lzs_section as usize)
            .ok_or_else(|| JsValue::from_str(&format!("deep[{id}]: section gone")))?
            .clone();
        let off = t.offset_in_section as usize;
        *self.deep_section_cache.borrow_mut() = Some((t.entry_index, sections));
        Ok((section, off))
    }

    /// Render deep-catalog TIM `id` with CLUT `clut` into the 2D canvas named
    /// `canvas_id`.
    pub fn render_deep_catalog_tim(
        &self,
        id: u32,
        clut: u32,
        canvas_id: &str,
    ) -> Result<(), JsValue> {
        let (section, off) = self.deep_section_bytes(id)?;
        let tim = legaia_tim::parse(&section[off..])
            .map_err(|e| JsValue::from_str(&format!("deep[{id}] TIM parse: {e}")))?;
        let nclut = tim.palette_count();
        let clut_idx = if nclut > 0 {
            (clut as usize).min(nclut - 1)
        } else {
            0
        };
        let rgba = legaia_tim::decode_rgba8(&tim, clut_idx)
            .map_err(|e| JsValue::from_str(&format!("deep[{id}] decode: {e}")))?;
        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        if w == 0 || h == 0 {
            return Err(JsValue::from_str(&format!(
                "deep[{id}]: empty TIM ({w}x{h})"
            )));
        }
        let canvas = resolve_canvas(canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("deep catalog canvas has no 2D context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        canvas.set_width(w);
        canvas.set_height(h);
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    /// Open a world-map kingdom's 7-asset bundle, LZS-decode slot 0
    /// (TIM_LIST) into a shared VRAM, and LZS-decode slot 1 (TMD pack) for
    /// per-slot mesh access. Returns the pack count (= number of scene-pool
    /// TMDs available to `pack_mesh`).
    ///
    /// `prot_base` is the kingdom's leading PROT entry index - 85 for Drake
    /// (`map01`), 244 for Sebucus (`map02`), 391 for Karisto (`map03`).
    /// Either the `scene_scripted_asset_table` (PROT base) or the bare
    /// `scene_asset_table` (PROT base+1) works; the detector finds the
    /// 7-asset table at the first 0x800-aligned offset whose `u32_le[0] == 7`
    /// and `descriptor[0].data_offset == 0x40`.
    ///
    /// Implementation mirrors `FUN_8001F05C case 2` (TMD-pack dispatch): the
    /// pack is `[u32 count][u32 word_offsets[count]][TMD bodies]` with
    /// offsets in 4-byte words (`puVar1 + puVar5[1]` on `uint*`). The
    /// VRAM upload is unconditional (every TIM in slot 0 is uploaded);
    /// per-prim filtering happens later in `pack_mesh_*`.
    pub fn set_scene_kingdom(&mut self, prot_base: u32) -> Result<u32, JsValue> {
        let entries = parse_prot_toc(&self.disc)
            .ok_or_else(|| JsValue::from_str("set_scene_kingdom: no PROT TOC available"))?;
        // Try PROT base first (scene_scripted_asset_table variant), then
        // base+1 (bare scene_asset_table). Either carries the same 7-asset
        // bundle for the world-map kingdoms.
        let pack = self
            .try_load_kingdom_at(&entries, prot_base)
            .or_else(|_| self.try_load_kingdom_at(&entries, prot_base + 1))
            .map_err(|e| {
                JsValue::from_str(&format!("set_scene_kingdom({prot_base}) failed: {e}"))
            })?;
        let count = pack.byte_offsets.len() as u32;
        console_log(&format!(
            "kingdom PROT {} loaded: {} TMDs in pack ({} bytes), VRAM filled",
            pack.prot_index,
            count,
            pack.pack.len()
        ));
        self.kingdom = Some(pack);
        // Sweep nearby entries (+8..+12) for a second 7-asset table holding
        // the bulk continent terrain TMDs. Drake's is at +8 (entry 0093, 70
        // TMDs); Sebucus's at +9 (entry 0253, 43 TMDs); Karisto's not yet
        // pinned. We try a window of plausible offsets and accept the first
        // entry that decodes cleanly.
        self.continent = None;
        for off in 8..=12u32 {
            let candidate = prot_base + off;
            if let Ok(p) = self.try_load_kingdom_at(&entries, candidate) {
                console_log(&format!(
                    "continent PROT {} loaded: {} TMDs in pack ({} bytes)",
                    p.prot_index,
                    p.byte_offsets.len(),
                    p.pack.len()
                ));
                self.continent = Some(p);
                break;
            }
        }

        // Build the walk-view continent ground heightfield for this kingdom.
        // The native engine's world-map render draws this surface (the slot-1
        // pack is only the sparse landmarks); reproducing it here brings the
        // site viewer to terrain parity. Sources the walk `.MAP` floor grid +
        // the kingdom MAN's floor-height LUT; reuses `build_walk_heightfield`.
        self.walk_ground = build_walk_ground(&self.disc, &entries, prot_base);
        if let Some(hf) = &self.walk_ground {
            console_log(&format!(
                "walk heightfield: {} quads ({} verts) for PROT base {}",
                hf.quad_count(),
                hf.positions.len(),
                prot_base
            ));
        } else {
            console_log(&format!(
                "walk heightfield: unavailable for PROT base {prot_base} (no walk .MAP / floor LUT)"
            ));
        }

        // Resolve the walk-frame placed landmarks (the slot-1 pack meshes
        // `FUN_8003A55C` stamps on the continent) in the same world frame as
        // the heightfield, so the viewer can draw them on top of the terrain.
        self.walk_placements = build_walk_placements(&self.disc, &entries, prot_base);
        if let Some(ps) = &self.walk_placements {
            console_log(&format!(
                "walk placements: {} landmarks for PROT base {prot_base}",
                ps.len()
            ));
        }
        Ok(count)
    }

    /// Number of walk-frame placed landmarks for the currently-loaded kingdom
    /// (slot-1 pack meshes positioned on the continent terrain). 0 when no
    /// kingdom is loaded or the walk `.MAP` / floor LUT couldn't be resolved.
    pub fn walk_placement_count(&self) -> u32 {
        self.walk_placements
            .as_ref()
            .map(|p| p.len() as u32)
            .unwrap_or(0)
    }

    /// Per-placement kingdom pack-mesh slot (record `+0x10`), one `u32` per
    /// walk-frame landmark in placement order. Feed each into `pack_mesh` to
    /// select the mesh, then draw it at the matching
    /// [`Self::walk_placement_positions`] entry.
    pub fn walk_placement_slots(&self) -> Vec<u32> {
        self.walk_placements
            .as_ref()
            .map(|ps| ps.iter().map(|p| p.pack_index).collect())
            .unwrap_or_default()
    }

    /// Per-placement world positions `[x, y, z, ...]` (flattened), in the same
    /// pre-Y-flip `col*128` world frame as [`Self::walk_ground_positions`], so
    /// the JS renderer draws each landmark with the same `(1, -1, 1)` model
    /// flip at scale `1` (the slot-1 meshes are already in true world units).
    pub fn walk_placement_positions(&self) -> Vec<f32> {
        let Some(ps) = self.walk_placements.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(ps.len() * 3);
        for p in ps {
            out.push(p.world_x as f32);
            out.push(p.world_y as f32);
            out.push(p.world_z as f32);
        }
        out
    }

    /// Per-vertex world positions of the walk-view continent ground
    /// heightfield, flattened `[x, y, z, ...]`. Empty until a kingdom is loaded.
    /// Same pre-Y-flip world frame as the landmark placement draws, so the JS
    /// renderer applies the same `(1, -1, 1)` model flip (scale 1, no offset).
    pub fn walk_ground_positions(&self) -> Vec<f32> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.positions.len() * 3);
        for p in &hf.positions {
            out.extend_from_slice(p);
        }
        out
    }

    /// Per-vertex page-local UVs (`u8` pairs) of the walk-view ground, flattened
    /// `[u, v, ...]`. Each cell's four corners cover its `32 x 32` atlas tile.
    pub fn walk_ground_uvs(&self) -> Vec<u8> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.uvs.len() * 2);
        for uv in &hf.uvs {
            out.extend_from_slice(uv);
        }
        out
    }

    /// Per-vertex `[clut, tpage]` (PSX CBA + tpage words) of the walk-view
    /// ground, flattened. Distinct per cell so grass / mountain / water / forest
    /// cells sample their own VRAM page from the kingdom slot-0 atlas.
    pub fn walk_ground_cba_tsb(&self) -> Vec<u16> {
        let Some(hf) = self.walk_ground.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(hf.cba_tsb.len() * 2);
        for ct in &hf.cba_tsb {
            out.extend_from_slice(ct);
        }
        out
    }

    /// Triangle indices of the walk-view ground (two triangles per cell quad).
    pub fn walk_ground_indices(&self) -> Vec<u32> {
        self.walk_ground
            .as_ref()
            .map(|hf| hf.indices.clone())
            .unwrap_or_default()
    }

    /// Number of ground cells (quads) in the walk-view heightfield. 0 when no
    /// kingdom is loaded or the heightfield couldn't be resolved.
    pub fn walk_ground_quad_count(&self) -> u32 {
        self.walk_ground
            .as_ref()
            .map(|hf| hf.quad_count() as u32)
            .unwrap_or(0)
    }

    /// Number of TMDs in the currently-loaded continent pack. 0 when no
    /// continent pack was found for this kingdom.
    pub fn continent_pack_count(&self) -> u32 {
        self.continent
            .as_ref()
            .map(|k| k.byte_offsets.len() as u32)
            .unwrap_or(0)
    }

    /// PROT index the continent pack was loaded from (0 when none).
    pub fn continent_prot_index(&self) -> u32 {
        self.continent.as_ref().map(|k| k.prot_index).unwrap_or(0)
    }

    /// VRAM bytes (1 MB) built from the continent pack's slot 0. Distinct from
    /// the landmark VRAM since the two packs ship independent TIM_LISTs.
    pub fn continent_pack_vram_bytes(&self) -> Vec<u8> {
        self.continent
            .as_ref()
            .map(|k| k.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Select the active continent pack slot. Parallel to `pack_mesh` but
    /// operates on the continent pack.
    pub fn continent_pack_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let k = self
            .continent
            .as_mut()
            .ok_or_else(|| JsValue::from_str("continent_pack_mesh: no continent loaded"))?;
        let s = slot as usize;
        if s >= k.byte_offsets.len() {
            return Err(JsValue::from_str(&format!(
                "continent_pack_mesh: slot {s} >= count {}",
                k.byte_offsets.len()
            )));
        }
        k.cur_slot = Some(s);
        Ok(slot)
    }

    pub fn continent_pack_mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn continent_pack_mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn continent_pack_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_continent_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn continent_pack_mesh_indices(&self) -> Vec<u32> {
        self.build_continent_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    pub fn continent_pack_mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_continent_mesh() else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let cz = (lo[2] + hi[2]) * 0.5;
        let dx = (hi[0] - lo[0]) * 0.5;
        let dy = (hi[1] - lo[1]) * 0.5;
        let dz = (hi[2] - lo[2]) * 0.5;
        let r = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
        vec![cx, cy, cz, r]
    }

    /// Set the active pack-mesh slot. Subsequent `pack_mesh_*` calls source
    /// from `pack[byte_offsets[slot]..byte_ends[slot]]`. Returns an error
    /// when no kingdom is loaded or `slot >= pack count`.
    pub fn pack_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        let k = self
            .kingdom
            .as_mut()
            .ok_or_else(|| JsValue::from_str("pack_mesh: no kingdom loaded"))?;
        let s = slot as usize;
        if s >= k.byte_offsets.len() {
            return Err(JsValue::from_str(&format!(
                "pack_mesh: slot {s} >= count {}",
                k.byte_offsets.len()
            )));
        }
        k.cur_slot = Some(s);
        Ok(slot)
    }

    /// Number of TMDs in the currently-loaded kingdom pack. 0 when no
    /// kingdom is loaded.
    pub fn pack_count(&self) -> u32 {
        self.kingdom
            .as_ref()
            .map(|k| k.byte_offsets.len() as u32)
            .unwrap_or(0)
    }

    /// VRAM bytes (1 MB) built from every TIM in the kingdom's slot 0
    /// (TIM_LIST). Reuse across every `pack_mesh_*` call - the kingdom
    /// pack's per-slot TMDs all sample from this one shared image.
    pub fn pack_vram_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .map(|k| k.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Ocean tile pixel data (4bpp indexed), 64 halfwords × 256 rows =
    /// 32 768 bytes. Each byte holds 2 pixels (low nibble first). The
    /// CLUT index addressing is `pixel = byte >> 4` for the high pixel
    /// and `byte & 0x0F` for the low pixel. Empty when the kingdom is
    /// not a world-map kingdom or the ocean TIM wasn't found.
    pub fn ocean_texture_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.texture.clone())
            .unwrap_or_default()
    }

    /// Static base CLUT for the ocean tile row: 256 entries × 2 bytes
    /// (BGR555 LE) = 512 bytes. The first 16 entries are the ones the
    /// animation cycle overrides each frame; entries 16..255 stay fixed
    /// and belong to other tiles sharing the same VRAM row.
    pub fn ocean_base_clut_bytes(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.base_clut.clone())
            .unwrap_or_default()
    }

    /// 13-frame ocean CLUT animation table: 13 × 32 bytes = 416 bytes,
    /// frame-0 first. Each frame is 16 BGR555 entries (the same shape as
    /// the first 16 entries of [`Self::ocean_base_clut_bytes`]). The
    /// runtime DMAs one frame at a time onto VRAM (0, 506) to cycle
    /// the wave colours through the ocean tile.
    pub fn ocean_animation_frames(&self) -> Vec<u8> {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| o.animation_frames.clone())
            .unwrap_or_default()
    }

    /// Number of valid ocean animation frames (typically 13). Returns 0
    /// when the kingdom doesn't have ocean assets.
    pub fn ocean_frame_count(&self) -> u32 {
        self.kingdom
            .as_ref()
            .and_then(|k| k.ocean.as_ref())
            .map(|o| (o.animation_frames.len() / 32) as u32)
            .unwrap_or(0)
    }

    /// Parallel to [`Self::mesh_positions`] but sources from the currently
    /// selected kingdom pack slot.
    pub fn pack_mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn pack_mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn pack_mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn pack_mesh_indices(&self) -> Vec<u32> {
        self.build_kingdom_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    pub fn pack_mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_kingdom_mesh() else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let cz = (lo[2] + hi[2]) * 0.5;
        let dx = (hi[0] - lo[0]) * 0.5;
        let dy = (hi[1] - lo[1]) * 0.5;
        let dz = (hi[2] - lo[2]) * 0.5;
        let r = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
        vec![cx, cy, cz, r]
    }

    /// Decode the slot-4 world-map overlay wireframe for the kingdom at
    /// `prot_base` and return a packed line-segment list for top-down
    /// rendering.
    ///
    /// The wireframe is the dev-menu top-view overlay - coastline curves
    /// (Drake body 12 = 1200-vertex outline) and the ±32K world-boundary
    /// frame (Drake body 13). Loaded verbatim into RAM at `0x8011A624` for
    /// Drake (32304 bytes); format is fully reversed (see
    /// [`docs/formats/world-map-overlay.md`]).
    ///
    /// `style` selects the polyline-construction mode:
    /// `"row"` (each group as one polyline), `"col"` (each record-slot as
    /// one polyline across groups), `"pairs"` (every 2 consecutive
    /// records emit one segment), or `"grid"` (both row and column
    /// edges of the `count_a x count_b` vertex grid). Unknown values
    /// fall back to `"row"`.
    ///
    /// Output layout (single packed `Vec<u8>`, little-endian):
    ///
    /// ```text
    /// [u32 line_count]
    /// [Line; line_count]   ; struct, 12 bytes each:
    ///     u8  body_index
    ///     u8  group_index_low   ; group_index = (low | (high << 8))
    ///     u8  group_index_high
    ///     u8  _pad
    ///     i16 x0
    ///     i16 z0
    ///     i16 x1
    ///     i16 z1
    /// ```
    ///
    /// Returns an empty buffer when slot 4 is missing or fails to parse.
    /// The JS-side renderer assigns per-body colors based on `body_index`.
    pub fn slot4_wireframe_lines(&self, prot_base: u32, style: &str, axes: &str) -> Vec<u8> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let mode = match style {
            "col" => legaia_asset::world_map_overlay::PolylineMode::ColumnMajor,
            "pairs" => legaia_asset::world_map_overlay::PolylineMode::PairWise,
            "grid" => legaia_asset::world_map_overlay::PolylineMode::Grid,
            _ => legaia_asset::world_map_overlay::PolylineMode::RowMajor,
        };
        let opts = legaia_asset::world_map_overlay::WireframeOptions {
            mode,
            axes: parse_axes(axes),
            ..Default::default()
        };
        let lines = legaia_asset::world_map_overlay::top_down_lines(&slot, &opts);

        let mut out = Vec::with_capacity(4 + lines.len() * 12);
        out.extend_from_slice(&(lines.len() as u32).to_le_bytes());
        for l in &lines {
            out.push(l.body_index);
            out.push((l.group_index & 0xFF) as u8);
            out.push((l.group_index >> 8) as u8);
            out.push(0); // pad
            out.extend_from_slice(&l.x0.to_le_bytes());
            out.extend_from_slice(&l.z0.to_le_bytes());
            out.extend_from_slice(&l.x1.to_le_bytes());
            out.extend_from_slice(&l.z1.to_le_bytes());
        }
        out
    }

    /// Decode the slot-4 world-map overlay as a topology-free point cloud.
    /// Useful when the on-disc draw-mode dispatch isn't fully reverse-
    /// engineered: the points themselves are byte-verified against live
    /// RAM, so plotting them straight is the most honest visualization.
    ///
    /// Output layout (little-endian):
    ///
    /// ```text
    /// [u32 point_count]
    /// [Point; point_count] ; 8 bytes each:
    ///     u8  body_index
    ///     u8  group_index_low
    ///     u8  group_index_high
    ///     u8  _pad
    ///     i16 x
    ///     i16 z
    /// ```
    pub fn slot4_wireframe_points(&self, prot_base: u32, axes: &str) -> Vec<u8> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let opts = legaia_asset::world_map_overlay::WireframeOptions {
            axes: parse_axes(axes),
            ..Default::default()
        };
        let pts = legaia_asset::world_map_overlay::record_points(&slot, &opts);

        let mut out = Vec::with_capacity(4 + pts.len() * 8);
        out.extend_from_slice(&(pts.len() as u32).to_le_bytes());
        for (body, group, x, z) in &pts {
            out.push(*body);
            out.push((*group & 0xFF) as u8);
            out.push((*group >> 8) as u8);
            out.push(0); // pad
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&z.to_le_bytes());
        }
        out
    }

    /// Bounding box of every non-zero record in the kingdom's slot-4
    /// wireframe, as `[amin, bmin, amax, bmax]` (i32) for the requested
    /// axis pair (`"xz"` / `"xy"` / `"zy"`, etc). Useful for re-framing
    /// the top-down camera when the overlay is toggled on. Empty vec
    /// when slot 4 can't be decoded.
    pub fn slot4_wireframe_bounds(&self, prot_base: u32, axes: &str) -> Vec<i32> {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return Vec::new();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return Vec::new();
        };
        let (ah, av) = parse_axes(axes);
        match legaia_asset::world_map_overlay::axis_bounds(&slot, ah, av) {
            Some((amin, bmin, amax, bmax)) => {
                vec![amin as i32, bmin as i32, amax as i32, bmax as i32]
            }
            None => Vec::new(),
        }
    }

    /// Per-body inventory of the slot-4 wireframe, as a JSON string.
    /// Used by the inspector panel to show which bodies are present.
    /// Returns `"[]"` when slot 4 can't be decoded.
    pub fn slot4_body_inventory_json(&self, prot_base: u32) -> String {
        let Some(decoded) = self.decode_kingdom_slot4(prot_base) else {
            return "[]".into();
        };
        let Ok(slot) = legaia_asset::world_map_overlay::parse(&decoded) else {
            return "[]".into();
        };
        let mut s = String::from("[");
        for (i, b) in slot.bodies.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (xlo, xhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::X,
            )
            .unwrap_or((0, 0));
            let (ylo, yhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::Y,
            )
            .unwrap_or((0, 0));
            let (zlo, zhi) = legaia_asset::world_map_overlay::body_axis_range(
                b,
                legaia_asset::world_map_overlay::Axis::Z,
            )
            .unwrap_or((0, 0));
            s.push_str(&format!(
                r#"{{"index":{},"count_a":{},"count_b":{},"flag_a":{},"flag_b":{},"kind":{},"records":{},"x":[{},{}],"y":[{},{}],"z":[{},{}]}}"#,
                b.index,
                b.count_a,
                b.count_b,
                b.flag_a,
                b.flag_b,
                b.kind,
                b.records.len(),
                xlo, xhi, ylo, yhi, zlo, zhi,
            ));
        }
        s.push(']');
        s
    }

    /// Decode the live PSX GPU primitive pool out of a mednafen save state
    /// and return per-vertex attribute arrays for replay in WebGL2 against
    /// the save state's VRAM.
    ///
    /// Pool location is per `legaia_mednafen::prim_pool::POOL_BASE_DEFAULT`
    /// (= `0x800AD400`, consistent across the Drake / Sebucus / Karisto
    /// top-view captures). Each accepted primitive (POLY_FT4, POLY_GT4,
    /// POLY_FT3, POLY_GT3, SPRT_16, SPRT_8) is expanded into two
    /// triangles in screen-space.
    ///
    /// Return layout (single packed `Vec<u8>`, little-endian, in this order):
    ///
    /// ```text
    /// [u16 vram_width = 1024]
    /// [u16 vram_height = 512]
    /// [u32 vram_byte_len = 1048576]
    /// [u8;  1048576] VRAM bytes (raw BGR555+STP halfwords)
    /// [u16 screen_w]
    /// [u16 screen_h]
    /// [u32 vertex_count]
    /// [Vertex; vertex_count]   ; struct, 14 bytes each:
    ///     i16 x, i16 y
    ///     u8  u, u8 v
    ///     u16 cba, u16 tsb
    ///     u8  r, u8 g, u8 b, u8 flags
    /// ```
    ///
    /// JSON dump of the world-map quick-travel menu parsed out of
    /// `SCUS_942.54` at disc-load time. Returns `null` if no disc was
    /// loaded as a Mode2/2352 image (raw PROT.DAT paths skip SCUS).
    ///
    /// Shape:
    /// ```json
    /// { "names": [..16 strings..],
    ///   "placements": [{ "index": u32, "name_idx": u8,
    ///                    "discovery_flag": u8, "scene_id": u16,
    ///                    "menu_x": u8, "menu_y": u8 }, ...] }
    /// ```
    pub fn worldmap_menu_json(&self) -> String {
        match &self.worldmap_menu {
            Some(menu) => serde_json::to_string(menu)
                .unwrap_or_else(|e| format!("{{\"error\":\"serialize failed: {e}\"}}")),
            None => "null".to_string(),
        }
    }

    /// Decode the global monster stat archive (PROT entry 867, the
    /// `battle_data` block's extended footprint) into a JSON array of every
    /// populated record. Sony bytes never leave the browser — the archive is
    /// LZS-decoded from the user's own loaded disc, the same client-side model
    /// the rest of this viewer uses; nothing is shipped with the static site.
    ///
    /// Shape:
    /// ```json
    /// { "records": [ { "id": u16, "name": "Gimard", "hp": u16, "mp": u16,
    ///                  "stats": [u16; 6], "magic_count": u8, "gold": u16,
    ///                  "exp": u16, "drop_item": u8, "drop_chance_pct": u8,
    ///                  "spells": [ { "id": u8, "sp_cost": u8,
    ///                               "castable": bool } ] }, ... ] }
    /// ```
    ///
    /// Returns `{"records":[]}` when the entry isn't present (a standalone-TIM
    /// or regional load that lacks PROT 867), or `{"error":...}` on a genuine
    /// LZS decode failure.
    pub fn monster_archive_json(&self) -> String {
        const MONSTER_ARCHIVE_INDEX: u32 = 867;
        let Some(meta) = parse_prot_toc(&self.disc)
            .and_then(|es| es.into_iter().find(|e| e.index == MONSTER_ARCHIVE_INDEX))
        else {
            return "{\"records\":[]}".to_string();
        };
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        if end > self.disc.len() {
            return "{\"records\":[]}".to_string();
        }
        let records = match legaia_asset::monster_archive::records(&self.disc[off..end]) {
            Ok(r) => r,
            Err(e) => return format!("{{\"error\":\"monster archive decode failed: {e}\"}}"),
        };
        // Resolve a raw drop id into its in-game name via the SCUS item-name
        // table (present only on full-disc loads). `null` falls the JS back to
        // the raw id.
        let drop_name = |id: u8| -> Option<&str> {
            (id != 0)
                .then(|| self.item_names.as_ref().and_then(|t| t.name(id)))
                .flatten()
        };
        // Resolve a monster's global magic-attack ids into named spells via the
        // SCUS spell table. Each entry carries the name + MP cost; `null` name
        // falls the JS back to the raw id.
        let magic_attacks = |ids: &[u8]| -> Vec<serde_json::Value> {
            ids.iter()
                .map(|&id| {
                    serde_json::json!({
                        "id": id,
                        "name": self.spell_names.as_ref().and_then(|t| t.name(id)),
                        "mp": self.spell_names.as_ref().and_then(|t| t.mp(id)),
                    })
                })
                .collect()
        };
        let arr: Vec<serde_json::Value> = records
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "name": r.name,
                    "hp": r.hp,
                    "mp": r.mp,
                    "stats": r.stats,
                    "magic_count": r.magic_count,
                    "gold": r.gold,
                    "exp": r.exp,
                    "drop_item": r.drop_item,
                    "drop_item_name": drop_name(r.drop_item),
                    "drop_chance_pct": r.drop_chance_pct,
                    "spells": r.spells.iter().map(|s| serde_json::json!({
                        "id": s.id,
                        "sp_cost": s.sp_cost,
                        "castable": s.is_castable(),
                    })).collect::<Vec<_>>(),
                    "magic": magic_attacks(&r.magic_attacks),
                })
            })
            .collect();
        serde_json::json!({ "records": arr }).to_string()
    }

    /// Slice of the disc holding the monster stat archive (PROT entry 867,
    /// extended footprint). Shared by [`Self::monster_archive_json`] and the
    /// per-monster mesh accessors.
    fn monster_archive_slice(&self) -> Option<&[u8]> {
        const MONSTER_ARCHIVE_INDEX: u32 = 867;
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == MONSTER_ARCHIVE_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    /// Build monster `id`'s embedded mesh (the Legaia TMD at archive-block
    /// `+0x04`) as a [`legaia_tmd::mesh::VramMesh`]. All monster prims are
    /// textured, so this keeps the full geometry with per-vertex UVs +
    /// CBA/TSB; the JS side textures it from the decoded pool (see
    /// [`Self::monster_texture_indices`]) and directional-lights it via the
    /// per-vertex normals. Returns `None` for a filler / out-of-range id.
    fn build_monster_mesh(&self, id: u16) -> Option<legaia_tmd::mesh::VramMesh> {
        let slice = self.monster_archive_slice()?;
        let mesh = legaia_asset::monster_archive::mesh(slice, id).ok()??;
        let tmd = legaia_tmd::parse(mesh.tmd_bytes()).ok()?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, mesh.tmd_bytes()))
    }

    /// Decode monster `id`'s texture pool (archive-block `+0x08`). `None` for a
    /// filler / out-of-range id or a slot with no pool.
    fn build_monster_texture(
        &self,
        id: u16,
    ) -> Option<legaia_asset::monster_archive::MonsterTexture> {
        let slice = self.monster_archive_slice()?;
        let mesh = legaia_asset::monster_archive::mesh(slice, id).ok()??;
        mesh.texture()
    }

    /// Per-vertex `[x, y, z]` positions for monster `id`'s mesh (flat array,
    /// 3 floats per vertex). Empty if the id has no mesh.
    pub fn monster_mesh_positions(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex smooth normals for monster `id`'s mesh (parallel to
    /// [`Self::monster_mesh_positions`]).
    pub fn monster_mesh_normals(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for monster `id`'s mesh (`u32`, multiple of 3).
    pub fn monster_mesh_indices(&self, id: u16) -> Vec<u32> {
        self.build_monster_mesh(id)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Bounding-sphere `[cx, cy, cz, r]` for monster `id`'s mesh, so the JS
    /// side can frame the model without re-parsing the geometry.
    pub fn monster_mesh_bounds(&self, id: u16) -> Vec<f32> {
        let Some(mesh) = self.build_monster_mesh(id) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let c = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let d = [
            (hi[0] - lo[0]) * 0.5,
            (hi[1] - lo[1]) * 0.5,
            (hi[2] - lo[2]) * 0.5,
        ];
        let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
        vec![c[0], c[1], c[2], r]
    }

    /// Per-vertex texture coords for monster `id`'s mesh, normalised to
    /// `[0, 1]` against the texture-page dimensions (parallel to
    /// [`Self::monster_mesh_positions`], 2 floats per vertex). Empty if the id
    /// has no mesh or no texture.
    pub fn monster_mesh_uvs(&self, id: u16) -> Vec<f32> {
        let (Some(mesh), Some(tex)) = (self.build_monster_mesh(id), self.build_monster_texture(id))
        else {
            return Vec::new();
        };
        // Texel-centre offset so the JS shader's NEAREST sample lands on the
        // intended texel rather than rounding to its neighbour at edges.
        let (w, h) = (tex.width as f32, tex.height as f32);
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[(uv[0] as f32 + 0.5) / w, (uv[1] as f32 + 0.5) / h]);
        }
        out
    }

    /// Per-vertex palette index (`cba & 0x3F`) for monster `id`'s mesh, as
    /// floats (parallel to [`Self::monster_mesh_positions`]). The JS shader
    /// uses it to pick the row of the palette texture.
    pub fn monster_mesh_palette_index(&self, id: u16) -> Vec<f32> {
        self.build_monster_mesh(id)
            .map(|m| m.cba_tsb.iter().map(|ct| (ct[0] & 0x3F) as f32).collect())
            .unwrap_or_default()
    }

    /// Monster `id`'s 4bpp texture page as one palette index (`0..=15`) per
    /// texel, row-major (`width * height` bytes). Upload as an `R8UI`/`R8`
    /// texture and pair with [`Self::monster_texture_palette_rgba`]. Empty if
    /// the id has no texture.
    pub fn monster_texture_indices(&self, id: u16) -> Vec<u8> {
        self.build_monster_texture(id)
            .map(|t| t.indices)
            .unwrap_or_default()
    }

    /// Monster `id`'s 15 palettes flattened to a `15 * 16` RGBA8 row (palette
    /// `p`, colour `c` at pixel `p * 16 + c`). Index-0 transparent colours
    /// carry alpha 0. Empty if the id has no texture.
    pub fn monster_texture_palette_rgba(&self, id: u16) -> Vec<u8> {
        self.build_monster_texture(id)
            .map(|t| t.palette_rgba())
            .unwrap_or_default()
    }

    /// `[width, height]` of monster `id`'s texture page in texels (128 or 256
    /// wide, always 256 tall). `[0, 0]` if the id has no texture.
    pub fn monster_texture_dims(&self, id: u16) -> Vec<u32> {
        self.build_monster_texture(id)
            .map(|t| vec![t.width as u32, t.height as u32])
            .unwrap_or_else(|| vec![0, 0])
    }

    /// Per-vertex TMD object (body-part) index for monster `id`'s mesh, parallel
    /// to [`Self::monster_mesh_positions`]. The JS idle-animation player uses it
    /// to apply each animated part's per-frame transform. Empty if no mesh.
    pub fn monster_mesh_object_ids(&self, id: u16) -> Vec<u32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(Some(mesh)) = legaia_asset::monster_archive::mesh(slice, id).ok() else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1
    }

    /// `[part_count, frame_count]` for monster `id`'s **idle** animation (action
    /// index 0). `[0, 0]` if the slot has no decodable animation. Pair with
    /// [`Self::monster_idle_animation_frames`].
    pub fn monster_idle_animation_header(&self, id: u16) -> Vec<u32> {
        let Some(slice) = self.monster_archive_slice() else {
            return vec![0, 0];
        };
        match legaia_asset::monster_archive::idle_animation(slice, id)
            .ok()
            .flatten()
        {
            Some(a) => vec![a.part_count as u32, a.frame_count as u32],
            None => vec![0, 0],
        }
    }

    /// Monster `id`'s idle animation keyframes as a flat `i32` array, six values
    /// per part per frame: `[tx, ty, tz, rx, ry, rz]`. Frame `f`, part `p`,
    /// component `c` is at `(f * part_count + p) * 6 + c`. Translations are
    /// signed model units; rotations are unsigned 12-bit angles (`4096` = a full
    /// turn). Empty if the slot has no decodable idle animation.
    pub fn monster_idle_animation_frames(&self, id: u16) -> Vec<i32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(anim) = legaia_asset::monster_archive::idle_animation(slice, id)
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(anim.frame_count * anim.part_count * 6);
        for frame in &anim.frames {
            for p in frame {
                out.extend_from_slice(&[
                    p.tx as i32,
                    p.ty as i32,
                    p.tz as i32,
                    p.rx as i32,
                    p.ry as i32,
                    p.rz as i32,
                ]);
            }
        }
        out
    }

    /// Metadata for **every** decodable action animation of monster `id`, as a
    /// JSON array in `+0x4C` action-table order:
    /// `[{"action_id":N,"part_count":P,"frame_count":F}, ...]`. Array index `0`
    /// is the idle loop (see [`Self::monster_idle_animation_header`]); the rest
    /// are the monster's attack / spell / special actions. The array index is
    /// the handle the JS viewer passes to [`Self::monster_animation_frames_at`]
    /// to fetch a given action's keyframes. `"[]"` if the slot is empty / filler
    /// or carries no decodable animation.
    pub fn monster_animations_json(&self, id: u16) -> String {
        let Some(slice) = self.monster_archive_slice() else {
            return "[]".to_string();
        };
        let anims = match legaia_asset::monster_archive::animations(slice, id) {
            Ok(Some(a)) => a,
            _ => return "[]".to_string(),
        };
        let labels = legaia_asset::monster_archive::action_labels(&anims);
        let arr: Vec<serde_json::Value> = anims
            .iter()
            .enumerate()
            .map(|(i, a)| {
                serde_json::json!({
                    "action_id": a.action_id,
                    "part_count": a.part_count,
                    "frame_count": a.frame_count,
                    "label": labels.get(i),
                })
            })
            .collect();
        serde_json::Value::Array(arr).to_string()
    }

    /// Keyframes for monster `id`'s action animation at array `index` (the
    /// position in [`Self::monster_animations_json`]). Same flat layout as
    /// [`Self::monster_idle_animation_frames`]: six `i32` per part per frame,
    /// `[tx, ty, tz, rx, ry, rz]`, with frame `f` / part `p` / component `c` at
    /// `(f * part_count + p) * 6 + c`. Empty if the index is out of range or the
    /// slot has no decodable animation.
    pub fn monster_animation_frames_at(&self, id: u16, index: u32) -> Vec<i32> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        let Some(anims) = legaia_asset::monster_archive::animations(slice, id)
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
        let Some(anim) = anims.get(index as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(anim.frame_count * anim.part_count * 6);
        for frame in &anim.frames {
            for p in frame {
                out.extend_from_slice(&[
                    p.tx as i32,
                    p.ty as i32,
                    p.tz as i32,
                    p.rx as i32,
                    p.ry as i32,
                    p.rz as i32,
                ]);
            }
        }
        out
    }

    /// Monster `id`'s mesh + baked texture + **all** action animations packed
    /// into one binary glTF (`.glb`) blob — the universal format that carries
    /// geometry, material, and animation together (Blender / three.js / etc.).
    /// Each TMD object becomes an animated node; the texture is baked into a
    /// per-palette atlas. Empty if the slot has no exportable mesh.
    pub fn monster_glb(&self, id: u16) -> Vec<u8> {
        let Some(slice) = self.monster_archive_slice() else {
            return Vec::new();
        };
        legaia_asset::monster_gltf::export_glb(slice, id)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Player-character pack (PROT 0874 §0) — Vahn / Noa / Gala + 2 auxiliary
    //
    // Sister accessors of `monster_*`: surface the five character TMDs the
    // engine keeps resident at `DAT_8007C018[0..=4]`. The active-party slots
    // expose the `FUN_8001EBEC` equipment swap so the JS viewer can flip the
    // visible weapon-bearing group descriptor in place.
    // -----------------------------------------------------------------------

    /// JSON summary of the five character-pack slots.
    ///
    /// Shape:
    /// ```json
    /// { "slots": [
    ///     { "slot": 0, "label": "Vahn", "disc_nobj": 12,
    ///       "tmd_bytes": 13220,
    ///       "patch": { "patched_group_index": 0,
    ///                  "equip_byte_record_offset": 406 } },
    ///     ...
    ///   ],
    ///   "patched_group_offset": 12,
    ///   "group_descriptor_bytes": 28,
    ///   "equip_group_zero_offset": 320,
    ///   "equip_group_nonzero_offset": 292
    /// }
    /// ```
    /// `patch` is present only for the 3 active-party slots (0..=2); slots
    /// 3/4 carry the auxiliary actors with no equipment swap. Returns
    /// `{"slots":[],"error":"..."}` when the disc is missing PROT 0874 or
    /// the LZS section fails to decode.
    pub fn character_pack_json(&self) -> String {
        let Some(slice) = self.character_pack_slice() else {
            return r#"{"slots":[]}"#.to_string();
        };
        let pack = match legaia_asset::character_pack::parse(slice) {
            Ok(p) => p,
            Err(e) => {
                return format!(r#"{{"slots":[],"error":"character pack: {e}"}}"#);
            }
        };
        let active = legaia_asset::character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;
        let slots: Vec<serde_json::Value> = pack
            .slots()
            .iter()
            .map(|s| {
                let patch = active
                    .iter()
                    .find(|p| (p.slot as usize) == s.slot)
                    .map(|p| {
                        serde_json::json!({
                            "patched_group_index": p.patched_group_index,
                            "equip_byte_record_offset": p.equip_byte_record_offset,
                        })
                    });
                serde_json::json!({
                    "slot": s.slot,
                    "label": legaia_asset::character_pack::slot_label(s.slot),
                    "disc_nobj": s.disc_nobj,
                    "tmd_bytes": s.tmd_bytes.len(),
                    "patch": patch,
                })
            })
            .collect();
        serde_json::json!({
            "slots": slots,
            "first_group_descriptor_offset":
                legaia_asset::character_pack::FIRST_GROUP_DESCRIPTOR_OFFSET,
            "group_descriptor_bytes":
                legaia_asset::character_pack::GROUP_DESCRIPTOR_BYTES,
            "equip_group_zero_offset":
                legaia_asset::character_pack::EQUIP_GROUP_ZERO_OFFSET,
            "equip_group_nonzero_offset":
                legaia_asset::character_pack::EQUIP_GROUP_NONZERO_OFFSET,
        })
        .to_string()
    }

    /// Slice of the disc holding PROT 0874 (`befect_data` extended footprint).
    /// Shared by every `character_*` accessor.
    fn character_pack_slice(&self) -> Option<&[u8]> {
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == legaia_asset::character_pack::PROT_ENTRY_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    /// Build slot `slot`'s renderable mesh, optionally with the equipment
    /// swap applied. `equip` of `None` returns the disc-form mesh
    /// (with retail's 10-group cap applied so groups 10/11 templates aren't
    /// drawn directly); `Some(byte)` runs the FUN_8001EBEC patch with that
    /// byte before parsing.
    fn build_character_mesh(
        &self,
        slot: usize,
        equip: Option<u8>,
    ) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let raw = self.character_pack_slice()?;
        let pack = legaia_asset::character_pack::parse(raw).ok()?;
        let cslot = pack.slot(slot)?;
        // Retail caps the active-party slots at 10 live groups (FUN_8001E890);
        // mirror that so groups 10/11 (the equip templates) aren't drawn as
        // visible geometry. The patched copy still contains the templates at
        // their disc offsets but `parse` walks only the first `nobj`.
        let mut tmd_bytes = if let Some(equip_byte) = equip {
            let active = legaia_asset::character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;
            if let Some(patch) = active.iter().find(|p| (p.slot as usize) == slot) {
                legaia_asset::character_pack::equipment_swap::apply(
                    &cslot.tmd_bytes,
                    *patch,
                    equip_byte,
                )
            } else {
                cslot.tmd_bytes.clone()
            }
        } else {
            cslot.tmd_bytes.clone()
        };
        if cslot.is_active_party() && tmd_bytes.len() >= 0x0C {
            // Overwrite TMD header `nobj` to 10 — the retail cap.
            let cap = 10u32.to_le_bytes();
            tmd_bytes[0x08..0x0C].copy_from_slice(&cap);
        }
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        Some((tmd, tmd_bytes))
    }

    /// Convenience: return the renderable `VramMesh` for slot `slot` under
    /// the chosen equipment toggle.
    fn build_character_vram_mesh(
        &self,
        slot: usize,
        equip: Option<u8>,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, bytes) = self.build_character_mesh(slot, equip)?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes))
    }

    /// Per-vertex positions for the player character at pack slot `slot`,
    /// optionally with the equipment swap applied (`equip_byte` < 0 means
    /// "no swap, draw disc-form mesh"). Empty if `slot` is out of range or
    /// the disc isn't loaded.
    pub fn character_mesh_positions(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex normals parallel to [`Self::character_mesh_positions`].
    pub fn character_mesh_normals(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for the player character at pack slot `slot`,
    /// `u32`, multiple of 3.
    pub fn character_mesh_indices(&self, slot: u32, equip_byte: i32) -> Vec<u32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        self.build_character_vram_mesh(slot as usize, equip)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` integer texel coords (parallel to
    /// [`Self::character_mesh_positions`], 2 i32 per vertex). The site page
    /// pairs these with the PROT 0876 atlas page to do its own NEAREST
    /// sample; we keep the integer texels here instead of normalising
    /// because the atlas dimensions aren't surfaced yet.
    pub fn character_mesh_uvs(&self, slot: u32, equip_byte: i32) -> Vec<i32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (CLUT-base / texture-page descriptor) so the
    /// JS shader can resolve VRAM texel + palette per the standard PSX TMD
    /// model. `2 u32` per vertex, parallel to [`Self::character_mesh_positions`].
    pub fn character_mesh_cba_tsb(&self, slot: u32, equip_byte: i32) -> Vec<u32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Bounding-sphere `[cx, cy, cz, r]` so the JS viewer can frame the model.
    /// Uses [`centroid_bounds`] so asymmetric poses (weapon extended, arm out)
    /// don't pull the camera target off the body.
    pub fn character_mesh_bounds(&self, slot: u32, equip_byte: i32) -> Vec<f32> {
        let equip = (equip_byte >= 0).then_some(equip_byte as u8);
        let Some(mesh) = self.build_character_vram_mesh(slot as usize, equip) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&mesh.positions)
    }

    /// Raw disc-form TMD bytes for slot `slot` — the same bytes the engine
    /// installs into `DAT_8007C018[slot]`. Useful for an in-page .tmd
    /// download / debug round-trip.
    pub fn character_tmd_bytes(&self, slot: u32) -> Vec<u8> {
        let Some(raw) = self.character_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::character_pack::parse(raw) else {
            return Vec::new();
        };
        pack.slot(slot as usize)
            .map(|s| s.tmd_bytes.clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Battle-form character pack — PROT 1204 (`other5`).
    //
    // Sister pack to the field-form one above. Same 5-slot shape, but
    // higher-fidelity battle TMDs (typical disc-nobj 15/16/15 vs 12/12/12)
    // and an explicit 7-atlas trailer (256x256 4bpp TIMs at fixed stride).
    // ------------------------------------------------------------------

    /// JSON summary of PROT 1204 (`other5`) — the battle-form mesh pack:
    /// 5 TMD slots + 7 character-atlas TIMs. Shape:
    /// ```text
    /// {
    ///   "slots":   [{"slot":0,"label":"Vahn","disc_nobj":15,"tmd_bytes":33516,"file_offset":4}, ...],
    ///   "atlases": [{"atlas":0,"clut_fb_y":490,"tim_bytes":33316,"file_offset":154628}, ...],
    ///   "atlas_stride_bytes": 33316,
    ///   "first_atlas_offset": 154628
    /// }
    /// ```
    pub fn battle_char_pack_json(&self) -> String {
        let Some(slice) = self.battle_char_pack_slice() else {
            return r#"{"slots":[],"atlases":[]}"#.to_string();
        };
        let pack = match legaia_asset::battle_char_pack::parse(slice) {
            Ok(p) => p,
            Err(e) => {
                return format!(r#"{{"slots":[],"atlases":[],"error":"battle char pack: {e}"}}"#);
            }
        };
        let slots: Vec<serde_json::Value> = pack
            .slots()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "slot": s.slot,
                    "label": legaia_asset::battle_char_pack::slot_label(s.slot),
                    "disc_nobj": s.disc_nobj,
                    "tmd_bytes": s.tmd_bytes.len(),
                    "file_offset": s.file_offset,
                })
            })
            .collect();
        let atlases: Vec<serde_json::Value> = pack
            .atlases
            .iter()
            .map(|a| {
                serde_json::json!({
                    "atlas": a.atlas_index,
                    "clut_fb_y": a.clut_fb_y,
                    "tim_bytes": a.tim_bytes.len(),
                    "file_offset": a.file_offset,
                })
            })
            .collect();
        serde_json::json!({
            "slots": slots,
            "atlases": atlases,
            "atlas_stride_bytes": legaia_asset::battle_char_pack::ATLAS_STRIDE_BYTES,
            "first_atlas_offset": legaia_asset::battle_char_pack::FIRST_ATLAS_OFFSET,
        })
        .to_string()
    }

    fn battle_char_pack_slice(&self) -> Option<&[u8]> {
        let meta = parse_prot_toc(&self.disc)?
            .into_iter()
            .find(|e| e.index == legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)?;
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        self.disc.get(off..end)
    }

    fn build_battle_char_mesh(&self, slot: usize) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let raw = self.battle_char_pack_slice()?;
        let pack = legaia_asset::battle_char_pack::parse(raw).ok()?;
        let cslot = pack.slot(slot)?;
        let tmd_bytes = cslot.tmd_bytes.clone();
        let tmd = legaia_tmd::parse(&tmd_bytes).ok()?;
        Some((tmd, tmd_bytes))
    }

    fn build_battle_char_vram_mesh(&self, slot: usize) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, bytes) = self.build_battle_char_mesh(slot)?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &bytes))
    }

    /// Per-vertex positions for the battle-form character at pack slot `slot`.
    pub fn battle_char_mesh_positions(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex normals for the battle-form character at slot `slot`.
    pub fn battle_char_mesh_normals(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.normals.len() * 3);
        for n in &mesh.normals {
            out.extend_from_slice(&[n[0], n[1], n[2]]);
        }
        out
    }

    /// Triangle indices for the battle-form character at slot `slot`.
    pub fn battle_char_mesh_indices(&self, slot: u32) -> Vec<u32> {
        self.build_battle_char_vram_mesh(slot as usize)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]` integer texel coords for the battle-form character.
    pub fn battle_char_mesh_uvs(&self, slot: u32) -> Vec<i32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` for the battle-form character.
    pub fn battle_char_mesh_cba_tsb(&self, slot: u32) -> Vec<u32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Bounding-sphere `[cx, cy, cz, r]` for the battle-form character.
    /// Uses the **vertex centroid** (mean position) rather than the AABB
    /// midpoint, so asymmetric poses (e.g. Vahn's stance with the weapon
    /// extended past the body's X axis) don't pull the camera target off the
    /// torso. Radius is the max distance from the centroid to any vertex.
    pub fn battle_char_mesh_bounds(&self, slot: u32) -> Vec<f32> {
        let Some(mesh) = self.build_battle_char_vram_mesh(slot as usize) else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        centroid_bounds(&mesh.positions)
    }

    /// Raw disc-form TMD bytes for battle-form slot `slot`.
    pub fn battle_char_tmd_bytes(&self, slot: u32) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        pack.slot(slot as usize)
            .map(|s| s.tmd_bytes.clone())
            .unwrap_or_default()
    }

    /// Raw TIM bytes for battle-form atlas `atlas` (0..=6). 256x256 4bpp with
    /// a 256x1 sub-CLUT row inside the TIM block.
    pub fn battle_char_atlas_bytes(&self, atlas: u32) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        pack.atlas(atlas as usize)
            .map(|a| a.tim_bytes.clone())
            .unwrap_or_default()
    }

    /// Build the 1 MB PSX VRAM the battle-form character pack would have
    /// at boot — each of the seven atlas TIMs uploaded at its declared
    /// `(fb_x, fb_y)`. Returns the raw 1024×512×2 byte blob suitable for
    /// `TmdRenderer.uploadVram`. Empty if PROT 1204 is absent or any atlas
    /// fails to parse. Mirrors [`Self::current_vram_bytes`] but specialized
    /// to the battle character atlas pack.
    pub fn battle_char_vram_bytes(&self) -> Vec<u8> {
        let Some(raw) = self.battle_char_pack_slice() else {
            return Vec::new();
        };
        let Ok(pack) = legaia_asset::battle_char_pack::parse(raw) else {
            return Vec::new();
        };
        let mut vram = legaia_tim::Vram::new();
        for atlas in &pack.atlases {
            if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                vram.upload_tim(&tim);
            }
        }
        vram.as_bytes().to_vec()
    }

    // ------------------------------------------------------------------
    // Player ANM bundles — per-scene asset bundle, section 2, type 0x05
    // ("MOVE" label but canonical ANM content with marker_1 = 0x080C).
    // See `legaia_asset::player_anm` + docs/formats/anm.md.
    // ------------------------------------------------------------------

    /// JSON summary of every player-ANM bundle accessible from this disc.
    /// Shape:
    /// ```text
    /// {
    ///   "bundles": [
    ///     {
    ///       "prot_index": 4,
    ///       "record_count": 69,
    ///       "decoded_bytes": 96448,
    ///       "records": [
    ///         { "index": 0, "offset": 0x118, "size": 496, "marker_1": 0x080C },
    ///         ...
    ///       ]
    ///     }, ...
    ///   ]
    /// }
    /// ```
    /// Surveys the corpus by walking each scene's first PROT slot
    /// (parse_player_lzs descriptor count = 6, the canonical scene-bundle
    /// shape) and emitting one entry per cleanly-decoded type-0x05 section.
    pub fn player_anm_corpus_json(&self) -> String {
        let toc = match parse_prot_toc(&self.disc) {
            Some(t) => t,
            None => return r#"{"bundles":[],"error":"no PROT TOC"}"#.to_string(),
        };
        let mut bundles: Vec<serde_json::Value> = Vec::new();
        for meta in &toc {
            let off = meta.byte_offset as usize;
            let end = off.saturating_add(meta.size_bytes as usize);
            let Some(buf) = self.disc.get(off..end) else {
                continue;
            };
            // The vast majority of scene bundles use 6 descriptors; that's the
            // detector spread the disc-gated test pins. Lower counts catch a
            // handful of `befect_data` / `other5` variants.
            for desc_count in [6, 3, 5, 7] {
                let found = legaia_asset::player_anm::find_in_entry(buf, desc_count);
                if !found.is_empty() {
                    for b in found {
                        let recs: Vec<serde_json::Value> = (0..b.record_count as usize)
                            .map(|i| {
                                let bytes = b.record_bytes(i);
                                serde_json::json!({
                                    "index": i,
                                    "offset": b.record_offsets[i],
                                    "size": bytes.len(),
                                    "marker_1": b.record_marker_1(i).unwrap_or(0),
                                })
                            })
                            .collect();
                        bundles.push(serde_json::json!({
                            "prot_index": meta.index,
                            "record_count": b.record_count,
                            "decoded_bytes": b.decoded.len(),
                            "records": recs,
                        }));
                    }
                    break;
                }
            }
        }
        serde_json::json!({ "bundles": bundles }).to_string()
    }

    /// Find a single player-ANM bundle by its PROT entry index and return
    /// the LZS-decoded bytes. Empty if the entry doesn't carry a bundle.
    pub fn player_anm_decoded(&self, prot_index: u32) -> Vec<u8> {
        let toc = match parse_prot_toc(&self.disc) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let Some(meta) = toc.into_iter().find(|e| e.index == prot_index) else {
            return Vec::new();
        };
        let off = meta.byte_offset as usize;
        let end = off.saturating_add(meta.size_bytes as usize);
        let Some(buf) = self.disc.get(off..end) else {
            return Vec::new();
        };
        for desc_count in [6, 3, 5, 7] {
            let found = legaia_asset::player_anm::find_in_entry(buf, desc_count);
            if let Some(b) = found.into_iter().next() {
                return b.decoded;
            }
        }
        Vec::new()
    }

    /// Raw bytes of one record from the player-ANM bundle at `prot_index`.
    /// Includes the per-record header (`marker_1 = 0x080C`, flag, …) plus
    /// the per-bone keyframe data following it.
    pub fn player_anm_record_bytes(&self, prot_index: u32, record_index: u32) -> Vec<u8> {
        let decoded = self.player_anm_decoded(prot_index);
        let Ok(bundle) = legaia_asset::player_anm::parse(&decoded) else {
            return Vec::new();
        };
        bundle.record_bytes(record_index as usize).to_vec()
    }

    /// Fog LUT bytes extracted from `SCUS_942.54` at disc-load time.
    /// 4 KiB = 2048 u16 BGR555-shaped entries that the world-map overlay's
    /// per-prim leaves at `0x801F7644..0x801F8690` consult on every vertex
    /// (the shared depth-cue ramp; the per-kingdom hue mixes in from the
    /// `fog_color` field at gp-0x2DC).
    ///
    /// Returns an empty Vec when no LUT was located - the JS side should
    /// treat empty as "fall back to the kingdom-tinted baseline" and not
    /// upload anything to the renderer.
    pub fn fog_lut_bytes(&self) -> Vec<u8> {
        self.fog_lut.clone().unwrap_or_default()
    }

    /// `flags` packs the prim cmd-byte mode bits: bit 0 = semi-transparent,
    /// bit 1 = raw texture (skip color modulation). JS computes the model-view
    /// matrix from `screen_w / screen_h` (orthographic 0..w x h..0 viewport).
    pub fn save_state_prim_replay(&self, save_state_bytes: Vec<u8>) -> Result<Vec<u8>, JsValue> {
        let save = legaia_mednafen::container::SaveState::from_compressed(&save_state_bytes)
            .map_err(|e| JsValue::from_str(&format!("parse save state: {e}")))?;
        let gpu = legaia_mednafen::gpu::PsxGpu::new(&save);
        let vram = gpu
            .vram_bytes()
            .ok_or_else(|| JsValue::from_str("save state has no GPU/VRAM section"))?;
        let regs = gpu.regs();
        let dot_clock_div = match regs.display_mode_raw.unwrap_or(0) & 0x07 {
            0 => 10,
            1 => 8,
            2 => 5,
            3 => 4,
            _ => 7,
        };
        let screen_w = match regs.display_h_range {
            Some((hs, he)) => (he.saturating_sub(hs) / dot_clock_div).clamp(256, 640) as u16,
            None => 320,
        };
        let screen_h = match regs.display_v_range {
            Some((vs, ve)) => ve.saturating_sub(vs).max(224) as u16,
            None => 240,
        };
        // Slice the prim pool out of main RAM (kuseg-relative).
        let ram = save
            .main_ram()
            .map_err(|e| JsValue::from_str(&format!("read main RAM: {e}")))?;
        let pool_start_kuseg = legaia_mednafen::prim_pool::POOL_BASE_DEFAULT;
        let pool_end_kuseg = 0x80102000u32;
        let pool_lo = (pool_start_kuseg - 0x8000_0000) as usize;
        let pool_hi = (pool_end_kuseg - 0x8000_0000) as usize;
        let pool = &ram[pool_lo..pool_hi.min(ram.len())];
        let prims = legaia_mednafen::prim_pool::decode(pool, pool_start_kuseg);

        // Build vertex array. Two triangles per quad prim. Three vertices
        // for tri prims (zero-area duplicate to keep stride uniform).
        // Sprites expand to a quad based on their fixed cmd-byte size.
        let mut verts: Vec<u8> = Vec::with_capacity(prims.len() * 14 * 6);
        let push_vertex = |verts: &mut Vec<u8>,
                           x: i16,
                           y: i16,
                           u: u8,
                           v: u8,
                           cba: u16,
                           tsb: u16,
                           color: [u8; 3],
                           flags: u8| {
            verts.extend_from_slice(&x.to_le_bytes());
            verts.extend_from_slice(&y.to_le_bytes());
            verts.push(u);
            verts.push(v);
            verts.extend_from_slice(&cba.to_le_bytes());
            verts.extend_from_slice(&tsb.to_le_bytes());
            verts.extend_from_slice(&color);
            verts.push(flags);
        };
        let flags_for = |cmd: u8| -> u8 {
            let mut f = 0u8;
            if cmd & 0x02 != 0 {
                f |= 0x01;
            } // semi-transparent
            if cmd & 0x01 != 0 {
                f |= 0x02;
            } // raw texture
            f
        };
        for p in &prims {
            match *p {
                legaia_mednafen::prim_pool::Prim::PolyFt4 {
                    cmd,
                    color,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    // PSX winding: (v0, v1, v2) + (v1, v3, v2)
                    for &i in &[0usize, 1, 2, 1, 3, 2] {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyGt4 {
                    cmd,
                    colors,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for &i in &[0usize, 1, 2, 1, 3, 2] {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyFt3 {
                    cmd,
                    color,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                    // Pad to 6-vertex stride so JS can stream draw as TRIANGLES.
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, color, fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::PolyGt3 {
                    cmd,
                    colors,
                    verts: v,
                    uvs,
                    clut,
                    tpage,
                } => {
                    let fl = flags_for(cmd);
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                    for i in 0..3 {
                        push_vertex(
                            &mut verts, v[i].0, v[i].1, uvs[i].0, uvs[i].1, clut, tpage, colors[i],
                            fl,
                        );
                    }
                }
                legaia_mednafen::prim_pool::Prim::Sprt16 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                } => {
                    sprite_to_quad(&mut verts, cmd, color, pos, uv, clut, 16);
                }
                legaia_mednafen::prim_pool::Prim::Sprt8 {
                    cmd,
                    color,
                    pos,
                    uv,
                    clut,
                } => {
                    sprite_to_quad(&mut verts, cmd, color, pos, uv, clut, 8);
                }
            }
        }
        let vertex_count = (verts.len() / 14) as u32;

        // Pack the output buffer.
        let mut out = Vec::with_capacity(2 + 2 + 4 + vram.len() + 2 + 2 + 4 + verts.len());
        out.extend_from_slice(&1024u16.to_le_bytes()); // vram_width
        out.extend_from_slice(&512u16.to_le_bytes()); // vram_height
        out.extend_from_slice(&(vram.len() as u32).to_le_bytes());
        out.extend_from_slice(vram);
        out.extend_from_slice(&screen_w.to_le_bytes());
        out.extend_from_slice(&screen_h.to_le_bytes());
        out.extend_from_slice(&vertex_count.to_le_bytes());
        out.extend_from_slice(&verts);
        Ok(out)
    }

    /// Parse a mednafen save state and return the GPU's currently-displayed
    /// framebuffer as an RGBA8 byte buffer + dimensions.
    ///
    /// Layout of the returned `Vec<u8>`:
    /// `[u16 width, u16 height, RGBA8 pixels...]` packed little-endian. JS
    /// reads the leading 4 bytes for the dimensions and then wraps the rest
    /// in an `ImageData` to blit into a 2D canvas.
    ///
    /// This is the in-game top-down world-map view: the game's renderer has
    /// already composed the ~10,000 textured polygons that form the kingdom
    /// terrain, and the result is sitting in VRAM at the display-area
    /// offset. We just read it back. Source-mesh reconstruction is a separate
    /// follow-up (the live PSX GPU prim-pool sits around `0x800AD408` and
    /// the underlying mesh / tilemap data lives in the kingdom's
    /// `scene_v12_table` at PROT base+8 - both still being characterised).
    pub fn save_state_framebuffer(&self, save_state_bytes: Vec<u8>) -> Result<Vec<u8>, JsValue> {
        let save = legaia_mednafen::container::SaveState::from_compressed(&save_state_bytes)
            .map_err(|e| JsValue::from_str(&format!("parse save state: {e}")))?;
        let gpu = legaia_mednafen::gpu::PsxGpu::new(&save);
        let vram = gpu
            .vram_bytes()
            .ok_or_else(|| JsValue::from_str("save state has no GPU/VRAM section"))?;
        let regs = gpu.regs();
        // Display-area X/Y inside VRAM (defaults if regs absent: top-left).
        let (fb_x, fb_y) = regs.display_fb.unwrap_or((0, 0));
        // Display window width is derived from horizontal range; the standard
        // PSX 320x224 / 384x240 dot-clocks land around (608..3168) ~= 320 wide
        // at 7MHz, or (488..3288) ~= 384 wide at 5MHz, divided by the clock
        // ticks per pixel. Mednafen records `HorizStart/HorizEnd` in dot-clock
        // ticks. The simplest robust extraction is to compute the active
        // pixel count via the difference, falling back to 320x240 if the
        // registers are missing.
        let dot_clock_div = match regs.display_mode_raw.unwrap_or(0) & 0x07 {
            0 => 10, // 256
            1 => 8,  // 320
            2 => 5,  // 512
            3 => 4,  // 640
            _ => 7,  // 384 (mode bit 6)
        };
        let width = match (regs.display_h_range, regs.display_mode_raw) {
            (Some((hs, he)), _) => {
                let span = he.saturating_sub(hs);
                (span / dot_clock_div).clamp(256, 640) as u16
            }
            _ => 320,
        };
        let height = match (regs.display_v_range, regs.display_mode_raw) {
            (Some((vs, ve)), _) => (ve.saturating_sub(vs) as u16).max(224),
            _ => 240,
        };
        let w = width as usize;
        let h = height as usize;
        let mut out = Vec::with_capacity(4 + w * h * 4);
        out.push((width & 0xFF) as u8);
        out.push((width >> 8) as u8);
        out.push((height & 0xFF) as u8);
        out.push((height >> 8) as u8);
        // VRAM is 1024×512 BGR555 (2 bytes/pixel). Crop the (fb_x, fb_y)
        // window. Wrap around the right edge if necessary (PSX VRAM is
        // logically a torus on the X axis at 1024 pixels).
        for row in 0..h {
            let vy = (fb_y as usize + row) & 0x1FF;
            for col in 0..w {
                let vx = (fb_x as usize + col) & 0x3FF;
                let off = (vy * 1024 + vx) * 2;
                let word = u16::from_le_bytes([vram[off], vram[off + 1]]);
                let [r, g, b, _a] = legaia_mednafen::gpu::bgr555_to_rgba8(word);
                out.push(r);
                out.push(g);
                out.push(b);
                out.push(0xFF); // opaque
            }
        }
        Ok(out)
    }

    /// True if the current entry has a parseable TMD, suitable for the 3D
    /// rendering path. JS uses this to decide whether to switch to the 3D
    /// render loop instead of the TIM blit.
    pub fn current_has_tmd(&self) -> bool {
        self.viewable
            .get(self.current)
            .map(|e| e.tmd_source.is_some())
            .unwrap_or(false)
    }

    /// JSON-encoded summary of the current entry - class label, byte size,
    /// MES record count (if any), SEQ presence (if any), VAB presence
    /// (if any). The JS side parses this and shows it in the inspector
    /// panel without needing N round-trips for each individual field.
    pub fn current_entry_info_json(&self) -> String {
        let Some(entry) = self.viewable.get(self.current) else {
            return "{}".into();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        let buf: &[u8] = if end <= self.disc.len() {
            &self.disc[off..end]
        } else {
            &[]
        };
        let class = format!("{:?}", entry.class);
        let mes = legaia_mes::parse(buf).ok();
        let mes_format = mes.as_ref().map(|m| format!("{:?}", m.format));
        let mes_records = mes.as_ref().and_then(|m| m.records.as_ref().map(Vec::len));
        let mes_offsets = mes
            .as_ref()
            .and_then(|m| m.offset_table.as_ref().map(Vec::len));
        let seq_off = buf.windows(4).position(|w| w == b"pQES");
        let vab_off = buf.windows(4).position(|w| w == b"pBAV");
        let tim_count = entry.tim_count;
        let prot_idx = entry.meta.index;

        // Hand-rolled JSON to keep wasm size down (no serde_json on this
        // path - the data is fixed-shape).
        let mut s = String::new();
        s.push('{');
        s.push_str(&format!(r#""prot_index":{prot_idx},"#));
        s.push_str(&format!(r#""size_bytes":{},"#, buf.len()));
        s.push_str(&format!(r#""class":"{class}","#));
        s.push_str(&format!(r#""tim_count":{tim_count},"#));
        s.push_str(&format!(r#""has_tmd":{},"#, entry.tmd_source.is_some()));
        if let Some(off) = vab_off {
            s.push_str(&format!(r#""vab_offset":{off},"#));
        }
        if let Some(off) = seq_off {
            s.push_str(&format!(r#""seq_offset":{off},"#));
            // Try parsing the SEQ header for the JS-visible BPM display.
            if let Ok(hdr) = legaia_seq::parse_header(&buf[off..]) {
                s.push_str(&format!(r#""seq_ppqn":{},"#, hdr.ppqn));
                s.push_str(&format!(r#""seq_bpm":{:.1},"#, hdr.bpm()));
            }
        }
        if let Some(fmt) = mes_format {
            s.push_str(&format!(r#""mes_format":"{fmt}","#));
        }
        if let Some(n) = mes_records {
            s.push_str(&format!(r#""mes_records":{n},"#));
        }
        if let Some(n) = mes_offsets {
            s.push_str(&format!(r#""mes_offsets":{n},"#));
        }
        // Trim trailing comma if present.
        if s.ends_with(',') {
            s.pop();
        }
        s.push('}');
        s
    }

    /// JSON metadata for the boot publisher-logo TIMs from PROT 0895
    /// (`init.pak`). Returns an empty array `"[]"` if the disc doesn't
    /// have PROT 0895 or the entry doesn't parse as init.pak.
    ///
    /// Each element shape:
    ///   `{ "name": str, "width": u32, "height": u32, "mode": u32,
    ///      "fb_x": u32, "fb_y": u32 }`
    pub fn init_pak_logos_json(&self) -> String {
        let bytes = match self.prot_0895_bytes() {
            Some(b) => b,
            None => return "[]".into(),
        };
        let pak = match legaia_asset::init_pak::parse(bytes) {
            Ok(p) => p,
            Err(_) => return "[]".into(),
        };
        const NAMES: [&str; 4] = ["PROKION", "Contrail", "SCEA", "WARNING"];
        let mut out = String::from("[");
        for (i, logo) in pak.logos.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            // Decode TIM to get the bpp-corrected pixel dimensions.
            let (w, h) = match legaia_tim::parse(logo.bytes) {
                Ok(t) => (t.pixel_width() as u32, t.image.h as u32),
                Err(_) => (logo.pixel_rect.2 as u32 * 2, logo.pixel_rect.3 as u32),
            };
            out.push_str(&format!(
                r#"{{"name":"{}","width":{},"height":{},"mode":{},"fb_x":{},"fb_y":{}}}"#,
                NAMES[i], w, h, logo.mode, logo.pixel_rect.0, logo.pixel_rect.1,
            ));
        }
        out.push(']');
        out
    }

    /// Decoded RGBA8 pixels for one publisher-logo TIM (0..3). Returns
    /// an empty vec when the disc doesn't have PROT 0895 or `idx` is
    /// out of range. Width / height come from [`init_pak_logos_json`].
    pub fn init_pak_logo_rgba(&self, idx: u32) -> Vec<u8> {
        let Some(bytes) = self.prot_0895_bytes() else {
            return Vec::new();
        };
        let Ok(pak) = legaia_asset::init_pak::parse(bytes) else {
            return Vec::new();
        };
        let Some(logo) = pak.logos.get(idx as usize) else {
            return Vec::new();
        };
        let Ok(tim) = legaia_tim::parse(logo.bytes) else {
            return Vec::new();
        };
        legaia_tim::decode_rgba8(&tim, 0).unwrap_or_default()
    }

    /// Locate PROT 0895's byte range in the loaded disc image. Returns
    /// the entry bytes, or `None` when the disc isn't loaded or PROT
    /// 0895 isn't present (single-TIM mode, raw PROT.DAT without the
    /// disc walk, etc.).
    fn prot_0895_bytes(&self) -> Option<&[u8]> {
        let entries = parse_prot_toc(&self.disc)?;
        let meta = entries.iter().find(|e| e.index == 895)?;
        let off = meta.byte_offset as usize;
        let end = (meta.byte_offset + meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        Some(&self.disc[off..end])
    }

    /// Resolve a MES message id to its first 64 bytes as a hex string (for
    /// preview in the inspector panel). Returns an empty string if the
    /// current entry isn't a MES container or `text_id` is out of range.
    pub fn current_mes_message_hex(&self, text_id: u32) -> String {
        let Some(entry) = self.viewable.get(self.current) else {
            return String::new();
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return String::new();
        }
        let buf = &self.disc[off..end];
        let Ok(blob) = legaia_mes::parse(buf) else {
            return String::new();
        };
        let body_off = match blob.format {
            legaia_mes::Format::Compact => blob
                .offset_table
                .as_ref()
                .and_then(|t| t.get(text_id as usize).copied())
                .map(|v| v as usize),
            legaia_mes::Format::Records => blob
                .records
                .as_ref()
                .and_then(|r| r.get(text_id as usize))
                .map(|r| r.offset),
        };
        let Some(start) = body_off else {
            return String::new();
        };
        if start >= buf.len() {
            return String::new();
        }
        let n = (buf.len() - start).min(64);
        let mut s = String::with_capacity(n * 3);
        for &b in &buf[start..start + n] {
            s.push_str(&format!("{b:02X} "));
        }
        s
    }

    /// Build a 1024×512 PSX VRAM from every TIM the current entry contains.
    /// Returns the raw bytes (2 MB if a CLUT block is present, but VRAM is
    /// always exactly 1 MB = 1024×512×2). Used by the WebGL2 path to upload
    /// to a R16UI texture.
    pub fn current_vram_bytes(&self) -> Vec<u8> {
        self.build_current_vram()
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Returns the mesh data for the current entry's TMD as four typed arrays
    /// concatenated by use:
    ///   `[positions(f32 ×3 per vert), uvs(u8 ×2), cba_tsb(u16 ×2), indices(u32)]`
    /// Each as a separate getter so JS can pull them as typed arrays without
    /// reparsing JSON.
    pub fn mesh_positions(&self) -> Vec<f32> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
        }
        out
    }

    pub fn mesh_uvs(&self) -> Vec<u8> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.push(uv[0]);
            out.push(uv[1]);
        }
        out
    }

    pub fn mesh_cba_tsb(&self) -> Vec<u16> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.push(ct[0]);
            out.push(ct[1]);
        }
        out
    }

    pub fn mesh_indices(&self) -> Vec<u32> {
        self.build_current_vram_mesh()
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Returns the model's bounding sphere center (`[cx, cy, cz]`) and radius
    /// `r` packed as `[cx, cy, cz, r]`. JS uses this to build the MVP matrix
    /// without re-parsing the TMD each frame.
    pub fn mesh_bounds(&self) -> Vec<f32> {
        let Some(mesh) = self.build_current_vram_mesh() else {
            return vec![0.0; 4];
        };
        if mesh.positions.is_empty() {
            return vec![0.0; 4];
        }
        let (lo, hi) = mesh.aabb();
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let cz = (lo[2] + hi[2]) * 0.5;
        let dx = (hi[0] - lo[0]) * 0.5;
        let dy = (hi[1] - lo[1]) * 0.5;
        let dz = (hi[2] - lo[2]) * 0.5;
        let r = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
        vec![cx, cy, cz, r]
    }

    /// Render the current entry's TMD at the given rotation into a flat
    /// `Vec<f32>` of triangle data (7 floats per triangle, painter's-sorted
    /// back-to-front).
    ///
    /// Format per triangle: `[x0, y0, x1, y1, x2, y2, brightness 0..1]`.
    ///
    /// Returns an empty vec if the current entry has no TMD or the TMD has
    /// no triangles.
    #[allow(clippy::too_many_arguments)]
    pub fn render_tmd_triangles(
        &self,
        yaw: f32,
        pitch: f32,
        distance: f32,
        pan_x: f32,
        pan_y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<f32> {
        let Some(entry) = self.viewable.get(self.current) else {
            return Vec::new();
        };
        let Some(tmd_buf) = self.tmd_bytes_for(entry) else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(&tmd_buf) else {
            return Vec::new();
        };
        let Some(prepared) = tmd3d::prepare(&tmd, &tmd_buf) else {
            return Vec::new();
        };
        tmd3d::render(
            &prepared, yaw, pitch, distance, pan_x, pan_y, viewport_w, viewport_h,
        )
    }

    /// JSON status string: PROT index, class name, dims, current slot.
    pub fn status(&self) -> String {
        let Some(e) = self.viewable.get(self.current) else {
            return "{}".into();
        };
        let (w, h, bpp) = match &e.first_tim {
            Some(t) => (t.width, t.height, t.bpp),
            None => (0, 0, 0),
        };
        let has_tmd = e.tmd_source.is_some();
        let (tmd_tris, tmd_verts) = match e.tmd_source {
            Some(_) => self.tmd_stats(e),
            None => (0, 0),
        };
        format!(
            "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"width\":{},\"height\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_tris\":{},\"tmd_verts\":{},\"tmd_pack_count\":{}}}",
            self.current,
            e.meta.index,
            e.class.name(),
            w,
            h,
            bpp,
            e.tim_count,
            has_tmd,
            tmd_tris,
            tmd_verts,
            e.tmd_pack_count,
        )
    }

    /// Returns a JSON array describing every viewable entry: PROT index, class,
    /// dimensions, has-TMD flag. The UI uses this to populate a sidebar list / search.
    pub fn entry_list_json(&self) -> String {
        let mut s = String::from("[");
        for (i, e) in self.viewable.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let (w, h, bpp) = match &e.first_tim {
                Some(t) => (t.width, t.height, t.bpp),
                None => (0, 0, 0),
            };
            let has_tmd = e.tmd_source.is_some();
            s.push_str(&format!(
                "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"w\":{},\"h\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_pack_count\":{}}}",
                i,
                e.meta.index,
                e.class.name(),
                w,
                h,
                bpp,
                e.tim_count,
                has_tmd,
                e.tmd_pack_count,
            ));
        }
        s.push(']');
        s
    }
}

impl LegaiaViewer {
    fn render_current(&mut self) -> Result<(), JsValue> {
        let Some(entry) = self.viewable.get(self.current).cloned() else {
            return Ok(());
        };
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Err(JsValue::from_str("entry out of bounds"));
        }
        let entry_bytes = &self.disc[off..end];
        let label = format!("PROT {} · {}", entry.meta.index, entry.class.name());

        // The 3D path is driven from JS via render_tmd_triangles; if the
        // entry has a TMD, leave the canvas as the JS side set it up
        // (the rAF loop will repaint it). Don't try to acquire a 2D
        // context here - JS may already have bound webgl2 to it, in
        // which case getContext("2d") returns null.
        if entry.tmd_source.is_some() {
            return Ok(());
        }

        // 2D path: render the entry's first decodable TIM.
        let Some(hit) = &entry.first_tim else {
            return self.draw_message(&format!(
                "{label}: classified, but no decodable TIM or TMD found"
            ));
        };

        match hit.source.clone() {
            TimSource::Raw(o) => {
                let buf = &entry_bytes[o..].to_vec();
                self.render_tim_at(buf, 0, &label)
            }
            TimSource::Lzs { section, offset } => {
                let scan = tim_scan::scan_entry(entry_bytes);
                let Some(s) = scan.lzs_sections.get(section) else {
                    return self.draw_message(&format!("{label}: LZS section vanished"));
                };
                let buf = s[offset..].to_vec();
                self.render_tim_at(&buf, 0, &format!("{label} (LZS)"))
            }
        }
    }

    /// Build the VRAM the current entry would have at boot (every TIM the
    /// entry contains, uploaded at its declared `(fb_x, fb_y)`). Returns
    /// `None` when there's no current entry or the entry is out of bounds.
    /// Used by both [`Self::current_vram_bytes`] (GPU upload) and the
    /// [`Self::build_current_vram_mesh`] filter (drops prims whose texture
    /// pages weren't supplied so the WebGL pipeline doesn't rasterise
    /// solid-`CLUT[0]` tints over correctly-textured geometry).
    ///
    /// When the entry has a parseable TMD, the upload is *targeted* to
    /// just the TIMs whose image / CLUT regions overlap something the
    /// mesh actually samples. A single PROT entry can contain hundreds
    /// of TIMs, and uploading all of them into the 1 MB VRAM produces
    /// collisions (last-write-wins clobbers a CLUT row with image data
    /// from an unrelated TIM) which the paletted decode then renders as
    /// rainbow noise.
    fn build_current_vram(&self) -> Option<legaia_tim::Vram> {
        let entry = self.viewable.get(self.current)?;
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        let buf = &self.disc[off..end];
        let needs = self.tmd_prim_targets();
        let mut vram = legaia_tim::Vram::new();
        let scan = tim_scan::scan_entry(buf);
        for (source, hit) in &scan.hits {
            let tim_buf: Option<&[u8]> = match source {
                tim_scan::Source::Raw => Some(&buf[hit.offset..]),
                tim_scan::Source::Lzs(idx) => scan.lzs_sections.get(*idx).map(|s| &s[hit.offset..]),
            };
            if let Some(b) = tim_buf
                && let Ok(tim) = legaia_tim::parse(b)
            {
                if needs.is_empty() {
                    vram.upload_tim(&tim);
                } else {
                    let (img, clut) = tim_block_targeting(&tim, &needs);
                    if !img && !clut {
                        continue;
                    }
                    vram.upload_tim_partial(&tim, img, clut);
                }
            }
        }
        Some(vram)
    }

    /// Collect the CLUT + page rectangles every textured primitive in the
    /// current entry's TMD samples. Empty when the entry has no TMD or
    /// the TMD has no textured prims (= no targeting; upload everything).
    fn tmd_prim_targets(&self) -> Vec<PrimTarget> {
        let Some((tmd, tmd_buf)) = self.parse_current_tmd() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for o in &tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                &tmd_buf,
                o.primitives_byte_offset,
                o.primitives_byte_size,
            );
            for g in &groups {
                for p in &g.prims {
                    if p.uvs.is_empty() {
                        continue;
                    }
                    let (cx, cy) = p.cba_xy();
                    let (px, py, depth, _) = p.tpage_xy();
                    let clut_w: u16 = match depth {
                        4 => 16,
                        8 => 256,
                        _ => 0,
                    };
                    let mut umin = u8::MAX;
                    let mut umax = 0u8;
                    let mut vmin = u8::MAX;
                    let mut vmax = 0u8;
                    for &(u, v) in &p.uvs {
                        umin = umin.min(u);
                        umax = umax.max(u);
                        vmin = vmin.min(v);
                        vmax = vmax.max(v);
                    }
                    let (u_lo, u_hi) = match depth {
                        4 => (umin as u16 >> 2, umax as u16 >> 2),
                        8 => (umin as u16 >> 1, umax as u16 >> 1),
                        _ => (umin as u16, umax as u16),
                    };
                    out.push(PrimTarget {
                        clut: (cx, cy, clut_w, 1),
                        page: (
                            px + u_lo,
                            py + vmin as u16,
                            u_hi.saturating_sub(u_lo) + 1,
                            (vmax as u16).saturating_sub(vmin as u16) + 1,
                        ),
                    });
                }
            }
        }
        out
    }

    /// Build the current entry's mesh, dropped down to just the primitives
    /// whose texture pages have data in the entry's VRAM. Returns `None`
    /// if the entry has no parseable TMD.
    fn build_current_vram_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        let (tmd, tmd_buf) = self.parse_current_tmd()?;
        let vram = self.build_current_vram();
        Some(match vram {
            Some(v) => {
                legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&tmd, &tmd_buf, |cba, tsb, uvs| {
                    v.prim_has_texture_data(cba, tsb, uvs)
                })
            }
            None => legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_buf),
        })
    }

    /// Parse the current entry's TMD if it has one. Returns the parsed TMD
    /// plus the byte slice it was parsed from (caller may need it again to
    /// walk per-object primitive sections).
    /// Resolve an entry's renderable TMD bytes for any [`TmdSource`] variant,
    /// decompressing the LZS section on demand for the environment-geometry
    /// mesh pack. Centralises the slicing the 3D paths share.
    fn tmd_bytes_for(&self, entry: &ViewerEntry) -> Option<Vec<u8>> {
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        let buf = self.disc.get(off..end)?;
        match entry.tmd_source? {
            TmdSource::Direct { offset } => buf.get(offset..).map(<[u8]>::to_vec),
            TmdSource::SceneTmdStream { offset, len } => {
                buf.get(offset..offset + len).map(<[u8]>::to_vec)
            }
            TmdSource::Lzs {
                section,
                offset,
                len,
            } => {
                let scan = legaia_asset::tmd_scan::scan_entry(buf);
                scan.lzs_sections
                    .get(section)?
                    .get(offset..offset + len)
                    .map(<[u8]>::to_vec)
            }
        }
    }

    fn parse_current_tmd(&self) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let entry = self.viewable.get(self.current)?;
        let tmd_buf = self.tmd_bytes_for(entry)?;
        let tmd = legaia_tmd::parse(&tmd_buf).ok()?;
        Some((tmd, tmd_buf))
    }

    /// Decode slot 4 (world-map overlay outlines) of the kingdom bundle at
    /// `prot_base`. Mirrors the runtime loader's "try base, then base+1"
    /// fallback so both `scene_scripted_asset_table` and bare
    /// `scene_asset_table` variants succeed.
    fn decode_kingdom_slot4(&self, prot_base: u32) -> Option<Vec<u8>> {
        let entries = parse_prot_toc(&self.disc)?;
        for candidate in [prot_base, prot_base + 1] {
            let meta = entries.iter().find(|e| e.index == candidate)?;
            let off = meta.byte_offset as usize;
            let end = (meta.byte_offset + meta.size_bytes) as usize;
            if end > self.disc.len() {
                continue;
            }
            let buf = &self.disc[off..end];
            if let Ok(decoded) = legaia_asset::kingdom_bundle::decode_slot(buf, 4) {
                return Some(decoded);
            }
        }
        None
    }

    /// Try to load a kingdom 7-asset bundle from the PROT entry at `prot_index`.
    /// Returns the populated [`KingdomPack`] or a human-readable error.
    fn try_load_kingdom_at(
        &self,
        entries: &[EntryMeta],
        prot_index: u32,
    ) -> Result<KingdomPack, String> {
        let meta = entries
            .iter()
            .find(|e| e.index == prot_index)
            .ok_or_else(|| format!("PROT entry {prot_index} not in TOC"))?;
        let off = meta.byte_offset as usize;
        let end = (meta.byte_offset + meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Err(format!(
                "PROT entry {prot_index} ranges [{off}..{end}) exceed disc len {}",
                self.disc.len()
            ));
        }
        let buf = &self.disc[off..end];

        // Find the 7-asset table inside the entry. We scan 0x800-aligned
        // offsets for `[u32 count = 7]` + `descriptor[0].data_offset == 0x40`;
        // this catches both the prescript-prefixed variant (PROT base) and
        // the bare scene_asset_table variant (PROT base+1) without needing
        // separate detectors.
        let table_off = find_asset_table_offset(buf)
            .ok_or_else(|| format!("PROT entry {prot_index}: no 7-asset table found"))?;
        let table = &buf[table_off..];

        // Slot 0 = TIM_LIST (type 0x01).
        let slot0_ts = read_u32_le_slice(table, 8)?;
        let slot0_off = read_u32_le_slice(table, 12)? as usize;
        let slot0_type = (slot0_ts >> 24) as u8;
        let slot0_size = (slot0_ts & 0x00FF_FFFF) as usize;
        if slot0_type != 0x01 {
            return Err(format!("slot 0 type 0x{slot0_type:02X} != 0x01 (TIM_LIST)"));
        }

        // Slot 1 = TMD pack (type 0x02).
        let slot1_ts = read_u32_le_slice(table, 16)?;
        let slot1_off = read_u32_le_slice(table, 20)? as usize;
        let slot1_type = (slot1_ts >> 24) as u8;
        let slot1_size = (slot1_ts & 0x00FF_FFFF) as usize;
        if slot1_type != 0x02 {
            return Err(format!("slot 1 type 0x{slot1_type:02X} != 0x02 (TMD)"));
        }

        // Each descriptor's `data_offset` is the table-relative file position
        // of that slot's LZS-compressed payload. Decode each independently.
        let tim_src = table
            .get(slot0_off..)
            .ok_or_else(|| format!("slot 0 offset 0x{slot0_off:X} out of range"))?;
        let tim_decoded = legaia_lzs::decompress(tim_src, slot0_size)
            .map_err(|e| format!("slot 0 LZS decode failed: {e}"))?;

        let pack_src = table
            .get(slot1_off..)
            .ok_or_else(|| format!("slot 1 offset 0x{slot1_off:X} out of range"))?;
        let pack = legaia_lzs::decompress(pack_src, slot1_size)
            .map_err(|e| format!("slot 1 LZS decode failed: {e}"))?;

        // Build VRAM by walking every TIM the TIM_LIST decompresses to.
        // TIM_LIST format is `[u32 count][u32 word_offsets[count]][TIMs]`
        // (same shape as the TMD pack but with TIM bodies). Mirror the
        // pointer math from `FUN_8001F05C case 1` -> byte_offset = word * 4.
        let mut vram = legaia_tim::Vram::new();
        if tim_decoded.len() >= 4 {
            let count = u32::from_le_bytes(tim_decoded[0..4].try_into().unwrap()) as usize;
            let table_bytes = 4 + count * 4;
            if tim_decoded.len() >= table_bytes {
                for k in 0..count {
                    let woff =
                        u32::from_le_bytes(tim_decoded[4 + k * 4..8 + k * 4].try_into().unwrap())
                            as usize;
                    let bo = woff.saturating_mul(4);
                    if bo >= tim_decoded.len() {
                        continue;
                    }
                    if let Ok(tim) = legaia_tim::parse(&tim_decoded[bo..]) {
                        vram.upload_tim(&tim);
                    }
                }
            }
        }
        let ocean = find_ocean_assets(&tim_decoded);

        // Parse the TMD-pack table.
        if pack.len() < 4 {
            return Err("TMD pack < 4 bytes".into());
        }
        let count = u32::from_le_bytes(pack[0..4].try_into().unwrap()) as usize;
        let table_bytes = 4 + count * 4;
        if pack.len() < table_bytes {
            return Err(format!(
                "TMD pack header truncated (need {table_bytes}, have {})",
                pack.len()
            ));
        }
        let mut byte_offsets = Vec::with_capacity(count);
        for k in 0..count {
            let woff = u32::from_le_bytes(pack[4 + k * 4..8 + k * 4].try_into().unwrap()) as usize;
            byte_offsets.push(woff.saturating_mul(4));
        }
        let mut byte_ends = Vec::with_capacity(count);
        for k in 0..count {
            byte_ends.push(byte_offsets.get(k + 1).copied().unwrap_or(pack.len()));
        }

        Ok(KingdomPack {
            prot_index,
            vram,
            pack,
            byte_offsets,
            byte_ends,
            cur_slot: None,
            ocean,
        })
    }

    /// Build the textured mesh for the currently-selected kingdom pack slot.
    ///
    /// Uses the unfiltered [`legaia_tmd::mesh::tmd_to_vram_mesh`] (not the
    /// VRAM-targeted filter the per-PROT-entry path uses). The kingdom's
    /// TIM_LIST packs ~50 TIMs into rows 479-510, so many CLUT rows hold
    /// data from multiple TIMs and the filter's depth-mismatch heuristic
    /// drops almost every prim. We accept that some prims may sample
    /// "wrong" CLUT data (whichever TIM won the last-write-wins race for
    /// that VRAM row) in exchange for geometry coverage: the assembled
    /// continent view is "show the kingdom's shape and colour" first,
    /// "pixel-perfect texturing" later. A future refinement is per-TMD
    /// targeted upload (compute the prim CBA/TSB set, upload only the
    /// matching TIMs), which would avoid the collision but require
    /// rebuilding VRAM per slot.
    fn build_kingdom_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        Self::build_pack_mesh(self.kingdom.as_ref()?)
    }

    /// Mirror of `build_kingdom_mesh` for the bulk-continent pack loaded
    /// from slot +N of the kingdom bundle (Drake +8, Sebucus +9). The
    /// continent pack carries its own VRAM + its own TMD list, so the
    /// pack-mesh accessors route through it via `cur_slot`.
    fn build_continent_mesh(&self) -> Option<legaia_tmd::mesh::VramMesh> {
        Self::build_pack_mesh(self.continent.as_ref()?)
    }

    fn build_pack_mesh(k: &KingdomPack) -> Option<legaia_tmd::mesh::VramMesh> {
        let slot = k.cur_slot?;
        let start = *k.byte_offsets.get(slot)?;
        let end = *k.byte_ends.get(slot)?;
        let tmd_buf = k.pack.get(start..end)?;
        let tmd = legaia_tmd::parse(tmd_buf).ok()?;
        Some(legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, tmd_buf))
    }

    fn tmd_stats(&self, entry: &ViewerEntry) -> (usize, usize) {
        let Some(tmd_buf) = self.tmd_bytes_for(entry) else {
            return (0, 0);
        };
        let Ok(tmd) = legaia_tmd::parse(&tmd_buf) else {
            return (0, 0);
        };
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &tmd_buf);
        (mesh.triangle_count(), mesh.vertex_count())
    }

    fn render_tim_at(&self, src: &[u8], offset: usize, label: &str) -> Result<(), JsValue> {
        let buf = &src[offset..];
        let tim = legaia_tim::parse(buf)
            .map_err(|e| JsValue::from_str(&format!("{label}: TIM parse: {e}")))?;
        let rgba = legaia_tim::decode_rgba8(&tim, self.clut_idx)
            .map_err(|e| JsValue::from_str(&format!("{label}: decode: {e}")))?;

        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        if w == 0 || h == 0 {
            return self.draw_message(&format!("{label}: empty TIM ({}x{})", w, h));
        }

        let (canvas, ctx) = self.acquire_2d_context()?;
        canvas.set_width(w);
        canvas.set_height(h);

        let clamped = rgba;
        let img = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&clamped), w, h)?;
        ctx.put_image_data(&img, 0.0, 0.0)?;
        Ok(())
    }

    fn draw_message(&self, msg: &str) -> Result<(), JsValue> {
        let (canvas, ctx) = self.acquire_2d_context()?;
        canvas.set_width(800);
        canvas.set_height(200);
        ctx.set_fill_style_str("#0a0e15");
        ctx.fill_rect(0.0, 0.0, 800.0, 200.0);
        ctx.set_fill_style_str("#8b949e");
        ctx.set_font("16px JetBrains Mono, ui-monospace, monospace");
        ctx.fill_text(msg, 16.0, 100.0)?;
        Ok(())
    }

    /// Resolve `canvas_id` to its current `HtmlCanvasElement` and a fresh
    /// 2D rendering context. The element is re-fetched from the DOM each
    /// time because the JS UI replaces the canvas when switching between
    /// the 2D (TIM blit) and 3D (WebGL2) modes - getContext returns null
    /// for the second context type bound to a single canvas, and any
    /// cached reference goes stale the moment `oldCanvas.replaceWith(...)`
    /// runs in `startTexturedTmdLoop` / `startFlatTmdLoop`.
    fn acquire_2d_context(&self) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
        let canvas = resolve_canvas(&self.canvas_id)?;
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| {
                JsValue::from_str(
                    "no 2d context (canvas was already bound to webgl - JS must \
                     replace the canvas element before requesting a 2D draw)",
                )
            })?
            .dyn_into::<CanvasRenderingContext2d>()?;
        Ok((canvas, ctx))
    }
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

/// Vertex-centroid bounding sphere — `[cx, cy, cz, r]`. The center is the
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
    size: i16,
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
    let x1 = x.saturating_add(size);
    let y1 = y.saturating_add(size);
    let u1 = u.saturating_add(size as u8);
    let v1 = v.saturating_add(size as u8);
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
/// `legaia_engine_core::scene::FIELD_MAP_LEN`.
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
///   [`WALK_FIELD_MAP_LEN`] (`0x12000`). Inside the block the first `0x12000`
///   entry is a decoy with only a handful of `0x1000` cells, so the preceding
///   "duplicate"-cluster entry is the real grid (pinned 14/14 against a live
///   `map01` walk capture). Falls back to scanning the block when that slot
///   isn't a `0x12000` entry.
/// - **Floor-height LUT** is `man[+0x02..+0x22]` (16 `s16` LE) from the kingdom
///   bundle's MAN slot (slot 2), the same bytes `Scene::field_floor_height_lut`
///   reads.
///
/// Reuses [`legaia_asset::field_objects::build_walk_heightfield`] for the grid
/// math. Returns `None` when either source can't be resolved.
pub fn build_walk_ground(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<legaia_asset::field_objects::WalkHeightfield> {
    let (map_bytes, lut) = resolve_walk_map_and_lut(disc, entries, prot_base)?;
    let hf = legaia_asset::field_objects::build_walk_heightfield(map_bytes, &lut);
    (!hf.indices.is_empty()).then_some(hf)
}

/// One walk-frame landmark placement resolved for the world-overview viewer: a
/// slot-1 pack mesh index plus its world position in the **same `col*128`
/// world frame** the walk heightfield is built in.
///
/// Mirrors the native engine's `resolve_placement_draws` world transform: the
/// placement anchor sits at `world_y = -lut[floor_nibble] + y_off` (the runtime
/// stores the floor LUT negated) and the JS renderer applies the shared
/// `(1, -1, 1)` model flip at scale `1` — the slot-1 pack meshes are already in
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
    /// World Y: `-lut[floor_nibble] + y_off` (pre-Y-flip, same frame as the
    /// heightfield's stored positions).
    pub world_y: i32,
    /// World Z in the `row*128` walk frame.
    pub world_z: i32,
}

/// Resolve the kingdom's walk-frame placed landmarks (the `flags & 0x4` slot-1
/// pack objects `FUN_8003A55C` draws) in the same world frame as
/// [`build_walk_ground`], so the world-overview viewer can overlay them on the
/// continent terrain.
///
/// Reads the same walk `.MAP` + floor-height LUT [`build_walk_ground`] does
/// (see [`resolve_walk_map_and_lut`]), runs
/// [`legaia_asset::field_objects::parse_placements`], and resolves each
/// placement's world Y from the floor nibble exactly like the native
/// `resolve_placement_draws`. Placements whose mesh isn't in the scene pack
/// (protagonist / NPC ids, `pack_index == None`) are dropped. Returns `None`
/// when the walk `.MAP` / floor LUT can't be resolved.
pub fn build_walk_placements(
    disc: &[u8],
    entries: &[EntryMeta],
    prot_base: u32,
) -> Option<Vec<WalkPlacement>> {
    let (map_bytes, lut) = resolve_walk_map_and_lut(disc, entries, prot_base)?;
    let placements = legaia_asset::field_objects::parse_placements(map_bytes);
    let resolved = placements
        .iter()
        .filter_map(|p| {
            let pack_index = p.pack_index?;
            // World Y from the floor-height LUT (`-lut[nibble] + y_off`), or the
            // ground plane when the nibble is unavailable - matches the native
            // engine's `resolve_placement_draws`.
            let world_y = match p.floor_nibble {
                Some(nib) => -(lut[(nib & 0x0F) as usize] as i32) + p.y_off as i32,
                None => 0,
            };
            Some(WalkPlacement {
                pack_index: pack_index as u32,
                world_x: p.world_x,
                world_y,
                world_z: p.world_z,
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
///   [`WALK_FIELD_MAP_LEN`] (`0x12000`). Inside the block the first `0x12000`
///   entry is a decoy with only a handful of `0x1000` cells, so the preceding
///   "duplicate"-cluster entry is the real grid (pinned 14/14 against a live
///   `map01` walk capture). Falls back to scanning the block when that slot
///   isn't a `0x12000` entry.
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
    let man = legaia_asset::kingdom_bundle::decode_slot(buf, 2).ok()?;
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
        && let Some(s) = legaia_asset::scene_tmd_stream::detect(buf)
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
    let scan = legaia_asset::tmd_scan::scan_entry(buf);
    let lzs_hits: Vec<_> = scan
        .hits
        .iter()
        .filter_map(|(src, hit)| match src {
            legaia_asset::tmd_scan::Source::Lzs(idx) => Some((*idx, hit)),
            legaia_asset::tmd_scan::Source::Raw => None,
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
// LegaiaAudio: WASM bindings for site/audio.html
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
    entries: Vec<disc::EntryMeta>,
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

#[wasm_bindgen]
impl LegaiaAudio {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self {
            disc: Vec::new(),
            prot: Vec::new(),
            entries: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
            str_video: None,
        }
    }

    /// Load a full Mode2/2352 disc image. Extracts `PROT.DAT` via the same
    /// in-memory ISO walker the viewer uses, parses the TOC, and stashes
    /// both slices for later VAB / BGM / XA queries. Returns the PROT entry
    /// count for the JS UI.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<u32, JsValue> {
        let prot = disc::extract_prot_dat(&bytes).ok_or_else(|| {
            JsValue::from_str(
                "audio: not a Mode2/2352 disc image (the audio page requires a full .bin)",
            )
        })?;
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("audio: PROT.DAT TOC parse failed"))?;
        console_log(&format!(
            "Audio: loaded disc ({} MB), {} PROT entries",
            bytes.len() / 1024 / 1024,
            entries.len()
        ));
        self.entries = entries;
        self.prot = prot;
        self.disc = bytes;
        Ok(self.entries.len() as u32)
    }

    /// JSON list of every VAB sound bank in the loaded disc.
    /// Shape: `[{ prot_index, vab_offset, version, program_count, sample_count, has_seq }, ...]`.
    pub fn enumerate_vabs_json(&self) -> String {
        let v = audio::enumerate_vabs(&self.prot, &self.entries);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"prot_index":{},"vab_offset":{},"version":{},"program_count":{},"sample_count":{},"has_seq":{}}}"#,
                x.prot_index,
                x.vab_offset,
                x.version,
                x.program_count,
                x.sample_count,
                x.has_seq,
            ));
        }
        s.push(']');
        s
    }

    /// JSON list of every BGM pair (`pBAV` + `pQES` in the same PROT entry).
    /// Shape: `[{ prot_index, vab_offset, seq_offset, program_count, sample_count, ppqn, bpm }, ...]`.
    pub fn enumerate_bgm_pairs_json(&self) -> String {
        let v = audio::enumerate_bgm_pairs(&self.prot, &self.entries);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"prot_index":{},"vab_offset":{},"seq_offset":{},"program_count":{},"sample_count":{},"ppqn":{},"bpm":{:.1}}}"#,
                x.prot_index,
                x.vab_offset,
                x.seq_offset,
                x.program_count,
                x.sample_count,
                x.ppqn,
                x.bpm,
            ));
        }
        s.push(']');
        s
    }

    /// JSON list of every `*.STR` / `*.XA` file on the disc, with its raw LBA
    /// and byte size. Shape: `[{ path, lba, size }, ...]`.
    pub fn enumerate_xa_files_json(&self) -> String {
        let v = audio::enumerate_xa_files(&self.disc);
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // Escape only the bare minimum (path is ASCII filename, no quotes).
            s.push_str(&format!(
                r#"{{"path":"{}","lba":{},"size":{}}}"#,
                x.path.replace('\\', "/"),
                x.lba,
                x.size,
            ));
        }
        s.push(']');
        s
    }

    /// Sample rate the JS side should use when playing a VAG-decoded buffer.
    pub fn vab_sample_rate(&self) -> u32 {
        audio::VAB_SAMPLE_RATE
    }

    /// JSON metadata for every VAG sample inside one VAB bank.
    /// Shape: `[{ size_bytes, decoded_samples, duration_ms }, ...]`.
    /// `decoded_samples` is the actual PCM length after walking the ADPCM
    /// blocks (which stop at the first loop-end / garbage block), so it
    /// reflects the audible length, not the raw on-disc body size. Useful
    /// for the UI to dim out tiny/zero-length samples that would be
    /// inaudible.
    pub fn vab_sample_list_json(&self, prot_index: u32, vab_offset: u32) -> String {
        let Some((report, _)) =
            audio::parse_vab_at(&self.prot, &self.entries, prot_index, vab_offset)
        else {
            return "[]".into();
        };
        let mut s = String::from("[");
        for (i, span) in report.vag_samples.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // Decoding each sample once at enumeration time gives the
            // UI accurate duration. The cost is one full decode per
            // sample - fast in WASM, run once per bank-open.
            let decoded_len = audio::decode_vag_sample(
                &self.prot,
                &self.entries,
                prot_index,
                vab_offset,
                i as u32,
            )
            .map(|p| p.len())
            .unwrap_or(0);
            let duration_ms = (decoded_len as f64 * 1000.0 / audio::VAB_SAMPLE_RATE as f64) as u32;
            s.push_str(&format!(
                r#"{{"size_bytes":{},"decoded_samples":{},"duration_ms":{}}}"#,
                span.size, decoded_len, duration_ms,
            ));
        }
        s.push(']');
        s
    }

    /// Decode one VAG sample to mono i16 PCM at `vab_sample_rate()`.
    /// Empty when the sample doesn't exist or has zero length.
    pub fn decode_vab_sample_i16(
        &self,
        prot_index: u32,
        vab_offset: u32,
        sample_idx: u32,
    ) -> Vec<i16> {
        audio::decode_vag_sample(
            &self.prot,
            &self.entries,
            prot_index,
            vab_offset,
            sample_idx,
        )
        .unwrap_or_default()
    }

    /// Demux + decode an XA stream. Returns the decoded PCM of the first
    /// audio channel (file_no=0, ch_no=0 typically) along with metadata
    /// packed as JSON in the first method, then the PCM via this one.
    ///
    /// Two-step API so the JS side can show metadata (channels, sample rate)
    /// before paying the decode cost.
    pub fn xa_metadata_json(&self, lba: u32, size: u32) -> String {
        let streams = audio::decode_xa_in_memory(&self.disc, lba, size);
        let mut s = String::from("[");
        for (i, x) in streams.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"file_no":{},"ch_no":{},"sample_rate":{},"stereo":{},"sample_count":{}}}"#,
                x.file_no,
                x.ch_no,
                x.sample_rate,
                x.stereo,
                x.pcm.len(),
            ));
        }
        s.push(']');
        s
    }

    /// Decode XA stream and return the i16 PCM for the channel at `stream_idx`
    /// (index into the `xa_metadata_json` array). Empty when out of range.
    pub fn decode_xa_stream_i16(&self, lba: u32, size: u32, stream_idx: u32) -> Vec<i16> {
        let streams = audio::decode_xa_in_memory(&self.disc, lba, size);
        streams
            .into_iter()
            .nth(stream_idx as usize)
            .map(|x| x.pcm)
            .unwrap_or_default()
    }

    /// Open an `MV*.STR` movie for video playback. Demuxes every MDEC video
    /// frame's bitstream off the disc (skipping the interleaved audio) and
    /// caches them, keyed by `lba`. Returns JSON
    /// `{ "width", "height", "frame_count", "fps" }`. Frames are NOT decoded to
    /// RGBA here - call `str_decode_frame(idx)` per displayed frame so the page
    /// pays MDEC cost incrementally (a whole movie's RGBA is hundreds of MB).
    ///
    /// Idempotent for the same `lba`: a second open returns the cached metadata
    /// without re-walking the disc. `.XA` (audio-only) files have no video and
    /// come back with `frame_count: 0`.
    pub fn str_video_open(&mut self, lba: u32, size: u32) -> String {
        if self.str_video.as_ref().map(|(l, _)| *l) != Some(lba) {
            let video = audio::demux_str_video(&self.disc, lba, size);
            self.str_video = Some((lba, video));
        }
        let (_, video) = self.str_video.as_ref().unwrap();
        format!(
            r#"{{"width":{},"height":{},"frame_count":{},"fps":{:.4}}}"#,
            video.width,
            video.height,
            video.frames.len(),
            video.fps,
        )
    }

    /// Decode the frame at `frame_idx` of the currently-open STR movie to a
    /// row-major RGBA8 buffer (`width * height * 4` bytes). Empty when no movie
    /// is open or the index is out of range. Call `str_video_open` first.
    pub fn str_decode_frame(&self, frame_idx: u32) -> Vec<u8> {
        let Some((_, video)) = self.str_video.as_ref() else {
            return Vec::new();
        };
        video
            .frames
            .get(frame_idx as usize)
            .map(audio::decode_str_frame_rgba)
            .unwrap_or_default()
    }

    /// Drop the cached STR movie frames (frees the bitstream buffers).
    pub fn str_video_close(&mut self) {
        self.str_video = None;
    }

    /// Start BGM playback for the given (`prot_index`, `vab_offset`,
    /// `seq_offset`) tuple. Constructs the WebAudio output on the first call
    /// (must be invoked from a user-gesture handler), parses VAB + SEQ,
    /// uploads the bank to the embedded clean-room SPU, and attaches the
    /// sequencer.
    #[cfg(target_arch = "wasm32")]
    pub fn start_bgm(
        &mut self,
        prot_index: u32,
        vab_offset: u32,
        seq_offset: u32,
    ) -> Result<(), JsValue> {
        let e = self
            .entries
            .iter()
            .find(|x| x.index == prot_index)
            .ok_or_else(|| JsValue::from_str("start_bgm: PROT entry not found"))?;
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        let buf = self
            .prot
            .get(off..end)
            .ok_or_else(|| JsValue::from_str("start_bgm: entry slice OOB"))?;

        let vab_report = legaia_vab::parse(buf, vab_offset as usize)
            .map_err(|e| JsValue::from_str(&format!("VAB parse: {e}")))?;
        let seq = legaia_seq::Seq::parse(&buf[seq_offset as usize..])
            .map_err(|e| JsValue::from_str(&format!("SEQ parse: {e}")))?;

        // Lazy WebAudio open. Browser autoplay policy requires this to run
        // inside a user gesture - the JS side wires this method up to a
        // button click.
        if self.audio_out.is_none() {
            let out = legaia_engine_audio::WebAudioOut::new()
                .map_err(|e| JsValue::from_str(&format!("WebAudioOut: {e}")))?;
            self.audio_out = Some(out);
        }
        let out = self.audio_out.as_ref().unwrap();

        // Upload bank into the SPU model (which lives inside WebAudioOut's
        // resampler). Then build the sequencer and attach.
        let bank = out.with_spu(|spu| {
            let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(0x1000, 0x40_000);
            legaia_engine_audio::VabBank::upload(
                spu,
                &mut alloc,
                &vab_report,
                &buf[vab_offset as usize..],
            )
        });
        let sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        out.attach_sequencer(sequencer);
        Ok(())
    }

    /// Stop the currently-playing BGM. Safe to call even when nothing is
    /// playing (no-op).
    #[cfg(target_arch = "wasm32")]
    pub fn stop_bgm(&mut self) {
        if let Some(out) = self.audio_out.as_ref() {
            out.detach_sequencer();
        }
    }

    /// Resume the BGM AudioContext. Browsers often construct the
    /// `AudioContext` in `suspended` state even when the constructor
    /// runs inside a user-gesture handler; the JS side calls this
    /// immediately after `start_bgm` to make the audio actually audible.
    #[cfg(target_arch = "wasm32")]
    pub fn resume_bgm(&mut self) -> js_sys::Promise {
        match self.audio_out.as_ref() {
            Some(out) => out.resume(),
            None => js_sys::Promise::resolve(&JsValue::UNDEFINED),
        }
    }

    /// Pause / resume the active BGM sequencer. Notes that are already
    /// sounding decay through their ADSR envelopes; the sequencer clock
    /// freezes.
    #[cfg(target_arch = "wasm32")]
    pub fn set_bgm_paused(&mut self, paused: bool) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_sequencer_paused(paused);
        }
    }

    /// Set the BGM playback gain. Retail SEQ + clean-room SPU output sits
    /// around 1% of the i16 range, so the audio page defaults to ~25x to
    /// bring playback to a comfortable level. `1.0` matches the native
    /// engine-shell cpal path.
    #[cfg(target_arch = "wasm32")]
    pub fn set_bgm_gain(&mut self, gain: f32) {
        if let Some(out) = self.audio_out.as_ref() {
            out.set_gain(gain);
        }
    }

    /// Sample rate of the browser's BGM `AudioContext`, or 0 when the BGM
    /// output hasn't been opened yet. Surfaced to the JS console for
    /// diagnostics when playback speed is off.
    #[cfg(target_arch = "wasm32")]
    pub fn bgm_device_rate(&self) -> u32 {
        self.audio_out
            .as_ref()
            .map(|o| o.device_rate())
            .unwrap_or(0)
    }

    /// Render `duration_seconds` worth of interleaved stereo i16 PCM at
    /// the SPU's 44.1 kHz rate for the BGM pair at (`prot_index`,
    /// `vab_offset`, `seq_offset`). Used by the audio page to pre-render
    /// a chunk and play it through `AudioBufferSourceNode` (sample-
    /// accurate timing) instead of through `ScriptProcessorNode` (callback-
    /// paced, drifts on some browsers).
    pub fn render_bgm_pcm_i16(
        &self,
        prot_index: u32,
        vab_offset: u32,
        seq_offset: u32,
        duration_seconds: f32,
    ) -> Vec<i16> {
        let Some(e) = self.entries.iter().find(|x| x.index == prot_index) else {
            return Vec::new();
        };
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        let Some(buf) = self.prot.get(off..end) else {
            return Vec::new();
        };
        let Ok(vab_report) = legaia_vab::parse(buf, vab_offset as usize) else {
            return Vec::new();
        };
        let Ok(seq) = legaia_seq::Seq::parse(&buf[seq_offset as usize..]) else {
            return Vec::new();
        };
        let mut spu = legaia_engine_audio::Spu::new();
        let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(0x1000, 0x40_000);
        let bank = legaia_engine_audio::VabBank::upload(
            &mut spu,
            &mut alloc,
            &vab_report,
            &buf[vab_offset as usize..],
        );
        let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
        let duration_samples =
            (duration_seconds * legaia_engine_audio::SPU_INTERNAL_RATE as f32) as usize;
        legaia_engine_audio::render_bgm_to_pcm(&mut sequencer, &mut spu, duration_samples)
    }

    /// Sample rate produced by [`Self::render_bgm_pcm_i16`] (the SPU's
    /// internal 44.1 kHz). Surfaced so the JS side can build a correct
    /// WAV header for `decodeAudioData`.
    pub fn bgm_render_rate(&self) -> u32 {
        legaia_engine_audio::SPU_INTERNAL_RATE
    }
}

impl Default for LegaiaAudio {
    fn default() -> Self {
        Self::new()
    }
}
