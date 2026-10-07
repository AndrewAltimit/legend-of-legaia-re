//! Retail battle **stage seats** - the authored formation positions the
//! battle setup stamps into every combatant at battle start.
//!
//! Two static SCUS tables drive placement (`FUN_800513F0`, the battle
//! setup): the party table at `0x800775C8` (rows indexed by *party count*,
//! stride `0x18` = 3 slots x 8 bytes) and the monster table at `0x80077608`
//! (rows indexed by *monster count* plus `4` for the alternate formation
//! family, stride `0x20` = 4 slots x 8 bytes). Each 8-byte entry is
//! `[i16 x, i16 y, i16 z, i16 pad]`; `FUN_80024c88` copies it verbatim into
//! the spawn node (`+0x14..+0x19`), and the setup then writes it to the
//! actor's seat pair `+0x3C`/`+0x40` and copies that into the live position
//! `+0x34`/`+0x38`. The alternate family is selected by `DAT_8007BD60`
//! bit 7 (the same bit stored to `ctx+0x287`, the no-escape flag) or by
//! formation ids `0x3D..0x3F` - the pincer / scripted-fight seatings.
//!
//! Pinned against seven battle library save states: every solo battle
//! reads the party row-1 / monster row-1 seats byte-exactly
//! (`(0,-800)` vs `(0,+800)`), across all four camera-orbit angle saves and
//! the three Tetsu tutorial anchors. The full-party capture reads the
//! count-3 rows with a uniform scene offset (mid-battle drift), keeping the
//! authored values unambiguous. See `docs/subsystems/battle.md`
//! ("Stage seats").
//!
//! Coordinates are PSX battle-world units: the party faces `+Z`, monsters
//! face `-Z`, and the camera orbits the origin between them.

/// One authored seat: battle-world X/Y/Z (Y is `0` on every retail row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

const fn seat(x: i16, z: i16) -> Seat {
    Seat { x, y: 0, z }
}

/// Party seat rows (`0x800775C8`), indexed by `party_count - 1`, then by
/// party slot. Only slots `0..count` are seated; the tail entries are the
/// table's authored padding.
// PORT: FUN_800513F0 (party placement rows at 0x800775C8)
pub const PARTY_SEATS: [[Seat; 3]; 3] = [
    // 1 member: alone at centre-front.
    [seat(0, -800), seat(600, -1000), seat(-600, -1000)],
    // 2 members: side by side.
    [seat(300, -800), seat(-300, -800), seat(0, -1000)],
    // 3 members: lead centre, flankers ahead at the wings.
    [seat(0, -825), seat(600, -775), seat(-600, -775)],
];

/// Monster seat rows (`0x80077608`), normal family, indexed by
/// `monster_count - 1`, then by monster slot (the retail placer seats at
/// most 4 monsters).
// PORT: FUN_800513F0 (monster placement rows at 0x80077608)
pub const MONSTER_SEATS: [[Seat; 4]; 4] = [
    [seat(0, 800), seat(-600, 900), seat(600, 900), seat(0, 1400)],
    [
        seat(-300, 800),
        seat(300, 800),
        seat(900, 900),
        seat(-900, 900),
    ],
    [seat(-600, 825), seat(0, 750), seat(600, 825), seat(0, 1400)],
    [
        seat(-900, 900),
        seat(-300, 800),
        seat(300, 800),
        seat(900, 900),
    ],
];

/// Monster seat rows for the **alternate family** (rows `5..8`; selected by
/// `DAT_8007BD60` bit 7 / formation ids `0x3D..0x3F`). Counts 1 and 2 are
/// authored identical to the normal family; counts 3 and 4 differ (the
/// pincer arrangements).
pub const MONSTER_SEATS_ALT: [[Seat; 4]; 4] = [
    [seat(0, 800), seat(-600, 900), seat(600, 900), seat(0, 1400)],
    [
        seat(-300, 800),
        seat(300, 800),
        seat(900, 900),
        seat(-900, 900),
    ],
    [seat(0, 900), seat(-600, 700), seat(600, 700), seat(0, 1400)],
    [seat(0, 1000), seat(-600, 800), seat(600, 800), seat(0, 600)],
];

/// The authored seat for party `slot` in a `party_count`-member battle.
pub fn party_seat(party_count: u8, slot: usize) -> Seat {
    let row = (party_count.clamp(1, 3) - 1) as usize;
    PARTY_SEATS[row][slot.min(2)]
}

/// The authored seat for monster `slot` (0-based within the monster block)
/// in a `monster_count`-enemy battle. `alt` selects the alternate formation
/// family. The retail placer seats at most 4 monsters; a 5th engine slot
/// reuses the row's last entry.
pub fn monster_seat(monster_count: u8, slot: usize, alt: bool) -> Seat {
    let row = (monster_count.clamp(1, 4) - 1) as usize;
    let table = if alt {
        &MONSTER_SEATS_ALT
    } else {
        &MONSTER_SEATS
    };
    table[row][slot.min(3)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_battle_seats_match_the_save_pinned_values() {
        // Every solo library save reads exactly these.
        assert_eq!(party_seat(1, 0), seat(0, -800));
        assert_eq!(monster_seat(1, 0, false), seat(0, 800));
    }

    #[test]
    fn full_party_row_matches_the_pinned_layout() {
        assert_eq!(party_seat(3, 0), seat(0, -825));
        assert_eq!(party_seat(3, 1), seat(600, -775));
        assert_eq!(party_seat(3, 2), seat(-600, -775));
    }

    #[test]
    fn alt_family_differs_only_for_counts_3_and_4() {
        for count in 1..=2u8 {
            for slot in 0..4 {
                assert_eq!(
                    monster_seat(count, slot, false),
                    monster_seat(count, slot, true)
                );
            }
        }
        assert_ne!(monster_seat(3, 0, false), monster_seat(3, 0, true));
        assert_ne!(monster_seat(4, 0, false), monster_seat(4, 0, true));
    }

    #[test]
    fn every_authored_row_is_flat() {
        for row in PARTY_SEATS.iter() {
            assert!(row.iter().all(|s| s.y == 0));
        }
        for row in MONSTER_SEATS.iter().chain(MONSTER_SEATS_ALT.iter()) {
            assert!(row.iter().all(|s| s.y == 0));
        }
    }
}
