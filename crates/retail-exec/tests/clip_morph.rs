//! The monster clip morph `FUN_800495C8` against the port's three pieces:
//! `parse_clip_morph` over the descriptor bytes the routine reads,
//! `clip_morph_weight` for each lane's envelope at the actor's clip cursor,
//! and `apply_clip_morphs` for the blend (the GTE blend `FUN_8005B038` runs in
//! the interpreter).
//!
//! The actor's clip entry carries the two lane pointers at `+0x04` / `+0x08`
//! (either may be null), its seat `+0x5A` names the record whose `+0x230`
//! model holds the TMD object table (`+0xC`, 28-byte stride, vertex pointer
//! first). Compared: every object's vertices after the call, and the rest
//! copies the routine saves into the work buffer behind `gp[0xA0C] + 0x1060`.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::monster_archive::{
    apply_clip_morphs, clip_morph_weight, parse_clip_morph,
};
use psx_retail_exec::Stop;

const MORPH: u32 = 0x8004_95C8;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const SEAT: u32 = 0x8013_1000;
const ENTRY: u32 = 0x8013_2000;
const MODEL: u32 = 0x8013_3000;
const VERTS: u32 = 0x8014_0000;
const MORPHS: u32 = 0x8015_0000;
const WORK_CTX: u32 = 0x8016_0000;
const WORK: u32 = 0x8017_0000;
/// `gp[0xA0C]` (the battle context pointer) and `gp[0xA50]`.
const WORK_CTX_PTR: u32 = 0x8007_BD24;
const WORK_PTR: u32 = 0x8007_BD68;

fn tmd(objects: &[Vec<[i16; 3]>]) -> legaia_tmd::Tmd {
    legaia_tmd::Tmd {
        header: legaia_tmd::Header {
            id: 0x8000_0002,
            flags: 0,
            nobj: objects.len() as u32,
            flist_bit_set: false,
        },
        objects: objects
            .iter()
            .map(|verts| legaia_tmd::Object {
                header: legaia_tmd::ObjectHeader {
                    vert_top: 0,
                    n_vert: verts.len() as u32,
                    normal_top: 0,
                    n_normal: 0,
                    prim_top: 0,
                    n_primitive: 0,
                    scale: 0,
                },
                vertices: verts
                    .iter()
                    .map(|v| legaia_tmd::Vector {
                        x: v[0],
                        y: v[1],
                        z: v[2],
                        _pad: 0,
                    })
                    .collect(),
                normals: Vec::new(),
                primitives_byte_offset: 0,
                primitives_byte_size: 0,
                claimed_n_primitive: 0,
                primitives_psx_walk: None,
            })
            .collect(),
    }
}

#[test]
fn the_clip_morph_blends_what_retail_blends() {
    differential!(scus(MORPH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x434C_4D50);
    let (mut ramps, mut holds, mut idle, mut lanes_run) = (0, 0, 0, 0);
    for round in 0..3_000 {
        let n_obj = 1 + rng.below(3) as usize;
        let objects: Vec<Vec<[i16; 3]>> = (0..n_obj)
            .map(|_| {
                (0..8 + rng.below(24))
                    .map(|_| [rng.next() as i16, rng.next() as i16, rng.next() as i16])
                    .collect()
            })
            .collect();
        // The model: an object table whose rows point at each object's
        // vertices (`x, y, z, pad` halfwords).
        let seat_idx = rng.below(8);
        cpu.mem.wr32(ACTORS + seat_idx * 4, SEAT);
        cpu.mem.wr32(SEAT + 0x230, MODEL);
        let mut vbase = VERTS;
        for (o, verts) in objects.iter().enumerate() {
            cpu.mem.wr32(MODEL + 0xC + 28 * o as u32, vbase);
            for (k, v) in verts.iter().enumerate() {
                let a = vbase + 8 * k as u32;
                cpu.mem.wr16(a, v[0] as u16);
                cpu.mem.wr16(a + 2, v[1] as u16);
                cpu.mem.wr16(a + 4, v[2] as u16);
                cpu.mem.wr16(a + 6, 0);
            }
            vbase += 8 * verts.len() as u32 + 0x40;
        }
        // The two lanes' descriptors.
        let mut block = vec![0u8; 0x1000];
        let mut lane_offsets = [None; 2];
        let mut at = 0usize;
        for lane in lane_offsets.iter_mut() {
            if rng.below(4) == 0 {
                continue;
            }
            let mut keys = [0u8; 4];
            let mut k = rng.below(8) as u8;
            for key in keys.iter_mut() {
                k += rng.below(6) as u8;
                *key = k;
            }
            *lane = Some(at as u32);
            block[at] = rng.next() as u8;
            block[at + 1..at + 5].copy_from_slice(&keys);
            let records = 1 + rng.below(3);
            block[at + 8..at + 12].copy_from_slice(&records.to_le_bytes());
            at += 0xC;
            for _ in 0..records {
                let obj = rng.below(n_obj as u32);
                let n = objects[obj as usize].len() as u32;
                let first = rng.below(n);
                let count = 1 + rng.below(n - first);
                for w in [obj, first, count] {
                    block[at..at + 4].copy_from_slice(&w.to_le_bytes());
                    at += 4;
                }
                for _ in 0..count * 4 {
                    let d = (rng.below(0x2000) as i16 - 0x1000).to_le_bytes();
                    block[at..at + 2].copy_from_slice(&d);
                    at += 2;
                }
            }
            at = (at + 3) & !3;
        }
        cpu.mem.load(MORPHS, &block);
        for (i, off) in lane_offsets.iter().enumerate() {
            cpu.mem
                .wr32(ENTRY + 4 + 4 * i as u32, off.map_or(0, |o| MORPHS + o));
        }
        let phase = rng.below(0x22 * 16) as i16;
        cpu.mem.wr32(ACTOR + 0x4C, ENTRY);
        cpu.mem.wr16(ACTOR + 0x5A, seat_idx as u16);
        cpu.mem.wr16(ACTOR + 0x68, phase as u16);
        cpu.mem.wr32(WORK_CTX_PTR, WORK_CTX);
        cpu.mem.wr32(WORK_PTR, WORK);
        assert_eq!(cpu.call(MORPH, &[ACTOR]), Stop::Returned, "round {round}");

        let morphs: Vec<_> = lane_offsets
            .iter()
            .map(|o| o.map(|o| parse_clip_morph(&block, o).expect("a descriptor")))
            .collect();
        let mut model = tmd(&objects);
        let lanes: Vec<_> = morphs
            .iter()
            .flatten()
            .map(|m| (m, clip_morph_weight(m.keys, phase)))
            .collect();
        for &(_, w) in &lanes {
            lanes_run += 1;
            match w {
                0 => idle += 1,
                0x1000 => holds += 1,
                _ => ramps += 1,
            }
        }
        apply_clip_morphs(&mut model, &lanes);
        let mut vbase = VERTS;
        for (o, obj) in model.objects.iter().enumerate() {
            for (k, v) in obj.vertices.iter().enumerate() {
                let a = vbase + 8 * k as u32;
                let got = [
                    cpu.mem.rd16(a) as i16,
                    cpu.mem.rd16(a + 2) as i16,
                    cpu.mem.rd16(a + 4) as i16,
                ];
                assert_eq!(
                    got,
                    [v.x, v.y, v.z],
                    "round {round}: object {o} vertex {k} phase {phase:#x}"
                );
            }
            vbase += 8 * obj.vertices.len() as u32 + 0x40;
        }
        // The rest copies: each record's run as it stands just before that
        // record blends (an earlier record over the same vertices has moved
        // them), lane after lane, record after record.
        let mut saved = Vec::new();
        let mut running = tmd(&objects);
        for (m, w) in &lanes {
            for r in &m.records {
                let obj = &running.objects[r.object as usize];
                for v in &obj.vertices[r.first_vertex as usize..][..r.deltas.len()] {
                    for c in [v.x, v.y, v.z, 0] {
                        saved.extend_from_slice(&c.to_le_bytes());
                    }
                }
                let one = legaia_actor_formats::monster_archive::ClipMorph {
                    keys: m.keys,
                    records: vec![r.clone()],
                };
                apply_clip_morphs(&mut running, &[(&one, *w)]);
            }
        }
        assert_eq!(
            cpu.mem.read(WORK, saved.len()),
            saved,
            "round {round}: rest copies"
        );
    }
    assert!(
        ramps > 500 && holds > 200 && idle > 500,
        "{ramps} {holds} {idle} of {lanes_run}"
    );
}
