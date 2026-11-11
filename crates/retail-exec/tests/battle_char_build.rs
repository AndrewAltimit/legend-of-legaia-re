//! The party battle-mesh assembler `FUN_80052FA0` run whole, the three
//! seats at once over the player files (extraction `0863..0866`) and random
//! loadouts, from the per-seat stream table `0x801C92F0` the loader leaves.
//! Live beneath it: the LZS decoder, the object splice and post-pass, the
//! texture-upload helper `FUN_80053B9C` and the two record copiers. The
//! battle seed `FUN_80053CB8` (its own differential) is stubbed; `LoadImage`
//! (`FUN_800583C8`) is hooked and its rects and words logged.
//!
//! Held against the port's pieces: `assemble_character` (the merged TMD at
//! the record's `+0x50`, its tags and attach bytes), `swing_battle_animations`
//! (each installed swing entry and its `+0x88` stream pointer) and
//! `record0_texture_uploads` / `section_texture_upload` (every upload, in
//! order, CLUT row then pixels). Not compared: the attach-key scan that
//! links each section's attach-object records into the matching action
//! entries' `+0x04` / `+0x08`, which no port kernel carries.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_actor_formats::battle_char_assembly::{
    SECTION_COUNT, SWING_COST_OFFSET, TextureUpload, assemble_character, record0_texture_uploads,
    section_texture_upload, select_sections, swing_battle_animations,
};
use legaia_actor_formats::battle_data_pack::{decode_record, parse};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const ASSEMBLE: u32 = 0x8005_2FA0;
const SEED: u32 = 0x8005_3CB8;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const ALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const STREAMS: u32 = 0x801C_92F0;
const RECORDS: u32 = 0x801C_9360;
const PARTY: u32 = 0x8007_BD10;
const HEAP_START: u32 = 0x8007_B85C;
const HEAP_CURSOR: u32 = 0x8007_BD7C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x801E_0000;
const INPUT: u32 = 0x800D_0000;
const INPUT_STRIDE: u32 = 0x2_A000;
const HEAP: u32 = 0x8015_0000;
const HEAP_END: u32 = 0x801B_0000;
const SCRATCH: u32 = 0x801B_0000;
const RING: u32 = 0x801C_A000;

/// One `LoadImage`: `(x, y, w, h)` and the words it read.
type Image = ([i16; 4], Vec<u8>);

fn expected_images(u: &TextureUpload) -> Vec<Image> {
    let clut = (
        [u.clut_x as i16, u.clut_row() as i16, u.clut.len() as i16, 1],
        u.clut_bytes(),
    );
    let pixels = (
        [
            u.fb_x() as i16,
            u.fb_y() as i16,
            u.rect.w as i16,
            u.rect.h as i16,
        ],
        u.pixels.clone(),
    );
    vec![clut, pixels]
}

#[test]
fn the_party_mesh_assembler_matches_the_port() {
    differential!(scus(ASSEMBLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Three record[0] decodes and fifteen sections through the LZS decoder.
    cpu.step_budget = 400_000_000;
    cpu.stub(SEED, 0);
    cpu.stub(FREE, 0);
    // The assembler's `0x19000` scratch, and the LZS decoder's own ring.
    cpu.hook(ALLOC, |r, _| {
        r.r[V0] = if r.r[A1] == 0x1014 { RING } else { SCRATCH }
    });
    let images = Rc::new(RefCell::new(Vec::<Image>::new()));
    {
        let images = images.clone();
        cpu.hook(LOAD_IMAGE, move |r, m| {
            let rect: [i16; 4] = std::array::from_fn(|k| m.rd16(r.r[A0] + 2 * k as u32) as i16);
            let len = (rect[2].max(0) as usize) * (rect[3].max(0) as usize) * 2;
            images.borrow_mut().push((rect, m.read(r.r[A1], len)));
        });
    }
    let files: Vec<_> = (863..=866)
        .map(|e| {
            let buf = d.read_entry(e).expect("player file");
            let pack = parse(&buf).expect("pack");
            (buf, pack)
        })
        .collect();
    let ids: Vec<Vec<Vec<u32>>> = files
        .iter()
        .map(|(_, pack)| {
            let mut ids = vec![Vec::new(); SECTION_COUNT];
            let mut s = 0;
            for r in &pack.records {
                if s >= SECTION_COUNT {
                    break;
                }
                ids[s].push(r.id);
                if r.id == 0 {
                    s += 1;
                }
            }
            ids
        })
        .collect();
    let mut rng = Rng(0x4255_494C);
    let (mut built, mut swings, mut uploads) = (0usize, 0usize, 0usize);
    for round in 0..40 {
        // Three seats, each a player file and a loadout the port can build.
        let mut seats = Vec::new();
        while seats.len() < 3 {
            let f = rng.below(files.len() as u32) as usize;
            let equipped: [u8; SECTION_COUNT] = std::array::from_fn(|k| {
                let c = &ids[f][k];
                c[rng.below(c.len() as u32) as usize] as u8
            });
            let (buf, pack) = &files[f];
            if assemble_character(buf, pack, &equipped).is_ok() {
                seats.push((f, equipped));
            }
        }
        cpu.mem.load(CTX, &[0; 0x1100]);
        // The heap as a fresh battle leaves it: the merged TMD's table is
        // sized for 24 objects, and the bytes between a shorter table and
        // the data (`blob + 0x2C4`) are whatever the heap held.
        cpu.mem.load(HEAP, &vec![0; (HEAP_END - HEAP) as usize]);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(HEAP_START, HEAP);
        for (seat, (f, equipped)) in seats.iter().enumerate() {
            let (buf, pack) = &files[*f];
            let base = INPUT + seat as u32 * INPUT_STRIDE;
            // The file head through `record[0]`, then the five sections.
            let mut at = base;
            cpu.mem.load(at, &buf[..pack.table_offset]);
            cpu.mem.wr32(STREAMS + seat as u32 * 0x1C, at);
            at += (pack.table_offset as u32 + 0xF) & !0xF;
            for (k, rec) in select_sections(pack, equipped).unwrap().iter().enumerate() {
                let o = rec.file_offset(pack.data_base);
                let bytes = &buf[o..(o + rec.size as usize).min(buf.len())];
                cpu.mem.load(at, bytes);
                cpu.mem
                    .wr32(STREAMS + seat as u32 * 0x1C + 4 * (k as u32 + 1), at);
                at += (bytes.len() as u32 + 0xF) & !0xF;
            }
            assert!(at <= base + INPUT_STRIDE, "seat {seat} input fits");
            cpu.mem.wr8(PARTY + seat as u32, 1 + *f as u8);
        }
        images.borrow_mut().clear();
        assert_eq!(cpu.call(ASSEMBLE, &[]), Stop::Returned, "round {round}");
        assert!(cpu.mem.rd32(HEAP_CURSOR) <= HEAP_END, "the heap fits");
        assert_eq!(cpu.mem.rd8(CTX), 3, "seats counted");

        let mut want_images = Vec::new();
        for (seat, (f, equipped)) in seats.iter().enumerate() {
            let (buf, pack) = &files[*f];
            let ctx = format!(
                "round {round} seat {seat} file {} loadout {equipped:x?}",
                863 + f
            );
            let rec = cpu.mem.rd32(RECORDS + 4 * seat as u32);

            let port = assemble_character(buf, pack, equipped).unwrap();
            let blob = cpu.mem.rd32(rec + 0x50);
            let nobj = port.bone_tags.len();
            assert_eq!(cpu.mem.rd32(blob + 0x20) as usize, nobj, "objects, {ctx}");
            let got = cpu.mem.read(blob + 0x18, port.tmd.len());
            assert_eq!(
                got,
                port.tmd,
                "merged TMD, {ctx}: {:?}",
                diff_offsets(&got, &port.tmd)
            );
            assert_eq!(cpu.mem.read(blob, nobj), port.bone_tags, "tags, {ctx}");
            assert_eq!(
                cpu.mem.read(blob + nobj as u32, port.attach_bones.len()),
                port.attach_bones,
                "attach bytes, {ctx}"
            );

            let records = select_sections(pack, equipped).unwrap();
            for s in swing_battle_animations(buf, pack, equipped).expect("swings") {
                let p = cpu.mem.rd32(rec + 4 * u32::from(s.slot));
                let entry = decode_record(buf, pack, records[s.section].index)
                    .unwrap()
                    .bytes;
                let off = {
                    let w = if s.slot == 0xF { 8 } else { 4 };
                    u32::from_le_bytes(entry[w..w + 4].try_into().unwrap()) as usize
                };
                let (parts, frames) = (entry[off + 0xAC] as usize, entry[off + 0xAD] as usize);
                let len = 0xAC + ((parts * frames * 9 + 5) >> 2) * 4;
                let src = &entry[off..(off + len).min(entry.len())];
                let mut copied = cpu.mem.read(p, src.len());
                // The `+0x88` stream word is the copy's own `+0xAC`.
                let mut want = src.to_vec();
                want[0x88..0x8C].copy_from_slice(&(p + 0xAC).to_le_bytes());
                // `+0x04` / `+0x08` take the attach-object links the key scan
                // writes (`0x800533E4..0x80053450`), which the swing kernel
                // does not carry; the words beside them are the record's.
                copied[4..0xC].copy_from_slice(&want[4..0xC]);
                assert_eq!(
                    copied,
                    want,
                    "swing slot {:#x}, {ctx}: {:?}",
                    s.slot,
                    diff_offsets(&copied, &want)
                );
                assert_eq!(cpu.mem.rd8(p), s.entry_tag, "{ctx}");
                assert_eq!(cpu.mem.rd8(p + SWING_COST_OFFSET as u32), s.cost, "{ctx}");
                swings += 1;
            }

            let slot = seat as u8;
            for u in record0_texture_uploads(buf, slot).expect("record[0] uploads") {
                want_images.extend(expected_images(&u));
            }
            for (k, r) in records.iter().enumerate() {
                let e = decode_record(buf, pack, r.index).unwrap();
                if let Some(u) = section_texture_upload(&e.bytes, k, slot).expect("section upload")
                {
                    want_images.extend(expected_images(&u));
                }
            }
            built += 1;
        }
        let got = images.borrow().clone();
        assert_eq!(got.len(), want_images.len(), "uploads, round {round}");
        for (k, (g, w)) in got.iter().zip(&want_images).enumerate() {
            assert_eq!(g.0, w.0, "upload {k} rect, round {round}");
            assert!(
                g.1 == w.1,
                "upload {k} words, round {round}: {:?}",
                diff_offsets(&g.1, &w.1)
            );
        }
        uploads += got.len();
    }
    assert!(
        built >= 120 && swings > 200 && uploads > 400,
        "built {built} swings {swings} uploads {uploads}"
    );
}
