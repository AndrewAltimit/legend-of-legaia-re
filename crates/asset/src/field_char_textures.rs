//! Field-character texture pack — PROT 0874 **section 2** (the third LZS
//! descriptor of the `player.lzs` container, the "etim.dat" texture section).
//!
//! The field-form player meshes ([`crate::character_pack`], PROT 0874 §0)
//! reference a 4bpp texture page at PSX texpage `(832, 256)` (`tsb 0x3D`) with
//! per-character CLUTs packed into VRAM **row 478**. Those textures are *not*
//! in PROT 0876 (`player_data` — that is VAB + an empty TIM_LIST + SEQ); they
//! live LZS-compressed in PROT 0874 §2 and are uploaded to VRAM at field-init.
//!
//! ## Loader provenance
//!
//! `FUN_8001E890` (the field player loader) loads `data\field\player.lzs`
//! (disc index `0x36c`) — the same 3-descriptor [`crate::parse_player_lzs`]
//! container the per-entry extractor labels PROT 0874. It LZS-decodes all three
//! sections (`piVar2[2..7]`):
//!
//! - §0 → the 5-TMD character mesh pack (`DAT_8007C018[0..4]`) — [`crate::character_pack`].
//! - §1 → effect / vdf models (`DAT_8007b75c`).
//! - **§2 → a [`crate::pack`] of asset chunks, each uploaded to VRAM via
//!   `FUN_800198e0`.** This module decodes that section.
//!
//! ## Upload semantic (`FUN_800198e0`)
//!
//! Each pack entry is a standard PSX TIM (`magic 0x10`, `flags & 8` = has CLUT,
//! 4bpp). `FUN_800198e0` uploads it with one **non-standard** detail: the CLUT
//! block is written as a **flat horizontal strip** — `LoadImage(rect = { x =
//! clut_x, y = clut_y, w = clut_w * clut_h, h = 1 })` — rather than the declared
//! `clut_w × clut_h` rectangle. So a TIM whose CLUT header says `(0, 478, 16, 4)`
//! places 64 colours at VRAM row 478, columns 0..63 (four 16-colour palettes
//! side by side), which is exactly where the meshes' per-primitive CBA columns
//! sample. STP (`| 0x8000` on non-zero colours) is applied only when
//! `_DAT_8007b998 != 0`; for the field upload that flag is **0**, so field CLUTs
//! are stored bit-15-clear (unlike the row-479 NPC CLUTs, which are STP-set by a
//! separate upload). See [`upload_to_vram`].
//!
//! The eight entries (byte-exact vs a live field VRAM dump):
//!
//! ```text
//! entry | image (x, y, w_words, h) | CLUT (x, y, w*h colours) | role
//! ------|--------------------------|--------------------------|---------------------------
//!   0   | (448,   0, 64, 256)      | (0, 473, 256)            | shared 256-colour page
//!   1   | (832, 256, 20, 128)      | (0,   478, 64)           | Vahn atlas + palettes 0..63
//!   2   | (852, 256, 20, 128)      | (64,  478, 64)           | Noa  atlas + palettes 64..127
//!   3   | (872, 256, 20, 128)      | (128, 478, 64)           | Gala atlas + palettes 128..191
//!   4   | (320, 256, 64, 256)      | (0, 475, 256)            | shared 256-colour page
//!   5   | (384, 256, 64, 256)      | (0, 475, 256)            | shared 256-colour page
//!   6   | (880, 384, 16, 64)       | (192, 478, 32)           | atlas extension (lower)
//!   7   | (880, 448, 16, 64)       | (224, 478, 32)           | atlas extension (lower)
//! ```
//!
//! Entries 1/2/3 tile horizontally (832 + 20 + 20 = 872) to fill the 4bpp page
//! `(832, 256)` the field meshes sample; their CLUTs occupy row 478 columns
//! 0..191 (Vahn 0..63, Noa 64..127, Gala 128..191).

use anyhow::{Context, Result, bail};
use legaia_tim::{Tim, Vram, parse as parse_tim};

use crate::{DecodeMode, decode, parse_player_lzs};

/// PROT entry index that carries the player `player.lzs` container.
pub const PROT_ENTRY_INDEX: u32 = 874;

/// Index of the LZS-compressed section that carries the field texture pack
/// (the third descriptor of the 3-descriptor `player.lzs` container).
pub const CONTAINER_SECTION: usize = 2;

/// Number of descriptors the PROT 0874 `player.lzs` container header carries.
pub const CONTAINER_DESCRIPTORS: usize = 3;

/// One uploaded texture entry: a parsed PSX TIM plus its 0-based index in the
/// pack. The TIM's `image` block is uploaded verbatim at its declared
/// `(fb_x, fb_y)`; the `clut` block is uploaded as a **flat strip** (see the
/// module docs and [`upload_to_vram`]).
#[derive(Debug, Clone)]
pub struct FieldTexture {
    /// 0-based entry index in the PROT 0874 §2 pack.
    pub index: usize,
    /// The parsed TIM (all field entries are 4bpp with a CLUT).
    pub tim: Tim,
}

/// The decoded field-character texture pack (PROT 0874 §2).
#[derive(Debug, Clone)]
pub struct FieldCharTextures {
    /// The pack's TIMs in disc order. Entries 1/2/3 are the Vahn/Noa/Gala
    /// character atlas pages; the rest are shared / auxiliary pages.
    pub textures: Vec<FieldTexture>,
}

impl FieldCharTextures {
    /// Upload every entry into `vram`, replicating `FUN_800198e0`:
    /// the image block goes to its declared rect; the CLUT block goes to a
    /// **flat strip** `(clut.fb_x, clut.fb_y, clut.w * clut.h, 1)`. When
    /// `stp` is set, non-zero CLUT colours get bit 15 OR-ed in (the retail
    /// field upload runs with `stp = false`).
    pub fn upload_to_vram(&self, vram: &mut Vram, stp: bool) {
        for t in &self.textures {
            // Image: declared rect, verbatim.
            vram.upload_tim_partial(&t.tim, true, false);
            // CLUT: flat horizontal strip of `w * h` colours at (fb_x, fb_y).
            if let Some(clut) = t.tim.clut.as_ref() {
                let mut bytes = Vec::with_capacity(clut.entries.len() * 2);
                for &c in &clut.entries {
                    let v = if stp && c != 0 { c | 0x8000 } else { c };
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                vram.write_clut_row(clut.fb_x, clut.fb_y, &bytes);
            }
        }
    }
}

/// Parse the field-character texture pack from the raw bytes of PROT entry 874.
///
/// Mirrors the `FUN_8001E890` chain for section 2:
/// `parse_player_lzs(buf, 3)` → §2 descriptor → LZS-decompress →
/// [`crate::pack::extract_pack`] → N TIMs.
pub fn parse(prot_0874_bytes: &[u8]) -> Result<FieldCharTextures> {
    let container = parse_player_lzs(prot_0874_bytes, CONTAINER_DESCRIPTORS)
        .context("parse PROT 0874 as a 3-descriptor player.lzs-shaped container")?;
    let section = container
        .descriptors
        .get(CONTAINER_SECTION)
        .ok_or_else(|| anyhow::anyhow!("PROT 0874 container has no section {CONTAINER_SECTION}"))?;
    let decoded = decode(prot_0874_bytes, section, DecodeMode::Lzs)
        .context("LZS-decode PROT 0874 section 2 (field texture pack)")?;
    let bodies =
        crate::pack::extract_pack(&decoded).context("walk PROT 0874 section 2 as an asset pack")?;
    if bodies.is_empty() {
        bail!("PROT 0874 section 2 pack is empty");
    }
    let mut textures = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.into_iter().enumerate() {
        let tim = parse_tim(body)
            .with_context(|| format!("parse PROT 0874 §2 pack entry {index} as a TIM"))?;
        textures.push(FieldTexture { index, tim });
    }
    Ok(FieldCharTextures { textures })
}
