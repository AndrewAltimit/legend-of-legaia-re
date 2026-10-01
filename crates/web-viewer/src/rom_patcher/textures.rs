//! Texture replacement: scan, preview, apply, and change packs.
//! Split out of `rom_patcher.rs`.

use super::*;

// --- Texture replacement --------------------------------------------------
//
// The same client-side model as the randomizer: the user's disc bytes are
// scanned in WASM memory, the edited PNG is validated + encoded here, and the
// patched image is downloaded locally. Nothing is uploaded.
//
// Every family-specific rule lives in [`crate::texture_registry`] - which
// families exist, how each enumerates, decodes and writes. The bindings below
// are a translation layer to JS values and nothing else, so adding a texture
// family does not touch this file.

use legaia_patcher::texture::{ExportFormat, replace_texture_png};
use legaia_patcher::texture_palettes::texture_palettes;
use legaia_patcher::{battle_texture, monster_texture, save_icon};
use legaia_tim::encode::{EncodeOptions, decode_png_rgba};
use legaia_tim::multi_palette::{ImportKind, View};

use crate::texture_pack::{self, PackEntry, PackMeta};
use crate::texture_registry::{self as reg, ReplaceOp, Rgba, ScanCtx, TexCoord, TexRow};

/// Nearest-neighbour downscale of an RGBA8 image to fit in `max` on the long
/// side (thumbnails for the texture browser).
pub(super) fn downscale_rgba(
    rgba: &[u8],
    w: usize,
    h: usize,
    max: usize,
) -> (usize, usize, Vec<u8>) {
    if w <= max && h <= max {
        return (w, h, rgba.to_vec());
    }
    let step = w.max(h).div_ceil(max).max(1);
    let tw = (w / step).max(1);
    let th = (h / step).max(1);
    let mut out = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let i = (y * step * w + x * step) * 4;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (tw, th, out)
}

/// `{ w, h, rgba: Uint8Array }` for a decoded image.
pub(super) fn rgba_js(w: usize, h: usize, rgba: &[u8]) -> Result<JsValue, JsValue> {
    let o = Object::new();
    Reflect::set(&o, &"w".into(), &JsValue::from_f64(w as f64))?;
    Reflect::set(&o, &"h".into(), &JsValue::from_f64(h as f64))?;
    let arr = Uint8Array::new_with_length(rgba.len() as u32);
    arr.copy_from(rgba);
    Reflect::set(&o, &"rgba".into(), &arr)?;
    Ok(o.into())
}

/// A registry coordinate from the page's `(tier, entry, section, offset)`
/// quad. The tier string is resolved against the registry so an unknown
/// family is refused here rather than silently taking some other family's
/// writer.
pub(super) fn coord_of(
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
) -> Result<TexCoord, JsValue> {
    let id = reg::tier(tier)
        .ok_or_else(|| err(format!("unknown texture family {tier:?}")))?
        .id;
    Ok(TexCoord {
        tier: id,
        entry: entry as i64,
        section: section as i64,
        offset: offset as u64,
    })
}

/// What the scan needs out of a disc image, read before the image is
/// dropped: the `PROT.DAT` payload, the CDNAME block map, and the executable.
///
/// Peak-memory discipline: the scan only needs these, and a full image plus
/// payload plus scan state would not fit comfortably in 32-bit WASM memory.
///
/// The executable is here because a texture family can need data that is not
/// in `PROT.DAT` at all - the battle-equipment tier names its rows after the
/// equipment they belong to, and that name table lives in `SCUS_942.54`. It
/// is the disc that holds both, so the disc is where both get read.
pub(super) struct DiscScanInput {
    pub(super) prot: Vec<u8>,
    pub(super) blocks: Option<legaia_prot::cdname::IndexMap>,
    pub(super) scus: Option<Vec<u8>>,
}

pub(super) fn disc_scan_input(image: Vec<u8>) -> Result<DiscScanInput, JsValue> {
    let prot = legaia_iso::iso9660::read_file_in_image(&image, "PROT.DAT")
        .ok_or_else(|| err("PROT.DAT not found in disc image"))?;
    let blocks = crate::disc::extract_cdname_txt(&image)
        .and_then(|t| legaia_prot::cdname::parse_str(&t).ok());
    let scus = crate::disc::extract_scus(&image);
    drop(image);
    Ok(DiscScanInput { prot, blocks, scus })
}

/// Every TOC entry's `(byte_offset, size_bytes, index)`.
pub(super) fn entry_spans(prot: &[u8]) -> Result<Vec<(u64, u64, u32)>, JsValue> {
    let archive = legaia_prot::archive::Archive::from_bytes(prot.to_vec())
        .map_err(|e| err(format!("parse PROT.DAT TOC: {e}")))?;
    Ok(archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect())
}

/// The CDNAME block a PROT entry belongs to.
///
/// `Archive` entry indices are extraction-frame indices, and CDNAME `#define`
/// numbers are raw in-RAM TOC indices, so the lookup must go through the +2
/// shift - reading the define numbers as extraction indices names the wrong
/// block near every block boundary.
pub(super) fn block_name(blocks: Option<&legaia_prot::cdname::IndexMap>, entry: i64) -> String {
    if entry < 0 {
        return "unindexed gap (boot UI)".to_string();
    }
    blocks
        .and_then(|m| legaia_prot::cdname::block_for_extraction_index(m, entry as u32))
        .unwrap_or("")
        .to_string()
}

/// One scan row as a JS object.
pub(super) fn row_js(
    row: &TexRow,
    replaceable: bool,
    block: &str,
    thumb: JsValue,
) -> Result<JsValue, JsValue> {
    let o = Object::new();
    let num = JsValue::from_f64;
    Reflect::set(&o, &"tier".into(), &row.coord.tier.into())?;
    Reflect::set(&o, &"entry".into(), &num(row.coord.entry as f64))?;
    Reflect::set(&o, &"section".into(), &num(row.coord.section as f64))?;
    Reflect::set(&o, &"offset".into(), &num(row.coord.offset as f64))?;
    Reflect::set(&o, &"width".into(), &num(row.width as f64))?;
    Reflect::set(&o, &"height".into(), &num(row.height as f64))?;
    Reflect::set(&o, &"bpp".into(), &num(row.bpp as f64))?;
    Reflect::set(&o, &"cluts".into(), &num(row.cluts as f64))?;
    Reflect::set(&o, &"bytes".into(), &num(row.bytes as f64))?;
    Reflect::set(
        &o,
        &"label".into(),
        &row.label.as_deref().unwrap_or("").into(),
    )?;
    // A 64-bit fingerprint does not survive a JS number, and a pack compares
    // it for equality - so it crosses as hex text, never as a float.
    Reflect::set(
        &o,
        &"fnv1a".into(),
        &format!("{:016x}", row.fnv1a).as_str().into(),
    )?;
    Reflect::set(&o, &"replaceable".into(), &JsValue::from_bool(replaceable))?;
    Reflect::set(&o, &"block".into(), &block.into())?;
    match row.vram {
        Some((x, y, w, h)) => {
            let v = Object::new();
            Reflect::set(&v, &"x".into(), &num(x as f64))?;
            Reflect::set(&v, &"y".into(), &num(y as f64))?;
            Reflect::set(&v, &"w".into(), &num(w as f64))?;
            Reflect::set(&v, &"h".into(), &num(h as f64))?;
            Reflect::set(&o, &"vram".into(), &v)?;
        }
        None => {
            Reflect::set(&o, &"vram".into(), &JsValue::NULL)?;
        }
    };
    match row.clut_vram {
        Some((x, y)) => {
            let v = Object::new();
            Reflect::set(&v, &"x".into(), &num(x as f64))?;
            Reflect::set(&v, &"y".into(), &num(y as f64))?;
            Reflect::set(&o, &"clut_vram".into(), &v)?;
        }
        None => {
            Reflect::set(&o, &"clut_vram".into(), &JsValue::NULL)?;
        }
    };
    Reflect::set(&o, &"thumb".into(), &thumb)?;
    Ok(o.into())
}

/// Scan a user-supplied disc image for every texture the registry can reach,
/// with thumbnails.
///
/// Returns `{ tiers: [{ id, title, about, replaceable, count }], textures:
/// [{ tier, entry, section, offset, width, height, bpp, cluts, bytes, label,
/// fnv1a, replaceable, block, vram, clut_vram, thumb }] }` plus `raw_count` /
/// `lzs_count` / `save_icon_count` for the page's headline note.
///
/// `entry` is `-1` for the unindexed gap before entry 0; `section` is `-1`
/// where the family does not use it. `thumb_max` caps the thumbnail's long
/// side (0 = no thumbnails). `fnv1a` is 16 hex digits.
#[wasm_bindgen]
pub fn scan_textures(image: Vec<u8>, thumb_max: u32) -> Result<JsValue, JsValue> {
    let DiscScanInput { prot, blocks, scus } = disc_scan_input(image)?;
    let spans = entry_spans(&prot)?;
    let ctx = ScanCtx::with_scus(&prot, &spans, scus.as_deref());

    let textures = js_sys::Array::new();
    let mut counts: Vec<(&'static str, usize)> =
        reg::tiers().iter().map(|t| (t.id, 0usize)).collect();

    // The sink thumbnails and drops each decode as it arrives - full-size
    // pixels for every texture on the disc would not fit in WASM memory.
    let mut sink_err: Option<JsValue> = None;
    {
        let mut sink = |row: TexRow, rgba: Option<Rgba>| -> Result<(), String> {
            let thumb = match (thumb_max, rgba) {
                (0, _) | (_, None) => JsValue::NULL,
                (max, Some(img)) => {
                    let (tw, th, small) = downscale_rgba(&img.data, img.w, img.h, max as usize);
                    rgba_js(tw, th, &small).unwrap_or(JsValue::NULL)
                }
            };
            let replaceable = reg::tier(row.coord.tier).is_some_and(|t| t.replaceable);
            let block = block_name(blocks.as_ref(), row.coord.entry);
            match row_js(&row, replaceable, &block, thumb) {
                Ok(js) => {
                    textures.push(&js);
                    if let Some(c) = counts.iter_mut().find(|(id, _)| *id == row.coord.tier) {
                        c.1 += 1;
                    }
                    Ok(())
                }
                Err(e) => {
                    sink_err = Some(e);
                    Err("could not build a row object".to_string())
                }
            }
        };
        if let Err(msg) = reg::scan_all(&ctx, thumb_max > 0, &mut sink) {
            return Err(sink_err.unwrap_or_else(|| err(msg)));
        }
    }

    let count_of = |id: &str| counts.iter().find(|(i, _)| *i == id).map_or(0, |c| c.1);
    let tiers = js_sys::Array::new();
    for t in reg::tiers() {
        let o = Object::new();
        Reflect::set(&o, &"id".into(), &t.id.into())?;
        Reflect::set(&o, &"title".into(), &t.title.into())?;
        Reflect::set(&o, &"about".into(), &t.about.into())?;
        Reflect::set(
            &o,
            &"replaceable".into(),
            &JsValue::from_bool(t.replaceable),
        )?;
        Reflect::set(
            &o,
            &"count".into(),
            &JsValue::from_f64(count_of(t.id) as f64),
        )?;
        tiers.push(&o);
    }

    let out = Object::new();
    let num = JsValue::from_f64;
    // Kept for the page's headline note. These are emitted-row counts (what
    // the grid actually offers), not catalog lengths.
    Reflect::set(
        &out,
        &"raw_count".into(),
        &num(count_of(reg::TIER_RAW) as f64),
    )?;
    Reflect::set(
        &out,
        &"lzs_count".into(),
        &num(count_of(reg::TIER_LZS) as f64),
    )?;
    Reflect::set(
        &out,
        &"save_icon_count".into(),
        &num(count_of(reg::TIER_SAVE_ICON) as f64),
    )?;
    Reflect::set(&out, &"tiers".into(), &tiers)?;
    Reflect::set(&out, &"textures".into(), &textures)?;
    Ok(out.into())
}

/// Decode one texture full-size straight from the disc, without going near
/// the writer. This is how a read-only family previews and exports.
/// Returns `{ w, h, rgba }`.
#[wasm_bindgen]
pub fn decode_texture(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let input = disc_scan_input(image)?;
    let spans = entry_spans(&input.prot)?;
    let ctx = ScanCtx::new(&input.prot, &spans);
    let img = reg::read_row(&ctx, &coord).map_err(err)?;
    rgba_js(img.w, img.h, &img.data)
}

/// Validate one texture replacement against the user's disc and build the
/// side-by-side preview. Never writes.
///
/// Returns `{ ok, error, original: { w, h, rgba }, preview: { w, h, rgba } |
/// null, width, height, bpp, cluts, new_palette_entries, quantized_pixels,
/// fit: { capacity, recompressed } | null }`. `preview` is the replacement as
/// it will *display* on disc (15-bit rounding + any quantization applied), so
/// what the user sees is what the game gets.
///
/// One entry point for every family: the registry decides which writer a
/// coordinate resolves to.
///
/// TIM families take any shape `tim-replace` does (image through any palette
/// or the in-game palettes, composite, palette strip, indexed PNG) and add
/// `import_kind` (what the PNG was recognised as) and
/// `palette_entries_changed`. `view` (`-1` = the in-game palettes, `k` =
/// palette `k`, absent = the in-game palettes when known) picks how the
/// original is drawn.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn preview_texture_replace(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
    png: &[u8],
    quantize: bool,
    view: Option<i32>,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let op = reg::replace_op(&coord).map_err(err)?;

    let out = Object::new();
    let num = JsValue::from_f64;
    let fail = |out: &Object, msg: String| -> Result<JsValue, JsValue> {
        Reflect::set(out, &"ok".into(), &JsValue::from_bool(false))?;
        Reflect::set(out, &"error".into(), &msg.as_str().into())?;
        Ok(out.clone().into())
    };

    match op {
        ReplaceOp::SaveIconSlot(slot) => {
            use legaia_asset::save_icon as si;
            let size = si::TILE_SIZE;
            Reflect::set(&out, &"width".into(), &num(size as f64))?;
            Reflect::set(&out, &"height".into(), &num(size as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(1.0))?;
            let sheet = save_icon::read_sheet(&patcher)
                .map_err(|e| err(format!("read save-icon sheet: {e:#}")))?;
            let original = match save_icon::export_slot(&sheet, slot) {
                Ok(rgba) => rgba,
                Err(e) => return fail(&out, format!("{e:#}")),
            };
            Reflect::set(&out, &"original".into(), &rgba_js(size, size, &original)?)?;
            let (w, h, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            if (w, h) != (size, size) {
                return fail(
                    &out,
                    format!("a save-slot portrait must be {size}x{size}, got {w}x{h}"),
                );
            }
            match save_icon::preview_slot(&sheet, slot, &rgba, quantize) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(size, size, &p.rgba)?)?;
                    Reflect::set(
                        &out,
                        &"new_palette_entries".into(),
                        &num(p.palette_entries_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_pixels as f64),
                    )?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
        ReplaceOp::Tim(target) => {
            let orig = legaia_patcher::texture::read_texture(&patcher, &target)
                .map_err(|e| err(format!("read texture: {e:#}")))?;
            let pals = texture_palettes(&patcher, &orig.tim)
                .map_err(|e| err(format!("palette map: {e:#}")))?;
            let (ow, oh) = (orig.tim.pixel_width(), orig.tim.pixel_height());
            let base_view = view_of(view);
            let orig_rgba = legaia_patcher::texture::decode_view(&orig.tim, &pals, base_view)
                .map_err(|e| err(format!("decode original: {e}")))?;
            Reflect::set(&out, &"original".into(), &rgba_js(ow, oh, &orig_rgba)?)?;
            Reflect::set(&out, &"width".into(), &num(ow as f64))?;
            Reflect::set(&out, &"height".into(), &num(oh as f64))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.tim.palette_count() as f64))?;
            if png.is_empty() {
                return fail(&out, "no PNG chosen".to_string());
            }

            let opts = EncodeOptions {
                quantize,
                ..Default::default()
            };
            // One call: the same recognition, encode and (for the compressed
            // tier) recompression the write performs, stopped before the
            // patch.
            let outcome = match replace_texture_png(&mut patcher, &target, png, &opts, true) {
                Ok(o) => o,
                Err(e) => return fail(&out, format!("{e:#}")),
            };
            let imp = legaia_tim::multi_palette::import_png(&orig.tim, png, &pals.context, &opts)
                .map_err(|e| err(format!("{e:#}")))?;
            let ptim = legaia_tim::parse(&imp.encoded.bytes)
                .map_err(|e| err(format!("re-parse encoded TIM: {e}")))?;
            // Show the result the way the edit was made: through the view it
            // was drawn in, or (strip / indexed) through the page's view.
            let shown = match imp.kind {
                ImportKind::Image(v) | ImportKind::Composite(v) => v,
                ImportKind::Indexed(p) => View::Palette(p),
                ImportKind::PaletteStrip => base_view,
            };
            let prgba = legaia_patcher::texture::decode_view(&ptim, &pals, shown)
                .map_err(|e| err(format!("decode encoded TIM: {e}")))?;
            Reflect::set(&out, &"preview".into(), &rgba_js(ow, oh, &prgba)?)?;
            Reflect::set(&out, &"import_kind".into(), &imp.kind.to_string().into())?;
            Reflect::set(
                &out,
                &"palette_entries_changed".into(),
                &num(outcome.palette_entries_changed as f64),
            )?;
            Reflect::set(
                &out,
                &"new_palette_entries".into(),
                &num(outcome.new_palette_entries as f64),
            )?;
            Reflect::set(
                &out,
                &"quantized_pixels".into(),
                &num(outcome.quantized_pixels as f64),
            )?;
            Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
            Reflect::set(&out, &"error".into(), &"".into())?;
            Reflect::set(&out, &"bpp".into(), &num(outcome.bpp as f64))?;
            if let Some(fit) = outcome.lzs {
                let f = Object::new();
                Reflect::set(&f, &"capacity".into(), &num(fit.capacity as f64))?;
                Reflect::set(&f, &"recompressed".into(), &num(fit.recompressed as f64))?;
                Reflect::set(&out, &"fit".into(), &f)?;
            }
        }
        ReplaceOp::BattleEquip(target) => {
            let orig = battle_texture::export_block(&patcher, &target, reg::BATTLE_PREVIEW_PALETTE)
                .map_err(|e| err(format!("read battle texture: {e:#}")))?;
            Reflect::set(
                &out,
                &"original".into(),
                &rgba_js(orig.width, orig.height, &orig.rgba)?,
            )?;
            Reflect::set(&out, &"width".into(), &num(orig.width as f64))?;
            Reflect::set(&out, &"height".into(), &num(orig.height as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.palette_count as f64))?;

            let (pw, ph, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            // One call: the same encode and the same recompression the write
            // performs, stopped before the patch. A separate "preview" encode
            // could disagree with the writer about a folded colour.
            match battle_texture::preview_block(
                &patcher,
                &target,
                &rgba,
                pw,
                ph,
                reg::BATTLE_PREVIEW_PALETTE,
                quantize,
            ) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(pw, ph, &p.rgba)?)?;
                    Reflect::set(
                        &out,
                        &"new_palette_entries".into(),
                        &num(p.palette_entries_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_pixels as f64),
                    )?;
                    let f = Object::new();
                    Reflect::set(&f, &"capacity".into(), &num(p.fit.capacity as f64))?;
                    Reflect::set(&f, &"recompressed".into(), &num(p.fit.recompressed as f64))?;
                    Reflect::set(&out, &"fit".into(), &f)?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
        ReplaceOp::MonsterPage(target) => {
            let orig = monster_texture::export_page(&patcher, &target)
                .map_err(|e| err(format!("read monster texture: {e:#}")))?;
            Reflect::set(
                &out,
                &"original".into(),
                &rgba_js(orig.width, orig.height, &orig.rgba)?,
            )?;
            Reflect::set(&out, &"width".into(), &num(orig.width as f64))?;
            Reflect::set(&out, &"height".into(), &num(orig.height as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.palettes_populated as f64))?;

            let (pw, ph, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            match monster_texture::preview_page(&patcher, &target, &rgba, pw, ph, quantize) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(pw, ph, &p.rgba)?)?;
                    // This family never rewrites a palette (a monster's CLUTs
                    // upload verbatim, so their blend bits are live state), so
                    // the page's own counter is texels re-indexed instead.
                    Reflect::set(&out, &"new_palette_entries".into(), &num(0.0))?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_texels as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"texels_changed".into(),
                        &num(p.texels_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"dead_texels_ignored".into(),
                        &num(p.dead_texels_ignored as f64),
                    )?;
                    let f = Object::new();
                    Reflect::set(&f, &"capacity".into(), &num(p.fit.capacity as f64))?;
                    Reflect::set(&f, &"recompressed".into(), &num(p.fit.recompressed as f64))?;
                    Reflect::set(&out, &"fit".into(), &f)?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
    }
    Ok(out.into())
}

/// One queued replacement, read off a JS spec object.
pub(super) struct Spec {
    pub(super) coord: TexCoord,
    pub(super) png: Vec<u8>,
    pub(super) quantize: bool,
}

pub(super) fn read_spec(spec: &JsValue) -> Result<Spec, JsValue> {
    let get_num = |k: &str| -> Result<f64, JsValue> {
        Reflect::get(spec, &k.into())?
            .as_f64()
            .ok_or_else(|| err(format!("texture spec missing numeric {k}")))
    };
    let tier = Reflect::get(spec, &"tier".into())?
        .as_string()
        .ok_or_else(|| err("texture spec missing tier"))?;
    Ok(Spec {
        coord: coord_of(
            &tier,
            get_num("entry")? as i32,
            get_num("section")? as i32,
            get_num("offset")?,
        )?,
        png: Uint8Array::from(Reflect::get(spec, &"png".into())?).to_vec(),
        quantize: Reflect::get(spec, &"quantize".into())?
            .as_bool()
            .unwrap_or(false),
    })
}

/// Apply a queue of validated texture replacements to a disc image. `specs`
/// is an array of `{ tier, entry, section, offset, png: Uint8Array, quantize
/// }` (same coordinate conventions as [`preview_texture_replace`]). Applied
/// in order; a failing spec aborts with its error (nothing partial is
/// returned). Returns `{ data, summary }` - the same shape the page consumes
/// from [`patch_rom`], so texture patches chain after a randomizer run.
///
/// Async with the same optional trailing `progress` callback as [`patch_rom`]:
/// one stage to parse the disc, one per replacement spec, one to assemble the
/// output image.
#[wasm_bindgen]
pub async fn apply_texture_replacements(
    image: Vec<u8>,
    specs: JsValue,
    progress: Option<js_sys::Function>,
) -> Result<JsValue, JsValue> {
    let list = js_sys::Array::from(&specs);
    let mut prog = Progress::new(progress, list.length() + 2);
    prog.stage("parsing disc image").await;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let mut summary = String::new();
    for (i, raw) in list.iter().enumerate() {
        prog.stage(&format!("texture {} of {}", i + 1, list.length()))
            .await;
        let spec = read_spec(&raw)?;
        let at = format!(
            "{} entry {} section {} +0x{:X}",
            spec.coord.tier, spec.coord.entry, spec.coord.section, spec.coord.offset
        );
        let op =
            reg::replace_op(&spec.coord).map_err(|e| err(format!("texture {i} ({at}): {e}")))?;
        let (w, h, rgba) =
            decode_png_rgba(&spec.png).map_err(|e| err(format!("texture {i} ({at}): {e}")))?;
        match op {
            ReplaceOp::SaveIconSlot(slot) => {
                let size = legaia_asset::save_icon::TILE_SIZE;
                if (w, h) != (size, size) {
                    return Err(err(format!(
                        "save-icon {i} (slot {slot}): portrait must be {size}x{size}, got {w}x{h}"
                    )));
                }
                let outcome = save_icon::replace_slot(&mut patcher, slot, &rgba, spec.quantize)
                    .map_err(|e| err(format!("save-icon {i} (slot {slot}): {e:#}")))?;
                summary.push_str(&format!(
                    "save icon: slot {} (save number {}) replaced{}\n",
                    outcome.slot,
                    outcome.slot + 1,
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                ));
            }
            ReplaceOp::Tim(target) => {
                let outcome = replace_texture_png(
                    &mut patcher,
                    &target,
                    &spec.png,
                    &EncodeOptions {
                        quantize: spec.quantize,
                        ..Default::default()
                    },
                    false,
                )
                .map_err(|e| err(format!("texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "texture: {target} replaced ({}x{} {} bpp{}{}{})\n",
                    outcome.width,
                    outcome.height,
                    outcome.bpp,
                    if outcome.new_palette_entries > 0 {
                        format!(", {} new palette color(s)", outcome.new_palette_entries)
                    } else {
                        String::new()
                    },
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                    match outcome.lzs {
                        Some(f) => format!(
                            ", recompressed {}B into the {}B stream",
                            f.recompressed, f.capacity
                        ),
                        None => String::new(),
                    },
                ));
            }
            ReplaceOp::BattleEquip(target) => {
                let outcome = battle_texture::replace_block(
                    &mut patcher,
                    &target,
                    &rgba,
                    w,
                    h,
                    reg::BATTLE_PREVIEW_PALETTE,
                    spec.quantize,
                    false,
                )
                .map_err(|e| err(format!("battle texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "battle art: {target} replaced ({}x{} 4 bpp, {}{}{})\n",
                    outcome.width,
                    outcome.height,
                    outcome.palette,
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                    if outcome.unchanged {
                        " - identical to retail, nothing written".to_string()
                    } else {
                        format!(
                            ", recompressed {}B into the {}B slot",
                            outcome.fit.recompressed, outcome.fit.capacity
                        )
                    },
                ));
            }
            ReplaceOp::MonsterPage(target) => {
                let outcome = monster_texture::replace_page(
                    &mut patcher,
                    &target,
                    &rgba,
                    w,
                    h,
                    spec.quantize,
                    false,
                )
                .map_err(|e| err(format!("monster texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "monster skin: {} #{} repainted ({}x{} 4 bpp, {} texel(s) changed{}{}{})\n",
                    outcome.name,
                    outcome.id,
                    outcome.width,
                    outcome.height,
                    outcome.texels_changed,
                    if outcome.quantized_texels > 0 {
                        format!(", {} folded onto a nearer colour", outcome.quantized_texels)
                    } else {
                        String::new()
                    },
                    if outcome.dead_texels_ignored > 0 {
                        format!(
                            ", {} painted where nothing samples the page (ignored)",
                            outcome.dead_texels_ignored
                        )
                    } else {
                        String::new()
                    },
                    if outcome.unchanged {
                        " - identical to retail, nothing written".to_string()
                    } else {
                        format!(
                            ", recompressed {}B into the {}B slot",
                            outcome.fit.recompressed, outcome.fit.capacity
                        )
                    },
                ));
            }
        }
    }
    if list.length() == 0 {
        summary.push_str("textures: untouched\n");
    }

    prog.stage("assembling patched image").await;
    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);
    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Ok(out.into())
}

// --- Multi-palette views -----------------------------------------------------

/// The page's view number: `-1` (or absent) = the in-game palettes, `k` =
/// palette `k`.
fn view_of(v: Option<i32>) -> View {
    match v {
        Some(k) if k >= 0 => View::Palette(k as usize),
        _ => View::InGame,
    }
}

fn tim_target(coord: &TexCoord) -> Result<legaia_patcher::texture::TextureTarget, JsValue> {
    match reg::replace_op(coord).map_err(err)? {
        ReplaceOp::Tim(t) => Ok(t),
        _ => Err(err(
            "palette views apply to TIM textures only (this family has its own palette rules)",
        )),
    }
}

/// What is known about a TIM texture's palettes: `{ count, has_map, source,
/// notes: [..], unclaimed, regions: [{ x, y, w, h, palette, subpalette,
/// widgets: [..], part }] }`. `palette >= count` is a read-only palette of a
/// sibling texture. Families other than TIM report `{ count: 0 }`.
#[wasm_bindgen]
pub fn texture_palette_info(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let out = Object::new();
    let num = JsValue::from_f64;
    let Ok(target) = tim_target(&coord) else {
        Reflect::set(&out, &"count".into(), &num(0.0))?;
        return Ok(out.into());
    };
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let orig = legaia_patcher::texture::read_texture(&patcher, &target)
        .map_err(|e| err(format!("read texture: {e:#}")))?;
    let pals = texture_palettes(&patcher, &orig.tim).map_err(|e| err(format!("{e:#}")))?;
    Reflect::set(&out, &"count".into(), &num(orig.tim.palette_count() as f64))?;
    Reflect::set(&out, &"has_map".into(), &JsValue::from_bool(pals.has_map()))?;
    Reflect::set(&out, &"source".into(), &pals.source.as_str().into())?;
    Reflect::set(
        &out,
        &"unclaimed".into(),
        &num(pals.unclaimed_pixels as f64),
    )?;
    Reflect::set(
        &out,
        &"contested".into(),
        &num(pals.contested_pixels as f64),
    )?;
    let notes = js_sys::Array::new();
    for n in &pals.notes {
        notes.push(&n.as_str().into());
    }
    Reflect::set(&out, &"notes".into(), &notes)?;
    // One row per distinct (rect, palette, part), with every widget id.
    let mut rows: Vec<(&legaia_patcher::texture_palettes::PaletteRegion, Vec<u8>)> = Vec::new();
    for r in &pals.regions {
        match rows
            .iter_mut()
            .find(|(k, _)| k.rect == r.rect && k.palette == r.palette && k.part == r.part)
        {
            Some((_, ids)) => ids.push(r.widget),
            None => rows.push((r, vec![r.widget])),
        }
    }
    let regions = js_sys::Array::new();
    for (r, ids) in rows {
        let o = Object::new();
        Reflect::set(&o, &"x".into(), &num(r.rect.0 as f64))?;
        Reflect::set(&o, &"y".into(), &num(r.rect.1 as f64))?;
        Reflect::set(&o, &"w".into(), &num(r.rect.2 as f64))?;
        Reflect::set(&o, &"h".into(), &num(r.rect.3 as f64))?;
        Reflect::set(&o, &"palette".into(), &num(r.palette as f64))?;
        Reflect::set(&o, &"subpalette".into(), &num(r.subpalette as f64))?;
        Reflect::set(
            &o,
            &"part".into(),
            &format!("{:?}", r.part).to_lowercase().into(),
        )?;
        let w = js_sys::Array::new();
        for id in ids {
            w.push(&num(id as f64));
        }
        Reflect::set(&o, &"widgets".into(), &w)?;
        regions.push(&o);
    }
    Reflect::set(&out, &"regions".into(), &regions)?;
    Ok(out.into())
}

/// Decode a TIM texture through a view (`-1` = in-game palettes, `k` =
/// palette `k`). Returns `{ w, h, rgba }`.
#[wasm_bindgen]
pub fn decode_texture_view(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
    view: i32,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let target = tim_target(&coord)?;
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let orig = legaia_patcher::texture::read_texture(&patcher, &target)
        .map_err(|e| err(format!("read texture: {e:#}")))?;
    let pals = texture_palettes(&patcher, &orig.tim).map_err(|e| err(format!("{e:#}")))?;
    let rgba = legaia_patcher::texture::decode_view(&orig.tim, &pals, view_of(Some(view)))
        .map_err(|e| err(format!("decode: {e}")))?;
    rgba_js(orig.tim.pixel_width(), orig.tim.pixel_height(), &rgba)
}

/// Encode a TIM texture as a download: `format` is `image` / `composite` /
/// `strip` / `indexed`, `view` as in [`decode_texture_view`]. Returns `{
/// png: Uint8Array, w, h, description }`. Every shape comes back through
/// [`preview_texture_replace`] / [`apply_texture_replacements`] unchanged.
#[wasm_bindgen]
pub fn export_texture_as(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
    format: &str,
    view: i32,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let target = tim_target(&coord)?;
    let format: ExportFormat = format.parse().map_err(err)?;
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let ex =
        legaia_patcher::texture::export_texture_png(&patcher, &target, format, view_of(Some(view)))
            .map_err(|e| err(format!("{e:#}")))?;
    let o = Object::new();
    let arr = Uint8Array::new_with_length(ex.png.len() as u32);
    arr.copy_from(&ex.png);
    Reflect::set(&o, &"png".into(), &arr)?;
    Reflect::set(&o, &"w".into(), &JsValue::from_f64(ex.width as f64))?;
    Reflect::set(&o, &"h".into(), &JsValue::from_f64(ex.height as f64))?;
    Reflect::set(&o, &"description".into(), &ex.description.as_str().into())?;
    Ok(o.into())
}

// --- Change packs -----------------------------------------------------------

/// Serialize a queue of replacements into a shareable texture change pack.
///
/// `specs` adds `fnv1a` (16 hex digits), `width`, `height`, `bpp` and `label`
/// to the shape [`apply_texture_replacements`] takes - the fingerprint of the
/// *retail* texture the edit was authored against, which is what lets an
/// import verify it landed on the right disc.
///
/// A pack carries the user's own images plus those fingerprints. It never
/// carries retail pixels, so it is shareable; that is enforced by the pack
/// module, not by this binding.
#[wasm_bindgen]
pub fn export_texture_pack(
    specs: JsValue,
    name: &str,
    author: &str,
    note: &str,
) -> Result<String, JsValue> {
    let list = js_sys::Array::from(&specs);
    let mut entries = Vec::with_capacity(list.length() as usize);
    for raw in list.iter() {
        let spec = read_spec(&raw)?;
        let hex = Reflect::get(&raw, &"fnv1a".into())?
            .as_string()
            .ok_or_else(|| err("texture spec missing fnv1a"))?;
        let original_fnv1a = u64::from_str_radix(&hex, 16)
            .map_err(|_| err(format!("fnv1a is not 16 hex digits: {hex:?}")))?;
        let num = |k: &str| -> u32 {
            Reflect::get(&raw, &k.into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32
        };
        entries.push(PackEntry {
            coord: spec.coord,
            original_fnv1a,
            original_width: num("width"),
            original_height: num("height"),
            original_bpp: num("bpp"),
            label: Reflect::get(&raw, &"label".into())?
                .as_string()
                .unwrap_or_default(),
            quantize: spec.quantize,
            png: spec.png,
        });
    }
    let meta = PackMeta {
        name: name.to_string(),
        author: author.to_string(),
        note: note.to_string(),
    };
    Ok(texture_pack::to_json(&meta, &entries))
}

/// Read a texture change pack and grade every entry against the user's own
/// disc.
///
/// Returns `{ name, author, note, version, entries: [{ tier, entry, section,
/// offset, label, quantize, width, height, fnv1a, status, detail, usable, png
/// }] }`. `status` is one of `ok` / `unknown-family` / `not-found` /
/// `hash-mismatch` / `size-mismatch`; `detail` is a sentence to show; `usable`
/// says whether the page may queue it.
///
/// Verification reads the *current* image, so a texture already patched on
/// this disc reports `hash-mismatch` rather than being replaced twice.
/// `accept_hash_mismatch` marks those usable anyway - the deliberate
/// "re-apply on top of my own edit" case.
#[wasm_bindgen]
pub fn import_texture_pack(
    image: Vec<u8>,
    json: &str,
    accept_hash_mismatch: bool,
) -> Result<JsValue, JsValue> {
    let pack = texture_pack::from_json(json).map_err(err)?;
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;

    let entries = js_sys::Array::new();
    for e in &pack.entries {
        let status = texture_pack::verify(&patcher, e);
        let usable = match &status {
            texture_pack::EntryStatus::Ok => true,
            texture_pack::EntryStatus::HashMismatch { .. } => accept_hash_mismatch,
            _ => false,
        };
        let o = Object::new();
        let num = JsValue::from_f64;
        Reflect::set(&o, &"tier".into(), &e.coord.tier.into())?;
        Reflect::set(&o, &"entry".into(), &num(e.coord.entry as f64))?;
        Reflect::set(&o, &"section".into(), &num(e.coord.section as f64))?;
        Reflect::set(&o, &"offset".into(), &num(e.coord.offset as f64))?;
        Reflect::set(&o, &"width".into(), &num(e.original_width as f64))?;
        Reflect::set(&o, &"height".into(), &num(e.original_height as f64))?;
        Reflect::set(&o, &"bpp".into(), &num(e.original_bpp as f64))?;
        Reflect::set(
            &o,
            &"fnv1a".into(),
            &format!("{:016x}", e.original_fnv1a).as_str().into(),
        )?;
        Reflect::set(&o, &"label".into(), &e.label.as_str().into())?;
        Reflect::set(&o, &"quantize".into(), &JsValue::from_bool(e.quantize))?;
        Reflect::set(&o, &"status".into(), &status.tag().into())?;
        Reflect::set(&o, &"detail".into(), &status.detail().as_str().into())?;
        Reflect::set(&o, &"usable".into(), &JsValue::from_bool(usable))?;
        let png = Uint8Array::new_with_length(e.png.len() as u32);
        png.copy_from(&e.png);
        Reflect::set(&o, &"png".into(), &png)?;
        entries.push(&o);
    }

    let out = Object::new();
    Reflect::set(&out, &"name".into(), &pack.meta.name.as_str().into())?;
    Reflect::set(&out, &"author".into(), &pack.meta.author.as_str().into())?;
    Reflect::set(&out, &"note".into(), &pack.meta.note.as_str().into())?;
    Reflect::set(
        &out,
        &"version".into(),
        &JsValue::from_f64(pack.version as f64),
    )?;
    Reflect::set(&out, &"entries".into(), &entries)?;
    Ok(out.into())
}
