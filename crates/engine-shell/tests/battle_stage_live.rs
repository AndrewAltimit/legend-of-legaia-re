//! Save-library oracle for the battle stage: which backdrop mesh a battle is
//! fought inside, and which stage *overlay* the battle scene loader pages in.
//!
//! Two independent pins, both read straight out of retail battle save states:
//!
//! 1. **Backdrop mesh.** The scene loader's type-`0x01` chunk walker
//!    `FUN_8001FE70` leaves the stage stream's base in `_DAT_8007B864`. Reading
//!    that pointer's TMD header gives object 0's live vertex pool, which
//!    byte-matches exactly one PROT entry once over-read tails are rejected.
//!    For the Tetsu tutorial battle that entry is **7**, not the `town01`
//!    block's first stage stream (6) - the defect
//!    `ProtIndex::battle_stage_entry_for_scene` exists to fix.
//!
//! 2. **Stage overlay.** `FUN_800520F0` reads the stage id at `_DAT_8007B64A`
//!    and, when non-zero, calls `FUN_8003EC70(stage_id + 0x47)`. The Tetsu
//!    battle is the catalogued library's only stage-id-`1` battle, and its
//!    loader-B current-id tracker `0x8007BC4C` reads `0x48` = extraction PROT
//!    967. Every other battle state reads stage id `0` (no stage overlay).
//!
//! Skips unless `LEGAIA_DISC_BIN`, `extracted/`, `scripts/scenarios.toml` and
//! `saves/library` are all present.
use std::path::{Path, PathBuf};

use legaia_engine_core::overlay_loader::battle_stage_overlay_entry;
use legaia_engine_core::scene::{ProtIndex, SceneHost};
use legaia_mednafen::{SaveState, ScenarioManifest};

const RAM_BASE: u32 = 0x8000_0000;
/// Stage-bundle base the type-`0x01` chunk walker records.
const DOME_PTR: u32 = 0x8007_B864;
/// Battle-stage id the battle scene loader feeds the `+0x47` band.
const STAGE_ID: u32 = 0x8007_B64A;
/// Overlay loader B's current-id tracker (`gp+0x934`).
const LOADER_B_ID: u32 = 0x8007_BC4C;

fn ru32(ram: &[u8], va: u32) -> u32 {
    let off = (va - RAM_BASE) as usize & 0x1F_FFFF;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}
fn rbytes(ram: &[u8], va: u32, n: usize) -> &[u8] {
    let off = (va - RAM_BASE) as usize & 0x1F_FFFF;
    &ram[off..off + n]
}

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists())
}

fn library() -> Option<(ScenarioManifest, PathBuf)> {
    let manifest = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())?;
    let lib = ["saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())?;
    Some((ScenarioManifest::from_path(&manifest).ok()?, lib))
}

fn state(manifest: &ScenarioManifest, lib: &Path, label: &str) -> Option<SaveState> {
    let sc = manifest.scenarios.iter().find(|s| s.label == label)?;
    let path = manifest.library_save_path(sc, lib)?;
    path.exists().then(|| SaveState::from_path(&path).ok())?
}

/// Object 0's live vertex pool at the stage-bundle base `_DAT_8007B864`.
fn resident_dome_pool(ram: &[u8]) -> Option<Vec<u8>> {
    let base = ru32(ram, DOME_PTR);
    if !(RAM_BASE..RAM_BASE + 0x20_0000).contains(&base) || ru32(ram, base) != 0x8000_0002 {
        return None;
    }
    let vp = ru32(ram, base + 0x0C);
    let nv = ru32(ram, base + 0x10) as usize;
    if !(RAM_BASE..RAM_BASE + 0x20_0000).contains(&vp) || nv == 0 || nv > 5000 {
        return None;
    }
    Some(rbytes(ram, vp, nv * 8).to_vec())
}

/// PROT entries whose **unique** on-disc content (not the over-read tail)
/// contains `pool`.
fn entries_containing(index: &ProtIndex, pool: &[u8], range: std::ops::Range<u32>) -> Vec<u32> {
    range
        .filter(|&idx| {
            let Ok(bytes) = index.entry_bytes(idx) else {
                return false;
            };
            let Some(pos) = bytes.windows(pool.len()).position(|w| w == pool) else {
                return false;
            };
            // Reject a hit that starts past this entry's own content: PROT
            // extraction over-reads into the following entries, so the
            // neighbour's dome shows up inside every predecessor's file.
            // Unique length = `(next_start_lba - start_lba) * 0x800`.
            index
                .entry_lba_count_retail(idx as u16)
                .is_some_and(|lbas| pos < lbas as usize * 0x800)
        })
        .collect()
}

#[test]
fn tetsu_battle_backdrop_is_prot_7() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scenarios.toml or saves/library missing");
        return;
    };
    let Some(save) = state(&manifest, &lib, "v0_1_battle_start_tetsu") else {
        eprintln!("[skip] v0_1_battle_start_tetsu missing");
        return;
    };
    let ram = save.main_ram().expect("main ram");
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let pool = resident_dome_pool(ram).expect("resident stage dome");
    // The Tetsu battle's dome is object 0 of a 2-object stream: 311 vertices.
    assert_eq!(pool.len(), 311 * 8, "resident dome object-0 vertex count");

    // Search the whole `town01` bundle: exactly one entry owns these bytes.
    let hits = entries_containing(&host.index, &pool, 1..10);
    eprintln!("Tetsu resident dome byte-matches town01 entries {hits:?}");
    assert_eq!(
        hits,
        vec![7],
        "the Tetsu backdrop is PROT 7 and nothing else in the bundle"
    );

    // ...and that is what the engine now picks.
    assert_eq!(host.index.battle_stage_entry_for_scene("town01"), Some(7));
    // The block's first stage stream - the pre-fix choice - is a different
    // sub-area's backdrop.
    assert_ne!(
        host.index.battle_stage_entries("town01").first().copied(),
        Some(7),
        "regression guard: PROT 7 is deliberately not the block's first stream"
    );
}

#[test]
fn overworld_battle_backdrop_is_prot_88() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scenarios.toml or saves/library missing");
        return;
    };
    let Some(save) = state(&manifest, &lib, "overworld_battle_bg_angle_a") else {
        eprintln!("[skip] overworld_battle_bg_angle_a missing");
        return;
    };
    let ram = save.main_ram().expect("main ram");
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let pool = resident_dome_pool(ram).expect("resident stage dome");
    let hits = entries_containing(&host.index, &pool, 83..95);
    eprintln!("overworld resident dome byte-matches map01 entries {hits:?}");
    assert!(
        hits.contains(&88),
        "the overworld backdrop is PROT 88, got {hits:?}"
    );
    assert_eq!(host.index.battle_stage_entry_for_scene("map01"), Some(88));
}

#[test]
fn tetsu_is_the_librarys_only_stage_overlay_battle() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scenarios.toml or saves/library missing");
        return;
    };
    let mut with_overlay = Vec::new();
    let mut without = 0usize;
    for sc in manifest
        .scenarios
        .iter()
        .filter(|s| s.phase.as_deref() == Some("battle"))
    {
        let Some(path) = manifest.library_save_path(sc, lib.as_path()) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let Ok(save) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = save.main_ram() else { continue };
        let stage_id = rbytes(ram, STAGE_ID, 1)[0];
        match battle_stage_overlay_entry(stage_id) {
            None => without += 1,
            Some(entry) => {
                with_overlay.push((sc.label.clone(), stage_id, entry, ru32(ram, LOADER_B_ID)))
            }
        }
    }
    if with_overlay.is_empty() && without == 0 {
        eprintln!("[skip] no battle states resolved");
        return;
    }
    eprintln!("stage-overlay battles: {with_overlay:?} (plus {without} with none)");
    assert!(
        without > 0,
        "control: the overwhelming majority of battles load no stage overlay"
    );
    // Every stage-overlay battle in the library is a Tetsu tutorial anchor.
    let tetsu = [
        "v0_1_battle_loading_tetsu",
        "v0_1_battle_start_tetsu",
        "v0_1_battle_command_menu",
        "v0_1_battle_command_submenu",
    ];
    for (label, stage_id, entry, loader_b) in &with_overlay {
        assert!(
            tetsu.contains(&label.as_str()),
            "{label}: only the Tetsu tutorial battle pages in a stage overlay"
        );
        assert_eq!(*stage_id, 1, "{label}: stage id");
        assert_eq!(*entry, 967, "{label}: stage overlay entry");
        // Loader B's tracker holds the raw parameter `stage_id + 0x47 = 0x48`
        // once the load has issued. The `loading` anchor is captured *before*
        // that point in the loader SM, so it still shows the field-era value.
        if *label != "v0_1_battle_loading_tetsu" {
            assert_eq!(*loader_b, 0x48, "{label}: loader-B tracker");
        }
    }
    assert!(
        !with_overlay.is_empty(),
        "the Tetsu anchors must be present for this to be non-vacuous"
    );
}
