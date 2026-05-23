//! Verify the opening Rim Elm training-fight formation against the retail
//! save-state corpus.
//!
//! The global formation cell at `0x8007BD0C..0x8007BD0F` holds up to four
//! monster ids (one per battle slot). Across the training-fight captures it
//! reads empty in the pre-battle field and a lone monster id `0x4F` ("Tetsu")
//! from battle-load onward — see `docs/formats/encounter.md`. This pins that
//! observation so a regression in the formation-cell address or the corpus
//! surfaces here.
//!
//! Library-gated, not disc-gated: the capture states live as immutable,
//! content-hashed backups under `saves/library/mednafen/` (gitignored Sony
//! RAM) and resolve via each scenario's `backup_fingerprint`. The test
//! skip-passes when the manifest or the backups are absent, so CI stays green
//! without the save corpus.

use std::path::{Path, PathBuf};

use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice, scenarios};

/// PSX address of the active-formation cell (`u8[4]`, one monster id per slot).
const FORMATION_CELL: u32 = 0x8007_BD0C;

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Resolve a scenario's mednafen library backup and read its 4-byte formation
/// cell. Returns `None` (skip) when the scenario, its backup_fingerprint, or
/// the backup file is missing.
fn formation_cell(manifest: &ScenarioManifest, lib: &Path, label: &str) -> Option<[u8; 4]> {
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let fp = scn.backup_fingerprint.as_deref()?;
    let path = scenarios::library_backup_for("mednafen", lib, fp)?;
    let save = SaveState::from_path(&path)
        .unwrap_or_else(|e| panic!("parse training save {label} ({}): {e:#}", path.display()));
    let ram = save
        .main_ram()
        .unwrap_or_else(|e| panic!("main RAM for {label}: {e:#}"));
    let cell = ram_slice(ram, FORMATION_CELL, FORMATION_CELL + 4)
        .unwrap_or_else(|e| panic!("formation cell slice for {label}: {e:#}"));
    Some([cell[0], cell[1], cell[2], cell[3]])
}

#[test]
fn training_fight_formation_cell_matches_corpus() {
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(lib) = library_dir() else {
        eprintln!("[skip] saves/library not present (gitignored save corpus)");
        return;
    };

    // The battle-load anchor is the linchpin: the formation is installed by
    // then. If it isn't backed up locally, there's nothing to assert.
    let Some(loading) = formation_cell(&manifest, &lib, "v0_1_battle_loading_tetsu") else {
        eprintln!("[skip] training battle-loading capture not in saves/library");
        return;
    };
    assert_eq!(
        loading,
        [0x4F, 0, 0, 0],
        "battle-load: lone training monster (id 0x4F) in slot 0"
    );

    // Pre-battle field: the cell is clear (no formation installed yet).
    if let Some(pre) = formation_cell(&manifest, &lib, "v0_1_pre_battle_tetsu") {
        assert_eq!(
            pre,
            [0, 0, 0, 0],
            "pre-battle field: no formation installed (cell clear)"
        );
    }

    // Through the rest of the fight (and into the post-battle field, where the
    // cell is retained until the next install) the lone-monster formation
    // stands.
    for label in [
        "v0_1_battle_start_tetsu",
        "v0_1_battle_command_menu",
        "v0_1_battle_command_submenu",
        "v0_1_post_battle_tetsu_town",
    ] {
        if let Some(cell) = formation_cell(&manifest, &lib, label) {
            assert_eq!(
                cell,
                [0x4F, 0, 0, 0],
                "{label}: lone training-monster formation"
            );
        }
    }
}
