//! The hop arc helper's tick `FUN_801D5C08`, with the Bezier `FUN_801E45BC`
//! live beneath it, against `advance_hop_arc`: start, end, control and frame
//! rate shapes over a whole flight, the parent's position each tick and the
//! arrival.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::field_ledge_hop_arc::{HopArc, HopTarget, advance_hop_arc, build_hop_arc};
use psx_retail_exec::Stop;

const TICK: u32 = 0x801D_5C08;
const OVERLAY_BASE: u32 = 0x801C_E818;
const PLAYER_PTR: u32 = 0x8007_C364;
const HELPER: u32 = 0x8013_0000;
const PARENT: u32 = 0x8013_4000;
const FRAME_STEP: u32 = 0x1F80_0393;

fn wr3(cpu: &mut psx_retail_exec::Cpu, a: u32, v: (i16, i16, i16)) {
    cpu.mem.wr16(a, v.0 as u16);
    cpu.mem.wr16(a + 2, v.1 as u16);
    cpu.mem.wr16(a + 4, v.2 as u16);
}

#[test]
fn the_hop_arc_flies_as_retail_flies() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(PLAYER_PTR, PARENT);
    let mut rng = Rng(0x484F_5041);
    let (mut ticks, mut failures) = (0u32, Vec::new());
    for round in 0..8000 {
        let mut c = |s: u32| (rng.below(2 * s + 1) as i32 - s as i32) as i16;
        let start = (c(0x2000), -(c(0x200).abs()), c(0x2000));
        let target = HopTarget {
            x: c(0x2000),
            y: -(c(0x200).abs()),
            z: c(0x2000),
        };
        let apex = [0x10i16, 0x18, 0x40][rng.below(3) as usize];
        let frames = [0x10i16, 8, 7, 0x20, 3][rng.below(5) as usize];
        let step_scalar = 1 + rng.below(4) as u8;
        let mut arc: HopArc = build_hop_arc(start, target, apex, frames);

        cpu.mem.wr32(HELPER + 0x90, PARENT);
        wr3(&mut cpu, HELPER + 0x14, arc.start);
        wr3(&mut cpu, HELPER + 0x24, arc.end);
        wr3(&mut cpu, HELPER + 0x3C, arc.control);
        cpu.mem.wr16(HELPER + 0x9C, 0);
        cpu.mem.wr16(HELPER + 0x9E, arc.step as u16);
        cpu.mem.wr32(HELPER + 0x10, 0);
        cpu.mem.wr32(PARENT + 0x10, 0);
        cpu.mem.wr8(FRAME_STEP, step_scalar);
        for tick in 0..0x1100 {
            assert_eq!(cpu.call(TICK, &[HELPER]), Stop::Returned, "{round} {tick}");
            let retail = (
                cpu.mem.rd16(PARENT + 0x14) as i16,
                cpu.mem.rd16(PARENT + 0x16) as i16,
                cpu.mem.rd16(PARENT + 0x18) as i16,
                cpu.mem.rd32(HELPER + 0x10) & 8 != 0,
            );
            let p = advance_hop_arc(&mut arc, step_scalar);
            let port = (p.position.0, p.position.1, p.position.2, p.arrived);
            if retail != port && failures.len() < 12 {
                failures.push(format!(
                    "round {round} tick {tick} frames {frames} apex {apex} step {step_scalar}: \
                     retail {retail:?} port {port:?}"
                ));
                break;
            }
            ticks += 1;
            if p.arrived {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(ticks > 40_000, "{ticks}");
}
