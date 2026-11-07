//! PROT 0941's **Steal** body (`0x801F730C`, action `0x51`): the framing, the
//! caster's headings and the arm gates over [`crate::cast_arm_ticks::steal_tick`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_steal_0941_801f730c.txt`.

use super::capture::{CaptureCamArm, CaptureSeatWrite};
use super::{ModuleCamSeats, ModuleCamState, SPEED_SCALAR, bearing_12bit_approx};

/// PROT 0941's Steal tick body.
pub const STEAL_BODY: u32 = crate::cast_arm_ticks::STEAL_TICK;

/// **Steal** (a `beq` / `slti` chain over `0..=3` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`). Every arm
/// frames through the action SM's case 6 on the caster; the body seats no
/// record and calls no `FUN_801D829C`:
///
/// | arm | seats | word |
/// |---|---|---|
/// | 0 (`0x801F7400`) | heading onto the victim; the run clip | `= scalar << 5` |
/// | 1 (`0x801F759C`) | heading onto the victim; holds until `FUN_8004E2F0` reads it in reach; then its back to the victim, and the steal | - |
/// | 2 (`0x801F7B44`) | framing style `ctx[+0xD] = 0`; back to the victim through the gate; then onto the victim, clip `0` | `+= scalar << 6` |
/// | 3 (`0x801F7CB0`) | - | gate, to `0xFF` |
///
/// Arm 1's way out also steps the caster back along that heading until it
/// leaves reach (`0x801F7620..0x801F76E4`), by the stride of its clip record;
/// not ported, the seat stays where it arrived. The phase chain and the
/// outcome are [`crate::cast_arm_ticks::steal_tick`]'s; this director owns the
/// gates and the framing.
///
/// PORT: overlay_cast_steal_0941_801f730c (PROT 0941 `0x51`; the framing, headings and gates)
pub fn steal_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    // `FUN_80019B28(victim[+0x38], victim[+0x34], caster[+0x38], caster[+0x34])`.
    let bearing = bearing_12bit_approx(v.z, v.x, c.z, c.x) & 0xFFF;
    let onto = bearing.wrapping_add(0x800) & 0xFFF;
    let facing = |f: u16| CaptureSeatWrite {
        facing: Some(f),
        ..Default::default()
    };
    let mut arm = CaptureCamArm {
        follow_caster: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR << 5;
            arm.caster_write = facing(onto);
        }
        1 => {
            if st.caster_in_reach {
                arm.caster_write = facing(bearing);
            } else {
                arm.caster_write = facing(onto);
                arm.hold = true;
            }
        }
        2 => {
            // `sb zero,0xd(s2)` ahead of the arm's case 6 (`0x801F7B44`) on
            // every pass; the passing one stores the `1` back after it
            // (`0x801F7BF8`).
            arm.follow_style = Some(0);
            if st.countdown.drain_above(0) {
                arm.caster_write = facing(bearing);
                arm.hold = true;
            } else {
                arm.caster_write = facing(onto);
                st.countdown.add(1 << 6);
            }
        }
        3 => arm.hold = st.countdown.drain_above(0),
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 0,
                y: 0,
                z: 1000,
                facing: 0,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: -1000,
                facing: 0,
            },
            action: 0x51,
            ..Default::default()
        }
    }

    /// Arm 1 walks until in reach; arms 2 and 3 run their words' retail
    /// lengths: `32` and `64` vsyncs.
    #[test]
    fn the_arms_hold_on_reach_and_their_words() {
        let mut st = ModuleCamState::default();
        assert!(!steal_camera(&mut st, 0, seats()).hold);
        assert!(steal_camera(&mut st, 1, seats()).hold);
        st.caster_in_reach = true;
        assert!(!steal_camera(&mut st, 1, seats()).hold);
        let mut n = 1;
        while steal_camera(&mut st, 2, seats()).hold {
            n += 1;
        }
        assert_eq!(n, 32);
        let mut n = 1;
        while steal_camera(&mut st, 3, seats()).hold {
            n += 1;
        }
        assert_eq!(n, 64);
    }

    /// Every arm frames through case 6, and the headings turn.
    #[test]
    fn every_arm_follows_and_the_back_turns() {
        let mut st = ModuleCamState::default();
        let a = steal_camera(&mut st, 0, seats());
        assert!(a.follow_caster);
        st.caster_in_reach = true;
        let away = steal_camera(&mut st, 1, seats())
            .caster_write
            .facing
            .unwrap();
        let onto = a.caster_write.facing.unwrap();
        assert_eq!(onto.wrapping_add(0x800) & 0xFFF, away);
    }
}
