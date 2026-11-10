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
