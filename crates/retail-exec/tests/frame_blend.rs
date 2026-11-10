//! The field actor frame blender `FUN_8001BE80` off the disc, with the angle
//! lerp `FUN_8001D088` live beneath it, against
//! `PlayerAnmBundle::sample_bone`: random clips (bone and frame counts, the
//! blend gate), every part and frame, sub-frame cursors, the hold bit and the
//! loop wrap. Compared: the pose the blender leaves at `0x1F8002C0` (the
//! translation it hands the GTE and the three angles), and the GTE's own
//! `MVMVA` of that translation through the part matrix at `0x1F8002D4`.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::player_anm::{ANM_MARKER_1, parse};
use psx_retail_exec::Stop;

const BLEND: u32 = 0x8001_BE80;
const LERP: u32 = 0x8001_D088;
const ACTOR: u32 = 0x8010_0000;
const RECORD: u32 = 0x8011_0000;
const POSE: u32 = 0x1F80_02C0;
const MATRIX: u32 = 0x1F80_02D4;

#[test]
fn the_frame_blender_matches_sample_bone() {
    differential!(scus(BLEND));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x424C_4E44);
    let (mut blended, mut wrapped, mut held, mut flipped) = (0, 0, 0, 0);
    for round in 0..400 {
        let bones = 1 + rng.below(20) as usize;
        let frames = 1 + rng.below(8) as usize;
        let blends = rng.below(4) != 0;
        let a = bones as u16 | if blends { 0x100 } else { 0 };
        let mut rec = Vec::new();
        rec.extend(a.to_le_bytes());
        rec.extend((frames as u16).to_le_bytes());
        rec.extend(ANM_MARKER_1.to_le_bytes());
        rec.extend(2u16.to_le_bytes());
        for _ in 0..bones * frames * 8 {
            rec.push(rng.next() as u8);
        }
        rec.extend([0u8; 8]);
        let mut bundle = Vec::new();
        bundle.extend(1u32.to_le_bytes());
        bundle.extend(8u32.to_le_bytes());
        bundle.extend(&rec);
        let parsed = parse(&bundle).expect("synthetic bundle parses");
        cpu.mem.load(RECORD, &rec);

        for _ in 0..24 {
            let frame = rng.below(frames as u32) as usize;
            let part = rng.below(bones as u32) as usize;
            let frac = if rng.below(4) == 0 { 0 } else { rng.below(16) };
            let cursor = ((frame as u32) << 4 | frac) as i16;
            let hold = rng.below(2) == 0;
            cpu.mem.load(ACTOR, &[0u8; 0x80]);
            cpu.mem.wr16(ACTOR + 0x68, cursor as u16);
            cpu.mem.wr16(
                ACTOR + 0x62,
                if hold { 8 } else { 0 } | rng.below(8) as u16 & !8,
            );
            cpu.mem.wr32(ACTOR + 0x4C, RECORD);
            let m: Vec<u32> = (0..8).map(|_| rng.next()).collect();
            for (k, w) in m.iter().enumerate() {
                // Rotation halfwords, then a modest translation.
                let w = if k >= 5 { w & 0xFFF } else { *w };
                cpu.mem.wr32(MATRIX + 4 * k as u32, w);
            }
            let entry = RECORD + 8 + ((frame * bones + part) * 8) as u32;
            assert_eq!(
                cpu.call(BLEND, &[ACTOR, entry, RECORD, part as u32]),
                Stop::Returned
            );
            let port = parsed.sample_bone(0, cursor, hold, part).expect("in range");
            let ctx = format!(
                "round {round} bones {bones} frames {frames} blends {blends} frame {frame} part {part} frac {frac} hold {hold}"
            );
            let got: Vec<u16> = [0u32, 2, 4, 8, 10, 12]
                .iter()
                .map(|o| cpu.mem.rd16(POSE + o))
                .collect();
            let want: Vec<u16> = [port.t_x, port.t_y, port.t_z, port.r_x, port.r_y, port.r_z]
                .iter()
                .map(|v| *v as u16)
                .collect();
            assert_eq!(got, want, "pose, {ctx}");

            // The MVMVA the blender leaves for its caller: the part matrix
            // times the blended translation, plus the matrix translation.
            let rot: Vec<i64> = (0..9)
                .map(|k| i64::from(cpu.mem.rd16(MATRIX + 2 * k) as i16))
                .collect();
            let t = [port.t_x, port.t_y, port.t_z].map(|v| i64::from(v as i16));
            for r in 0..3 {
                let tr = i64::from(cpu.mem.rd32(MATRIX + 0x14 + 4 * r as u32) as i32);
                let sum =
                    (tr << 12) + rot[3 * r] * t[0] + rot[3 * r + 1] * t[1] + rot[3 * r + 2] * t[2];
                let mac = (sum >> 12) as i32;
                assert_eq!(cpu.gte.read_data(25 + r) as i32, mac, "MAC{}, {ctx}", r + 1);
            }

            if blends && frac != 0 {
                blended += 1;
                assert!(cpu.calls.contains(&LERP), "the lerp ran, {ctx}");
                if frame == frames - 1 {
                    if hold {
                        held += 1;
                    } else {
                        wrapped += 1;
                    }
                }
                flipped += usize::from(cpu.calls.iter().filter(|c| **c == LERP).count() == 6);
            }
        }
    }
    assert!(
        blended > 1000 && wrapped > 50 && held > 50 && flipped > 20,
        "blended {blended} wrapped {wrapped} held {held} flipped {flipped}"
    );
}

#[test]
fn the_angle_lerp_matches_its_journal_and_counter() {
    differential!(scus(LERP));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const COUNTER: u32 = 0x8007_BD28;
    const JOURNAL: u32 = 0x8008_91A8;
    let mut rng = Rng(0x4C45_5250);
    for _ in 0..20000 {
        let from = rng.next() as i16 as i32;
        let to = rng.next() as i16 as i32;
        let frac = rng.below(16) as i32;
        let slot = rng.below(3);
        let start = rng.below(0x4000);
        cpu.mem.wr32(COUNTER, start);
        let args = [from as u32, to as u32, frac as u32, slot];
        assert_eq!(cpu.call(LERP, &args), Stop::Returned);
        let port = legaia_actor_formats::player_anm::lerp_angle_12_journaled(from, to, frac);
        let ctx = format!("from {from:#x} to {to:#x} frac {frac}");
        assert_eq!(
            cpu.regs.r[psx_retail_exec::V0],
            u32::from(port.value),
            "{ctx}"
        );
        assert_eq!(
            cpu.mem.rd32(COUNTER),
            start + port.magnitude as u32,
            "counter, {ctx}"
        );
        let j = JOURNAL + 8 * slot;
        assert_eq!(cpu.mem.rd16(j) as i16 as i32, port.from_unwrapped, "{ctx}");
        assert_eq!(
            cpu.mem.rd16(j + 2) as i16 as i32,
            port.to_unwrapped,
            "{ctx}"
        );
    }
}
