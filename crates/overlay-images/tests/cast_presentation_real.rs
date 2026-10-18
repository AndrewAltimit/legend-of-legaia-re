//! Disc-gated: the slot-B band's presentation calls
//! (`legaia_overlay_images::cast_presentation`), decoded off every module and
//! checked against the fades and the CLUT copy three modules carry as hand
//! ports (PROT 0903, 0905, 0966).
//!
//! `LEGAIA_CAST_PRES_DUMP=1` prints every module's switch and sites.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_overlay_images::cast_presentation::{self, PresentationCall, SiteTiming};
use legaia_overlay_images::slot_b_module::{SLOT_B_LINK_BASE, content_end};
use std::collections::{BTreeMap, BTreeSet};
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

fn read_entry(archive: &mut legaia_prot::archive::Archive, idx: u32) -> Vec<u8> {
    let entry = archive.entries[idx as usize].clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read entry");
    bytes
}

fn dump(p: &cast_presentation::CastPresentation) -> String {
    let mut s = String::new();
    for d in &p.dispatches {
        s += &format!(
            "  body {:08X} load {:08X} arms {:X?} holding {:?}\n",
            d.body, d.load, d.arms, d.holding
        );
    }
    for x in &p.sites {
        s += &format!(
            "  {:08X} body {:08X} arm {:>3} {:?} {:?}\n",
            x.va, x.body, x.arm, x.timing, x.call
        );
    }
    for x in &p.unplaced {
        s += &format!("  {:08X} UNPLACED {:?}\n", x.va, x.call);
    }
    s
}

/// A resolved fade as `(kind, duration, start, end, delay, hold)`.
type Fade = (i16, i16, [i16; 3], [i16; 3], i16, i16);

fn fades_of(p: &cast_presentation::CastPresentation, arm: u8) -> Vec<Fade> {
    p.arm_sites(None, arm)
        .filter_map(|s| match s.call {
            PresentationCall::Fade(f) => f.resolved(),
            _ => None,
        })
        .map(|r| (r.kind, r.duration, r.start_rgb, r.end_rgb, r.delay, r.hold))
        .collect()
}

#[test]
fn decode_every_module() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] cast presentation: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let verbose = std::env::var_os("LEGAIA_CAST_PRES_DUMP").is_some();
    let (mut placed, mut resolved, mut all_sites) = (0, 0, 0);
    let mut by_entry = BTreeMap::new();
    for idx in 903..=966u32 {
        let bytes = read_entry(&mut archive, idx);
        let own = content_end(&bytes, SLOT_B_LINK_BASE);
        let p = cast_presentation::decode(&bytes, own);
        // Every module but the null stub opens on a phase switch.
        assert_eq!(
            p.dispatches.is_empty(),
            idx == 926,
            "PROT {idx} phase switch"
        );
        let mut seen = BTreeSet::new();
        for s in &p.sites {
            if let PresentationCall::Fade(f) = s.call
                && seen.insert(s.va)
            {
                placed += 1;
                resolved += usize::from(f.resolved().is_some());
            }
        }
        all_sites += seen.len()
            + p.unplaced
                .iter()
                .filter(|u| matches!(u.call, PresentationCall::Fade(_)))
                .count();
        if verbose {
            eprintln!("PROT {idx} own {own:#X}\n{}", dump(&p));
        }
        by_entry.insert(idx, p);
    }
    // Every `jal 0x80024E80` in the band's own code is found, placed or not.
    assert_eq!(all_sites, 258, "fade call sites across the band");
    assert!(placed >= 240, "{placed} fade sites placed on a phase arm");
    assert!(
        resolved + 2 >= placed,
        "{resolved} of {placed} templates resolve"
    );

    // PROT 0905's arm 2 rise and arm 4 pair - the fades Vera's director
    // carries by hand (`cast_module_camera::seru::VERA_RISE_FADE` /
    // `VERA_CUT_FADES`).
    let vera = &by_entry[&905];
    assert_eq!(
        fades_of(vera, 2),
        vec![(1, 0x20, [0, 0, 0], [0xFF, 0xFF, 0xFF], 0, -1)]
    );
    assert_eq!(
        fades_of(vera, 4),
        vec![
            (1, 0x40, [0xFF, 0xE0, 0x80], [0, 0, 0], 0, 0),
            (1, 0x20, [0, 0x1F, 0x7F], [0, 0, 0], 0, 0),
        ]
    );
    for s in vera.arm_sites(None, 4) {
        if matches!(s.call, PresentationCall::Fade(_)) {
            assert_eq!(s.timing, SiteTiming::Pass, "arm 4 spawns on its pass");
        }
    }
    // PROT 0966's arm 2 / 4 / 26 fades, as its hand port reads them
    // (`world::battle::cast_band::evil_seru_magic_fade`); arm 26 spawns
    // three, the last the delayed white-in.
    let evil = &by_entry[&966];
    assert_eq!(
        fades_of(evil, 2),
        vec![(1, 0x10, [0xFF, 0x40, 0x40], [0, 0, 0], 0, 0)]
    );
    assert_eq!(
        fades_of(evil, 4),
        vec![(1, 0x40, [0, 0, 0], [0xFF, 0xFF, 0xFF], 0, -1)]
    );
    let arm26 = fades_of(evil, 26);
    assert_eq!(arm26.len(), 3);
    assert_eq!(arm26[2], (1, 0x40, [0, 0, 0], [0xFF, 0xFF, 0xFF], 0xC0, -1));
    // PROT 0903's arm 8 effect-CLUT copy - Gimard's director's
    // `GIMARD_BREATH_CLUT_MOVE`.
    let gimard: Vec<_> = by_entry[&903].arm_sites(None, 8).map(|s| s.call).collect();
    assert!(gimard.contains(&PresentationCall::VramMove {
        rect: [Some(0xD0), Some(0x1DC), Some(16), Some(1)],
        dst: [Some(0xE0), Some(0x1DC)],
    }));
    // The stage byte `ctx[+0x278]`, against the retail states that hold it:
    // `cort_mystic_circle_mid_cast` reads `3` (PROT 0938's `0xB7` arm 0,
    // `0x801F6BA0`), `horn_summon_mid_cast` `2` in arm 2 (PROT 0930's arm 0,
    // `0x801F6B54`; arm 7 stores `3` in a `jal` delay slot, `0x801F73CC`),
    // `zeto_big_wave_mid_cast` `2` (PROT 0946's arm 0, `0x801F6BF8`).
    let stage = |entry: u32, va: u32| {
        by_entry[&entry].sites.iter().find_map(|s| match s.call {
            PresentationCall::StageByte { value } if s.va == va => Some((s.arm, value)),
            _ => None,
        })
    };
    assert_eq!(stage(938, 0x801F_6BA0), Some((0, Some(3))));
    assert_eq!(stage(930, 0x801F_6B54), Some((0, Some(2))));
    assert_eq!(stage(930, 0x801F_73CC), Some((7, Some(3))));
    assert_eq!(stage(946, 0x801F_6BF8), Some((0, Some(2))));
    // A tick port that writes the byte by hand agrees with the decode:
    // Gimard's cut and tunnel (`GIMARD_CUT_CTX_278` / `GIMARD_TUNNEL_CTX_278`).
    assert_eq!(stage(903, 0x801F_6F2C), Some((4, Some(3))));
    assert_eq!(stage(903, 0x801F_7170), Some((6, Some(2))));
}
