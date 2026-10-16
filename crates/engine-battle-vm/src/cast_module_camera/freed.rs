//! PROT 0912 (Freed)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::freed_tick`).

use super::body_paced::*;
use super::*;

/// The TR y global PROT 0912's arm 13 climbs to and holds at (`slti
/// v0,v0,0xc01` / `li v0,0xc00` at `0x801F7A60..0x801F7A6C`).
pub const FREED_RISE_CAP: i32 = 0xC00;

/// The TR y arm 11's cut leaves, where arm 13's climb starts.
const FREED_RISE_FROM: i32 = 0x600;

/// PROT 0912 (Freed). `c` is the creature seat `actor_table[7]` (`s2`) and
/// `P` the per-tick frame product. Arm 2 stores the creature's heading `0`
/// (`sh zero,0x46(s2)` at `0x801F6C54`) and nothing turns it after, so every
/// creature-relative yaw below is the literal less `0`.
///
/// | arm | when | camera |
/// |---:|---|---|
/// | 0 | its pass | cut: pitch `0x40`, TR `(0, 0x800, 0x1000)`, focus the caster (`0x801F6BD4`) |
/// | 2 | its pass, the creature seated | TR `(0, 0x600, 0x1200)`, focus `c`, over `0x80` frames (`0x801F6C78`) |
/// | 3 | its pass | TR `(0, 0x800, 0x800)`, focus `c`, over `0x40` frames (`0x801F6F24`) |
/// | 4 | its pass | cut: pitch `-0x40`, TR `(0, 0x800, 0x800)`, focus `c` (`0x801F7004`) |
/// | 5, 6 | every pass | yaw `+= P`, TR z `+= 2P` |
/// | 7 | every pass | yaw `+= P`, TR y `-= P`, TR z `+= 8P` |
/// | 8 | every pass | yaw `+= P`, TR z `-= 4P` |
/// | 8 | its pass | the spell name; pitch `0x40`, yaw `0x840`, TR `(0x100, 0x300, 0xA00)`, focus `c`, over `0x30` frames (`0x801F76E0`) |
/// | 10 | its pass | the attack name; cut: pitch `0x100`, yaw `0x980`, TR `(0, 0x300, 0x800)`, focus `c` (`0x801F783C`) |
/// | 11 | its pass | cut: yaw `0x800`, TR `(0x140, 0x600, 0x600)`, focus `c` (`0x801F78B0`) |
/// | 13 | every pass | TR y `+= 4P`, held at [`FREED_RISE_CAP`] |
/// | 13 | its pass | cut: pitch `-0x200`, yaw `0xA00`, TR `(0x200, 0x800, 0x100)`, focus `c` (`0x801F7AEC`) |
/// | 14 | every pass | TR y `+= 2P` |
/// | 14 | its pass | cut: yaw `0x800`, TR `(0, 0x2000, 0x600)`, focus the origin (`0x801F7BCC`) |
/// | 15 | every pass | TR z `-= P / 2` |
/// | 15 | its pass | pitch `0x180`, yaw `0x800`, TR `(0, 0, 0x1000)`, focus the origin, over `0x40` frames (`0x801F7E00`) |
/// | 16 | every pass | yaw `+= 2P` |
/// | 17 | every pass | yaw `+= 2P`, TR z `+= 4P`, pitch `-= P / 2` |
/// | 18 | every pass | TR z `+= 4P`, pitch `+= P / 4` |
/// | 19 | every pass | TR z `-= P` |
///
/// Arm 0x0B's wait (the creature's clip cursor reaching `0x49`) and arm
/// 0x13's (every enemy seat idle) are the body's, not camera waits: their
/// length follows the creature's and the monsters' animation rates.
///
/// PORT: FUN_801F69D8 (PROT 0912; every camera arm, the drifts of arms 5..8 and 13..19, and the arm 8/10 captions)
pub fn freed_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let caster = seats.caster;
    let p = MODULE_DRAIN_PER_TICK;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| shot([0x40, 0], [0, 0x800, 0x1000], focus_on(caster), 1),
            pass: |st, arm| {
                let c = focus_on(st.creature_live.or(st.creature).unwrap_or_default());
                match arm {
                    2 => shot([0, 0], [0, 0x600, 0x1200], c, 0x80),
                    3 => shot([0, 0], [0, 0x800, 0x800], c, 0x40),
                    4 => shot([-0x40, 0], [0, 0x800, 0x800], c, 1),
                    8 => captioned(
                        shot([0x40, 0x840], [0x100, 0x300, 0xA00], c, 0x30),
                        ModuleCaption::SpellName,
                    ),
                    0x0A => captioned(
                        shot([0x100, 0x980], [0, 0x300, 0x800], c, 1),
                        ModuleCaption::AttackName,
                    ),
                    0x0B => shot([0, 0x800], [0x140, 0x600, 0x600], c, 1),
                    0x0D => shot([-0x200, 0xA00], [0x200, 0x800, 0x100], c, 1),
                    0x0E => shot([0, 0x800], [0, 0x2000, 0x600], [0; 3], 1),
                    0x0F => shot([0x180, 0x800], [0, 0, 0x1000], [0; 3], 0x40),
                    _ => ArmDirection::PASS,
                }
            },
            each: |_, arm, ticks| match arm {
                5 | 6 => drift(0, p, 0, 2 * p),
                7 => drift(0, p, -p, 8 * p),
                8 => drift(0, p, 0, -4 * p),
                0x0D => {
                    let at = FREED_RISE_FROM + 4 * p * ticks;
                    drift(0, 0, (FREED_RISE_CAP - at).clamp(0, 4 * p), 0)
                }
                0x0E => drift(0, 0, 2 * p, 0),
                0x0F => drift(0, 0, 0, -p / 2),
                0x10 => drift(0, 2 * p, 0, 0),
                0x11 => drift(-p / 2, 2 * p, 0, 4 * p),
                0x12 => drift(p / 4, 0, 0, 4 * p),
                0x13 => drift(0, 0, 0, -p),
                _ => ArmDirection::PASS,
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arm 13's climb stops at the cap however long the arm holds.
    #[test]
    fn freed_climb_holds_at_the_cap() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        freed_direct(&mut st, 0x0C, seats);
        let mut climbed = 0;
        for _ in 0..200 {
            climbed += i32::from(freed_direct(&mut st, 0x0D, seats).drift.unwrap().tr_y);
        }
        assert_eq!(climbed, FREED_RISE_CAP - FREED_RISE_FROM);
    }

    /// The pass shots land on the next arm's first tick, once.
    #[test]
    fn freed_pass_shots_land_once() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        assert!(freed_direct(&mut st, 0, seats).shot.is_some());
        freed_direct(&mut st, 8, seats);
        let d = freed_direct(&mut st, 9, seats);
        assert_eq!(d.shot.map(|s| s.frames), Some(0x30));
        assert_eq!(d.caption, Some(ModuleCaption::SpellName));
        assert!(freed_direct(&mut st, 9, seats).shot.is_none());
    }
}
