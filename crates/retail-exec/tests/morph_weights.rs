//! The `4C D8` morph-weight actor's tick `FUN_8002174C` against
//! `apply_morph_weights` and `MorphWeightEnvelope::tick`: every group's
//! vertex buffer after the rest-pose restore and the weighted blend (the GTE
//! blend `FUN_8005B038` runs in the interpreter), and the ping-pong weight
//! with its direction. Blocks of one record (what the disc ships) and of
//! several, two naming one group, at the routine's `n_vert * 0x60` slot
//! pitch.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::morph_weight_apply::{
    MorphWeightEnvelope, apply_morph_weights, slot_span,
};
use psx_retail_exec::Stop;

const TICK: u32 = 0x8002_174C;
const ACTOR: u32 = 0x8013_0000;
const TMD: u32 = 0x8014_0000;
const VERTS: u32 = 0x8015_0000;
const BLOCK: u32 = 0x8016_0000;
const REST: u32 = 0x8017_0000;

#[test]
fn the_morph_weight_tick_matches_retail() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D4F_5250);
    let (mut records_seen, mut flips) = (0, 0);
    for run in 0..1500 {
        let n_groups = 1 + rng.below(4) as usize;
        let counts: Vec<usize> = (0..n_groups).map(|_| 1 + rng.below(12) as usize).collect();
        let mut groups: Vec<Vec<u8>> = counts
            .iter()
            .map(|&n| (0..n * 8).map(|_| rng.next() as u8).collect())
            .collect();
        // The object table at TMD + 0xC, 0x1C stride.
        let mut at = VERTS;
        for (g, verts) in groups.iter().enumerate() {
            let obj = TMD + 0xC + 0x1C * g as u32;
            cpu.mem.wr32(obj, at);
            cpu.mem.wr32(obj + 4, (verts.len() / 8) as u32);
            cpu.mem.load(at, verts);
            at += 0x200;
        }
        // The block: records at the n_vert * 0x60 pitch.
        let n_rec = if rng.below(2) == 0 {
            1
        } else {
            1 + rng.below(3) as usize
        };
        let mut block = (n_rec as u32).to_le_bytes().to_vec();
        let mut rest = Vec::new();
        for r in 0..n_rec {
            let gid = if r > 0 && rng.below(3) == 0 {
                u32::from_le_bytes(block[4..8].try_into().unwrap())
            } else {
                rng.below(n_groups as u32)
            };
            let n = counts[gid as usize];
            let first = rng.below(n as u32) as usize;
            // At least one delta: `FUN_8005B038` is a do-while that tests
            // its end pointer only after a step, so a zero count walks off
            // until the pointer wraps (the port applies nothing).
            let cnt = 1 + rng.below((n - first) as u32) as usize;
            let mut rec = Vec::new();
            rec.extend(gid.to_le_bytes());
            rec.extend((first as u32).to_le_bytes());
            rec.extend((cnt as u32).to_le_bytes());
            for _ in 0..cnt {
                for _ in 0..3 {
                    rec.extend((rng.next() as i16 >> rng.below(6)).to_le_bytes());
                }
                rec.extend([0, 0]);
            }
            rec.resize(slot_span(n), 0xA5);
            block.extend(rec);
            rest.extend((0..n * 8).map(|_| rng.next() as u8));
        }
        cpu.mem.load(BLOCK, &block);
        cpu.mem.load(REST, &rest);
        let mut env = MorphWeightEnvelope {
            weight: [0i16, 0x800, 0x1000, rng.below(0x1001) as i16][rng.below(4) as usize],
            up_rate: rng.below(0x200) as i16 - 0x20,
            down_rate: rng.below(0x200) as i16 - 0x20,
            descending: rng.below(2) == 0,
        };
        let dt = [1u8, 2, 3][rng.below(3) as usize];
        cpu.mem.wr8(0x1F80_0393, dt);
        cpu.mem.wr32(ACTOR + 0x48, TMD);
        cpu.mem.wr32(ACTOR + 0x4C, BLOCK);
        cpu.mem.wr32(ACTOR + 0x90, REST);
        cpu.mem.wr16(ACTOR + 0x6E, env.weight as u16);
        cpu.mem.wr16(ACTOR + 0x3C, env.up_rate as u16);
        cpu.mem.wr16(ACTOR + 0x3E, env.down_rate as u16);
        cpu.mem.wr16(ACTOR + 0x40, u16::from(env.descending));
        assert_eq!(cpu.call(TICK, &[ACTOR]), Stop::Returned, "run {run}");

        records_seen += apply_morph_weights(&block, &mut groups, &rest, env.weight);
        let was = env.descending;
        env.tick(dt);
        flips += usize::from(was != env.descending);
        let label = format!(
            "run {run}: counts {counts:?} records {n_rec} weight {}",
            env.weight
        );
        for (g, verts) in groups.iter().enumerate() {
            let retail = cpu.mem.read(VERTS + 0x200 * g as u32, verts.len());
            assert_eq!(
                &retail,
                verts,
                "{label} group {g}: {:?}",
                crate::common::diff_offsets(&retail, verts)
            );
        }
        assert_eq!(cpu.mem.rd16(ACTOR + 0x6E) as i16, env.weight, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x40) != 0, env.descending, "{label}");
    }
    assert!(records_seen > 2000 && flips > 100, "{records_seen} {flips}");
}
