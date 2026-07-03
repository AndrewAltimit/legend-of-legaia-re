//! Texture relocation + upload: per-slot runtime-band TSB/CBA rewrite and
//! the record[0] / equipment-section battle-texture upload blocks.

use anyhow::{Context, Result, bail};

use crate::battle_data_pack::{BattleDataPack, decode_record};

use super::assembly::select_sections;
use super::{SECTION_COUNT, read_u32};

/// VRAM CLUT row of party slot 0's runtime palette (rows `481 + slot`,
/// i.e. 481/482/483 for Vahn/Noa/Gala).
pub const RUNTIME_CLUT_ROW_BASE: u16 = 0x1E1;

/// 5-bit TSB texpage index of party slot 0's first runtime page
/// (`0x18` = VRAM `(512, 256)`); slot `s` uses pages `0x18 + 2s` /
/// `0x19 + 2s`, packing the party band into `x ∈ [512, 896), y = 256`.
pub const RUNTIME_TEXPAGE_BASE: u16 = 0x18;

/// The authoring texpage index every player-file section meshes at; the
/// relocation maps it to the slot's **first** runtime page and every other
/// authoring page to the **second** (the player files author exactly two
/// pages, `0x15`/`0x16`, so this is a faithful per-page remap: `+3` on the
/// texpage index for slot 0).
pub const AUTHORING_FIRST_TEXPAGE: u16 = 0x15;

/// Relocate an assembled battle TMD's texture addressing into party slot
/// `slot`'s runtime VRAM band, in place. Retail runs this pass at battle
/// registration, right after installing the blob into
/// `DAT_8007C018[slot]`; the on-disc (authoring) TSB/CBA - texpages
/// `0x15`/`0x16` = `(320, 256)`/`(384, 256)`, CLUT row 480 - are never
/// sampled by a normal battle. Per **textured** primitive (group-header
/// mode byte TME bit set):
///
/// - **CBA**: CLUT row (bits 6..14) ← `481 + slot`; the column
///   (`(cba & 0x3F) * 16`) and the high bit are preserved. For the
///   authoring row 480 this is the `+0x40` CLUT-id rewrite the live
///   runtime blob exhibits.
/// - **TSB**: texpage index (bits 0..4) ← `0x18 + 2*slot` when the
///   authoring page is `0x15`, else `0x19 + 2*slot`; ABR / colour-depth
///   bits are preserved. For the authoring pages `0x15`/`0x16` this is
///   the `+3` texpage rewrite (slot 0).
///
/// Untextured groups (`F*`/`G*`) carry no texture block and are left
/// untouched. Returns the number of primitives rewritten.
// PORT: FUN_80053a28 - the per-slot TSB/CBA relocation loop (walks each
// object's primitive groups, gated on the group mode byte's TME bit;
// CBA word & 0x803fffff | (0x1e1+slot)<<22, TSB word & 0xffe0ffff |
// (slot*2 + (page==0x15 ? 0x18 : 0x19))<<16).
// REF: FUN_800513F0 - the battle scene-loader state that calls it per
// party slot right after registering the assembled blob.
pub fn relocate_tsb_cba(tmd_bytes: &mut [u8], slot: u8) -> Result<usize> {
    let tmd = legaia_tmd::parse(tmd_bytes).context("parse assembled TMD for relocation")?;
    // Collect each textured prim's texture-block start first (immutable
    // walk), then rewrite. The block is `[u0, v0, cba, u1, v1, tsb, ...]`
    // ending at the descriptor's vertex-index offset.
    let mut blocks: Vec<usize> = Vec::new();
    for obj in &tmd.objects {
        let groups = legaia_tmd::legaia_prims::iter_groups(
            tmd_bytes,
            obj.primitives_byte_offset,
            obj.primitives_byte_size,
        )
        .context("walk primitive groups for relocation")?;
        for group in groups {
            // Retail gates on the group mode byte's TME bit (mode & 4).
            if group.header.mode & 0x04 == 0 {
                continue;
            }
            let Some(vert_off) = legaia_tmd::legaia_prims::vertex_offset_bytes(group.header.flags)
            else {
                continue;
            };
            let block_len = 4 + group.header.n_vertices() * 2;
            if vert_off < block_len {
                continue;
            }
            for prim in &group.prims {
                blocks.push(prim.bytes_offset + vert_off - block_len);
            }
        }
    }
    for &bs in &blocks {
        let Some(block) = tmd_bytes.get_mut(bs..bs + 8) else {
            bail!("texture block at {bs:#x} past TMD end");
        };
        let cba = u16::from_le_bytes([block[2], block[3]]);
        let new_cba = (cba & 0x803F) | ((RUNTIME_CLUT_ROW_BASE + slot as u16) << 6);
        block[2..4].copy_from_slice(&new_cba.to_le_bytes());
        let tsb = u16::from_le_bytes([block[6], block[7]]);
        let page = if tsb & 0x1F == AUTHORING_FIRST_TEXPAGE {
            RUNTIME_TEXPAGE_BASE + slot as u16 * 2
        } else {
            RUNTIME_TEXPAGE_BASE + 1 + slot as u16 * 2
        };
        let new_tsb = (tsb & 0xFFE0) | page;
        block[6..8].copy_from_slice(&new_tsb.to_le_bytes());
    }
    Ok(blocks.len())
}

/// VRAM x base (in halfwords / 16bpp pixels) of party slot 0's battle
/// texture band; slot `s` starts at `BAND_X_BASE + s * BAND_X_STRIDE`.
pub const BAND_X_BASE: u16 = 0x200;

/// Per-party-slot x stride of the battle texture band (= two 64-halfword
/// texpages, the pages `relocate_tsb_cba` retargets).
pub const BAND_X_STRIDE: u16 = 0x80;

/// VRAM y base every battle-character image rect offsets from.
pub const BAND_Y_BASE: u16 = 0x100;

/// One image rect of the battle-texture placement, in the pre-band frame
/// the retail tables author: the upload lands at
/// `(BAND_X_BASE + party_slot * BAND_X_STRIDE + x0, BAND_Y_BASE + y0)`.
/// `w` is in VRAM halfwords (32 halfwords = 128 px at 4bpp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureRect {
    /// Band-relative x (halfwords).
    pub x0: u16,
    /// Band-relative y (rows).
    pub y0: u16,
    /// Width in halfwords.
    pub w: u16,
    /// Height in rows.
    pub h: u16,
}

impl TextureRect {
    /// Absolute VRAM x of the upload for party slot `party_slot`.
    pub fn fb_x(&self, party_slot: u8) -> u16 {
        self.x0 + BAND_X_BASE + party_slot as u16 * BAND_X_STRIDE
    }

    /// Absolute VRAM y of the upload.
    pub fn fb_y(&self) -> u16 {
        self.y0 + BAND_Y_BASE
    }

    /// Byte size of the rect's pixel payload (`w * h` halfwords).
    pub fn pixel_bytes(&self) -> usize {
        self.w as usize * self.h as usize * 2
    }
}

/// Per-section texture-pool placement rects - mirror of the static
/// `SCUS_942.54` table at `0x800775B8` (4 u16 per equip section, read by
/// `FUN_80052FA0`'s per-section decode loop and handed to `FUN_80053B9C`).
/// Together with [`RECORD0_TEXTURE_RECTS`] the seven rects tile each party
/// slot's 128-halfword x 256-row band exactly.
pub const SECTION_TEXTURE_RECTS: [TextureRect; SECTION_COUNT] = [
    TextureRect {
        x0: 0x00,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x00,
        y0: 0x00,
        w: 0x40,
        h: 0x80,
    },
    TextureRect {
        x0: 0x40,
        y0: 0x00,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x40,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x60,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
];

/// Placement rects of the two `record[0]` image blocks (the blocks at the
/// file header's `clut_a_off` / `clut_b_off` within record[0]'s decoded
/// output). Inline constants of `FUN_80052FA0` (`0x800020` / `0x60` +
/// `0x800020` packed `(x0,y0)` / `(w,h)` pairs).
pub const RECORD0_TEXTURE_RECTS: [TextureRect; 2] = [
    TextureRect {
        x0: 0x20,
        y0: 0x80,
        w: 0x20,
        h: 0x80,
    },
    TextureRect {
        x0: 0x60,
        y0: 0x00,
        w: 0x20,
        h: 0x80,
    },
];

/// One decoded battle-texture upload block in the `FUN_80053B9C` frame:
/// `[u16 clut_x][u16 clut_n][clut_n x u16 BGR555][w*h halfwords pixels]`.
/// The CLUT half LoadImages to `(clut_x, 0x1E1 + party_slot, clut_n, 1)`
/// with the STP bit forced on every non-zero entry; the pixel half
/// LoadImages to the rect's banded `(fb_x, fb_y, w, h)`.
#[derive(Debug, Clone)]
pub struct TextureUpload {
    /// Placement rect (pre-band frame; see [`TextureRect`]).
    pub rect: TextureRect,
    /// Party slot the block was decoded for (0..=2).
    pub party_slot: u8,
    /// VRAM x (halfwords) of the CLUT run on row `0x1E1 + party_slot`.
    pub clut_x: u16,
    /// CLUT entries with the retail STP pass applied (`e |= 0x8000` on
    /// every non-zero entry). Empty in both `record[0]` blocks.
    pub clut: Vec<u16>,
    /// Pixel payload (`rect.w * rect.h` halfwords, row-major).
    pub pixels: Vec<u8>,
}

impl TextureUpload {
    /// Absolute VRAM x of the pixel upload.
    pub fn fb_x(&self) -> u16 {
        self.rect.fb_x(self.party_slot)
    }

    /// Absolute VRAM y of the pixel upload.
    pub fn fb_y(&self) -> u16 {
        self.rect.fb_y()
    }

    /// VRAM row of the CLUT run (`0x1E1 + party_slot` - the same rows the
    /// `relocate_tsb_cba` CBA rewrite targets).
    pub fn clut_row(&self) -> u16 {
        RUNTIME_CLUT_ROW_BASE + self.party_slot as u16
    }

    /// CLUT entries as little-endian bytes (ready for a VRAM row write).
    pub fn clut_bytes(&self) -> Vec<u8> {
        self.clut.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

/// Decode one battle-texture upload block (the `FUN_80053B9C` source
/// frame) out of `block`, placing it at `rect` for `party_slot`.
// PORT: FUN_80053b9c - the battle-texture upload helper: reads the
// [clut_x, clut_n, entries] prefix, ORs STP onto non-zero entries,
// LoadImages the CLUT run to (clut_x, 0x1E1+slot, clut_n, 1) and the
// pixels to (x0 + 0x200 + slot*0x80, y0 + 0x100, w, h). (The shadow CLUT
// copy into the battle context at +0x894 is engine-context, not VRAM.)
// REF: FUN_800583c8 - the LoadImage wrapper it issues both rects through.
pub fn parse_upload_block(
    block: &[u8],
    rect: TextureRect,
    party_slot: u8,
) -> Result<TextureUpload> {
    if party_slot > 2 {
        bail!("party slot {party_slot} out of the 0..=2 battle band");
    }
    let clut_x = u16::from_le_bytes(
        block
            .get(0..2)
            .ok_or_else(|| anyhow::anyhow!("upload block shorter than its clut_x word"))?
            .try_into()
            .unwrap(),
    );
    let clut_n = u16::from_le_bytes(
        block
            .get(2..4)
            .ok_or_else(|| anyhow::anyhow!("upload block shorter than its clut_n word"))?
            .try_into()
            .unwrap(),
    ) as usize;
    if clut_n > 0x400 {
        bail!("implausible CLUT run length {clut_n}");
    }
    let clut_bytes = block
        .get(4..4 + clut_n * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT run past block end"))?;
    let clut = clut_bytes
        .chunks_exact(2)
        .map(|c| {
            let e = u16::from_le_bytes([c[0], c[1]]);
            if e != 0 { e | 0x8000 } else { e }
        })
        .collect();
    let pix_off = 4 + clut_n * 2;
    let pixels = block
        .get(pix_off..pix_off + rect.pixel_bytes())
        .ok_or_else(|| anyhow::anyhow!("pixel payload past block end"))?
        .to_vec();
    Ok(TextureUpload {
        rect,
        party_slot,
        clut_x,
        clut,
        pixels,
    })
}

/// The two `record[0]` texture uploads of a player file: LZS-decode
/// `record[0]` (header `budget` at `+0x0C`, stream at `+0x10`) and frame
/// the blocks at the header's `clut_a_off` / `clut_b_off` with the
/// [`RECORD0_TEXTURE_RECTS`] placement.
// PORT: FUN_80052FA0 (record[0] texture half) - the two FUN_80053b9c
// calls right after the record[0] decode, before the per-section loop.
pub fn record0_texture_uploads(file: &[u8], party_slot: u8) -> Result<Vec<TextureUpload>> {
    let clut_a = read_u32(file, 4)? as usize;
    let clut_b = read_u32(file, 8)? as usize;
    let budget = read_u32(file, 0xC)? as usize;
    if budget == 0 || budget > 0x40_0000 || clut_a >= clut_b || clut_b >= budget {
        bail!(
            "implausible player-file header (clut_a {clut_a:#x} clut_b {clut_b:#x} budget {budget:#x})"
        );
    }
    let stream = file
        .get(0x10..)
        .ok_or_else(|| anyhow::anyhow!("file shorter than its record[0] stream"))?;
    let decoded = legaia_lzs::decompress(stream, budget)?;
    let mut out = Vec::with_capacity(2);
    for (off, rect) in [
        (clut_a, RECORD0_TEXTURE_RECTS[0]),
        (clut_b, RECORD0_TEXTURE_RECTS[1]),
    ] {
        let block = decoded
            .get(off..)
            .ok_or_else(|| anyhow::anyhow!("record[0] block at {off:#x} past decoded end"))?;
        out.push(
            parse_upload_block(block, rect, party_slot)
                .with_context(|| format!("record[0] block at {off:#x}"))?,
        );
    }
    Ok(out)
}

/// The texture upload of one decoded equipment section, when the section
/// is flagged for upload (`u16` at `+0x12` non-zero): the block at
/// `decoded + tmd_body_end` placed at [`SECTION_TEXTURE_RECTS`]`[section]`.
/// `Ok(None)` for unflagged sections (their pool bytes are dead - retail
/// overwrites them with the next section's decode without uploading).
// PORT: FUN_80052FA0 (per-section texture half) - the `lh 0x12(s2)` gate
// + the FUN_80053b9c call at decoded+tmd_body_end with the DAT_800775b8
// per-section rect.
pub fn section_texture_upload(
    decoded: &[u8],
    section: usize,
    party_slot: u8,
) -> Result<Option<TextureUpload>> {
    if section >= SECTION_COUNT {
        bail!("section index {section} out of the 5-slot table");
    }
    let flag = u16::from_le_bytes(
        decoded
            .get(0x12..0x14)
            .ok_or_else(|| anyhow::anyhow!("decoded section shorter than its header"))?
            .try_into()
            .unwrap(),
    );
    if flag == 0 {
        return Ok(None);
    }
    let pool = read_u32(decoded, 0xC)? as usize;
    let block = decoded
        .get(pool..)
        .ok_or_else(|| anyhow::anyhow!("texture pool at {pool:#x} past decoded end"))?;
    Ok(Some(
        parse_upload_block(block, SECTION_TEXTURE_RECTS[section], party_slot)
            .with_context(|| format!("section {section} pool at {pool:#x}"))?,
    ))
}

/// Every battle-texture upload of one character, in retail order: the two
/// `record[0]` blocks, then the five equipped sections' flagged pools.
/// `equipped` is the char record's `+0x196..+0x19A` bytes; `party_slot`
/// is the character's 0-based ordinal among the *present* battle party
/// (the band selector - not the character id).
pub fn character_texture_uploads(
    file: &[u8],
    pack: &BattleDataPack,
    equipped: &[u8; SECTION_COUNT],
    party_slot: u8,
) -> Result<Vec<TextureUpload>> {
    let mut out = record0_texture_uploads(file, party_slot)?;
    let records = select_sections(pack, equipped)?;
    for (i, rec) in records.iter().enumerate() {
        let entry = decode_record(file, pack, rec.index)
            .with_context(|| format!("decode section {i} (id {:#x})", rec.id))?;
        if let Some(upload) = section_texture_upload(&entry.bytes, i, party_slot)
            .with_context(|| format!("section {i} (id {:#x})", rec.id))?
        {
            out.push(upload);
        }
    }
    Ok(out)
}
