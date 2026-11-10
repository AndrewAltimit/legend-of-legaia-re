//! The battle weapon trail off the disc. The band emitter `FUN_800485BC`
//! (the yaw turn onto the slot's ground base, the billboard projector
//! `FUN_800195A8` live beneath it with the interpreter's GTE, the `POLY_G4`
//! packets and their `addPrim` links), run over random battle cameras,
//! bodies, sweeps and colours, against `trail_world_point`,
//! `project_trail_point`, `band_quads` and `trail_packet`: the packet
//! buffer and the ordering table are compared whole. Then the sweep driver
//! `FUN_80048310` with the pose decoder and the emitter answered by hooks,
//! against `band_schedule`: the capture walk's step count and point copy, and
//! every band call it makes.

use crate::common::{Rng, scus_cpu};
use legaia_engine_camera::projection::BattleCamPose;
use legaia_render_kernels::battle_trail::{
    MAX_SWEEP_STEPS, TRAIL_PACKET_UNWRITTEN, TrailVertex, band_quads, band_schedule,
    project_trail_point, trail_packet, trail_world_point,
};
use legaia_render_kernels::gte::FieldGteView;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const EMIT: u32 = 0x8004_85BC;
const SWEEP: u32 = 0x8004_8310;
const DECODE: u32 = 0x8004_998C;
const POINTS: u32 = 0x801C_8DA0;
const SLOTS: u32 = 0x801C_9370;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SAVED_CAMERA: u32 = 0x1F80_03C8;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_SHIFT: u32 = 0x1F80_03A4;
const OT_PTR: u32 = 0x1F80_03F4;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTOR: u32 = 0x8010_0000;
const RECORD: u32 = 0x8010_0400;
const SLOT_CTX: u32 = 0x8010_1000;
const PACKETS: u32 = 0x8011_0000;
const OT: u32 = 0x8012_0000;
/// The deepest entry a sum of four `0x3FFF` depths reaches, and one past.
const OT_ENTRIES: u32 = 0x4000;
const POSE_CTX: u32 = 0x8018_0000;
/// `Cpu::call`'s stack top: the fifth argument sits at `+0x10`.
const STACK: u32 = 0x801F_FF00;
const FILL: u8 = 0xA5;

fn seed_luts(cpu: &mut Cpu) {
    cpu.mem.wr32(SIN_PTR, 0x8007_0A2C);
    cpu.mem.wr32(COS_PTR, 0x8007_0A2C + 0x800);
}

/// A battle camera: the pose the phase script stages, its integer matrix
/// loaded into the GTE and saved where the projector restores it from.
fn seed_camera(cpu: &mut Cpu, rng: &mut Rng, shift: u32) -> FieldGteView {
    let pose = BattleCamPose {
        pitch: (rng.below(0x400) as f32) - 0x100 as f32,
        yaw: rng.below(0x1000) as f32,
        tr: [
            rng.below(0x800) as f32 - 0x400 as f32,
            rng.below(0x1000) as f32 - 0x800 as f32,
            0x400 as f32 + rng.below(0x3000) as f32,
        ],
        focus: [
            rng.below(0x800) as f32 - 0x400 as f32,
            rng.below(0x200) as f32 - 0x100 as f32,
            rng.below(0x800) as f32 - 0x400 as f32,
        ],
    };
    let view = FieldGteView::from_battle_pose(&pose);
    let e: Vec<i16> = view.rot.m.iter().flatten().copied().collect();
    let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
    let words = [
        p(e[0], e[1]),
        p(e[2], e[3]),
        p(e[4], e[5]),
        p(e[6], e[7]),
        e[8] as u16 as u32,
        view.tr[0] as u32,
        view.tr[1] as u32,
        view.tr[2] as u32,
    ];
    for (k, w) in words.iter().enumerate() {
        cpu.gte.write_ctrl(k, *w);
    }
    for (k, v) in e.iter().enumerate() {
        cpu.mem.wr16(SAVED_CAMERA + 2 * k as u32, *v as u16);
    }
    for k in 0..3 {
        cpu.mem
            .wr32(SAVED_CAMERA + 0x14 + 4 * k, view.tr[k as usize] as u32);
    }
    cpu.gte.write_ctrl(24, (view.ofx << 16) as u32);
    cpu.gte.write_ctrl(25, (view.ofy << 16) as u32);
    cpu.gte.write_ctrl(26, u32::from(view.h));
    cpu.mem.wr8(OT_SHIFT, shift as u8);
    view
}

fn point_at(step: usize, point: usize) -> u32 {
    POINTS + 8 * step as u32 + 0x80 * point as u32
}

#[test]
fn trail_bands_match_the_emitter() {
    differential!(scus(EMIT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    seed_luts(&mut cpu);
    let mut rng = Rng(0x5452_4C45);
    let (mut quads, mut behind, mut negative_x) = (0usize, 0usize, 0usize);
    for round in 0..1500 {
        let shift = rng.below(3);
        let view = seed_camera(&mut cpu, &mut rng, shift);
        let yaw = rng.next() as u16;
        let slot = rng.below(8);
        let base = [
            (rng.below(0x1000) as i32 - 0x800) as i16,
            (rng.below(0x1000) as i32 - 0x800) as i16,
        ];
        cpu.mem.wr16(ACTOR + 0x26, yaw);
        cpu.mem.wr16(ACTOR + 0x5A, slot as u16);
        cpu.mem.wr32(SLOTS + 4 * slot, SLOT_CTX);
        cpu.mem.wr16(SLOT_CTX + 0x34, base[0] as u16);
        cpu.mem.wr16(SLOT_CTX + 0x38, base[1] as u16);
        let count = 2 + rng.below(3) as usize;
        let seg = rng.below(15) as usize;
        let mut local = [[[0i16; 3]; 4]; 2];
        for (t, step) in local.iter_mut().enumerate() {
            for (i, p) in step.iter_mut().enumerate().take(count) {
                // Every eighth sweep reaches the halfword edges, where the
                // turn's sums wrap.
                *p = if round % 8 == 0 {
                    std::array::from_fn(|_| rng.next() as i16)
                } else {
                    std::array::from_fn(|_| (rng.below(0x800) as i32 - 0x400) as i16)
                };
                for (k, v) in p.iter().enumerate() {
                    cpu.mem.wr16(point_at(seg + t, i) + 2 * k as u32, *v as u16);
                }
            }
        }
        let col: [u32; 2] = [rng.next() & 0xFF_FFFF, rng.next() & 0xFF_FFFF];

        cpu.mem.wr32(PACKET_PTR, PACKETS);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.load(PACKETS, &[FILL; 0x24 * 4]);
        let ot_init: Vec<u8> = (0..OT_ENTRIES)
            .flat_map(|e| (0x00C0_0000 | e).to_le_bytes())
            .collect();
        cpu.mem.load(OT, &ot_init);
        cpu.mem.wr32(STACK + 0x10, col[1]);
        let stop = cpu.call(EMIT, &[ACTOR, count as u32, seg as u32, col[0]]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        assert!(cpu.gte_ops > 0, "the projector ran on the GTE");

        // The port, from the same inputs.
        let rgb = |w: u32| [(w >> 16) as u8, (w >> 8) as u8, w as u8];
        let project = |t: usize| -> Vec<TrailVertex> {
            (0..count)
                .map(|i| {
                    project_trail_point(&view, shift, trail_world_point(base, yaw, local[t][i]))
                })
                .collect()
        };
        let (a, b) = (project(0), project(1));
        let bands = band_quads(&a, &b, rgb(col[0]), rgb(col[1]));
        assert_eq!(bands.len(), count - 1);
        let mut packets = vec![FILL; 0x24 * 4];
        let mut ot = ot_init.clone();
        for (k, q) in bands.iter().enumerate() {
            let at = 0x24 * k;
            for (w, word) in trail_packet(q).iter().enumerate() {
                for (j, byte) in word.to_le_bytes().iter().enumerate() {
                    let off = 4 * w + j;
                    if !TRAIL_PACKET_UNWRITTEN.contains(&off) {
                        packets[at + off] = *byte;
                    }
                }
            }
            // `addPrim` (`FUN_8003D2C4`) at the entry the packet names.
            let e = (q.ot_entry * 4) as usize;
            let entry = u32::from_le_bytes(ot[e..e + 4].try_into().unwrap());
            let tag = u32::from_le_bytes(packets[at..at + 4].try_into().unwrap());
            let addr = PACKETS + at as u32;
            packets[at..at + 4]
                .copy_from_slice(&((entry & 0xFF_FFFF) | (tag & 0xFF00_0000)).to_le_bytes());
            ot[e..e + 4]
                .copy_from_slice(&((addr & 0xFF_FFFF) | (entry & 0xFF00_0000)).to_le_bytes());
            quads += 1;
            negative_x += usize::from(q.xy.iter().any(|p| p.0 < 0));
        }
        behind += usize::from(a.iter().chain(&b).any(|v| v.otz == 0));
        let ctx = format!("round {round} count {count} seg {seg} yaw {yaw:#x} base {base:?}");
        let got = cpu.mem.read(PACKETS, packets.len());
        assert_eq!(
            got,
            packets,
            "packets, {ctx}: {:?}",
            crate::common::diff_offsets(&got, &packets)
        );
        assert_eq!(
            cpu.mem.rd32(PACKET_PTR),
            PACKETS + 0x24 * (count as u32 - 1),
            "packet cursor, {ctx}"
        );
        let got = cpu.mem.read(OT, ot.len());
        assert_eq!(
            got,
            ot,
            "ordering table, {ctx}: {:?}",
            crate::common::diff_offsets(&got, &ot)
        );
    }
    assert!(
        quads > 2000 && behind > 0 && negative_x > 0,
        "quads {quads} behind {behind} off-left {negative_x}"
    );
}

/// What the driver did, as its hooks saw it.
#[derive(Debug, Default)]
struct Sweep {
    /// The cursor each decode ran at.
    decodes: Vec<i16>,
    /// `(count, seg, colA, colB)` per emitter call.
    bands: Vec<(u32, u32, u32, u32)>,
}

#[test]
fn the_sweep_schedule_matches_the_driver() {
    differential!(scus(SWEEP));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let seen = Rc::new(RefCell::new(Sweep::default()));
    {
        let seen = seen.clone();
        cpu.hook(DECODE, move |r, m| {
            // Stand in for the pose decode: a pose pool whose every object
            // origin names the cursor it was decoded at.
            let cursor = m.rd16(r.r[A0] + 0x68) as i16;
            seen.borrow_mut().decodes.push(cursor);
            for obj in 0..32u32 {
                let at = POSE_CTX + 0x6F4 + 12 * obj;
                m.wr16(at, cursor as u16);
                m.wr16(at + 2, obj as u16);
                m.wr16(at + 4, (cursor as u16) ^ (obj as u16) << 8);
            }
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(EMIT, move |r, m| {
            let col_b = m.rd32(r.r[SP] + 0x10);
            seen.borrow_mut()
                .bands
                .push((r.r[A1], r.r[A2], r.r[A3], col_b));
        });
    }
    cpu.mem.wr32(CTX_PTR, POSE_CTX);
    let mut rng = Rng(0x5357_4550);
    let (mut capped, mut two, mut none) = (0, 0, 0);
    for round in 0..4000 {
        *seen.borrow_mut() = Sweep::default();
        let rate = 1 + rng.below(8) as u8;
        let cursor: i16 = match round % 5 {
            0 => rng.below(0x40) as i16 - 0x10,
            1 => rng.next() as i16,
            _ => rng.below(0x200) as i16,
        };
        let base = rng.below(16);
        let count = 2 + rng.below(3);
        let tint = rng.next() & 0xFF_FFFF;
        cpu.mem.wr16(ACTOR + 0x68, cursor as u16);
        cpu.mem.wr32(ACTOR + 0x4C, RECORD);
        cpu.mem.wr8(RECORD + 0x78, rate);
        assert_eq!(cpu.call(SWEEP, &[ACTOR, base, count, tint]), Stop::Returned);
        let ctx = format!("round {round} cursor {cursor} rate {rate}");
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x68) as i16,
            cursor,
            "cursor restored, {ctx}"
        );

        // The capture walk: one step per cursor `> 0`, rewinding two
        // frames of the clip rate each, at most sixteen.
        let mut want = Vec::new();
        let mut c = cursor;
        while c > 0 && want.len() < MAX_SWEEP_STEPS {
            want.push(c);
            c = c.wrapping_sub(2 * i16::from(rate));
        }
        let s = seen.borrow();
        assert_eq!(s.decodes, want, "decoded cursors, {ctx}");
        for (step, c) in want.iter().enumerate() {
            for i in 0..count as usize {
                let obj = base + i as u32;
                let got: [u16; 3] =
                    std::array::from_fn(|k| cpu.mem.rd16(point_at(step, i) + 2 * k as u32));
                let want = [*c as u16, obj as u16, (*c as u16) ^ (obj as u16) << 8];
                assert_eq!(got, want, "step {step} point {i}, {ctx}");
            }
        }

        // The bands: the port's schedule, plus the stale second band a
        // two-step sweep calls for and the port skips (disclosed on
        // `band_schedule`).
        let word = |c: [u8; 3]| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]);
        let rgb = [(tint >> 16) as u8, (tint >> 8) as u8, tint as u8];
        let mut port: Vec<(u32, u32, u32, u32)> = band_schedule(want.len(), rgb)
            .iter()
            .map(|b| (count, b.seg as u32, word(b.lead), word(b.tail)))
            .collect();
        if want.len() == 2 {
            port.insert(1, (count, 1, 0x7F_7F7F, 0));
            two += 1;
        }
        assert_eq!(s.bands, port, "bands, {ctx}");
        capped += usize::from(want.len() == MAX_SWEEP_STEPS);
        none += usize::from(want.len() < 2);
    }
    assert!(
        capped > 0 && two > 0 && none > 0,
        "capped {capped} two {two} short {none}"
    );
}

const TRIGGER: u32 = 0x8005_112C;
const CHAR_IDS: u32 = 0x8007_BD10;

/// The per-character trigger `FUN_8005112C`, the sweep driver hooked, over
/// every roster id, seat and `+0x77` identity byte, against `trail_trigger`:
/// which seats sweep, from which object and in which tint.
#[test]
fn the_trail_trigger_matches_the_ladder() {
    differential!(scus(TRIGGER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let swept = Rc::new(RefCell::new(Vec::new()));
    {
        let swept = swept.clone();
        cpu.hook(SWEEP, move |r, _| {
            swept.borrow_mut().push((r.r[A1], r.r[A2], r.r[A3]));
        });
    }
    let mut fired = 0;
    for character in 0..6u8 {
        for seat in 0..5u16 {
            for key in 0..=0xFFu8 {
                for cursor in [0i16, 1, 0x40] {
                    swept.borrow_mut().clear();
                    cpu.mem.wr8(CHAR_IDS + u32::from(seat), character);
                    cpu.mem.wr16(ACTOR + 0x5A, seat);
                    cpu.mem.wr16(ACTOR + 0x68, cursor as u16);
                    cpu.mem.wr32(ACTOR + 0x4C, RECORD);
                    cpu.mem.wr8(RECORD + 0x77, key);
                    assert_eq!(cpu.call(TRIGGER, &[ACTOR]), Stop::Returned);
                    let want: Vec<(u32, u32, u32)> =
                        legaia_render_kernels::battle_trail::trail_trigger(character, key)
                            .filter(|_| seat < 3 && cursor != 0)
                            .map(|t| {
                                let rgb = u32::from(t.rgb[0]) << 16
                                    | u32::from(t.rgb[1]) << 8
                                    | u32::from(t.rgb[2]);
                                (
                                    t.base_part as u32,
                                    legaia_render_kernels::battle_trail::TRAIL_POINTS as u32,
                                    rgb,
                                )
                            })
                            .into_iter()
                            .collect();
                    assert_eq!(
                        *swept.borrow(),
                        want,
                        "character {character} seat {seat} key {key:#x} cursor {cursor}"
                    );
                    fired += want.len();
                }
            }
        }
    }
    assert!(fired > 0);
}
