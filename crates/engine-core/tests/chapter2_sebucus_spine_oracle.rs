//! Disc-gated runtime oracle: the chapter-2 (Sebucus) progression gate spine
//! driven through the engine's own C1/C2 record-gate machinery.
//!
//! The gate spine was mined from the chapter-2 state-poll capture and pinned
//! statically in `man_variant_carrier_census_disc::chapter2_sebucus_gate_spine`
//! (the exact C1/C2 lists). This oracle proves the ENGINE sequences that chain:
//! it runs the pinned disc gates through the runtime evaluator
//! [`World::p2_record_gates_pass`] and the real seeder entry point
//! [`World::install_gated_p2_record`] against the actual scene MANs.
//!
//! No chapter-specific engine code is needed - unlike chapter 1's Caruban beat
//! (whose `0x142` latch is a post-BATTLE record the engine can't execute, so it
//! needs the `SCRIPTED_SCENE_BOSSES` victory stand-in), every chapter-2 beat
//! here is a field-mode CUTSCENE record whose script `SysFlag.Set` latches its
//! flag organically through [`World::system_flag_set`] (the field-VM host path,
//! already proven organic by the town01 `549` walk-on oracle). So the generic
//! seeder drives the whole arc for free; this oracle is the proof.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use legaia_asset::man_section::{ManFile, parse as parse_man};
use legaia_engine_core::man_field_scripts::partition2_record_gates;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;
use std::path::PathBuf;
use std::sync::Arc;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open ProtIndex"),
    ))
}

fn load_man(index: &ProtIndex, scene_name: &str) -> (ManFile, Vec<u8>) {
    let scene =
        Scene::load(index, scene_name).unwrap_or_else(|e| panic!("load {scene_name}: {e:#}"));
    let man = scene
        .field_man_payload(index)
        .expect("payload")
        .unwrap_or_else(|| panic!("{scene_name} MAN resolves"));
    let mf = parse_man(&man).expect("parse MAN");
    (mf, man)
}

fn gates(mf: &ManFile, man: &[u8], rec: usize) -> (Vec<u16>, Vec<u16>) {
    partition2_record_gates(mf, man, rec).expect("record gates")
}

/// The full Sebucus arc sequenced through the runtime gate evaluator: at each
/// progression step exactly the records that retail would spawn pass their
/// C1/C2 gates, and the played beats self-latch shut.
#[test]
fn chapter2_sebucus_arc_sequences_through_the_engine() {
    let Some(index) = open_index() else { return };
    let (teien_mf, teien) = load_man(&index, "teien");
    let (tower_mf, tower) = load_man(&index, "tower");
    let (geremi_mf, geremi) = load_man(&index, "geremi");
    let (map02_mf, map02) = load_man(&index, "map02");

    let teien1 = gates(&teien_mf, &teien, 1);
    let teien2 = gates(&teien_mf, &teien, 2);
    let teien5 = gates(&teien_mf, &teien, 5);
    let tower2 = gates(&tower_mf, &tower, 2);
    let geremi1 = gates(&geremi_mf, &geremi, 1);
    let map02_9 = gates(&map02_mf, &map02, 9);

    let pass = |w: &World, g: &(Vec<u16>, Vec<u16>)| w.p2_record_gates_pass(&g.0, &g.1);

    let mut w = World::new();

    // Step 0 - fresh arc: only the first teien beat is available.
    assert!(pass(&w, &teien1), "teien P2[1] available at arc start");
    assert!(!pass(&w, &teien2), "teien P2[2] blocked (C2 needs 0x1C8)");
    assert!(!pass(&w, &teien5), "teien P2[5] blocked (C2 needs 0x1C9)");
    assert!(
        !pass(&w, &tower2),
        "tower blocked until the teien arc (0x1C9)"
    );
    assert!(
        !pass(&w, &geremi1),
        "geremi beat blocked until tower done (0x1C7)"
    );
    assert!(
        !pass(&w, &map02_9),
        "overworld mirror blocked (C2 needs 0x1C9)"
    );

    // Step 1 - teien P2[1] executes: sets 0x1C8.
    w.system_flag_set(0x1C8);
    assert!(
        pass(&w, &teien1),
        "P2[1] still open (its C1 is 0x1C9, unset)"
    );
    assert!(pass(&w, &teien2), "P2[2] unlocked once 0x1C8 set");

    // Step 2 - teien P2[2] executes: sets 0x1C9 (the teien-arc-reached flag).
    w.system_flag_set(0x1C9);
    assert!(!pass(&w, &teien1), "P2[1] self-latches shut (C1 0x1C9)");
    assert!(!pass(&w, &teien2), "P2[2] self-latches shut (C1 0x1C9)");
    assert!(pass(&w, &teien5), "P2[5] unlocked (C2 0x1C9)");
    assert!(pass(&w, &tower2), "tower unlocked (C2 0x1C9)");
    assert!(pass(&w, &map02_9), "overworld mirror unlocked (C2 0x1C9)");

    // Step 3 - teien P2[5] executes: sets 0x332.
    w.system_flag_set(0x332);
    assert!(!pass(&w, &teien5), "P2[5] self-latches shut (C1 0x332)");
    assert!(
        !pass(&w, &map02_9),
        "overworld mirror latches shut (C1 0x332)"
    );

    // Step 4 - tower P2[2] executes: sets tower-clear 0x1C7.
    w.system_flag_set(0x1C7);
    assert!(
        !pass(&w, &tower2),
        "tower self-latches shut once cleared (C1 0x1C7)"
    );
    assert!(
        pass(&w, &geremi1),
        "geremi beat unlocked (C2 requires the tower)"
    );
}

/// The balden self-latch pair, independent of the teien/tower chain.
#[test]
fn chapter2_balden_self_latch_sequences() {
    let Some(index) = open_index() else { return };
    let (mf, man) = load_man(&index, "balden");
    let b19 = gates(&mf, &man, 19);
    let b18 = gates(&mf, &man, 18);

    let mut w = World::new();
    assert!(
        w.p2_record_gates_pass(&b19.0, &b19.1),
        "balden P2[19] available (C1 0x5B3 unset)"
    );
    assert!(
        !w.p2_record_gates_pass(&b18.0, &b18.1),
        "balden P2[18] successor blocked (C2 needs 0x5B3)"
    );
    w.system_flag_set(0x5B3);
    assert!(
        !w.p2_record_gates_pass(&b19.0, &b19.1),
        "P2[19] self-latches shut (C1 0x5B3)"
    );
    assert!(
        w.p2_record_gates_pass(&b18.0, &b18.1),
        "P2[18] successor unlocked once 0x5B3 set"
    );
}

/// The real seeder ENTRY POINT (`install_gated_p2_record`, retail
/// `FUN_8003BDE0`) honors the pinned gates against the actual scene MANs, in
/// both gate directions: a C1 one-shot (teien P2[1], blocked once 0x1C9 set)
/// and a C2 requires-all (balden P2[18], blocked until 0x5B3 set).
#[test]
fn chapter2_seeder_entry_point_honors_the_disc_gates() {
    let Some(index) = open_index() else { return };
    let (teien_mf, teien) = load_man(&index, "teien");
    let (balden_mf, balden) = load_man(&index, "balden");

    // C1 one-shot: teien P2[1] spawns on a fresh arc, blocks once 0x1C9 set.
    let mut w = World::new();
    assert!(
        w.install_gated_p2_record(&teien_mf, &teien, 1),
        "seeder spawns teien P2[1] when its C1 (0x1C9) is clear"
    );
    let mut w = World::new();
    w.system_flag_set(0x1C9);
    assert!(
        !w.install_gated_p2_record(&teien_mf, &teien, 1),
        "seeder refuses teien P2[1] once 0x1C9 (its C1) is set"
    );

    // C2 requires-all: balden P2[18] blocks until 0x5B3, spawns after.
    let mut w = World::new();
    assert!(
        !w.install_gated_p2_record(&balden_mf, &balden, 18),
        "seeder refuses balden P2[18] while its C2 (0x5B3) is unmet"
    );
    let mut w = World::new();
    w.system_flag_set(0x5B3);
    assert!(
        w.install_gated_p2_record(&balden_mf, &balden, 18),
        "seeder spawns balden P2[18] once 0x5B3 is set"
    );
}
