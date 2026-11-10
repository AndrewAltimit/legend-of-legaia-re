//! PROT 0955's **Melt Spray** body (`0x801F7FA4`, action `0x6F`): the shot on
//! the victim, its push and the two countdowns that pace the spray.
//!
//! See `ghidra/scripts/funcs/overlay_cast_white_shield_0955_801f7fa4.txt`.

use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, MODULE_DRAIN_PER_TICK, ModuleCamSeats,
    ModuleCamState, ModuleShot, bearing_12bit_approx, focus_on, yaw_from,
};

/// PROT 0955's Melt Spray tick body.
pub const MELT_SPRAY_BODY: u32 = crate::cast_module_ticks::MELT_SPRAY_TICK;

/// The clip arm 2 stages on the caster.
const SPRAY_CLIP: u8 = 7;

/// **Melt Spray** (a `beq` / `slti` chain over `0..=4` and `0xFF`). Arms 0 and
/// 1 are the walk-in the approach leg carries
/// ([`super::capture_approach_follows`]).
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 2 (`0x801F81C4`) | caster and victim turn on each other, the caster's spray clip `7` staged with the bump; a shot on the victim: pitch `-0x20`, yaw `0xE00 - facing`, TR `(0, 0x400, 0x700)`, `0xC` frames | `= scalar << 7` |
/// | 3 (`0x801F828C`) | every pass TR z `+product` and the staged byte cleared once clip `7` plays; the debuff on the gate is the tick's | `+= scalar << 6` |
/// | 4 (`0x801F85B0`) | the same drift; the gate, then the victim's settle | - |
///
/// The body seats no record (no `FUN_80021B04` / `FUN_80050ED4` call), so
/// nothing of the cell's image goes in ([`super::capture_director_seats_all`]).
///
/// PORT: overlay_cast_white_shield_0955_801f7fa4 (PROT 0955 `0x6F`; the camera arms and the arm pacing)
pub fn melt_spray_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let push = CaptureDrift {
        tr_z: MODULE_DRAIN_PER_TICK as i16,
        ..CaptureDrift::default()
    };
    match phase {
        2 => {
            // The turn reads the victim's display pair `+0x3C` / `+0x40`
            // (`lh a0,0x40(s0)` / `lh a1,0x3c(s0)` at `0x801F81CC`).
            let vb = st.victim_body.unwrap_or(v);
            let caster_facing =
                bearing_12bit_approx(vb.z, vb.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            let victim_facing = caster_facing.wrapping_add(0x800) & 0xFFF;
            // The spray clip, staged with the bump (`li v1,0x7` /
            // `sb v1,0x1da(s1)`, `0x801F8264..0x801F8274`).
            arm.caster_write = CaptureSeatWrite {
                facing: Some(caster_facing),
                stage: Some(SPRAY_CLIP),
                restage: true,
                ..Default::default()
            };
            arm.victim_write = CaptureSeatWrite {
                facing: Some(victim_facing),
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0xE00, victim_facing), 0],
                tr: [0, 0x400, 0x700],
                focus: focus_on(v),
                frames: 0xC,
            });
            st.countdown.0 = 0;
            st.countdown.add(1 << 7);
        }
        3 => {
            // Every pass clears the staged byte once the clip it names plays
            // (`0x801F828C..0x801F82A0`), so the spray runs out onto the idle.
            if st.caster_clip == Some(SPRAY_CLIP) {
                arm.caster_write.stage = Some(0);
            }
            arm.drift = Some(push);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            st.countdown.add(1 << 6);
        }
        4 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) || !st.victim_settled {
                arm.hold = true;
            }
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast_module_camera::ModuleSeat;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 900,
                y: 0,
                z: 100,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: 700,
                facing: 0,
            },
            action: 0x6F,
            ..Default::default()
        }
    }

    #[test]
    fn the_spray_holds_on_the_countdowns_and_ends_on_the_victims_settle() {
        let mut st = ModuleCamState::default();
        let shot = melt_spray_camera(&mut st, 2, seats()).shot.unwrap();
        assert_eq!(shot.tr, [0, 0x400, 0x700]);
        assert_eq!(shot.frames, 0xC);
        assert!(melt_spray_camera(&mut st, 3, seats()).hold);
        st.countdown.0 = 0;
        let pass = melt_spray_camera(&mut st, 3, seats());
        assert!(!pass.hold);
        assert!(st.countdown.0 > 0);
        st.countdown.0 = 0;
        assert!(melt_spray_camera(&mut st, 4, seats()).hold);
        st.countdown.0 = 0;
        st.victim_settled = true;
        assert!(!melt_spray_camera(&mut st, 4, seats()).hold);
    }
}
