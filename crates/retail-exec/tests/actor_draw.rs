//! The battle per-actor draw `FUN_80048A08` against the `battle_actor_draw`
//! kernels: per object, the colour word and blend the leaf is handed
//! (`object_draw_words` over the SCUS Rot-limb table `RotLimbTable`), the
//! ordering-table bias (`depth_bias`) and the leaf (`draw_path`, the lit
//! leaf's light row); the render-scale gate (`render_scale`); and the
//! ground shadow (`shadow_plan`: the disc's two colours, radius and segment
//! count handed to the mesh builder, and whether its flat draw is issued).
//!
//! Party and monster seats, every Rot limb bit over every roster id, object
//! counts `0..=24`, the three draw paths, scales on and off `0x1000`, the
//! depth-bias flag, heights zero and not, colour words with and without the
//! `0x83000000` bits, sizes of both signs, and tilted bodies.
//!
//! The matrix helpers, the pose decoder and the three leaves are hooked
//! (the leaves record what they were handed); the GTE loads between them run
//! on the interpreter's GTE.

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_battle_vm::battle_actor_draw::{
    DrawPath, RotLimbTable, SCALE_ONE, SHADOW_FLAG_WORD, depth_bias, draw_path, object_draw_words,
    render_scale, shadow_plan,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DRAW: u32 = 0x8004_8A08;
const NO_OPS: [u32; 11] = [
    0x8002_6988,
    0x8005_B3A8,
    0x8004_95C8,
    0x8004_998C,
    0x8003_D178,
    0x8003_D20C,
    0x8004_638C,
    0x8004_629C,
    0x8004_61A4,
    0x8003_D1A4,
    0x8003_D344,
];
const SCALE: u32 = 0x8005_B4E8;
const EFFECT_SETUP: u32 = 0x8001_C204;
const LEAF_DISPATCH: u32 = 0x8004_3390;
const LEAF_LIT: u32 = 0x8002_9888;
const LEAF_EFFECT: u32 = 0x8002_735C;
const SHADOW_MESH: u32 = 0x8002_8158;
const AFTER: u32 = 0x8004_9858;
const COLOUR: u32 = 0x8007_BCF0;
const BLEND: u32 = 0x8007_BCF4;
const OT_PTR: u32 = 0x1F80_03F4;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const SEAT_ACTOR: u32 = 0x8013_0000;
const SEATS: u32 = 0x8007_BD10;
const ASSET_BUF_PTR: u32 = 0x8007_B85C;
const ASSET_BUF: u32 = 0x8018_0000;
const LIGHT_TABLE: u32 = 0x8007_BE60;
const ACTOR: u32 = 0x8014_0000;
const CLIP: u32 = 0x8014_1000;
const COUNT: u32 = 0x8014_1100;
const OBJECTS: u32 = 0x8014_1200;
const OT: u32 = 0x8015_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Scale([u32; 3]),
    Leaf {
        leaf: u32,
        object: u32,
        colour: u32,
        blend: u32,
        ot: u32,
        light: u32,
    },
    Shadow {
        inner: u32,
        outer: u32,
        radius: i16,
        segments: u32,
    },
    ShadowDraw(u32, u32, u32),
}

#[test]
fn the_battle_actor_draw_decides_as_retail() {
    differential!(scus(DRAW));
    let (Some(mut cpu), Some(exe)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let limbs = RotLimbTable::from_scus(&exe).expect("rot limb table");
    for va in NO_OPS.into_iter().chain([EFFECT_SETUP, AFTER]) {
        cpu.stub(va, 0);
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let calls = calls.clone();
        cpu.hook(SCALE, move |r, m| {
            let v = std::array::from_fn(|k| m.rd32(r.r[A1] + 4 * k as u32));
            calls.borrow_mut().push(Call::Scale(v));
        });
    }
    for leaf in [LEAF_DISPATCH, LEAF_LIT, LEAF_EFFECT] {
        let calls = calls.clone();
        cpu.hook(leaf, move |r, m| {
            let colour = m.rd32(COLOUR);
            let blend = m.rd32(BLEND);
            let c = if leaf == LEAF_DISPATCH && r.r[A1] == SHADOW_FLAG_WORD {
                Call::ShadowDraw(r.r[A0], r.r[A1], r.r[A2])
            } else {
                assert_eq!((r.r[A1], r.r[A2]), (colour, blend), "the leaf's words");
                Call::Leaf {
                    leaf,
                    object: r.r[A0],
                    colour,
                    blend,
                    ot: m.rd32(OT_PTR),
                    light: if leaf == LEAF_LIT { r.r[A3] } else { 0 },
                }
            };
            calls.borrow_mut().push(c);
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(SHADOW_MESH, move |r, m| {
            assert_eq!(r.r[A0], ASSET_BUF + 0x6_2400);
            assert_eq!(r.r[A1], 1);
            let s = r.r[A3];
            calls.borrow_mut().push(Call::Shadow {
                inner: m.rd32(s + 4),
                outer: m.rd32(s + 8),
                radius: m.rd16(s + 0x1A) as i16,
                segments: r.r[A2],
            });
        });
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ASSET_BUF_PTR, ASSET_BUF);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + 4 * s, SEAT_ACTOR + 0x400 * s);
    }
    let mut rng = Rng(0x4452_4157);
    let (mut paths, mut dimmed, mut shadows, mut scaled) = ([0u32; 3], 0u32, [0u32; 2], 0u32);
    for n in 0..20_000 {
        let seat = rng.below(7) as i16;
        let roster = 1 + rng.below(4) as u8;
        cpu.mem.wr8(SEATS + seat.min(2) as u32, roster);
        let status = match rng.below(3) {
            0 => rng.next() as u16,
            _ => (rng.below(8) as u16) << 3,
        };
        cpu.mem
            .wr16(SEAT_ACTOR + 0x400 * seat as u32 + 0x16E, status);
        let count = rng.below(25);
        let mut a: Vec<u8> = (0..0x80).map(|_| rng.next() as u8).collect();
        let set16 =
            |a: &mut Vec<u8>, o: usize, v: u16| a[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let set32 =
            |a: &mut Vec<u8>, o: usize, v: u32| a[o..o + 4].copy_from_slice(&v.to_le_bytes());
        let effect = if rng.below(4) == 0 {
            1 + rng.below(3) as u16
        } else {
            0
        };
        let light = if rng.below(3) == 0 {
            rng.next() as u16 | 1
        } else {
            0
        };
        let row = rng.below(8) as u8;
        let scale = if rng.below(2) == 0 {
            SCALE_ONE
        } else {
            rng.next() as u16
        };
        let flags = rng.next() & !0x80_0000 | if rng.below(2) == 0 { 0x80_0000 } else { 0 };
        let colour = rng.next() & !0x8300_0000
            | if rng.below(2) == 0 {
                0x8300_0000 & rng.next()
            } else {
                0
            };
        let blend = rng.next() as u16;
        let height = if rng.below(2) == 0 {
            0
        } else {
            rng.next() as u16
        };
        let size = rng.next() as u16;
        let no_shadow = if rng.below(3) == 0 { 1 } else { 0 };
        let pitch = if rng.below(3) == 0 {
            rng.next() as u16
        } else {
            0
        };
        let roll = if rng.below(3) == 0 {
            rng.next() as u16
        } else {
            0
        };
        set32(&mut a, 0x10, flags);
        set16(&mut a, 0x16, height);
        set16(&mut a, 0x24, pitch);
        set16(&mut a, 0x28, roll);
        set16(&mut a, 0x42, effect);
        set32(&mut a, 0x44, OBJECTS);
        set32(&mut a, 0x4C, CLIP);
        set16(&mut a, 0x58, size);
        set16(&mut a, 0x5A, seat as u16);
        set16(&mut a, 0x6A, no_shadow);
        a[0x6D] = row;
        set16(&mut a, 0x72, scale);
        set32(&mut a, 0x74, colour);
        set16(&mut a, 0x78, blend);
        set16(&mut a, 0x7A, light);
        cpu.mem.load(ACTOR, &a);
        cpu.mem.wr32(CLIP + 0x88, COUNT);
        cpu.mem.wr8(COUNT, count as u8);
        for o in 0..count {
            cpu.mem.wr32(OBJECTS + 4 + 4 * o, 0x8016_0000 + 0x100 * o);
        }
        cpu.mem.wr32(OT_PTR, OT);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(DRAW, &[ACTOR]), Stop::Returned, "n {n}");

        // The port.
        let mut want = Vec::new();
        if render_scale(scale) != 1.0 {
            scaled += 1;
            let s = u32::from(scale);
            want.push(Call::Scale([s; 3]));
            if effect != 0 {
                want.push(Call::Scale([s; 3]));
            }
        }
        let path = draw_path(effect as i16, light as i16, row);
        for o in 0..count {
            let (c, b) =
                object_draw_words(colour, blend, seat, status, limbs.row(roster), o as usize);
            dimmed += u32::from(c != colour);
            let (leaf, lit) = match path {
                DrawPath::ObjectEffect => (LEAF_EFFECT, 0),
                DrawPath::Lit { row } => {
                    let word = cpu.mem.rd32(LIGHT_TABLE + 12 * u32::from(row) + 8);
                    (LEAF_LIT, word << 16 | light as i16 as i32 as u32)
                }
                DrawPath::Dispatch => (LEAF_DISPATCH, 0),
            };
            want.push(Call::Leaf {
                leaf,
                object: 0x8016_0000 + 0x100 * o,
                colour: c,
                blend: u32::from(b),
                ot: (OT as i32 + depth_bias(flags)) as u32,
                light: lit,
            });
        }
        paths[match path {
            DrawPath::ObjectEffect => 0,
            DrawPath::Lit { .. } => 1,
            DrawPath::Dispatch => 2,
        }] += u32::from(count > 0);
        if let Some(p) = shadow_plan(
            no_shadow as i16,
            colour,
            height as i16,
            size as i16,
            pitch as i16,
            roll as i16,
        ) {
            want.push(Call::Shadow {
                inner: p.inner,
                outer: p.outer,
                radius: p.radius,
                segments: u32::from(p.segments),
            });
            if p.drawn {
                want.push(Call::ShadowDraw(ASSET_BUF + 0x6_240C, SHADOW_FLAG_WORD, 1));
            }
            shadows[usize::from(p.drawn)] += 1;
        }
        assert_eq!(
            *calls.borrow(),
            want,
            "n {n} seat {seat} roster {roster} status {status:#x} count {count}"
        );
        assert_eq!(cpu.mem.rd32(OT_PTR), OT, "n {n}: the bias is undone");
    }
    eprintln!("paths {paths:?}, {dimmed} dimmed objects, shadows {shadows:?}, {scaled} scaled");
    assert!(paths.iter().all(|&c| c > 500) && dimmed > 1000 && scaled > 1000);
    assert!(shadows.iter().all(|&c| c > 1000), "{shadows:?}");
}
