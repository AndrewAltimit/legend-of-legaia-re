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
//! The lone `0x4F` is not an inline script literal: it is town01 **MAN
//! formation index 4**, selected by the scripted carrier. The in-RAM 8-byte
//! formation table is byte-identical to the engine's MAN parse
//! (`RIM_ELM_TRAINING_FORMATION_ID`); the engine reaches the fight via
//! `World::install_man_formation(4)`. See `docs/formats/encounter.md` →
//! "Worked example: the Rim Elm training fight".
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

/// PSX address of the player-context pointer (`_DAT_8007C364`); its u32 value is
/// the player actor struct's base address. See `docs/subsystems/field-locomotion.md`.
const PLAYER_CTX_PTR: u32 = 0x8007_C364;
/// Player actor flag-word offset (`actor[+0x10]`).
const PLAYER_FLAGS_OFF: u32 = 0x10;
/// `actor[+0x10]` bit: free movement is disabled (an interaction / dialogue /
/// encounter / cutscene holds the player). Clear during free locomotion.
const FLAG_MOVE_DISABLED: u32 = 0x0008_0000;

/// Read a little-endian u32 from a PSX main-RAM address.
fn read_u32(ram: &[u8], addr: u32) -> u32 {
    let s = ram_slice(ram, addr, addr + 4)
        .unwrap_or_else(|e| panic!("u32 slice @ {addr:#010x}: {e:#}"));
    u32::from_le_bytes([s[0], s[1], s[2], s[3]])
}

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

/// Resolve a scenario's mednafen library backup and read the player actor's
/// `+0x10` flag word (via the `_DAT_8007C364` context pointer). `None` (skip)
/// when the scenario, its backup_fingerprint, or the backup file is missing.
fn player_flags(manifest: &ScenarioManifest, lib: &Path, label: &str) -> Option<u32> {
    let scn = manifest.scenarios.iter().find(|s| s.label == label)?;
    let fp = scn.backup_fingerprint.as_deref()?;
    let path = scenarios::library_backup_for("mednafen", lib, fp)?;
    let save = SaveState::from_path(&path)
        .unwrap_or_else(|e| panic!("parse save {label} ({}): {e:#}", path.display()));
    let ram = save
        .main_ram()
        .unwrap_or_else(|e| panic!("main RAM for {label}: {e:#}"));
    let player = read_u32(ram, PLAYER_CTX_PTR);
    Some(read_u32(ram, player + PLAYER_FLAGS_OFF))
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

/// The pre-fight dialogue-accept frame (`v0_1_tetsu_dialogue_accept`) is a
/// field-mode actor-interaction frame, distinct from the free-roam pre-battle
/// field in two retail-observable ways:
///
///   1. **No formation is installed yet** — the global formation cell is clear,
///      exactly like the free-roam frame. The lone-Tetsu formation is written at
///      the engage -> battle-load transition, not while the prompt is up. This
///      matches the engine's carrier SM, which installs the formation when the
///      carrier fires (`World::begin_field_carrier_battle`), not on interaction.
///   2. **Free movement is locked** — the player actor's flag word carries the
///      `0x80000` movement-disabled bit, which is clear in the free-roam frame.
///
/// Together these pin the precondition the deferred field-VM dialogue-accept
/// auto-arm must detect: a movement-locked actor interaction with the formation
/// not yet installed. (The `0x1000000` "action requested" bit and the `+0x98`
/// interaction-target pointer are both non-distinguishing here — set / non-null
/// in the free-roam frame too — so the load-bearing signal is `0x80000`.)
#[test]
fn dialogue_accept_frame_is_movement_locked_pre_install() {
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(lib) = library_dir() else {
        eprintln!("[skip] saves/library not present (gitignored save corpus)");
        return;
    };

    let Some(cell) = formation_cell(&manifest, &lib, "v0_1_tetsu_dialogue_accept") else {
        eprintln!("[skip] dialogue-accept capture not in saves/library");
        return;
    };
    assert_eq!(
        cell,
        [0, 0, 0, 0],
        "dialogue-accept: formation not installed until engage (cell clear)"
    );

    let accept = player_flags(&manifest, &lib, "v0_1_tetsu_dialogue_accept")
        .expect("dialogue-accept player flags");
    assert_ne!(
        accept & FLAG_MOVE_DISABLED,
        0,
        "dialogue-accept: free movement is locked during the interaction (flags {accept:#010x})"
    );

    // Differential: the free-roam pre-battle field has movement enabled. (It
    // resolves from its immutable library backup.)
    if let Some(free) = player_flags(&manifest, &lib, "v0_1_pre_battle_tetsu") {
        assert_eq!(
            free & FLAG_MOVE_DISABLED,
            0,
            "free-roam pre-battle field has movement enabled (flags {free:#010x})"
        );
    }
}
