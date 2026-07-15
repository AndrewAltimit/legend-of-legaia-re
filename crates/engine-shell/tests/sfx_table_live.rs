//! Savestate cross-check for the static SFX descriptor table (`DAT_8006F198`).
//!
//! `legaia_asset::sfx_table` parses the 100-entry table out of `SCUS_942.54`.
//! This test proves the parse against **ground-truth live RAM**: it reads the
//! same VA window out of a catalogued mednafen save state's main RAM and parses
//! it with the identical decoder, then drives the disc-decoded descriptors into
//! `legaia_engine_audio::SfxBank` - the data path the engine actually uses to
//! fire cues through the SPU.
//!
//! Because the table is static rodata it is identical in every state, so any
//! catalogued backup suffices; we just take the first one on disk. Skips and
//! passes when neither `scripts/scenarios.toml` + a library backup is present
//! (CI has no Sony bytes).

use legaia_asset::sfx_table::{SFX_TABLE_ENTRIES, SFX_TABLE_VA, SfxTable};
use legaia_engine_audio::SfxBank;
use legaia_mednafen::{SaveState, ScenarioManifest, ram_slice};
use std::path::PathBuf;

fn first_existing<const N: usize>(cands: [&str; N]) -> Option<PathBuf> {
    cands.into_iter().map(PathBuf::from).find(|p| p.exists())
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

#[test]
fn live_ram_table_matches_the_parser_and_feeds_the_sfx_bank() {
    let (Some(manifest_path), Some(lib)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scenarios.toml or saves/library not present");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");

    // First catalogued mednafen backup on disk - the table is static, so any
    // state holds it.
    let save_path = manifest.scenarios.iter().find_map(|scn| {
        manifest
            .library_save_path(scn, lib.as_path())
            .filter(|p| p.exists())
    });
    let Some(save_path) = save_path else {
        eprintln!("[skip] no catalogued mednafen backup on disk");
        return;
    };

    let save = SaveState::from_path(&save_path).expect("load save state");
    let ram = save.main_ram().expect("main RAM section");
    let table_bytes = ram_slice(
        ram,
        SFX_TABLE_VA,
        SFX_TABLE_VA + (SFX_TABLE_ENTRIES * 8) as u32,
    )
    .expect("slice SFX table window");

    let table = SfxTable::from_table_bytes(table_bytes);
    assert_eq!(table.len(), SFX_TABLE_ENTRIES, "100 live descriptors");

    // Same structural invariants as the disc parse: every entry active, voice
    // counts in range, trailing bytes zero.
    assert_eq!(table.active().count(), SFX_TABLE_ENTRIES, "all active");
    for (id, d) in table.active() {
        assert!(
            (1..=3).contains(&d.voice_count()),
            "id {id:#x} voice count {}",
            d.voice_count()
        );
        assert_eq!(d.reserved, [0, 0, 0], "id {id:#x} reserved zero");
    }

    // The two cue ids the engine's default SfxBank references, pinned from RAM.
    let e1a = table.get(0x1A).expect("0x1A present");
    assert_eq!((e1a.program, e1a.note), (3, 67));
    let e4c = table.get(0x4C).expect("0x4C present");
    assert_eq!((e4c.program, e4c.tone), (3, 8));

    // Drive the descriptors into the engine bank (program / tone / note /
    // voice-count) and confirm the cues resolve with the full descriptor.
    let bank = SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );
    assert_eq!(bank.len(), SFX_TABLE_ENTRIES);
    let cue = bank.get(0x1A).expect("0x1A in bank");
    // The strike cue: program 3, tone 0 (named by index), note 67, 1 voice.
    assert_eq!(
        (cue.program_index, cue.tone, cue.key, cue.voices),
        (3, 0, 67, 1)
    );

    // If the disc SCUS is also extracted, the live table must equal the disc
    // parse byte-for-byte (proves static residency + parser offset end to end).
    if let Some(scus) = first_existing(["extracted/SCUS_942.54", "../../extracted/SCUS_942.54"]) {
        let bytes = std::fs::read(scus).expect("read SCUS");
        let disc = SfxTable::from_scus(&bytes).expect("parse SCUS table");
        assert_eq!(disc, table, "disc parse equals live RAM table");
    }

    eprintln!(
        "[ok] {} live SFX descriptors validated from {}",
        table.len(),
        save_path.display()
    );
}
