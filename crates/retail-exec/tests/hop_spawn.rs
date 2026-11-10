//! The two arc-hop spawners of PROT 0897, with the allocator answering a
//! cleared record per call: the player's ledge hop `FUN_801D2404` against
//! `build_hop_arc`, and the field VM's scripted arc `FUN_801D25EC` against
//! `spawn_arc_with_emitter` - the arc helper's Bezier block, cursor and step,
//! and the release watcher it chains, including its tear-down of the helper
//! when the second allocation fails.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::field_ledge_hop_arc::{
    HopArc, HopTarget, build_hop_arc, spawn_arc_with_emitter,
};
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const LEDGE: u32 = 0x801D_2404;
const SCRIPTED: u32 = 0x801D_25EC;
const ALLOC: u32 = 0x8002_0DE0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8013_0400;
const ARC: u32 = 0x8013_1000;
const SECOND: u32 = 0x8013_2000;
const TARGET: u32 = 0x8013_3000;
const STACK_TOP: u32 = 0x801F_FF00;

/// Hand out `ARC`, then `SECOND` unless `fail_second`, each cleared.
fn allocator(cpu: &mut Cpu) -> (Rc<Cell<u32>>, Rc<Cell<bool>>) {
    let calls = Rc::new(Cell::new(0u32));
    let fail = Rc::new(Cell::new(false));
    let (c, f) = (calls.clone(), fail.clone());
    cpu.hook(ALLOC, move |r, m| {
        let n = c.get();
        c.set(n + 1);
        let at = match n {
            0 => ARC,
            _ if f.get() => 0,
            _ => SECOND,
        };
        if at != 0 {
            m.load(at, &[0u8; 0x100]);
        }
        r.r[V0] = at;
    });
    (calls, fail)
}

fn read_arc(cpu: &mut Cpu) -> HopArc {
    let h = |cpu: &mut Cpu, o: u32| cpu.mem.rd16(ARC + o) as i16;
    HopArc {
        start: (h(cpu, 0x14), h(cpu, 0x16), h(cpu, 0x18)),
        end: (h(cpu, 0x24), h(cpu, 0x26), h(cpu, 0x28)),
        control: (h(cpu, 0x3C), h(cpu, 0x3E), h(cpu, 0x40)),
        cursor: h(cpu, 0x9C),
        step: h(cpu, 0x9E),
        paired_frames: 0,
    }
}

fn seat(cpu: &mut Cpu, at: u32, rng: &mut Rng) -> (i16, i16, i16) {
    let p = (
        rng.next() as i16 >> rng.below(4),
        rng.next() as i16 >> rng.below(6),
        rng.next() as i16 >> rng.below(4),
    );
    cpu.mem.wr16(at + 0x14, p.0 as u16);
    cpu.mem.wr16(at + 0x16, p.1 as u16);
    cpu.mem.wr16(at + 0x18, p.2 as u16);
    cpu.mem.wr16(at + 0x1A, rng.next() as u16);
    p
}

fn target(cpu: &mut Cpu, rng: &mut Rng) -> HopTarget {
    let t = HopTarget {
        x: rng.next() as i16 >> rng.below(4),
        y: rng.next() as i16 >> rng.below(6),
        z: rng.next() as i16 >> rng.below(4),
    };
    cpu.mem.wr16(TARGET, t.x as u16);
    cpu.mem.wr16(TARGET + 2, t.y as u16);
    cpu.mem.wr16(TARGET + 4, t.z as u16);
    t
}

fn frames(rng: &mut Rng) -> i16 {
    [0x10i16, 1, 3, 0x30, 0, -4, 0x1001][rng.below(7) as usize]
}

#[test]
fn the_ledge_hop_spawns_the_arc_retail_spawns() {
    differential!(prot(897, LEDGE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let (calls, _) = allocator(&mut cpu);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x484F_5031);
    for round in 0..20_000 {
        let start = seat(&mut cpu, PLAYER, &mut rng);
        let flags = rng.next() & !0x8_0000;
        cpu.mem.wr32(PLAYER + 0x10, flags);
        let t = target(&mut cpu, &mut rng);
        let apex = [0x10i16, 0x18, 0, -0x20, rng.next() as i16][rng.below(5) as usize];
        let n = frames(&mut rng);
        calls.set(0);
        cpu.regs.r[V0] = 0xDEAD_BEEF;
        let stop = cpu.call(LEDGE, &[TARGET, apex as i32 as u32, n as i32 as u32]);
        assert_eq!(stop, Stop::Returned);
        let mut port = build_hop_arc(start, t, apex, n);
        port.paired_frames = 0;
        let label = format!("round {round} start {start:?} {t:?} apex {apex} frames {n}");
        assert_eq!(read_arc(&mut cpu), port, "{label}");
        assert_eq!(cpu.mem.rd32(ARC + 0x90), PLAYER, "{label}");
        // The second record holds the raw count; the player takes the walk lock.
        assert_eq!(cpu.mem.rd16(SECOND + 0x9E) as i16, n, "{label}");
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags | 0x8_0000, "{label}");
    }
}

#[test]
fn the_scripted_arc_spawns_what_retail_spawns() {
    differential!(prot(897, SCRIPTED));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let (calls, fail) = allocator(&mut cpu);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x484F_5032);
    let (mut player, mut torn) = (0, 0);
    for round in 0..20_000 {
        let is_player = rng.below(3) == 0;
        let src = if is_player { PLAYER } else { ACTOR };
        let start = seat(&mut cpu, src, &mut rng);
        let t = target(&mut cpu, &mut rng);
        let apex = [0x10i16, 0x18, 0x40, -0x20][rng.below(4) as usize];
        let n = frames(&mut rng);
        let (release_ctx, mask, class) = (
            rng.next(),
            [0x400u32, rng.next()][rng.below(2) as usize],
            rng.next() as u8,
        );
        cpu.mem.wr32(STACK_TOP + 0x10, release_ctx);
        cpu.mem.wr32(STACK_TOP + 0x14, mask);
        cpu.mem.wr32(
            STACK_TOP + 0x18,
            u32::from(class) | rng.next() & 0xFFFF_FF00,
        );
        let fails = rng.below(8) == 0;
        fail.set(fails);
        calls.set(0);
        cpu.regs.r[V0] = 0xDEAD_BEEF;
        let stop = cpu.call(
            SCRIPTED,
            &[src, TARGET, apex as i32 as u32, n as i32 as u32],
        );
        assert_eq!(stop, Stop::Returned);
        let label = format!("round {round} {t:?} apex {apex} frames {n}");
        let spawn =
            spawn_arc_with_emitter(Some(start), is_player, t, apex, n, release_ctx, mask, class)
                .expect("a source actor");
        let mut arc = spawn.arc;
        arc.paired_frames = 0;
        assert_eq!(read_arc(&mut cpu), arc, "{label}");
        if fails {
            // The helper is torn down and the spawn returns null.
            assert_eq!(cpu.regs.r[V0], 0, "{label}");
            assert_eq!(cpu.mem.rd32(ARC + 0x10) & 8, 8, "{label}");
            torn += 1;
            continue;
        }
        let e = spawn.emitter.expect("emitter");
        // The routine hands back the watcher, the last record it allocated.
        assert_eq!(cpu.regs.r[V0], SECOND, "{label}");
        assert_eq!(cpu.mem.rd32(SECOND + 0x90), ARC, "{label}");
        assert_eq!(cpu.mem.rd32(SECOND + 0x94), e.release_ctx, "{label}");
        assert_eq!(cpu.mem.rd32(SECOND + 0x74), e.release_mask, "{label}");
        assert_eq!(cpu.mem.rd16(SECOND + 0x5C), e.class, "{label}");
        assert_eq!(cpu.mem.rd16(SECOND + 0x9E) as i16, e.frames, "{label}");
        assert_eq!(
            cpu.mem.rd16(SECOND + 0x50) != 0,
            e.owner_is_player,
            "{label}"
        );
        player += u32::from(e.owner_is_player);
    }
    assert!(player > 3000 && torn > 1000, "{player} {torn}");
}
