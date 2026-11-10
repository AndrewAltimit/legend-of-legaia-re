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

/// The boot loader's PROT TOC read `FUN_8003E4E8`.
const TOC_LOAD: u32 = 0x8003_E4E8;

#[test]
fn the_toc_load_installs_the_ports_toc() {
    differential!(scus(TOC_LOAD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use psx_retail_exec::{A0, A1, A2};
    use std::cell::RefCell;
    use std::rc::Rc;
    const RAM_TOC: u32 = 0x801C_70F0;
    const NAME: u32 = 0x8015_0000;
    let prot = disc.read_named_file("PROT.DAT").expect("PROT.DAT");
    let head = prot[..3 * 0x800].to_vec();
    fn cstr(m: &mut psx_retail_exec::Memory, mut a: u32) -> Vec<u8> {
        let mut s = Vec::new();
        loop {
            let b = m.rd8(a);
            if b == 0 {
                return s;
            }
            s.push(b);
            a += 1;
        }
    }
    // BIOS A(0x15) `strcat`, which the name is built with.
    cpu.hook(0x8005_6728, |r, m| {
        let mut end = r.r[A0];
        while m.rd8(end) != 0 {
            end += 1;
        }
        let src = cstr(m, r.r[A1]);
        for (i, b) in src.iter().chain(std::iter::once(&0)).enumerate() {
            m.wr8(end + i as u32, *b);
        }
        r.r[V0] = r.r[A0];
    });
    cpu.stub(0x8005_6758, 0);
    cpu.stub(0x8005_BEFC, 1);
    cpu.stub(0x8005_EA84, 0);
    let searched: Rc<RefCell<Vec<u8>>> = Rc::default();
    {
        let searched = searched.clone();
        cpu.hook(0x8005_DBB4, move |r, m| {
            *searched.borrow_mut() = cstr(m, r.r[A1]);
            r.r[V0] = 1;
        });
    }
    let reads: Rc<RefCell<Vec<(u32, u32, u32)>>> = Rc::default();
    {
        let reads = reads.clone();
        cpu.hook(0x8005_E9A4, move |r, m| {
            reads.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]));
            m.load(r.r[A1], &head[..(r.r[A0] as usize * 0x800).min(head.len())]);
            r.r[V0] = 1;
        });
    }
    cpu.mem.load(NAME, b"prot.dat\0");
    assert_eq!(cpu.call(TOC_LOAD, &[NAME, 1]), Stop::Returned);
    let name = String::from_utf8_lossy(&searched.borrow()).into_owned();
    assert_eq!(name, "\\PROT.DAT;1", "the name searched");
    assert_eq!(*reads.borrow(), vec![(3, RAM_TOC, 0x80)]);
    let archive = legaia_prot::archive::Archive::from_bytes(prot).expect("archive");
    for (i, &w) in archive.toc.iter().take(3 * 0x200 - 2).enumerate() {
        assert_eq!(
            cpu.mem.rd32(RAM_TOC + 4 * (i as u32 + 2)),
            w,
            "TOC word {i} (RAM word {})",
            i + 2
        );
    }
    assert!(archive.toc.len() > 0x400);
}
