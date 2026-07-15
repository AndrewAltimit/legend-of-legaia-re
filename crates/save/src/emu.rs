//! Emulator save-container normalization.
//!
//! Players export memory-card saves from their emulator in a handful of
//! containers. This module detects the container, exposes the SC save blocks
//! inside it (read and in-place write), and preserves the container verbatim
//! so a round-trip with no edits is **byte-identical** - the baseline contract
//! for "import your retail save, play, export it back".
//!
//! Accepted containers:
//!
//! - **Raw 128 KiB card image** (`.mcr` / `.mcd` / `.srm` / mednafen's card
//!   file): `MC` magic at offset 0. The canonical layout
//!   [`crate::card`] walks.
//! - **DexDrive** (`.gme`): `123-456-STD` ASCII header, raw card at `+0xF40`.
//! - **Single-save** (`.mcs`): one 128-byte directory frame followed by the
//!   save's 8 KiB block(s); block data begins at `+0x80` with the `SC` magic.
//!
//! Explicitly **rejected**: PS3 `.psv` exports (`\0VSP` magic). Those are
//! cryptographically signed; an in-place patch would produce a file the PS3
//! refuses, so offering it would be a trap. Convert to `.mcr`/`.mcs` first.
//!
//! There is no game-side checksum to fix after an SC-block edit: the retail
//! save payload is a plain memcpy of the RAM window
//! (`FUN_8001A8B0(SC_base=0x80084140, staging, 0x1A18)` - see
//! `docs/subsystems/save-screen.md`). The only checksum in a card image is
//! the per-directory-frame XOR (frame byte `0x7F`), which SC-block edits
//! never touch.

use anyhow::{Result, bail};

use crate::card::{self, BLOCK_SIZE, CARD_MAGIC, CARD_SIZE, DIR_FRAME_SIZE, SAVE_BLOCK_MAGIC};

/// DexDrive `.gme` header size - the raw card image follows it.
pub const DEXDRIVE_HEADER_SIZE: usize = 0xF40;
/// DexDrive `.gme` magic prefix.
pub const DEXDRIVE_MAGIC: &[u8] = b"123-456-STD";
/// PS3 `.psv` magic prefix (rejected - signed container).
pub const PSV_MAGIC: &[u8] = b"\0VSP";

/// Detected save-container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Raw 128 KiB memory-card image (`.mcr` / `.mcd` / `.srm`).
    RawCard,
    /// DexDrive `.gme` (raw card at [`DEXDRIVE_HEADER_SIZE`]).
    DexDrive,
    /// Single-save `.mcs` (directory frame + block data at `+0x80`).
    Mcs,
}

impl Format {
    /// Stable lowercase label (`"mcr"`, `"gme"`, `"mcs"`) for UI/JSON use.
    pub fn label(self) -> &'static str {
        match self {
            Format::RawCard => "mcr",
            Format::DexDrive => "gme",
            Format::Mcs => "mcs",
        }
    }
}

/// One save visible in the container.
#[derive(Debug, Clone)]
pub struct SaveRef {
    /// Block index the save starts at (1..=15; `1` for `.mcs`).
    pub block: u8,
    /// Product/region code from the directory frame (e.g. `BASCUS-94254...`).
    pub product_code: String,
    /// `true` when the block bytes start with the `SC` save magic.
    pub has_sc_magic: bool,
}

/// A detected container view over caller-owned bytes.
#[derive(Debug, Clone, Copy)]
pub struct CardView {
    /// The detected container format.
    pub format: Format,
    /// Byte offset of the raw card image inside the container
    /// ([`Format::RawCard`] = 0, [`Format::DexDrive`] = `0xF40`);
    /// unused for [`Format::Mcs`].
    card_base: usize,
}

/// Detect the container format of `bytes`. Errors on unknown containers and
/// on PS3 `.psv` exports (signed - see the module docs).
pub fn detect(bytes: &[u8]) -> Result<CardView> {
    if bytes.starts_with(PSV_MAGIC) {
        bail!(
            ".psv (PS3 export) saves are cryptographically signed - an edited copy would be \
             rejected by the PS3. Export a raw .mcr/.mcs from your emulator instead."
        );
    }
    if bytes.len() >= CARD_SIZE && bytes[..2] == CARD_MAGIC {
        return Ok(CardView {
            format: Format::RawCard,
            card_base: 0,
        });
    }
    if bytes.starts_with(DEXDRIVE_MAGIC) && bytes.len() >= DEXDRIVE_HEADER_SIZE + CARD_SIZE {
        return Ok(CardView {
            format: Format::DexDrive,
            card_base: DEXDRIVE_HEADER_SIZE,
        });
    }
    // `.mcs`: 128-byte directory frame + N × 8 KiB blocks, SC magic at +0x80.
    if bytes.len() >= DIR_FRAME_SIZE + BLOCK_SIZE
        && (bytes.len() - DIR_FRAME_SIZE).is_multiple_of(BLOCK_SIZE)
        && bytes[DIR_FRAME_SIZE..DIR_FRAME_SIZE + 2] == SAVE_BLOCK_MAGIC
    {
        return Ok(CardView {
            format: Format::Mcs,
            card_base: 0,
        });
    }
    bail!(
        "unrecognised save container ({} bytes) - expected a raw .mcr/.mcd card image, a \
         DexDrive .gme, or a single-save .mcs",
        bytes.len()
    )
}

impl CardView {
    /// Enumerate the saves the container holds. For a raw card / DexDrive
    /// this walks the card directory; a `.mcs` holds exactly one save.
    pub fn saves(&self, bytes: &[u8]) -> Result<Vec<SaveRef>> {
        match self.format {
            Format::RawCard | Format::DexDrive => {
                let card = &bytes[self.card_base..];
                let saves = card::parse_card(card)?;
                Ok(saves
                    .iter()
                    .map(|s| SaveRef {
                        block: s.block,
                        product_code: s.product_code.clone(),
                        has_sc_magic: card::read_block(card, s.block)
                            .map(|b| b[..2] == SAVE_BLOCK_MAGIC)
                            .unwrap_or(false),
                    })
                    .collect())
            }
            Format::Mcs => {
                let frame = &bytes[..DIR_FRAME_SIZE];
                let product_code = frame[10..0x1E]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| {
                        if (0x20..=0x7E).contains(&c) {
                            c as char
                        } else {
                            '?'
                        }
                    })
                    .collect();
                Ok(vec![SaveRef {
                    block: 1,
                    product_code,
                    has_sc_magic: bytes[DIR_FRAME_SIZE..DIR_FRAME_SIZE + 2] == SAVE_BLOCK_MAGIC,
                }])
            }
        }
    }

    /// Byte range of save block `block`'s 8 KiB SC block inside the container.
    fn block_range(&self, len: usize, block: u8) -> Option<(usize, usize)> {
        match self.format {
            Format::RawCard | Format::DexDrive => {
                let i = block as usize;
                if i == 0 || i > card::DIR_FRAMES {
                    return None;
                }
                let start = self.card_base + BLOCK_SIZE * i;
                let end = start + BLOCK_SIZE;
                (end <= len).then_some((start, end))
            }
            Format::Mcs => {
                if block != 1 {
                    return None;
                }
                let start = DIR_FRAME_SIZE;
                let end = start + BLOCK_SIZE;
                (end <= len).then_some((start, end))
            }
        }
    }

    /// Borrow save block `block`'s 8 KiB SC block.
    pub fn sc_block<'a>(&self, bytes: &'a [u8], block: u8) -> Option<&'a [u8]> {
        let (s, e) = self.block_range(bytes.len(), block)?;
        Some(&bytes[s..e])
    }

    /// Mutably borrow save block `block`'s 8 KiB SC block for a targeted
    /// in-place patch. Everything outside the returned slice is preserved
    /// verbatim, so an untouched container round-trips byte-identical.
    pub fn sc_block_mut<'a>(&self, bytes: &'a mut [u8], block: u8) -> Option<&'a mut [u8]> {
        let (s, e) = self.block_range(bytes.len(), block)?;
        Some(&mut bytes[s..e])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_card() -> Vec<u8> {
        let mut buf = vec![0u8; CARD_SIZE];
        buf[..2].copy_from_slice(&CARD_MAGIC);
        // One active save in block 1.
        let f = DIR_FRAME_SIZE;
        buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
        buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        buf[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
        let b = BLOCK_SIZE;
        buf[b..b + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        buf
    }

    #[test]
    fn detects_raw_card_and_lists_saves() {
        let card = raw_card();
        let view = detect(&card).unwrap();
        assert_eq!(view.format, Format::RawCard);
        let saves = view.saves(&card).unwrap();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].block, 1);
        assert!(saves[0].has_sc_magic);
        assert!(saves[0].product_code.starts_with("BASCUS-94254"));
        assert_eq!(view.sc_block(&card, 1).unwrap().len(), BLOCK_SIZE);
    }

    #[test]
    fn detects_dexdrive_wrapper() {
        let mut gme = vec![0u8; DEXDRIVE_HEADER_SIZE];
        gme[..DEXDRIVE_MAGIC.len()].copy_from_slice(DEXDRIVE_MAGIC);
        gme.extend_from_slice(&raw_card());
        let view = detect(&gme).unwrap();
        assert_eq!(view.format, Format::DexDrive);
        let saves = view.saves(&gme).unwrap();
        assert_eq!(saves.len(), 1);
        let block = view.sc_block(&gme, 1).unwrap();
        assert_eq!(&block[..2], &SAVE_BLOCK_MAGIC);
    }

    #[test]
    fn detects_mcs_single_save() {
        let mut mcs = vec![0u8; DIR_FRAME_SIZE + BLOCK_SIZE];
        mcs[..4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
        mcs[10..22].copy_from_slice(b"BASCUS-94254");
        mcs[DIR_FRAME_SIZE..DIR_FRAME_SIZE + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        let view = detect(&mcs).unwrap();
        assert_eq!(view.format, Format::Mcs);
        let saves = view.saves(&mcs).unwrap();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].block, 1);
        assert!(saves[0].has_sc_magic);
        assert!(view.sc_block(&mcs, 2).is_none());
    }

    #[test]
    fn rejects_psv_and_garbage() {
        let mut psv = vec![0u8; 0x2000];
        psv[..4].copy_from_slice(PSV_MAGIC);
        let err = detect(&psv).unwrap_err().to_string();
        assert!(err.contains("signed"), "{err}");
        assert!(detect(&[0u8; 64]).is_err());
    }

    #[test]
    fn targeted_coin_patch_only_touches_four_bytes() {
        let mut card = raw_card();
        // Fill the SC block with a pattern so any stray write shows.
        for (i, b) in card[BLOCK_SIZE..2 * BLOCK_SIZE].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31) ^ 0x3C;
        }
        card[BLOCK_SIZE..BLOCK_SIZE + 2].copy_from_slice(&SAVE_BLOCK_MAGIC);
        let before = card.clone();
        let view = detect(&card).unwrap();
        let block = view.sc_block_mut(&mut card, 1).unwrap();
        crate::card::write_retail_coins(block, 12345).unwrap();
        assert_eq!(crate::card::read_retail_coins(block), Some(12345));
        let diff: Vec<usize> = before
            .iter()
            .zip(card.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert!(
            diff.iter()
                .all(|&i| (BLOCK_SIZE + crate::card::RETAIL_COINS_OFFSET
                    ..BLOCK_SIZE + crate::card::RETAIL_COINS_OFFSET + 4)
                    .contains(&i)),
            "only the 4 coin bytes may change: {diff:?}"
        );
    }
}
