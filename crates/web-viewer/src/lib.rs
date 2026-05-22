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
use legaia_asset::tim_scan;
use legaia_asset::worldmap_menu;
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

fn console_log(s: &str) {
    web_sys::console::log_1(&JsValue::from_str(s));
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
}

#[derive(Clone, Copy)]
enum TmdSource {
    /// Bare TMD at offset 0 of the entry.
    Direct { offset: usize },
    /// scene_tmd_stream wrapper: 4-byte chunk0 header + bare TMD.
    SceneTmdStream { offset: usize, len: usize },
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
        console_log(&format!(
            "Found {} PROT entries - classifying…",
            entries.len()
        ));
        self.disc = prot_bytes;

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

            // Skip classes that never carry TIMs.
            if matches!(
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
            ) {
                continue;
            }

            let scan = tim_scan::scan_entry(buf);
            let tim_count = scan.hits.len();

            // Find the first hit whose bytes actually decode (not just magic match).
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

            // Detect a leading TMD for the 3D viewer path.
            let tmd_source = detect_tmd_in_entry(buf, report.class);

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
        self.render_current()?;
        Ok(self.current_index())
    }

    pub fn set_clut(&mut self, idx: u32) -> Result<(), JsValue> {
        self.clut_idx = idx as usize;
        self.render_current()
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
        Ok(count)
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
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return Vec::new();
        }
        let buf = &self.disc[off..end];

        let (tmd_buf, tmd_len) = match entry.tmd_source {
            Some(TmdSource::Direct { offset }) => (&buf[offset..], buf.len() - offset),
            Some(TmdSource::SceneTmdStream { offset, len }) => (&buf[offset..offset + len], len),
            None => return Vec::new(),
        };
        let _ = tmd_len;
        let Ok(tmd) = legaia_tmd::parse(tmd_buf) else {
            return Vec::new();
        };
        let Some(prepared) = tmd3d::prepare(&tmd, tmd_buf) else {
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
            "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"width\":{},\"height\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{},\"tmd_tris\":{},\"tmd_verts\":{}}}",
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
                "{{\"slot\":{},\"prot_index\":{},\"class\":\"{}\",\"w\":{},\"h\":{},\"bpp\":{},\"tim_count\":{},\"has_tmd\":{}}}",
                i,
                e.meta.index,
                e.class.name(),
                w,
                h,
                bpp,
                e.tim_count,
                has_tmd,
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
    fn parse_current_tmd(&self) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let entry = self.viewable.get(self.current)?;
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return None;
        }
        let buf = &self.disc[off..end];
        let tmd_buf: Vec<u8> = match entry.tmd_source? {
            TmdSource::Direct { offset } => buf[offset..].to_vec(),
            TmdSource::SceneTmdStream { offset, len } => buf[offset..offset + len].to_vec(),
        };
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
        let off = entry.meta.byte_offset as usize;
        let end = (entry.meta.byte_offset + entry.meta.size_bytes) as usize;
        if end > self.disc.len() {
            return (0, 0);
        }
        let buf = &self.disc[off..end];
        let tmd_buf = match entry.tmd_source {
            Some(TmdSource::Direct { offset }) => &buf[offset..],
            Some(TmdSource::SceneTmdStream { offset, len }) => &buf[offset..offset + len],
            None => return (0, 0),
        };
        let Ok(tmd) = legaia_tmd::parse(tmd_buf) else {
            return (0, 0);
        };
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, tmd_buf);
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
fn detect_tmd_in_entry(buf: &[u8], class: Class) -> Option<TmdSource> {
    if class == Class::SceneTmdStream
        && let Some(s) = legaia_asset::scene_tmd_stream::detect(buf)
    {
        let r = s.tmd_range();
        // Validate the TMD actually parses; the detector is structural only.
        if legaia_tmd::parse(&buf[r.start..r.end]).is_ok() {
            return Some(TmdSource::SceneTmdStream {
                offset: r.start,
                len: r.end - r.start,
            });
        }
    }
    // Bare TMD at offset 0?
    if buf.len() >= legaia_tmd::HEADER_SIZE
        && let Ok(_) = legaia_tmd::parse(buf)
    {
        return Some(TmdSource::Direct { offset: 0 });
    }
    None
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
