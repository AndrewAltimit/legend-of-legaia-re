//! Monster mesh + battle texture: the embedded TMD and its CLUT/page pool.

use anyhow::Result;

use super::decode_block;

/// TMD magic of the Legaia variant the monster meshes use (custom PSX TMD).
pub(super) const TMD_MAGIC: u32 = 0x8000_0002;

/// A monster's embedded 3D model, located inside its decoded archive block.
///
/// The monster mesh is a [Legaia TMD](../../tmd) stored verbatim in the block
/// at the offset held in the stat record's `+0x04` field (the same pointer the
/// battle loader fixes up into the actor's `+0x230` attack-effect/animation
/// data slot - the "0x1C-stride geometry records" walked by `FUN_80049858`
/// are this TMD's per-object table). The matching texture / CLUT pool is at
/// `+0x08`; [`texture`](MonsterMesh::texture) decodes it into palettes + a 4bpp
/// page (layout pinned from the loader `FUN_80055468`; see [`MonsterTexture`]).
#[derive(Debug, Clone)]
pub struct MonsterMesh {
    /// 1-based monster id (archive slot index + 1).
    pub id: u16,
    /// The full LZS-decoded archive block. The TMD and texture pool are slices
    /// into this buffer.
    pub block: Vec<u8>,
    /// Block-relative byte offset of the embedded TMD (stat record `+0x04`).
    pub tmd_offset: usize,
    /// Block-relative byte offset of the texture / CLUT pool (stat record
    /// `+0x08`). `0` when the record carries no pool pointer.
    pub texture_pool_offset: usize,
}

impl MonsterMesh {
    /// The embedded TMD bytes (from [`tmd_offset`](Self::tmd_offset) to the end
    /// of the block). The TMD parser stops at the model's own extent, so the
    /// trailing pool/spell bytes are harmless. Parse with `legaia_tmd::parse`.
    pub fn tmd_bytes(&self) -> &[u8] {
        &self.block[self.tmd_offset..]
    }

    /// The texture / CLUT pool bytes (from
    /// [`texture_pool_offset`](Self::texture_pool_offset) to the end of the
    /// block), or `None` when the record carries no pool pointer or the
    /// offset is out of range. See [`texture`](Self::texture) for the decoded
    /// palettes + 4bpp page.
    pub fn texture_pool_bytes(&self) -> Option<&[u8]> {
        if self.texture_pool_offset == 0 || self.texture_pool_offset >= self.block.len() {
            return None;
        }
        Some(&self.block[self.texture_pool_offset..])
    }

    /// Decode the texture pool into its palettes + 4bpp page.
    ///
    /// Returns `None` when there is no pool or it's too small to hold the CLUT
    /// region plus at least one texture row. See [`MonsterTexture`] for the
    /// layout and the `FUN_80055468` provenance.
    pub fn texture(&self) -> Option<MonsterTexture> {
        let pool = self.texture_pool_bytes()?;
        if pool.len() <= CLUT_REGION_BYTES {
            return None;
        }
        // 15 sequential 16-colour CLUTs at the head; the loader uploads the
        // whole 240-colour region to one VRAM row and a prim picks palette
        // `cba & 0x3F`. Index-0 colour 0x0000 is the PSX transparent texel.
        let palettes: Vec<[[u8; 4]; 16]> = (0..CLUT_COUNT)
            .map(|c| {
                let mut pal = [[0u8; 4]; 16];
                for (i, slot) in pal.iter_mut().enumerate() {
                    let raw = legaia_bytes::u16_le(pool, c * 32 + i * 2).unwrap_or(0);
                    *slot = bgr555_to_rgba(raw);
                }
                pal
            })
            .collect();

        // The 4bpp page is always 256 rows tall (StoreImage RECT.h); width is
        // whatever the remaining bytes divide into across those rows (64 B/row
        // = 128 texels for narrow monsters, 128 B/row = 256 texels for wide).
        let pixels = &pool[CLUT_REGION_BYTES..];
        let bytes_per_row = pixels.len() / TEXTURE_HEIGHT;
        if bytes_per_row == 0 {
            return None;
        }
        let width = bytes_per_row * 2;
        let mut indices = vec![0u8; width * TEXTURE_HEIGHT];
        for y in 0..TEXTURE_HEIGHT {
            for xb in 0..bytes_per_row {
                let b = pixels[y * bytes_per_row + xb];
                indices[y * width + xb * 2] = b & 0x0F;
                indices[y * width + xb * 2 + 1] = b >> 4;
            }
        }
        Some(MonsterTexture {
            palettes,
            indices,
            width,
            height: TEXTURE_HEIGHT,
        })
    }

    /// Build a renderable, battle-slot-relocated [`legaia_tmd::mesh::VramMesh`]
    /// for this monster and inject its texture pool into `vram` at the
    /// coordinates the battle loader `FUN_80055468` uses for `slot`.
    ///
    /// The on-disc CBA/TSB in the embedded TMD are nominal defaults; the
    /// loader relocates them per battle slot. This mirrors that relocation so
    /// the standard PSX VRAM texture path renders the monster directly:
    ///
    /// - the CLUT region ([`CLUT_REGION_BYTES`], 15 palettes) is written to
    ///   VRAM row `484 + slot` at x=0, and every prim's CBA row is moved by
    ///   `slot + 3` ([`relocate_cba`]: on-disc row `481` lands there);
    /// - the 4bpp page is written at [`monster_page_origin`] (`((5+slot)*64,
    ///   256)`), and every prim's TSB column is moved by `slot` with
    ///   `tpage_y` raised to 256 ([`relocate_tsb`]: on-disc column `5` lands
    ///   there). A boss's prims on row `482` / column `6` land on the boss
    ///   effect page instead ([`super::write_boss_effect_page`]).
    ///
    /// Per-vertex UVs are page-local and left untouched - they resolve
    /// correctly once the page sits at the relocated tpage origin. Returns
    /// `None` if the embedded TMD doesn't parse; otherwise a mesh with the
    /// relocated CBA/TSB (possibly empty if the monster has no textured
    /// prims). `slot` is the 0-based battle monster slot (`0..=4`).
    ///
    /// PORT: FUN_80055468
    pub fn battle_render_mesh(
        &self,
        slot: u8,
        vram: &mut legaia_tim::Vram,
    ) -> Option<legaia_tmd::mesh::VramMesh> {
        let tmd = legaia_tmd::parse(self.tmd_bytes()).ok()?;
        let mut mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, self.tmd_bytes());

        // Inject the texture pool at the loader's per-slot VRAM coords so the
        // relocated CBA/TSB resolve against populated VRAM.
        if let Some(pool) = self.texture_pool_bytes()
            && pool.len() > CLUT_REGION_BYTES
        {
            vram.write_clut_row(0, monster_clut_row(slot), &battle_clut_region(pool));

            let page = &pool[CLUT_REGION_BYTES..];
            let bytes_per_row = page.len() / TEXTURE_HEIGHT;
            if bytes_per_row != 0 {
                let (page_x, page_y) = monster_page_origin(slot);
                // One VRAM cell is one halfword = 4 4bpp texels = 2 source
                // bytes, so the per-row cell count is `bytes_per_row / 2`.
                vram.write_block(
                    page_x,
                    page_y,
                    (bytes_per_row / 2) as u16,
                    TEXTURE_HEIGHT as u16,
                    page,
                );
            }
        }

        for ct in &mut mesh.cba_tsb {
            ct[0] = relocate_cba(ct[0], slot);
            ct[1] = relocate_tsb(ct[1], slot);
        }
        Some(mesh)
    }
}

/// VRAM row the battle loader (`FUN_80055468`) uploads a monster's CLUT
/// region to: row `484 + slot`, palettes packed from x=0.
pub const MONSTER_CLUT_ROW_BASE: u16 = 484;
/// Texture-page x-origin in VRAM tpage columns (64 px each). The loader bases
/// the monster page at 320 px = column 5, then offsets by the battle slot.
const MONSTER_PAGE_TPAGE_BASE: u16 = 5;
/// Texture-page y-origin in VRAM rows (always 256; the loader's StoreImage
/// `RECT.y`).
const MONSTER_PAGE_Y: u16 = 256;

/// The CLUT region as the battle loader uploads it: every non-zero entry
/// takes the STP bit (`0x8000`), the zero entry stays the transparent texel.
///
/// `FUN_80055468` walks the region's `0xF0` halfwords before the upload
/// (`0x8005552C..0x8005554C`: `lh` / `beq v0,zero` / `or v0,v1,a1` with
/// `a1 = -0x8000`, `sh`). The bit is what lets a semi-transparent draw blend
/// the body at all - the PSX blends only STP texels of a semi-transparent
/// prim and draws the rest opaque - so without it the near-camera ghost
/// pass (`B + F/4`) and the defeat / capture fade drew the body solid.
pub fn battle_clut_region(pool: &[u8]) -> Vec<u8> {
    let mut out = pool[..CLUT_REGION_BYTES.min(pool.len())].to_vec();
    for c in out.as_chunks_mut::<2>().0 {
        let v = u16::from_le_bytes([c[0], c[1]]);
        if v != 0 {
            *c = (v | 0x8000).to_le_bytes();
        }
    }
    out
}

/// VRAM row of the monster CLUT region for battle `slot`.
fn monster_clut_row(slot: u8) -> u16 {
    MONSTER_CLUT_ROW_BASE + slot as u16
}

/// Top-left `(x, y)` in VRAM pixels of the monster 4bpp texture page for
/// battle `slot`: `((5 + slot) * 64, 256)`.
pub fn monster_page_origin(slot: u8) -> (u16, u16) {
    ((MONSTER_PAGE_TPAGE_BASE + slot as u16) * 64, MONSTER_PAGE_Y)
}

/// Relocate a prim's CBA to battle `slot` the way the loader does: it **adds**
/// `slot + 3` to the CLUT row (`addu v0,v0,t7` at `0x800555FC`, `t7 = (slot +
/// 3) << 22` on the word whose upper half is the CBA). The on-disc rows are
/// `481` (the monster's own palettes, which land on `484 + slot`, where
/// [`MonsterMesh::battle_render_mesh`] writes them) and, for the bosses
/// `0xB3..=0xB6`, `482` - the boss effect page's CLUT row `485` the loader
/// stages for those ids ([`super::BOSS_PAGE_CLUT_ROW`]). The palette index
/// `cba & 0x3F` is untouched.
///
/// PORT: FUN_80055468 (`0x800555E8..0x80055610`, the CBA half)
pub fn relocate_cba(cba: u16, slot: u8) -> u16 {
    cba.wrapping_add((u16::from(slot) + 3) << 6)
}

/// Relocate a prim's TSB to battle `slot` the way the loader does: it adds
/// `slot + 0x10` (`addu v1,v1,t2` at `0x80055600`, `t2 = (slot + 0x10) << 16`
/// on the word whose upper half is the TSB) - the tpage column moves by the
/// slot and the `+0x10` raises `tpage_y` to 256. The on-disc columns are `5`
/// (the monster's own page, [`monster_page_origin`]) and, for the bosses
/// `0xB3..=0xB6`, `6` - the boss effect page at `(0x180, 0x100)`
/// ([`super::BOSS_PAGE_RECT`]). Depth and abr bits ride through; bit 15 (the
/// engine-side semi-transparency enable packed by
/// `legaia_tmd::mesh::pack_tsb_semi`) is kept out of the add.
///
/// PORT: FUN_80055468 (`0x800555E8..0x80055610`, the TSB half)
pub fn relocate_tsb(tsb: u16, slot: u8) -> u16 {
    (tsb & 0x8000) | ((tsb & 0x7FFF).wrapping_add(u16::from(slot) + 0x10) & 0x7FFF)
}

/// Size of the CLUT region at the head of the texture pool: 15 sequential
/// 16-colour palettes (`15 * 16 * 2` bytes). The loader (`FUN_80055468`)
/// uploads this region to VRAM row `484 + battle_slot`, 256 colours wide.
pub const CLUT_REGION_BYTES: usize = 0x1E0;
/// Number of 16-colour palettes in the CLUT region. A prim selects palette
/// `cba & 0x3F`; the rest are zero-padded for monsters that use fewer.
pub const CLUT_COUNT: usize = 15;
/// Texture-page height in texels. Always 256 (the `FUN_80055468` StoreImage
/// `RECT.h`); the page width varies (128 or 256 texels).
pub const TEXTURE_HEIGHT: usize = 256;

/// Convert a PSX BGR555 colour to RGBA8. The all-zero colour (`0x0000`) is the
/// PSX transparent texel and maps to alpha 0; every other colour is opaque.
fn bgr555_to_rgba(v: u16) -> [u8; 4] {
    let r = ((v & 0x1F) << 3) as u8;
    let g = (((v >> 5) & 0x1F) << 3) as u8;
    let b = (((v >> 10) & 0x1F) << 3) as u8;
    let a = if v == 0 { 0 } else { 255 };
    [r, g, b, a]
}

/// A monster's decoded battle texture: the palette set plus the 4bpp page.
///
/// Reverse-engineered from the battle loader `FUN_80055468` (see
/// `ghidra/scripts/funcs/80055468.txt`), which the streaming archive loader
/// `FUN_800542C8` calls with the pool pointer (record `+0x08`), the embedded
/// TMD (record `+0x04`), and the battle slot index. The pool is laid out as:
///
/// ```text
/// +0x000  15 x [16 BGR555 colours]   ; CLUT region (0x1E0 bytes, zero-padded)
/// +0x1E0  4bpp indices               ; width x 256 texels, row-major
/// ```
///
/// A textured prim references CLUT base `cba` (palette = `cba & 0x3F`) and
/// samples the page at its per-vertex `(u, v)`; index 0 is transparent.
#[derive(Debug, Clone)]
pub struct MonsterTexture {
    /// The 15 palettes, each 16 RGBA8 colours. A prim with CLUT base `cba`
    /// uses `palettes[(cba & 0x3F) as usize]` (clamp to `CLUT_COUNT`).
    pub palettes: Vec<[[u8; 4]; 16]>,
    /// One 4bpp palette index per texel, row-major (`width * height` bytes).
    pub indices: Vec<u8>,
    /// Page width in texels (128 for narrow monsters, 256 for wide ones).
    pub width: usize,
    /// Page height in texels (always [`TEXTURE_HEIGHT`] = 256).
    pub height: usize,
}

impl MonsterTexture {
    /// Bake the page into a flat RGBA8 image using the given palette index
    /// (`cba & 0x3F` of the prim you want to preview). Transparent texels keep
    /// alpha 0. `width * height * 4` bytes, row-major top-to-bottom.
    pub fn to_rgba(&self, palette: usize) -> Vec<u8> {
        let pal = &self.palettes[palette.min(self.palettes.len() - 1)];
        let mut out = Vec::with_capacity(self.indices.len() * 4);
        for &idx in &self.indices {
            out.extend_from_slice(&pal[idx as usize]);
        }
        out
    }

    /// Flatten the 15 palettes into a single `15 * 16` RGBA8 row, suitable for
    /// uploading as a palette lookup texture (palette `p`, colour `c` is at
    /// pixel `p * 16 + c`).
    pub fn palette_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CLUT_COUNT * 16 * 4);
        for pal in &self.palettes {
            for colour in pal {
                out.extend_from_slice(colour);
            }
        }
        out
    }
}

/// Locate monster id `id`'s embedded 3D mesh.
///
/// Returns `Ok(None)` for an out-of-range id, an empty / filler slot, or a slot
/// whose `+0x04` pointer does not land on a TMD magic (`0x80000002`). Returns
/// `Err` only on a genuine LZS decode failure. The mesh is a Legaia TMD; see
/// [`MonsterMesh`].
pub fn mesh(entry: &[u8], id: u16) -> Result<Option<MonsterMesh>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    // The stat record's +0x04 holds the block-relative TMD offset (and +0x08
    // the texture pool). Validate the TMD magic before trusting the pointer so
    // filler / non-mesh slots return None rather than a bogus offset.
    let Some(tmd_offset) = legaia_bytes::u32_le(&block, 0x04).map(|v| v as usize) else {
        return Ok(None);
    };
    if tmd_offset + 4 > block.len() || legaia_bytes::u32_le(&block, tmd_offset) != Some(TMD_MAGIC) {
        return Ok(None);
    }
    let texture_pool_offset = legaia_bytes::u32_le(&block, 0x08).unwrap_or(0) as usize;
    Ok(Some(MonsterMesh {
        id,
        block,
        tmd_offset,
        texture_pool_offset,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loader's STP pass: non-zero entries take bit 15, the transparent
    /// zero entry stays zero, and an entry already carrying the bit keeps it.
    #[test]
    fn the_battle_clut_region_sets_stp_on_every_non_zero_entry() {
        let mut pool = vec![0u8; CLUT_REGION_BYTES + 4];
        pool[2..4].copy_from_slice(&0x0A21u16.to_le_bytes());
        pool[4..6].copy_from_slice(&0x8001u16.to_le_bytes());
        let out = battle_clut_region(&pool);
        assert_eq!(out.len(), CLUT_REGION_BYTES);
        let at = |i: usize| u16::from_le_bytes([out[i * 2], out[i * 2 + 1]]);
        assert_eq!(at(0), 0);
        assert_eq!(at(1), 0x8A21);
        assert_eq!(at(2), 0x8001);
        assert!((3..CLUT_REGION_BYTES / 2).all(|i| at(i) == 0));
    }

    /// `relocate_cba` adds `slot + 3` to the row: the monster's own on-disc
    /// row `481` lands on `484 + slot`, where `battle_render_mesh` writes the
    /// palettes, and a boss's `482` on the boss page's row `485` at slot 0.
    #[test]
    fn relocate_cba_adds_the_slot_to_the_row() {
        for slot in 0u8..5 {
            for palette in 0u16..15 {
                let relocated = relocate_cba((481u16 << 6) | palette, slot);
                assert_eq!(relocated & 0x3F, palette, "palette preserved");
                assert_eq!(
                    (relocated >> 6) & 0x1FF,
                    MONSTER_CLUT_ROW_BASE + slot as u16,
                    "CLUT row = 484 + slot"
                );
            }
        }
        assert_eq!(
            relocate_cba((482 << 6) | 9, 0) >> 6,
            super::super::BOSS_PAGE_CLUT_ROW
        );
    }

    /// `relocate_tsb` adds `slot + 0x10`: column `5` lands on `5 + slot` at
    /// `tpage_y = 256`, a boss's column `6` on the boss page, and abr, depth
    /// and the engine's bit 15 ride through.
    #[test]
    fn relocate_tsb_adds_the_slot_to_the_page() {
        for slot in 0u8..5 {
            for abr in 0u16..4 {
                let on_disc = 0x05 | (abr << 5);
                let relocated = relocate_tsb(on_disc, slot);
                let tpage_x = (relocated & 0xF) * 64;
                let tpage_y = ((relocated >> 4) & 1) * 256;
                assert_eq!(tpage_x, monster_page_origin(slot).0, "page x = (5+slot)*64");
                assert_eq!(tpage_y, 256, "tpage_y = 256");
                assert_eq!((relocated >> 7) & 0x3, 0, "4bpp depth");
                assert_eq!((relocated >> 5) & 0x3, abr, "abr preserved");
                let semi = relocate_tsb(on_disc | 0x8000, slot);
                assert_eq!(semi & 0x8000, 0x8000, "bit 15 preserved");
                assert_eq!(semi & 0x7FFF, relocated, "low bits unchanged");
                assert_eq!(relocated & 0x8000, 0, "bit 15 not invented");
            }
        }
        let boss = relocate_tsb(0x06, 0);
        assert_eq!(
            ((boss & 0xF) * 64, ((boss >> 4) & 1) * 256),
            (
                super::super::BOSS_PAGE_RECT.0,
                super::super::BOSS_PAGE_RECT.1
            )
        );
    }

    /// The texture page never overlaps any slot's CLUT row: palettes occupy
    /// x in `0..240`, pages start at x>=320, so injection slots are disjoint.
    #[test]
    fn monster_page_clear_of_clut_region() {
        for slot in 0u8..5 {
            let (px, py) = monster_page_origin(slot);
            assert!(px >= 320, "page x past the 240-wide CLUT region");
            assert_eq!(py, 256);
            assert!(MONSTER_CLUT_ROW_BASE + slot as u16 >= 484);
        }
    }
}
