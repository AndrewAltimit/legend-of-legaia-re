//! Menu-overlay **Arrange rank table** + the bag-sort kernel behind the
//! Items screen's "Arrange" command.
//!
//! The menu overlay (PROT 0899, base `0x801CE818` -
//! `legaia_asset::menu_windows::MENU_OVERLAY_BASE_VA`) carries a 256-byte
//! display-order table at VA `0x801E4A88` (file offset `0x16270`):
//! `table[rank] = item_id` - the canonical bag ordering, one byte per
//! rank slot over the full 8-bit item-id space.
//!
//! The retail Arrange kernel `FUN_801D64A8` (menu overlay, reached from
//! the Use / Throw Out / Arrange command SM `FUN_801D7C00` phase 2):
//!
//! 1. allocates a 256-byte scratch and inverts the table into an
//!    id -> rank map (`scratch[table[i]] = i`, ascending `i`, so a
//!    duplicated id keeps its **last** rank),
//! 2. selection-sorts the bag slot pairs (`0x80085958 + slot*2` =
//!    `[id, count]`, over `_DAT_8007B5EA.._DAT_8007B5EC`): for each
//!    position it scans forward for the occupied slot (both bytes
//!    non-zero) with the smallest rank and swaps the 2-byte pairs,
//! 3. stops early once no occupied slot remains - emptied / zero-count
//!    slots sink behind the occupied run.
//!
//! See `docs/subsystems/field-menu.md` (Items screen - Arrange) and
//! `ghidra/scripts/funcs/overlay_menu_801d64a8.txt`.

use anyhow::{Result, bail};

/// VA of the Arrange display-order table inside the resident menu
/// overlay (`DAT_801E4A88`).
pub const ARRANGE_TABLE_VA: u32 = 0x801E_4A88;

/// File offset of the table inside the as-loaded menu-overlay image
/// (PROT 0899), via the same base the window-descriptor table uses.
pub const ARRANGE_TABLE_OFFSET: usize =
    (ARRANGE_TABLE_VA - legaia_asset::menu_windows::MENU_OVERLAY_BASE_VA) as usize;

/// Number of rank slots (the full 8-bit item-id space).
pub const ARRANGE_TABLE_LEN: usize = 0x100;

/// The id -> sort-rank map the retail kernel builds from the overlay
/// table (the inversion step of `FUN_801D64A8`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrangeRankTable {
    /// `rank_of[item_id]` = the id's display rank (smaller sorts first).
    pub rank_of: [u8; ARRANGE_TABLE_LEN],
}

impl ArrangeRankTable {
    /// Build the id -> rank inversion from a display-order table
    /// (`order[rank] = item_id`). Ascending walk: an id listed twice
    /// keeps its **last** rank, matching the retail scratch fill.
    pub fn from_display_order(order: &[u8; ARRANGE_TABLE_LEN]) -> Self {
        let mut rank_of = [0u8; ARRANGE_TABLE_LEN];
        for (rank, &id) in order.iter().enumerate() {
            rank_of[id as usize] = rank as u8;
        }
        Self { rank_of }
    }

    /// Identity fallback for hosts without the overlay: rank = item id
    /// (keeps Arrange functional as an id-order sort).
    pub fn id_order() -> Self {
        let mut rank_of = [0u8; ARRANGE_TABLE_LEN];
        for (i, r) in rank_of.iter_mut().enumerate() {
            *r = i as u8;
        }
        Self { rank_of }
    }

    /// The sort rank of `item_id`.
    pub fn rank(&self, item_id: u8) -> u8 {
        self.rank_of[item_id as usize]
    }
}

/// Parse the Arrange display-order table out of the menu-overlay image
/// (PROT 0899, extended entry bytes - the table sits past the TOC size
/// like the window-descriptor table) and invert it into the rank map.
pub fn parse_arrange_rank_table(overlay: &[u8]) -> Result<ArrangeRankTable> {
    let Some(bytes) = overlay.get(ARRANGE_TABLE_OFFSET..ARRANGE_TABLE_OFFSET + ARRANGE_TABLE_LEN)
    else {
        bail!(
            "menu overlay too short for the Arrange table at {:#x}",
            ARRANGE_TABLE_OFFSET
        );
    };
    let mut order = [0u8; ARRANGE_TABLE_LEN];
    order.copy_from_slice(bytes);
    Ok(ArrangeRankTable::from_display_order(&order))
}

/// The Arrange bag sort over `[id, count]` slot pairs: selection sort by
/// [`ArrangeRankTable::rank`], considering only occupied slots (both
/// bytes non-zero) and stopping once none remain - empty / zero-count
/// slots sink behind the occupied run, in the order the swaps leave
/// them.
// PORT: FUN_801D64A8 (menu overlay; Items screen "Arrange")
pub fn arrange_bag_slots(slots: &mut [(u8, u8)], rank: &ArrangeRankTable) {
    let n = slots.len();
    for pos in 0..n.saturating_sub(1) {
        // Forward scan for the occupied slot with the smallest rank.
        let mut best: Option<(usize, u8)> = None;
        for (j, &(id, count)) in slots.iter().enumerate().skip(pos) {
            if id == 0 || count == 0 {
                continue;
            }
            let r = rank.rank(id);
            match best {
                Some((_, br)) if (r as i32) >= (br as i32) => {}
                _ => best = Some((j, r)),
            }
        }
        let Some((j, _)) = best else {
            // No occupied slot from here on - the retail loop breaks.
            break;
        };
        slots.swap(pos, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank_from_pairs(pairs: &[(u8, u8)]) -> ArrangeRankTable {
        // pairs = (item_id, rank)
        let mut rank_of = [0xFFu8; ARRANGE_TABLE_LEN];
        for &(id, r) in pairs {
            rank_of[id as usize] = r;
        }
        ArrangeRankTable { rank_of }
    }

    #[test]
    fn sorts_by_rank_and_sinks_empties() {
        let rank = rank_from_pairs(&[(0x30, 2), (0x10, 0), (0x20, 1)]);
        let mut bag = [
            (0x30, 3),
            (0x00, 0), // empty slot
            (0x20, 1),
            (0x10, 9),
            (0x40, 0), // zero-count slot: treated as empty
        ];
        arrange_bag_slots(&mut bag, &rank);
        assert_eq!(&bag[..3], &[(0x10, 9), (0x20, 1), (0x30, 3)]);
        // The two non-occupied pairs sink behind the occupied run.
        assert!(bag[3..].iter().all(|&(id, c)| id == 0 || c == 0));
    }

    #[test]
    fn all_empty_bag_is_untouched() {
        let rank = ArrangeRankTable::id_order();
        let mut bag = [(0u8, 0u8), (5, 0), (0, 7)];
        let before = bag;
        arrange_bag_slots(&mut bag, &rank);
        assert_eq!(bag, before);
    }

    #[test]
    fn ties_keep_first_found() {
        // Two ids sharing a rank: the forward scan's strict `<` keeps the
        // earlier slot first (the retail comparison is `slt` - strict).
        let rank = rank_from_pairs(&[(0x11, 4), (0x22, 4)]);
        let mut bag = [(0x22, 1), (0x11, 1)];
        arrange_bag_slots(&mut bag, &rank);
        assert_eq!(bag, [(0x22, 1), (0x11, 1)]);
    }

    #[test]
    fn inversion_keeps_last_rank_for_duplicate_ids() {
        let mut order = [0u8; ARRANGE_TABLE_LEN];
        order[3] = 0x55;
        order[9] = 0x55;
        let t = ArrangeRankTable::from_display_order(&order);
        assert_eq!(t.rank(0x55), 9);
    }

    #[test]
    fn parses_synthetic_overlay_table() {
        let mut overlay = vec![0u8; ARRANGE_TABLE_OFFSET + ARRANGE_TABLE_LEN];
        overlay[ARRANGE_TABLE_OFFSET] = 0x42; // rank 0 = item 0x42
        overlay[ARRANGE_TABLE_OFFSET + 1] = 0x17; // rank 1 = item 0x17
        let t = parse_arrange_rank_table(&overlay).expect("parse");
        assert_eq!(t.rank(0x42), 0);
        assert_eq!(t.rank(0x17), 1);
        assert!(parse_arrange_rank_table(&[0u8; 0x100]).is_err());
    }

    #[test]
    fn id_order_fallback_is_identity() {
        let t = ArrangeRankTable::id_order();
        assert_eq!(t.rank(0), 0);
        assert_eq!(t.rank(0xFE), 0xFE);
    }
}
