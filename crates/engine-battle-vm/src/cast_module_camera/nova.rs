//! PROT 0913 (Nova)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::nova_tick`).

use super::body_paced::*;
use super::*;
use legaia_engine_system::trig::trig12;

/// The heading arm `0x10` turns the victim through over its climb, as
/// retail's own passes add it up at the normal speed scalar: frame `k` of
/// the climb raises the countdown to `8k` (`(scalar * delta) / 2` a pass) and
/// takes `sin(8k) / 2` off the victim's `+0x46` (`0x801F7FE8..0x801F8038`),
/// until the pass that carries the countdown past `0x400`. The engine does
/// not spin the victim, so the framings that read its heading after the
/// climb (arms `0x11` and `0x12`) take it off the heading the victim kept.
pub fn nova_victim_spin() -> u16 {
    let mut spin: i32 = 0;
    let mut c = 0i32;
    loop {
        c += (SPEED_SCALAR * 2) / 2;
        let (sin, _) = trig12((c & 0xFFF) as u16);
        spin += i32::from(sin) / 2;
        if c > 0x400 {
            break;
        }
    }
    (spin & 0xFFF) as u16
}

/// PROT 0913 (Nova). `c` is the creature seat `actor_table[7]` (`s8`), `f`
/// its heading, `v` the victim `actor_table[caster[+0x1DD]]` (`s2`), `d` the
/// framing depth `ctx[+0x6D0]` and `P` the per-tick frame product.
///
/// | arm | when | camera |
/// |---:|---|---|
/// | 0 | its pass | cut: pitch `0x400`, yaw `0x900 - caster[+0x46]`, TR `(0, 0, 0x400)`, focus the caster (`0x801F6C50`) |
/// | 1, 2 | every pass | TR z `+= P / 4` |
/// | 2 | its pass | pitch `0x80`, yaw `0x900 - f`, TR `(0, 0x300, 0x800)`, focus `c`, over `0x40` frames (`0x801F6DB8`) |
/// | 3 | every pass | yaw `+= 8P` - one whole turn over the arm |
/// | 3 | its pass | yaw global `= 0x900 - f` (`0x801F6EB8`) |
/// | 4 | every pass | TR z `+= P / 4`, yaw `+= P / 4` |
/// | 4 | its pass | yaw global `= 0xA40 - f`, then pitch `-0x100`, yaw `0x940 - f`, TR `(0, 0x600, 0x400)`, focus `c`, over `0x40` frames (`0x801F6FE8`) |
/// | 5 | its pass | yaw `0x900 - f`, TR `(0, 0x800, 0x800)`, focus `c`, over `0x20` frames |
/// | 6 | its pass | the spell name; pitch `-0x80`, yaw `0x900 - f`, TR `(0, 0x400, 0x800)`, focus `c`, over `0x20` frames |
/// | 7 | its pass | the attack name; cut: pitch `-0x80`, yaw `0x700 - f`, TR `(0, 0x400, 0x600)`, focus `c` |
/// | 8 | its pass | pitch `-0x40`, yaw `0x780 - f`, TR `(0, 0x600, 0x400)`, focus `c`, over `0x80` frames |
/// | 9 | its pass | pitch `0x40`, yaw `0x800 - f`, TR `(0, 0x600, 0x800)`, focus `c`, over `0x80` frames |
/// | 11 | every pass | TR y `+= 4P`, TR z `+= 2P`, pitch `-= P / 4` |
/// | 12, 13 | its pass / every pass | pitch `-0x200`, yaw `0x800 - f`, TR `(0, 0x1000, 0x200)`, focus `c`, over `0x10` frames |
/// | 13 | its pass | the globals stored straight: pitch `-0x200`, yaw `0x800 - v[+0x46]`, TR y `0x1000`, TR z `0x200` (already scaled), focus `v` (`0x801F7B10..0x801F7B5C`) |
/// | 14 | its pass | pitch `0x40`, yaw `0x800 - v[+0x46]`, TR `(0, 0x600, d)`, focus `v`, over `0x40` frames |
/// | 15 | its pass | pitch `-0x300`, yaw `0x800 - v[+0x46]`, TR `(0, d / 2, -d / 2)`, focus `v`, over `0x100` frames |
/// | 16 | every pass | yaw `+= sin(n / 2) / 2`, `n` the climbing countdown (`0x801F8068..0x801F80A4`) |
/// | 17 | its pass | cut: pitch `-0x300`, yaw `-v[+0x46]`, TR `(0, 4d, 0)`, focus `v` |
/// | 18 | its pass | pitch `0x40`, yaw `0x800 - v[+0x46]`, TR `(0, 0x600, d)`, focus `v`, over `0x80` frames |
/// | 20 | every pass | `FUN_801D5854(caster, 8)`, the post-action framing |
///
/// Arm 0x10's yaw add is a fixed sine step a **pass**, not a multiple of the
/// frame product, so the engine's two ticks a battle frame each take half.
///
/// PORT: FUN_801F69F0 (PROT 0913; every camera arm, the drifts of arms 1..4, 11 and 16, the yaw stores of arms 3/4/13 and the arm 6/7 captions)
pub fn nova_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let caster = seats.caster;
    let v = seats.victim;
    let d = seats.depth_raw as i16;
    let p = MODULE_DRAIN_PER_TICK;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| {
                shot(
                    [0x400, yaw_from(0x900, caster.facing)],
                    [0, 0, 0x400],
                    focus_on(caster),
                    1,
                )
            },
            pass: |st, arm| {
                let cr = st.creature_live.or(st.creature).unwrap_or_default();
                let c = focus_on(cr);
                let f = cr.facing;
                let vf = if arm >= 0x11 {
                    v.facing.wrapping_sub(nova_victim_spin()) & 0xFFF
                } else {
                    v.facing
                };
                let on_v = |pitch: i16, k: i32, tr: [i16; 3], frames: u16| {
                    shot([pitch, yaw_from(k, vf)], tr, focus_on(v), frames)
                };
                match arm {
                    2 => shot([0x80, yaw_from(0x900, f)], [0, 0x300, 0x800], c, 0x40),
                    3 => set_yaw(st, i32::from(yaw_from(0x900, f))),
                    4 => ArmDirection {
                        drift: set_yaw(st, i32::from(yaw_from(0xA40, f))).drift,
                        ..shot([-0x100, yaw_from(0x940, f)], [0, 0x600, 0x400], c, 0x40)
                    },
                    5 => shot([0, yaw_from(0x900, f)], [0, 0x800, 0x800], c, 0x20),
                    6 => captioned(
                        shot([-0x80, yaw_from(0x900, f)], [0, 0x400, 0x800], c, 0x20),
                        ModuleCaption::SpellName,
                    ),
                    7 => captioned(
                        shot([-0x80, yaw_from(0x700, f)], [0, 0x400, 0x600], c, 1),
                        ModuleCaption::AttackName,
                    ),
                    8 => shot([-0x40, yaw_from(0x780, f)], [0, 0x600, 0x400], c, 0x80),
                    9 => shot([0x40, yaw_from(0x800, f)], [0, 0x600, 0x800], c, 0x80),
                    0x0C => shot([-0x200, yaw_from(0x800, f)], [0, 0x1000, 0x200], c, 0x10),
                    // The stores land the scaled TR z `0x200` itself: the
                    // raw depth the tween builder scales to it.
                    0x0D => on_v(-0x200, 0x800, [0, 0x1000, NOVA_SNAP_TR_Z_RAW], 1),
                    0x0E => on_v(0x40, 0x800, [0, 0x600, d], 0x40),
                    0x0F => on_v(-0x300, 0x800, [0, d / 2, d.wrapping_neg() / 2], 0x100),
                    0x11 => on_v(-0x300, 0, [0, d.wrapping_mul(4), 0], 1),
                    0x12 => on_v(0x40, 0x800, [0, 0x600, d], 0x80),
                    // The hit's `jal 0x801F44A0` is followed by
                    // `FUN_800319A8(0)` (`0x801F8460`), inside the victim's
                    // targetable branch (`+0x16E & 4` clear, `0x801F8434`).
                    0x13 => captioned(ArmDirection::PASS, ModuleCaption::Cleared),
                    _ => ArmDirection::PASS,
                }
            },
            each: |st, arm, ticks| match arm {
                1 | 2 => drift(0, 0, 0, p / 4),
                3 => drift(0, 8 * p, 0, 0),
                4 => drift(0, p / 4, 0, p / 4),
                0x0B => drift(-p / 4, 0, 4 * p, 2 * p),
                0x0D => {
                    let cr = st.creature_live.or(st.creature).unwrap_or_default();
                    shot(
                        [-0x200, yaw_from(0x800, cr.facing)],
                        [0, 0x1000, 0x200],
                        focus_on(cr),
                        0x10,
                    )
                }
                0x10 => {
                    // The body's countdown climbs `P / 2` a tick from the
                    // `0` arm 0x0F left.
                    let n = (p / 2) * (ticks + 1);
                    let (sin, _) = trig12(((n / 2) & 0xFFF) as u16);
                    drift(0, i32::from(sin) / 4, 0, 0)
                }
                0x14 => ArmDirection {
                    end_frame: true,
                    ..ArmDirection::PASS
                },
                _ => ArmDirection::PASS,
            },
        },
    )
}

/// The raw TR z that scales to the `0x200` arm 0x0D stores into the scaled
/// global: `(0x140 << 8) / 0xA0 = 0x200`.
const NOVA_SNAP_TR_Z_RAW: i16 = 0x140;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nova_snap_depth_scales_to_the_stored_global() {
        assert_eq!(
            crate::battle_cam_script::prescale_tr_z(i32::from(NOVA_SNAP_TR_Z_RAW)),
            512.0
        );
    }

    /// Arm 3's spin is one whole turn at the engine's arm length.
    #[test]
    fn nova_arm3_turns_once() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        nova_direct(&mut st, 2, seats);
        let mut yaw = 0;
        for _ in 0..64 {
            yaw += i32::from(nova_direct(&mut st, 3, seats).drift.unwrap().yaw);
        }
        assert_eq!(yaw, 0x1000);
    }

    /// The pass after arm 0x10 frames the victim on the heading the climb
    /// turned it to.
    #[test]
    fn nova_frames_the_victim_after_its_spin() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        nova_direct(&mut st, 0x11, seats);
        let s = nova_direct(&mut st, 0x12, seats)
            .shot
            .expect("arm 0x11's cut");
        assert_eq!(
            s.angles[1],
            yaw_from(0, nova_victim_spin().wrapping_neg() & 0xFFF)
        );
    }
}
