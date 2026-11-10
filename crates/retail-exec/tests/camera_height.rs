//! The swing camera's height pick `FUN_801F0348` against
//! `battle_formulas::camera_height_for_frame`: every acting seat and target
//! byte (the all-target `8` and stale bytes past it included) over monster
//! records whose size byte `+0x1F` spans the clamp's floor, its ceiling and
//! the whole byte.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::{RETAIL_MONSTER_SLOT_BASE, camera_height_for_frame};
use psx_retail_exec::Stop;

const HEIGHT: u32 = 0x801F_0348;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const ACTOR: u32 = 0x8013_0000;
const RECORD: u32 = 0x8014_0000;

#[test]
fn the_swing_camera_height_is_retails() {
    differential!(prot(898, HEIGHT));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    // Seats `3..=7` read records `0..=4`; a target byte of `8` or more reads
    // none (the outer gate).
    for m in 0..5 {
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, RECORD + m * 0x100);
    }
    let mut rng = Rng(0x4341_4D48);
    let mut seen = [0u32; 4];
    for _ in 0..40_000 {
        let sizes: Vec<u8> = (0..5)
            .map(|_| match rng.below(4) {
                0 => rng.below(0x18) as u8,
                1 => 0x18 + rng.below(0x10) as u8,
                _ => rng.next() as u8,
            })
            .collect();
        for (m, z) in sizes.iter().enumerate() {
            cpu.mem.wr8(RECORD + m as u32 * 0x100 + 0x1F, *z);
        }
        let attacker = rng.below(8) as u8;
        let target = match rng.below(5) {
            0 => 8,
            1 => 8 + rng.below(8) as u8,
            _ => rng.below(8) as u8,
        };
        cpu.mem.wr8(CTX + 0x13, attacker);
        cpu.mem
            .wr8(ACTOR + u32::from(attacker) * 0x400 + 0x1DD, target);
        cpu.mem.wr16(CTX + 0x6D0, rng.next() as u16);
        assert_eq!(cpu.call(HEIGHT, &[]), Stop::Returned);
        let retail = cpu.mem.rd16(CTX + 0x6D0) as i16;
        let port = camera_height_for_frame(attacker, target, RETAIL_MONSTER_SLOT_BASE, |s| {
            sizes[usize::from(s - 3)]
        });
        assert_eq!(
            retail, port,
            "attacker {attacker} target {target} {sizes:?}"
        );
        seen[match retail {
            _ if target >= 8 => 2,
            0x0C00 => 0,
            0x1400 => 1,
            _ => 3,
        }] += 1;
    }
    assert!(seen.iter().all(|&n| n > 500), "{seen:?}");
}
