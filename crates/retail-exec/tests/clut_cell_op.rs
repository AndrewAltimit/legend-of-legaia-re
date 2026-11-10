//! The field VM's `4C 61` CLUT-cell op emitter `FUN_801E4C58` (PROT 0897)
//! against `legaia_engine_field::clut_fx::ClutCellFxOp` and the world's
//! one-shot write (`World::spawn_clut_cell_fx` then `World::step_clut_fx`).
//!
//! `frames == 0` is the inline one-shot: retail's `MoveImage` (cell B onto
//! the destination) or `LoadImage` (a flat row of colour `B.x`) are applied
//! to a software VRAM and compared with the port's write on its own copy.
//! `frames != 0` spawns the fade actor, compared by its seat (the fade itself
//! is `FUN_801E4794`'s own differential).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::{ClutCellFxPhase, World};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use psx_tim::Vram;
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EMITTER: u32 = 0x801E_4C58;
const MOVE_IMAGE: u32 = 0x8005_8490;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const DRAW_SYNC: u32 = 0x8005_8104;
const SPAWN: u32 = 0x8002_0DE0;
const FADE_DESCRIPTOR: u32 = 0x801F_2918;
const EFFECT_LIST: u32 = 0x8007_C34C;

const OP: u32 = 0x8012_0000;
const CTX: u32 = 0x8012_1000;
const ACTOR: u32 = 0x8012_2000;

#[test]
fn the_clut_cell_op_matches_retail() {
    differential!(prot(897, EMITTER));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let vram = Rc::new(RefCell::new(Vram::new()));
    let spawned = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let v = vram.clone();
    cpu.hook(MOVE_IMAGE, move |r, m| {
        let rect = r.r[A0];
        let (sx, sy, w, h) = (m.rd16(rect), m.rd16(rect + 2), m.rd16(rect + 4), m.rd16(rect + 6));
        let (dx, dy) = (r.r[A1] as u16, r.r[A2] as u16);
        let mut v = v.borrow_mut();
        let block: Vec<u16> = (0..h)
            .flat_map(|j| (0..w).map(move |i| (i, j)))
            .map(|(i, j)| v.pixel(usize::from(sx + i), usize::from(sy + j)))
            .collect();
        let bytes: Vec<u8> = block.iter().flat_map(|x| x.to_le_bytes()).collect();
        v.write_clut_row(dx, dy, &bytes[..2 * usize::from(w)]);
        assert_eq!(h, 1, "one row");
    });
    let v = vram.clone();
    cpu.hook(LOAD_IMAGE, move |r, m| {
        let rect = r.r[A0];
        let (x, y, w, h) = (m.rd16(rect), m.rd16(rect + 2), m.rd16(rect + 4), m.rd16(rect + 6));
        assert_eq!((w, h), (16, 1), "one CLUT row");
        let bytes: Vec<u8> = (0..32).map(|i| m.rd8(r.r[A1] + i)).collect();
        v.borrow_mut().write_clut_row(x, y, &bytes);
    });
    cpu.hook(DRAW_SYNC, |_, _| {});
    let s = spawned.clone();
    cpu.hook(SPAWN, move |r, _| {
        s.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = ACTOR;
    });
    cpu.mem.wr32(EFFECT_LIST, 0x8012_7000);

    let mut rng = Rng(0x434C_5554);
    let (mut moved, mut filled, mut faded) = (0, 0, 0);
    for round in 0..600 {
        let cell = |rng: &mut Rng| -> (i16, i16) {
            ((rng.below(0x30) * 16) as i16, 1 + rng.below(0x1FE) as i16)
        };
        let a = cell(&mut rng);
        let b = if rng.below(3) == 0 {
            (rng.next() as i16, 0)
        } else {
            cell(&mut rng)
        };
        let dest = cell(&mut rng);
        let frames = if rng.below(3) == 0 { 1 + rng.below(0x40) as i16 } else { 0 };
        let mut payload = [0u8; 14];
        for (k, w) in [a.0, a.1, b.0, b.1, dest.0, dest.1, frames].iter().enumerate() {
            payload[2 * k..2 * k + 2].copy_from_slice(&w.to_le_bytes());
        }
        cpu.mem.wr8(OP, 0x61);
        for (i, x) in payload.iter().enumerate() {
            cpu.mem.wr8(OP + 1 + i as u32, *x);
        }
        // Fill the rows the op can touch.
        {
            let mut v = vram.borrow_mut();
            for y in [a.1, b.1, dest.1] {
                let row: Vec<u8> = (0..0x300 * 2).map(|_| rng.next() as u8).collect();
                v.write_clut_row(0, y as u16, &row);
            }
        }
        let before = vram.borrow().clone();
        for o in (0..0xA0).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0xA5A5_A5A5);
        }
        spawned.borrow_mut().clear();
        assert_eq!(cpu.call(EMITTER, &[CTX, OP]), Stop::Returned, "round {round}");
        let ctx = format!("round {round} a {a:?} b {b:?} dest {dest:?} frames {frames}");

        let mut w = World::new();
        w.spawn_clut_cell_fx(&payload);
        let fx = &w.ambient.clut_fx[0];
        assert_eq!(fx.op.frames, frames, "{ctx}");
        if frames == 0 {
            assert!(matches!(fx.phase, ClutCellFxPhase::Immediate), "{ctx}");
            assert!(spawned.borrow().is_empty(), "no actor: {ctx}");
            let mut port = before.clone();
            w.step_clut_fx(&mut port);
            for y in [a.1, b.1, dest.1] {
                for x in 0..0x300 {
                    assert_eq!(
                        vram.borrow().pixel(x, y as usize),
                        port.pixel(x, y as usize),
                        "vram ({x}, {y}): {ctx}"
                    );
                }
            }
            if fx_is_flat(b) {
                filled += 1;
            } else {
                moved += 1;
            }
        } else {
            assert!(matches!(fx.phase, ClutCellFxPhase::Pending), "{ctx}");
            assert_eq!(*spawned.borrow(), [(FADE_DESCRIPTOR, 0x8012_7000)], "{ctx}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x90), OP, "{ctx}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x94), CTX, "{ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), 0, "{ctx}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x50), 0, "{ctx}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x98), 0, "{ctx}");
            faded += 1;
        }
    }
    assert!(moved > 150 && filled > 80 && faded > 100, "{moved} {filled} {faded}");
}

fn fx_is_flat(b: (i16, i16)) -> bool {
    b.1 == 0
}
