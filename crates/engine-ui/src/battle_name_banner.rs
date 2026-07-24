//! Battle art-name banner placement (`FUN_8004C650`).
//!
//! When a battle actor commits an Art, retail centres the Art's name banner
//! by measuring the name string and writing one X coordinate into four
//! banner fields at once. This module is the placement law, split from the
//! table walk that finds the record.
//!
//! Two leading bytes of the name string are markers rather than glyphs, and
//! each shifts the centred X:
//!
//! * `0xCF` - nudge the banner three pixels right.
//! * `0xC1` - the banner is prefixed with the casting character's display
//!   name, so the X moves left by half that name's measured width. Retail
//!   reads the already-stored X back as an **unsigned** halfword before
//!   subtracting, which is what this port reproduces.
//!
//! The two markers are tested independently and in that order, so a name
//! leading with `0xCF` takes the nudge and skips the prefix shift, while a
//! name leading with `0xC1` takes the prefix shift from the un-nudged X.
//!
//! Ported from the disassembly in `ghidra/scripts/funcs/8004c650.txt`. The
//! record table it walks is the SCUS arts-name table at `DAT_80075EC4`
//! (`docs/formats/art-data.md`); the character display name it measures on
//! the `0xC1` path is the `+0x2A7` field of the `0x414`-byte character
//! record (`docs/formats/save-record.md`).
//!
//! # NOT WIRED
//!
//! Nothing builds an Art-name banner draw list. This crate's draw builders are
//! the pause menu, the title / save rack, the fishing HUD and the shared text
//! overlay; none of them emits the four banner sub-primitives whose X fields
//! ([`BANNER_X_FIELD_OFFSETS`]) this module places, and the battle HUD the
//! engine does draw carries no Art-name row.
//!
//! The wire also crosses a crate boundary that does not exist yet: the state
//! that would drive it - which actor committed which Art - lives in
//! `engine-core`, and `engine-core` does not depend on this crate (the
//! dependency runs the other way, through the renderer). So a caller has to be
//! a battle-HUD draw builder here that a renderer host feeds the committed Art
//! id, plus the two measured widths, and neither the builder nor the
//! measurement plumbing is present.

/// Stride of one arts-name table record (`DAT_80075EC4`).
pub const ARTS_RECORD_STRIDE: usize = 0x14;

/// Record byte `+0x0` value that terminates the arts-name table.
pub const ARTS_TABLE_SENTINEL: u8 = 0x63;

/// Screen X the banner is centred on.
pub const BANNER_CENTER_X: i32 = 0xA0;

/// Leading name byte that nudges the banner right by [`NUDGE_PIXELS`].
pub const MARKER_NUDGE: u8 = 0xCF;

/// Pixels the [`MARKER_NUDGE`] marker adds.
pub const NUDGE_PIXELS: i32 = 3;

/// Leading name byte that prefixes the character's display name.
pub const MARKER_CHAR_NAME_PREFIX: u8 = 0xC1;

/// The four halfword banner-X fields retail writes, as offsets from the
/// battle HUD block base `0x80076C10`. All four always receive the same
/// value; they are separate fields of the four banner sub-primitives.
pub const BANNER_X_FIELD_OFFSETS: [usize; 4] = [0x722, 0x72A, 0x73A, 0x742];

/// Stride of one live character record (`0x80084708 + slot * 0x414`).
pub const CHARACTER_RECORD_STRIDE: usize = 0x414;

/// Offset of the display-name string inside a character record.
pub const CHARACTER_NAME_OFFSET: usize = 0x2A7;

/// PORT: FUN_8004c650
///
/// Locate the arts-name record for `(char_id, art_id)`.
///
/// `table` is the raw `0x14`-stride table starting at record 0. The walk
/// stops at the first record whose byte `+0x0` is [`ARTS_TABLE_SENTINEL`];
/// a sentinel in record 0 means the table is empty and no banner is placed.
/// Returns the record index, matching retail's `a0` counter.
pub fn find_arts_record(table: &[u8], char_id: u8, art_id: u8) -> Option<usize> {
    if table.first().copied()? == ARTS_TABLE_SENTINEL {
        return None;
    }
    let mut idx = 0usize;
    loop {
        let off = idx * ARTS_RECORD_STRIDE;
        let rec = table.get(off..off + 2)?;
        if rec[0] == ARTS_TABLE_SENTINEL {
            return None;
        }
        if rec[0] == char_id && rec[1] == art_id {
            return Some(idx);
        }
        idx += 1;
    }
}

/// PORT: FUN_8004c650
///
/// Centred banner X for an Art name.
///
/// `art_name_width` is the measured pixel width of the name string
/// (retail's `FUN_80035F04`), `art_name_lead` its first byte, and
/// `char_name_width` the measured width of the casting character's display
/// name - only consulted when the lead byte is
/// [`MARKER_CHAR_NAME_PREFIX`].
pub fn banner_x(art_name_width: i32, art_name_lead: u8, char_name_width: i32) -> i16 {
    let mut x = (BANNER_CENTER_X - (art_name_width >> 1)) as i16;
    if art_name_lead == MARKER_NUDGE {
        x = x.wrapping_add(NUDGE_PIXELS as i16);
    }
    if art_name_lead == MARKER_CHAR_NAME_PREFIX {
        // Retail re-reads the stored halfword with `lhu`, so the subtraction
        // runs on the zero-extended value.
        x = (i32::from(x as u16) - (char_name_width >> 1)) as i16;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(records: &[(u8, u8)]) -> Vec<u8> {
        let mut t = vec![0u8; (records.len() + 1) * ARTS_RECORD_STRIDE];
        for (i, (a, b)) in records.iter().enumerate() {
            t[i * ARTS_RECORD_STRIDE] = *a;
            t[i * ARTS_RECORD_STRIDE + 1] = *b;
        }
        t[records.len() * ARTS_RECORD_STRIDE] = ARTS_TABLE_SENTINEL;
        t
    }

    #[test]
    fn record_walk_matches_on_both_bytes() {
        let t = table(&[(1, 5), (1, 6), (2, 5)]);
        assert_eq!(find_arts_record(&t, 1, 6), Some(1));
        assert_eq!(find_arts_record(&t, 2, 5), Some(2));
        assert_eq!(find_arts_record(&t, 2, 6), None);
    }

    #[test]
    fn sentinel_in_record_zero_places_no_banner() {
        let t = table(&[]);
        assert_eq!(find_arts_record(&t, 1, 1), None);
    }

    #[test]
    fn plain_name_is_centred_on_0xa0() {
        assert_eq!(banner_x(0x40, b'A', 0), 0xA0 - 0x20);
        // Odd widths round toward zero through the arithmetic shift.
        assert_eq!(banner_x(0x41, b'A', 0), 0xA0 - 0x20);
    }

    #[test]
    fn nudge_marker_adds_three() {
        assert_eq!(banner_x(0x40, MARKER_NUDGE, 0), 0xA0 - 0x20 + 3);
    }

    #[test]
    fn char_name_prefix_shifts_left_by_half_the_name_width() {
        assert_eq!(
            banner_x(0x40, MARKER_CHAR_NAME_PREFIX, 0x20),
            0xA0 - 0x20 - 0x10
        );
    }

    #[test]
    fn the_two_markers_are_exclusive_by_construction() {
        // A lead byte can only be one value, so the nudge never composes
        // with the prefix shift.
        assert_eq!(banner_x(0x10, MARKER_NUDGE, 0x80), 0xA0 - 8 + 3);
        assert_eq!(
            banner_x(0x10, MARKER_CHAR_NAME_PREFIX, 0x80),
            0xA0 - 8 - 0x40
        );
    }

    #[test]
    fn all_four_banner_fields_are_distinct_offsets() {
        let mut sorted = BANNER_X_FIELD_OFFSETS;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| assert!(w[0] < w[1]));
    }
}
