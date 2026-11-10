//! The reflection controller's tick `FUN_801E5154` (PROT 0897, the `4C 86`
//! handler) against `tick_reflection`: both ends' teardown, the source's
//! suppress bit, the tile rect, the anim-state copy on a matching set, and
//! the three mirror shapes.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_actor_reflect::{
    ReflectActor, ReflectController, ReflectOutcome, tick_reflection,
};
use psx_retail_exec::Stop;

const TICK: u32 = 0x801E_5154;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTRL: u32 = 0x8013_0000;
const DST: u32 = 0x8013_1000;
const SRC: u32 = 0x8013_2000;

fn seat(cpu: &mut psx_retail_exec::Cpu, at: u32, a: &ReflectActor) {
    cpu.mem.wr32(at + 0x10, a.flags);
    cpu.mem.wr16(at + 0x14, a.x as u16);
    cpu.mem.wr16(at + 0x16, a.y as u16);
    cpu.mem.wr16(at + 0x18, a.z as u16);
    cpu.mem.wr16(at + 0x26, a.facing as u16);
    cpu.mem.wr16(at + 0x5C, a.anim_cursor);
    cpu.mem.wr16(at + 0x64, a.anim_set as u16);
    cpu.mem.wr16(at + 0x68, a.anim_frame);
    cpu.mem.wr16(at + 0x6A, a.anim_timer);
}

fn read(cpu: &mut psx_retail_exec::Cpu, at: u32) -> ReflectActor {
    ReflectActor {
        flags: cpu.mem.rd32(at + 0x10),
        x: cpu.mem.rd16(at + 0x14) as i16,
        y: cpu.mem.rd16(at + 0x16) as i16,
        z: cpu.mem.rd16(at + 0x18) as i16,
        facing: cpu.mem.rd16(at + 0x26) as i16,
        anim_cursor: cpu.mem.rd16(at + 0x5C),
        anim_set: cpu.mem.rd16(at + 0x64) as i16,
        anim_frame: cpu.mem.rd16(at + 0x68),
        anim_timer: cpu.mem.rd16(at + 0x6A),
    }
}

#[test]
fn a_reflection_tick_writes_what_retail_writes() {
    differential!(prot(897, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(CTRL + 0x90, DST);
    cpu.mem.wr32(CTRL + 0x94, SRC);
    let mut rng = Rng(0x5245_464C);
    let mut seen = [0u32; 4];
    for round in 0..50_000 {
        let actor = |rng: &mut Rng| ReflectActor {
            flags: rng.next() & !0x0000_0108 & if rng.below(2) == 0 { !0x0100_0000 } else { !0 }
                | if rng.below(20) == 0 { 0x8 } else { 0 }
                | if rng.below(10) == 0 { 0x100 } else { 0 },
            x: rng.next() as i16 >> rng.below(4),
            y: rng.next() as i16,
            z: rng.next() as i16 >> rng.below(4),
            facing: rng.next() as i16,
            anim_cursor: rng.next() as u16,
            anim_set: rng.below(3) as i16,
            anim_frame: rng.next() as u16,
            anim_timer: rng.next() as u16,
        };
        let mut dst = actor(&mut rng);
        let src = actor(&mut rng);
        let tile = |rng: &mut Rng| rng.below(0x200) as i16 - 0x100;
        let (lx, lz) = (tile(&mut rng), tile(&mut rng));
        let mut ctrl = ReflectController {
            flags: rng.next() & !8,
            mirror_x: if rng.below(3) == 0 {
                0
            } else {
                rng.next() as i16
            },
            mirror_z: if rng.below(3) == 0 {
                0
            } else {
                rng.next() as i16
            },
            min_tile_x: lx,
            min_tile_z: lz,
            max_tile_x: lx + rng.below(0x200) as i16,
            max_tile_z: lz + rng.below(0x200) as i16,
        };
        seat(&mut cpu, DST, &dst);
        seat(&mut cpu, SRC, &src);
        cpu.mem.wr32(CTRL + 0x10, ctrl.flags);
        for (i, v) in [
            ctrl.mirror_x,
            ctrl.mirror_z,
            ctrl.min_tile_x,
            ctrl.min_tile_z,
            ctrl.max_tile_x,
            ctrl.max_tile_z,
        ]
        .iter()
        .enumerate()
        {
            cpu.mem.wr16(CTRL + 0x80 + i as u32 * 2, *v as u16);
        }
        assert_eq!(cpu.call(TICK, &[CTRL]), Stop::Returned);
        let out = tick_reflection(&mut ctrl, &mut dst, &src);
        seen[match out {
            ReflectOutcome::TornDown => 0,
            ReflectOutcome::Suppressed => 1,
            ReflectOutcome::OutOfRect => 2,
            ReflectOutcome::Reflected => 3,
        }] += 1;
        assert_eq!(read(&mut cpu, DST), dst, "round {round} {out:?}");
        assert_eq!(read(&mut cpu, SRC), src, "round {round} {out:?}");
        assert_eq!(
            cpu.mem.rd32(CTRL + 0x10),
            ctrl.flags,
            "round {round} {out:?}"
        );
    }
    assert!(seen.iter().all(|n| *n > 2000), "{seen:?}");
}
