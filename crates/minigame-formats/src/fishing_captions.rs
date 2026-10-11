//! The fishing HUD's **lure row captions** - the strings `FUN_801D13F0` prints
//! around the lures-remaining count.
//!
//! ## Provenance
//!
//! `FUN_801D13F0` (PROT 0972 at slot-A base `0x801CE818`) draws the row with
//! the SCUS text primitive `FUN_80036888(str, 0, 0, x, y = 0xC)`:
//!
//! | x | string VA | what it is |
//! |---|---|---|
//! | `0x98` | `0x801CEFA4 + lure * 4` (`lure` = `_DAT_80084450`, `0..=2`; `0x801D14D0..0x801D14F8`) | a two-byte MES item-name token `0xC2, id` - the lure's own name |
//! | `0xF3` | `0x801CEFBC` (`0x801D150C`) | the "remaining" caption |
//! | `0x100` | - | the count: `FUN_80034B78(count, 4, x, y)`, four blank-padded 8-px cells |
//! | `0x12A` | `0x801CEFC4` (`0x801D154C`) | the trailing caption |
//!
//! The count is the bag count of item `lure + 0x9D` (`0x801D152C..0x801D1534`).
//!
//! ## No Sony bytes
//!
//! Addresses only; the strings are read from the user's disc.

use crate::fishing_species::FISHING_OVERLAY_BASE_VA;

/// VA of the first lure-label token; label `i` is at `+ 4 * i`.
pub const LURE_LABEL_VA: u32 = 0x801C_EFA4;
/// VA of the caption drawn before the count.
pub const LURES_LEFT_VA: u32 = 0x801C_EFBC;
/// VA of the caption drawn after the count.
pub const LURE_SUFFIX_VA: u32 = 0x801C_EFC4;
/// The MES token byte that prints an item name (`0xC2, id`).
pub const ITEM_NAME_TOKEN: u8 = 0xC2;

/// The lure row's text, raw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FishingCaptionsRaw {
    /// The item id each of the three lure labels names (`0xC2, id`).
    pub lure_items: [u8; 3],
    /// The caption before the count.
    pub lures_left: Vec<u8>,
    /// The caption after the count.
    pub suffix: Vec<u8>,
}

fn c_string(overlay: &[u8], va: u32, max: usize) -> Option<Vec<u8>> {
    let off = va.checked_sub(FISHING_OVERLAY_BASE_VA)? as usize;
    let tail = overlay.get(off..)?;
    let end = tail.iter().take(max).position(|&b| b == 0)?;
    Some(tail[..end].to_vec())
}

/// Read the row's strings out of the fishing overlay image. `None` when the
/// image is short or a label is not an item-name token.
pub fn parse(overlay: &[u8]) -> Option<FishingCaptionsRaw> {
    let mut lure_items = [0u8; 3];
    for (i, slot) in lure_items.iter_mut().enumerate() {
        let tok = c_string(overlay, LURE_LABEL_VA + 4 * i as u32, 4)?;
        match tok.as_slice() {
            [ITEM_NAME_TOKEN, id] => *slot = *id,
            _ => return None,
        }
    }
    Some(FishingCaptionsRaw {
        lure_items,
        lures_left: c_string(overlay, LURES_LEFT_VA, 8)?,
        suffix: c_string(overlay, LURE_SUFFIX_VA, 8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tokens_and_both_captions() {
        let off = |va: u32| (va - FISHING_OVERLAY_BASE_VA) as usize;
        let mut img = vec![0u8; off(LURE_SUFFIX_VA) + 8];
        for i in 0..3 {
            img[off(LURE_LABEL_VA) + 4 * i] = ITEM_NAME_TOKEN;
            img[off(LURE_LABEL_VA) + 4 * i + 1] = 0x40 + i as u8;
        }
        img[off(LURES_LEFT_VA)..][..3].copy_from_slice(b"ab:");
        img[off(LURE_SUFFIX_VA)] = b'z';
        let c = parse(&img).unwrap();
        assert_eq!(c.lure_items, [0x40, 0x41, 0x42]);
        assert_eq!(
            (c.lures_left.as_slice(), c.suffix.as_slice()),
            (&b"ab:"[..], &b"z"[..])
        );
        img[off(LURE_LABEL_VA)] = 0x41;
        assert!(parse(&img).is_none());
    }
}
