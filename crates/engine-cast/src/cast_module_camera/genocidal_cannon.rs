//! PROT 0963's **Genocidal Cannon** body (`0x801F6A20`, action `0xB3`): the
//! camera arms, the record runs and the seat hides. The phase chain is
//! [`crate::cast_module_ticks::GENOCIDAL_CANNON_CHAIN`], its countdowns
//! [`super::capture_countdown::GENOCIDAL_CANNON`], which a director runs for
//! its body.
//!
//! See `ghidra/scripts/funcs/overlay_cast_genocidal_cannon_0963_801f6a20.txt`.

use super::capture_countdown::GENOCIDAL_CANNON;
use super::v_windhash::seat;
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat,
    ModuleShot, ModuleSpawn, SpawnAnchor, SpawnRecord, arm_countdown, bearing_12bit_approx,
    focus_on, yaw_from,
};

/// PROT 0963's tick body.
pub const GENOCIDAL_CANNON_BODY: u32 = crate::cast_module_ticks::GENOCIDAL_CANNON_CHAIN.body;

/// Arm 0's two `FUN_80021B04` records, on the stack trio `FUN_801DCEAC`
/// left (`0x801F6B38..0x801F6B64`); the second's handle goes to
/// `0x801F92A8`, which arm `0x0C` hides.
const ARM0_RECORDS: [u32; 2] = [0x801F_8490, 0x801F_84AC];

/// Arm 1's record at the origin (`0x801F6C9C..0x801F6CF0`).
const ARM1_RECORD: u32 = 0x801F_864C;

/// Arm 2's ten records at the origin (`0x801F6DBC..0x801F6EB0`).
const ARM2_RECORDS: [u32; 10] = [
    0x801F_8514,
    0x801F_8574,
    0x801F_85E0,
    0x801F_86C4,
    0x801F_873C,
    0x801F_87B4,
    0x801F_8D44,
    0x801F_8DB4,
    0x801F_8E24,
    0x801F_8E94,
];

/// Arm 3's three records on the caster (`a0 = caster + 0x34`, `a1 = caster +
/// 0x44`, `0x801F7028..0x801F7070`).
const ARM3_RECORDS: [u32; 3] = [0x801F_891C, 0x801F_89C8, 0x801F_8A2C];

/// The six records arms 7, 9 and `0x0B` each seat on the caster
/// (`0x801F72BC..0x801F7354`, `0x801F74A4..0x801F7548`,
/// `0x801F7694..0x801F7738`).
const PULSE_RECORDS: [u32; 6] = [
    0x801F_8A90,
    0x801F_8AF8,
    0x801F_8BA8,
    0x801F_8BF8,
    0x801F_8C5C,
    0x801F_8CD0,
];

/// Arm `0x0E`'s twelve records at the origin, in call order
/// (`0x801F7A28..0x801F7B44`); the sixth goes through `FUN_80021B04`.
const ARM_E_RECORDS: [u32; 12] = [
    0x801F_8F04,
    0x801F_8F5C,
    0x801F_8FA4,
    0x801F_8FEC,
    0x801F_904C,
    0x801F_91CC,
    0x801F_90AC,
    0x801F_90F4,
    0x801F_913C,
    0x801F_9184,
    0x801F_91E8,
    0x801F_9248,
];

/// The clip arm 2 stages on the caster (the chain's stage), whose action
/// entry `record[+0x4C + 8 * 4]` (`lw v1,0x6c(v0)`) arms 2 and 6 rewrite.
const CHARGE_CLIP: u8 = 8;

/// The tint word a shown seat takes (`lui 0x2008; ori 0x200`).
const SHOWN: u32 = 0x2008_0200;

/// A seat taken off the draw: tint `0`, render flag `0xFF`.
fn hide() -> CaptureSeatWrite {
    CaptureSeatWrite {
        colour: Some(0),
        render_flag: Some(0xFF),
        ..Default::default()
    }
}

/// A seat put back on the draw: tint [`SHOWN`], render flag `0`.
fn show() -> CaptureSeatWrite {
    CaptureSeatWrite {
        colour: Some(SHOWN),
        render_flag: Some(0),
        ..Default::default()
    }
}

/// A one-frame `FUN_801D829C` cut (`a3 = 1`).
fn cut(pitch: i16, yaw: i16, tr: [i16; 3], focus: [i16; 3]) -> Option<ModuleShot> {
    Some(ModuleShot {
        angles: [pitch, yaw, 0],
        tr,
        focus,
        frames: 1,
    })
}

fn drift(pitch: i16, yaw: i16, tr_y: i16, tr_z: i16) -> Option<CaptureDrift> {
    Some(CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x: 0,
    })
}

/// Records on the caster's `+0x34` trio and `+0x44` angles.
fn on_caster(records: &[u32], out: &mut [Option<ModuleSpawn>]) {
    for (o, &r) in out.iter_mut().zip(records) {
        *o = Some(ModuleSpawn {
            record: SpawnRecord::Module(r),
            anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
        });
    }
}

/// Records at the point `pos` on zero angles, the run split across the
/// pass's two spawn lists (the charge list first).
fn at_point(records: &[u32], pos: [i16; 3], arm: &mut CaptureCamArm) {
    let (charge, pass) = records.split_at(records.len().min(arm.charge_spawns.len()));
    for (o, &r) in arm.charge_spawns.iter_mut().zip(charge) {
        *o = Some(seat(r, pos, [0; 3]));
    }
    for (o, &r) in arm.pass_spawns.iter_mut().zip(pass) {
        *o = Some(seat(r, pos, [0; 3]));
    }
}

/// **Genocidal Cannon** (`sltiu 0x12` through the table at `0x801F69D8`;
/// caster `actor_table[ctx + 0x13]`, target code `caster[+0x1DD]`). Every
/// gate drains the word `0x801F92C8` by the frame step; every drift is a
/// per-frame-step write into the camera globals ahead of the gate:
///
/// | arm | every pass | as it passes |
/// |---|---|---|
/// | 0 (`0x801F6AD4`) | - | the caster turned on its target (the party row's centroid, `FUN_801DCEAC(8)`); two records on the negated centroid; a cut on the caster: yaw `0x800 - facing`, TR `(0, 0, 0x4E20)` |
/// | 1 (`0x801F6BD8`) | the party hidden; pitch `-8`, TR y `+16`, TR z `-256` | one record at the origin |
/// | 2 (`0x801F6D00`) | pitch `-8`, TR y `+16`, TR z `-256` | ten records at the origin |
/// | 3 (`0x801F6F4C`) | - | pool flushed; a cut on the caster: yaw `0x800 - facing`, TR `(0, 0xB00, 0x600)`; three records on it |
/// | 6 (`0x801F7110`) | TR y `-2`, TR z `+8` | pool flushed; a cut on the caster: pitch `-0x100`, yaw `0xA00 - facing`, TR `(0, 0xA00, 0x400)`; caster rate `3` |
/// | 7 (`0x801F7204`) | pitch `+11`, yaw `+24`, TR y `-4`, TR z `+64` | caster rate `4`; six records on it |
/// | 8 (`0x801F73C0`) | - | caster rate `3`; a cut on it: pitch `0x100`, yaw `0x700 - facing`, TR `(0, 0x600, 0xC00)`; pool flushed |
/// | 9 (`0x801F7464`) | yaw `+2` | caster rate `4`; the six records again |
/// | `0x0A` (`0x801F75A8`) | yaw `+2` | caster rate `3`; a cut on it: pitch `-0x20`, yaw `0x180 - facing`, TR `(0, 0x700, 0xA00)`; pool flushed |
/// | `0x0B` (`0x801F7658`) | yaw `+1` | caster rate `4`; the six records again |
/// | `0x0C` (`0x801F7838`) | yaw `+1` | pool flushed; the caster hidden, the party shown; a cut on the origin: yaw `0x800 - facing`, TR `(0, 0x400, 0x2000)` |
/// | `0x0D` (`0x801F799C`) | pitch `-2`, TR y `+40` | - |
/// | `0x0E` (`0x801F7A08`) | - | twelve records at the origin (the sweep is the chain's) |
/// | `0x0F` (`0x801F7D80`) | TR z `+24` | a cut on the origin: pitch `0x180`, yaw `-facing`, TR `(0, 0, 0x3C00)` |
/// | `0x10` (`0x801F7ED0`) | party rate `0`; TR z `+32` | pool flushed; a cut on the caster: yaw `0x800 - facing`, TR `(0, 0x700, 0xA00)`; the caster shown and its loop released, the party hidden |
/// | `0x11` (`0x801F80AC`) | TR z `+8` | the party shown at rate `8` |
///
/// Every cut is one frame. Arms 2 and 6 also move clip 8's loop bounds
/// (`+0x85` / `+0x86` of the action entry at `record[+0x6C]`) to `0x0C..0x11`
/// and `0x15..0x17`, and arm `0x11` spends the cannon latch (record `+0x1C =
/// 0x16`). The colour-wash bytes `ctx[+0x27A..+0x286]` / `+0x32C`, the armed
/// wash `FUN_8004695C(0x202020)` on arms 3 and 6, the fades, the pool flushes
/// and arm `0x0C`'s kill of arm 0's parked part are the presentation decode's
/// (`World::fire_cast_presentation`); the rumble words behind `0x800845C8` are
/// `cast_module_rumble`'s.
///
/// PORT: overlay_cast_genocidal_cannon_0963_801f6a20 (PROT 0963 `0xB3`; the camera arms, the record runs and the seat hides)
pub fn genocidal_cannon_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    // The per-pass halves, ahead of each arm's gate.
    match phase {
        1 => {
            arm.party_write = hide();
            arm.drift = drift(-8, 0, 16, -256);
        }
        2 => arm.drift = drift(-8, 0, 16, -256),
        6 => arm.drift = drift(0, 0, -2, 8),
        7 => arm.drift = drift(11, 24, -4, 64),
        9 | 0x0A => arm.drift = drift(0, 2, 0, 0),
        0x0B | 0x0C => arm.drift = drift(0, 1, 0, 0),
        0x0D => arm.drift = drift(-2, 0, 40, 0),
        0x0F => arm.drift = drift(0, 0, 0, 24),
        0x10 => {
            arm.party_write.rate = Some(0);
            arm.drift = drift(0, 0, 0, 32);
        }
        0x11 => arm.drift = drift(0, 0, 0, 8),
        _ => {}
    }
    if let Some(a) = arm_countdown(GENOCIDAL_CANNON, phase) {
        if a.holds(&mut st.countdown) {
            arm.hold = true;
            return arm;
        }
        a.pass(&mut st.countdown);
    }
    match phase {
        0 => {
            // `FUN_801DCEAC(8, sp+0x20, sp+0x18)` only for the party row
            // (`bne s1,8` at `0x801F6AE0`); the trio stays zero otherwise and
            // the caster turns on its victim seat.
            let row = st.target_aim.filter(|_| st.target_code == Some(8));
            let (onto, point) = match row {
                Some(a) => (
                    bearing_12bit_approx(
                        a.centroid_z.wrapping_neg(),
                        a.centroid_x.wrapping_neg(),
                        c.z,
                        c.x,
                    ),
                    [a.centroid_x, 0, a.centroid_z],
                ),
                None => (
                    bearing_12bit_approx(seats.victim.z, seats.victim.x, c.z, c.x),
                    [0; 3],
                ),
            };
            let facing = onto.wrapping_add(0x800) & 0xFFF;
            arm.caster_write.facing = Some(facing);
            for (o, &r) in arm.pass_spawns.iter_mut().zip(&ARM0_RECORDS) {
                *o = Some(seat(r, point, [0; 3]));
            }
            let turned = ModuleSeat { facing, ..c };
            arm.shot = cut(0, yaw_from(0x800, facing), [0, 0, 0x4E20], focus_on(turned));
        }
        1 => at_point(&[ARM1_RECORD], [0; 3], &mut arm),
        2 => {
            at_point(&ARM2_RECORDS, [0; 3], &mut arm);
            // `record[+0x6C]`, clip 8's action entry: `+0x85 = 0x0C`,
            // `+0x86 = 0x11` (`0x801F6ED4..0x801F6EF8`).
            arm.caster_clip_bounds = Some((CHARGE_CLIP, [0x0C, 0x11]));
        }
        3 => {
            arm.flushes_pool = true;
            arm.shot = cut(0, yaw_from(0x800, c.facing), [0, 0xB00, 0x600], focus_on(c));
            on_caster(&ARM3_RECORDS, &mut arm.charge_spawns);
        }
        6 => {
            arm.flushes_pool = true;
            arm.shot = cut(
                -0x100,
                yaw_from(0xA00, c.facing),
                [0, 0xA00, 0x400],
                focus_on(c),
            );
            arm.caster_write.rate = Some(3);
            // The same entry's window moved on: `0x15..0x17`
            // (`0x801F71CC..0x801F71F4`).
            arm.caster_clip_bounds = Some((CHARGE_CLIP, [0x15, 0x17]));
        }
        7 | 9 | 0x0B => {
            arm.caster_write.rate = Some(4);
            on_caster(&PULSE_RECORDS, &mut arm.charge_spawns);
        }
        8 => {
            arm.caster_write.rate = Some(3);
            arm.shot = cut(
                0x100,
                yaw_from(0x700, c.facing),
                [0, 0x600, 0xC00],
                focus_on(c),
            );
            arm.flushes_pool = true;
        }
        0x0A => {
            arm.caster_write.rate = Some(3);
            arm.shot = cut(
                -0x20,
                yaw_from(0x180, c.facing),
                [0, 0x700, 0xA00],
                focus_on(c),
            );
            arm.flushes_pool = true;
        }
        0x0C => {
            arm.flushes_pool = true;
            arm.caster_write = hide();
            arm.party_write = show();
            arm.shot = cut(0, yaw_from(0x800, c.facing), [0, 0x400, 0x2000], [0; 3]);
        }
        0x0E => at_point(&ARM_E_RECORDS, [0; 3], &mut arm),
        0x0F => {
            arm.shot = cut(0x180, yaw_from(0, c.facing), [0, 0, 0x3C00], [0; 3]);
        }
        0x10 => {
            arm.flushes_pool = true;
            arm.shot = cut(0, yaw_from(0x800, c.facing), [0, 0x700, 0xA00], focus_on(c));
            arm.caster_write = CaptureSeatWrite {
                loop_release: true,
                ..show()
            };
            arm.party_write = hide();
        }
        0x11 => {
            // `sb 0x16, 0x1c(record)` on the first monster record
            // (`0x801F80E8..0x801F80F8`): the cannon latch spent.
            arm.first_monster_record_1c = Some(0x16);
            arm.party_write = CaptureSeatWrite {
                rate: Some(8),
                ..show()
            };
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 0,
                y: 0,
                z: -1000,
                facing: 0,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: 1000,
                facing: 0x800,
            },
            ..Default::default()
        }
    }

    /// Arm 0 frames the caster from far out on its new facing, and the
    /// first gate holds for its `0x20` seed.
    #[test]
    fn the_cannon_opens_far_out_and_zooms_in() {
        let mut st = ModuleCamState::default();
        let a0 = genocidal_cannon_camera(&mut st, 0, seats());
        let shot = a0.shot.expect("arm 0 cuts");
        assert_eq!(shot.tr, [0, 0, 0x4E20]);
        assert_eq!(shot.frames, 1);
        assert!(a0.pass_spawns[0].is_some() && a0.pass_spawns[1].is_some());
        let a1 = genocidal_cannon_camera(&mut st, 1, seats());
        assert!(a1.hold, "arm 1 holds on the seed");
        assert_eq!(a1.drift.map(|d| d.tr_z), Some(-256));
        assert_eq!(a1.party_write.render_flag, Some(0xFF));
    }

    /// Arms 2 and 6 move clip 8's loop bounds; arm `0x11` spends the latch.
    #[test]
    fn the_charge_loop_moves_and_the_latch_is_spent() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 0;
        let a2 = genocidal_cannon_camera(&mut st, 2, seats());
        assert_eq!(a2.caster_clip_bounds, Some((CHARGE_CLIP, [0x0C, 0x11])));
        st.countdown.0 = 0;
        let a6 = genocidal_cannon_camera(&mut st, 6, seats());
        assert_eq!(a6.caster_clip_bounds, Some((CHARGE_CLIP, [0x15, 0x17])));
        st.countdown.0 = 0;
        let a11 = genocidal_cannon_camera(&mut st, 0x11, seats());
        assert_eq!(a11.first_monster_record_1c, Some(0x16));
    }

    /// Arm 2's ten records and arm `0x0E`'s twelve all go in.
    #[test]
    fn the_record_runs_fit_the_pass() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 0;
        let a2 = genocidal_cannon_camera(&mut st, 2, seats());
        let n = |a: &CaptureCamArm| {
            a.charge_spawns.iter().flatten().count() + a.pass_spawns.iter().flatten().count()
        };
        assert_eq!(n(&a2), ARM2_RECORDS.len());
        st.countdown.0 = 0;
        let ae = genocidal_cannon_camera(&mut st, 0x0E, seats());
        assert_eq!(n(&ae), ARM_E_RECORDS.len());
    }
}
