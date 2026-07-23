//! Area-effect target-group aiming: `FUN_801DCEAC`.
//!
//! Several battle-action readers share a compact **target-group code** in place
//! of an explicit actor list. `FUN_801DCEAC` is the geometry half of that: it
//! decodes the code into an actor-slot range, walks the live slots in the range,
//! and reports where to aim an area effect - the group's centroid, plus the
//! larger of the group's two horizontal extents as a radius/scale hint.
//!
//! The code space is shared with `FUN_801DEA50` (the staged-value reader) and is
//! tabulated in `docs/formats/art-data.md`:
//!
//! | Code | Slot range `[start, end)` | Meaning |
//! |---|---|---|
//! | `8` | `[0, 3)` | the party |
//! | `9` | `[3, 7)` | the enemy row |
//! | `0xA` | `[0, 7)` | everyone |
//! | anything else | `[code, code + 1)` | one explicit actor |
//!
//! Two things about the output are easy to get wrong and are faithful here:
//!
//! * **The centroid is negated.** Retail divides `-sum` by the accepted count,
//!   so the returned pair is the *camera-space translation* that brings the
//!   group to the origin, not the group's own position.
//! * **The extent lands at `+4` of its output struct, not `+0`.** Retail writes
//!   a single halfword at `out_extent + 4` and never touches `+0`, and it clamps
//!   the value to `0x400`.
//!
//! Slot liveness is read differently for the two halves of the actor table: a
//! party slot (`< 3`) is live when the roster byte `DAT_8007BD10[slot]` is
//! non-zero (that is the per-slot character id, so zero means "no such party
//! member"), while a monster slot is live when the actor record's `+0x4` word is
//! non-zero. A range whose slots are all dead yields no answer at all - retail
//! would divide by zero there, so this port returns `None`.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801dceac.txt`.

/// The maximum the extent output is clamped to (`0x400`).
pub const MAX_GROUP_EXTENT: i16 = 0x400;

/// One actor slot's contribution to the group geometry.
#[derive(Debug, Clone, Copy)]
pub struct GroupSlot {
    /// The slot is present and renderable: for a party slot (`< 3`) the roster
    /// byte `DAT_8007BD10[slot] != 0`; for a monster slot the actor's `+0x4`
    /// word `!= 0`. A dead slot contributes nothing.
    pub live: bool,
    /// Actor world X (`+0x34`, i16).
    pub x: i16,
    /// Actor world Z (`+0x38`, i16).
    pub z: i16,
}

/// What `FUN_801DCEAC` writes through its two output pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupAim {
    /// `*(i16 *)(out_centroid + 0)`: `-sum_x / count`.
    pub centroid_x: i16,
    /// `*(i16 *)(out_centroid + 4)`: `-sum_z / count`.
    pub centroid_z: i16,
    /// `*(i16 *)(out_extent + 4)`: `max(max_x - min_x, max_z - min_z)`, clamped
    /// to [`MAX_GROUP_EXTENT`].
    pub extent: i16,
}

/// Decode a target-group code into the actor-slot range `[start, end)` the
/// group covers.
///
/// PORT: FUN_801DCEAC (group-code decode)
pub fn target_group_range(code: u8) -> (u8, u8) {
    match code {
        8 => (0, 3),
        9 => (3, 7),
        0xA => (0, 7),
        other => (other, other.wrapping_add(1)),
    }
}

/// Centroid + extent of a target group, given every slot in the decoded range.
///
/// `slots` is indexed by actor slot, so a caller passes the whole 7-slot battle
/// actor table and this function selects the range itself. Returns `None` when
/// the range is empty or every slot in it is dead - the case retail reaches its
/// divide-by-zero trap on.
///
/// PORT: FUN_801DCEAC
pub fn target_group_aim(code: u8, slots: &[GroupSlot]) -> Option<GroupAim> {
    let (start, end) = target_group_range(code);

    let mut count: i32 = 0;
    let (mut min_x, mut max_x, mut sum_x) = (0i32, 0i32, 0i32);
    let (mut min_z, mut max_z, mut sum_z) = (0i32, 0i32, 0i32);

    for slot in start..end {
        let Some(s) = slots.get(slot as usize) else {
            continue;
        };
        if !s.live {
            continue;
        }
        let (x, z) = (s.x as i32, s.z as i32);
        if count == 0 {
            // First accepted slot seeds both extremes and both sums.
            min_x = x;
            max_x = x;
            sum_x = x;
            min_z = z;
            max_z = z;
            sum_z = z;
        } else {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
            sum_x += x;
            sum_z += z;
        }
        count += 1;
    }

    if count == 0 {
        return None;
    }

    // Retail divides the *negated* sums - the outputs are a translation toward
    // the origin, not the group's position.
    let centroid_x = (-sum_x) / count;
    let centroid_z = (-sum_z) / count;

    let extent = (max_x - min_x).max(max_z - min_z);
    let extent = extent.min(MAX_GROUP_EXTENT as i32);

    Some(GroupAim {
        centroid_x: centroid_x as i16,
        centroid_z: centroid_z as i16,
        extent: extent as i16,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(x: i16, z: i16) -> GroupSlot {
        GroupSlot { live: true, x, z }
    }
    fn dead() -> GroupSlot {
        GroupSlot {
            live: false,
            x: 9999,
            z: 9999,
        }
    }

    #[test]
    fn group_codes_decode_to_the_documented_ranges() {
        assert_eq!(target_group_range(8), (0, 3));
        assert_eq!(target_group_range(9), (3, 7));
        assert_eq!(target_group_range(0xA), (0, 7));
        for one in [0u8, 1, 2, 3, 6, 7, 0xB, 0x20] {
            assert_eq!(target_group_range(one), (one, one + 1));
        }
    }

    #[test]
    fn centroid_is_the_negated_mean_of_the_live_slots() {
        let slots = [live(10, 20), live(30, 40), live(50, 60)];
        let aim = target_group_aim(8, &slots).unwrap();
        // mean x = 30, mean z = 40, both negated.
        assert_eq!(aim.centroid_x, -30);
        assert_eq!(aim.centroid_z, -40);
        // extents: x 50-10 = 40, z 60-20 = 40.
        assert_eq!(aim.extent, 40);
    }

    #[test]
    fn dead_slots_are_skipped_entirely() {
        let slots = [live(10, 0), dead(), live(30, 0)];
        let aim = target_group_aim(8, &slots).unwrap();
        assert_eq!(aim.centroid_x, -20);
        // The dead slot's 9999 must not reach the extremes.
        assert_eq!(aim.extent, 20);
    }

    #[test]
    fn the_larger_of_the_two_extents_wins() {
        let slots = [live(0, 0), live(5, 100), live(10, 50)];
        // x span 10, z span 100.
        assert_eq!(target_group_aim(8, &slots).unwrap().extent, 100);
    }

    #[test]
    fn extent_is_clamped() {
        let slots = [live(-4000, 0), live(4000, 0), live(0, 0)];
        assert_eq!(
            target_group_aim(8, &slots).unwrap().extent,
            MAX_GROUP_EXTENT
        );
    }

    #[test]
    fn an_explicit_single_actor_code_reads_only_that_slot() {
        let slots = [live(10, 10), live(400, 400), live(0, 0)];
        let aim = target_group_aim(1, &slots).unwrap();
        assert_eq!((aim.centroid_x, aim.centroid_z), (-400, -400));
        assert_eq!(aim.extent, 0);
    }

    #[test]
    fn an_all_dead_group_has_no_answer_instead_of_dividing_by_zero() {
        let slots = [dead(), dead(), dead()];
        assert!(target_group_aim(8, &slots).is_none());
        // Out-of-range slot indices are the same case, not a panic.
        assert!(target_group_aim(9, &slots).is_none());
    }

    #[test]
    fn division_truncates_toward_zero_like_mips_div() {
        // sum = 5, count = 2 -> -5/2 = -2 (toward zero), not -3.
        let slots = [live(2, 0), live(3, 0), dead()];
        assert_eq!(target_group_aim(8, &slots).unwrap().centroid_x, -2);
    }
}
