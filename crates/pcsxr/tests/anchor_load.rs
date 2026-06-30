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

/// A cataloged anchor + the facts its capture pinned (`scripts/scenarios.toml`).
struct AnchorExpect {
    label: &'static str,
    fingerprint: &'static str,
    scene: &'static str,
    mode: u8,
    pos: Option<(i16, i16)>,
}

const ANCHORS: &[AnchorExpect] = &[
    AnchorExpect {
        label: "s3_rimelm_freeroam",
        fingerprint: "2fba9adf4ade2f14de2a10c82e066b76025ac7ded1f063b852de9d498be00a6a",
        scene: "town01",
        mode: 0x03,
        pos: Some((4160, 11840)),
    },
    AnchorExpect {
        label: "s4_rimelm_door_transition",
        fingerprint: "a89f131f74811b56ef12146fcae0f49867f2a3307941a39c292bbd15831c890e",
        scene: "town01",
        mode: 0x03,
        pos: Some((3264, 3520)),
    },
    AnchorExpect {
        // battle: player parked at (0,0); only scene+mode asserted
        label: "s5_tetsu_battle",
        fingerprint: "4e9c1e5ffd5972c33da9bdf2304964979037cdfaf77a50df5b03a68c67a55e6f",
        scene: "town01",
        mode: 0x15,
        pos: None,
    },
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
    for a in ANCHORS {
        let Some(path) = library_save(a.fingerprint) else {
            eprintln!("[skip] {}: no library save on disk", a.label);
            continue;
        };
        let st = SaveState::from_path(&path).expect("load .sstate");
        eprintln!(
            "[{}] scene={:?} mode=0x{:02X} player_ptr={:?} pos={:?}",
            a.label,
            st.scene_name(),
            st.game_mode(),
            st.player_ptr().map(|p| format!("0x{p:08X}")),
            st.player_pos(),
        );
        assert_eq!(st.scene_name(), a.scene, "[{}] scene", a.label);
        assert_eq!(st.game_mode(), a.mode, "[{}] game_mode", a.label);
        if let Some(expected) = a.pos {
            assert_eq!(st.player_pos(), Some(expected), "[{}] player_pos", a.label);
        }
        checked += 1;
    }
    assert!(
        checked >= 1,
        "expected at least one anchor save present to validate the reader"
    );
}
