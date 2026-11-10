//! The HUD's glide stepper `FUN_801D9BBC` against `SpriteGlide::step`: every
//! record shape over a walk of the forty handles at once, each with a handle
//! or none, written positions, deactivations and the stepper's return.

use crate::common::{Rng, scus_cpu};
use legaia_engine_minigames::muscle_dome::{GlideStep, SpriteGlide};
use psx_retail_exec::{Stop, V0};

const STEPPER: u32 = 0x801D_9BBC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const HANDLES: u32 = CTX + 0x1074;
const RECORDS: u32 = CTX + 0x11B4;

#[test]
fn the_glide_stepper_is_retails() {
    differential!(prot(898, STEPPER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x474C_4944);
    let (mut arrived, mut moved) = (0u32, 0u32);
    for round in 0..4000 {
        let step = 1 + rng.below(4) as u8;
        cpu.mem.wr8(FRAME_STEP, step);
        let mut ports: Vec<Option<SpriteGlide>> = Vec::new();
        for i in 0..0x28 {
            let at = RECORDS + i * 0xC;
            let live = rng.below(3) != 0;
            let total = if live { 1 + rng.below(0x30) as u8 } else { 0 };
            let elapsed = if total > 1 {
                rng.below(u32::from(total)) as u8
            } else {
                0
            };
            let (tx, ty, sx, sy) = (
                rng.next() as i16 >> 2,
                rng.next() as i16 >> 2,
                rng.next() as i16 >> 2,
                rng.next() as i16 >> 2,
            );
            let handle = if live && rng.below(10) == 0 {
                0
            } else {
                0x8014_0000 + i * 0x20
            };
            cpu.mem.wr8(at, total);
            cpu.mem.wr8(at + 1, elapsed);
            cpu.mem.wr8(at + 3, 1);
            cpu.mem.wr16(at + 4, tx as u16);
            cpu.mem.wr16(at + 6, ty as u16);
            cpu.mem.wr16(at + 8, sx as u16);
            cpu.mem.wr16(at + 0xA, sy as u16);
            cpu.mem.wr32(HANDLES + i * 4, handle);
            if handle != 0 {
                cpu.mem.wr16(handle + 0xA, 0x7777);
                cpu.mem.wr16(handle + 0xC, 0x7777);
            }
            ports.push((total != 0 && handle != 0).then_some(SpriteGlide {
                total,
                elapsed,
                target: (tx, ty),
                start: (sx, sy),
            }));
        }
        cpu.regs.r[V0] = 0;
        assert_eq!(cpu.call(STEPPER, &[]), Stop::Returned, "round {round}");
        let mut ret = 0u32;
        for (i, g) in ports.iter_mut().enumerate() {
            let at = RECORDS + i as u32 * 0xC;
            let handle = cpu.mem.rd32(HANDLES + i as u32 * 4);
            let Some(g) = g else {
                if handle == 0 {
                    assert_eq!(cpu.mem.rd8(at), 0, "round {round} slot {i}: no handle");
                }
                continue;
            };
            let total = g.total;
            let step_out = g.step(step);
            let written = (
                cpu.mem.rd16(handle + 0xA) as i16,
                cpu.mem.rd16(handle + 0xC) as i16,
            );
            match step_out {
                GlideStep::Moving { pos, remaining } => {
                    moved += 1;
                    assert_eq!(written, pos, "round {round} slot {i}");
                    // The stepper rewrites its return while its seed is set.
                    ret = remaining;
                    assert_eq!(cpu.mem.rd8(at), total);
                    assert_eq!(cpu.mem.rd8(at + 1), g.elapsed);
                }
                GlideStep::Arrived { pos } => {
                    arrived += 1;
                    assert_eq!(written, pos, "round {round} slot {i}");
                    assert_eq!(cpu.mem.rd8(at), 0);
                    if ret == 0 {
                        ret = 1;
                    }
                }
                GlideStep::Idle => unreachable!(),
            }
        }
        // The flag seeds `a3` from the first live record with a handle, and
        // an arrived record then leaves it alone.
        assert_eq!(cpu.regs.r[V0], ret, "round {round}: the return");
    }
    assert!(arrived > 3000 && moved > 30_000, "{arrived} {moved}");
}
