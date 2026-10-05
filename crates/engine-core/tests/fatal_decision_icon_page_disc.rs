//! Disc-gated: **where Fatal Decision's wheel icons come from**.
//!
//! PROT 0954's icon records (`0x801F86DC + id * 0x4C`) name tpage `0x8A`
//! (8bpp at `(640, 0)`) and CLUT `0x7A80` (row 490). Those are the side-band
//! applier's second texture target, which a monster turn streams from
//! `readef.DAT` slot `3 * group + 1`. The roulette's three casters (monster
//! ids 119..=121) all carry group `7`, so the page is slot 22 - and the
//! battle-entry VRAM log has to put exactly that page there.
//!
//! A retail capture pins the same page: the `player_steal_skeleton_*` states
//! (a Skeleton, group 7) hold `readef.DAT` slot 22 at `(640, 0)` with its
//! CLUT on row 490.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::summon_readef::SLOT_BYTES;
use legaia_engine_core::battle_sideband_textures::{
    formation_readef_groups, write_readef_group_textures,
};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn entry(dir: &std::path::Path, idx: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let e = archive.entries.get(idx).cloned().expect("entry");
    let mut bytes = Vec::new();
    archive.read_entry(&e, &mut bytes).expect("read entry");
    bytes
}

#[test]
fn the_fatal_decision_casters_stream_the_icon_sheet_to_the_page_the_icons_sample() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    // The icon records' sprite fields, off the module image.
    let module = entry(&dir, 954);
    const BASE: usize = 0x801F_69D8;
    for id in 0..16usize {
        let rec = 0x801F_86DC + id * 0x4C - BASE;
        let h = |o: usize| u16::from_le_bytes([module[rec + o], module[rec + o + 1]]);
        assert_eq!(h(0x32), 0x008A, "icon {id} tpage");
        assert_eq!(h(0x34), 0x7A80, "icon {id} CLUT (0, 490)");
    }

    let archive = entry(&dir, 867);
    assert_eq!(formation_readef_groups(&archive, &[119, 120, 121]), vec![7]);

    let readef = entry(&dir, 894);
    let mut vram = legaia_tim::Vram::new();
    assert_eq!(write_readef_group_textures(&readef, 7, &mut vram), 2);
    let slot = &readef[22 * SLOT_BYTES..23 * SLOT_BYTES];
    assert_eq!(u32::from_le_bytes(slot[..4].try_into().unwrap()), 2);
    let bytes = vram.as_bytes();
    let at = |x: usize, y: usize| (y * 1024 + x) * 2;
    assert_eq!(&bytes[at(0, 490)..at(0, 490) + 0x200], &slot[4..0x204]);
    for y in [0usize, 63, 128, 255] {
        let row = &slot[0x204 + y * 256..0x204 + (y + 1) * 256];
        assert_eq!(&bytes[at(640, y)..at(640, y) + 256], row, "page row {y}");
    }
    eprintln!("[ran] icon records -> (640,0)/row 490 <- readef slot 22");
}
