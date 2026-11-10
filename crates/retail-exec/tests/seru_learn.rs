//! Two battle-overlay leaves the Seru-learn and magic-level edges run:
//!
//! - `FUN_801E92DC`, the learn commit, against `magic_xp::learn_spell_prepend`
//!   over whole character records: lists from empty to long, the shift of the
//!   three parallel arrays (ids `+0x13D`, levels `+0x161`, XP words `+0x8`),
//!   the new head and the count. Retail takes the id less `0x80` and re-adds
//!   it at the store (`0x801E93A8`); the port takes the stored id.
//! - `FUN_801F452C`, the "magic level increased" banner, against
//!   `magic_xp::magic_level_increased_message` over every spell id the cast
//!   path can level: the string it copies and appends into the context's
//!   message buffer `+0x1F9`.

use crate::common::{Rng, diff_offsets, disc, scus, scus_cpu};
use legaia_engine_battle::magic_xp::{learn_spell_prepend, magic_level_increased_message};
use legaia_game_tables::spell_names::SpellNameTable;
use legaia_save::CharacterRecord;
use psx_retail_exec::Stop;

const LEARN: u32 = 0x801E_92DC;
const BANNER: u32 = 0x801F_452C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

#[test]
fn a_learned_seru_lands_where_retail_lands_it() {
    differential!(prot(898, LEARN));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4C45_524E);
    let (mut empty, mut long) = (0, 0);
    for round in 0..4000 {
        let seat = rng.below(3);
        let roster = 1 + rng.below(4);
        cpu.mem.wr8(CTX + 0x13, seat as u8);
        cpu.mem.wr8(SEATS + seat, roster as u8);
        // Every byte of the record random, the list's count in reach of a
        // game and past it.
        let mut raw: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
        let count = match rng.below(4) {
            0 => 0,
            1 => 1 + rng.below(3),
            2 => rng.below(12),
            _ => rng.below(35),
        } as u8;
        raw[0x13C] = count;
        if count == 0 {
            empty += 1;
        }
        if count > 11 {
            long += 1;
        }
        let id = 0x81 + rng.below(0x15) as u8;
        let rec = RECORDS + (roster - 1) * STRIDE;
        cpu.mem.load(rec, &raw);
        assert_eq!(
            cpu.call(LEARN, &[u32::from(id.wrapping_sub(0x80))]),
            Stop::Returned
        );
        let retail = cpu.mem.read(rec, STRIDE as usize);

        let mut port = CharacterRecord::parse(&raw).unwrap();
        learn_spell_prepend(&mut port, id);
        assert!(
            retail == port.raw,
            "round {round} count {count} id {id:#x}: {:?}",
            diff_offsets(&retail, &port.raw)
        );
    }
    assert!(empty > 500 && long > 500, "{empty} {long}");
}

#[test]
fn the_magic_level_banner_reads_what_retail_writes() {
    differential!(prot(898, BANNER));
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let names = SpellNameTable::from_scus(&scus).expect("spell names");
    let actor = 0x8013_0000;
    let mut compared = 0;
    for seat in 0..3u32 {
        cpu.mem.wr32(ACTORS + seat * 4, actor + seat * 0x400);
    }
    for id in 0x81u8..=0x95 {
        let Some(name) = names.name(id) else { continue };
        for seat in 0..3u32 {
            cpu.mem.wr8(CTX + 0x13, seat as u8);
            cpu.mem.wr8(actor + seat * 0x400 + 0x1DF, id);
            // A stale line in the buffer, which the copy must replace.
            cpu.mem.load(CTX + 0x1F9, &[0x41; 0x60]);
            assert_eq!(cpu.call(BANNER, &[]), Stop::Returned);
            let buf = cpu.mem.read(CTX + 0x1F9, 0x80);
            let end = buf.iter().position(|&b| b == 0).expect("terminated");
            // The banner channel spells the element plate `0xCE op` as
            // `^` and the letter `op + 0x2D` (`World::spell_banner_name`).
            let retail = match &buf[..end] {
                [0xCE, op, rest @ ..] => {
                    format!("^{}{}", (op + 0x2D) as char, String::from_utf8_lossy(rest))
                }
                line => String::from_utf8_lossy(line).into_owned(),
            };
            let name = match names.icon(id) {
                Some(op) => format!("^{} {name}", (op + 0x2D) as char),
                None => name.to_string(),
            };
            let port = magic_level_increased_message(&name);
            assert_eq!(retail, port, "id {id:#x} seat {seat}");
            compared += 1;
        }
    }
    assert!(compared >= 30, "{compared}");
}
