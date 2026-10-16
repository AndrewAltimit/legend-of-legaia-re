//! PROT 0910 (Swordie)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::swordie_tick`).

use super::*;

/// One `FUN_801D829C` call of PROT 0910 framing its own creature: focus on
/// the creature's seat, yaw `k - creature[+0x46]`.
fn on_creature(c: ModuleSeat, pitch: i16, k: i32, tr: [i16; 3], frames: u16) -> ArmDirection {
    ArmDirection::shot(ModuleShot {
        angles: [pitch, yaw_from(k, c.facing), 0],
        tr,
        focus: focus_on(c),
        frames,
    })
}

/// PROT 0910 (Swordie).
///
/// With `h` the heading from the victim to the caster turned half round
/// (`FUN_80019B28(victim, caster) + 0x800`): a one-frame cut at arm 0 -
/// pitch `0x40`, yaw `0x800 - h`, TR `(0, 0x300, 0xC00)`, on the point half
/// a unit from the victim along `h` (`0x801F6B38..0x801F6C08`). Arm 2, past
/// its CD poll, seats the creature on that same point facing `h`
/// (`jal 0x801F19EC`, then `+0x46 = h`, `+0x34 = victim.x - sin(h) / 2`,
/// `+0x38 = victim.z - cos(h) / 2` at `0x801F6D10..0x801F6DD4`): Swordie
/// stands between the victim and the caster, which is what the split panels
/// open onto.
///
/// The later framings are all on the creature (`f` its facing):
///
/// | Call | Arm | Pitch | Yaw | TR | Frames |
/// |---|---|---|---|---|---|
/// | `0x801F7194` | 5, on its pass | `0` | `0x900 - f` | `(0, 0x300, 0x400)` | 1 |
/// | `0x801F768C` | 6 | `-0x40` | `0xC00 - f` | `(-0x300, 0x300, 0x800)` | `0x100` |
/// | `0x801F7750` | 7, on its pass | `0x80` | `0x700 - f` | `(0, 0x400, 0x400)` | 1 |
/// | `0x801F77BC` | 8 | `0x60` | `0x820 - f` | `(0, 0x300, 0x2400)` | `0x80` |
///
/// and arm 9's pass (`0x801F79D8`) cuts onto the victim's body pair -
/// pitch `0x40`, yaw `-f`, TR `(0, 0x500, ctx[+0x6D0] / 2)`.
///
/// The director answers the phase the body is about to run and cannot see
/// whether an arm passes on this tick, so each call lands on the **first
/// tick of the next arm** - one tick late. A pass cut and the next arm's
/// tween are therefore a tick apart, as in retail, where the tween starts
/// from the cut. The countdown word, unused by this module's director,
/// holds the last phase answered (`phase + 1`, `0` before the first).
///
/// Not directed: arm 10's per-pass sway (`0x801F7B20`, a 12-frame tween
/// whose yaw steps through the table at `0x801F8D60`).
///
/// PORT: overlay_summon_swordie_0910_801f69ec (PROT 0910; camera arms 0, 5..9 and the arm 2 creature placement)
pub fn swordie_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let point = ModuleSeat {
        x: (i32::from(v.x) - half(sin)) as i16,
        y: 0,
        z: (i32::from(v.z) - half(cos)) as i16,
        facing: h,
    };
    let entered = st.countdown.0 != i32::from(phase) + 1;
    st.countdown.0 = i32::from(phase) + 1;
    let creature = st.creature_live.or(st.creature).unwrap_or(point);
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x40, yaw_from(0x800, h), 0],
            tr: [0, 0x300, 0xC00],
            focus: focus_on(point),
            frames: 1,
        }),
        2 => {
            st.creature = Some(point);
            ArmDirection::PASS
        }
        6 if entered => on_creature(creature, 0, 0x900, [0, 0x300, 0x400], 1),
        7 if entered => on_creature(creature, -0x40, 0xC00, [-0x300, 0x300, 0x800], 0x100),
        8 if entered => on_creature(creature, 0x80, 0x700, [0, 0x400, 0x400], 1),
        9 if entered => on_creature(creature, 0x60, 0x820, [0, 0x300, 0x2400], 0x80),
        SWORDIE_SETTLE_PHASE if entered => ArmDirection::shot(ModuleShot {
            angles: [0x40, yaw_from(0, creature.facing), 0],
            tr: [0, 0x500, (seats.depth_raw / 2) as i16],
            focus: focus_on(v),
            frames: 1,
        }),
        _ => ArmDirection::PASS,
    }
}

use crate::seru_magic_ticks::SWORDIE_SETTLE_PHASE;

#[cfg(test)]
mod tests {
    use super::*;

    /// Arm 2 seats Swordie half a unit from the victim toward the caster,
    /// and each later framing lands once, on the next arm's first tick.
    #[test]
    fn swordie_seats_between_victim_and_caster_and_frames_it_once() {
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                z: -2000,
                ..Default::default()
            },
            victim: ModuleSeat {
                z: 1000,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        swordie_direct(&mut st, 2, seats);
        let c = st.creature.expect("arm 2 seats the creature");
        assert_eq!(c.x, 0);
        assert!(c.z < 1000 && c.z > -2000, "between the two: {}", c.z);
        assert!(swordie_direct(&mut st, 6, seats).shot.is_some());
        assert!(swordie_direct(&mut st, 6, seats).shot.is_none());
        assert_eq!(
            swordie_direct(&mut st, 7, seats).shot.map(|s| s.frames),
            Some(0x100)
        );
    }
}
