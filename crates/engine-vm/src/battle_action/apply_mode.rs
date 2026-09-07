//! The damage kernel's **apply mode** - the register `FUN_801EC3E4` computes
//! after every admitted hit to decide whether the accumulated combo total
//! lands on live HP now, later, or not at all.
//!
//! `docs/subsystems/battle-action.md` § "Damage is one power byte per
//! animation hit event" names the three values. Only the `s2 = 0` arm was
//! ported; this module is the other two, and the look-ahead that produces
//! them.
//!
//! The kernel body appears **twice** in `FUN_801EC3E4` - the party-attacker
//! branch at `0x801EDEE4..0x801EE130` and the monster-attacker branch at
//! `0x801EE790..0x801EE980`. The two are instruction-identical apart from the
//! registers, so one port serves both; the transcription below cites the
//! party copy.

/// `s2 = 0` - the ordinary arm: the total lands when the strike cursor is
/// parked and this is the clip's last listed beat.
pub const APPLY_MODE_NORMAL: u8 = 0;
/// `s2 = 1` - the early arm: nothing left in the action can connect with the
/// target's size class, so the total lands on this hit.
pub const APPLY_MODE_EARLY: u8 = 1;
/// `s2 = 0xFF` - the carry arm: the War God Icon's *Attack x2* pair is still
/// running, so this action applies nothing and its total carries.
pub const APPLY_MODE_CARRY: u8 = 0xFF;

/// Class bit `0x1` - some remaining power byte is in `0x01..=0x10`
/// (`sltiu v0,a0,0x11` / `ori a2,a2,0x1` at `0x801EE004..0x801EE010`).
pub const CLASS_BIT_LOW: u8 = 0x1;
/// Class bit `0x2` - some remaining power byte is in `0x11..=0x15`
/// (`0x801EE014..0x801EE020`).
pub const CLASS_BIT_HIGH: u8 = 0x2;

/// The target `+0x1E` class whose connect test is [`CLASS_BIT_LOW`]
/// (`li v0,0x2` / `bne v1,v0` at `0x801EE084..0x801EE088`).
pub const MISS_CLASS_LOW: u8 = 2;
/// The target `+0x1E` class whose connect test is [`CLASS_BIT_HIGH`]
/// (`li v0,0x3` at `0x801EE09C`).
pub const MISS_CLASS_HIGH: u8 = 3;

/// The ability bit of the character record's `+0xF4` word that carries the
/// War God Icon's *Attack x2* (`andi v0,v0,0x2000` at `0x801EE0F8`; bit `0x0D`
/// of the 64-slot accessory-passive index space).
pub const WAR_GOD_ATTACK_X2_BIT: u32 = 0x2000;

/// The bound the Attack x2 pass counter `ctx[+0x16]` is compared against
/// (`sltiu v0,v0,0x2` at `0x801EE114`).
pub const ATTACK_X2_PASS_BOUND: u8 = 2;

/// The power-byte run of one action entry (`entry[+0x00..+0x04]`), the same
/// four bytes the hit-event kernel indexes with `actor[+0x1F4]`.
pub const POWER_RUN_LEN: usize = 4;

/// The queue-scan bound of the look-ahead's stream walk
/// (`sltiu v0,a0,0x10` at `0x801EDF70` / `0x801EE054`).
pub const LOOKAHEAD_QUEUE_LEN: usize = 0x10;

/// The stream-byte threshold the walk classifies on (`sltiu v0,v0,0x10` at
/// `0x801EDFAC`). Retail spells both this and [`LOOKAHEAD_QUEUE_LEN`] as the
/// literal `0x10`, but they are different quantities - one is an index bound,
/// this one is the art-constant floor - so they are named apart here.
pub const STREAM_ART_BYTE_MIN: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerByteVerdict {
    Continue,
    EndOfRun,
    EndOfScan,
}

/// Fold one power byte into the class-bit accumulator.
///
/// Returns [`PowerByteVerdict::EndOfScan`] when the byte ends the **whole**
/// scan - retail's `ori a2,a2,0x3; li s2,0x4; li a1,0x10` at
/// `0x801EE02C..0x801EE034`, which forces both loop counters past their
/// bounds.
///
/// ```text
/// 801ee000  beq   a0,zero,801ee04c ; 0x00 ends this run
/// 801ee004  _sltiu v0,a0,0x11
/// 801ee010  ori   a2,a2,0x1        ; 0x01..=0x10
/// 801ee014  sltiu v0,v0,0x5        ; (b - 0x11) < 5
/// 801ee020  ori   a2,a2,0x2        ; 0x11..=0x15
/// 801ee024  bne   v0,zero,801ee03c ; b < 0x16 -> keep going
/// 801ee02c  ori   a2,a2,0x3        ; b >= 0x16 -> both bits, stop everything
/// ```
///
/// PORT: FUN_801EC3E4 (`0x801EE000..0x801EE034`)
fn fold_power_byte(bits: &mut u8, b: u8) -> PowerByteVerdict {
    match b {
        0x00 => PowerByteVerdict::EndOfRun,
        0x01..=0x10 => {
            *bits |= CLASS_BIT_LOW;
            PowerByteVerdict::Continue
        }
        0x11..=0x15 => {
            *bits |= CLASS_BIT_HIGH;
            PowerByteVerdict::Continue
        }
        _ => {
            *bits |= CLASS_BIT_LOW | CLASS_BIT_HIGH;
            PowerByteVerdict::EndOfScan
        }
    }
}

/// Walk every hit the action still has left and return the class bits those
/// hits can connect with.
///
/// Two walks, in retail's order:
///
/// 1. **the rest of this entry's power run** - `power_run[next_hit_index..4]`.
///    Retail seeds the index from the attacker's own hit counter
///    (`lbu v0,0x1f4(v0)` / `addiu s2,v0,0x1` at `0x801EDEE4..0x801EDEF0`),
///    i.e. the hit after the one being resolved.
/// 2. **every remaining stream byte's entry** - the queue from
///    `stream_cursor - 1` (`addiu a1,v0,-0x1` at `0x801EDF68`), stopping at a
///    `0x00` terminator or at index `0x10`. A byte **at or above** `0x10` - an
///    art starter or an art constant - sets both class bits and ends the scan
///    (`bne v0,zero,0x801EDFC4` at `0x801EDFB0` branches on `byte < 0x10`, so
///    the `ori a2,a2,0x3` at `0x801EDFB8` is the fall-through for the *high*
///    bytes); a byte below `0x10`, i.e. a direction swing, resolves through
///    the per-actor action-entry table `0x801C9360[slot][byte]` and has its
///    entry's whole power run folded the same way, from index 0.
///
/// A parked cursor (`0xFF`) makes `stream_cursor - 1` land at `0xFE`, which
/// fails the `< 0x10` bound before the walk starts - so once the band has
/// left the strike loop the look-ahead covers only the current entry. That is
/// a retail behaviour, not a simplification.
///
/// `entry_power_run` resolves a stream byte to its entry's four power bytes;
/// `None` (a byte with no entry) contributes nothing, matching a run of zeros.
///
/// PORT: FUN_801EC3E4 (`0x801EDEE4..0x801EE058`)
pub fn remaining_hit_class_bits(
    power_run: &[u8; POWER_RUN_LEN],
    next_hit_index: u8,
    queue: &[u8],
    stream_cursor: u8,
    entry_power_run: impl Fn(u8) -> Option<[u8; POWER_RUN_LEN]>,
) -> u8 {
    let mut bits = 0u8;
    for &b in power_run.iter().skip(usize::from(next_hit_index)) {
        match fold_power_byte(&mut bits, b) {
            PowerByteVerdict::Continue => {}
            PowerByteVerdict::EndOfRun => break,
            PowerByteVerdict::EndOfScan => return bits,
        }
    }
    let mut idx = stream_cursor.wrapping_sub(1);
    while usize::from(idx) < LOOKAHEAD_QUEUE_LEN {
        let byte = queue.get(usize::from(idx)).copied().unwrap_or(0);
        if byte == 0 {
            break;
        }
        if byte >= STREAM_ART_BYTE_MIN {
            // An art starter / art constant: both classes, and the scan ends.
            return bits | CLASS_BIT_LOW | CLASS_BIT_HIGH;
        }
        if let Some(run) = entry_power_run(byte) {
            let mut ended = false;
            for &b in run.iter() {
                match fold_power_byte(&mut bits, b) {
                    PowerByteVerdict::Continue => {}
                    PowerByteVerdict::EndOfRun => break,
                    PowerByteVerdict::EndOfScan => {
                        ended = true;
                        break;
                    }
                }
            }
            if ended {
                return bits;
            }
        }
        idx = idx.wrapping_add(1);
    }
    bits
}

/// The apply mode itself, from the class bits, the target's `+0x1E` class and
/// the attacker's Attack x2 state.
///
/// ```text
/// 801ee080  lbu   v1,0x1e(v0)      ; target record +0x1E, via 0x801C9348
/// 801ee088  bne   v1,v0,801ee09c   ;   != 2
/// 801ee094  beq   v0,zero,801ee0b4 ;   class 2 and no CLASS_BIT_LOW -> s2 = 1
/// 801ee0a0  bne   v1,v0,801ee0bc   ;   != 3
/// 801ee0ac  bne   v0,zero,801ee0c0 ;   class 3 and CLASS_BIT_HIGH -> s2 stays 0
/// 801ee0f8  andi  v0,v0,0x2000     ; attacker record +0xF4, War God Icon
/// 801ee114  sltiu v0,v0,0x2        ; ctx[+0x16] < 2
/// 801ee120  li    s2,0xff
/// ```
///
/// The size-class arm is a **miss** model: a class-`2` target can only be
/// connected with by a power byte in `0x01..=0x10` and a class-`3` target only
/// by one in `0x11..=0x15`, so when the look-ahead finds no such byte left,
/// nothing after this hit can add to the total and it lands now. Every other
/// class connects with everything, which is why the port's previous behaviour
/// (always `s2 = 0`) was retail's answer for every target it could represent.
///
/// The War God arm overrides both: while the *Attack x2* pair is running the
/// action applies nothing at all.
///
/// PORT: FUN_801EC3E4 (`0x801EE060..0x801EE128`)
pub fn apply_mode(
    class_bits: u8,
    target_swing_class: u8,
    attacker_ability_bits: u32,
    attack_x2_pass: u8,
) -> u8 {
    let mut mode = APPLY_MODE_NORMAL;
    if target_swing_class == MISS_CLASS_LOW {
        if class_bits & CLASS_BIT_LOW == 0 {
            mode = APPLY_MODE_EARLY;
        }
    } else if target_swing_class == MISS_CLASS_HIGH && class_bits & CLASS_BIT_HIGH == 0 {
        mode = APPLY_MODE_EARLY;
    }
    if attacker_ability_bits & WAR_GOD_ATTACK_X2_BIT != 0 && attack_x2_pass < ATTACK_X2_PASS_BOUND {
        mode = APPLY_MODE_CARRY;
    }
    mode
}

#[cfg(test)]
mod apply_mode_tests {
    use super::*;
    use std::cell::Cell;

    fn no_entries(_b: u8) -> Option<[u8; POWER_RUN_LEN]> {
        None
    }

    #[test]
    fn class_bits_split_the_power_byte_space_at_0x11_and_0x16() {
        let bits = |b: u8| remaining_hit_class_bits(&[b, 0, 0, 0], 0, &[0u8; 16], 0xFF, no_entries);
        assert_eq!(bits(0x01), CLASS_BIT_LOW);
        assert_eq!(bits(0x10), CLASS_BIT_LOW);
        assert_eq!(bits(0x11), CLASS_BIT_HIGH);
        assert_eq!(bits(0x15), CLASS_BIT_HIGH);
        assert_eq!(bits(0x16), CLASS_BIT_LOW | CLASS_BIT_HIGH);
        assert_eq!(bits(0x00), 0, "a zero byte ends the run and sets nothing");
    }

    #[test]
    fn a_high_power_byte_ends_the_whole_scan() {
        // The 0x16 byte forces both loop counters past their bounds, so the
        // stream walk never runs.
        let seen = Cell::new(0usize);
        let bits = remaining_hit_class_bits(&[0x16, 0, 0, 0], 0, &[0x0C, 0, 0], 1, |_| {
            seen.set(seen.get() + 1);
            Some([0x05, 0, 0, 0])
        });
        assert_eq!(bits, CLASS_BIT_LOW | CLASS_BIT_HIGH);
        assert_eq!(seen.get(), 0, "the stream walk never ran");
    }

    #[test]
    fn the_lookahead_starts_after_the_hit_being_resolved() {
        // Power run [0x05, 0x12]: resolving hit 0 leaves 0x12 ahead (HIGH),
        // resolving hit 1 leaves nothing.
        let ahead = |idx: u8| {
            remaining_hit_class_bits(&[0x05, 0x12, 0, 0], idx, &[0u8; 16], 0xFF, no_entries)
        };
        assert_eq!(ahead(1), CLASS_BIT_HIGH);
        assert_eq!(ahead(2), 0);
    }

    #[test]
    fn an_art_constant_left_in_the_stream_connects_with_everything() {
        // Cursor 2 -> the walk starts at queue[1], a 0x1F art constant. A
        // byte at or above 0x10 sets both class bits and ends the scan
        // without ever consulting the entry table.
        let seen = Cell::new(0usize);
        let bits = remaining_hit_class_bits(&[0, 0, 0, 0], 0, &[0x0C, 0x1F, 0], 2, |_| {
            seen.set(seen.get() + 1);
            None
        });
        assert_eq!(bits, CLASS_BIT_LOW | CLASS_BIT_HIGH);
        assert_eq!(seen.get(), 0, "art constants are not looked up");
    }

    #[test]
    fn a_direction_swing_left_in_the_stream_folds_its_entrys_power_run() {
        // Cursor 2 -> the walk starts at queue[1], a 0x0E swing, whose
        // entry's power run is folded from index 0.
        let bits = remaining_hit_class_bits(&[0, 0, 0, 0], 0, &[0x0C, 0x0E, 0], 2, |b| {
            assert_eq!(b, 0x0E);
            Some([0x12, 0, 0, 0])
        });
        assert_eq!(bits, CLASS_BIT_HIGH);
    }

    #[test]
    fn a_parked_cursor_skips_the_stream_walk() {
        let seen = Cell::new(0usize);
        let bits = remaining_hit_class_bits(&[0, 0, 0, 0], 0, &[0x0E; 16], 0xFF, |_| {
            seen.set(seen.get() + 1);
            Some([0x05, 0, 0, 0])
        });
        assert_eq!(bits, 0);
        assert_eq!(seen.get(), 0);
    }

    #[test]
    fn a_size_class_three_target_misses_a_class_two_power_run() {
        // Everything left is 0x01..=0x10 (class LOW); a class-3 target can
        // only be connected with by 0x11..=0x15, so the total lands early.
        let bits = remaining_hit_class_bits(&[0x05, 0x05, 0, 0], 1, &[0u8; 16], 0xFF, no_entries);
        assert_eq!(bits, CLASS_BIT_LOW);
        assert_eq!(apply_mode(bits, MISS_CLASS_HIGH, 0, 0), APPLY_MODE_EARLY);
        // ...and a matching byte keeps it on the ordinary arm.
        assert_eq!(
            apply_mode(bits | CLASS_BIT_HIGH, MISS_CLASS_HIGH, 0, 0),
            APPLY_MODE_NORMAL
        );
    }

    #[test]
    fn a_size_class_two_target_misses_a_high_power_run() {
        assert_eq!(
            apply_mode(CLASS_BIT_HIGH, MISS_CLASS_LOW, 0, 0),
            APPLY_MODE_EARLY
        );
        assert_eq!(
            apply_mode(CLASS_BIT_LOW, MISS_CLASS_LOW, 0, 0),
            APPLY_MODE_NORMAL
        );
    }

    #[test]
    fn every_other_size_class_is_the_ordinary_arm() {
        for class in 0u8..=0xFF {
            if class == MISS_CLASS_LOW || class == MISS_CLASS_HIGH {
                continue;
            }
            assert_eq!(
                apply_mode(0, class, 0, 0),
                APPLY_MODE_NORMAL,
                "class {class}"
            );
        }
    }

    #[test]
    fn war_god_carries_both_passes_and_overrides_the_early_arm() {
        // The bit alone is not enough - the pass counter has to be under 2.
        assert_eq!(apply_mode(0, 0, WAR_GOD_ATTACK_X2_BIT, 0), APPLY_MODE_CARRY);
        assert_eq!(apply_mode(0, 0, WAR_GOD_ATTACK_X2_BIT, 1), APPLY_MODE_CARRY);
        assert_eq!(
            apply_mode(0, 0, WAR_GOD_ATTACK_X2_BIT, 2),
            APPLY_MODE_NORMAL
        );
        // It is applied last, so it wins over a size-class early apply.
        assert_eq!(
            apply_mode(CLASS_BIT_HIGH, MISS_CLASS_LOW, WAR_GOD_ATTACK_X2_BIT, 0),
            APPLY_MODE_CARRY
        );
        // Without the bit the counter is inert.
        assert_eq!(apply_mode(0, 0, 0, 0), APPLY_MODE_NORMAL);
    }
}
