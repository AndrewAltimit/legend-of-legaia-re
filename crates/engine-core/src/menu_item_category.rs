//! Menu-overlay **item category / weapon-favor table** + the validity
//! check the Best-Equipment chooser scores with.
//!
//! The menu overlay (PROT 0899, base `0x801CE818` -
//! `legaia_asset::menu_windows::MENU_OVERLAY_BASE_VA`) carries a small
//! zero-terminated byte-pair table at VA `0x801E4B88` (file offset
//! `0x16370`, computed with the same VA-to-file map as the window
//! descriptor table at `0x801E4738`):
//!
//! ```text
//! [item_id: u8][mask: u8]  ...repeated...  [0x00 terminator]
//! ```
//!
//! Each entry keys a **weapon item id** (every retail key resolves to a
//! `kind == 1` item whose stat-bonus record carries slot type `0x40` -
//! see the disc-gated `menu_item_category_disc` test) to a per-character
//! bit mask read as `bit (char_index + group * 4)`: the low nibble is
//! favor group 0, the high nibble favor group 1, bits 0/1/2 = Vahn /
//! Noa / Gala. The nibbles encode the character's weapon specialty
//! (knives = Vahn, throwing = Noa, clubs/axes = Gala; class weapons
//! carry `0x77` = favored by their sole wielder's whole row); on retail
//! both nibbles agree for every entry except the Astral Sword
//! (`mask = 0x01`: group 0 only), which keeps the Best-Equipment
//! auto-pick from ever selecting it.
//!
//! The checker `FUN_801DD0C0` walks the table for the item id and
//! returns a flat `1000` score bonus when the character's bit is set
//! (`0` on a clear bit, missing entry, or empty table). Its traced
//! caller is the Best-Equipment chooser
//! (`ghidra/scripts/funcs/overlay_menu_801cf88c.txt`, group 1), which
//! adds the bonus to the candidate weapon's attack byte so a favored
//! weapon out-scores any unfavored one.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_menu_801dd0c0.txt` (the
//! sibling `overlay_0897_801dd0c0.txt` dump at the same VA is a garbled
//! alias fragment - attribute by containment in the menu overlay).
// REF: FUN_801CF88C (Best-Equipment chooser; calls the check with group 1)

use anyhow::{Result, bail};

/// VA of the category table inside the resident menu overlay
/// (`DAT_801E4B88`).
pub const CATEGORY_TABLE_VA: u32 = 0x801E_4B88;

/// File offset of the table inside the as-loaded menu-overlay image
/// (PROT 0899), via the same base the window-descriptor table uses.
pub const CATEGORY_TABLE_OFFSET: usize =
    (CATEGORY_TABLE_VA - legaia_asset::menu_windows::MENU_OVERLAY_BASE_VA) as usize;

/// Parse bound: the retail table holds 27 entries; anything unterminated
/// within this many entries is rejected as a mis-located read.
pub const CATEGORY_TABLE_MAX_ENTRIES: usize = 128;

/// The score bonus the retail check returns on a favored item (`0x3E8`).
pub const CATEGORY_MATCH_SCORE: u32 = 1000;

/// One `[item_id, mask]` table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryEntry {
    /// Item id the entry keys (a weapon id on every retail row).
    pub item_id: u8,
    /// Per-character favor bits: bit `(char_index + group * 4)`.
    pub mask: u8,
}

impl CategoryEntry {
    /// The favor nibble for `group` (0 = low nibble, 1 = high nibble).
    pub fn nibble(&self, group: u32) -> u8 {
        if group == 0 {
            self.mask & 0xF
        } else {
            self.mask >> 4
        }
    }
}

/// Parse the category table out of the menu-overlay image (PROT 0899,
/// extended entry bytes - the table sits past the TOC size like the
/// window table).
pub fn parse_category_table(overlay: &[u8]) -> Result<Vec<CategoryEntry>> {
    let mut entries = Vec::new();
    let mut at = CATEGORY_TABLE_OFFSET;
    loop {
        if entries.len() >= CATEGORY_TABLE_MAX_ENTRIES {
            bail!(
                "menu item-category table unterminated within {} entries",
                CATEGORY_TABLE_MAX_ENTRIES
            );
        }
        let Some(&item_id) = overlay.get(at) else {
            bail!("menu overlay too short for item-category table at {at:#x}");
        };
        if item_id == 0 {
            break;
        }
        let Some(&mask) = overlay.get(at + 1) else {
            bail!("menu overlay truncates an item-category entry at {at:#x}");
        };
        entries.push(CategoryEntry { item_id, mask });
        at += 2;
    }
    Ok(entries)
}

/// Item-category favor check: walk `table` for `item_id` and score the
/// character's favor bit.
///
/// Faithful to the retail routine:
/// - the walk stops at the first zero `item_id` (the in-RAM terminator;
///   a parsed retail table never contains one, but synthetic slices may),
/// - the **first** matching entry decides - on a match the routine
///   returns immediately, [`CATEGORY_MATCH_SCORE`] if bit
///   `(char_index + group * 4)` of the mask is set, `0` if clear,
/// - the shift count is masked to 5 bits by the hardware (`srav`), so
///   any `char_index + group * 4 >= 8` lands past the 8-bit mask and
///   scores `0`,
/// - no match (or an empty table) scores `0`.
// PORT: FUN_801DD0C0 (menu overlay; a0 = char_index, a1 = item_id, a2 = group)
//
// NOT WIRED: its retail caller is the Best-Equipment chooser
// (`FUN_801CF88C`, ported as
// `crate::equip_session::best_equipment_candidates`), which binds this as
// the `weapon_category_score` argument - and that scan is itself
// unreached because the engine's Equip screen has no selectable row 0 for
// Best Equipment (see the tag on
// `crate::equip_session::EquipSession::slot_browse_confirm`). The table
// parse is exercised by the disc-gated `menu_item_category_disc` test,
// but the score has no live consumer until that row exists.
pub fn category_check(table: &[CategoryEntry], char_index: u32, item_id: u8, group: u32) -> u32 {
    let shift = char_index.wrapping_add(group.wrapping_mul(4)) & 0x1F;
    for entry in table {
        if entry.item_id == 0 {
            break;
        }
        if entry.item_id == item_id {
            return if (u32::from(entry.mask) >> shift) & 1 != 0 {
                CATEGORY_MATCH_SCORE
            } else {
                0
            };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<CategoryEntry> {
        vec![
            CategoryEntry {
                item_id: 0x22,
                mask: 0x11,
            },
            CategoryEntry {
                item_id: 0x28,
                mask: 0x22,
            },
            CategoryEntry {
                item_id: 0x31,
                mask: 0x44,
            },
            CategoryEntry {
                item_id: 0x1B,
                mask: 0x77,
            },
            CategoryEntry {
                item_id: 0xBA,
                mask: 0x01,
            },
        ]
    }

    #[test]
    fn match_bit_set_scores_1000() {
        let t = table();
        // char 0 (bit 0), group 0 -> mask 0x11 bit 0 set.
        assert_eq!(category_check(&t, 0, 0x22, 0), CATEGORY_MATCH_SCORE);
        // char 1 (bit 5 with group 1) -> mask 0x22 bit 5 set.
        assert_eq!(category_check(&t, 1, 0x28, 1), CATEGORY_MATCH_SCORE);
        // 0x77 mask: all three characters, both groups.
        for c in 0..3 {
            for g in 0..2 {
                assert_eq!(category_check(&t, c, 0x1B, g), CATEGORY_MATCH_SCORE);
            }
        }
    }

    #[test]
    fn match_bit_clear_scores_0() {
        let t = table();
        // char 1 against a Vahn-only mask.
        assert_eq!(category_check(&t, 1, 0x22, 0), 0);
        assert_eq!(category_check(&t, 1, 0x22, 1), 0);
        // Astral-Sword shape: group 0 favored, group 1 not.
        assert_eq!(category_check(&t, 0, 0xBA, 0), CATEGORY_MATCH_SCORE);
        assert_eq!(category_check(&t, 0, 0xBA, 1), 0);
    }

    #[test]
    fn group_selects_nibble_via_times_4_offset() {
        // Mask with only high-nibble bit 2 set: char 2 group 1 = bit 6.
        let t = vec![CategoryEntry {
            item_id: 5,
            mask: 0x40,
        }];
        assert_eq!(category_check(&t, 2, 5, 1), CATEGORY_MATCH_SCORE);
        assert_eq!(category_check(&t, 2, 5, 0), 0);
        // Shift past the byte (char 4 + group 1 = bit 8) scores 0.
        assert_eq!(category_check(&t, 4, 5, 1), 0);
        // The retail srav masks the shift to 5 bits: bit index 32 wraps
        // to bit 0.
        let t0 = vec![CategoryEntry {
            item_id: 5,
            mask: 0x01,
        }];
        assert_eq!(category_check(&t0, 32, 5, 0), CATEGORY_MATCH_SCORE);
    }

    #[test]
    fn no_match_and_zero_terminator_score_0() {
        let t = table();
        assert_eq!(category_check(&t, 0, 0x99, 0), 0);
        assert_eq!(category_check(&[], 0, 0x22, 0), 0);
        // An in-slice zero item_id terminates the walk: entries after it
        // are unreachable (the retail table ends at the first zero byte).
        let t = vec![
            CategoryEntry {
                item_id: 0,
                mask: 0xFF,
            },
            CategoryEntry {
                item_id: 0x22,
                mask: 0xFF,
            },
        ];
        assert_eq!(category_check(&t, 0, 0x22, 0), 0);
    }

    #[test]
    fn first_match_wins_even_with_clear_bit() {
        // Retail returns immediately on the first id match; a later
        // duplicate with the bit set is never consulted.
        let t = vec![
            CategoryEntry {
                item_id: 7,
                mask: 0x00,
            },
            CategoryEntry {
                item_id: 7,
                mask: 0xFF,
            },
        ];
        assert_eq!(category_check(&t, 0, 7, 0), 0);
    }

    #[test]
    fn parses_synthetic_overlay_table() {
        let mut overlay = vec![0u8; CATEGORY_TABLE_OFFSET + 8];
        let raw = [0x22u8, 0x11, 0xBA, 0x01, 0x00];
        overlay[CATEGORY_TABLE_OFFSET..CATEGORY_TABLE_OFFSET + raw.len()].copy_from_slice(&raw);
        let t = parse_category_table(&overlay).expect("parse");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].item_id, 0x22);
        assert_eq!(t[0].nibble(0), 1);
        assert_eq!(t[1].mask, 0x01);
        assert_eq!(t[1].nibble(1), 0);
    }

    #[test]
    fn parse_rejects_short_and_unterminated_input() {
        // Too short to reach the table.
        assert!(parse_category_table(&[0u8; 0x100]).is_err());
        // Ends mid-entry (id byte present, mask byte off the end).
        let mut overlay = vec![0u8; CATEGORY_TABLE_OFFSET + 1];
        overlay[CATEGORY_TABLE_OFFSET] = 0x22;
        assert!(parse_category_table(&overlay).is_err());
        // Unterminated within the entry bound.
        let mut overlay = vec![0u8; CATEGORY_TABLE_OFFSET + CATEGORY_TABLE_MAX_ENTRIES * 2 + 2];
        for e in overlay[CATEGORY_TABLE_OFFSET..].iter_mut() {
            *e = 0x33;
        }
        assert!(parse_category_table(&overlay).is_err());
    }
}
