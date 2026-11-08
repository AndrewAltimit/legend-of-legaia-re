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
