//! Where the replay reads its game from: the extracted tree, the scene host
//! boot, and the committed baseline.

use super::*;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

pub(super) fn extracted_dir() -> Option<PathBuf> {
    let d = repo_root().join("extracted");
    if d.is_dir() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

pub(super) fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // A cold boot has a party: seed the retail New Game roster (Vahn, the
    // SCUS `0x80078C4C` template) exactly as `NativeSession::begin_new_game`
    // does. Without it every party slot is hollow (`max_hp == 0`, no
    // record) - the port-only unseeded state whose encounters can neither be
    // fought nor survived, which made rung 3's "one encounter fought and
    // survived" unmeasurable (and, before the wipe hold existed, silently
    // scored a total party kill as `Fight::Survived`).
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS_942.54");
    let party = legaia_game_tables::new_game::StartingParty::from_scus(&scus)
        .expect("SCUS new-game template");
    host.world.seed_starting_party(&party);
    Some(host)
}

pub(super) fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/critical_path_baseline.toml")
}

/// Parse the `reached = N` line out of the baseline file. A missing file
/// reads as `0`, so a fresh clone starts from "no progress claimed" rather
/// than failing on a file it cannot see.
pub(super) fn read_baseline() -> usize {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return 0;
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            l.strip_prefix("reached")?
                .trim()
                .strip_prefix('=')?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
