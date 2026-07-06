//! Per-scene **scripted gold charges** (inn stays, guided tours, rides,
//! casino-coin purchases) embedded in a scene MAN's field-VM script.
//!
//! An inn's price is not an overlay data table: like the town gold-shop
//! stock ([`crate::shop_stock`]), it lives **inline in the scene's field-VM
//! script** (the MAN, asset type `0x03`) as an affordability-gate + debit
//! pair of literal-operand ops:
//!
//! ```text
//! 0x4E <pp> 0x30 <cost u16> <skip u16>   ; if gold < cost, jump +skip
//! ...                                    ;   (the "can't afford" text)
//! 0x3A <sext24(-cost)>                   ; gold -= cost
//! ```
//!
//! Op `0x4E` **sub-op 3** (operand byte 1 high nibble) loads the party gold
//! `_DAT_8008459C` and compares it against the u16 literal at operand `+2`
//! (low nibble 0 = "jump if gold < literal" - the can't-afford branch; the
//! page byte is unused by this sub-op). Sub-op 10 is the 32-bit variant
//! (literal lo16 at `+2` / hi16 at `+6`, 9 bytes) used where a price can
//! exceed 65535 (the casino gold-to-coin counter). Op `0x3A` (`ADD_MONEY`)
//! then applies the signed 24-bit delta. Provenance: the `0x4E` inner jump
//! table at overlay-0897 VA `0x801CEE30` (12 entries); the sub-3 arm at
//! `0x801E0AEC` loads `_DAT_8008459C`, sub-2 at `0x801E0AC0` loads the
//! per-character level byte, sub-9 at `0x801E0B34` loads the coin bank
//! `_DAT_800845A4` (see `ghidra/scripts/funcs/overlay_0897_801de840.txt`;
//! the decompiled-C case labels collapse these arms - the disassembly +
//! jump-table words are the ground truth).
//!
//! A site only counts as a charge when the **pair** matches: a gold compare
//! whose literal reappears as the magnitude of a negative `ADD_MONEY`
//! within a few ops after the gate (retail sites sit 7..~16 bytes apart).
//! Random data virtually never satisfies the joint constraint, so a byte
//! scan (robust to the dialogue-picker jump tables that desync a linear
//! walk - see [`crate::shop_stock`]) is safe.
//!
//! The engine consumes an inn site's cost as the `open_inn(cost)` argument
//! (`docs/subsystems/inn.md`); the debit itself replays through the field
//! VM's own `ADD_MONEY` op, so no separate cost table exists anywhere.

use crate::scene_asset_table;

/// Scene-MAN asset type byte.
const MAN_TYPE: u8 = 0x03;
/// Field-VM gold-delta opcode (`ADD_MONEY`, signed 24-bit operand).
const ADD_MONEY_OPCODE: u8 = 0x3A;
/// Field-VM compare opcode hosting the gold-bank sub-ops.
const CMP_OPCODE: u8 = 0x4E;
/// Retail gold clamp (`_DAT_8008459C` saturates here); bounds a plausible cost.
const MAX_GOLD: u32 = 9_999_999;
/// Max distance (bytes) from the compare opcode to its paired debit opcode.
/// Retail sites interleave at most a couple of short ops (a flag SET, a
/// sound cue) between the gate and the debit.
const PAIR_WINDOW: usize = 40;

/// One paired gold-charge site inside a decoded MAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoldCharge {
    /// Absolute offset of the `0x4E` gold-compare opcode byte.
    pub compare_off: usize,
    /// Absolute offset of the paired `0x3A` debit opcode byte.
    pub add_money_off: usize,
    /// The charge in gold (compare literal == debit magnitude).
    pub cost: u32,
    /// Compare sub-op: 3 (u16 literal, 7 bytes) or 10 (u32 literal, 9 bytes).
    pub sub_op: u8,
}

/// Decode the signed 24-bit `ADD_MONEY` operand at `man[off+1..off+4]`.
fn sext24(man: &[u8], off: usize) -> Option<i32> {
    let b = man.get(off + 1..off + 4)?;
    let raw = u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16);
    Some(if raw & 0x80_0000 != 0 {
        (raw | 0xFF00_0000) as i32
    } else {
        raw as i32
    })
}

/// Decode a gold-bank compare at `man[off]`, returning `(literal, sub_op,
/// record_len)`. Sub-op 3: `[4E][pp][3r][lit16][skip16]` (7 bytes). Sub-op
/// 10: `[4E][pp][Ar][lo16][skip16][hi16]` (9 bytes). Low nibble `r` must be
/// 0 or 1 (the two compare directions).
fn gold_compare(man: &[u8], off: usize) -> Option<(u32, u8, usize)> {
    if *man.get(off)? != CMP_OPCODE {
        return None;
    }
    let mode = *man.get(off + 2)?;
    if mode & 0xF > 1 {
        return None;
    }
    match mode >> 4 {
        3 => {
            let lit = u32::from(u16::from_le_bytes([*man.get(off + 3)?, *man.get(off + 4)?]));
            man.get(off + 6)?;
            Some((lit, 3, 7))
        }
        10 => {
            let lo = u32::from(u16::from_le_bytes([*man.get(off + 3)?, *man.get(off + 4)?]));
            let hi = u32::from(u16::from_le_bytes([*man.get(off + 7)?, *man.get(off + 8)?]));
            Some((lo | (hi << 16), 10, 9))
        }
        _ => None,
    }
}

/// Scan a decompressed MAN for paired gold-charge sites: a gold compare
/// (op `0x4E` sub-op 3 or 10) whose literal reappears as a negative
/// `ADD_MONEY` within [`PAIR_WINDOW`] bytes after the gate. Byte scan, not
/// an opcode walk (see the module docs).
pub fn scan(man: &[u8]) -> Vec<GoldCharge> {
    let mut out = Vec::new();
    for op in 0..man.len() {
        let Some((cost, sub_op, len)) = gold_compare(man, op) else {
            continue;
        };
        if cost == 0 || cost > MAX_GOLD {
            continue;
        }
        let lo = op + len;
        let hi = (op + PAIR_WINDOW).min(man.len());
        for debit in lo..hi {
            if man[debit] != ADD_MONEY_OPCODE {
                continue;
            }
            if sext24(man, debit) == Some(-(cost as i32)) {
                out.push(GoldCharge {
                    compare_off: op,
                    add_money_off: debit,
                    cost,
                    sub_op,
                });
                break;
            }
        }
    }
    out
}

/// A scene MAN located + decompressed from a PROT entry, with its
/// paired gold-charge sites.
#[derive(Debug, Clone)]
pub struct LocatedGoldCharges {
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Decompressed MAN (the [`GoldCharge`] offsets index into this).
    pub decoded: Vec<u8>,
    /// The paired gold-charge sites found in this scene's MAN.
    pub charges: Vec<GoldCharge>,
}

/// Locate + decompress a scene-bundle's MAN and scan it for gold charges.
/// Returns `None` when the entry isn't a scene bundle, has no MAN, the MAN
/// fails to decompress to its declared size, or has no paired charge site.
pub fn locate(entry: &[u8]) -> Option<LocatedGoldCharges> {
    let table = scene_asset_table::detect(entry)?;
    let man = table
        .used()
        .iter()
        .find(|d| d.type_byte == MAN_TYPE)
        .copied()?;
    if man.size == 0 || man.data_offset == 0 {
        return None;
    }
    let man_offset = man.data_offset as usize;
    let body = entry.get(man_offset..)?;
    let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
    if decoded.len() != man.size as usize {
        return None;
    }
    let charges = scan(&decoded);
    if charges.is_empty() {
        return None;
    }
    Some(LocatedGoldCharges {
        man_offset,
        decoded,
        charges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic MAN: a sub-3 gold gate on 200 + the paired -200 debit,
    /// with a flag-SET op between them (the retail interleave shape).
    #[test]
    fn paired_sub3_site_is_found() {
        let mut man = vec![0u8; 4];
        // 0x4E pp=1 mode=0x30 lit16=200 skip16=0x28
        man.extend_from_slice(&[0x4E, 0x01, 0x30, 200, 0, 0x28, 0]);
        man.extend_from_slice(&[0x53, 0x46]); // flag SET between gate + debit
        // 0x3A sext24(-200) = 0xFFFF38 -> bytes 38 FF FF
        man.extend_from_slice(&[0x3A, 0x38, 0xFF, 0xFF]);
        let sites = scan(&man);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].cost, 200);
        assert_eq!(sites[0].sub_op, 3);
        assert_eq!(sites[0].compare_off, 4);
        assert_eq!(sites[0].add_money_off, 13);
    }

    /// Sub-10 (u32 literal) variant: 90000 gold, above the u16 range.
    #[test]
    fn paired_sub10_site_is_found() {
        let mut man = vec![0u8; 2];
        // 90000 = 0x15F90: lo16 = 0x5F90, hi16 = 1.
        man.extend_from_slice(&[0x4E, 0x00, 0xA0, 0x90, 0x5F, 8, 0, 1, 0]);
        // -90000 = 0xFEA070 -> bytes 70 A0 FE
        man.extend_from_slice(&[0x3A, 0x70, 0xA0, 0xFE]);
        let sites = scan(&man);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].cost, 90_000);
        assert_eq!(sites[0].sub_op, 10);
    }

    /// An unpaired gate (no matching debit in the window), a positive
    /// delta, and a non-gold sub-op are all rejected.
    #[test]
    fn unpaired_or_wrong_sites_are_rejected() {
        let mut man = vec![0u8; 2];
        man.extend_from_slice(&[0x4E, 0x01, 0x30, 60, 0, 8, 0]); // gate on 60
        man.extend_from_slice(&[0x3A, 0xCE, 0xFF, 0xFF]); // -50: wrong size
        man.extend_from_slice(&[0x3A, 60, 0, 0]); // +60: a grant
        man.extend_from_slice(&[0x4E, 0x01, 0x00, 50, 0, 8, 0]); // sub-0: HP%
        man.extend_from_slice(&[0x3A, 0xCE, 0xFF, 0xFF]); // -50 debit
        assert!(scan(&man).is_empty());
    }
}
