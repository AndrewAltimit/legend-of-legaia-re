//! The map-install tail `FUN_8003ADAC` and the fade spawner `FUN_80024E80`
//! beneath it.
//!
//! The tail runs whole against `World::seat_map_install_tail`: the player's
//! movement-lock bit, the post-load hold `_DAT_8007B6B4`, system flag `0x1B`
//! (bit `0x10` of `0x8008575B`) and the two fades it spawns, each read as the
//! template and id the spawner receives and played out to its end against
//! the port's fade list.
//!
//! The spawner runs on both branches of its allocation (`FUN_80020DE0`
//! hooked): the id stamped into the template's last word and the block the
//! loader `FUN_80020B00` is handed, against `fade::spawn_fade`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_system::fade::{FadeState, FadeTemplate, spawn_fade};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const INSTALL_TAIL: u32 = 0x8003_ADAC;
const SPAWN_FADE: u32 = 0x8002_4E80;
const ALLOC: u32 = 0x8002_0DE0;
const FADE_LOAD: u32 = 0x8002_0B00;
const PLAYER_PTR: u32 = 0x8007_C364;
const HOLD: u32 = 0x8007_B6B4;
const FLAG_BYTE: u32 = 0x8008_575B;
const PLAYER: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const TEMPLATE: u32 = 0x8014_0000;

fn read_template(mem: &mut psx_retail_exec::Memory, at: u32) -> FadeTemplate {
    let w: Vec<i16> = (0..13).map(|i| mem.rd16(at + 2 * i) as i16).collect();
    FadeTemplate {
        kind: w[0],
        duration: w[1],
        start_rgb: [w[3], w[4], w[5]],
        end_rgb: [w[7], w[8], w[9]],
        mode: [w[10], w[11], w[12]],
    }
}

#[test]
fn the_map_install_tail_is_retails() {
    differential!(scus(INSTALL_TAIL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The spawner's two arguments, the template as the caller left it.
    let spawned = Rc::new(RefCell::new(Vec::new()));
    let s = spawned.clone();
    cpu.hook(SPAWN_FADE, move |r, m| {
        let mut t = read_template(m, r.r[A0]);
        // What the spawner stamps (`sh s2,0x18(s1)`).
        t.mode[2] = r.r[A1] as i16;
        s.borrow_mut().push(t);
        r.r[V0] = ACTOR;
    });
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x4D41_5049);
    let mut whites = 0;
    for case in 0..2000 {
        let flags = rng.next();
        let byte = rng.next() as u8;
        cpu.mem.wr32(PLAYER + 0x10, flags);
        cpu.mem.wr32(HOLD, rng.next());
        cpu.mem.wr8(FLAG_BYTE, byte);
        spawned.borrow_mut().clear();
        assert_eq!(cpu.call(INSTALL_TAIL, &[]), Stop::Returned);

        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        w.install_field_player(0);
        w.actors[0].move_state.flags = flags;
        for bit in 0..8u16 {
            if byte & (0x80 >> bit) != 0 {
                w.system_flag_set(0x18 + bit);
            }
        }
        w.seat_map_install_tail();

        assert_eq!(
            cpu.mem.rd32(PLAYER + 0x10),
            w.actors[0].move_state.flags,
            "case {case}: the player's flag word"
        );
        assert_eq!(cpu.mem.rd32(HOLD) as i32, w.post_load_hold());
        let after = cpu.mem.rd8(FLAG_BYTE);
        for bit in 0..8u16 {
            assert_eq!(
                after & (0x80 >> bit) != 0,
                w.system_flag_test(0x18 + bit),
                "case {case}: system flag {:#x}",
                0x18 + bit
            );
        }
        // Both fades to their ends, quad by quad.
        let mut retail: Vec<FadeState> = spawned.borrow().iter().map(FadeState::load).collect();
        assert_eq!(retail.len(), 2);
        for frame in 0..64 {
            let want: Vec<(u32, u8, u32)> = retail
                .iter()
                .filter(|f| f.visible())
                .map(|f| {
                    let [r, g, b] = f.rgb();
                    (
                        u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
                        f.kind as u8,
                        f.mode[2] as u32,
                    )
                })
                .collect();
            assert_eq!(w.screen_fade_draws(), want, "case {case} frame {frame}");
            retail.retain_mut(|f| f.step_vsyncs(1) || f.holds_at_end());
            w.lead_screen_fades(1);
        }
        assert!(w.screen_fade_draws().is_empty());
        whites += usize::from(byte & 0x10 != 0);
    }
    assert!(whites > 500, "{whites}");
}

#[test]
fn the_fade_spawner_is_retails() {
    differential!(scus(SPAWN_FADE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pool = Rc::new(RefCell::new((0u32, Vec::new())));
    let p = pool.clone();
    cpu.hook(ALLOC, move |r, _| {
        let mut p = p.borrow_mut();
        p.1.push((r.r[A0], r.r[A1]));
        r.r[V0] = p.0;
    });
    let loaded = Rc::new(RefCell::new(Vec::new()));
    let l = loaded.clone();
    cpu.hook(FADE_LOAD, move |r, m| {
        let t = read_template(m, r.r[A1]);
        l.borrow_mut().push((r.r[A0], r.r[A1], t));
    });
    cpu.mem.wr32(0x8007_C34C, 0x8015_0000);
    let mut rng = Rng(0x4641_4445);
    let (mut full, mut failures) = (0, Vec::new());
    for case in 0..4000 {
        let w: Vec<i16> = (0..13).map(|_| rng.next() as i16).collect();
        for (i, v) in w.iter().enumerate() {
            cpu.mem.wr16(TEMPLATE + 2 * i as u32, *v as u16);
        }
        let before = read_template(&mut cpu.mem, TEMPLATE);
        let id = rng.next() as i16;
        let exhausted = rng.below(4) == 0;
        pool.borrow_mut().0 = if exhausted { 0 } else { ACTOR };
        pool.borrow_mut().1.clear();
        loaded.borrow_mut().clear();
        assert_eq!(cpu.call(SPAWN_FADE, &[TEMPLATE, id as u32]), Stop::Returned);
        // The generic effect-actor list and its descriptor.
        assert_eq!(pool.borrow().1, vec![(0x8007_0674, 0x8015_0000)]);
        if exhausted {
            // No stamp and no load: the port's seat always takes the fade, so
            // this arm has no counterpart there.
            assert_eq!(cpu.regs.r[V0], 0);
            assert!(loaded.borrow().is_empty());
            assert_eq!(read_template(&mut cpu.mem, TEMPLATE), before);
            full += 1;
            continue;
        }
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let loaded = loaded.borrow();
        assert_eq!(loaded.len(), 1);
        let (block, from, stamped) = loaded[0];
        assert_eq!((block, from), (ACTOR + 0x7C, TEMPLATE));
        let mut seat = None;
        spawn_fade(&mut seat, &before, id);
        if seat != Some(FadeState::load(&stamped)) && failures.len() < 8 {
            failures.push(format!("case {case}: {before:?} id {id}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(full > 500, "{full}");
}

const SPAWN_AT: u32 = 0x8002_4C88;
const VIEW_TRANSFORM: u32 = 0x8003_D344;

/// The positioned spawn `FUN_80024C88` against `World::spawn_at_position`
/// (the move VM's allocator host): the two pool words it forwards, where the
/// node stands, the view transform it asks for, and the null it hands back
/// from a full pool.
#[test]
fn the_positioned_spawn_is_retails() {
    use legaia_engine_vm::actor_alloc::{ActorAllocatorHost, SpawnPosition};
    differential!(scus(SPAWN_AT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pool = Rc::new(RefCell::new((0u32, Vec::new())));
    let p = pool.clone();
    cpu.hook(ALLOC, move |r, _| {
        let mut p = p.borrow_mut();
        p.1.push((r.r[A0], r.r[A1]));
        r.r[V0] = p.0;
    });
    let transformed = Rc::new(RefCell::new(Vec::new()));
    let t = transformed.clone();
    cpu.hook(VIEW_TRANSFORM, move |r, _| {
        t.borrow_mut().push((r.r[A0], r.r[A1]));
    });
    let mut rng = Rng(0x5350_4154);
    let mut full = 0;
    for case in 0..4000 {
        let pos = SpawnPosition {
            x: rng.next() as i16,
            y: rng.next() as i16,
            z: rng.next() as i16,
        };
        let (a, b) = (rng.next(), rng.next());
        let exhausted = rng.below(5) == 0;
        for (i, v) in [pos.x, pos.y, pos.z].into_iter().enumerate() {
            cpu.mem.wr16(TEMPLATE + 2 * i as u32, v as u16);
        }
        cpu.mem.load(ACTOR, &[0xEE; 0x40]);
        pool.borrow_mut().0 = if exhausted { 0 } else { ACTOR };
        pool.borrow_mut().1.clear();
        transformed.borrow_mut().clear();
        assert_eq!(cpu.call(SPAWN_AT, &[TEMPLATE, a, b]), Stop::Returned);
        assert_eq!(pool.borrow().1, vec![(a, b)], "the pool pair is forwarded");

        let mut w = World::new();
        if exhausted {
            for actor in w.actors.iter_mut() {
                actor.active = true;
            }
        }
        let slot = w.spawn_at_position(pos, a, b);
        assert_eq!(cpu.regs.r[V0] != 0, slot.is_some(), "case {case}");
        let Some(slot) = slot else {
            assert!(transformed.borrow().is_empty());
            full += 1;
            continue;
        };
        let m = &w.actors[slot as usize].move_state;
        assert_eq!(
            [0x14, 0x16, 0x18].map(|o| cpu.mem.rd16(ACTOR + o) as i16),
            [m.world_x, m.world_y, m.world_z],
            "case {case}"
        );
        // Nothing else of the node's head is written here.
        assert_eq!(cpu.mem.read(ACTOR, 0x14), vec![0xEE; 0x14]);
        assert_eq!(cpu.mem.read(ACTOR + 0x1A, 0x26), vec![0xEE; 0x26]);
        assert_eq!(*transformed.borrow(), vec![(ACTOR + 0x14, ACTOR + 0x2C)]);
    }
    assert!(full > 500, "{full}");
}
