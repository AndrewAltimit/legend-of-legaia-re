//! Disc-gated: the battle-heap placement law for monster blocks and the
//! four-slot event-list read it decides
//! (`legaia_actor_formats::monster_archive::{monster_block_bases,
//! four_slot_beats}`), against values read out of retail battle save states.
//!
//! Every row below is a measurement, not disc data: the payload address the
//! seat's record pointer (`0x801C9348[seat]`) held in a retail PCSX-Redux
//! battle state, with the seated party count (`0x8007BD10[0..3]`) and the
//! formation (`0x8007BD0C[0..4]`) of that state. The law has to reproduce
//! each address from the disc's block sizes alone.
//!
//! Skips when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use legaia_actor_formats::monster_archive;
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

type Case = (usize, [u16; 4], &'static [(u16, u32)]);

/// `(seated party, formation, [(id, measured block payload address)])`.
const PLACEMENTS: &[Case] = &[
    // Noa and Terra (the AI-companion seat counts) - a mednafen state.
    (2, [25, 22, 0, 0], &[(25, 0x800AA8DC), (22, 0x800B0BD0)]),
    (1, [10, 0, 0, 0], &[(10, 0x800AFD4C)]),
    (1, [4, 0, 0, 0], &[(4, 0x800AD4F8)]),
    (3, [1, 0, 0, 0], &[(1, 0x800AA984)]),
    (3, [7, 4, 0, 0], &[(7, 0x800A836C), (4, 0x800AD3A8)]),
    (3, [4, 4, 0, 0], &[(4, 0x800AD3A8)]),
    (1, [79, 0, 0, 0], &[(79, 0x800A89C4)]),
    (1, [163, 0, 0, 0], &[(163, 0x800A5580)]),
    (1, [164, 0, 0, 0], &[(164, 0x800A5268)]),
    (1, [162, 0, 0, 0], &[(162, 0x800A4A20)]),
    (1, [182, 0, 0, 0], &[(182, 0x800A66A8)]),
    (3, [181, 0, 0, 0], &[(181, 0x800A2CE0)]),
    (3, [166, 0, 0, 0], &[(166, 0x800A64D4)]),
    (3, [114, 131, 0, 0], &[(114, 0x800AEEB0), (131, 0x800A0C20)]),
    (3, [33, 68, 0, 0], &[(33, 0x800AA73C), (68, 0x800A2BF0)]),
    (
        3,
        [141, 141, 150, 0],
        &[(141, 0x800AD6E4), (150, 0x8009F0A0)],
    ),
    (3, [113, 113, 24, 0], &[(113, 0x800A3EA8), (24, 0x800AE904)]),
    (3, [55, 52, 0, 0], &[(55, 0x800A1704), (52, 0x800ACB08)]),
    (1, [4, 1, 0, 0], &[(4, 0x8009E570), (1, 0x800AAAD4)]),
    (3, [159, 159, 0, 0], &[(159, 0x800A815C)]),
    (3, [93, 41, 0, 0], &[(93, 0x8009C68C), (41, 0x800AA964)]),
    (3, [161, 161, 161, 0], &[(161, 0x800A8158)]),
    (1, [104, 104, 0, 0], &[(104, 0x800AB008)]),
    (1, [37, 31, 31, 0], &[(37, 0x8009E76C), (31, 0x800AA890)]),
    (3, [145, 145, 0, 0], &[(145, 0x800B0034)]),
    (3, [114, 89, 0, 0], &[(114, 0x800A2A04), (89, 0x800AD460)]),
    (3, [89, 144, 0, 0], &[(89, 0x800AD460), (144, 0x800A3B8C)]),
    (3, [160, 160, 0, 0], &[(160, 0x800A8158)]),
    (3, [101, 45, 0, 0], &[(101, 0x800A69B0), (45, 0x800AE5D8)]),
    (3, [134, 101, 0, 0], &[(134, 0x800A5004), (101, 0x800B1CE4)]),
    (3, [144, 80, 0, 0], &[(144, 0x800A57EC), (80, 0x800AF0C0)]),
    (3, [70, 17, 0, 0], &[(70, 0x800A7A78), (17, 0x800AD40C)]),
    (3, [70, 86, 0, 0], &[(70, 0x800B3F78), (86, 0x800A7F80)]),
    (1, [19, 0, 0, 0], &[(19, 0x800B08E4)]),
    (1, [13, 0, 0, 0], &[(13, 0x800AAEF8)]),
    (3, [3, 108, 0, 0], &[(3, 0x800AA984), (108, 0x8009A788)]),
    (3, [40, 108, 0, 0], &[(40, 0x800AA968), (108, 0x8009A76C)]),
];

/// `(seated party, formation, id, entry index, measured four-slot beat)`:
/// the byte the anim tick's `FUN_80050E00` read lands on in RAM.
const BEATS: &[(usize, [u16; 4], u16, usize, u8)] = &[
    (3, [3, 108, 0, 0], 3, 5, 209),
    (3, [1, 0, 0, 0], 1, 5, 209),
    (1, [4, 1, 0, 0], 1, 5, 16),
    (3, [101, 45, 0, 0], 101, 6, 0),
    // The read lands on `+0x8B`, the high byte of the relocated `+0x88`
    // stream pointer.
    (3, [134, 101, 0, 0], 101, 6, 0x80),
    (3, [70, 86, 0, 0], 86, 9, 227),
    (3, [113, 113, 24, 0], 24, 5, 215),
    (1, [19, 0, 0, 0], 19, 9, 8),
    (1, [164, 0, 0, 0], 164, 0x0C, 0),
    (2, [25, 22, 0, 0], 22, 5, 0),
];

fn bases(entry: &[u8], seated: usize, form: &[u16; 4]) -> std::collections::BTreeMap<u16, u32> {
    monster_archive::monster_block_bases(seated, form, |id| {
        monster_archive::decode_block(entry, id)
            .ok()
            .flatten()
            .and_then(|b| monster_archive::block_heap_bytes(&b))
    })
}

#[test]
fn block_bases_match_retail_battle_states() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    for &(seated, form, want) in PLACEMENTS {
        let got = bases(&entry, seated, &form);
        for &(id, addr) in want {
            assert_eq!(
                got.get(&id).copied(),
                Some(addr),
                "seated {seated} formation {form:?} id {id}"
            );
        }
    }
}

#[test]
fn four_slot_beats_match_retail_battle_states() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    for &(seated, form, id, idx, want) in BEATS {
        let base = bases(&entry, seated, &form)[&id];
        let block = monster_archive::decode_block(&entry, id).unwrap().unwrap();
        let beats = monster_archive::four_slot_beats(&block, base);
        assert_eq!(
            beats.get(idx).copied().flatten(),
            Some(want),
            "seated {seated} formation {form:?} id {id} entry {idx:#x}"
        );
    }
}
