//! The New Game seed: `FUN_800560B4`, the party-record expander, against
//! `new_game::seed_live_records` over the disc's own starting-party template,
//! and `FUN_80034A6C`, the world-state seed that calls it, against
//! `new_game_seed_words`, the starting inventory `StartingInventory::from_scus`
//! and the entry seat `NEW_GAME_ENTRY_SEAT`.
//!
//! The save context `SC` (`0x80084140`) is filled with noise first, so every
//! store the routine makes is visible; the whole block is compared with the
//! noise plus the port's stores. The stores the halfword list does not carry
//! (each record's next-level threshold, the level byte pair, the name copy) are
//! checked on their own.

use crate::common::{Rng, diff_offsets, scus, scus_cpu};
use legaia_game_tables::new_game::{
    self as ng, NEW_GAME_ENTRY_SEAT, SeedWidth, StartingInventory, StartingParty,
};
use psx_retail_exec::{A0, A1, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const EXPAND: u32 = 0x8005_60B4;
const SEED: u32 = 0x8003_4A6C;
const SC: u32 = 0x8008_4140;
const SC_LEN: usize = 0x1A18;
const STRCPY: u32 = 0x8005_6758;
/// `_DAT_8007B868`, the debug word the seed's tail tests.
const DEBUG_WORD: u32 = 0x8007_B868;

/// The four records' next-level thresholds `+0x4` (`sw` at
/// `0x800560F8..0x80056120`), literals the halfword list does not carry; the
/// engine takes them from the level curve.
const XP_WORDS: [(u32, u32); 4] = [(0x5CC, 0x79), (0x9E0, 0x66), (0xDF4, 0x8C), (0x1208, 0x79)];

/// The name copies `strcpy` was asked for, as `(dst, src)`.
type Names = Rc<RefCell<Vec<(u32, u32)>>>;

fn seeded_cpu() -> Option<(Cpu, Names)> {
    let mut cpu = scus_cpu()?;
    let names: Names = Rc::default();
    {
        let names = names.clone();
        cpu.hook(STRCPY, move |r, _| {
            names.borrow_mut().push((r.r[A0], r.r[A1]))
        });
    }
    Some((cpu, names))
}

/// The noise block with the expander's stores laid over it.
fn expanded(noise: &[u8], party: &StartingParty) -> Vec<u8> {
    let mut want = noise.to_vec();
    for s in ng::seed_live_records(party) {
        let o = s.sc_offset as usize;
        want[o..o + 2].copy_from_slice(&s.value.to_le_bytes());
    }
    for (o, v) in XP_WORDS {
        want[o as usize..o as usize + 4].copy_from_slice(&v.to_le_bytes());
    }
    for slot in 0..4u32 {
        let base = (ng::LIVE_RECORD_0_XP_OFFSET + slot * ng::LIVE_RECORD_STRIDE) as usize;
        want[base + ng::LEVEL_OFFSET as usize] = 1;
        want[base + ng::SEEDED_UNREAD_OFFSET as usize] = 1;
    }
    want
}

fn check_names(names: &[(u32, u32)]) {
    let want: Vec<(u32, u32)> = (0..4)
        .map(|n| {
            (
                SC + ng::seeded_name_offset(n),
                ng::PARTY_TEMPLATE_VA + n as u32 * ng::RECORD_STRIDE as u32 + 0x10,
            )
        })
        .collect();
    assert_eq!(names, want, "name copies");
}

#[test]
fn the_party_expander_seeds_what_retail_seeds() {
    differential!(scus(EXPAND));
    let (Some((mut cpu, names)), Some(exe)) = (seeded_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let party = StartingParty::from_scus(&exe).expect("template");
    let mut rng = Rng(0x5345_4544);
    for round in 0..8 {
        let noise: Vec<u8> = (0..SC_LEN).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SC, &noise);
        names.borrow_mut().clear();
        assert_eq!(cpu.call(EXPAND, &[]), Stop::Returned, "round {round}");
        let got = cpu.mem.read(SC, SC_LEN);
        let want = expanded(&noise, &party);
        assert_eq!(got, want, "round {round}: {:?}", diff_offsets(&got, &want));
        check_names(&names.borrow());
        for n in 0..4 {
            assert_eq!(cpu.mem.rd8(0x8007_BDBC + n), 0xFF);
        }
    }
}

#[test]
fn the_world_state_seed_is_retails() {
    differential!(scus(SEED));
    let (Some((mut cpu, names)), Some(exe)) = (seeded_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let party = StartingParty::from_scus(&exe).expect("template");
    let inventory = StartingInventory::from_scus(&exe).expect("inventory");
    let mut rng = Rng(0x5747_5344);
    for round in 0..8 {
        let noise: Vec<u8> = (0..SC_LEN).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SC, &noise);
        cpu.mem.wr32(DEBUG_WORD, 0);
        names.borrow_mut().clear();
        assert_eq!(cpu.call(SEED, &[]), Stop::Returned, "round {round}");
        let got = cpu.mem.read(SC, SC_LEN);
        let mut want = noise.clone();
        for w in ng::new_game_seed_words() {
            let o = w.sc_offset as usize;
            match w.width {
                SeedWidth::Byte => want[o] = w.value as u8,
                SeedWidth::Word => want[o..o + 4].copy_from_slice(&w.value.to_le_bytes()),
            }
        }
        want = expanded(&want, &party);
        let flags = ng::STORY_FLAGS_SC_OFFSET as usize;
        want[flags..flags + ng::STORY_FLAGS_LEN as usize].fill(0);
        for (k, &(id, count)) in inventory.items().iter().enumerate() {
            let o = ng::INVENTORY_SC_OFFSET as usize + 2 * k;
            want[o] = id;
            want[o + 1] = count;
        }
        assert_eq!(got, want, "round {round}: {:?}", diff_offsets(&got, &want));
        check_names(&names.borrow());
        let seat = (
            cpu.mem.rd32(0x8007_3EF4) as i16,
            cpu.mem.rd32(0x8007_3EF8) as i16,
        );
        assert_eq!(seat, NEW_GAME_ENTRY_SEAT);
        assert_eq!(cpu.mem.rd32(0x8007_3EFC), 0);
    }
}
