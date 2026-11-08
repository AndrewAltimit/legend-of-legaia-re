//! PROT 0952's **Bloody Horns** body (`0x801F7118`, action `0x5C`): the camera
//! arms, the slowed wind-up, and the victim thrown up and dropped.
//!
//! See `ghidra/scripts/funcs/overlay_cast_bloody_horns_0952_801f7118.txt`.

use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR,
    bearing_12bit_approx, focus_on,
};

/// PROT 0952's Bloody Horns tick body.
pub const BLOODY_HORNS_BODY: u32 = crate::cast_module_ticks::BLOODY_HORNS_TICK;

/// The module's countdown word (`lui 0x801F` / `0x7BA8`).
pub const BLOODY_HORNS_COUNTDOWN: u32 = 0x801F_7BA8;

/// Rates both seats take on arm 2 (`li v1,0x1` at `0x801F749C`), and the full
/// rate arms 3 and 5 hand back (`li v0,0x8`).
const SLOW_RATE: u8 = 1;
const FULL_RATE: u8 = 8;
/// The height the victim's saved pair takes on arm 4's exit (`li v0,-0x1400` at
/// `0x801F77B4`), which arm 5 walks back down at eight drains a pass.
const THROW_TOP: i16 = -0x1400;

/// The shot every arm from 3 on re-aims each pass.
fn sweep(on: [i16; 3], facing: u16, cd: i32, base: i32, far: i32) -> ModuleShot {
    ModuleShot {
        angles: [0x40, (cd / 8 + base - i32::from(facing as i16)) as i16, 0],
        tr: [0, 0x500, (far - cd / 2) as i16],
        focus: on,
        frames: 0xC,
    }
}

/// The two pairs each pass writes back: the victim's as arm 0 saved it
/// (`0x801F7BB0`), the caster's as arm 2 saved it (`0x801F7BB8`). The clips
/// carry root motion the arms hold off.
fn pin(st: &ModuleCamState, arm: &mut CaptureCamArm) {
    let [cx, cz, vx, vz] = st.horns_pin;
    arm.caster_write.x = Some(cx);
    arm.caster_write.z = Some(cz);
    arm.victim_write.x = Some(vx);
    arm.victim_write.z = Some(vz);
}

/// **Bloody Horns** (`sltiu 7` through the table at `0x801F69F0`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0, 1 (`0x801F71D0`, `0x801F7334`) | the walk into reach, framed by case 6 (the host's approach leg) | - |
/// | 2 (`0x801F7414`) | both seats' rate `1`, the caster on clip `9`; on the caster: pitch `0x40`, yaw `0xA00 - facing`, TR `(0, 0x500, 0x600)`, `0xC` frames | `= scalar << 6` |
/// | 3 (`0x801F752C`) | every pass on the caster: yaw base `0x9C0`, TR z `0x700 - cd/2`, clip `0xA` next. On the gate: the caster back to full rate, the victim's flinch at rate `1` | `+= scalar << 6` |
/// | 4 (`0x801F7678`) | every pass on the victim: yaw base `0x980`, TR z `0x800 - cd/2`, the victim rising `3 * drain`. On the gate: the height to `-0x1400` | `+= scalar * 0x50` |
/// | 5 (`0x801F77D4`) | the same: yaw base `0x930`, TR z `0x940 - cd/2`, falling `8 * drain`. On the gate: the hit | `+= scalar * 0x60` |
/// | 6 (`0x801F79DC`) | on the victim's body: yaw base `0x8D0`, TR z `0xAC0 - cd/2`; ends on the gate and the victim's settle | drained twice a pass |
///
/// The phase chain is [`crate::cast_module_ticks::bloody_horns_tick`].
///
/// PORT: overlay_cast_bloody_horns_0952_801f7118 (PROT 0952 `0x5C`; the camera arms and the seat writes)
pub fn bloody_horns_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (mut c, mut v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    // Arm 0 saves the victim's pair (`0x801F7218..0x801F723C`) and arms 0 to 2
    // write it back (`0x801F7334..0x801F7358`, `0x801F73E8..0x801F7408`,
    // `0x801F743C..0x801F7460`). Arm 0 runs one pass; a host that enters on
    // arm 1 saves there.
    if phase <= 1 && st.pin_saved & 1 == 0 {
        st.horns_pin[2] = v.x;
        st.horns_pin[3] = v.z;
        st.pin_saved |= 1;
    }
    if phase <= 2 && st.pin_saved & 1 != 0 {
        (v.x, v.z) = (st.horns_pin[2], st.horns_pin[3]);
        arm.victim_write.x = Some(v.x);
        arm.victim_write.z = Some(v.z);
    }
    // Arms 3 to 5 read both pairs after [`pin`] writes them.
    if (3..=5).contains(&phase) {
        let [cx, cz, vx, vz] = st.horns_pin;
        (c.x, c.z, v.x, v.z) = (cx, cz, vx, vz);
    }
    match phase {
        // `0x801F72EC..0x801F7328`.
        0 => arm.vram_move = Some(super::GIMARD_BREATH_CLUT_MOVE),
        2 => {
            // `0x801F7414..0x801F7438`: the caster's pair saved.
            st.horns_pin[0] = c.x;
            st.horns_pin[1] = c.z;
            if st.pin_saved & 1 == 0 {
                st.horns_pin[2] = v.x;
                st.horns_pin[3] = v.z;
            }
            st.pin_saved |= 3;
            st.horns_lift = 0;
            arm.caster_write = CaptureSeatWrite {
                rate: Some(SLOW_RATE),
                stage: Some(9),
                restage: true,
                ..Default::default()
            };
            arm.victim_write.rate = Some(SLOW_RATE);
            arm.shot = Some(ModuleShot {
                angles: [0x40, (0xA00 - i32::from(c.facing as i16)) as i16, 0],
                tr: [0, 0x500, 0x600],
                focus: focus_on(c),
                frames: 0xC,
            });
            st.countdown.0 = SPEED_SCALAR << 6;
        }
        3 => {
            let held = st.countdown.drain_above(0);
            arm.shot = Some(sweep(focus_on(c), c.facing, st.countdown.0, 0x9C0, 0x700));
            arm.caster_write.stage = Some(0x0A);
            pin(st, &mut arm);
            if held {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 6);
                arm.caster_write.rate = Some(FULL_RATE);
                arm.victim_write.rate = Some(SLOW_RATE);
                arm.victim_write.stage = Some(seats.victim_flinch);
                arm.victim_write.restage = true;
            }
        }
        4 => {
            let held = st.countdown.drain_above(0);
            st.horns_lift -= (3 * SPEED_SCALAR) as i16;
            arm.shot = Some(sweep(focus_on(v), c.facing, st.countdown.0, 0x980, 0x800));
            arm.victim_write.stage = Some(0);
            arm.victim_write.y = Some(st.horns_lift);
            pin(st, &mut arm);
            if held {
                arm.hold = true;
            } else {
                st.horns_lift = THROW_TOP;
                st.countdown.add(0x50);
            }
        }
        5 => {
            let held = st.countdown.drain_above(0);
            st.horns_lift += (8 * SPEED_SCALAR) as i16;
            arm.shot = Some(sweep(focus_on(v), c.facing, st.countdown.0, 0x930, 0x940));
            arm.victim_write.y = Some(st.horns_lift);
            pin(st, &mut arm);
            if held {
                arm.hold = true;
            } else {
                st.countdown.add(0x60);
                arm.victim_write.y = Some(0);
                arm.victim_write.rate = Some(FULL_RATE);
                // `0x801F79AC..0x801F79C4`: the victim turned off the caster,
                // so the knockdown carries it away.
                arm.victim_write.facing =
                    Some(bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF);
                arm.caster_write.stage = Some(0);
                arm.caster_write.loop_release = true;
                arm.folds_hit = true;
            }
        }
        6 => {
            // Two drains a pass, as Scythe Wind's last arm: one ahead of the
            // shot, one behind it while the word stays positive
            // (`0x801F7A84..0x801F7AB0`), then the wait on the victim's
            // clip.
            st.countdown.drain();
            let cd = st.countdown.0;
            let on = st.victim_body.unwrap_or(v);
            arm.shot = Some(sweep(focus_on(on), c.facing, cd, 0x8D0, 0xAC0));
            arm.hold = (cd > 0 && st.countdown.drain_above(0)) || !st.victim_settled;
            if !arm.hold {
                st.countdown.drain_rest(super::MODULE_DRAIN_PER_TICK);
            }
        }
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
                x: 388,
                y: 0,
                z: 575,
                facing: 0x600,
            },
            victim: ModuleSeat {
                x: 104,
                y: 0,
                z: 404,
                facing: 0,
            },
            victim_flinch: 0x0E,
            ..Default::default()
        }
    }

    #[test]
    fn the_victim_is_thrown_up_and_dropped_back_to_the_floor() {
        let mut st = ModuleCamState::default();
        bloody_horns_camera(&mut st, 2, seats());
        let mut left = 0;
        while bloody_horns_camera(&mut st, 3, seats()).hold {
            left += 1;
        }
        assert_eq!(left, 63, "scalar << 6 drains in 64 passes");
        let mut height = 0;
        loop {
            let a = bloody_horns_camera(&mut st, 4, seats());
            height = a.victim_write.y.unwrap_or(height);
            if !a.hold {
                break;
            }
        }
        assert_eq!(height, -1536, "three drains a pass over 64 passes");
        let a = loop {
            let a = bloody_horns_camera(&mut st, 5, seats());
            if !a.hold {
                break a;
            }
        };
        assert!(a.folds_hit);
        assert_eq!(a.victim_write.y, Some(0));
    }

    /// The victim's pair is the one arm 1 found, not where it stood by arm 2.
    #[test]
    fn the_victim_is_pinned_from_the_walk() {
        let mut st = ModuleCamState::default();
        bloody_horns_camera(&mut st, 1, seats());
        let mut drift = seats();
        drift.victim.z += 9;
        let a = bloody_horns_camera(&mut st, 2, drift);
        assert_eq!(a.victim_write.z, Some(404));
        assert_eq!(st.horns_pin, [388, 575, 104, 404]);
        let a = bloody_horns_camera(&mut st, 3, drift);
        assert_eq!(a.shot.unwrap().focus, [-388, 0, -575]);
        assert_eq!(a.victim_write.z, Some(404));
    }

    #[test]
    fn arm_three_holds_the_shot_on_its_law() {
        // Retail's recording: yaw and TR z on the arm's first pass walk back to
        // the base as the word drains.
        let mut st = ModuleCamState::default();
        bloody_horns_camera(&mut st, 2, seats());
        let shot = bloody_horns_camera(&mut st, 3, seats())
            .shot
            .expect("a shot");
        assert_eq!(shot.tr[2], (0x700 - (512 - 8) / 2) as i16);
    }
}
