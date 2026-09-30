//! Multi-palette texture editing: one set of pixel indices, several CLUTs.
//!
//! A 4/8 bpp TIM stores **indices**, and every sprite the game cuts out of it
//! picks its own palette (the CLUT address in the draw packet). A texture
//! with sixteen palettes is therefore not one image but sixteen colourings of
//! the same indices, and which colouring a region is *meant* to be seen in is
//! a property of the draw site, not of the file. Exporting "the texture as a
//! PNG" through one palette shows most regions in the wrong colours, and a
//! colour edit made against that view can only be written back into that one
//! palette.
//!
//! This module is the file-format half of editing such a texture:
//!
//! * a per-pixel **palette map** (`map[i]` = which palette pixel `i` is drawn
//!   with) turns the indices into the image the game actually shows
//!   ([`decode_mapped`]); a caller that knows the draw sites supplies it,
//!   everyone else uses [`uniform_map`];
//! * a **palette strip** ([`render_strip`] / [`read_strip`]) lays every
//!   palette out as one row of solid colour cells, so colours can be edited
//!   without touching a single index;
//! * a **composite** ([`render_composite`]) is the mapped image with the
//!   strip appended below it - one file that carries both halves;
//! * an **indexed PNG** ([`indexed_png`]) carries the indices verbatim with
//!   one palette as its `PLTE`, for editors that paint in indices;
//! * [`import_png`] recognises which of those shapes a PNG is and writes it
//!   back into a TIM of exactly the original layout, leaving every palette
//!   the edit did not touch byte-identical.
//!
//! The palettes a map may point at are the TIM's own (editable) followed by
//! any **external** palettes the caller supplies - CLUTs that live in a
//! different file but that the game draws some of this texture's pixels
//! through. Those are read-only here: a pixel drawn through one must be
//! painted with a colour that palette already has.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::encode::{ColorSample, EncodeError, EncodeOptions, Encoded, rgba8_to_bgr555, serialize};
use crate::{PixelMode, Tim, bgr555_to_rgba8};

/// Entries per palette for an indexed mode (`None` for 16/24 bpp).
pub fn entries_per_palette(tim: &Tim) -> Option<usize> {
    match tim.mode {
        PixelMode::Bpp4 => Some(16),
        PixelMode::Bpp8 => Some(256),
        _ => None,
    }
}

/// The TIM's own palettes, in the flat chunk order [`crate::Clut::palette`]
/// reads.
pub fn own_palettes(tim: &Tim) -> Vec<Vec<u16>> {
    let n = tim.palette_count();
    (0..n)
        .filter_map(|p| {
            tim.clut
                .as_ref()
                .and_then(|c| c.palette(tim.mode, p))
                .map(<[u16]>::to_vec)
        })
        .collect()
}

/// Every pixel's palette index, row-major (`w * h` values).
pub fn indices(tim: &Tim) -> Vec<u8> {
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let stride = tim.image.fb_w as usize * 2;
    let mut out = Vec::with_capacity(w * h);
    for row in 0..h {
        for col in 0..w {
            let v = match tim.mode {
                PixelMode::Bpp4 => {
                    let b = tim.image.data[row * stride + col / 2];
                    if col & 1 == 0 { b & 0x0F } else { b >> 4 }
                }
                _ => tim.image.data[row * stride + col],
            };
            out.push(v);
        }
    }
    out
}

/// Pack row-major indices back into the TIM's image block.
fn pack_indices(tim: &mut Tim, idx: &[u8]) {
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let stride = tim.image.fb_w as usize * 2;
    let mut data = vec![0u8; stride * h];
    for row in 0..h {
        for col in 0..w {
            let v = idx[row * w + col];
            match tim.mode {
                PixelMode::Bpp4 => {
                    let b = &mut data[row * stride + col / 2];
                    *b |= if col & 1 == 0 {
                        v & 0x0F
                    } else {
                        (v & 0x0F) << 4
                    };
                }
                _ => data[row * stride + col] = v,
            }
        }
    }
    tim.image.data = data;
}

/// A map that draws every pixel through palette `p`.
pub fn uniform_map(tim: &Tim, p: u16) -> Vec<u16> {
    vec![p; tim.pixel_width() * tim.pixel_height()]
}

/// Decode `tim`'s indices through a per-pixel palette choice. `sets` is the
/// full palette list the map indexes (own palettes, then external ones).
pub fn decode_mapped(tim: &Tim, sets: &[Vec<u16>], map: &[u16]) -> Result<Vec<u8>> {
    let idx = indices(tim);
    if map.len() != idx.len() {
        bail!(
            "palette map has {} entries, texture has {} pixels",
            map.len(),
            idx.len()
        );
    }
    let mut out = Vec::with_capacity(idx.len() * 4);
    for (i, &k) in idx.iter().enumerate() {
        let pal = sets
            .get(map[i] as usize)
            .with_context(|| format!("palette map names palette {} of {}", map[i], sets.len()))?;
        let c = pal.get(k as usize).copied().unwrap_or(0);
        out.extend_from_slice(&bgr555_to_rgba8(c));
    }
    Ok(out)
}

// --- palette strip ----------------------------------------------------------

/// Shape of a palette strip: `count` rows of `per` cells, each cell
/// `cell_w x cell_h` pixels of one solid colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripGeom {
    pub per: usize,
    pub count: usize,
    pub cell_w: usize,
    pub cell_h: usize,
}

impl StripGeom {
    pub fn width(&self) -> usize {
        self.per * self.cell_w
    }
    pub fn height(&self) -> usize {
        self.count * self.cell_h
    }
}

/// Cell height of the strip a composite carries below the image.
pub const COMPOSITE_CELL_H: usize = 8;

/// The strip geometry a standalone palette-strip download uses: big, square
/// cells for 4 bpp (easy to click in any editor), thin ones for 8 bpp. The
/// height is nudged so a strip can never be mistaken for the texture itself.
pub fn standalone_strip_geom(tim: &Tim) -> Option<StripGeom> {
    let per = entries_per_palette(tim)?;
    let count = tim.palette_count();
    if count == 0 {
        return None;
    }
    let cell_w = if per == 16 { 16 } else { 2 };
    let dims = (tim.pixel_width(), tim.pixel_height());
    let composite = composite_geom(tim).map(|g| (dims.0, dims.1 + g.height()));
    for cell_h in [16usize, 12, 10, 9] {
        let g = StripGeom {
            per,
            count,
            cell_w,
            cell_h,
        };
        let d = (g.width(), g.height());
        if d != dims && Some(d) != composite {
            return Some(g);
        }
    }
    None
}

/// The strip a composite appends: the image's own width split into `per`
/// cells (any remainder is left transparent), [`COMPOSITE_CELL_H`] rows per
/// palette. `None` when the image is narrower than one pixel per entry.
pub fn composite_geom(tim: &Tim) -> Option<StripGeom> {
    let per = entries_per_palette(tim)?;
    let count = tim.palette_count();
    let cell_w = tim.pixel_width() / per;
    if count == 0 || cell_w == 0 {
        return None;
    }
    Some(StripGeom {
        per,
        count,
        cell_w,
        cell_h: COMPOSITE_CELL_H,
    })
}

/// Render `sets[..geom.count]` as a strip `width` pixels wide (`width >=
/// geom.width()`; the remainder is transparent).
pub fn render_strip(sets: &[Vec<u16>], geom: StripGeom, width: usize) -> Vec<u8> {
    let h = geom.height();
    let mut out = vec![0u8; width * h * 4];
    for (p, pal) in sets.iter().take(geom.count).enumerate() {
        for (e, &entry) in pal.iter().take(geom.per).enumerate() {
            let c = bgr555_to_rgba8(entry);
            for y in p * geom.cell_h..(p + 1) * geom.cell_h {
                for x in e * geom.cell_w..(e + 1) * geom.cell_w {
                    out[(y * width + x) * 4..(y * width + x) * 4 + 4].copy_from_slice(&c);
                }
            }
        }
    }
    out
}

/// Map an edited strip colour back to a 16-bit CLUT entry, keeping the
/// original entry verbatim when it still displays the same colour (so its
/// STP bit survives), and otherwise keeping the original entry's STP bit
/// unless the alpha asks for something else explicitly:
/// `a == 0` -> transparent `0x0000`; `0 < a < 255` -> STP set; `a == 255`
/// -> the original entry's STP bit. An opaque colour that would encode as
/// `0x0000` becomes `0x8000` (STP-only black), as the image encoder does.
pub fn strip_entry(orig: u16, px: [u8; 4]) -> u16 {
    if bgr555_to_rgba8(orig) == bgr555_to_rgba8(rgba8_to_bgr555(px)) && px[3] == 255 {
        return orig;
    }
    if px[3] == 0 {
        return if bgr555_to_rgba8(orig)[3] == 0 {
            orig
        } else {
            0x0000
        };
    }
    let rgb = rgba8_to_bgr555([px[0], px[1], px[2], 255]) & 0x7FFF;
    let stp = if px[3] < 255 { 0x8000 } else { orig & 0x8000 };
    let c = rgb | stp;
    if c == 0 { 0x8000 } else { c }
}

/// Read `count` palettes of `per` entries back out of a strip of any integer
/// cell size (`w` must be a multiple of `per`, `h` of `count`). Every cell
/// must be one solid colour - a cell that is not says which one, because a
/// strip that was resized with smoothing is the usual cause.
pub fn read_strip(
    rgba: &[u8],
    w: usize,
    h: usize,
    geom: StripGeom,
    originals: &[Vec<u16>],
) -> Result<Vec<Vec<u16>>> {
    if geom.width() > w || geom.height() > h {
        bail!(
            "palette strip is {w}x{h}, needs at least {}x{}",
            geom.width(),
            geom.height()
        );
    }
    let px = |x: usize, y: usize| -> [u8; 4] {
        let o = (y * w + x) * 4;
        rgba[o..o + 4].try_into().unwrap()
    };
    let mut out = Vec::with_capacity(geom.count);
    for (p, orig) in originals.iter().take(geom.count).enumerate() {
        let mut pal = Vec::with_capacity(geom.per);
        for (e, &orig_entry) in orig.iter().take(geom.per).enumerate() {
            let (x0, y0) = (e * geom.cell_w, p * geom.cell_h);
            let c = px(x0, y0);
            for y in y0..y0 + geom.cell_h {
                for x in x0..x0 + geom.cell_w {
                    let q = px(x, y);
                    // Alpha 0 is one colour however its RGB is stored.
                    let same = q == c || (q[3] == 0 && c[3] == 0);
                    if !same {
                        bail!(
                            "palette {p} colour {e} (cell at {x0},{y0}) is not one solid colour - \
                             paint each cell flat and do not resize the strip with smoothing"
                        );
                    }
                }
            }
            pal.push(strip_entry(orig_entry, c));
        }
        out.push(pal);
    }
    Ok(out)
}

/// The composite download: the image drawn through `map`, with the strip of
/// the TIM's own palettes appended below it.
pub fn render_composite(
    tim: &Tim,
    sets: &[Vec<u16>],
    map: &[u16],
) -> Result<(usize, usize, Vec<u8>)> {
    let geom = composite_geom(tim).context("texture has no palettes to append")?;
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let mut rgba = decode_mapped(tim, sets, map)?;
    rgba.extend_from_slice(&render_strip(sets, geom, w));
    Ok((w, h + geom.height(), rgba))
}

// --- PNG I/O -----------------------------------------------------------------

/// Encode an RGBA8 image as a PNG.
pub fn rgba_png(w: usize, h: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().context("write PNG header")?;
        wr.write_image_data(rgba).context("write PNG data")?;
    }
    Ok(out)
}

/// An 8-bit indexed PNG of the texture's indices, with `palette` as its
/// `PLTE` (+ `tRNS` so transparent entries stay transparent). The indices
/// are the TIM's own, so an editor that paints in palette indices edits
/// exactly what the game stores.
pub fn indexed_png(tim: &Tim, palette: &[u16]) -> Result<Vec<u8>> {
    let (w, h) = (tim.pixel_width(), tim.pixel_height());
    let idx = indices(tim);
    let mut plte = Vec::with_capacity(palette.len() * 3);
    let mut trns = Vec::with_capacity(palette.len());
    for &c in palette {
        let p = bgr555_to_rgba8(c);
        plte.extend_from_slice(&p[..3]);
        trns.push(p[3]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(plte);
        enc.set_trns(trns);
        let mut wr = enc.write_header().context("write PNG header")?;
        wr.write_image_data(&idx).context("write PNG data")?;
    }
    Ok(out)
}

/// A decoded indexed PNG: `(w, h, row-major indices, PLTE as RGBA)`.
pub type IndexedImage = (usize, usize, Vec<u8>, Vec<[u8; 4]>);

/// An indexed PNG's indices and `PLTE`, or `None` when the PNG is not
/// palette-based.
pub fn read_indexed_png(png_bytes: &[u8]) -> Option<IndexedImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().ok()?;
    let (plte, trns) = {
        let info = reader.info();
        if info.color_type != png::ColorType::Indexed {
            return None;
        }
        (
            info.palette.as_ref()?.to_vec(),
            info.trns.as_ref().map(|t| t.to_vec()).unwrap_or_default(),
        )
    };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let out = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (out.width as usize, out.height as usize);
    let depth = out.bit_depth as usize;
    let line = out.line_size;
    let mut idx = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = &buf[y * line..(y + 1) * line];
        for x in 0..w {
            let v = match depth {
                8 => row[x],
                1 | 2 | 4 => {
                    let per_byte = 8 / depth;
                    let b = row[x / per_byte];
                    let shift = 8 - depth * (x % per_byte + 1);
                    (b >> shift) & ((1u8 << depth) - 1)
                }
                _ => return None,
            };
            idx.push(v);
        }
    }
    let pal = plte
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| [c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)])
        .collect();
    Some((w, h, idx, pal))
}

// --- the mapped encoder --------------------------------------------------------

/// Statistics of one mapped encode.
#[derive(Debug, Clone, Copy, Default)]
pub struct MappedStats {
    /// Palette slots given a colour the palette lacked.
    pub new_entries: usize,
    /// Pixels folded to a nearest colour (quantize mode only).
    pub quantized: usize,
}

/// Assign every pixel an index into the palette the map draws it with.
///
/// * `orig_sets` - the palettes as the *original* texture displayed them;
///   a pixel that still shows `orig_sets[p][original index]` keeps its
///   original index, so a palette edit made in `new_sets` carries through
///   untouched pixels.
/// * `new_sets` - the palettes after any strip edit; colours are matched
///   against these, and palettes `< editable` may take new colours in slots
///   no pixel uses. Palettes `>= editable` are read-only (external).
///
/// Returns the per-pixel indices; `new_sets` is updated in place.
#[allow(clippy::too_many_arguments)]
pub fn index_mapped(
    orig_index: &[u8],
    orig_sets: &[Vec<u16>],
    new_sets: &mut [Vec<u16>],
    editable: usize,
    per: usize,
    map: &[u16],
    rgba: &[u8],
    w: usize,
    opts: &EncodeOptions,
) -> Result<(Vec<u8>, MappedStats), EncodeError> {
    let n = orig_index.len();
    let mut target = Vec::with_capacity(n);
    let mut canon = Vec::with_capacity(n);
    for i in 0..n {
        let t = rgba8_to_bgr555(rgba[i * 4..i * 4 + 4].try_into().unwrap());
        target.push(t);
        canon.push(bgr555_to_rgba8(t));
    }

    const UNSET: usize = usize::MAX;
    let mut idx = vec![UNSET; n];
    // Slot usage is per palette: slot `s` of palette `p` is taken when a
    // pixel the map draws through `p` stores index `s`. A slot no pixel of
    // `p` uses may take a new colour in `p` without changing any other
    // pixel's display - pixels of other palettes read their own palette's
    // slot `s`. (Under a uniform map this is the plain "unused index" rule.)
    let mut used = vec![vec![false; per]; new_sets.len()];

    // Pass A: positional reuse against what the original displayed.
    for i in 0..n {
        let (p, oi) = (map[i] as usize, orig_index[i] as usize);
        if oi < per && bgr555_to_rgba8(orig_sets[p][oi]) == canon[i] {
            idx[i] = oi;
            used[p][oi] = true;
        }
    }
    // Pass B: first entry of the pixel's (edited) palette with the colour.
    let mut first_slot: Vec<HashMap<[u8; 4], usize>> = Vec::with_capacity(new_sets.len());
    for pal in new_sets.iter() {
        let mut m = HashMap::new();
        for (s, &c) in pal.iter().enumerate() {
            m.entry(bgr555_to_rgba8(c)).or_insert(s);
        }
        first_slot.push(m);
    }
    for i in 0..n {
        if idx[i] == UNSET
            && let Some(&s) = first_slot[map[i] as usize].get(&canon[i])
        {
            idx[i] = s;
            used[map[i] as usize][s] = true;
        }
    }

    // Pass C: colours the pixel's palette lacks, grouped per palette.
    struct Pending {
        palette: usize,
        texel: u16,
        first: (usize, usize),
        first_rgba: [u8; 4],
        pixels: Vec<usize>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut by_key: HashMap<(usize, [u8; 4]), usize> = HashMap::new();
    for i in 0..n {
        if idx[i] != UNSET {
            continue;
        }
        let p = map[i] as usize;
        let k = *by_key.entry((p, canon[i])).or_insert_with(|| {
            pending.push(Pending {
                palette: p,
                texel: target[i],
                first: (i % w, i / w),
                first_rgba: rgba[i * 4..i * 4 + 4].try_into().unwrap(),
                pixels: Vec::new(),
            });
            pending.len() - 1
        });
        pending[k].pixels.push(i);
    }

    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by_key(|&k| std::cmp::Reverse(pending[k].pixels.len()));
    let mut stats = MappedStats::default();
    let mut new_in: Vec<usize> = vec![0; new_sets.len()];
    let mut leftover: Vec<usize> = Vec::new();
    for &k in &order {
        let pd = &pending[k];
        let free = (pd.palette < editable)
            .then(|| (0..per).find(|&s| !used[pd.palette][s]))
            .flatten();
        if let Some(slot) = free {
            new_sets[pd.palette][slot] = pd.texel;
            used[pd.palette][slot] = true;
            new_in[pd.palette] += 1;
            stats.new_entries += 1;
            for &i in &pd.pixels {
                idx[i] = slot;
            }
        } else {
            leftover.push(k);
        }
    }

    if !leftover.is_empty() && !opts.quantize {
        let samples: Vec<ColorSample> = leftover
            .iter()
            .take(8)
            .map(|&k| ColorSample {
                x: pending[k].first.0 as u32,
                y: pending[k].first.1 as u32,
                rgba: pending[k].first_rgba,
            })
            .collect();
        let fixed = leftover.iter().any(|&k| pending[k].palette >= editable);
        if fixed {
            return Err(EncodeError::FixedPaletteMiss {
                colors: leftover.len(),
                samples,
            });
        }
        let p0 = pending[leftover[0]].palette;
        let matched = used[p0].iter().filter(|&&u| u).count() - new_in[p0];
        let wanted = pending.iter().filter(|pd| pd.palette == p0).count();
        return Err(EncodeError::TooManyColors {
            capacity: per,
            needed: matched + wanted,
            overflow: leftover.len(),
            samples,
        });
    }

    for &k in &leftover {
        let pd = &pending[k];
        let pal = &new_sets[pd.palette];
        let want = bgr555_to_rgba8(pd.texel);
        let live: Vec<usize> = (0..per).filter(|&s| used[pd.palette][s]).collect();
        let live = if live.is_empty() {
            (0..per).collect()
        } else {
            live
        };
        let classed: Vec<usize> = live
            .iter()
            .copied()
            .filter(|&s| (bgr555_to_rgba8(pal[s])[3] == 0) == (want[3] == 0))
            .collect();
        let candidates = if classed.is_empty() { &live } else { &classed };
        let nearest = candidates
            .iter()
            .copied()
            .min_by_key(|&s| {
                let d = bgr555_to_rgba8(pal[s]);
                let dr = d[0] as i32 - want[0] as i32;
                let dg = d[1] as i32 - want[1] as i32;
                let db = d[2] as i32 - want[2] as i32;
                dr * dr + dg * dg + db * db
            })
            .expect("a palette has at least one slot");
        stats.quantized += pd.pixels.len();
        for &i in &pd.pixels {
            idx[i] = nearest;
        }
    }
    debug_assert!(idx.iter().all(|&i| i < per));
    Ok((idx.into_iter().map(|i| i as u8).collect(), stats))
}

/// Encode an RGBA image drawn through `map` into a TIM laid out exactly like
/// `original`. `new_own` replaces the TIM's own palettes (pass
/// [`own_palettes`] to keep them), `external` are read-only palettes the map
/// may also name (indices `own.len()..`). Only palettes that end up
/// different from the original are rewritten; every other CLUT entry stays
/// byte-identical.
#[allow(clippy::too_many_arguments)]
pub fn encode_mapped(
    original: &Tim,
    new_own: &[Vec<u16>],
    external: &[Vec<u16>],
    map: &[u16],
    rgba: &[u8],
    w: usize,
    h: usize,
    opts: &EncodeOptions,
) -> Result<Encoded, EncodeError> {
    let (ew, eh) = (original.pixel_width(), original.pixel_height());
    if (w, h) != (ew, eh) {
        return Err(EncodeError::DimensionMismatch {
            expected_w: ew,
            expected_h: eh,
            got_w: w,
            got_h: h,
        });
    }
    if rgba.len() != w * h * 4 {
        return Err(EncodeError::PixelBufferSize {
            expected: w * h * 4,
            got: rgba.len(),
        });
    }
    let Some(per) = entries_per_palette(original) else {
        return Err(EncodeError::UnsupportedMode);
    };
    let own = own_palettes(original);
    if own.is_empty() {
        return Err(EncodeError::MissingClut {
            needed: per,
            have: original.clut.as_ref().map_or(0, |c| c.entries.len()),
        });
    }
    let editable = own.len();
    let mut orig_sets = own.clone();
    orig_sets.extend(external.iter().cloned());
    let mut new_sets: Vec<Vec<u16>> = new_own.to_vec();
    new_sets.extend(external.iter().cloned());
    if new_sets.len() != orig_sets.len() || new_sets.iter().any(|p| p.len() != per) {
        return Err(EncodeError::MissingClut {
            needed: per,
            have: new_own.first().map_or(0, Vec::len),
        });
    }
    if let Some(&bad) = map.iter().find(|&&p| p as usize >= new_sets.len()) {
        return Err(EncodeError::NoSuchPalette {
            palette: bad as usize,
            count: new_sets.len(),
        });
    }
    let orig_index = indices(original);
    let (idx, stats) = index_mapped(
        &orig_index,
        &orig_sets,
        &mut new_sets,
        editable,
        per,
        map,
        rgba,
        w,
        opts,
    )?;

    let mut tim = original.clone();
    let mut rewritten = false;
    {
        let clut = tim.clut.as_mut().expect("indexed TIM has a CLUT");
        for (p, pal) in new_sets.iter().take(editable).enumerate() {
            if *pal != own[p] {
                clut.entries[p * per..(p + 1) * per].copy_from_slice(pal);
                rewritten = true;
            }
        }
    }
    pack_indices(&mut tim, &idx);
    let bytes = serialize(&tim).expect("structure copied from a parsed TIM serializes");
    debug_assert_eq!(bytes.len(), original.byte_extent());
    Ok(Encoded {
        bytes,
        new_palette_entries: stats.new_entries,
        quantized_pixels: stats.quantized,
        clut_rows_rewritten: rewritten,
    })
}

// --- recognising what a PNG is ---------------------------------------------------

/// What a caller knows about how the game draws a texture's pixels: the
/// read-only palettes that live elsewhere, and the per-pixel map (indices
/// into own palettes then `external`). `map: None` = nothing is known, every
/// palette is an equally plausible view.
#[derive(Debug, Clone, Default)]
pub struct PaletteContext {
    pub external: Vec<Vec<u16>>,
    pub map: Option<Vec<u16>>,
}

/// Which palette(s) an image was drawn through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Each pixel through the palette the game draws it with.
    InGame,
    /// Every pixel through one of the TIM's own palettes.
    Palette(usize),
}

impl std::fmt::Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            View::InGame => write!(f, "the in-game palettes"),
            View::Palette(p) => write!(f, "palette {p}"),
        }
    }
}

/// The shape [`import_png`] recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// A plain colour image (the texture's own size), drawn through `View`.
    Image(View),
    /// A composite: image through `View` plus the palette strip below it.
    Composite(View),
    /// A standalone palette strip - colours only, indices untouched.
    PaletteStrip,
    /// An indexed PNG whose `PLTE` is palette `usize` - indices only.
    Indexed(usize),
}

impl std::fmt::Display for ImportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportKind::Image(v) => write!(f, "image drawn through {v}"),
            ImportKind::Composite(v) => {
                write!(f, "composite (image through {v} + palette strip)")
            }
            ImportKind::PaletteStrip => write!(f, "palette strip (colours only)"),
            ImportKind::Indexed(p) => {
                write!(f, "indexed PNG (indices only, shown through palette {p})")
            }
        }
    }
}

/// A recognised and encoded PNG.
#[derive(Debug, Clone)]
pub struct Imported {
    pub kind: ImportKind,
    pub encoded: Encoded,
    /// CLUT entries (across all own palettes) that changed.
    pub palette_entries_changed: usize,
}

/// The map a view stands for.
pub fn view_map(tim: &Tim, ctx: &PaletteContext, view: View) -> Vec<u16> {
    match (view, &ctx.map) {
        (View::InGame, Some(m)) => m.clone(),
        (View::InGame, None) => uniform_map(tim, 0),
        (View::Palette(p), _) => uniform_map(tim, p as u16),
    }
}

/// Every palette the context can name: own, then external.
pub fn all_sets(tim: &Tim, ctx: &PaletteContext) -> Vec<Vec<u16>> {
    let mut s = own_palettes(tim);
    s.extend(ctx.external.iter().cloned());
    s
}

/// Pick the view that explains the most pixels of `rgba` as unedited -
/// the one the image was exported through. Ties prefer the in-game view,
/// then the lowest palette.
pub fn guess_view(tim: &Tim, ctx: &PaletteContext, rgba: &[u8]) -> View {
    let sets = all_sets(tim, ctx);
    let mut views = Vec::new();
    if ctx.map.is_some() {
        views.push(View::InGame);
    }
    views.extend((0..tim.palette_count()).map(View::Palette));
    let canon: Vec<[u8; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| bgr555_to_rgba8(rgba8_to_bgr555(*p)))
        .collect();
    let mut best = (View::Palette(0), 0usize);
    let mut first = true;
    for v in views {
        let Ok(dec) = decode_mapped(tim, &sets, &view_map(tim, ctx, v)) else {
            continue;
        };
        let score = dec
            .as_chunks::<4>()
            .0
            .iter()
            .zip(&canon)
            .filter(|(a, b)| *a == *b)
            .count();
        if first || score > best.1 {
            best = (v, score);
            first = false;
        }
    }
    best.0
}

fn count_changed(a: &[Vec<u16>], b: &[Vec<u16>]) -> usize {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.iter().zip(y).filter(|(p, q)| p != q).count())
        .sum()
}

/// Recognise what `png_bytes` is relative to `original` and encode it.
///
/// | Shape | Recognised by | Writes |
/// |---|---|---|
/// | indexed PNG | texture size, `PLTE` equal to one of the TIM's palettes | indices only |
/// | image | texture size | pixels, drawn through the view that explains most pixels |
/// | composite | texture width, height + the composite strip | the strip's palettes, then the pixels |
/// | palette strip | `per x count` solid cells of any integer size | palettes only |
///
/// A 16/24 bpp texture only has the image shape.
pub fn import_png(
    original: &Tim,
    png_bytes: &[u8],
    ctx: &PaletteContext,
    opts: &EncodeOptions,
) -> Result<Imported> {
    let (w, h) = (original.pixel_width(), original.pixel_height());
    let Some(per) = entries_per_palette(original) else {
        let (pw, ph, rgba) = crate::encode::decode_png_rgba(png_bytes)?;
        let encoded = crate::encode::encode_replacement(original, &rgba, pw, ph, opts)?;
        return Ok(Imported {
            kind: ImportKind::Image(View::Palette(0)),
            encoded,
            palette_entries_changed: 0,
        });
    };
    let own = own_palettes(original);
    if own.is_empty() {
        bail!("indexed texture carries no palette");
    }

    // Indexed PNG whose PLTE is one of ours: indices verbatim.
    if let Some((iw, ih, idx, plte)) = read_indexed_png(png_bytes)
        && (iw, ih) == (w, h)
        && plte.len() >= per
        && let Some(p) = own.iter().position(|pal| {
            // RGB only: an editor may drop the tRNS chunk.
            pal.iter().zip(&plte).all(|(&c, q)| {
                bgr555_to_rgba8(rgba8_to_bgr555([q[0], q[1], q[2], 255]))[..3]
                    == bgr555_to_rgba8(c)[..3]
            })
        })
    {
        if let Some((i, &v)) = idx.iter().enumerate().find(|&(_, &v)| v as usize >= per) {
            bail!(
                "indexed PNG pixel ({}, {}) uses palette index {v}, but this texture's palettes \
                 hold only {per} colours",
                i % w,
                i / w
            );
        }
        let mut tim = original.clone();
        pack_indices(&mut tim, &idx);
        let bytes = serialize(&tim)?;
        return Ok(Imported {
            kind: ImportKind::Indexed(p),
            encoded: Encoded {
                bytes,
                new_palette_entries: 0,
                quantized_pixels: 0,
                clut_rows_rewritten: false,
            },
            palette_entries_changed: 0,
        });
    }

    let (pw, ph, rgba) = crate::encode::decode_png_rgba(png_bytes)?;

    // Plain image.
    if (pw, ph) == (w, h) {
        let view = guess_view(original, ctx, &rgba);
        let map = view_map(original, ctx, view);
        let encoded = encode_mapped(original, &own, &ctx.external, &map, &rgba, w, h, opts)?;
        let after = crate::parse_strict(&encoded.bytes)?;
        let changed = count_changed(&own, &own_palettes(&after));
        return Ok(Imported {
            kind: ImportKind::Image(view),
            encoded,
            palette_entries_changed: changed,
        });
    }

    // Composite.
    if let Some(g) = composite_geom(original)
        && (pw, ph) == (w, h + g.height())
    {
        let top = &rgba[..w * h * 4];
        let strip = &rgba[w * h * 4..];
        let new_own = read_strip(strip, w, g.height(), g, &own)?;
        let view = guess_view(original, ctx, top);
        let map = view_map(original, ctx, view);
        let encoded = encode_mapped(original, &new_own, &ctx.external, &map, top, w, h, opts)?;
        let after = crate::parse_strict(&encoded.bytes)?;
        let changed = count_changed(&own, &own_palettes(&after));
        return Ok(Imported {
            kind: ImportKind::Composite(view),
            encoded,
            palette_entries_changed: changed,
        });
    }

    // Standalone palette strip, any integer cell size.
    let count = own.len();
    if pw % per == 0 && ph % count == 0 && pw >= per && ph >= count {
        let g = StripGeom {
            per,
            count,
            cell_w: pw / per,
            cell_h: ph / count,
        };
        let new_own = read_strip(&rgba, pw, ph, g, &own)?;
        let mut tim = original.clone();
        {
            let clut = tim.clut.as_mut().expect("indexed TIM has a CLUT");
            for (p, pal) in new_own.iter().enumerate() {
                clut.entries[p * per..(p + 1) * per].copy_from_slice(pal);
            }
        }
        let bytes = serialize(&tim)?;
        let changed = count_changed(&own, &new_own);
        return Ok(Imported {
            kind: ImportKind::PaletteStrip,
            encoded: Encoded {
                bytes,
                new_palette_entries: 0,
                quantized_pixels: 0,
                clut_rows_rewritten: changed > 0,
            },
            palette_entries_changed: changed,
        });
    }

    let mut shapes = format!("the texture itself ({w}x{h})");
    if let Some(g) = composite_geom(original) {
        shapes.push_str(&format!(", a composite ({w}x{})", h + g.height()));
    }
    shapes.push_str(&format!(
        ", or a palette strip ({per} x {count} solid cells, e.g. {}x{})",
        per * 16,
        count * 16
    ));
    bail!("PNG is {pw}x{ph}; this texture accepts {shapes}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_rgba8, parse_strict};

    /// 4bpp 16x4 TIM, four palettes (CLUT 16x4), indices 1..=6 per row.
    fn tim4() -> Vec<u8> {
        let mut b = vec![];
        b.extend_from_slice(&0x10u32.to_le_bytes());
        b.extend_from_slice(&0x08u32.to_le_bytes());
        b.extend_from_slice(&(12u32 + 16 * 4 * 2).to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&511u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        for p in 0..4u16 {
            for e in 0..16u16 {
                // Distinct colour per (palette, entry); entry 0 transparent.
                let c = if e == 0 {
                    0
                } else {
                    (e & 0x1F) | (((p * 7) & 0x1F) << 5) | (((e + p) & 0x1F) << 10)
                };
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        // 16x4 @4bpp: fb_w = 4, h = 4.
        b.extend_from_slice(&(12u32 + 4 * 4 * 2).to_le_bytes());
        b.extend_from_slice(&896u16.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        for row in 0..4u8 {
            for pair in 0..8u8 {
                let a = (pair * 2 + row) % 6 + 1;
                let c = (pair * 2 + 1 + row) % 6 + 1;
                b.push(a | (c << 4));
            }
        }
        b
    }

    fn quadrant_map(tim: &Tim) -> Vec<u16> {
        // Left half palette 1, right half palette 3.
        let w = tim.pixel_width();
        (0..w * tim.pixel_height())
            .map(|i| if i % w < w / 2 { 1 } else { 3 })
            .collect()
    }

    #[test]
    fn composite_round_trip_is_byte_identical() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let ctx = PaletteContext {
            external: vec![],
            map: Some(quadrant_map(&tim)),
        };
        let sets = all_sets(&tim, &ctx);
        let (w, h, rgba) = render_composite(&tim, &sets, ctx.map.as_ref().unwrap()).unwrap();
        let png = rgba_png(w, h, &rgba).unwrap();
        let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
        assert_eq!(imp.kind, ImportKind::Composite(View::InGame));
        assert_eq!(imp.encoded.bytes, bytes);
        assert_eq!(imp.palette_entries_changed, 0);
    }

    #[test]
    fn strip_edit_changes_only_the_edited_entry() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let g = standalone_strip_geom(&tim).unwrap();
        let own = own_palettes(&tim);
        let mut rgba = render_strip(&own, g, g.width());
        // Repaint palette 2, entry 5 to pure red.
        for y in 2 * g.cell_h..3 * g.cell_h {
            for x in 5 * g.cell_w..6 * g.cell_w {
                let o = (y * g.width() + x) * 4;
                rgba[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
        let imp = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap();
        assert_eq!(imp.kind, ImportKind::PaletteStrip);
        assert_eq!(imp.palette_entries_changed, 1);
        let out = parse_strict(&imp.encoded.bytes).unwrap();
        let got = own_palettes(&out);
        for p in 0..4 {
            for e in 0..16 {
                if (p, e) == (2, 5) {
                    assert_eq!(got[p][e], 0x001F);
                } else {
                    assert_eq!(got[p][e], own[p][e], "palette {p} entry {e} must not move");
                }
            }
        }
        // Indices untouched.
        assert_eq!(indices(&out), indices(&tim));
    }

    #[test]
    fn strip_of_any_integer_scale_is_accepted_and_smoothing_is_refused() {
        let tim = parse_strict(&tim4()).unwrap();
        let own = own_palettes(&tim);
        let g = StripGeom {
            per: 16,
            count: 4,
            cell_w: 3,
            cell_h: 5,
        };
        let mut rgba = render_strip(&own, g, g.width());
        let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
        let imp = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap();
        assert_eq!(imp.kind, ImportKind::PaletteStrip);
        assert_eq!(imp.palette_entries_changed, 0);
        // One off-colour pixel inside the entry-1 cell (a smoothed edge).
        rgba[(g.width() + 4) * 4] ^= 0x40;
        let png = rgba_png(g.width(), g.height(), &rgba).unwrap();
        let err = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not one solid colour"), "{err}");
    }

    #[test]
    fn indexed_png_round_trip_writes_indices_only() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let own = own_palettes(&tim);
        let png = indexed_png(&tim, &own[2]).unwrap();
        let imp = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap();
        assert_eq!(imp.kind, ImportKind::Indexed(2));
        assert_eq!(imp.encoded.bytes, bytes);

        // Repaint one pixel to index 9 in the indexed domain.
        let (w, h, mut idx, _) = read_indexed_png(&png).unwrap();
        idx[3] = 9;
        let mut edited = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut edited, w as u32, h as u32);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(png::BitDepth::Eight);
            let plte: Vec<u8> = own[2]
                .iter()
                .flat_map(|&c| bgr555_to_rgba8(c)[..3].to_vec())
                .collect();
            enc.set_palette(plte);
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(&idx).unwrap();
        }
        let imp = import_png(
            &tim,
            &edited,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap();
        let out = parse_strict(&imp.encoded.bytes).unwrap();
        assert_eq!(indices(&out)[3], 9);
        assert_eq!(own_palettes(&out), own, "no palette touched");
    }

    #[test]
    fn image_through_one_palette_is_recognised_and_writes_only_that_palette() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let own = own_palettes(&tim);
        let mut rgba = decode_rgba8(&tim, 3).unwrap();
        // New colour on pixel 0: must land in palette 3 only.
        rgba[0..4].copy_from_slice(&[255, 0, 255, 255]);
        let png = rgba_png(16, 4, &rgba).unwrap();
        let imp = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap();
        assert_eq!(imp.kind, ImportKind::Image(View::Palette(3)));
        let out = parse_strict(&imp.encoded.bytes).unwrap();
        let got = own_palettes(&out);
        for p in [0, 1, 2] {
            assert_eq!(got[p], own[p], "palette {p} must stay byte-identical");
        }
        assert_eq!(&decode_rgba8(&out, 3).unwrap()[0..4], &[255, 0, 255, 255]);
    }

    #[test]
    fn mapped_image_keeps_each_region_in_its_palette() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let map = quadrant_map(&tim);
        let ctx = PaletteContext {
            external: vec![],
            map: Some(map.clone()),
        };
        let sets = all_sets(&tim, &ctx);
        let mut rgba = decode_mapped(&tim, &sets, &map).unwrap();
        // Repaint right-half pixel (15, 0) with palette 3's entry-2 colour:
        // an existing colour of that region's palette, so no palette moves.
        let want = bgr555_to_rgba8(sets[3][2]);
        rgba[15 * 4..16 * 4].copy_from_slice(&want);
        let png = rgba_png(16, 4, &rgba).unwrap();
        let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
        assert_eq!(imp.kind, ImportKind::Image(View::InGame));
        assert_eq!(imp.palette_entries_changed, 0);
        let out = parse_strict(&imp.encoded.bytes).unwrap();
        assert_eq!(indices(&out)[15], 2);
        let mut expect = indices(&tim);
        expect[15] = 2;
        assert_eq!(indices(&out), expect);
    }

    #[test]
    fn external_palette_is_read_only() {
        let tim = parse_strict(&tim4()).unwrap();
        let ext: Vec<u16> = (0..16u16)
            .map(|e| if e == 0 { 0 } else { 0x4000 | e })
            .collect();
        // Pixel 0 drawn through the external palette (index 4), rest palette 0.
        let mut map = uniform_map(&tim, 0);
        map[0] = 4;
        let ctx = PaletteContext {
            external: vec![ext],
            map: Some(map.clone()),
        };
        let sets = all_sets(&tim, &ctx);
        let mut rgba = decode_mapped(&tim, &sets, &map).unwrap();
        rgba[0..4].copy_from_slice(&[255, 255, 0, 255]); // not in the external palette
        let png = rgba_png(16, 4, &rgba).unwrap();
        let err = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap_err();
        assert!(
            err.to_string().contains("belongs to another texture"),
            "{err}"
        );
        // Quantize folds it instead.
        let opts = EncodeOptions {
            quantize: true,
            ..Default::default()
        };
        let imp = import_png(&tim, &png, &ctx, &opts).unwrap();
        assert_eq!(imp.encoded.quantized_pixels, 1);
        assert_eq!(imp.palette_entries_changed, 0);
    }

    #[test]
    fn composite_strip_edit_carries_through_untouched_pixels() {
        let bytes = tim4();
        let tim = parse_strict(&bytes).unwrap();
        let ctx = PaletteContext {
            external: vec![],
            map: Some(quadrant_map(&tim)),
        };
        let sets = all_sets(&tim, &ctx);
        let (w, h, mut rgba) = render_composite(&tim, &sets, ctx.map.as_ref().unwrap()).unwrap();
        let g = composite_geom(&tim).unwrap();
        // Recolour palette 1 entry 1 in the strip only.
        let top_h = tim.pixel_height();
        for y in top_h + g.cell_h..top_h + 2 * g.cell_h {
            for x in g.cell_w..2 * g.cell_w {
                let o = (y * w + x) * 4;
                rgba[o..o + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        let png = rgba_png(w, h, &rgba).unwrap();
        let imp = import_png(&tim, &png, &ctx, &EncodeOptions::default()).unwrap();
        assert_eq!(imp.palette_entries_changed, 1);
        let out = parse_strict(&imp.encoded.bytes).unwrap();
        assert_eq!(indices(&out), indices(&tim), "indices untouched");
        assert_eq!(own_palettes(&out)[1][1], 0x7C00);
    }

    #[test]
    fn unknown_shape_names_the_accepted_ones() {
        let tim = parse_strict(&tim4()).unwrap();
        let png = rgba_png(5, 5, &[0u8; 100]).unwrap();
        let err = import_png(
            &tim,
            &png,
            &PaletteContext::default(),
            &EncodeOptions::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("palette strip"), "{err}");
    }
}
