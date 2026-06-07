//! The SFX cues' **program bank** is the active scene's music VAB.
//!
//! `DAT_8006F198` keys each sound effect to `[program, tone, ...]` (see
//! `legaia_asset::sfx_table`). Those program ids index a VAB - and that VAB is
//! not a dedicated SFX master: it is the per-scene `scene_vab_stream` bank that
//! the BGM sequencer has open. The retail SFX voice setup `FUN_80065034` reads
//! the libsnd "current bank" globals (`_DAT_801ce33c` header base,
//! `_DAT_801ce334` ProgAtr at `+0x20`, `_DAT_801ce340` VagAtr at `+0x820`), so
//! sound effects play through the same bank the music does.
//!
//! Three legs, all skip + pass without the extracted disc / save library:
//!
//! * `sfx_programs_resolve_in_the_music01_bank` - the SFX descriptors' program
//!   ids land on populated programs of a real scene VAB (PROT 1004 `music_01`).
//! * `live_music01_bank_is_byte_identical_to_disc` - in every catalogued state
//!   whose open bank is the `music_01` VAB, the live header + program
//!   attributes match the disc bank (only the PsyQ reserved pointer fields are
//!   runtime-patched).
//! * `sfx_bank_varies_per_scene` - across captures the open bank is many
//!   different VABs, proving it tracks the scene rather than a fixed master.
//!
//! Note: scene banks range from 1 program (cutscene stings) to 16, so a cue's
//! program/tone resolves only in scenes whose bank is big enough - SFX
//! availability is scene-dependent, not a guaranteed reservation.

use legaia_asset::sfx_table::SfxTable;
use legaia_mednafen::{SaveState, ScenarioManifest, extract, ram_slice};
use std::path::{Path, PathBuf};

/// libsnd "current sound bank" VAB-header base pointer.
const VAB_HEADER_PTR_VA: u32 = 0x801C_E33C;
/// `fsize` of the `music_01` scene VAB (16 programs / 27 tones / 16 VAGs).
const MUSIC01_FSIZE: u32 = 0x0002_2660;

fn first_existing<const N: usize>(cands: [&str; N]) -> Option<PathBuf> {
    cands.into_iter().map(PathBuf::from).find(|p| p.exists())
}

fn scus() -> Option<PathBuf> {
    first_existing(["extracted/SCUS_942.54", "../../extracted/SCUS_942.54"])
}

fn music01_prot() -> Option<PathBuf> {
    first_existing([
        "extracted/PROT/1004_music_01.BIN",
        "../../extracted/PROT/1004_music_01.BIN",
    ])
}

#[test]
fn sfx_programs_resolve_in_the_music01_bank() {
    let (Some(scus_path), Some(vab_path)) = (scus(), music01_prot()) else {
        eprintln!("[skip] extracted SCUS / music_01 PROT entry not present");
        return;
    };
    let table = SfxTable::from_scus(&std::fs::read(&scus_path).expect("read SCUS"))
        .expect("parse SFX table");
    // The music_01 scene VAB sits at +4 (the `[u32 chunk header][VAB]` wrapper).
    let vab_bytes = std::fs::read(&vab_path).expect("read music_01 PROT entry");
    let vab = legaia_vab::parse(&vab_bytes, 4).expect("parse music_01 VAB");

    let max_program = table.active().map(|(_, d)| d.program).max().unwrap();
    assert!(
        (vab.programs.len() as u8) > max_program,
        "music_01 VAB has {} programs, SFX uses up to program {max_program}",
        vab.programs.len()
    );
    // Every program a sound effect references is a real, populated program of
    // this scene bank.
    for (id, d) in table.active() {
        let prog = vab
            .programs
            .get(d.program as usize)
            .unwrap_or_else(|| panic!("cue {id:#x} program {} missing from VAB", d.program));
        assert!(
            prog.tones > 0,
            "cue {id:#x} program {} unpopulated",
            d.program
        );
    }
    eprintln!(
        "[ok] all {} SFX cues' programs resolve in the music_01 bank ({} programs)",
        table.active().count(),
        vab.programs.len()
    );
}

fn library_dir() -> Option<PathBuf> {
    first_existing(["saves/library", "../saves/library", "../../saves/library"])
}

fn manifest_path() -> Option<PathBuf> {
    first_existing([
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ])
}

/// The live bank header (`0x20` VabHdr + 16 ProgAtr entries) out of a state, or
/// `None` if no VAB is open. Resolves the header through the libsnd pointer
/// rather than a fixed address.
fn live_bank_header(save_path: &Path) -> Option<Vec<u8>> {
    let save = SaveState::from_path(save_path).ok()?;
    let ram = save.main_ram().ok()?;
    let base = extract::read_u32_le(ram, VAB_HEADER_PTR_VA).ok()?;
    if !(0x8001_0000..0x8020_0000).contains(&base) {
        return None;
    }
    let hdr = ram_slice(ram, base, base + 0x20 + 16 * 0x10).ok()?;
    (&hdr[0..4] == b"\x70\x42\x41\x56").then(|| hdr.to_vec())
}

fn fsize_of(hdr: &[u8]) -> u32 {
    u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]])
}

fn catalogued_states() -> Option<Vec<PathBuf>> {
    let (mp, lib) = (manifest_path()?, library_dir()?);
    let manifest = ScenarioManifest::from_path(&mp).ok()?;
    Some(
        manifest
            .scenarios
            .iter()
            .filter_map(|scn| manifest.mednafen_save_path(scn, Some(lib.as_path())).ok())
            .filter(|p| p.exists())
            .collect(),
    )
}

#[test]
fn live_music01_bank_is_byte_identical_to_disc() {
    let (Some(states), Some(vab_path)) = (catalogued_states(), music01_prot()) else {
        eprintln!("[skip] save library or music_01 PROT entry not present");
        return;
    };
    let disc = std::fs::read(&vab_path).expect("read music_01 PROT entry");
    let disc = &disc[4..]; // past the chunk header

    let mut matched = 0usize;
    for path in &states {
        let Some(hdr) = live_bank_header(path) else {
            continue;
        };
        if fsize_of(&hdr) != MUSIC01_FSIZE {
            continue; // a different scene's bank
        }
        // VabHdr (first 0x20) is byte-identical.
        assert_eq!(&hdr[0..0x20], &disc[0..0x20], "VabHdr mismatch in {path:?}");
        // ProgAtr meaningful fields (bytes 0..8 of each 16-byte entry); bytes
        // 8..16 are the PsyQ reserved pointer field the runtime patches.
        for p in 0..16 {
            let o = 0x20 + p * 0x10;
            assert_eq!(
                &hdr[o..o + 8],
                &disc[o..o + 8],
                "program {p} attr mismatch in {path:?}"
            );
        }
        matched += 1;
    }
    assert!(
        matched > 0,
        "expected at least one catalogued state with the music_01 bank open"
    );
    eprintln!("[ok] live music_01 bank == disc PROT 1004 in {matched} state(s)");
}

#[test]
fn sfx_bank_varies_per_scene() {
    let Some(states) = catalogued_states() else {
        eprintln!("[skip] save library not present");
        return;
    };
    let mut sigs = std::collections::BTreeSet::new();
    let mut open = 0usize;
    for path in &states {
        if let Some(hdr) = live_bank_header(path) {
            open += 1;
            // (programs, fsize) uniquely fingerprints the bank for this purpose.
            let programs = u16::from_le_bytes([hdr[0x12], hdr[0x13]]);
            sigs.insert((programs, fsize_of(&hdr)));
        }
    }
    if open == 0 {
        eprintln!("[skip] no catalogued state had an open sound bank");
        return;
    }
    eprintln!(
        "[ok] {open} states with an open bank, {} distinct banks",
        sigs.len()
    );
    if open >= 3 {
        assert!(
            sigs.len() >= 2,
            "the SFX bank should track the scene, but saw 1 unique bank across {open} states"
        );
    }
}
