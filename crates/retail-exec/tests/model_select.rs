//! The placement spawner `FUN_8003A1E4`'s model-bank select against
//! `legaia_engine_scene::model_bank::resolve_model_id`: a synthetic scene
//! MAN whose placement record carries every model byte, the spawn
//! `FUN_80024C88` hooked to end the run where the select has stored its pool
//! index into the spawn template (`0x80073E5C`) and the translucent bit into
//! `s3`. The two bank-base globals hold the port's bases.

use crate::common::scus_cpu;
use legaia_engine_scene::model_bank::{PLAYER_BANK_BASE, SCENE_BANK_BASE, resolve_model_id};
use psx_retail_exec::{RA, RETURN_SENTINEL, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const SPAWNER: u32 = 0x8003_A1E4;
const SPAWN: u32 = 0x8002_4C88;
const MAN_PTR: u32 = 0x8007_B898;
const SCENE_BASE: u32 = 0x8007_B6F8;
const PLAYER_BASE: u32 = 0x8007_B824;
const TEMPLATE_ID: u32 = 0x8007_3E5C;
const MAN: u32 = 0x8012_0000;
const S3: usize = 19;

#[test]
fn the_placement_spawner_bank_select_matches_retail() {
    differential!(scus(SPAWNER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let seen = Rc::new(RefCell::new(None));
    let s = seen.clone();
    cpu.hook(SPAWN, move |r, m| {
        *s.borrow_mut() = Some((m.rd16(TEMPLATE_ID), r.r[S3]));
        r.r[RA] = RETURN_SENTINEL;
    });
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr16(SCENE_BASE, SCENE_BANK_BASE);
    cpu.mem.wr16(PLAYER_BASE, PLAYER_BANK_BASE);
    // Partition counts `[0, 1, 0]`: placement 0 is the table's first record.
    let mut man = vec![0u8; 0x40];
    man[0x24] = 1;
    // One 3-byte offset, then the record: no skipped pairs, the model byte,
    // its cell.
    man[0x2B..0x2E].copy_from_slice(&[0, 0, 0]);
    cpu.mem.load(MAN, &man);
    let record = MAN + 0x2E;
    for id in 0..=255u8 {
        cpu.mem.load(record, &[0, id, 3, 4]);
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(SPAWNER, &[0, 0]), Stop::Returned);
        let (pool, s3) = seen.borrow().expect("the spawn was reached");
        let port = resolve_model_id(i16::from(id));
        assert_eq!(pool, port.pool_index, "model byte {id:#x}");
        assert_eq!(s3 == 0x0100_0000, port.translucent, "model byte {id:#x}");
    }
}

/// The same spawner's placement read up to the spawn: the record found
/// through partition 1's offset table past partition 0, its local pairs
/// skipped (`0x8003A2B0..0x8003A2CC`), and the spawn vector off the tile
/// bytes, the half-tile bit 7 of each (`0x8003A328..0x8003A384`), against
/// `ManFile::actor_placement`.
#[test]
fn the_placement_spawner_reads_the_record_the_port_reads() {
    use crate::common::Rng;
    use legaia_scene_formats::man_section;
    use psx_retail_exec::A0;
    const S1: usize = 17;
    differential!(scus(SPAWNER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let seen = Rc::new(RefCell::new(None));
    let s = seen.clone();
    cpu.hook(SPAWN, move |r, m| {
        let v = r.r[A0];
        *s.borrow_mut() = Some((
            m.rd16(v) as i16,
            m.rd16(v + 2) as i16,
            m.rd16(v + 4) as i16,
            r.r[S1],
        ));
        r.r[RA] = RETURN_SENTINEL;
    });
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr16(SCENE_BASE, SCENE_BANK_BASE);
    cpu.mem.wr16(PLAYER_BASE, PLAYER_BANK_BASE);
    let mut rng = Rng(0x504C_4143);
    let (mut halves, mut skipped) = (0, 0);
    for case in 0..3000 {
        let (n0, n1) = (rng.below(4) as usize, 2 + rng.below(5) as usize);
        let mut man = vec![0u8; 0x2B];
        man[0x22..0x24].copy_from_slice(&(n0 as i16).to_le_bytes());
        man[0x24..0x26].copy_from_slice(&(n1 as i16).to_le_bytes());
        // The records, then the offset table in front of them.
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for _ in 0..n0 + n1 {
            offsets.push(data.len() as u32);
            let locals = rng.below(4) as usize;
            data.push(locals as u8);
            data.extend((0..2 * locals).map(|_| rng.next() as u8));
            data.extend([
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
            ]);
            data.extend([0u8; 4]);
        }
        for o in &offsets {
            man.extend(&o.to_le_bytes()[..3]);
        }
        man[0x28..0x2B].copy_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
        man.extend(&data);
        // The section chain behind the records: five one-byte sections and
        // the terminator.
        for _ in 0..5 {
            man.extend([1, 0, 0, 0]);
        }
        man.extend([0u8; 16]);
        cpu.mem.load(MAN, &man);
        let index = rng.below(n1 as u32) as usize;
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(SPAWNER, &[index as u32, 0]), Stop::Returned);
        let (x, y, z, header) = seen.borrow().expect("the spawn was reached");
        let file = man_section::parse(&man).expect("MAN");
        let p = file.actor_placement(&man, index).expect("placement");
        let what = format!("case {case}: n0 {n0} n1 {n1} index {index}");
        assert_eq!((x, y, z), (p.world_x, 0, p.world_z), "{what}");
        assert_eq!(
            header - MAN,
            (p.record_offset + 1 + 2 * p.local_count) as u32,
            "{what}: the header"
        );
        let hb = man[p.record_offset + 1 + 2 * p.local_count + 2];
        halves += usize::from(hb & 0x80 != 0);
        skipped += usize::from(p.local_count > 0);
    }
    assert!(halves > 500 && skipped > 1000, "{halves} {skipped}");
}
