//! The near-camera ghost pass `FUN_8004DC68`, with the bearing
//! `FUN_80019B28` live beneath it, against `camera_ghost_pass` called the
//! way `World` calls it (the retail rotation tables, the integer bearing
//! over clamped coordinates): camera yaws, depths and focuses, bodies near
//! the view axis and far off it, seats with and without a record, every
//! command-flow byte and action state the gates name, whole-side targets,
//! the run, the pre-emptive round and the battle's end. Each seat's `+0x08`
//! word is compared.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_action::{GhostInputs, GhostSlot, camera_ghost_pass};
use legaia_engine_system::rotation_lut::{RotationLut, retail_rotation_lut};
use psx_retail_exec::Stop;

const GHOST: u32 = 0x8004_DC68;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const YAW: u32 = 0x8007_B792;
const DIST: u32 = 0x8008_40C0;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const BATTLE_END: u32 = 0x8007_BD71;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;

#[test]
fn the_ghost_pass_fades_what_retail_fades() {
    differential!(scus(GHOST));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let lut = retail_rotation_lut();
    let mut rng = Rng(0x4748_4F53);
    let (mut set, mut cleared) = (0u32, 0u32);
    for n in 0..40_000 {
        let i = GhostInputs {
            yaw: rng.next() as u16,
            focus_x: rng.below(0x1000) as i32 - 0x800,
            focus_z: rng.below(0x1000) as i32 - 0x800,
            dist: match rng.below(3) {
                0 => rng.below(0x4000) as i32,
                _ => 0x800 + rng.below(0x1800) as i32,
            },
            acting: rng.below(7) as u8,
            target: match rng.below(4) {
                0 => 8 + rng.below(3) as u8,
                _ => rng.below(7) as u8,
            },
            category: rng.below(7) as u8,
            flow: match rng.below(4) {
                0 => [0x32u8, 0x6E, 0xFE, 0x64, 0x65, 0xFF, 0x1E, 0x1F][rng.below(8) as usize],
                _ => rng.next() as u8,
            },
            state: match rng.below(3) {
                0 => 0x28 + rng.below(7) as u8,
                1 => [0x0Au8, 0x0B, 0x0C][rng.below(3) as usize],
                _ => rng.next() as u8,
            },
            formation: rng.below(3) as u8,
            battle_end: if rng.below(4) == 0 { 0xFE } else { 0xFF },
            ctx_26b: rng.below(2) as u8,
        };
        cpu.mem.wr16(YAW, i.yaw);
        cpu.mem.wr32(DIST, i.dist as u32);
        cpu.mem.wr32(FOCUS_X, i.focus_x.wrapping_neg() as u32);
        cpu.mem.wr32(FOCUS_Z, i.focus_z.wrapping_neg() as u32);
        cpu.mem.wr8(CTX + 6, i.flow);
        cpu.mem.wr8(CTX + 7, i.state);
        cpu.mem.wr8(CTX + 0x13, i.acting);
        cpu.mem.wr8(CTX + 0x290, i.formation);
        cpu.mem.wr8(CTX + 0x26B, i.ctx_26b);
        cpu.mem.wr8(BATTLE_END, i.battle_end);
        let mut slots: Vec<GhostSlot> = (0..7)
            .map(|_| {
                let near = rng.below(2) == 0;
                let spread = if near { 0x200 } else { 0x1800 };
                GhostSlot {
                    present: rng.below(5) != 0,
                    x: (i.focus_x + rng.below(2 * spread) as i32 - spread as i32) as i16,
                    z: (i.focus_z + rng.below(2 * spread) as i32 - spread as i32) as i16,
                    flag_word: rng.next(),
                }
            })
            .collect();
        for (k, s) in slots.iter().enumerate() {
            let a = ACTOR + k as u32 * 0x400;
            cpu.mem.wr32(a + 0x22C, u32::from(s.present) * 0x8014_0000);
            cpu.mem.wr16(a + 0x34, s.x as u16);
            cpu.mem.wr16(a + 0x38, s.z as u16);
            cpu.mem.wr32(a + 0x08, s.flag_word);
        }
        let a = ACTOR + u32::from(i.acting) * 0x400;
        cpu.mem.wr8(a + 0x1DD, i.target);
        cpu.mem.wr8(a + 0x1DE, i.category);
        assert_eq!(cpu.call(GHOST, &[]), Stop::Returned, "n {n}");

        let before: Vec<u32> = slots.iter().map(|s| s.flag_word).collect();
        camera_ghost_pass(
            &i,
            &mut slots,
            |a| lut.b(a),
            |a| lut.a(a),
            |p1z, p1x, p2z, p2x| {
                let c = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                bearing_12bit_approx(c(p1z), c(p1x), c(p2z), c(p2x))
            },
        );
        for (k, s) in slots.iter().enumerate() {
            let retail = cpu.mem.rd32(ACTOR + k as u32 * 0x400 + 0x08);
            assert_eq!(retail, s.flag_word, "n {n} slot {k} {i:?} {:?}", slots[k]);
            if s.flag_word != before[k] {
                if s.flag_word & 0x8300_0000 == 0x8300_0000 {
                    set += 1;
                } else {
                    cleared += 1;
                }
            }
        }
    }
    assert!(set > 10_000 && cleared > 10_000, "{set} {cleared}");
}
