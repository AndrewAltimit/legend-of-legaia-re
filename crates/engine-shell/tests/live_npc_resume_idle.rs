//! A memory-card resume with the free-roam liveliness mode on
//! (`FieldNpcState::animate`, the `play-window` default) leaves the player
//! where the save put them, and opens no dialogue, when no button is pressed.
//!
//! Retail runs a placement's script only while its actor carries the engaged
//! bit (`+0x10 & 0x100`), which the touch post `FUN_801D5B5C` raises and the
//! runner `FUN_80039B7C` drops at the interaction's `0x21`; a spawned
//! placement starts disengaged (see `docs/subsystems/script-vm.md`
//! § Engagement and the system script). So an idle resume never runs a talk
//! body: nothing walks the player (`vell` `P1[3]`'s talk body spawns a record
//! that does) and no placement's lines open (`koin1`).
//!
//! Seeds from the retail library states `vell_fog_field` (mist forest) and
//! the two `koin1` ticket-counter states through the engine's own card-load path
//! ([`BootSession::resume_save`]), seats the player on retail's `(X, Z)`,
//! turns the liveliness on and ticks 120 frames with no input.
//!
//! Skips (passes) unless `LEGAIA_DISC_BIN` is set and the save library and
//! extracted disc are found (`LEGAIA_SAVES_LIBRARY` / `LEGAIA_EXTRACTED_DIR`).

use std::path::{Path, PathBuf};

use legaia_engine_shell::BootSession;
use legaia_engine_shell::boot::{BootConfig, FieldLiveOpts};
use legaia_parity::retail_compare::{enumerate_corpus, read_retail, resolve_dirs};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const LABELS: &[&str] = &[
    "vell_fog_field",
    "casino_ticket_counter",
    "casino_ticket_counter_dialog",
];
const TICKS: usize = 120;

#[test]
fn an_idle_resume_with_live_npcs_does_not_move_the_player() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (_, library, extracted) = resolve_dirs();
    let (Some(library), Some(extracted)) = (library, extracted) else {
        eprintln!("[skip] save library or extracted disc not found");
        return;
    };
    let manifest =
        legaia_mednafen::ScenarioManifest::from_path(repo().join("scripts/scenarios.toml"))
            .expect("scenario manifest");
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("SCUS");
    let entries = enumerate_corpus(&manifest, &library);
    let mut ran = 0;
    let mut failures: Vec<String> = Vec::new();
    for label in LABELS {
        let Some(entry) = entries.iter().find(|e| e.label == *label) else {
            eprintln!("[skip] {label}: no library backup");
            continue;
        };
        let retail = read_retail(entry, &scus).expect("read retail state");
        let (Some(save), Some([x, _, z])) = (retail.save.clone(), retail.player) else {
            panic!("{label}: state carries no save window / player");
        };
        let cfg = BootConfig {
            scene: retail.scene.clone(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("open session");
        let _ = session.resume_save(save, &retail.scene, &FieldLiveOpts::default());
        session.host.world.npcs.animate = true;
        assert!(
            session.host.world.debug_seat_player(x, z),
            "{label}: seat on retail ({x}, {z})"
        );
        session.camera.zone.arm_arrival();
        let pos = |s: &BootSession| {
            let w = &s.host.world;
            let a = &w.actors[w.player_actor_slot? as usize];
            Some((a.move_state.world_x, a.move_state.world_z))
        };
        let start = pos(&session);
        for t in 0..TICKS {
            session.tick().expect("tick");
            let now = pos(&session);
            let w = &session.host.world;
            if w.dialogue_owns_input() || w.cutscene_timeline_active() {
                failures.push(format!(
                    "{label} ({}): a script took the stage by tick {t} (dialogue {}, timeline {})",
                    retail.scene,
                    w.dialogue_owns_input(),
                    w.cutscene_timeline_active()
                ));
                break;
            }
            if now != start {
                failures.push(format!(
                    "{label} ({}): player moved {start:?} -> {now:?} by tick {t}",
                    retail.scene
                ));
                break;
            }
        }
        eprintln!(
            "[ran] {label} ({}): player at {:?}",
            retail.scene,
            pos(&session)
        );
        ran += 1;
    }
    assert!(failures.is_empty(), "{failures:#?}");
    if ran == 0 {
        eprintln!("[skip] no library state found");
    }
}
