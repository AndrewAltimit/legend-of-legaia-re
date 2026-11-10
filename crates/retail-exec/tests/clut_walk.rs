//! The SCUS actor walker `FUN_8001ADA4` on its case `0xB` - the CLUT-walk
//! shimmer stepper - against `ClutWalkAnim::game_tick`: a list of walker
//! actors over generated tables (frame loops, holds, accumulators part-way
//! through), stepped with varied frame steps. `MoveImage` `FUN_80058490` is
//! hooked and its copies applied to a VRAM image that started equal to the
//! port's; compared each tick: the two VRAM images, and every actor's
//! accumulator and frame index.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene::clut_walk_anim::ClutWalkAnim;
use legaia_scene_formats::clut_walk::{ClutWalkEntry, ClutWalkFrame, ClutWalkTable};
use psx_retail_exec::{A0, A1, A2, Stop};
use psx_tim::Vram;
use std::cell::RefCell;
use std::rc::Rc;

const WALKER: u32 = 0x8001_ADA4;
const MOVE_IMAGE: u32 = 0x8005_8490;
const LIST: u32 = 0x8010_0000;
const ACTORS: u32 = 0x8010_0100;
const ACTOR_STRIDE: u32 = 0x100;
const RECORDS: u32 = 0x8012_0000;

/// One `MoveImage`: source rect, then destination.
type Move = ([u16; 4], u16, u16);

fn patterned() -> Vram {
    let mut v = Vram::new();
    let bytes: Vec<u8> = (0..256u32 * 32)
        .flat_map(|i| ((i.wrapping_mul(0x9E37) >> 3) as u16).to_le_bytes())
        .collect();
    v.write_block(0, 480, 256, 32, &bytes);
    v
}

#[test]
fn the_clut_walk_stepper_matches_the_actor_walker() {
    differential!(scus(WALKER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let moves: Rc<RefCell<Vec<Move>>> = Rc::default();
    let m = moves.clone();
    cpu.hook(MOVE_IMAGE, move |r, mem| {
        let a = r.r[A0];
        let rect = [
            mem.rd16(a),
            mem.rd16(a + 2),
            mem.rd16(a + 4),
            mem.rd16(a + 6),
        ];
        m.borrow_mut().push((rect, r.r[A1] as u16, r.r[A2] as u16));
    });
    let mut rng = Rng(0x434C_5554);
    let mut fired = 0usize;
    for case in 0..200 {
        let n = 1 + rng.below(6) as usize;
        let mut table = ClutWalkTable {
            entries: Vec::new(),
        };
        let mut state = Vec::new();
        let mut at = RECORDS;
        cpu.mem.wr32(LIST, ACTORS);
        for i in 0..n {
            let frames: Vec<ClutWalkFrame> = (0..1 + rng.below(6))
                .map(|_| ClutWalkFrame {
                    hold_vsyncs: rng.below(12) as u8,
                    src_x: 16 * rng.below(15) as u16,
                    src_y: 480 + rng.below(32) as u16,
                })
                .collect();
            let entry = ClutWalkEntry {
                kind: 1,
                cumulative_size: 0,
                dest_x: 16 * rng.below(15) as u16,
                dest_y: 480 + rng.below(32) as u16,
                frames,
            };
            // The record the actor points at: count, destination, frames.
            let mut rec = vec![1u8, entry.frames.len() as u8, 0, 0];
            rec.extend_from_slice(&entry.dest_x.to_le_bytes());
            rec.extend_from_slice(&entry.dest_y.to_le_bytes());
            for f in &entry.frames {
                rec.extend_from_slice(&[0, f.hold_vsyncs, 0, 0]);
                rec.extend_from_slice(&f.src_x.to_le_bytes());
                rec.extend_from_slice(&f.src_y.to_le_bytes());
            }
            cpu.mem.load(at, &rec);
            let acc = if case % 3 == 0 { 100 } else { rng.below(12) };
            let idx = rng.below(entry.frames.len() as u32) as usize;
            let actor = ACTORS + ACTOR_STRIDE * i as u32;
            cpu.mem.load(actor, &[0; ACTOR_STRIDE as usize]);
            let next = if i + 1 < n { actor + ACTOR_STRIDE } else { 0 };
            cpu.mem.wr32(actor, next);
            cpu.mem.wr16(actor + 0x56, 0xB);
            cpu.mem.wr32(actor + 0x4C, at);
            cpu.mem.wr16(actor + 0x68, acc as u16);
            cpu.mem.wr16(actor + 0x6E, idx as u16);
            at += rec.len() as u32;
            table.entries.push(entry);
            state.push((acc, idx));
        }
        let mut anim = ClutWalkAnim::Walk { table, state };
        let (mut port_vram, mut retail_vram) = (patterned(), patterned());
        for tick in 0..60 {
            let dt = 1 + rng.below(4);
            cpu.mem.wr8(0x1F80_0393, dt as u8);
            moves.borrow_mut().clear();
            assert_eq!(cpu.call(WALKER, &[LIST]), Stop::Returned);
            for (r, dx, dy) in moves.borrow().iter() {
                retail_vram.move_image(r[0], r[1], r[2], r[3], *dx, *dy);
            }
            fired += moves.borrow().len();
            anim.game_tick(dt, &mut port_vram);
            let ClutWalkAnim::Walk { state, .. } = &anim else {
                unreachable!()
            };
            for (i, (acc, idx)) in state.iter().enumerate() {
                let actor = ACTORS + ACTOR_STRIDE * i as u32;
                assert_eq!(
                    (
                        u32::from(cpu.mem.rd16(actor + 0x68)),
                        cpu.mem.rd16(actor + 0x6E) as usize
                    ),
                    (*acc, *idx),
                    "case {case} tick {tick} actor {i}"
                );
            }
            assert!(
                retail_vram.as_u16() == port_vram.as_u16(),
                "case {case} tick {tick}: VRAM"
            );
        }
    }
    assert!(fired > 5000, "{fired}");
}
