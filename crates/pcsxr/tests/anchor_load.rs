//! Disc-gated: the PCSX-Redux `.sstate` reader loads the cataloged playthrough
//! anchors and reads back the facts those captures pinned (scene, game_mode,
//! player position). This is the bridge's own oracle - it proves the anchor
//! search locates main RAM in a PCSX-Redux state and the field offsets are
//! right. Skips when the SCUS binary or the library saves are absent.

use std::path::{Path, PathBuf};

use legaia_pcsxr::SaveState;

/// Find `extracted/SCUS_942.54` and point `LEGAIA_SCUS` at it (the anchor search
/// reads it). Returns false if not present.
fn ensure_scus() -> bool {
    if std::env::var_os("LEGAIA_SCUS").is_some() {
        return true;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let p = PathBuf::from(c).join("SCUS_942.54");
        if p.exists() {
            // SAFETY: single-threaded test setup before any SaveState load.
            unsafe { std::env::set_var("LEGAIA_SCUS", &p) };
            return true;
        }
    }
    false
}

fn library_save(fp: &str) -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let p = Path::new(c).join("pcsx-redux").join(format!("{fp}.sstate"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// `(label, fingerprint, expected_scene, expected_mode, expected_pos)`. The
/// fingerprints + expectations are the cataloged facts in `scripts/scenarios.toml`.
const ANCHORS: &[(&str, &str, &str, u8, Option<(i16, i16)>)] = &[
    (
        "s3_rimelm_freeroam",
        "2fba9adf4ade2f14de2a10c82e066b76025ac7ded1f063b852de9d498be00a6a",
        "town01",
        0x03,
        Some((4160, 11840)),
    ),
    (
        "s4_rimelm_door_transition",
        "a89f131f74811b56ef12146fcae0f49867f2a3307941a39c292bbd15831c890e",
        "town01",
        0x03,
        Some((3264, 3520)),
    ),
    (
        "s5_tetsu_battle",
        "4e9c1e5ffd5972c33da9bdf2304964979037cdfaf77a50df5b03a68c67a55e6f",
        "town01",
        0x15,
        None, // battle: player parked at (0,0); only scene+mode asserted
    ),
];

#[test]
fn reads_back_cataloged_anchor_facts() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    if !ensure_scus() {
        eprintln!("[skip] extracted/SCUS_942.54 not found (set LEGAIA_SCUS)");
        return;
    }

    let mut checked = 0;
    for (label, fp, scene, mode, pos) in ANCHORS {
        let Some(path) = library_save(fp) else {
            eprintln!("[skip] {label}: no library save on disk");
            continue;
        };
        let st = SaveState::from_path(&path).expect("load .sstate");
        eprintln!(
            "[{label}] scene={:?} mode=0x{:02X} player_ptr={:?} pos={:?}",
            st.scene_name(),
            st.game_mode(),
            st.player_ptr().map(|p| format!("0x{p:08X}")),
            st.player_pos(),
        );
        assert_eq!(st.scene_name(), *scene, "[{label}] scene");
        assert_eq!(st.game_mode(), *mode, "[{label}] game_mode");
        if let Some(expected) = pos {
            assert_eq!(st.player_pos(), Some(*expected), "[{label}] player_pos");
        }
        checked += 1;
    }
    assert!(
        checked >= 1,
        "expected at least one anchor save present to validate the reader"
    );
}
