//! Library-gated: the retail Super-Art applier's live queue tail-replace,
//! re-checked from cataloged post-applier PCSX-Redux states.
//!
//! Each state (`scripts/scenarios.toml`, probe
//! `scripts/pcsx-redux/autorun_super_art_queue_inject.lua`, runbook
//! `docs/tooling/super-art-queue-capture.md`) is the
//! `party_basic_attack_vs_gobu_gobu` battle one vsync after the battle
//! overlay's Super applier `FUN_801EF9E4` ran over a queue holding a Super's
//! exact `find` bytes: the acting slot-0 actor's action queue at
//! `actor[+0x1DF..+0x1EE]` therefore holds that Super's byte-exact `replace`
//! string, written by the retail tail-replace loop (`sb` at `0x801EFB7C`).
//! This test re-derives the check: resolve the slot-0 battle actor from the
//! pointer table `0x801C9370`, read the 16-byte queue, and compare against
//! `legaia_art::SUPER_ARTS`'s modeled `replace` (zero fill after).
//!
//! One state per character is cataloged (each a Super with no prior
//! end-to-end execution): Vahn's Rolling Combo (the two-part `2F 30`
//! finisher), Noa's Triple Lizard and Gala's Back Punch x3 (the `x3`
//! finishers). The full 15/15 sweep ran through the same probe; its CSV
//! lives with the capture run (Sony-derived RAM bytes, not committed).
//!
//! Skips when the SCUS anchor source or the library backups are absent.

use std::path::{Path, PathBuf};

use legaia_art::SUPER_ARTS;
use legaia_pcsxr::SaveState;

/// Battle-actor pointer table (8 x u32; slots 0..2 = party).
const ACTOR_TABLE_VA: u32 = 0x801C_9370;
/// Per-actor action-parameter byte stream head.
const QUEUE_OFF: u32 = 0x1DF;
/// The queue proper: `FUN_801DA34C` preseeds exactly 16 bytes and the
/// applier's zero-scan caps at `0x10`.
const QUEUE_LEN: usize = 0x10;

/// `(scenario label, library fingerprint, SUPER_ARTS entry name)`.
const STATES: &[(&str, &str, &str)] = &[
    (
        "super_queue_replace_vahn_rolling_combo",
        "bfe53a9fafdfde84374050f72618bec765cc6c7dc0b49b5537e0d84e2cda3db8",
        "Rolling Combo",
    ),
    (
        "super_queue_replace_noa_triple_lizard",
        "cc470e28b1a44d60cfafcf85b7f77b8e8afa5b4117ada34481a26eb601f2f1d0",
        "Triple Lizard",
    ),
    (
        "super_queue_replace_gala_back_punch",
        "01c8b1f893a4c8a38016852afad6e7281fc146436d5fe5ee1bdadbba9f9dbce6",
        "Back Punch x3",
    ),
];

/// Find `extracted/SCUS_942.54` and point `LEGAIA_SCUS` at it (the pcsxr
/// main-RAM anchor search reads it). Returns false if not present.
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

#[test]
fn super_replace_strings_are_live_queue_resident() {
    if !ensure_scus() {
        eprintln!("[skip] no extracted/SCUS_942.54 (anchor source) - skipping");
        return;
    }

    let mut checked = 0usize;
    for &(label, fp, super_name) in STATES {
        let Some(path) = library_save(fp) else {
            eprintln!("[skip] {label}: library backup {fp} absent");
            continue;
        };
        let entry = SUPER_ARTS
            .iter()
            .find(|s| s.name == super_name)
            .expect("cataloged Super exists in SUPER_ARTS");

        let save = SaveState::from_path(&path).expect("parse .sstate");
        assert_eq!(save.game_mode(), 0x15, "{label}: not BattleMode");

        let actor = save.u32_at(ACTOR_TABLE_VA);
        assert!(
            (0x8000_0000..0x8020_0000).contains(&actor),
            "{label}: slot-0 actor pointer 0x{actor:08X} not in RAM"
        );

        let queue: Vec<u8> = (0..QUEUE_LEN as u32)
            .map(|i| save.u8_at(actor + QUEUE_OFF + i))
            .collect();

        // The queue proper must be exactly `replace` ++ zero fill: the
        // retail applier overwrote the injected `find` tail in place.
        let mut want = entry.replace.to_vec();
        want.resize(QUEUE_LEN, 0);
        assert_eq!(
            queue, want,
            "{label}: queue at actor[+0x1DF] != {super_name} replace string"
        );

        eprintln!(
            "{label}: actor 0x{actor:08X} queue == {super_name} replace ({} bytes + fill)",
            entry.replace.len()
        );
        checked += 1;
    }

    if checked == 0 {
        eprintln!("[skip] no super-queue library states available");
    }
}
