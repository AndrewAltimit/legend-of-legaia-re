//! The AI-controlled party member's delegated pick - `FUN_801EED1C`'s
//! `DAT_8007BD10[slot] == 4` block (`0x801EEE0C..0x801EF038`).
//!
//! The player queue builder has one arm that fills another actor's action
//! itself instead of reading the pad. It is keyed on the **roster character
//! id** `4`, which in the retail roster is Terra, and it is the only party-
//! side chooser in the battle overlay.
//!
//! Three things in `docs/subsystems/battle-action.md`'s table of this block
//! are corrected here, each read off the disassembly of PROT 0898 at slot-A
//! base `0x801CE818`:
//!
//! * **The gauge it reads is not its own.** The three magic conditions test
//!   `actor_table[0]` - `lw v1, -0x6c90(0x801D0000)` at `0x801EEE50` loads
//!   the *first* pointer of `0x801C9370` - while every write goes to
//!   `actor_table[1]` (`lw v1, 4(s0)`, a fixed `+4`). So this is a healer
//!   watching the party leader, not an actor watching itself.
//! * **The seat pair is fixed.** The `0xC8` gauge seed is gated on
//!   `DAT_8007BD11 == 4` (`lbu v0, 1(a0)` at `0x801EEE10`) and lands on
//!   `actor_table[1]`, so the companion is battle seat `1` and the watched
//!   leader is seat `0`.
//! * **The physical arm is not "a 1-2 hit stream of `0x0C`/`0x0D`/`0x0E`".**
//!   It first rolls a target - `rand() % ctx[+1] + 3`, redrawn while the
//!   drawn seat's `+0x14C` is zero - then reads that monster's **record**
//!   byte `+0x1E` through `0x801C9348[target - 3]`. On `+0x1E == 2` the
//!   stream is the single command `0x0E`; otherwise it is exactly two
//!   commands, each an independent `rand() % 2 + 0x0C`.
//!
//! The magic arms are as documented: category `2`, target byte `0`, spell id
//! `0x16` / `0x0D` / `0x11`, and `+0x1E7 = 9` on every one of the three
//! (`0x801EEEF8`..`0x801EEF08`). The coin flip is `rand() & 1`, and its zero
//! half writes `+0x1DE = 0` - the actor stands by.
//!
//! The roster id the block keys on is
//! [`crate::battle_action::AI_COMPANION_CHAR_ID`], already carried beside
//! `FUN_801DBA04`'s selectable-target scan, which tests the same byte.

/// Battle seat the block writes (`lw v1, 4(0x801C9370)`).
pub const AI_COMPANION_SEAT: u8 = 1;

/// Battle seat whose HP and status the three magic conditions read
/// (`lw v1, 0(0x801C9370)`).
pub const AI_COMPANION_WATCHED_SEAT: u8 = 0;

/// The gauge value the block seeds onto seat 1 when `DAT_8007BD11 == 4`
/// (`+0x174`, `+0x150`, `+0x172` and `+0x14C`, all `0xC8`).
pub const AI_COMPANION_GAUGE_SEED: u16 = 0xC8;

/// Spell the block casts when the watched seat is at zero HP.
pub const AI_COMPANION_REVIVE_SPELL: u8 = 0x16;
/// Spell it casts when the watched seat is below half HP.
pub const AI_COMPANION_HEAL_SPELL: u8 = 0x0D;
/// Spell it casts when the watched seat carries any status bit.
pub const AI_COMPANION_CURE_SPELL: u8 = 0x11;

/// The `+0x1E7` byte every magic arm writes beside the spell id.
pub const AI_COMPANION_MAGIC_SUB_ROUTE: u8 = 9;

/// The single command the physical arm queues against a monster whose record
/// byte `+0x1E` is `2`.
pub const AI_COMPANION_GUARDED_COMMAND: u8 = 0x0E;

/// The monster-record byte `+0x1E` value that selects the single-command
/// stream.
pub const AI_COMPANION_GUARDED_RECORD_BYTE: u8 = 2;

/// Base of the two-command stream: each byte is `rand() % 2 + 0x0C`.
pub const AI_COMPANION_COMMAND_BASE: u8 = 0x0C;

/// First monster seat, the base the target roll adds
/// (`addiu v1, v1, 3` at `0x801EEF74`).
pub const AI_COMPANION_FIRST_MONSTER_SEAT: u8 = 3;

/// How many target redraws the port allows. Retail's loop
/// (`beqz v0, 0x801eef2c` at `0x801EEF98`) is unbounded and spins forever if
/// every monster seat is dead; the port bounds it and stands by instead.
pub const AI_COMPANION_MAX_TARGET_DRAWS: usize = 16;

/// What the block wrote into the companion's action bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiCompanionPick {
    /// `+0x1DE = 2`, `+0x1DD = 0`, `+0x1DF = spell`, `+0x1E7 = 9`.
    Magic {
        /// The spell id.
        spell: u8,
    },
    /// `+0x1DE = 3`, `+0x1DD = target`, then one or two command bytes into
    /// `+0x1DF` / `+0x1E0`.
    Attack {
        /// Battle seat the target roll settled on.
        target: u8,
        /// One command byte, or two.
        commands: [Option<u8>; 2],
    },
    /// `+0x1DE = 0` - the coin flip's zero half, and the fallback when no
    /// monster seat is alive.
    StandBy,
}

/// The watched seat's state the three magic conditions read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiCompanionWatch {
    /// `+0x14C` - current HP.
    pub hp: u16,
    /// `+0x14E` - the max the half-HP test divides.
    pub hp_max: u16,
    /// `+0x16E` - the status word; any non-zero bit selects the cure arm.
    pub status: u16,
}

/// Run the delegated pick.
///
/// `monster_count` is `ctx[+1]`, the divisor of the target roll.
/// `seat_alive` answers `actor_table[seat][+0x14C] != 0` for a rolled seat and
/// `guard_byte` answers that seat's monster-record `+0x1E`.
///
/// The RNG order is retail's, which matters because the whole battle shares
/// one cursor: the coin flip first, then one draw per target attempt, then
/// the two command draws (and those only when the guard byte is not `2`).
///
/// PORT: FUN_801EED1C (the `DAT_8007BD10[slot] == 4` delegated-pick block, `0x801EEE40..0x801EF038`)
pub fn ai_companion_pick(
    watch: AiCompanionWatch,
    monster_count: u8,
    seat_alive: impl Fn(u8) -> bool,
    guard_byte: impl Fn(u8) -> u8,
    mut rand: impl FnMut() -> u32,
) -> AiCompanionPick {
    if watch.hp == 0 {
        return AiCompanionPick::Magic {
            spell: AI_COMPANION_REVIVE_SPELL,
        };
    }
    if watch.hp < watch.hp_max >> 1 {
        return AiCompanionPick::Magic {
            spell: AI_COMPANION_HEAL_SPELL,
        };
    }
    if watch.status != 0 {
        return AiCompanionPick::Magic {
            spell: AI_COMPANION_CURE_SPELL,
        };
    }
    if rand() & 1 == 0 {
        return AiCompanionPick::StandBy;
    }
    if monster_count == 0 {
        return AiCompanionPick::StandBy;
    }
    let mut target = None;
    for _ in 0..AI_COMPANION_MAX_TARGET_DRAWS {
        let seat = ((rand() % u32::from(monster_count)) as u8)
            .wrapping_add(AI_COMPANION_FIRST_MONSTER_SEAT);
        if seat_alive(seat) {
            target = Some(seat);
            break;
        }
    }
    let Some(target) = target else {
        return AiCompanionPick::StandBy;
    };
    if guard_byte(target) == AI_COMPANION_GUARDED_RECORD_BYTE {
        return AiCompanionPick::Attack {
            target,
            commands: [Some(AI_COMPANION_GUARDED_COMMAND), None],
        };
    }
    let a = (rand() % 2) as u8 + AI_COMPANION_COMMAND_BASE;
    let b = (rand() % 2) as u8 + AI_COMPANION_COMMAND_BASE;
    AiCompanionPick::Attack {
        target,
        commands: [Some(a), Some(b)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(_: u8) -> bool {
        true
    }

    #[test]
    fn the_three_magic_arms_are_ordered_by_severity() {
        let w = |hp, hp_max, status| AiCompanionWatch { hp, hp_max, status };
        // Zero HP wins over everything, including a status bit.
        assert_eq!(
            ai_companion_pick(w(0, 200, 0xFF), 3, alive, |_| 0, || 1),
            AiCompanionPick::Magic {
                spell: AI_COMPANION_REVIVE_SPELL
            }
        );
        // Strictly below half - `sltu a0, (max >> 1)`.
        assert_eq!(
            ai_companion_pick(w(99, 200, 0), 3, alive, |_| 0, || 1),
            AiCompanionPick::Magic {
                spell: AI_COMPANION_HEAL_SPELL
            }
        );
        assert_ne!(
            ai_companion_pick(w(100, 200, 0), 3, alive, |_| 0, || 0),
            AiCompanionPick::Magic {
                spell: AI_COMPANION_HEAL_SPELL
            }
        );
        assert_eq!(
            ai_companion_pick(w(200, 200, 1), 3, alive, |_| 0, || 1),
            AiCompanionPick::Magic {
                spell: AI_COMPANION_CURE_SPELL
            }
        );
    }

    #[test]
    fn the_coin_flips_zero_half_stands_by() {
        let w = AiCompanionWatch {
            hp: 200,
            hp_max: 200,
            status: 0,
        };
        assert_eq!(
            ai_companion_pick(w, 3, alive, |_| 0, || 0),
            AiCompanionPick::StandBy
        );
    }

    #[test]
    fn the_physical_arm_rolls_a_target_then_two_commands() {
        let w = AiCompanionWatch {
            hp: 200,
            hp_max: 200,
            status: 0,
        };
        // coin flip = 1, target roll = 1 (-> seat 4), then 0 and 1.
        let mut draws = [1u32, 1, 0, 1].into_iter();
        let pick = ai_companion_pick(w, 3, alive, |_| 0, || draws.next().unwrap_or(0));
        assert_eq!(
            pick,
            AiCompanionPick::Attack {
                target: 4,
                commands: [
                    Some(AI_COMPANION_COMMAND_BASE),
                    Some(AI_COMPANION_COMMAND_BASE + 1)
                ],
            }
        );
    }

    #[test]
    fn a_guarded_monster_gets_one_command_and_no_extra_draws() {
        let w = AiCompanionWatch {
            hp: 200,
            hp_max: 200,
            status: 0,
        };
        let mut draws = [1u32, 0].into_iter();
        let pick = ai_companion_pick(
            w,
            3,
            alive,
            |_| AI_COMPANION_GUARDED_RECORD_BYTE,
            || draws.next().expect("no third draw for a guarded target"),
        );
        assert_eq!(
            pick,
            AiCompanionPick::Attack {
                target: AI_COMPANION_FIRST_MONSTER_SEAT,
                commands: [Some(AI_COMPANION_GUARDED_COMMAND), None],
            }
        );
    }

    #[test]
    fn a_dead_seat_is_redrawn_and_an_all_dead_row_stands_by() {
        let w = AiCompanionWatch {
            hp: 200,
            hp_max: 200,
            status: 0,
        };
        // Seat 3 is dead, seat 4 is not: the first draw is discarded.
        let mut draws = [1u32, 0, 1, 0, 0].into_iter();
        let pick = ai_companion_pick(
            w,
            3,
            |seat| seat != AI_COMPANION_FIRST_MONSTER_SEAT,
            |_| 0,
            || draws.next().unwrap_or(0),
        );
        assert!(matches!(pick, AiCompanionPick::Attack { target: 4, .. }));

        // Nothing alive: retail spins, the port stands by.
        let pick = ai_companion_pick(w, 3, |_| false, |_| 0, || 1);
        assert_eq!(pick, AiCompanionPick::StandBy);
    }
}
