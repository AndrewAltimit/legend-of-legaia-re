//! The effect-placement clamp `FUN_801DF570` against the composition the
//! effect-script walker runs it as: the bearing from the target's seat pair
//! back to the attacker's live pair, a half-turn on, read off the rotation
//! table, then `approach_distance`. Positions near and far, both signs, and
//! requests negative, inside and either side of the `[3d/4, d]` band.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_approach::{ApproachPose, approach_angle, approach_distance};
use legaia_engine_system::rotation_lut::{RotationLut, retail_rotation_lut};
use psx_retail_exec::{Stop, V0};

const CLAMP: u32 = 0x801D_F570;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTORS: u32 = 0x801C_9370;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;

#[test]
fn the_approach_clamp_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    let (attacker, target) = (0x8013_0000u32, 0x8013_0400u32);
    let lut = retail_rotation_lut();
    let mut rng = Rng(0x4150_5052);
    let (mut floored, mut capped, mut passed) = (0, 0, 0);
    for round in 0..40_000 {
        let span = [0x40u32, 0x400, 0x2000, 0x7FFF][rng.below(4) as usize];
        let mut c = |s: u32| (rng.below(2 * s + 1) as i32 - s as i32) as i16;
        let pose = ApproachPose {
            x: c(span),
            z: c(span),
            ref_x: c(span),
            ref_z: c(span),
        };
        let requested = c(0x1000);
        let (slot, tslot) = (rng.below(7), 0);
        let tslot = if slot == tslot { 1 } else { tslot };
        cpu.mem.wr32(ACTORS + slot * 4, attacker);
        cpu.mem.wr32(ACTORS + tslot * 4, target);
        cpu.mem.wr8(attacker + 0x1DD, tslot as u8);
        cpu.mem.wr16(attacker + 0x34, pose.x as u16);
        cpu.mem.wr16(attacker + 0x38, pose.z as u16);
        cpu.mem.wr16(target + 0x3C, pose.ref_x as u16);
        cpu.mem.wr16(target + 0x40, pose.ref_z as u16);
        assert_eq!(
            cpu.call(CLAMP, &[slot, requested as u16 as u32]),
            Stop::Returned
        );
        let retail = cpu.regs.r[V0] as i16;
        let a = approach_angle(bearing_12bit_approx(pose.ref_z, pose.ref_x, pose.z, pose.x));
        let port = approach_distance(pose, requested, lut.b(a) as i16, lut.a(a) as i16);
        assert_eq!(retail, port, "round {round} {pose:?} requested {requested}");
        if port == requested {
            passed += 1;
        } else if port > requested {
            floored += 1;
        } else {
            capped += 1;
        }
    }
    assert!(
        floored > 2_000 && capped > 2_000 && passed > 200,
        "{floored} {capped} {passed}"
    );
}
