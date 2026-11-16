//! Disc-gated: the capture-class bodies that turn their caster onto the
//! target group's centroid, re-derived from the module bytes.
//!
//! Every capture-class module image (PROT 0935..=0966, slot-B base
//! `0x801F69D8`) is scanned for a `jal 0x801DCEAC` (`FUN_801DCEAC`, the
//! target group's centroid). In every entry that calls it, at least one call
//! heads the caster-turn idiom: the bearing `jal 0x80019B28` within a few
//! words, then `addiu v0,v0,0x800` / `andi v0,v0,0xfff` /
//! `sh v0,0x46(<caster>)` (Curse All also frames the centroid with a call of
//! its own). The set of entries that hold
//! it must be `cast_module_camera::CAPTURE_TARGET_AIM_BODIES`, the bodies the
//! world feeds the aim and whose directors port the turn
//! (`caster_turn_onto_target`).
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_cast::cast_module_camera::CAPTURE_TARGET_AIM_BODIES;
use std::path::PathBuf;

const CENTROID: u32 = 0x801D_CEAC;
const BEARING: u32 = 0x8001_9B28;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    eprintln!("[skip] extracted/PROT.DAT missing");
    None
}

fn jal_to(w: u32, target: u32) -> bool {
    w >> 26 == 3 && (((w & 0x03FF_FFFF) << 2) | 0x8000_0000) == target
}

/// `sh <rt>,0x46(<base>)`.
fn is_facing_store(w: u32) -> bool {
    w >> 26 == 0x29 && w & 0xFFFF == 0x46
}

#[test]
fn every_centroid_turn_in_the_band_is_a_fed_body() {
    let Some(dir) = extracted_dir() else { return };
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut found = Vec::new();
    for entry in 935u32..=966 {
        let e = archive.entries[entry as usize].clone();
        let mut bytes = Vec::new();
        archive
            .read_entry(&e, &mut bytes)
            .unwrap_or_else(|err| panic!("read PROT {entry}: {err:#}"));
        let words: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        let calls: Vec<usize> = (0..words.len())
            .filter(|&i| jal_to(words[i], CENTROID))
            .collect();
        if calls.is_empty() {
            continue;
        }
        // Each entry turns its caster off at least one of the calls; a call
        // may also only frame the centroid (Curse All's camera).
        let turns = calls.iter().any(|&i| {
            let tail = &words[i..(i + 24).min(words.len())];
            tail.iter()
                .position(|&w| jal_to(w, BEARING))
                .is_some_and(|b| tail[b..].iter().take(8).any(|&w| is_facing_store(w)))
        });
        assert!(turns, "PROT {entry}: no centroid call turns the caster");
        found.push(entry);
    }
    assert_eq!(found, CAPTURE_TARGET_AIM_BODIES);
}
