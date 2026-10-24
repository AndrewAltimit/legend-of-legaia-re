//! What makes an SC block read as a Legaia save in the BIOS card manager:
//! the directory filename, the Shift-JIS title with its slot digits and the
//! per-slot portrait icon.

use super::*;
use anyhow::{Result, bail};

/// Offset of the Shift-JIS save title inside an SC block.
pub const RETAIL_TITLE_OFFSET: usize = 0x04;

/// Offsets **within the title** of the two save-number digits' low bytes.
/// The digits are full-width, so only the low byte of each 2-byte character
/// varies.
///
/// PORT: FUN_801e1934 (`0x801E19E4..0x801E19E8`)
pub const RETAIL_TITLE_DIGIT_OFFSETS: [usize; 2] = [0x23, 0x25];

/// Offsets **within the title** of the play-time digits' low bytes: hours
/// tens / units, then minutes tens / units, of `Time HH:MM` - the title's
/// second half. Retail's titles carry the block's own play clock there
/// (hours `clock / 216000`, minutes `clock / 3600 % 60`), read off real
/// cards; the menu overlay keeps the template live at `0x801E4FC8` and the
/// composer copies `0x40` bytes of it (`FUN_8003D26C` with `a2 = 2`).
pub const RETAIL_TITLE_TIME_DIGIT_OFFSETS: [usize; 4] = [0x35, 0x37, 0x3B, 0x3D];

/// Bytes of the title template the composer copies into the block
/// (`FUN_8003D26C(0x80084144, 0x801E4FC8, 2)`: two 32-byte strides).
pub const RETAIL_TITLE_TEMPLATE_LEN: usize = 0x40;

/// Low byte of the Shift-JIS full-width digit zero. A title digit is
/// `SAVE_TITLE_DIGIT_BASE + digit`, which is what makes the BIOS render it
/// as a full-width numeral.
pub const SAVE_TITLE_DIGIT_BASE: u8 = 0x4F;

/// Offset of the 16-entry icon palette (32 bytes, u16 LE BGR555).
pub const RETAIL_ICON_CLUT_OFFSET: usize = 0x60;

/// Offsets of the three 128-byte icon frame slots.
///
/// Retail writes the **same** tile into all three even though the header
/// declares a single frame - three `StoreImage` calls on one rect.
///
/// PORT: FUN_801e1934 (`0x801E1B30..0x801E1B68`)
pub const RETAIL_ICON_FRAME_OFFSETS: [usize; 3] = [0x80, 0x100, 0x180];

/// Bytes in one icon frame (16x16 @ 4bpp).
pub const RETAIL_ICON_FRAME_BYTES: usize = 128;

/// Bytes in the icon palette.
pub const RETAIL_ICON_CLUT_BYTES: usize = 32;

/// Filename prefix retail gives a Legaia save on a USA disc. The **slot
/// number is the suffix**: save number `n` is written as file
/// `BASCUS-94254PRO-<n-1>`, zero-padded to two digits.
///
/// The separator is a hyphen. Verified two ways: the literal in the menu
/// overlay's data segment, and the directory frames of real cards.
pub const LEGAIA_SAVE_FILENAME_PREFIX: &str = "BASCUS-94254PRO-";

/// The directory-frame filename for save slot `slot` (0-based).
pub fn legaia_save_filename(slot: u32) -> String {
    format!("{LEGAIA_SAVE_FILENAME_PREFIX}{:02}", slot.min(99))
}

/// The two title digits for save slot `slot`.
///
/// The slot is displayed **1-based**, so slot `0` writes `"01"`.
///
/// PORT: FUN_801e1934 (`0x801E1974..0x801E19EC`)
pub fn save_title_digits(slot: u32) -> [u8; 2] {
    let n = slot.wrapping_add(1);
    [
        SAVE_TITLE_DIGIT_BASE.wrapping_add((n / 10) as u8),
        SAVE_TITLE_DIGIT_BASE.wrapping_add((n % 10) as u8),
    ]
}

/// The four play-time digits of a save title for a play clock of `ticks`
/// (`1/60` s): `HH:MM`, clamped at `99:59` as the save screen's own time line
/// clamps.
pub fn save_title_time_digits(ticks: u32) -> [u8; 4] {
    let (h, m) = if ticks / 216_000 > 99 {
        (99, 59)
    } else {
        (ticks / 216_000, ticks / 3600 % 60)
    };
    [h / 10, h % 10, m / 10, m % 10].map(|d| SAVE_TITLE_DIGIT_BASE + d as u8)
}

/// Copy the save-title template into a block whose title region is empty:
/// a free block a save is claiming has no title, and the BIOS card browser
/// then lists the save nameless. The template is the menu overlay's own
/// (PROT 0899, `legaia_ui_formats::save_icon::title_template`); this crate
/// stays disc-free, so the caller supplies it.
/// [`write_retail_block_identity`] then patches its digits. A block that
/// already carries a title keeps it. Restamps the checksum.
pub fn write_retail_block_title(
    sc_block: &mut [u8],
    template: &[u8; RETAIL_TITLE_TEMPLATE_LEN],
) -> Result<()> {
    let end = RETAIL_TITLE_OFFSET + RETAIL_TITLE_TEMPLATE_LEN;
    if sc_block.len() < end {
        bail!("SC block too small for its title");
    }
    if sc_block[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET]
        .iter()
        .all(|&b| b == 0)
    {
        sc_block[RETAIL_TITLE_OFFSET..end].copy_from_slice(template);
        restamp_sc_block_checksum(sc_block);
    }
    Ok(())
}

/// The per-slot portrait a save block carries as its memory-card icon.
///
/// Source it from the disc's portrait sheet
/// (`legaia_ui_formats::save_icon::SaveIconSheet::tile_block_pixels` +
/// `tile_clut_bytes` for the same slot); this crate stays disc-free, so the
/// caller supplies the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailBlockIcon {
    /// 16-entry palette, little-endian BGR555.
    pub clut: [u8; RETAIL_ICON_CLUT_BYTES],
    /// 16x16 @ 4bpp pixels in contiguous save-block layout.
    pub pixels: [u8; RETAIL_ICON_FRAME_BYTES],
}

/// Stamp the block-identity fields retail's composer writes and a payload
/// writer cannot derive: the four-byte header, the save-number digits in the
/// title, and the slot's icon.
///
/// `icon = None` leaves the icon region as found - correct when re-saving
/// over a block that already carries one, wrong for a previously-free block,
/// which is why the caller should supply it whenever it has the disc.
///
/// The title digits are only patched when the block already carries a title
/// (a non-zero byte in the title region): stamping two digits into an
/// otherwise-empty title would produce a name that is two numerals and
/// nothing else.
///
/// PORT: FUN_801e1934
pub fn write_retail_block_identity(
    sc_block: &mut [u8],
    slot: u32,
    icon: Option<&RetailBlockIcon>,
) -> Result<()> {
    if sc_block.len() < RETAIL_GAME_DATA_OFFSET {
        bail!(
            "SC block too small for its header: {} bytes, need {RETAIL_GAME_DATA_OFFSET}",
            sc_block.len()
        );
    }
    sc_block[..SAVE_BLOCK_HEADER.len()].copy_from_slice(&SAVE_BLOCK_HEADER);

    let clock = sc_block
        .get(RETAIL_PLAY_COUNTER_OFFSET..RETAIL_PLAY_COUNTER_OFFSET + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let title = &mut sc_block[RETAIL_TITLE_OFFSET..RETAIL_ICON_CLUT_OFFSET];
    if title.iter().any(|&b| b != 0) {
        let digits = save_title_digits(slot);
        for (off, d) in RETAIL_TITLE_DIGIT_OFFSETS.iter().zip(digits.iter()) {
            if let Some(slot_byte) = title.get_mut(*off) {
                *slot_byte = *d;
            }
        }
        if let Some(clock) = clock {
            for (off, d) in RETAIL_TITLE_TIME_DIGIT_OFFSETS
                .iter()
                .zip(save_title_time_digits(clock))
            {
                title[*off] = d;
            }
        }
    }
    // The save number in the save summary (`sb v1,0x2a(s6)` at
    // `0x801E19AC`) - the slot the load screen's preview files it under.
    if let Some(b) = sc_block.get_mut(RETAIL_SUMMARY_SLOT_OFFSET) {
        *b = slot.min(u32::from(u8::MAX)) as u8;
    }

    if let Some(icon) = icon {
        sc_block[RETAIL_ICON_CLUT_OFFSET..RETAIL_ICON_CLUT_OFFSET + RETAIL_ICON_CLUT_BYTES]
            .copy_from_slice(&icon.clut);
        for off in RETAIL_ICON_FRAME_OFFSETS {
            sc_block[off..off + RETAIL_ICON_FRAME_BYTES].copy_from_slice(&icon.pixels);
        }
    }
    // The header, the title digits and the portrait all sit inside the
    // summed range (the sum starts at the `SC` magic), and retail's composer
    // sums the staging block only after every one of them is in place
    // (`0x801e1be0`). Restamp here so an identity written after the payload
    // does not leave a block the loader refuses as "Damaged data.".
    restamp_sc_block_checksum(sc_block);
    Ok(())
}
