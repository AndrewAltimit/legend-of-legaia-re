//! Software emulation of the PSX 1MB VRAM (1024×512 16-bit pixels).
//!
//! Used by the renderer to do faithful PSX texture lookups: each TMD
//! primitive carries a CBA (CLUT base address) and TSB (texture sub-base /
//! "tpage") that index into VRAM, not into any individual TIM. To resolve
//! them we need every TIM in the scene placed at its canonical fb_x/fb_y
//! position - which is exactly what the PSX BIOS does at boot when the
//! game DMAs each TIM to its `fb_x/fb_y` slot.
//!
//! [`Vram::upload_tim`] writes both the image block and the CLUT block at
//! the positions stored in the TIM header. Overlapping uploads use last-
//! wins (matches the real hardware: later DMA writes overwrite earlier ones).
//!
//! Pixels are stored in raw 16-bit form (BGR555 + STP). Decoding to RGBA
//! happens in the fragment shader so per-prim bit-depth + CLUT lookup
//! decisions stay on the GPU.

use crate::Tim;

/// PSX VRAM is 1024 pixels wide × 512 pixels tall, 16 bits per pixel.
pub const VRAM_WIDTH: usize = 1024;
pub const VRAM_HEIGHT: usize = 512;
pub const VRAM_PIXELS: usize = VRAM_WIDTH * VRAM_HEIGHT;

/// Detailed verdict on whether a primitive's `(cba, tsb, uvs)` lookup will
/// produce coherent pixels in the current VRAM. Returned by
/// [`Vram::prim_texture_status`]; [`Vram::prim_has_texture_data`] is a thin
/// wrapper that just checks for `Ok` here.
///
/// "Coherent" means: the CLUT row has the right number of populated
/// entries for the prim's depth, AND the UV bbox inside the texture page
/// has non-zero data. A `ClutDepthMismatch` row is one where a 4bpp prim
/// references a CLUT scanline that's actually 256 wide (typical when the
/// wrong TIM was uploaded for the row) - rendering it gives rainbow noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimTextureStatus {
    /// CLUT (if any) and texture page both populated and depth-matched.
    Ok,
    /// CLUT row sits in unfilled VRAM (no TIM has uploaded this row).
    MissingClut { row: u16 },
    /// CLUT row has data but only at the wrong palette depth - e.g. a
    /// 4bpp prim sampling a 256-entry 8bpp palette. `populated_width` is
    /// the run of non-zero entries in the row; `expected_width` is what
    /// this prim's depth needs (16 or 256).
    ClutDepthMismatch {
        row: u16,
        populated_width: u16,
        expected_width: u16,
    },
    /// Texture page region (UV bbox) is empty - the TIM that should
    /// supply the texels for this prim wasn't uploaded.
    MissingTexturePage { tpage: u16 },
}

impl PrimTextureStatus {
    /// True when the prim should be drawn (no missing/mismatched data).
    pub fn ok(&self) -> bool {
        matches!(self, PrimTextureStatus::Ok)
    }
}

/// Software 1MB VRAM. Each cell is one 16-bit framebuffer word. Indexed
/// row-major: `pixels[y * VRAM_WIDTH + x]`.
#[derive(Clone)]
pub struct Vram {
    pixels: Vec<u16>,
}

impl Default for Vram {
    fn default() -> Self {
        Self::new()
    }
}

impl Vram {
    /// Fresh VRAM, all zeros.
    pub fn new() -> Self {
        Self {
            pixels: vec![0u16; VRAM_PIXELS],
        }
    }

    /// Raw pixel buffer in row-major order. Bytes are little-endian u16.
    pub fn as_u16(&self) -> &[u16] {
        &self.pixels
    }

    /// Same data, viewed as bytes - useful for GPU upload (R16Uint).
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.pixels)
    }

    /// Upload a TIM's image block (and CLUT, if present) at the positions
    /// stored in the TIM header. Out-of-bounds writes are clipped.
    pub fn upload_tim(&mut self, tim: &Tim) {
        self.upload_tim_partial(tim, true, true);
    }

    /// Like [`Self::upload_tim`] but lets the caller choose, per block,
    /// whether to write it. Asset-viewer / web-viewer use this for
    /// **targeted** uploads: when a TIM's image block would land on top
    /// of a VRAM region another primitive uses as its CLUT, the caller
    /// passes `upload_image = false` so the image bytes don't clobber a
    /// palette row (the symptom otherwise is rainbow noise - the
    /// paletted decode reads image data as BGR555 colours). The
    /// symmetric case (`upload_clut = false`) covers CLUT blocks
    /// landing on top of someone else's texture page.
    pub fn upload_tim_partial(&mut self, tim: &Tim, upload_image: bool, upload_clut: bool) {
        self.upload_tim_partial_opts(tim, upload_image, upload_clut, false);
    }

    /// Like [`Self::upload_tim_partial`] but with optional **merge** semantics
    /// for the CLUT block: when `merge_clut_zeros` is set, CLUT halfwords
    /// equal to `0x0000` skip the underlying VRAM cell instead of clobbering
    /// it. Used by the targeted upload to handle scenes where multiple TIMs
    /// target the same CLUT row but each only populates a subset of the
    /// 16-color slots (the remaining entries on disc are filler zeros).
    /// Without merge mode, the last TIM in iteration order wins and any
    /// slot it leaves zero erases earlier uploads. Image blocks always
    /// overwrite, since image data legitimately contains `0x0000` pixels.
    pub fn upload_tim_partial_opts(
        &mut self,
        tim: &Tim,
        upload_image: bool,
        upload_clut: bool,
        merge_clut_zeros: bool,
    ) {
        if upload_clut && let Some(clut) = tim.clut.as_ref() {
            // A Legaia CLUT block lands as a **linear run** of `w * h`
            // halfwords along row `fb_y`, NOT as a `w x h` rectangle: palette
            // `i` of a multi-palette block goes to `(fb_x + 16*i, fb_y)`, the
            // next 16-colour slot on the same row, not to `(fb_x, fb_y + i)`.
            //
            // Pinned byte-exactly against retail VRAM (the town0c field save
            // state): of the 69 non-zero secondary palettes the scene's TIMs
            // declare, 69 sit where the linear layout puts them and 0 sit
            // where the rectangle would. It is also what the prims ask for -
            // their CBAs index those odd 16-colour slots - and what makes the
            // NPC-palette blocks legal at all: `parse_strict` notes CLUTs at
            // `fb_y` 510..511 with `h` up to 16, which as a rectangle would
            // run off the bottom of a 512-row framebuffer and as a linear run
            // fits comfortably.
            //
            // Uploading these as rectangles left every odd CLUT slot empty, so
            // the 4bpp prims indexing them decoded to `0x0000`, were discarded
            // by the renderer, and left holes in the town ground.
            let run = clut.w.saturating_mul(clut.h);
            if merge_clut_zeros {
                self.write_words_merge_zeros(clut.fb_x, clut.fb_y, run, 1, &clut.entries);
            } else {
                self.write_words(clut.fb_x, clut.fb_y, run, 1, &clut.entries);
            }
        }
        if !upload_image {
            return;
        }
        // Image block: data is `fb_w * h` 16-bit words.
        let img = &tim.image;
        let n_words = img.fb_w as usize * img.h as usize;
        let mut words = Vec::with_capacity(n_words);
        for i in 0..n_words {
            let off = i * 2;
            if off + 2 > img.data.len() {
                break;
            }
            words.push(u16::from_le_bytes([img.data[off], img.data[off + 1]]));
        }
        self.write_words(img.fb_x, img.fb_y, img.fb_w, img.h, &words);
    }

    /// Write a single row of 16-bit words at `(fb_x, fb_y)` from raw bytes.
    /// Bytes must come in little-endian halfword pairs (BGR555 + STP).
    /// Pixels past `VRAM_WIDTH` / `VRAM_HEIGHT` are clipped silently.
    ///
    /// Used by engine consumers that source CLUT halfwords from a buffer
    /// that doesn't carry the standard TIM CLUT header (e.g. the
    /// `legaia_asset::battle_data_pack` post-TMD pool, where palettes
    /// live as a bare 32-byte run inside an LZS-decompressed record).
    pub fn write_clut_row(&mut self, fb_x: u16, fb_y: u16, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let halfwords: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        self.write_words(fb_x, fb_y, halfwords.len() as u16, 1, &halfwords);
    }

    /// Write a `w_words` x `h` packed image block from raw little-endian
    /// halfword bytes into VRAM at `(fb_x, fb_y)`. One VRAM cell holds one
    /// halfword, so a 4bpp page that is `n` texels wide occupies `n / 4`
    /// cells per row; the caller is responsible for that texel->word
    /// conversion. Pixels falling outside VRAM are skipped.
    ///
    /// Used by engine consumers that inject a bare (header-less) image
    /// block - e.g. the `legaia_asset::monster_archive` battle texture
    /// page, whose 4bpp pixel run lives after the CLUT region inside an
    /// LZS-decompressed archive block.
    pub fn write_block(&mut self, fb_x: u16, fb_y: u16, w_words: u16, h: u16, bytes: &[u8]) {
        if bytes.is_empty() || w_words == 0 || h == 0 {
            return;
        }
        let halfwords: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        self.write_words(fb_x, fb_y, w_words, h, &halfwords);
    }

    /// Fill every **zero** word of `self` from `base` - i.e. layer a
    /// boot-resident upload *underneath* an already-built scene VRAM.
    /// Scene words win wherever the scene build wrote (matching the
    /// retail boot-first-then-scene DMA order, where a later scene
    /// upload overwrites boot content); the boot content shows through
    /// everywhere else. Uses the codebase-wide "0 = unpopulated"
    /// convention (see [`Self::prim_has_texture_data`] and the VRAM
    /// parity oracle's incompleteness rule).
    pub fn underlay(&mut self, base: &Vram) {
        for (dst, &src) in self.pixels.iter_mut().zip(base.pixels.iter()) {
            if *dst == 0 {
                *dst = src;
            }
        }
    }

    /// VRAM-to-VRAM rectangle copy: `w x h` halfwords from `(src_x, src_y)`
    /// to `(dst_x, dst_y)` - the libgpu `MoveImage` primitive. A zero `w` or
    /// `h` is a no-op (the retail wrapper rejects those rects); cells falling
    /// outside the 1024x512 framebuffer are skipped. The source rect is
    /// snapshotted first, so overlapping copies behave like the GPU's
    /// buffered transfer.
    ///
    /// Used by the engine's battle facial animator, which re-stamps the
    /// current eye/mouth face frame from a member band's face-frame strip
    /// onto its live face rows every frame (`legaia_asset::face_anim`).
    // PORT: FUN_80058490 - the MoveImage wrapper the per-frame facial
    // animator FUN_8004C7B4 stamps through (skips rects with a zero
    // width or height).
    pub fn move_image(&mut self, src_x: u16, src_y: u16, w: u16, h: u16, dst_x: u16, dst_y: u16) {
        if w == 0 || h == 0 {
            return;
        }
        let mut snap = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h as usize {
            for col in 0..w as usize {
                snap.push(self.pixel(src_x as usize + col, src_y as usize + row));
            }
        }
        self.write_words(dst_x, dst_y, w, h, &snap);
    }

    /// Write `w * h` 16-bit words into VRAM starting at `(x, y)`.
    /// Pixels falling outside `[0..VRAM_WIDTH) × [0..VRAM_HEIGHT)` are skipped.
    fn write_words(&mut self, x: u16, y: u16, w: u16, h: u16, src: &[u16]) {
        let x0 = x as usize;
        let y0 = y as usize;
        for row in 0..h as usize {
            let dy = y0 + row;
            if dy >= VRAM_HEIGHT {
                break;
            }
            for col in 0..w as usize {
                let dx = x0 + col;
                if dx >= VRAM_WIDTH {
                    break;
                }
                let src_idx = row * w as usize + col;
                if src_idx >= src.len() {
                    return;
                }
                self.pixels[dy * VRAM_WIDTH + dx] = src[src_idx];
            }
        }
    }

    /// Like [`Self::write_words`] but `0x0000` source halfwords are skipped
    /// instead of overwriting. Used by CLUT merge uploads where multiple
    /// scene-pack TIMs share the same CLUT row but each only populates a
    /// subset of the 16-color slots.
    fn write_words_merge_zeros(&mut self, x: u16, y: u16, w: u16, h: u16, src: &[u16]) {
        let x0 = x as usize;
        let y0 = y as usize;
        for row in 0..h as usize {
            let dy = y0 + row;
            if dy >= VRAM_HEIGHT {
                break;
            }
            for col in 0..w as usize {
                let dx = x0 + col;
                if dx >= VRAM_WIDTH {
                    break;
                }
                let src_idx = row * w as usize + col;
                if src_idx >= src.len() {
                    return;
                }
                let val = src[src_idx];
                if val == 0 {
                    continue;
                }
                self.pixels[dy * VRAM_WIDTH + dx] = val;
            }
        }
    }

    /// Read one 16-bit pixel at `(x, y)`. Returns 0 outside VRAM.
    pub fn pixel(&self, x: usize, y: usize) -> u16 {
        if x >= VRAM_WIDTH || y >= VRAM_HEIGHT {
            return 0;
        }
        self.pixels[y * VRAM_WIDTH + x]
    }

    /// True if any pixel in the rectangle `[x..x+w) × [y..y+h)` is non-zero.
    /// Coordinates and dimensions outside VRAM are clipped silently. Used
    /// by mesh builders to decide whether a primitive's CLUT / texture
    /// page region was actually populated by the TIMs that have been
    /// uploaded so far - empty regions render as solid `CLUT[0]` (commonly
    /// a flat green or cyan), so it's better to drop those primitives at
    /// build time than rasterise garbage over them.
    pub fn region_has_data(&self, x: usize, y: usize, w: usize, h: usize) -> bool {
        let x_end = (x + w).min(VRAM_WIDTH);
        let y_end = (y + h).min(VRAM_HEIGHT);
        if x >= x_end || y >= y_end {
            return false;
        }
        for row in y..y_end {
            let base = row * VRAM_WIDTH;
            for col in x..x_end {
                if self.pixels[base + col] != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Predicate suitable for [`legaia_tmd::mesh::tmd_to_vram_mesh_filtered`]:
    /// returns `true` when both the prim's CLUT row and the UV bbox inside
    /// its texture page have non-zero VRAM data, AND the CLUT row's
    /// occupied width is plausibly the right depth (a 4bpp prim sampling
    /// a CLUT scanline that's clearly a 256-entry 8bpp palette is a
    /// strong signal that the wrong TIM is supplying this row).
    ///
    /// Returns `false` when either side is empty or the depth mismatch is
    /// large enough that rendering would produce rainbow noise (a 4bpp
    /// prim indexing the first 16 entries of an 8bpp palette gives an
    /// arbitrary slice of a 256-colour gradient, not a coherent 16-colour
    /// scheme - usually the wrong TIM was loaded for the asset).
    pub fn prim_has_texture_data(&self, cba: u16, tsb: u16, uvs: &[(u8, u8)]) -> bool {
        self.prim_texture_status(cba, tsb, uvs).ok()
    }

    /// Like [`Self::prim_has_texture_data`] but returns a structured verdict.
    /// Used by diagnostic surfaces that want to tell the user which prims
    /// were dropped and why.
    pub fn prim_texture_status(&self, cba: u16, tsb: u16, uvs: &[(u8, u8)]) -> PrimTextureStatus {
        // CLUT row: 1 row of 16 (4bpp) or 256 (8bpp) BGR555 entries.
        let cx = ((cba & 0x3F) * 16) as usize;
        let cy = ((cba >> 6) & 0x1FF) as usize;
        let depth_bits = match (tsb >> 7) & 0x3 {
            0 => 4u8,
            1 => 8,
            _ => 16,
        };
        let clut_w = match depth_bits {
            4 => 16usize,
            8 => 256,
            _ => 0,
        };
        if clut_w != 0 && !self.region_has_data(cx, cy, clut_w, 1) {
            return PrimTextureStatus::MissingClut {
                row: (cba >> 6) & 0x1FF,
            };
        }
        // Depth-mismatch check: if a 4bpp prim's CLUT row is filled far
        // past what 16 separate 4bpp palettes (= 256 entries) could
        // occupy, the first 16 entries are an arbitrary slice of a wide
        // gradient and the prim renders as rainbow stripes. Count the
        // populated run length so the diagnostic can tell the user how
        // wide the row actually is.
        //
        // Legaia character TIMs commonly pack 16 distinct 16-entry
        // palettes into a single 256-wide row (the prim's CBA low 6 bits
        // pick which palette to use). So the depth-mismatch threshold
        // is `clut_w * 16` for 4bpp (= 256-wide row legitimate) and
        // proportional for 8bpp. Anything past that is a real mismatch.
        if clut_w != 0 && clut_w < 256 {
            // Measure the populated run as a *contiguous* palette band: stop at
            // the first large zero gap so an unrelated texture region sharing
            // this scanline isn't counted as part of the palette. This matters
            // for the battle monster layout, where the per-slot CLUT rows
            // (`484 + slot`) fall inside the 256-tall texture-page y-band
            // (`y = 256`), so the slot's own 4bpp page sits far to the right of
            // the CLUT on the *same* scanline; an unbounded scan would walk
            // across the empty gap into the page and report a bogus ~480-wide
            // palette. `MAX_CLUT_GAP` tolerates a few fully-transparent palette
            // entries inside the band while staying well below the 80+ px gap
            // that separates the CLUT region from any texture page.
            const MAX_CLUT_GAP: usize = 64;
            let populated_width = self.row_run_width(cx, cy, VRAM_WIDTH, MAX_CLUT_GAP) as u16;
            let max_legitimate_width = match depth_bits {
                4 => clut_w * 16, // 16 distinct 16-entry palettes per row
                8 => clut_w * 2,  // 8bpp has 1 palette per row; 2x slack for stray pixels
                _ => clut_w,
            };
            if populated_width as usize > max_legitimate_width {
                return PrimTextureStatus::ClutDepthMismatch {
                    row: (cba >> 6) & 0x1FF,
                    populated_width,
                    expected_width: clut_w as u16,
                };
            }
        }

        // Texture page region: hash the UV bbox into VRAM coords and check
        // that some words are non-zero. UV byte layout matches the shader:
        // 4bpp packs 4 pixels per word (u >> 2), 8bpp packs 2 (u >> 1),
        // 15bpp uses one word per texel.
        let tpage_x = ((tsb & 0xF) * 64) as usize;
        let tpage_y = (((tsb >> 4) & 1) * 256) as usize;
        let mut umin = u8::MAX;
        let mut umax = 0u8;
        let mut vmin = u8::MAX;
        let mut vmax = 0u8;
        for &(u, v) in uvs {
            if u < umin {
                umin = u;
            }
            if u > umax {
                umax = u;
            }
            if v < vmin {
                vmin = v;
            }
            if v > vmax {
                vmax = v;
            }
        }
        let (vram_u_lo, vram_u_hi) = match depth_bits {
            4 => (umin as usize >> 2, umax as usize >> 2),
            8 => (umin as usize >> 1, umax as usize >> 1),
            _ => (umin as usize, umax as usize),
        };
        let page_w = vram_u_hi.saturating_sub(vram_u_lo) + 1;
        let page_h = (vmax as usize).saturating_sub(vmin as usize) + 1;
        if !self.region_has_data(tpage_x + vram_u_lo, tpage_y + vmin as usize, page_w, page_h) {
            return PrimTextureStatus::MissingTexturePage { tpage: tsb & 0x1F };
        }
        PrimTextureStatus::Ok
    }

    /// Length (in pixels) of the populated run starting at `(x, y)` in
    /// VRAM. Used to gauge how wide a CLUT row's contents are - 16 for a
    /// 4bpp palette, 256 for an 8bpp one. `max_w` caps the search so the
    /// scan is bounded.
    pub fn row_populated_width(&self, x: usize, y: usize, max_w: usize) -> usize {
        if y >= VRAM_HEIGHT || x >= VRAM_WIDTH {
            return 0;
        }
        let end = (x + max_w).min(VRAM_WIDTH);
        let mut last_nonzero: Option<usize> = None;
        let base = y * VRAM_WIDTH;
        for col in x..end {
            if self.pixels[base + col] != 0 {
                last_nonzero = Some(col);
            }
        }
        last_nonzero.map(|c| c + 1 - x).unwrap_or(0)
    }

    /// Width (in pixels) of the *contiguous* populated run starting at
    /// `(x, y)`, tolerating internal gaps of up to `max_gap` consecutive zero
    /// pixels but stopping at the first larger gap. Unlike
    /// [`Self::row_populated_width`] (which reports the distance to the last
    /// non-zero pixel within the window, crossing arbitrary gaps), this keeps a
    /// palette-row width measurement from leaking into an unrelated VRAM region
    /// that merely shares the same scanline. `max_w` bounds the scan.
    pub fn row_run_width(&self, x: usize, y: usize, max_w: usize, max_gap: usize) -> usize {
        if y >= VRAM_HEIGHT || x >= VRAM_WIDTH {
            return 0;
        }
        let end = (x + max_w).min(VRAM_WIDTH);
        let base = y * VRAM_WIDTH;
        let mut last_nonzero: Option<usize> = None;
        let mut gap = 0usize;
        for col in x..end {
            if self.pixels[base + col] != 0 {
                last_nonzero = Some(col);
                gap = 0;
            } else if last_nonzero.is_some() {
                gap += 1;
                if gap > max_gap {
                    break;
                }
            }
        }
        last_nonzero.map(|c| c + 1 - x).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Build a 4-pixel 16bpp TIM at fb_x=64, fb_y=128 - easiest case to verify
    /// (no CLUT, image data goes straight into VRAM as-is).
    fn tim_16bpp_at(fb_x: u16, fb_y: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
        // Image: 4 pixels × 1 row at 16bpp = fb_w=4 words
        // bs_len = 12 + 4 * 1 * 2 = 20
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(&fb_x.to_le_bytes());
        buf.extend_from_slice(&fb_y.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes()); // fb_w
        buf.extend_from_slice(&1u16.to_le_bytes()); // h
        // Four 16-bit pixels: 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD
        buf.extend_from_slice(&0xAAAAu16.to_le_bytes());
        buf.extend_from_slice(&0xBBBBu16.to_le_bytes());
        buf.extend_from_slice(&0xCCCCu16.to_le_bytes());
        buf.extend_from_slice(&0xDDDDu16.to_le_bytes());
        buf
    }

    #[test]
    fn upload_places_pixels_at_fbxy() {
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(64, 128);
        let tim = parse(&buf).unwrap();
        vram.upload_tim(&tim);
        assert_eq!(vram.pixel(64, 128), 0xAAAA);
        assert_eq!(vram.pixel(65, 128), 0xBBBB);
        assert_eq!(vram.pixel(66, 128), 0xCCCC);
        assert_eq!(vram.pixel(67, 128), 0xDDDD);
        // Untouched cells remain zero.
        assert_eq!(vram.pixel(0, 0), 0);
        assert_eq!(vram.pixel(63, 128), 0);
        assert_eq!(vram.pixel(68, 128), 0);
    }

    /// 4bpp TIM whose CLUT block declares `w=16, h=2` - two 16-colour
    /// palettes - loading at `(clut_x, clut_y)`.
    fn tim_4bpp_two_palettes(clut_x: u16, clut_y: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x08u32.to_le_bytes()); // pmode 0 = 4bpp, has CLUT
        // CLUT block: 12-byte header + 16*2 halfwords.
        buf.extend_from_slice(&(12u32 + 16 * 2 * 2).to_le_bytes());
        buf.extend_from_slice(&clut_x.to_le_bytes());
        buf.extend_from_slice(&clut_y.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes()); // w = 16 colours
        buf.extend_from_slice(&2u16.to_le_bytes()); // h = 2 palettes
        for i in 0..16u16 {
            buf.extend_from_slice(&(0x0100 + i).to_le_bytes()); // palette 0
        }
        for i in 0..16u16 {
            buf.extend_from_slice(&(0x0200 + i).to_le_bytes()); // palette 1
        }
        // Image block: 4 words of 4bpp indices at (0, 0).
        buf.extend_from_slice(&(12u32 + 4 * 2).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&[0u8; 8]);
        buf
    }

    /// A multi-palette CLUT block lands as a **linear run** along its row:
    /// palette `i` at `(fb_x + 16*i, fb_y)`, NOT stacked at `(fb_x, fb_y + i)`.
    ///
    /// This is what retail does (byte-pinned against the town0c field save
    /// state: 69 of 69 non-zero secondary palettes sit where the linear layout
    /// puts them, 0 where the rectangle would) and what the scene's prims ask
    /// for - their CBAs index those odd 16-colour slots. Uploading the block as
    /// a `w x h` rectangle left every odd slot empty, so the 4bpp prims reading
    /// them decoded to `0x0000`, got discarded, and punched holes in the town
    /// ground.
    #[test]
    fn multi_palette_clut_loads_linearly_along_the_row() {
        let mut vram = Vram::new();
        vram.upload_tim(&parse(&tim_4bpp_two_palettes(0, 480)).unwrap());

        // Palette 0: slot 0 of row 480.
        assert_eq!(vram.pixel(0, 480), 0x0100);
        assert_eq!(vram.pixel(15, 480), 0x010F);
        // Palette 1: the NEXT SLOT ON THE SAME ROW, not the row below.
        assert_eq!(
            vram.pixel(16, 480),
            0x0200,
            "palette 1 belongs at fb_x + 16"
        );
        assert_eq!(vram.pixel(31, 480), 0x020F);
        // The row below is untouched - a rectangle upload would have put
        // palette 1 there and left slot 1 of row 480 dead.
        assert_eq!(
            vram.pixel(0, 481),
            0,
            "palette 1 must not stack onto fb_y + 1"
        );
        assert_eq!(vram.pixel(15, 481), 0);
    }

    #[test]
    fn last_upload_wins_on_overlap() {
        let mut vram = Vram::new();
        vram.upload_tim(&parse(&tim_16bpp_at(0, 0)).unwrap());
        // Same address, different data
        let mut buf = tim_16bpp_at(0, 0);
        // Patch the first pixel of the image block (offset 8 + 4 = 12 then +12 for image header)
        // Image block starts at offset 8 (after header), pixel data starts at 8+12 = 20.
        buf[20] = 0x11;
        buf[21] = 0x11;
        vram.upload_tim(&parse(&buf).unwrap());
        assert_eq!(vram.pixel(0, 0), 0x1111);
    }

    #[test]
    fn upload_clips_at_vram_edge() {
        // fb_x = 1023 leaves only one column inside VRAM
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(1023, 0);
        vram.upload_tim(&parse(&buf).unwrap());
        assert_eq!(vram.pixel(1023, 0), 0xAAAA);
        // Other cells of the source were clipped, so this row stays zero past 1023.
        // (No way to read beyond VRAM_WIDTH; just verify nothing crashed.)
    }

    #[test]
    fn region_has_data_detects_filled_and_empty_rows() {
        let mut vram = Vram::new();
        // Plant a single non-zero pixel inside (10, 20).
        vram.write_words(10, 20, 1, 1, &[0x1234]);
        assert!(vram.region_has_data(0, 20, 64, 1));
        assert!(vram.region_has_data(10, 20, 1, 1));
        assert!(!vram.region_has_data(0, 19, 64, 1));
        // Out-of-bounds rectangles clip silently and report empty.
        assert!(!vram.region_has_data(2000, 0, 64, 1));
        assert!(!vram.region_has_data(0, 1000, 64, 1));
    }

    #[test]
    fn prim_has_texture_data_drops_empty_pages() {
        // Texture page at tpage_x=64, tpage_y=0 (TSB low nibble = 1, depth 4bpp).
        // CLUT row at cy=64, cx=0 (CBA = 64<<6 = 0x1000).
        let tsb = 0x0001;
        let cba = 64 << 6;
        let uvs = [(0, 0), (16, 16), (0, 16)];
        let mut vram = Vram::new();
        // Empty VRAM -> drop.
        assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
        // CLUT only -> still drop (page absent, would render flat CLUT[0]).
        vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
        assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
        // Page only -> also drop (no palette, would render transparent
        // holes anyway and just churn the GPU).
        let mut vram2 = Vram::new();
        vram2.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert!(!vram2.prim_has_texture_data(cba, tsb, &uvs));
        // Both populated -> keep.
        vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert!(vram.prim_has_texture_data(cba, tsb, &uvs));
        // Both populated, and CLUT row is exactly 256 wide for a 4bpp prim:
        // this is the standard multi-palette layout (16 distinct 16-entry
        // palettes per row, picked by the prim's CBA low 6 bits) so it's
        // KEPT - not a depth mismatch.
        let mut vram_multi = Vram::new();
        vram_multi.write_words(0, 64, 256, 1, &[0x1234; 256]);
        vram_multi.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert!(vram_multi.prim_has_texture_data(cba, tsb, &uvs));
        // CLUT row extends *past* 256 entries (= image data has spilled
        // onto the palette row from some other TIM upload) -> drop, the
        // 4bpp prim would index into junk pixels.
        let mut vram_spill = Vram::new();
        vram_spill.write_words(0, 64, 600, 1, &[0x1234; 600]);
        vram_spill.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert!(!vram_spill.prim_has_texture_data(cba, tsb, &uvs));
    }

    #[test]
    fn prim_texture_status_classifies_each_failure_mode() {
        // Texture page at tpage_x=64, tpage_y=0 (TSB low nibble = 1, depth 4bpp).
        // CLUT row at cy=64, cx=0 (CBA = 64<<6 = 0x1000).
        let tsb = 0x0001;
        let cba = 64 << 6;
        let uvs = [(0, 0), (16, 16), (0, 16)];

        // (1) Empty VRAM -> MissingClut.
        let vram = Vram::new();
        match vram.prim_texture_status(cba, tsb, &uvs) {
            PrimTextureStatus::MissingClut { row } => assert_eq!(row, 64),
            other => panic!("expected MissingClut, got {:?}", other),
        }

        // (2) CLUT only, sized correctly for 4bpp -> MissingTexturePage.
        let mut vram = Vram::new();
        vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
        match vram.prim_texture_status(cba, tsb, &uvs) {
            PrimTextureStatus::MissingTexturePage { .. } => {}
            other => panic!("expected MissingTexturePage, got {:?}", other),
        }

        // (3) Both populated, depth correct -> Ok.
        let mut vram = Vram::new();
        vram.write_words(0, 64, 16, 1, &[0x1234; 16]);
        vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert_eq!(
            vram.prim_texture_status(cba, tsb, &uvs),
            PrimTextureStatus::Ok
        );

        // (4) CLUT row populated *past* 16 4bpp palettes' worth (256
        // entries) for a 4bpp prim -> ClutDepthMismatch. Image data
        // from a different TIM has spilled onto this CLUT row, so the
        // 4bpp lookup would index into pixel data.
        let mut vram = Vram::new();
        vram.write_words(0, 64, 600, 1, &[0x1234; 600]);
        vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
        match vram.prim_texture_status(cba, tsb, &uvs) {
            PrimTextureStatus::ClutDepthMismatch {
                row,
                populated_width,
                expected_width,
            } => {
                assert_eq!(row, 64);
                assert_eq!(populated_width, 600);
                assert_eq!(expected_width, 16);
            }
            other => panic!("expected ClutDepthMismatch, got {:?}", other),
        }

        // (5) CLUT row exactly 256 wide for a 4bpp prim is *legitimate*
        // multi-palette (16 distinct 16-entry palettes packed in one
        // row, picked by CBA low 6 bits). Must NOT trigger depth
        // mismatch - this is Legaia's standard character-TIM layout.
        let mut vram = Vram::new();
        vram.write_words(0, 64, 256, 1, &[0x1234; 256]);
        vram.write_words(64, 0, 4, 16, &[0x4567; 64]);
        assert_eq!(
            vram.prim_texture_status(cba, tsb, &uvs),
            PrimTextureStatus::Ok
        );
    }

    #[test]
    fn row_populated_width_counts_run_length() {
        let mut vram = Vram::new();
        vram.write_words(0, 32, 16, 1, &[0xAAAA; 16]);
        assert_eq!(vram.row_populated_width(0, 32, 256), 16);
        assert_eq!(vram.row_populated_width(0, 32, 8), 8);
        // No data at this row.
        assert_eq!(vram.row_populated_width(0, 33, 256), 0);
        // Sparse: a single non-zero pixel at column 5.
        let mut vram = Vram::new();
        vram.write_words(5, 100, 1, 1, &[0xFFFF]);
        // Run length is "last non-zero column + 1 - start" = 6.
        assert_eq!(vram.row_populated_width(0, 100, 256), 6);
    }

    #[test]
    fn row_run_width_stops_at_a_large_gap() {
        let mut vram = Vram::new();
        // 240-wide palette band, then an empty gap, then unrelated data at 448.
        vram.write_words(0, 200, 240, 1, &[0x1234; 240]);
        vram.write_words(448, 200, 32, 1, &[0x4567; 32]);
        // Unbounded last-nonzero scan crosses the gap to col 479 -> 480.
        assert_eq!(vram.row_populated_width(0, 200, VRAM_WIDTH), 480);
        // Gap-tolerant run stops at the palette band (gap 240..448 = 208 > 64).
        assert_eq!(vram.row_run_width(0, 200, VRAM_WIDTH, 64), 240);
        // A small internal gap (<= max_gap) is tolerated and spanned.
        let mut vram = Vram::new();
        vram.write_words(0, 10, 16, 1, &[0xAAAA; 16]);
        vram.write_words(24, 10, 16, 1, &[0xBBBB; 16]); // gap of 8 zeros at 16..24
        assert_eq!(vram.row_run_width(0, 10, VRAM_WIDTH, 64), 40);
    }

    #[test]
    fn prim_texture_status_ok_when_clut_shares_scanline_with_a_distant_page() {
        // Reproduce the battle monster VRAM layout: a 4bpp prim whose CLUT row
        // sits inside the texture-page y-band, with the page far to the right
        // on the *same* scanline. The depth-mismatch heuristic must not count
        // the distant page as part of the palette. (Mirrors
        // legaia_asset::monster_archive::battle_render_mesh for slot 2: CLUT row
        // 486, page at x=448, both on row 486.)
        let row = 486u16;
        let cba = row << 6; // palette 0 on the monster CLUT row
        let tsb = 0x0007 | (1 << 4); // tpage_x col 7 (=448px), tpage_y=256, 4bpp
        let uvs = [(62, 38), (41, 0), (79, 0)];

        let mut vram = Vram::new();
        // 15 packed 16-colour palettes (240 cells) at x=0 on the CLUT row.
        vram.write_words(0, row, 240, 1, &[0x1234; 240]);
        // The slot's own 4bpp page: x=448, y=256, 256 rows tall, 32 cells wide
        // (128 texels). Row 486 is inside [256, 512), so the page shares the
        // CLUT's scanline.
        vram.write_words(448, 256, 32, 256, &[0x4567; 32 * 256]);

        assert_eq!(
            vram.prim_texture_status(cba, tsb, &uvs),
            PrimTextureStatus::Ok,
            "monster prim must render despite the CLUT row sharing a scanline with the page"
        );
    }

    #[test]
    fn prim_has_texture_data_15bpp_uses_page_only() {
        // 15bpp direct: depth bits = 2 in TSB. Bit 7..8: (tsb >> 7) & 0x3 = 2.
        let tsb = (2u16 << 7) | 1; // tpage_x = 64, depth = 15bpp
        let cba = 0; // ignored in 15bpp
        let uvs = [(0, 0), (8, 8), (0, 8)];
        let mut vram = Vram::new();
        assert!(!vram.prim_has_texture_data(cba, tsb, &uvs));
        vram.write_words(64, 0, 16, 8, &[0x7FFF; 128]);
        assert!(vram.prim_has_texture_data(cba, tsb, &uvs));
    }

    #[test]
    fn as_bytes_round_trips_le() {
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(0, 0);
        vram.upload_tim(&parse(&buf).unwrap());
        let bytes = vram.as_bytes();
        // First 8 bytes = first 4 pixels = 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD (LE)
        assert_eq!(
            &bytes[0..8],
            &[0xAA, 0xAA, 0xBB, 0xBB, 0xCC, 0xCC, 0xDD, 0xDD]
        );
    }

    #[test]
    fn write_clut_row_writes_halfwords_at_fbxy() {
        let mut vram = Vram::new();
        // 16 BGR555 halfwords spanning 0x0001..0x0010.
        let mut bytes = [0u8; 32];
        for i in 0..16u16 {
            bytes[(i as usize) * 2..(i as usize) * 2 + 2].copy_from_slice(&(i + 1).to_le_bytes());
        }
        vram.write_clut_row(128, 479, &bytes);
        for i in 0..16u16 {
            assert_eq!(vram.pixel(128 + i as usize, 479), i + 1);
        }
        // Adjacent pixels stay zero.
        assert_eq!(vram.pixel(127, 479), 0);
        assert_eq!(vram.pixel(144, 479), 0);
    }

    #[test]
    fn write_clut_row_skips_empty_input() {
        let mut vram = Vram::new();
        // Sanity: no panic, no writes.
        vram.write_clut_row(0, 0, &[]);
        assert_eq!(vram.pixel(0, 0), 0);
    }

    #[test]
    fn underlay_fills_only_zero_words() {
        let mut scene = Vram::new();
        let mut boot = Vram::new();
        // Scene wrote (5, 300); boot wrote (5, 300) and (6, 300).
        scene.write_block(5, 300, 1, 1, &0xAAAAu16.to_le_bytes());
        boot.write_block(5, 300, 1, 1, &0xBBBBu16.to_le_bytes());
        boot.write_block(6, 300, 1, 1, &0xCCCCu16.to_le_bytes());
        scene.underlay(&boot);
        // Scene word wins the overlap; boot shows through the zero word.
        assert_eq!(scene.pixel(5, 300), 0xAAAA);
        assert_eq!(scene.pixel(6, 300), 0xCCCC);
    }

    #[test]
    fn move_image_copies_a_rect() {
        let mut vram = Vram::new();
        // 2x2 source block at (10, 20).
        for (i, &v) in [0x1111u16, 0x2222, 0x3333, 0x4444].iter().enumerate() {
            let bytes = v.to_le_bytes();
            vram.write_block(10 + (i % 2) as u16, 20 + (i / 2) as u16, 1, 1, &bytes);
        }
        vram.move_image(10, 20, 2, 2, 100, 200);
        assert_eq!(vram.pixel(100, 200), 0x1111);
        assert_eq!(vram.pixel(101, 200), 0x2222);
        assert_eq!(vram.pixel(100, 201), 0x3333);
        assert_eq!(vram.pixel(101, 201), 0x4444);
        // Source untouched.
        assert_eq!(vram.pixel(10, 20), 0x1111);
        // A zero-size rect is a no-op (the retail wrapper rejects it).
        vram.move_image(10, 20, 0, 2, 300, 300);
        vram.move_image(10, 20, 2, 0, 300, 300);
        assert_eq!(vram.pixel(300, 300), 0);
    }
}
