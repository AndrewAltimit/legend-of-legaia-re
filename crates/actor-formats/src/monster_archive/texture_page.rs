//! The monster battle-texture page as an addressable, editable texture.
//!
//! [`mesh`](super::mesh) already decodes a monster's texture pool into
//! palettes plus a 4bpp page. What a texture *browser* needs on top of that
//! is the three things that make a page a row in a list rather than a byte
//! range: a name, an identity, and a colouring that is not a guess.
//!
//! ## Why the TIM scanners cannot see these
//!
//! A monster's pixels live inside its LZS-compressed archive slot as a bare
//! `[15 x 16 BGR555][w*h/2 bytes of 4bpp]` pool - no TIM magic, no header
//! word, and the page geometry comes from the *loader's* `StoreImage` rect
//! rather than from the bytes (see [`MonsterTexture`]). So the raw TIM
//! catalog reports nothing for PROT 867 and the deep (LZS) catalog reports
//! one 64x64 effect texture, which is not a monster. Every enemy, boss and
//! their Ra-Seru is invisible to a scan that looks for TIMs.
//!
//! ## No page has one colouring
//!
//! The pool ships fifteen palettes and a prim picks one with its CBA column,
//! so "the" colour of a texel is a property of the *primitive that samples
//! it*, not of the page. Decoding a whole page through palette 0 - the
//! obvious convention - is therefore a plausible-looking wrong image, and on
//! some monsters it is a spectacular one: Songi's transformed form (id 179)
//! spends 44% of its page on indices 14 and 15, which palette 0 paints pure
//! red and pure green. Through the palettes the model actually uses those
//! same texels are grey.
//!
//! [`MonsterPage::ownership`] resolves that the way the renderer does: walk
//! the embedded TMD and let each textured primitive claim the texels its UV
//! polygon covers, for its own palette. [`MonsterPage::rgba`] then decodes
//! every texel through its owner.
//!
//! A page is a sheet of art islands with filler between them, and the filler
//! is where the lurid colours live: on id 179 it is pure red and pure green.
//! Texels no primitive claims are **dead bytes** - nothing on the model
//! samples them - so they decode transparent instead of through some
//! palette's idea of index 14. That is what keeps a whole-page decode from
//! reading as a red/green checkerboard, and it is why the claim is by
//! polygon containment rather than by UV bounding box: a box around a
//! diagonal face swallows the filler beside it.

use anyhow::Result;

use super::mesh::{CLUT_COUNT, CLUT_REGION_BYTES, MonsterMesh, MonsterTexture};
use super::{decode_block, slot_count};

/// Bytes one 16-colour palette occupies in the pool's CLUT region.
pub const PALETTE_BYTES: usize = 32;
/// Colours in one palette.
pub const PALETTE_COLOURS: usize = 16;

/// One monster's texture page, with everything a browser row and a
/// replacement both need. Holds the decoded block, because the block is the
/// edit surface: a replacement splices into it and re-LZS's the whole thing.
#[derive(Debug, Clone)]
pub struct MonsterPage {
    /// 1-based monster id (the archive slot index + 1).
    pub id: u16,
    /// The monster's own name string, read out of its record. Duplicated
    /// across ids on purpose in retail - "Songi" is ids 76, 136 and 179 -
    /// so a caller that presents this must present [`id`](Self::id) too.
    pub name: String,
    /// The LZS-decoded archive block.
    pub block: Vec<u8>,
    /// Block-relative byte offset of the texture pool (record `+0x08`).
    pub pool_offset: usize,
    /// The decoded palettes + 4bpp page.
    pub texture: MonsterTexture,
}

impl MonsterPage {
    /// Page width in texels (128 or 256).
    pub fn width(&self) -> usize {
        self.texture.width
    }

    /// Page height in texels (always 256).
    pub fn height(&self) -> usize {
        self.texture.height
    }

    /// Bytes the pool occupies in the decoded block: the CLUT region plus
    /// the packed 4bpp page.
    pub fn byte_len(&self) -> usize {
        CLUT_REGION_BYTES + self.texture.indices.len() / 2
    }

    /// The pool's stored bytes - palettes and pixels, exactly as they sit in
    /// the decoded block. The identity a change pack pins a replacement to.
    pub fn pool_bytes(&self) -> &[u8] {
        let end = (self.pool_offset + self.byte_len()).min(self.block.len());
        &self.block[self.pool_offset..end]
    }

    /// How many of the fifteen palettes carry any colour at all. The rest
    /// are zero padding for a monster that uses fewer.
    pub fn palettes_populated(&self) -> usize {
        self.texture
            .palettes
            .iter()
            .filter(|p| p.iter().any(|c| c[3] != 0))
            .count()
    }

    /// Which palette each texel is sampled through, or `None` for a texel no
    /// textured primitive covers.
    ///
    /// A texel is claimed by a primitive whose UV polygon **contains** it,
    /// and the lowest palette id wins a texel two primitives contest, so the
    /// map does not depend on primitive order.
    ///
    /// Containment, not the UV bounding box, and that is the whole design.
    /// A page is a sheet of art islands with filler between them, and a box
    /// around a diagonal face swallows the filler beside it: on Songi #179 -
    /// where retail's filler is pure red and pure green - a box-based cover
    /// painted thousands of those filler texels as if they were the
    /// monster's skin. The islands are what the model reads; the gaps are
    /// not.
    ///
    /// One texel of dilation follows, because PSX sampling is nearest-texel
    /// and a face's outermost row sits exactly on its UV edge - without it a
    /// hairline of every island would read as unsampled. A dilated texel
    /// inherits the lowest palette among its claimed neighbours.
    pub fn ownership(&self) -> Vec<Option<u8>> {
        let (w, h) = (self.texture.width, self.texture.height);
        let mut claim: Vec<Option<u8>> = vec![None; w * h];
        let tmd_bytes = &self.block[self.tmd_offset()..];
        let Ok(tmd) = legaia_tmd::parse(tmd_bytes) else {
            return vec![None; w * h];
        };
        for object in &tmd.objects {
            let groups = legaia_tmd::legaia_prims::iter_groups_lenient(
                tmd_bytes,
                object.primitives_byte_offset,
                object.primitives_byte_size,
            );
            for group in &groups {
                for prim in &group.prims {
                    // Only a textured prim carries UVs, and only a textured
                    // prim samples the page.
                    if prim.uvs.is_empty() {
                        continue;
                    }
                    let palette = (prim.cba & 0x3F) as u8;
                    if palette as usize >= CLUT_COUNT {
                        continue;
                    }
                    let uv: Vec<(i32, i32)> = prim
                        .uvs
                        .iter()
                        .map(|&(u, v)| (u as i32, v as i32))
                        .collect();
                    let (mut x0, mut x1) = (i32::MAX, i32::MIN);
                    let (mut y0, mut y1) = (i32::MAX, i32::MIN);
                    for &(u, v) in &uv {
                        x0 = x0.min(u);
                        x1 = x1.max(u);
                        y0 = y0.min(v);
                        y1 = y1.max(v);
                    }
                    if x0 >= w as i32 || y0 >= h as i32 {
                        continue;
                    }
                    // A PSX quad is a strip: (0,1,2) and (1,3,2).
                    let tris: &[[usize; 3]] = match uv.len() {
                        3 => &[[0, 1, 2]],
                        4 => &[[0, 1, 2], [1, 3, 2]],
                        _ => &[],
                    };
                    for y in y0.max(0)..=y1.min(h as i32 - 1) {
                        for x in x0.max(0)..=x1.min(w as i32 - 1) {
                            if !tris
                                .iter()
                                .any(|t| point_in_tri((x, y), uv[t[0]], uv[t[1]], uv[t[2]]))
                            {
                                continue;
                            }
                            let slot = &mut claim[y as usize * w + x as usize];
                            if slot.is_none_or(|p| palette < p) {
                                *slot = Some(palette);
                            }
                        }
                    }
                }
            }
        }

        // The one-texel skirt. Read off the claimed map, written into a copy,
        // so a dilated texel can never seed a further dilation.
        let mut out = claim.clone();
        for y in 0..h {
            for x in 0..w {
                if claim[y * w + x].is_some() {
                    continue;
                }
                let mut best: Option<u8> = None;
                for (dy, dx) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (ny, nx) = (y as i32 + dy, x as i32 + dx);
                    if ny < 0 || nx < 0 || ny >= h as i32 || nx >= w as i32 {
                        continue;
                    }
                    if let Some(p) = claim[ny as usize * w + nx as usize]
                        && best.is_none_or(|b| p < b)
                    {
                        best = Some(p);
                    }
                }
                out[y * w + x] = best;
            }
        }
        out
    }

    /// Block-relative offset of the embedded TMD (record `+0x04`).
    fn tmd_offset(&self) -> usize {
        legaia_bytes::u32_le(&self.block, 0x04).unwrap_or(0) as usize
    }

    /// Decode the page the way the game colours it: every texel through the
    /// palette of the primitive that samples it, and a texel no primitive
    /// samples fully transparent.
    ///
    /// `width * height * 4` bytes, row-major. Pass an [`ownership`](Self::ownership)
    /// map so a caller that needs it twice pays for it once.
    pub fn rgba(&self, owner: &[Option<u8>]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.texture.indices.len() * 4);
        for (i, &idx) in self.texture.indices.iter().enumerate() {
            match owner.get(i).copied().flatten() {
                Some(p) => {
                    let pal = &self.texture.palettes[(p as usize).min(CLUT_COUNT - 1)];
                    out.extend_from_slice(&pal[idx as usize]);
                }
                // A dead texel. Nothing on the model reads it, so there is
                // no palette that would be the truth about its colour.
                None => out.extend_from_slice(&[0, 0, 0, 0]),
            }
        }
        out
    }

    /// The 16 stored BGR555 entries of palette `p`, as they sit on disc.
    pub fn palette_raw(&self, p: usize) -> Option<[u16; PALETTE_COLOURS]> {
        let base = self.pool_offset + p * PALETTE_BYTES;
        let mut out = [0u16; PALETTE_COLOURS];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = legaia_bytes::u16_le(&self.block, base + i * 2)?;
        }
        Some(out)
    }

    /// Byte offset of palette `p`'s entries inside the decoded block.
    pub fn palette_offset(&self, p: usize) -> usize {
        self.pool_offset + p * PALETTE_BYTES
    }

    /// Byte offset of the packed 4bpp page inside the decoded block.
    pub fn pixels_offset(&self) -> usize {
        self.pool_offset + CLUT_REGION_BYTES
    }
}

/// Is `p` inside triangle `abc` (edges included)? Integer half-plane test,
/// winding-agnostic - a UV triangle can be wound either way.
fn point_in_tri(p: (i32, i32), a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> bool {
    let cross = |u: (i32, i32), v: (i32, i32), w: (i32, i32)| {
        (v.0 - u.0) * (w.1 - u.1) - (v.1 - u.1) * (w.0 - u.0)
    };
    let (d0, d1, d2) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d0 < 0 || d1 < 0 || d2 < 0;
    let pos = d0 > 0 || d1 > 0 || d2 > 0;
    !(neg && pos)
}

/// FNV-1a-64, the fingerprint the texture catalogs key rows by.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Load monster `id`'s texture page: one LZS decode, then the name, the
/// mesh pointer and the pool.
///
/// Returns `Ok(None)` for an empty / filler slot, a slot whose `+0x04` does
/// not point at a TMD, or one carrying no decodable pool. `Err` only on a
/// genuine LZS failure.
pub fn page(entry: &[u8], id: u16) -> Result<Option<MonsterPage>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    let Some(record) = super::record::parse_block(id, &block) else {
        return Ok(None);
    };
    let tmd_offset = legaia_bytes::u32_le(&block, 0x04).unwrap_or(0) as usize;
    if tmd_offset + 4 > block.len()
        || legaia_bytes::u32_le(&block, tmd_offset) != Some(super::mesh::TMD_MAGIC)
    {
        return Ok(None);
    }
    let pool_offset = legaia_bytes::u32_le(&block, 0x08).unwrap_or(0) as usize;
    let mesh = MonsterMesh {
        id,
        block,
        tmd_offset,
        texture_pool_offset: pool_offset,
    };
    let Some(texture) = mesh.texture() else {
        return Ok(None);
    };
    Ok(Some(MonsterPage {
        id,
        name: record.name,
        block: mesh.block,
        pool_offset,
        texture,
    }))
}

/// Every populated monster's texture page, in id order.
///
/// One LZS decode per slot; skips filler slots silently. Propagates an `Err`
/// only on a genuine decode failure.
pub fn pages(entry: &[u8]) -> Result<Vec<MonsterPage>> {
    let mut out = Vec::new();
    for id in 1..=slot_count(entry) as u16 {
        if let Some(p) = page(entry, id)? {
            out.push(p);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The composite decode's contract, on a hand-built page: an owned texel
    /// takes its owner's palette, an unowned one is transparent - which is
    /// what keeps a dead region from being painted in some palette's colours
    /// and read as the monster's art.
    #[test]
    fn composite_decode_uses_the_owner_and_blanks_the_rest() {
        let mut palettes = vec![[[0u8; 4]; PALETTE_COLOURS]; CLUT_COUNT];
        palettes[0][3] = [255, 0, 0, 255];
        palettes[6][3] = [0, 255, 0, 255];
        let page = MonsterPage {
            id: 1,
            name: "Test".into(),
            block: vec![0u8; 0x100],
            pool_offset: 0,
            texture: MonsterTexture {
                palettes,
                indices: vec![3, 3, 3, 3],
                width: 2,
                height: 2,
            },
        };
        let owner = vec![Some(0u8), Some(6), None, Some(0)];
        let rgba = page.rgba(&owner);
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255], "owner 0 paints red");
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255], "owner 6 paints green");
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0], "an unowned texel is blank");
        assert_eq!(&rgba[12..16], &[255, 0, 0, 255]);
    }

    #[test]
    fn byte_len_is_the_clut_region_plus_the_packed_page() {
        let page = MonsterPage {
            id: 1,
            name: "Test".into(),
            block: vec![0u8; 0x1000],
            pool_offset: 0,
            texture: MonsterTexture {
                palettes: vec![[[0u8; 4]; PALETTE_COLOURS]; CLUT_COUNT],
                indices: vec![0; 128 * 256],
                width: 128,
                height: 256,
            },
        };
        assert_eq!(page.byte_len(), CLUT_REGION_BYTES + 128 * 256 / 2);
        assert_eq!(page.pixels_offset(), CLUT_REGION_BYTES);
        assert_eq!(page.palette_offset(2), 2 * PALETTE_BYTES);
    }
}
