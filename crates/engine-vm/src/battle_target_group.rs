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
//! Three things about the output are easy to get wrong and are faithful here:
//!
//! * **The centroid is negated.** Retail divides `-sum` by the accepted count,
//!   so the returned pair is the *camera-space translation* that brings the
//!   group to the origin, not the group's own position.
//! * **The extent lands at `+4` of its output struct, not `+0`.** Retail writes
//!   a single halfword at `out_extent + 4` and never touches `+0`.
//! * **`0x400` is the extent's floor, not its ceiling.** The tail compare is
//!   `slti v0, v0, 0x400` / `beq v0, zero, <ret>` (`0x801DD094`), so the store
//!   of `0x400` runs on the *`extent < 0x400`* side - a group tighter than
//!   `0x400` is widened to it, and a wider one is left alone. See
//!   [`MIN_GROUP_EXTENT`].
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801dceac.txt`.
//!
//! # Where it runs
//!
//! Every retail caller uses the centroid for one thing only: it feeds the two
//! negated components straight into the 12-bit bearing helper `FUN_80019B28`
//! and stores the result in the acting actor's facing halfword `+0x46`. That
//! is the shape at all three call sites - the battle-action SM's cast-begin
//! state (`overlay_0898_801e295c.txt` `0x801E4370..0x801E43A4`),
//! `FUN_801DC0A0` `0x801DC39C` and `0x801DC51C` - and none of them reads the
//! extent output back at all.
//!
//! The port runs the SM's copy:
//! [`magic_cast_begin`](crate::battle_action) assembles the eight
//! [`GroupSlot`]s from [`BattleActionHost::actor_position`] and calls
//! [`target_group_aim`] whenever the acting actor's target byte `+0x1DD` is a
//! group code rather than a slot. `engine-core`'s monster-AI target resolver
//! (`FUN_801E7320`) is what produces those codes in production: its class-`8`
//! arm writes `9` (the enemy row) and its class-`7` arm writes `8` (the party)
//! into `active_target`, one roll in three each.
//!
//! The walk is gated per slot, differently for the two halves of the actor
//! table, and the port reaches the second gate indirectly:
//!
//! * A **party** slot is live when the roster byte `DAT_8007BD10[slot]` is
//!   non-zero - the per-slot character id, so zero means "no such party
//!   member". The port's equivalent is seat occupancy: the actor table holds
//!   exactly the seated combatants.
//! * A **monster** slot is live when the actor record's `+0x4` word is
//!   non-zero. That word is the per-actor prim state the renderer emits
//!   through ([`BattleActor::render_color`](crate::battle_action::BattleActor)
//!   in the port), and the only routine that zeroes it - the summon-fade sweep
//!   at `0x801E4B50` - writes `+0x21C = 0xFF` in the next two instructions.
//!   The port maintains `+0x21C` and leaves `+0x4` at its default, so
//!   [`RENDER_FLAG_HIDDEN`] on `+0x21C` is the reachable half of the pair.
//!
//! A range whose slots are all dead yields no answer at all - retail would
//! divide by zero there, so this port returns `None`.

/// The value the extent output is **floored** at (`0x400`). Named for the
/// direction of retail's compare: see the module doc.
pub const MIN_GROUP_EXTENT: i16 = 0x400;

/// The `+0x21C` render flag the summon-fade sweep writes alongside zeroing the
/// `+0x4` prim word (`0x801E4B50`/`0x801E4B5C`). A monster carrying it is not
/// drawn, which is the state retail's `+0x4` gate rejects.
pub const RENDER_FLAG_HIDDEN: u8 = 0xFF;

/// One actor slot's contribution to the group geometry.
#[derive(Debug, Clone, Copy)]
pub struct GroupSlot {
    /// The slot is present and renderable - the module doc's two gates. A dead
    /// slot contributes nothing.
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
    /// `*(i16 *)(out_extent + 4)`: `max(max_x - min_x, max_z - min_z)`, floored
    /// at [`MIN_GROUP_EXTENT`].
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

    // `slti`/`beq` at `0x801DD094`: the `0x400` store runs when the measured
    // extent is BELOW it, so this is a floor.
    let extent = (max_x - min_x).max(max_z - min_z);
    let extent = extent.max(MIN_GROUP_EXTENT as i32);

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
        // Measured extents (x 50-10 = 40, z 60-20 = 40) are below the floor.
        assert_eq!(aim.extent, MIN_GROUP_EXTENT);
    }

    #[test]
    fn dead_slots_are_skipped_entirely() {
        let slots = [live(10, 0), dead(), live(30, 0)];
        let aim = target_group_aim(8, &slots).unwrap();
        assert_eq!(aim.centroid_x, -20);
        // The dead slot's 9999 must not reach the extremes: with it the X span
        // would be 9989 and clear the floor, so the floored answer is the
        // evidence it was skipped.
        assert_eq!(aim.extent, MIN_GROUP_EXTENT);
    }

    #[test]
    fn the_larger_of_the_two_extents_wins() {
        // Both spans clear the floor, so the max is observable.
        let slots = [live(0, 0), live(500, 3000), live(1000, 1500)];
        // x span 1000, z span 3000.
        assert_eq!(target_group_aim(8, &slots).unwrap().extent, 3000);
        // Mirrored: X wins when it is the larger.
        let slots = [live(0, 0), live(3000, 500), live(1500, 1000)];
        assert_eq!(target_group_aim(8, &slots).unwrap().extent, 3000);
    }

    /// `0x400` is retail's **floor**, not its ceiling: a tight group is widened
    /// to it and a wide one passes through untouched. The old reading here had
    /// the compare backwards, which only a group wider than `0x400` can show.
    #[test]
    fn extent_is_floored_not_capped() {
        let tight = [live(-10, 0), live(10, 0), live(0, 0)];
        assert_eq!(
            target_group_aim(8, &tight).unwrap().extent,
            MIN_GROUP_EXTENT
        );
        let wide = [live(-4000, 0), live(4000, 0), live(0, 0)];
        assert_eq!(target_group_aim(8, &wide).unwrap().extent, 8000);
    }

    #[test]
    fn an_explicit_single_actor_code_reads_only_that_slot() {
        let slots = [live(10, 10), live(400, 400), live(0, 0)];
        let aim = target_group_aim(1, &slots).unwrap();
        assert_eq!((aim.centroid_x, aim.centroid_z), (-400, -400));
        // One slot spans nothing, so the floor is what comes out.
        assert_eq!(aim.extent, MIN_GROUP_EXTENT);
    }

    #[test]
    fn an_all_dead_group_has_no_answer_instead_of_dividing_by_zero() {
        let slots = [dead(), dead(), dead()];
        assert!(target_group_aim(8, &slots).is_none());
        // Out-of-range slot indices are the same case, not a panic.
        assert!(target_group_aim(9, &slots).is_none());
    }

    /// The composition retail performs at `0x801E4370..0x801E43A4`, which
    /// `magic_cast_begin` now runs: retail negates the already-negated centroid
    /// back into a world position, takes the bearing from the actor to it,
    /// biases by a half-turn and masks to 12 bits before storing into `+0x46`.
    /// This is the unit-level statement of it; the SM-level one is
    /// `crates/engine-vm/tests/battle_cast_facing.rs`.
    #[test]
    fn cast_begin_composes_the_aim_with_the_live_bearing_kernel() {
        use crate::battle_action::bearing_12bit_approx;

        // A three-strong enemy row centred at (+400, 0) in world X/Z, and an
        // actor sitting at the origin: the group is due +X of it.
        let slots = [live(400, -100), live(400, 0), live(400, 100)];
        let aim = target_group_aim(8, &slots).unwrap();
        assert_eq!((aim.centroid_x, aim.centroid_z), (-400, 0));

        // Retail's `subu a0,zero,a0` / `subu a1,zero,a1` on the two outputs.
        let (group_z, group_x) = (-aim.centroid_z, -aim.centroid_x);
        // `bearing_12bit(p1z, p1x, p2z, p2x)` with the **group** as `p1` and
        // the actor as `p2` - the argument order at `0x801E4384..0x801E4394`,
        // which measures group -> actor. The half-turn is what flips it back
        // to actor -> group, and it is the same shape the single-target arm at
        // `0x801E4358` uses with the target in place of the centroid.
        let bearing = bearing_12bit_approx(group_z, group_x, 0, 0);
        let facing = (bearing.wrapping_add(0x800)) & 0xFFF;
        // The group is due +X of the actor, which is 0x400 of the 12-bit
        // circle: the raw bearing is 0xC00 and the bias lands 0x400.
        assert_eq!(bearing, 0xC00);
        assert_eq!(facing, 0x400);
    }

    #[test]
    fn division_truncates_toward_zero_like_mips_div() {
        // sum = 5, count = 2 -> -5/2 = -2 (toward zero), not -3.
        let slots = [live(2, 0), live(3, 0), dead()];
        assert_eq!(target_group_aim(8, &slots).unwrap().centroid_x, -2);
    }
}
