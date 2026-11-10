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
    differential!(prot(897, TICK));
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

const BEZIER: u32 = 0x801E_45BC;

/// The quadratic Bezier `FUN_801E45BC(c, p0, p2, t)` called on its own
/// against `bezier_point`, over the whole `u16` parameter range the move
/// VM's `lhu` can hand it (so past `0x1000`, where the 32-bit products wrap).
#[test]
fn the_bezier_point_matches_retail() {
    differential!(prot(897, BEZIER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    use legaia_engine_vm::move_vm::bezier_point;
    const C: u32 = 0x8013_8000;
    const P0: u32 = 0x8013_8010;
    const P2: u32 = 0x8013_8020;
    let mut rng = Rng(0x4245_5A49);
    for round in 0..50_000 {
        let v = |rng: &mut Rng| -> [i16; 3] {
            std::array::from_fn(|_| rng.next() as i16 >> rng.below(12))
        };
        let (c, p0, p2) = (v(&mut rng), v(&mut rng), v(&mut rng));
        let t = match rng.below(4) {
            0 => rng.next() as u16,
            1 => [0, 0x800, 0x1000, 0xFFFF][rng.below(4) as usize],
            _ => rng.below(0x1001) as u16,
        };
        for (at, p) in [(C, c), (P0, p0), (P2, p2)] {
            for (i, x) in p.iter().enumerate() {
                cpu.mem.wr16(at + 2 * i as u32, *x as u16);
            }
            cpu.mem.wr16(at + 6, 0x5A5A);
        }
        assert_eq!(cpu.call(BEZIER, &[C, P0, P2, u32::from(t)]), Stop::Returned);
        let retail: [i16; 3] = std::array::from_fn(|i| cpu.mem.rd16(C + 2 * i as u32) as i16);
        assert_eq!(
            retail,
            bezier_point(c, p0, p2, t),
            "round {round}: c {c:?} p0 {p0:?} p2 {p2:?} t {t:#x}"
        );
        assert_eq!(
            cpu.mem.rd16(C + 6),
            0x5A5A,
            "round {round}: the pad halfword"
        );
    }
}
