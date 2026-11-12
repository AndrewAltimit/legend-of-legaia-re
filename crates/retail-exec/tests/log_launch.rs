//! The commit log's launch record: the leaf `FUN_801DB7B0` (PROT 0898) that
//! registers a tracked-widget glide, then the stepper `FUN_801D9BBC` walking
//! it frame by frame, against `LogLaunch::new`, `LogLaunch::step` and
//! `LogLaunch::seat_dy`.
//!
//! The element spawner `FUN_801D8DE8` calls the leaf at `0x801D93D8` with
//! `total = ctx[+0x1C]` (the round reset seeds `0x10`), the glide flag
//! `ctx[+0x1B]`, the target seat, and the element's handle in
//! `ctx[+0x1074 + slot * 4]`, whose `+0x0A / +0x0C` the leaf copies as the
//! start. Each frame the record's `total` / `elapsed` bytes and the position
//! the stepper writes back into the handle compare with the port's record.

use crate::common::{Rng, STACK_TOP, scus_cpu};
use legaia_engine_battle_vm::battle_commit_log::{LAUNCH_GLIDE_FRAMES, LogLaunch};
use psx_retail_exec::Stop;

const LEAF: u32 = 0x801D_B7B0;
const STEPPER: u32 = 0x801D_9BBC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const HANDLES: u32 = CTX + 0x1074;
const RECORDS: u32 = CTX + 0x11B4;

#[test]
fn a_launch_registers_and_glides_as_retail_does() {
    differential!(prot(898, LEAF), prot(898, STEPPER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4C41_554E);
    let mut frames = 0u32;
    for round in 0..3000 {
        // Clear every record; one launch is live.
        for i in 0..0x28u32 {
            cpu.mem.wr8(RECORDS + i * 0xC, 0);
            cpu.mem.wr32(HANDLES + i * 4, 0);
        }
        let slot = rng.below(0x28);
        let handle = 0x8014_0000 + slot * 0x20;
        let (a, b) = (
            (rng.below(0x200) as i32 - 0x100) as i16,
            (rng.below(0x200) as i32 - 0x100) as i16,
        );
        let y = rng.below(0xF0) as i16;
        cpu.mem.wr32(HANDLES + slot * 4, handle);
        cpu.mem.wr16(handle + 0xA, a as u16);
        cpu.mem.wr16(handle + 0xC, y as u16);
        // The fifth argument, the target's y.
        cpu.mem.wr32(STACK_TOP + 0x10, y as u16 as u32);
        let inbound = rng.below(2) == 1;
        assert_eq!(
            cpu.call(
                LEAF,
                &[slot, u32::from(LAUNCH_GLIDE_FRAMES), 1, b as u16 as u32]
            ),
            Stop::Returned
        );
        let at = RECORDS + slot * 0xC;
        let mut port = LogLaunch::new(inbound);
        assert_eq!(
            (cpu.mem.rd8(at), cpu.mem.rd8(at + 1)),
            (port.total, port.elapsed),
            "round {round}: the record the leaf writes"
        );
        assert_eq!(cpu.mem.rd8(at + 2) as u32, slot);
        assert_eq!(cpu.mem.rd16(at + 8) as i16, a, "round {round}: the start");
        assert_eq!(cpu.mem.rd16(at + 4) as i16, b, "round {round}: the target");
        for frame in 0..40 {
            let step = 1 + rng.below(3) as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            assert_eq!(cpu.call(STEPPER, &[]), Stop::Returned);
            port.step(step);
            frames += 1;
            let what = format!("round {round} frame {frame} step {step}");
            let x = cpu.mem.rd16(handle + 0xA) as i16 as i32;
            assert_eq!(
                x - i32::from(b),
                LogLaunch::seat_dy(Some(&port), i32::from(a), i32::from(b)),
                "{what}: the position"
            );
            if port.settled() {
                assert_eq!(cpu.mem.rd8(at), 0, "{what}: retired");
                break;
            }
            assert_eq!(cpu.mem.rd8(at + 1), port.elapsed, "{what}");
        }
        assert!(port.settled(), "round {round}");
    }
    assert!(frames > 10_000, "{frames}");
}
