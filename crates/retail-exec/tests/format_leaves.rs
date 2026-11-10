//! Small `SCUS_942.54` leaves whose ports sit in the format crates:
//!
//! - `FUN_8005567C`, the battle-id formation fill, against
//!   `encounter_record::expand_battle_id` over every byte id: the formation
//!   cell `0x8007BD0C..0F`, the roster bytes `0x8007BD10..12` the boss ids
//!   seed, and the boss-arm byte `0x8007B64A` the id-`0` fallback clears.
//! - `FUN_8003CBF8`, the delimiter locator, against
//!   `field_disasm::packet::delimited_field_offset` over generated glyph
//!   strings with escapes, NULs and every `nth` from `0`.
//! - `FUN_8003E68C`, the in-RAM TOC's sector span, against
//!   `runtime_toc::entry_sector_span` over the disc's own TOC.

use crate::common::{Rng, disc, scus_cpu};
use legaia_scene_formats::encounter_record::expand_battle_id;
use legaia_scene_formats::field_disasm::delimited_field_offset;
use psx_retail_exec::{Stop, V0};

const BATTLE_ID_FILL: u32 = 0x8005_567C;
const LOCATE: u32 = 0x8003_CBF8;
const TOC_SPAN: u32 = 0x8003_E68C;

#[test]
fn the_battle_id_fill_is_retails() {
    differential!(scus(BATTLE_ID_FILL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ID: u32 = 0x8007_B7FC;
    const CELL: u32 = 0x8007_BD0C;
    const BOSS_ARM: u32 = 0x8007_B64A;
    let mut rng = Rng(0x4249_4446);
    for id in 0..=255u8 {
        for _ in 0..8 {
            let noise: Vec<u8> = (0..8).map(|_| rng.next() as u8).collect();
            cpu.mem.load(CELL, &noise);
            let arm = rng.next() as u8 | 1;
            cpu.mem.wr8(BOSS_ARM, arm);
            cpu.mem.wr16(ID, u16::from(id));
            assert_eq!(cpu.call(BATTLE_ID_FILL, &[]), Stop::Returned, "id {id:#x}");
            let f = expand_battle_id(id);
            let mut want = noise.clone();
            want[..4].copy_from_slice(&f.cell);
            if matches!(id, 0xA2..=0xA4) {
                want[4..7].copy_from_slice(&f.expansion);
            }
            assert_eq!(cpu.mem.read(CELL, 8), want, "id {id:#x}");
            let cleared = cpu.mem.rd8(BOSS_ARM) == 0;
            assert_eq!(cleared, f.clears_boss_arm, "id {id:#x}");
        }
    }
}

#[test]
fn the_delimiter_locator_is_retails() {
    differential!(scus(LOCATE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const TEXT: u32 = 0x8014_0000;
    const DEV_FLAG: u32 = 0x8007_B98C;
    cpu.mem.wr32(DEV_FLAG, 0);
    let mut rng = Rng(0x4445_4C4D);
    let (mut found, mut missed) = (0, 0);
    for round in 0..40_000 {
        let delimiter = [0xC1u8, 0x7C, 0x20, 0xCE][rng.below(4) as usize];
        let mut text = Vec::new();
        for _ in 0..rng.below(32) {
            match rng.below(6) {
                0 => text.push(delimiter),
                1 => text.extend([0xC0 | rng.below(16) as u8, rng.next() as u8]),
                _ => text.push(0x20 + rng.below(0x60) as u8),
            }
        }
        text.push(0);
        text.extend((0..8).map(|_| 0x20 + rng.below(0x60) as u8));
        text.push(0);
        let nth = rng.below(5);
        cpu.mem.load(TEXT, &text);
        assert_eq!(
            cpu.call(LOCATE, &[TEXT, u32::from(delimiter), nth]),
            Stop::Returned,
            "round {round}"
        );
        let retail = cpu.regs.r[V0];
        let port = delimited_field_offset(&text, delimiter, nth);
        // Retail answers `0` both for a hit at the head and for no hit.
        assert_eq!(
            retail,
            port.unwrap_or(0) as u32,
            "round {round}: {text:02x?} {delimiter:#x} nth {nth}"
        );
        if port.is_some() {
            found += 1;
        } else {
            missed += 1;
        }
    }
    assert!(found > 5_000 && missed > 5_000, "{found} {missed}");
}

#[test]
fn the_toc_span_is_retails() {
    differential!(scus(TOC_SPAN));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const RAM_TOC: u32 = 0x801C_70F0;
    // The boot loader copies the TOC's first three sectors verbatim.
    let prot = disc.read_named_file("PROT.DAT").expect("PROT.DAT");
    let head = &prot[..3 * 0x800];
    cpu.mem.load(RAM_TOC, head);
    let words: Vec<u32> = head
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    for i in 0..words.len() - 3 {
        assert_eq!(cpu.call(TOC_SPAN, &[i as u32]), Stop::Returned);
        let port = legaia_prot::runtime_toc::entry_sector_span(&words, i);
        assert_eq!(Some(cpu.regs.r[V0]), port, "RAM index {i}");
    }
}
