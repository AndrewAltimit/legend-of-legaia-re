//! Three small field-overlay (PROT 0897) routines against their ports: the
//! pooled menu-actor seed `FUN_801E5834` (`menu_actor_seed`), the clip pick
//! `FUN_801E58A8` (`actor_clip_pick`) and the reflection controller's spawn
//! `FUN_801E573C` (`spawn_controller`). The first two have no caller on the
//! disc; the third is op `4C 86`'s.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_field::field_actor_reflect::spawn_controller;
use legaia_engine_field::pooled_actor_seed::{actor_clip_pick, menu_actor_seed};
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SEED: u32 = 0x801E_5834;
const CLIP_PICK: u32 = 0x801E_58A8;
const REFLECT_SPAWN: u32 = 0x801E_573C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ALLOC: u32 = 0x8002_0DE0;
const MOVE_TABLE: u32 = 0x8002_04F8;
const STACK_TOP: u32 = 0x801F_FF00;
const ACTOR: u32 = 0x8013_0000;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

/// The allocator answering `ACTOR` (filled with junk) or null, checking the
/// descriptor and the pool head.
fn allocator(cpu: &mut Cpu, descriptor: u32, ok: Rc<Cell<bool>>, junk: Rc<Cell<u8>>) {
    let head = cpu.mem.rd32(0x8007_C34C);
    cpu.hook(ALLOC, move |r, m| {
        assert_eq!((r.r[A0], r.r[A1]), (descriptor, head));
        if ok.get() {
            m.load(ACTOR, &[junk.get(); 0x100]);
        }
        r.r[V0] = if ok.get() { ACTOR } else { 0 };
    });
}

#[test]
fn the_menu_actor_seed_matches_retail() {
    differential!(prot(897, SEED));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (ok, junk) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
    allocator(&mut cpu, 0x801F_2978, ok.clone(), junk.clone());
    let mut rng = Rng(0x5345_4544);
    for run in 0..5000 {
        let args: [u32; 4] = std::array::from_fn(|_| rng.next());
        ok.set(rng.below(8) != 0);
        junk.set(rng.next() as u8);
        assert_eq!(cpu.call(SEED, &args), Stop::Returned, "run {run}");
        if !ok.get() {
            continue;
        }
        let s = menu_actor_seed(
            args[0] as u16,
            args[1] as u16,
            args[2] as u16,
            args[3] as u16,
        );
        let mut port = vec![junk.get(); 0x100];
        for (off, v) in [
            (0x54, s.phase),
            (0x50, s.handler),
            (0x14, s.x),
            (0x16, s.y),
            (0x9C, s.param),
        ] {
            port[off..off + 2].copy_from_slice(&v.to_le_bytes());
        }
        let retail = cpu.mem.read(ACTOR, 0x100);
        assert_eq!(
            retail,
            port,
            "run {run}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
}

#[test]
fn the_clip_pick_matches_retail() {
    differential!(prot(897, CLIP_PICK));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const BASE: u32 = 0x8007_BDD8;
    const LEADER: u32 = 0x8007_B8F8;
    const OVERRIDE: u32 = 0x8007_B6AC;
    // The flag word the move-table consumer sees.
    let seen = Rc::new(Cell::new(None::<u32>));
    {
        let seen = seen.clone();
        cpu.hook(MOVE_TABLE, move |r, m| {
            assert_eq!(r.r[A0], ACTOR);
            seen.set(Some(m.rd32(ACTOR + 0x10)));
        });
    }
    let mut rng = Rng(0x434C_4950);
    let mut arms = [0u32; 4];
    for run in 0..20_000 {
        // `_DAT_8007BDD8` is read whole by the sentinel test and as a
        // halfword after it; the port's `u16` is the halfword, so the high
        // half stays clear here.
        let base = [99u16, 0, 1, 40, rng.next() as u16][rng.below(5) as usize];
        let leader =
            rng.below(4) as u16 | (rng.next() as u16 & 0xFFF0) * u16::from(rng.below(4) == 0);
        let over = [0u32, 0, 1, 3, rng.next()][rng.below(5) as usize];
        let flag_set = rng.below(2) == 0;
        let flags = rng.next() & !0x0100_0000 | if flag_set { 0x0100_0000 } else { 0 };
        cpu.mem.wr32(BASE, u32::from(base));
        cpu.mem.wr16(LEADER, leader);
        cpu.mem.wr32(OVERRIDE, over);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x5C, 0xA5A5);
        cpu.mem.wr16(ACTOR + 0x5E, 0xA5A5);
        seen.set(None);
        assert_eq!(cpu.call(CLIP_PICK, &[ACTOR]), Stop::Returned, "run {run}");
        let p = actor_clip_pick(base, leader, over, flag_set);
        let label =
            format!("run {run}: base {base} leader {leader} override {over:#x} flag {flag_set}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5C), p.clip, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x5E) as i16, -2, "{label}");
        let after = flags & !0x0100_0000 | if p.flag_set { 0x0100_0000 } else { 0 };
        assert_eq!(cpu.mem.rd32(ACTOR + 0x10), after, "{label}");
        // The consumer runs once, with the bit clear on the override arm.
        let during = if base != 99 && flag_set && over != 0 {
            flags & !0x0100_0000
        } else {
            after
        };
        assert_eq!(seen.get(), Some(during), "{label}");
        arms[if base == 99 {
            0
        } else if !flag_set {
            1
        } else if over != 0 {
            2
        } else {
            3
        }] += 1;
    }
    assert!(arms.iter().all(|&n| n > 1500), "{arms:?}");
}

#[test]
fn the_reflection_spawn_matches_retail() {
    differential!(prot(897, REFLECT_SPAWN));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (ok, junk) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
    allocator(&mut cpu, 0x801F_2948, ok.clone(), junk.clone());
    let mut rng = Rng(0x5245_464C);
    for run in 0..5000 {
        let words: [i16; 6] = std::array::from_fn(|_| rng.next() as i16);
        let (own, named) = (
            0x8013_1000 + rng.below(0x40) * 4,
            0x8013_2000 + rng.below(0x40) * 4,
        );
        for (i, w) in words[2..].iter().enumerate() {
            // Stacked words arrive as sign-extended words; the store is `sh`.
            cpu.mem.wr32(
                STACK_TOP + 0x10 + 4 * i as u32,
                i32::from(*w) as u32 ^ (rng.next() & 0xFFFF_0000),
            );
        }
        ok.set(rng.below(8) != 0);
        junk.set(rng.next() as u8);
        assert_eq!(
            cpu.call(
                REFLECT_SPAWN,
                &[own, named, words[0] as u16 as u32, words[1] as u16 as u32]
            ),
            Stop::Returned,
            "run {run}"
        );
        if !ok.get() {
            continue;
        }
        let c = spawn_controller(words);
        let mut port = vec![junk.get(); 0x100];
        port[0x90..0x94].copy_from_slice(&own.to_le_bytes());
        port[0x94..0x98].copy_from_slice(&named.to_le_bytes());
        port[0x54..0x56].copy_from_slice(&0u16.to_le_bytes());
        for (i, v) in [
            c.mirror_x,
            c.mirror_z,
            c.min_tile_x,
            c.min_tile_z,
            c.max_tile_x,
            c.max_tile_z,
        ]
        .iter()
        .enumerate()
        {
            port[0x80 + 2 * i..0x82 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        let retail = cpu.mem.read(ACTOR, 0x100);
        assert_eq!(
            retail,
            port,
            "run {run}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
}
