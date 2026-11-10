//! The party battle mesh's assembly off the disc: the object splice
//! `FUN_800536BC`, called once per equipment section as `FUN_80052FA0` calls
//! it, and the post-pass `FUN_80053898` that retags the surplus meshes and
//! sorts the objects by bone, over the four player files (extraction
//! `0863..0866`) and random loadouts, the sections decoded by the port's own
//! `decode_record`. Held against `assemble_character`: the merged TMD from its
//! magic to the end of the data pool, the tag bytes, the attach bytes the
//! post-pass leaves after them, and the battle-context byte `+0x240 + seat`.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_actor_formats::battle_char_assembly::{
    SECTION_COUNT, assemble_character, relocate_tsb_cba, select_sections,
};
use legaia_actor_formats::battle_data_pack::{decode_record, parse};
use psx_retail_exec::Stop;

const SPLICE: u32 = 0x8005_36BC;
const POST: u32 = 0x8005_3898;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8010_0000;
const CURSORS: u32 = 0x8010_2000;
const BLOB: u32 = 0x8011_0000;
const BLOB_LEN: usize = 0x3_0000;
const SECTIONS: u32 = 0x8014_0000;
/// `Cpu::call`'s stack top: the fifth argument sits at `+0x10`.
const STACK: u32 = 0x801F_FF00;

#[test]
fn the_battle_mesh_splice_matches_the_assembler() {
    differential!(scus(SPLICE), scus(POST));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x5350_4C43);
    let (mut built, mut pinned, mut fe) = (0usize, 0usize, 0usize);
    for (seat, entry) in (863..=866).enumerate() {
        let buf = disc.read_entry(entry).expect("player file");
        let pack = parse(&buf).expect("battle data pack");
        // Each section's candidate ids, in table order.
        let mut ids: Vec<Vec<u32>> = vec![Vec::new(); SECTION_COUNT];
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
        let seat = seat as u32 % 3;
        for round in 0..60 {
            let equipped: [u8; SECTION_COUNT] = std::array::from_fn(|k| {
                let c = &ids[k];
                c[rng.below(c.len() as u32) as usize] as u8
            });
            let Ok(port) = assemble_character(&buf, &pack, &equipped) else {
                continue;
            };
            let records = select_sections(&pack, &equipped).expect("sections");

            // The merged blob as the caller seats it: tags from `+0`, the TMD
            // header at `+0x18`, the object table at `+0x24`, the data
            // cursor at `+0x2C4`.
            cpu.mem.load(BLOB, &vec![0; BLOB_LEN]);
            cpu.mem.wr32(BLOB + 0x18, 0x8000_0002);
            cpu.mem.wr32(CURSORS, BLOB + 0x20);
            cpu.mem.wr32(CURSORS + 4, BLOB + 0x24);
            cpu.mem.wr32(CURSORS + 8, BLOB + 0x2C4);
            cpu.mem.wr32(CURSORS + 12, BLOB);
            cpu.mem.wr8(CTX + 0x240 + seat, 0xEE);
            let mut at = SECTIONS;
            for rec in &records {
                let d = decode_record(&buf, &pack, rec.index).expect("decode").bytes;
                cpu.mem.load(at, &d);
                let frame = at + u32::from_le_bytes(d[0..4].try_into().unwrap());
                cpu.mem.wr32(STACK + 0x10, CURSORS + 12);
                let args = [frame, CURSORS, CURSORS + 4, CURSORS + 8];
                assert_eq!(cpu.call(SPLICE, &args), Stop::Returned);
                at = (at + d.len() as u32 + 0xF) & !0xF;
            }
            assert_eq!(cpu.call(POST, &[BLOB, seat]), Stop::Returned);

            let ctx = format!("entry {entry} round {round} loadout {equipped:x?}");
            let nobj = port.bone_tags.len();
            assert_eq!(
                cpu.mem.rd32(BLOB + 0x20) as usize,
                nobj,
                "object count, {ctx}"
            );
            let got = cpu.mem.read(BLOB + 0x18, port.tmd.len());
            assert_eq!(
                got,
                port.tmd,
                "merged TMD, {ctx}: {:?}",
                diff_offsets(&got, &port.tmd)
            );
            assert_eq!(
                cpu.mem.rd32(CURSORS + 8),
                BLOB + 0x18 + port.tmd.len() as u32,
                "data cursor, {ctx}"
            );
            assert_eq!(cpu.mem.read(BLOB, nobj), port.bone_tags, "tags, {ctx}");
            let ff = port.attach_bones.len();
            assert_eq!(
                cpu.mem.read(BLOB + nobj as u32, ff),
                port.attach_bones,
                "attach bytes, {ctx}"
            );
            let has_fe = port.bone_tags.iter().any(|t| (100..200).contains(t));
            let byte = cpu.mem.rd8(CTX + 0x240 + seat);
            if has_fe {
                assert_eq!(byte, port.pinned_pair, "pinned pair, {ctx}");
                pinned += usize::from(port.pinned_pair != 0);
                fe += 1;
            } else {
                assert_eq!(byte, 0xEE, "no 0xFE surplus leaves the byte, {ctx}");
            }
            built += 1;
        }
    }
    assert!(
        built > 150 && fe > 0,
        "built {built} fe {fe} pinned {pinned}"
    );
}

const RELOCATE: u32 = 0x8005_3A28;

/// The registered blob's form: each object's primitive pointer absolute
/// (`+0x10` of a 7-word entry), as the relocation walks it.
fn absolutize(tmd: &mut [u8], base: u32) {
    let nobj = u32::from_le_bytes(tmd[8..12].try_into().unwrap()) as usize;
    for k in 0..nobj {
        let at = 12 + 0x1C * k + 0x10;
        let rel = u32::from_le_bytes(tmd[at..at + 4].try_into().unwrap());
        tmd[at..at + 4].copy_from_slice(&(base + 12 + rel).to_le_bytes());
    }
}

/// The battle texture relocation `FUN_80053A28` over every assembled mesh of
/// the loadouts above and every party slot, on the blob as registered,
/// against `relocate_tsb_cba`: the whole TMD after the walk.
#[test]
fn the_texture_relocation_matches_relocate_tsb_cba() {
    differential!(scus(RELOCATE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        return;
    };
    let mut rng = Rng(0x5453_4243);
    let mut rewritten = 0usize;
    for entry in 863..=866 {
        let buf = disc.read_entry(entry).expect("player file");
        let pack = parse(&buf).expect("battle data pack");
        let mut ids: Vec<Vec<u32>> = vec![Vec::new(); SECTION_COUNT];
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
        for round in 0..40 {
            let equipped: [u8; SECTION_COUNT] = std::array::from_fn(|k| {
                let c = &ids[k];
                c[rng.below(c.len() as u32) as usize] as u8
            });
            let Ok(port) = assemble_character(&buf, &pack, &equipped) else {
                continue;
            };
            let slot = rng.below(3) as u8;
            let mut want = port.tmd.clone();
            rewritten += relocate_tsb_cba(&mut want, slot).expect("relocate");
            absolutize(&mut want, BLOB);
            let mut seat = port.tmd.clone();
            absolutize(&mut seat, BLOB);
            cpu.mem.load(BLOB, &seat);
            assert_eq!(cpu.call(RELOCATE, &[BLOB, u32::from(slot)]), Stop::Returned);
            let got = cpu.mem.read(BLOB, want.len());
            assert_eq!(
                got,
                want,
                "entry {entry} round {round} slot {slot}: {:?}",
                diff_offsets(&got, &want)
            );
        }
    }
    assert!(rewritten > 1000, "rewritten {rewritten}");
}
