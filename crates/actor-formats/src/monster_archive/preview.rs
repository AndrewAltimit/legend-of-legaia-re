//! A monster's texture as one self-contained atlas, for previews and exports
//! that have no VRAM.
//!
//! The battle path needs none of this: [`super::MonsterMesh::battle_render_mesh`]
//! uploads the pool and relocates every prim's CBA / TSB, and
//! [`super::write_boss_effect_page`] stages the boss page, so the VRAM decode
//! resolves each prim on its own. A consumer that draws from the decoded
//! [`MonsterTexture`] alone (the site bestiary, the `.glb` export) has to make
//! the same two choices per prim that the VRAM address makes:
//!
//! - **which page** - the TSB column: on-disc `5` is the monster's own pool,
//!   `6` the boss effect page;
//! - **which palette** - the CBA row: on-disc `481` is the pool's 15 CLUTs,
//!   `482` the boss page's 256-entry row (16 CLUTs of 16).
//!
//! Only the ids `0xB3..=0xBA` carry column-6 / row-482 prims (Songi's third
//! form, both Cort forms, Koru and the four Cort copies); every other monster
//! maps through unchanged. Reading `cba & 0x3F` against the pool
//! alone - what the preview did - draws a boss prim's page-6 UVs over the
//! monster's own texels through one of its own palettes.
//!
//! The atlas puts the boss page to the right of the pool page
//! (`u + pool_width`) and its 16 palettes after the pool's 15 (`15 + p`).

use super::{
    CLUT_COUNT, MonsterMesh, MonsterTexture, TEXTURE_HEIGHT, bgr555_to_rgba, boss_effect_page,
};

/// On-disc CBA row of the monster's own palettes (before the per-slot
/// relocation).
pub const POOL_CLUT_ROW: u16 = 481;
/// On-disc CBA row a boss prim names for the boss effect page's palettes.
pub const BOSS_CLUT_ROW: u16 = 482;
/// On-disc TSB texture-page column of the monster's own pool.
pub const POOL_PAGE_COLUMN: u16 = 5;
/// On-disc TSB texture-page column a boss prim names for the boss page.
pub const BOSS_PAGE_COLUMN: u16 = 6;
/// Width of the boss effect page in 4bpp texels (`0x40` halfwords).
const BOSS_PAGE_TEXELS: usize = 0x40 * 4;
/// 16-colour palettes in the boss page's 256-entry CLUT row.
const BOSS_PALETTES: usize = 16;

/// A monster mesh with every vertex resolved against one texture atlas.
#[derive(Debug, Clone)]
pub struct MonsterPreview {
    /// The unrelocated mesh (on-disc CBA / TSB, page-local UVs).
    pub mesh: legaia_tmd::mesh::VramMesh,
    /// The atlas: the pool page, then the boss page when any prim samples it;
    /// the pool's palettes, then the boss page's. `None` without a pool.
    pub texture: Option<MonsterTexture>,
    /// Per-vertex texel coordinates into [`Self::texture`].
    pub atlas_uvs: Vec<[u16; 2]>,
    /// Per-vertex palette index into [`MonsterTexture::palettes`].
    pub atlas_palettes: Vec<u8>,
}

/// Whether an on-disc `[cba, tsb]` pair samples the boss effect page:
/// `(page, palette)`.
fn samples_boss(cba_tsb: [u16; 2]) -> (bool, bool) {
    let row = (cba_tsb[0] >> 6) & 0x1FF;
    let column = cba_tsb[1] & 0xF;
    (column == BOSS_PAGE_COLUMN, row == BOSS_CLUT_ROW)
}

impl MonsterMesh {
    /// Build the [`MonsterPreview`] of this monster. `archive` is the whole
    /// monster archive (PROT 867), which holds the boss page eight slots past
    /// the monster ([`boss_effect_page`]). `None` when the TMD does not parse.
    pub fn preview(&self, archive: &[u8]) -> Option<MonsterPreview> {
        let tmd = legaia_tmd::parse(self.tmd_bytes()).ok()?;
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, self.tmd_bytes());
        let mut texture = self.texture();
        let pool_width = texture.as_ref().map_or(0, |t| t.width);

        let wants_boss = mesh.cba_tsb.iter().any(|&ct| {
            let (page, pal) = samples_boss(ct);
            page || pal
        });
        let mut boss_staged = false;
        if wants_boss
            && let Some(tex) = texture.as_mut()
            && let Some((clut, pixels)) = boss_effect_page(archive, self.id)
        {
            append_boss_page(tex, &clut, pixels);
            boss_staged = true;
        }

        let palette_count = texture.as_ref().map_or(CLUT_COUNT, |t| t.palettes.len());
        let mut atlas_uvs = Vec::with_capacity(mesh.uvs.len());
        let mut atlas_palettes = Vec::with_capacity(mesh.uvs.len());
        for (i, uv) in mesh.uvs.iter().enumerate() {
            let ct = mesh.cba_tsb.get(i).copied().unwrap_or([0, 0]);
            let (page, pal) = if boss_staged {
                samples_boss(ct)
            } else {
                (false, false)
            };
            let u = u16::from(uv[0]) + if page { pool_width as u16 } else { 0 };
            atlas_uvs.push([u, u16::from(uv[1])]);
            let p = usize::from(ct[0] & 0x3F);
            let p = if pal {
                CLUT_COUNT + p.min(BOSS_PALETTES - 1)
            } else {
                p.min(CLUT_COUNT - 1)
            };
            atlas_palettes.push(p.min(palette_count.saturating_sub(1)) as u8);
        }
        Some(MonsterPreview {
            mesh,
            texture,
            atlas_uvs,
            atlas_palettes,
        })
    }
}

/// Widen `tex` by the boss page (4bpp, `0x40` halfwords x 256 rows) and append
/// its 16 palettes. `clut` is the 256-entry row as the loader uploads it.
fn append_boss_page(tex: &mut MonsterTexture, clut: &[u8], pixels: &[u8]) {
    let old_w = tex.width;
    let new_w = old_w + BOSS_PAGE_TEXELS;
    let mut indices = vec![0u8; new_w * TEXTURE_HEIGHT];
    let row_bytes = BOSS_PAGE_TEXELS / 2;
    for y in 0..TEXTURE_HEIGHT {
        indices[y * new_w..y * new_w + old_w]
            .copy_from_slice(&tex.indices[y * old_w..(y + 1) * old_w]);
        for xb in 0..row_bytes {
            let b = pixels.get(y * row_bytes + xb).copied().unwrap_or(0);
            indices[y * new_w + old_w + xb * 2] = b & 0x0F;
            indices[y * new_w + old_w + xb * 2 + 1] = b >> 4;
        }
    }
    tex.indices = indices;
    tex.width = new_w;
    // Pad the pool's palettes to the full 15 so boss palette `p` sits at
    // `15 + p` whatever the pool held.
    tex.palettes.resize(CLUT_COUNT, [[0u8; 4]; 16]);
    for p in 0..BOSS_PALETTES {
        let mut pal = [[0u8; 4]; 16];
        for (c, slot) in pal.iter_mut().enumerate() {
            let raw = legaia_bytes::u16_le(clut, (p * 16 + c) * 2).unwrap_or(0);
            *slot = bgr555_to_rgba(raw);
        }
        tex.palettes.push(pal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_column_6_and_row_482_name_the_boss_page() {
        assert_eq!(samples_boss([(481 << 6) | 3, 0x05]), (false, false));
        assert_eq!(samples_boss([(482 << 6) | 3, 0x06]), (true, true));
        assert_eq!(samples_boss([(482 << 6) | 3, 0x06 | 0x8000]), (true, true));
        assert_eq!(samples_boss([481 << 6, 0x06 | (1 << 5)]), (true, false));
    }

    #[test]
    fn the_boss_page_lands_right_of_the_pool_with_its_palettes_after() {
        let mut tex = MonsterTexture {
            palettes: vec![[[1, 1, 1, 255]; 16]; 3],
            indices: vec![7u8; 128 * TEXTURE_HEIGHT],
            width: 128,
            height: TEXTURE_HEIGHT,
        };
        let mut clut = vec![0u8; 0x200];
        clut[(16 + 1) * 2..(16 + 1) * 2 + 2].copy_from_slice(&0x801Fu16.to_le_bytes());
        let mut pixels = vec![0u8; 0x80 * TEXTURE_HEIGHT];
        pixels[0] = 0x21;
        append_boss_page(&mut tex, &clut, &pixels);
        assert_eq!(tex.width, 128 + 256);
        assert_eq!(tex.indices[127], 7, "pool texels kept");
        assert_eq!(
            &tex.indices[128..130],
            &[1, 2],
            "boss texels low nibble first"
        );
        assert_eq!(tex.palettes.len(), CLUT_COUNT + BOSS_PALETTES);
        assert_eq!(tex.palettes[CLUT_COUNT + 1][1], [0xF8, 0, 0, 255]);
        assert_eq!(
            tex.palettes[CLUT_COUNT + 1][0][3],
            0,
            "zero entry transparent"
        );
    }
}
