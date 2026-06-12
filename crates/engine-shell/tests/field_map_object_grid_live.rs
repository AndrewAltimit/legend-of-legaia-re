//! Disc + save-library gated: the field `.MAP` resolver's **object-index
//! grid** (`+0x8000`, the source of `Scene::field_object_placements` /
//! `field_terrain_tiles`) matches the live field buffer of real retail
//! sessions - the same live-RAM validation already pinning the collision
//! grid (`+0x4000`).
//!
//! For each catalogued capture of a field scene, the live field buffer
//! (scratchpad `_DAT_1f8003ec`) `+0x8000..+0x10000` region is diffed against
//! the entry the `define - 2` resolver picks. The residual must be small
//! (story-conditional cell mutations: opened chests, prescript object
//! toggles), and for the scenes whose first in-block `FIELD_MAP_LEN` entry
//! holds *different* content (keikoku, koin3 - the Rim Elm family's maps are
//! byte-identical, so they can't discriminate), the in-block candidate must
//! diff by orders of magnitude more - re-falsifying the old in-block rule
//! against live RAM on the placement region.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` /
//! `scripts/scenarios.toml` / `saves/library` (CI runs without Sony bytes).

use std::path::PathBuf;

use legaia_engine_core::scene::{FIELD_MAP_LEN, ProtIndex, Scene};
use legaia_mednafen::{SaveState, ScenarioManifest};

const OBJECT_GRID: std::ops::Range<usize> = 0x8000..0x10000;

/// Catalogued field-session captures (scenario label, active scene). Battle /
/// FMV / foreign-scene states repurpose the field buffer, so the sweep is an
/// explicit allowlist of saves whose field buffer holds the named scene.
const CAPTURES: &[(&str, &str)] = &[
    ("v0_1_pre_battle_tetsu", "town01"),
    ("rimelm_wall_press_left", "town0c"),
    ("keikoku_chest_pre", "keikoku"),
    ("minigame_dance_noa", "koin3"),
];

/// Scenes where the first in-block `FIELD_MAP_LEN` entry carries different
/// content than the `define - 2` entry, so live RAM can discriminate the two
/// rules. (The Rim Elm `town01`/`town0b`/`town0c` maps are byte-identical.)
const DISCRIMINATING: &[&str] = &["keikoku", "koin3"];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
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
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

#[test]
fn field_map_object_grid_matches_live_sessions() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let (Some(manifest_path), Some(library)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");

    let prot = std::fs::read(extracted.join("PROT.DAT")).expect("read PROT.DAT");
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("read CDNAME.TXT");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("build ProtIndex");

    let mut checked = 0;
    for &(label, scene_name) in CAPTURES {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        let state = SaveState::from_path(&save_path).expect("parse save state");
        let ram = state.main_ram().expect("main RAM");
        let scratch = state.scratch_ram().expect("scratch RAM");
        let fb = u32::from_le_bytes(scratch[0x3EC..0x3F0].try_into().unwrap());
        assert_eq!(
            fb & 0xFF00_0000,
            0x8000_0000,
            "{label}: field buffer pointer is a KSEG0 RAM address"
        );
        let base = (fb & 0x1F_FFFF) as usize;
        let live = &ram[base + OBJECT_GRID.start..base + OBJECT_GRID.end];

        let scene = Scene::load(&index, scene_name).expect("load scene");
        let map_idx = scene
            .field_map_index(&index)
            .expect("scene resolves a field .MAP");
        let disc = index
            .entry_bytes_extended(map_idx)
            .expect("read .MAP extended footprint");
        let resolved_diff = live
            .iter()
            .zip(&disc[OBJECT_GRID.start..OBJECT_GRID.end])
            .filter(|(a, b)| a != b)
            .count();
        // Story-conditional cell mutations (opened chests, prescript object
        // toggles) leave a small residual; the census measures 0..96 bytes
        // across the library. A wrong map diffs by thousands.
        assert!(
            resolved_diff <= 0x100,
            "{label} ({scene_name}): live object grid diffs {resolved_diff} bytes \
             vs the resolved .MAP entry {map_idx} - wrong map?"
        );

        if DISCRIMINATING.contains(&scene_name) {
            // The old rule: first FIELD_MAP_LEN entry INSIDE the block.
            let in_block = (scene.start..scene.end)
                .find(|&i| {
                    index
                        .entries()
                        .get(i as usize)
                        .is_some_and(|e| e.size_bytes as usize == FIELD_MAP_LEN)
                })
                .expect("block carries an in-block FIELD_MAP_LEN entry");
            assert_ne!(in_block, map_idx);
            let decoy = index
                .entry_bytes_extended(in_block)
                .expect("read in-block candidate");
            let decoy_diff = live
                .iter()
                .zip(&decoy[OBJECT_GRID.start..OBJECT_GRID.end])
                .filter(|(a, b)| a != b)
                .count();
            assert!(
                decoy_diff > resolved_diff.max(1) * 10,
                "{label} ({scene_name}): the in-block candidate (entry {in_block}) \
                 should diff far more ({decoy_diff}) than the resolved entry \
                 ({resolved_diff})"
            );
        }
        eprintln!("[ok] {label} ({scene_name}): object grid residual {resolved_diff} bytes");
        checked += 1;
    }
    if checked == 0 {
        eprintln!("[skip] no catalogued captures present");
    }
}
