//! The battle pose decoder `FUN_8004998C` against the port's pieces: the blend
//! `blend_part_pose` over the 9-byte part stream at every cursor nibble, its
//! pose-buffer centroid `pose_centroid_xz` and the body pair `body_offset`
//! writes back to the actor.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::monster_archive::PartPose;
use legaia_engine_battle_vm::battle_action::motion::{body_offset, pose_centroid_xz};
use legaia_engine_battle_vm::battle_pose_blend::blend_part_pose;
use legaia_engine_system::trig::sin12;
use psx_retail_exec::Stop;

const DECODE: u32 = 0x8004_998C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8014_0000;
const ENTRY: u32 = 0x8014_1000;
const STREAM: u32 = 0x8015_0000;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN: u32 = 0x8016_0000;
const COS: u32 = 0x8016_4000;
const POOL: u32 = CTX + 0x6F4;

fn twelve(v: i16) -> u16 {
    v as u16 & 0xFFF
}

fn pack(p: &PartPose) -> [u8; 9] {
    let (a, b, c, d, e, f) = (
        twelve(p.tx),
        twelve(p.ty),
        twelve(p.tz),
        p.rx & 0xFFF,
        p.ry & 0xFFF,
        p.rz & 0xFFF,
    );
    [
        a as u8,
        b as u8,
        (a >> 8) as u8 | ((b >> 8) as u8) << 4,
        c as u8,
        d as u8,
        (c >> 8) as u8 | ((d >> 8) as u8) << 4,
        e as u8,
        f as u8,
        (e >> 8) as u8 | ((f >> 8) as u8) << 4,
    ]
}

fn gen_part(rng: &mut Rng) -> PartPose {
    let t = |rng: &mut Rng| match rng.below(3) {
        0 => rng.below(0x100) as i16 - 0x80,
        1 => rng.below(0x1000) as i16 - 0x800,
        _ => 0,
    };
    let r = |rng: &mut Rng| match rng.below(4) {
        0 => 0x800 + rng.below(3) as u16 - 1,
        1 => rng.below(0x40) as u16,
        _ => rng.below(0x1000) as u16,
    };
    PartPose {
        tx: t(rng),
        ty: t(rng),
        tz: t(rng),
        rx: r(rng),
        ry: r(rng),
        rz: r(rng),
    }
}

#[test]
fn the_pose_decoder_is_retails() {
    differential!(scus(DECODE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    for a in 0..0x1400u32 {
        let s = sin12(a as u16) as u16;
        cpu.mem.wr16(SIN + a * 2, s);
        let c = sin12((a as u16).wrapping_add(0x400)) as u16;
        cpu.mem.wr16(COS + a * 2, c);
    }
    cpu.mem.wr32(NODE + 0x4C, ENTRY);
    cpu.mem.wr32(ENTRY + 0x88, STREAM);
    cpu.mem.wr16(NODE + 0x5A, 0);
    let mut rng = Rng(0x504F_5345);
    let (mut retries, mut failures) = (0, Vec::new());
    for round in 0..80_000 {
        let parts = 1 + rng.below(8) as usize;
        let frames = 3 + rng.below(6) as usize;
        let cur_frame = rng.below(frames as u32 - 2);
        let nibble = rng.below(16) as u16;
        let cursor = (cur_frame as u16) << 4 | nibble;
        let mut poses: Vec<Vec<PartPose>> = Vec::new();
        let mut stream = vec![parts as u8, frames as u8];
        for _ in 0..frames {
            let f: Vec<PartPose> = (0..parts).map(|_| gen_part(&mut rng)).collect();
            for p in &f {
                stream.extend(pack(p));
            }
            poses.push(f);
        }
        cpu.mem.load(STREAM, &stream);
        cpu.mem.wr16(NODE + 0x68, cursor);
        cpu.mem.wr8(ENTRY + 0x85, 0);
        cpu.mem.wr8(ENTRY + 0x86, 0);
        cpu.mem.wr8(ACTOR + 0x21B, 0);
        let (facing, scale, sx, sz, sy) = (
            rng.below(0x1000) as u16,
            0x800 + rng.below(0x1000) as u16,
            rng.below(0x800) as i16 - 0x400,
            rng.below(0x800) as i16 - 0x400,
            -(rng.below(0x200) as i16),
        );
        cpu.mem.wr16(ACTOR + 0x46, facing);
        cpu.mem.wr16(NODE + 0x72, scale);
        cpu.mem.wr16(ACTOR + 0x34, sx as u16);
        cpu.mem.wr16(ACTOR + 0x36, sy as u16);
        cpu.mem.wr16(ACTOR + 0x38, sz as u16);
        cpu.mem.wr16(ACTOR + 0x44, 0);
        cpu.mem.wr16(CTX + 0x328, 0);
        assert_eq!(cpu.call(DECODE, &[NODE]), Stop::Returned, "{round}");
        let cf = cur_frame as usize;
        let mut out_parts: Vec<([i16; 3], [i16; 3])> = Vec::new();
        for (i, (cur, next)) in poses[cf].iter().zip(&poses[cf + 1]).enumerate() {
            let p = blend_part_pose(*cur, *next, nibble as u8, 0);
            retries += u32::from(p.retried);
            out_parts.push((
                p.translation,
                [
                    p.rotation[0] as i16,
                    p.rotation[1] as i16,
                    p.rotation[2] as i16,
                ],
            ));
            let at = POOL + i as u32 * 0xC;
            let got: Vec<u16> = (0..6).map(|k| cpu.mem.rd16(at + k * 2)).collect();
            let mut want: Vec<u16> = p.translation.iter().map(|&v| v as u16).collect();
            want.extend(p.rotation.iter().copied());
            // The zero-nibble path stores the raw 12-bit translations.
            let got_t: Vec<i16> = got[..3]
                .iter()
                .map(|&v| {
                    if nibble == 0 {
                        ((v << 4) as i16) >> 4
                    } else {
                        v as i16
                    }
                })
                .collect();
            let want_t: Vec<i16> = p.translation.to_vec();
            if (got_t != want_t || got[3..] != want[3..]) && failures.len() < 12 {
                failures.push(format!(
                    "round {round} part {i} nibble {nibble}: retail {got:x?} port {want:x?}"
                ));
            }
        }
        let (cx, cz) = pose_centroid_xz(&out_parts);
        let (rcx, rcz) = (
            cpu.mem.rd16(POOL + 0x120) as i16,
            cpu.mem.rd16(POOL + 0x124) as i16,
        );
        let (ox, oz) = body_offset(facing, cx, cz, i32::from(scale));
        let body = (
            cpu.mem.rd16(ACTOR + 0x3C) as i16,
            cpu.mem.rd16(ACTOR + 0x40) as i16,
        );
        let want_body = (sx.wrapping_add(ox), sz.wrapping_add(oz));
        if ((rcx, rcz) != (cx, cz) || body != want_body) && failures.len() < 12 {
            failures.push(format!(
                "round {round} parts {parts}: retail centroid ({rcx}, {rcz}) body {body:?}, \
                 port ({cx}, {cz}) {want_body:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(retries > 500, "{retries}");
}

/// The next-entry rule (`0x80049A28..0x80049BE4`) against
/// `MonsterAnimPlayer`'s decode: on the frame before a loop window's end with
/// cycles left (`+0x86 - 1`, `+0x21B`) the blend runs toward the window's
/// start `+0x85`; on the last frame toward the queued clip's frame 0 - the
/// party entry table `0x801C9360[seat][+0x1DA]` or a monster record's
/// `+0x4C` list, the idle entry under `+0x1DC` bit 2, the clip itself when
/// HP is `0`, the queued id is `0x10` or more or a monster's queued stream
/// has another part count - with the committed entry's `+0x0E` on the Z delta
/// unless `+0x228` is up; otherwise toward the next frame.
#[test]
fn the_next_entry_rule_is_retails() {
    differential!(scus(DECODE));
    use legaia_actor_formats::monster_archive::parse_animation_stream;
    use legaia_engine_battle::battle_anim::{MonsterAnimPlayer, TweenTarget};
    const TABLES: u32 = 0x801C_9360;
    const RECORDS: u32 = 0x801C_9348;
    const TABLE: u32 = 0x8017_0000;
    const RECORD: u32 = 0x8017_1000;
    const QUEUED: u32 = 0x8017_2000;
    const QSTREAM: u32 = 0x8017_3000;
    const IDLE: u32 = 0x8017_4000;
    const ISTREAM: u32 = 0x8017_5000;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    for a in 0..0x1400u32 {
        cpu.mem.wr16(SIN + a * 2, sin12(a as u16) as u16);
        cpu.mem
            .wr16(COS + a * 2, sin12((a as u16).wrapping_add(0x400)) as u16);
    }
    let mut rng = Rng(0x4E45_5854);
    let (mut windowed, mut queued_blends, mut failures) = (0u32, 0u32, Vec::new());
    for round in 0..40_000 {
        let seat: u32 = if rng.below(2) == 0 { 0 } else { 3 };
        let actor = ACTOR + seat * 0x400;
        cpu.mem.wr32(ACTORS + seat * 4, actor);
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        let parts = 1 + rng.below(6) as usize;
        let frames = 2 + rng.below(7) as usize;
        let stream_of = |rng: &mut Rng, parts: usize, frames: usize| {
            let poses: Vec<Vec<PartPose>> = (0..frames)
                .map(|_| (0..parts).map(|_| gen_part(rng)).collect())
                .collect();
            let mut bytes = vec![parts as u8, frames as u8];
            for f in &poses {
                for p in f {
                    bytes.extend(pack(p));
                }
            }
            (poses, bytes)
        };
        let (poses, bytes) = stream_of(&mut rng, parts, frames);
        cpu.mem.load(STREAM, &bytes);
        // The window: a count, a start at or below its end and inside the
        // stream (a start on the frame count reads past the stream's end).
        let mut head = vec![0u8; 0x88];
        if rng.below(2) == 0 {
            let end = 1 + rng.below(frames as u32) as u8;
            head[0x84] = 1 + rng.below(3) as u8;
            head[0x85] = (rng.below(u32::from(end) + 1) as u8).min(frames as u8 - 1);
            head[0x86] = end;
        }
        let z_step = rng.below(0x200) as i16 - 0x100;
        for k in 0..0x88u32 {
            cpu.mem.wr8(ENTRY + k, head[k as usize]);
        }
        cpu.mem.wr16(ENTRY + 0xE, z_step as u16);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        let cycles_left = head[0x84] != 0 && rng.below(3) != 0;
        cpu.mem
            .wr8(actor + 0x21B, if cycles_left { head[0x84] } else { 0 });
        // The queued entry, its stream, and the idle entry.
        let staged = [0u8, 1, 2, 5, 0x0F, 0x10, 0x12][rng.below(7) as usize];
        let hp = if rng.below(5) == 0 {
            0
        } else {
            1 + rng.below(999) as u16
        };
        let to_idle = rng.below(4) == 0;
        let z_off = rng.below(4) == 0;
        let q_parts = if seat >= 3 && rng.below(4) == 0 {
            parts + 1
        } else {
            parts
        };
        let (qn, idn) = (1 + rng.below(3) as usize, 1 + rng.below(3) as usize);
        let (q_poses, q_bytes) = stream_of(&mut rng, q_parts, qn);
        let (i_poses, i_bytes) = stream_of(&mut rng, parts, idn);
        cpu.mem.load(QSTREAM, &q_bytes);
        cpu.mem.load(ISTREAM, &i_bytes);
        cpu.mem.wr32(QUEUED + 0x88, QSTREAM);
        cpu.mem.wr32(IDLE + 0x88, ISTREAM);
        for q in 0..0x14u32 {
            let e = if q == 0 { IDLE } else { QUEUED };
            cpu.mem.wr32(TABLE + q * 4, e);
            cpu.mem.wr32(RECORD + 0x4C + q * 4, e);
        }
        cpu.mem.wr32(TABLES, TABLE);
        cpu.mem.wr32(RECORDS, RECORD);
        cpu.mem.wr8(actor + 0x1DA, staged);
        cpu.mem.wr16(actor + 0x14C, hp);
        cpu.mem.wr8(actor + 0x1DC, if to_idle { 4 } else { 0 });
        cpu.mem.wr8(actor + 0x228, u8::from(z_off));
        cpu.mem.wr16(actor + 0x46, 0);
        cpu.mem.wr16(NODE + 0x72, 0x1000);
        cpu.mem.wr16(actor + 0x44, 0);
        let f = rng.below(frames as u32) as u16;
        let nibble = 1 + rng.below(15) as u16;
        cpu.mem.wr16(NODE + 0x68, f << 4 | nibble);
        assert_eq!(cpu.call(DECODE, &[NODE]), Stop::Returned, "{round}");

        // The port: the clip, its window state and the tween target.
        let mut block = bytes.clone();
        block.resize(2 + frames * parts * 9, 0);
        let anim = parse_animation_stream(&block, 0, 1, 0, 0, head.clone()).expect("stream");
        let mut p = MonsterAnimPlayer::new_one_shot(&anim).expect("player");
        if !cycles_left {
            p.release_loop_window();
        }
        let gate = hp != 0 && staged < 0x10;
        let target = if !gate {
            TweenTarget {
                frame0: None,
                z_bias: 0,
            }
        } else {
            let frame0 = if to_idle || staged == 0 {
                i_poses[0].clone()
            } else {
                q_poses[0].clone()
            };
            TweenTarget {
                frame0: Some(frame0),
                z_bias: if z_off { 0 } else { z_step },
            }
        };
        let last = usize::from(f) == frames - 1;
        let in_window =
            cycles_left && head[0x86] != 0 && usize::from(f) + 1 == usize::from(head[0x86]);
        windowed += u32::from(in_window);
        queued_blends += u32::from(last && !in_window && gate);
        p.set_tween_target(Some(target));
        let pose = p.pose_at_sixteenths(f << 4 | nibble);
        for i in 0..parts {
            let at = POOL + i as u32 * 0xC;
            let got: Vec<i16> = (0..6).map(|k| cpu.mem.rd16(at + k * 2) as i16).collect();
            let (t, r) = pose.bone_outputs[i];
            let want = [t[0], t[1], t[2], r[0], r[1], r[2]];
            if got != want && failures.len() < 12 {
                failures.push(format!(
                    "round {round} seat {seat} part {i} frame {f}/{frames} nibble {nibble} \
                     window {:?} cycles {cycles_left} staged {staged:#x} hp {hp} idle {to_idle} \
                     z_off {z_off} q_parts {q_parts}: retail {got:?} port {want:?}",
                    &head[0x84..0x87]
                ));
                break;
            }
        }
        let _ = &poses;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        windowed > 1000 && queued_blends > 1000,
        "{windowed} {queued_blends}"
    );
}

/// The burning-body emit loop at the decode's tail (`0x8004A5FC..0x8004A8D8`)
/// against `battle_impact_fx::burn_emit_point` / `burn_effect` and
/// `rumble::burning_body_write`, driven the way
/// `World::emit_battle_burn_sprites` drives them: one emit per `0x10` of
/// `ctx[+0x328]`, each a random object of the decoded pose jittered by the
/// node's `+0x58` size, fire for selector `1` past the red floor, the
/// selector-`2` effect and its rumble behind the vibration word, and the
/// `rand()` draws in retail's order.
#[test]
fn the_burn_emitter_spawns_what_retail_spawns() {
    differential!(scus(DECODE));
    use legaia_engine_battle_vm::battle_impact_fx::{burn_effect, burn_emit_point};
    use legaia_engine_system::rumble::burning_body_write;
    use psx_retail_exec::{A0, A1, A2, V0};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    const RAND: u32 = 0x8005_6798;
    const SPAWN: u32 = 0x801D_FDF0;
    const VIBRATION_OFF: u32 = 0x8008_45C8;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    for a in 0..0x1400u32 {
        cpu.mem.wr16(SIN + a * 2, sin12(a as u16) as u16);
        cpu.mem
            .wr16(COS + a * 2, sin12((a as u16).wrapping_add(0x400)) as u16);
    }
    let lcg = Rc::new(Cell::new(0u32));
    let stream = lcg.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    type Spawns = Vec<(u8, [i16; 3], i16)>;
    let spawned: Rc<RefCell<Spawns>> = Rc::new(RefCell::new(Vec::new()));
    let log = spawned.clone();
    cpu.hook(SPAWN, move |r, m| {
        let at = r.r[A1];
        log.borrow_mut().push((
            r.r[A0] as u8,
            [
                m.rd16(at) as i16,
                m.rd16(at + 2) as i16,
                m.rd16(at + 4) as i16,
            ],
            r.r[A2] as i16,
        ));
    });
    cpu.mem.wr32(NODE + 0x4C, ENTRY);
    cpu.mem.wr32(ENTRY + 0x88, STREAM);
    cpu.mem.wr16(NODE + 0x5A, 0);
    let rand = |s: &mut u32| -> u32 {
        *s = s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        (*s >> 16) & 0x7FFF
    };
    let mut rng = Rng(0x4255_524E);
    let (mut fires, mut puffs, mut quiet) = (0u32, 0u32, 0u32);
    for round in 0..20_000 {
        let parts = 1 + rng.below(8) as usize;
        let frames = 2 + rng.below(4) as usize;
        let mut stream = vec![parts as u8, frames as u8];
        let mut poses = Vec::new();
        for _ in 0..frames {
            let f: Vec<PartPose> = (0..parts).map(|_| gen_part(&mut rng)).collect();
            for p in &f {
                stream.extend(pack(p));
            }
            poses.push(f);
        }
        cpu.mem.load(STREAM, &stream);
        // A whole frame: the pool holds the raw decode.
        let f = rng.below(frames as u32) as usize;
        cpu.mem.wr16(NODE + 0x68, (f as u16) << 4);
        cpu.mem.wr8(ENTRY + 0x85, 0);
        cpu.mem.wr8(ENTRY + 0x86, 0);
        cpu.mem.wr8(ACTOR + 0x21B, 0);
        let selector = [0u8, 1, 1, 2, 2, 3][rng.below(6) as usize];
        let red = [0u8, 0x80, 0xAF, 0xB0, 0xFF][rng.below(5) as usize];
        let size = (8 + rng.below(0x400)) as i16;
        let facing = rng.below(0x1000) as u16;
        let base = [
            rng.below(0x800) as i16 - 0x400,
            -(rng.below(0x300) as i16) + 0x80,
            rng.below(0x800) as i16 - 0x400,
        ];
        let acc = [0x0Fi16, 0x10, 0x1F, 0x20, 0x35, 0x48][rng.below(6) as usize];
        let vibration_off = rng.below(3) == 0;
        cpu.mem.wr8(ACTOR + 0x21F, selector);
        cpu.mem.wr32(NODE + 0x74, u32::from(red));
        cpu.mem.wr16(NODE + 0x58, size as u16);
        cpu.mem.wr16(NODE + 0x72, 0x1000);
        cpu.mem.wr16(ACTOR + 0x46, facing);
        cpu.mem.wr16(ACTOR + 0x34, base[0] as u16);
        cpu.mem.wr16(ACTOR + 0x36, base[1] as u16);
        cpu.mem.wr16(ACTOR + 0x38, base[2] as u16);
        cpu.mem.wr16(ACTOR + 0x44, 0);
        cpu.mem.wr16(CTX + 0x328, acc as u16);
        cpu.mem.wr32(VIBRATION_OFF, u32::from(vibration_off));
        cpu.mem.wr32(0x8007_B92C, 0);
        cpu.mem.wr32(0x8007_B930, 0);
        let seed = rng.next();
        lcg.set(seed);
        spawned.borrow_mut().clear();
        assert_eq!(cpu.call(DECODE, &[NODE]), Stop::Returned, "{round}");
        let retail_rumble = (
            cpu.mem.rd32(0x8007_B92C) as i32,
            cpu.mem.rd32(0x8007_B930) as i32,
        );

        // The port's pieces, in the World emitter's order.
        let objects: Vec<[i16; 3]> = poses[f].iter().map(|p| [p.tx, p.ty, p.tz]).collect();
        let mut s = seed;
        let mut port_spawned = Vec::new();
        let mut port_rumble = (0, 0);
        if selector != 0 {
            for _ in 0..(acc as u16 / 0x10) {
                let idx = rand(&mut s) as usize % objects.len();
                let rands = [
                    rand(&mut s) as i32,
                    rand(&mut s) as i32,
                    rand(&mut s) as i32,
                ];
                let p = burn_emit_point(
                    base,
                    facing,
                    objects[idx],
                    size,
                    rands,
                    |a| sin12(a) as i32,
                    |a| sin12(a.wrapping_add(0x400)) as i32,
                );
                if p[1] <= 0
                    && let Some(fx) = burn_effect(selector, red)
                {
                    port_spawned.push((fx, p, facing as i16));
                    if selector == 2 && !vibration_off {
                        let (r0, r1) = (rand(&mut s), rand(&mut s));
                        let w = burning_body_write(r0, r1);
                        port_rumble = (w.pulse, w.level);
                    }
                }
            }
        }
        let retail_spawned = spawned.borrow().clone();
        match retail_spawned.first().map(|s| s.0) {
            Some(0x0B) => fires += 1,
            Some(_) => puffs += 1,
            None => quiet += 1,
        }
        assert_eq!(
            (retail_spawned, retail_rumble, lcg.get()),
            (port_spawned, port_rumble, s),
            "round {round} selector {selector} red {red:#x} size {size} facing {facing:#x} \
             acc {acc:#x} parts {parts} vibration_off {vibration_off}"
        );
    }
    assert!(
        fires > 500 && puffs > 500 && quiet > 500,
        "{fires} {puffs} {quiet}"
    );
}
