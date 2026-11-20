//! The SCUS actor walker `FUN_8001ADA4` on its case `0xB` - the CLUT-walk
//! shimmer stepper - against `ClutWalkAnim::game_tick`: a list of walker
//! actors over generated tables (frame loops, holds, accumulators part-way
//! through), stepped with varied frame steps. `MoveImage` `FUN_80058490` is
//! hooked and its copies applied to a VRAM image that started equal to the
//! port's; compared each tick: the two VRAM images, and every actor's
//! accumulator and frame index.
//!
//! Then its draw-kind-4 case: which builder each node's `+0x9E` arm calls and
//! with what, and whether the tail hands the node on or drops it by its view
//! depth, against `effect_sprite_arm::is_sprite_arm`,
//! `effect_ribbon::ribbon_call_args`, `effect_default_arm::call_args` and
//! `effect_ribbon::node_near_reject_depth`.

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

/// Case 4 of the walker over a list of draw-kind-4 nodes: the arm `+0x9E`
/// picks (`0x4000` the sprite builder `FUN_8002A5A4`, `0x2000` the ribbon
/// emitter `0x801CFA48`, neither the default ring `FUN_80028158`) with the
/// arguments the port builds, then the tail: a mode-`6` node goes to the
/// animated renderer `FUN_8001B964` and any other to the cull test
/// `FUN_8001B73C`, unless the view depth the walker measured (`+0x34`, from
/// the GTE when `+0x10 & 0x80`, else `0x7D0`) puts it nearer than the port's
/// `node_near_reject_depth`.
#[test]
fn the_actor_walker_routes_draw_kind_4_as_the_port_routes() {
    differential!(scus(WALKER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    use legaia_engine_effects::effect_default_arm::call_args;
    use legaia_engine_effects::effect_ribbon::{node_near_reject_depth, ribbon_call_args};
    use legaia_engine_effects::effect_sprite_arm::is_sprite_arm;
    use legaia_engine_vm::move_vm::ActorState;
    use psx_retail_exec::{A3, V0};
    const WALKER_END: u32 = 0x8001_B73C;
    const SPRITE: u32 = 0x8002_A5A4;
    const RIBBON: u32 = 0x801C_FA48;
    const DEFAULT: u32 = 0x8002_8158;
    const ANIMATED: u32 = 0x8001_B964;
    const CULL: u32 = 0x8001_B73C;
    /// The effect-model heap base the case repoints the model list into.
    const HEAP_PTR: u32 = 0x8007_B85C;
    const HEAP: u32 = 0x8013_0000;
    const MODEL_LIST: u32 = 0x8012_0000;
    const NODES: u32 = 0x8010_0100;
    const STRIDE: u32 = 0x200;
    /// The camera matrix block `0x1F8003C8` (rotation, then translation).
    const CAMERA: u32 = 0x1F80_03C8;
    /// `SetRotMatrix` + `SetTransMatrix` off one block.
    const LOAD_MATRIX: u32 = 0x8003_D1A4;
    cpu.stub_outside = Some((WALKER, WALKER_END));
    cpu.mem.wr32(HEAP_PTR, HEAP);
    cpu.hook_cpu(LOAD_MATRIX, |c| {
        let at = c.regs.r[A0];
        for k in 0..8 {
            let w = c.mem.rd32(at + 4 * k);
            c.gte.write_ctrl(k as usize, w);
        }
    });
    type Log = Rc<RefCell<Vec<(u32, [u32; 4], Vec<u8>)>>>;
    let log: Log = Rc::default();
    let cull_says = Rc::new(RefCell::new(1u32));
    for va in [SPRITE, RIBBON, DEFAULT, ANIMATED, CULL] {
        let (log, cull_says) = (log.clone(), cull_says.clone());
        cpu.hook(va, move |r, m| {
            let block = if va == DEFAULT {
                (0..0x24).map(|i| m.rd8(r.r[A3] + i)).collect()
            } else {
                Vec::new()
            };
            log.borrow_mut()
                .push((va, [r.r[A0], r.r[A1], r.r[A2], r.r[A3]], block));
            r.r[V0] = if va == CULL { *cull_says.borrow() } else { 0 };
        });
    }
    let mut rng = Rng(0x4B49_4E44);
    let mut seen = std::collections::BTreeMap::<&str, u32>::new();
    for round in 0..3_000 {
        let n = 1 + rng.below(4);
        let mut nodes = Vec::new();
        for i in 0..n {
            let at = NODES + STRIDE * i;
            let mut rec: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
            let next = if i + 1 < n { at + STRIDE } else { 0 };
            rec[0..4].copy_from_slice(&next.to_le_bytes());
            let mut put16 = |o: usize, v: u16| rec[o..o + 2].copy_from_slice(&v.to_le_bytes());
            // Now and then a draw kind outside the jump table's `1..=11`,
            // which the walker passes over (`0x8001AE70`).
            let kind = match rng.below(8) {
                0 => [0u16, 12, 0x20, 0xFFFF][rng.below(4) as usize],
                _ => 4,
            };
            put16(0x56, kind);
            let arm = [0u16, 0x4000, 0x2000, 0x6000][rng.below(4) as usize];
            let f9e = (rng.next() as u16 & 0x9FFF) | arm;
            put16(0x9E, f9e);
            put16(
                0x5A,
                if rng.below(3) == 0 {
                    6
                } else {
                    rng.below(6) as u16
                },
            );
            put16(
                0x72,
                if rng.below(4) == 0 {
                    0
                } else {
                    rng.next() as u16
                },
            );
            // `+0x52`: the near-reject override `0x20`, and `0x400`, which
            // op `0x15` sets only with the depth bit `+0x10 & 0x80` clear.
            let f52 = rng.next() as u16 & !0x420
                | if rng.below(4) == 0 { 0x20 } else { 0 }
                | if rng.below(6) == 0 { 0x400 } else { 0 };
            put16(0x52, f52);
            let mut flags = u32::from_le_bytes(rec[0x10..0x14].try_into().unwrap()) & !0x8A;
            if f52 & 0x400 == 0 && rng.below(4) != 0 {
                flags |= 0x80;
            }
            rec[0x10..0x14].copy_from_slice(&flags.to_le_bytes());
            rec[0x44..0x48].copy_from_slice(&MODEL_LIST.to_le_bytes());
            cpu.mem.load(at, &rec);
            nodes.push(at);
        }
        cpu.mem.wr32(MODEL_LIST, rng.below(3));
        // A view depth either side of the two thresholds: the GTE's
        // translation with a zero rotation is what MVMVA leaves in MAC3.
        let depth = 0x9C + rng.below(8) as i32 - if rng.below(5) == 0 { 0x200 } else { 0 };
        for r in 0..5 {
            cpu.gte.write_ctrl(r, 0);
        }
        cpu.gte.write_ctrl(5, 0);
        cpu.gte.write_ctrl(6, 0);
        cpu.gte.write_ctrl(7, depth as u32);
        // The camera block the model draw restores through `FUN_8003D1A4`
        // (`0x8001B5E8`, from `0x1F8003C8`) once it has loaded the node's
        // own matrix: the same view, so the next node measures through it.
        for k in 0..7 {
            cpu.mem.wr32(CAMERA + 4 * k, 0);
        }
        cpu.mem.wr32(CAMERA + 0x1C, depth as u32);
        *cull_says.borrow_mut() = rng.below(2);
        log.borrow_mut().clear();
        cpu.mem.wr32(LIST, nodes[0]);
        assert_eq!(cpu.call(WALKER, &[LIST]), Stop::Returned, "round {round}");

        let mut want: Vec<(u32, [u32; 4], Vec<u8>)> = Vec::new();
        for &at in &nodes {
            let mut st = ActorState::new();
            let mut rd = |o: usize| cpu.mem.rd16(at + o as u32);
            st.move_substate = rd(0x56) as i16;
            st.move_submode = rd(0x5A) as i16;
            st.field_52 = rd(0x52);
            st.field_72 = rd(0x72);
            st.field_9e = rd(0x9E);
            for o in (0x9C..0x100).step_by(2) {
                st.set_actor_u16(o, rd(o));
            }
            let flags = cpu.mem.rd32(at + 0x10);
            let model = HEAP + 0x5_DC00;
            let block = at + 0x9C;
            if is_sprite_arm(&st) {
                want.push((
                    SPRITE,
                    [model, u32::from(st.field_9e), 0, block],
                    Vec::new(),
                ));
            }
            if let Some((mode, packed)) = ribbon_call_args(&st) {
                want.push((RIBBON, [model, mode, packed, block], Vec::new()));
            }
            if let Some((mode, packed, src)) = call_args(&st) {
                want.push((DEFAULT, [model, mode, packed, block], src.to_vec()));
            }
            let measured = flags & 0x80 != 0;
            let walker_depth = if measured { depth } else { 0x7D0 };
            assert_eq!(
                cpu.mem.rd32(at + 0x34) as i32,
                walker_depth,
                "round {round}: node {at:#x}"
            );
            if st.move_substate != 4 {
                *seen.entry("no kind").or_default() += 1;
                continue;
            }
            let rejected = node_near_reject_depth(&st).is_some_and(|d| (walker_depth as f32) < d);
            if !rejected {
                want.push((
                    if st.move_submode == 6 { ANIMATED } else { CULL },
                    [at, 0, 0, 0],
                    Vec::new(),
                ));
            }
            *seen
                .entry(if rejected { "rejected" } else { "drawn" })
                .or_default() += 1;
            *seen
                .entry(if st.move_submode == 6 {
                    "mode 6"
                } else {
                    "model"
                })
                .or_default() += 1;
        }
        // The tail's calls take the node alone.
        let got: Vec<(u32, [u32; 4], Vec<u8>)> = log
            .borrow()
            .iter()
            .map(|(va, a, b)| match *va {
                ANIMATED | CULL => (*va, [a[0], 0, 0, 0], Vec::new()),
                // The sprite port takes the node's state, not the packed count.
                SPRITE => (*va, [a[0], a[1], 0, a[3]], Vec::new()),
                _ => (*va, *a, b.clone()),
            })
            .collect();
        assert_eq!(got, want, "round {round}");
        // The model list is repointed at the heap's model for every node.
        for k in 0..cpu.mem.rd32(MODEL_LIST) {
            assert_eq!(cpu.mem.rd32(MODEL_LIST + 4 + 4 * k), HEAP + 0x5_DC00 + 0xC);
        }
    }
    // An empty list draws nothing (`0x8001ADDC`).
    log.borrow_mut().clear();
    cpu.mem.wr32(LIST, 0);
    assert_eq!(cpu.call(WALKER, &[LIST]), Stop::Returned);
    assert!(log.borrow().is_empty());
    for what in ["rejected", "drawn", "mode 6", "model", "no kind"] {
        assert!(seen.get(what).copied().unwrap_or(0) > 500, "{seen:?}");
    }
}
