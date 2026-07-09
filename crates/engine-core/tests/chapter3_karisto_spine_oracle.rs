//! Disc-gated runtime oracle: the chapter-3 (Karisto) progression gate spine
//! driven through the engine's own C1/C2 record-gate machinery.
//!
//! The gate families were mined disc-static and pinned in
//! `man_variant_carrier_census_disc::{chapter3_karisto_castle_gate_families,
//! chapter3_koin_family_and_writer_pins, chapter3_conkram_gate_families}`.
//! This oracle proves the ENGINE sequences them: the pinned disc gates run
//! through the runtime evaluator [`World::p2_record_gates_pass`] and the real
//! seeder entry point [`World::install_gated_p2_record`] against the actual
//! scene MANs - the same no-chapter-specific-code shape the chapter-2 oracle
//! proved (`chapter2_sebucus_spine_oracle`). Every beat here is a field-mode
//! cutscene record whose script `SysFlag.Set` latches its flag organically.
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

/// The Conkram past-arc -> deroa -> chitei2 descent, sequenced through the
/// runtime evaluator: the 0x3E1 bridge flag written in Conkram opens the
/// underground descent in a DIFFERENT region's scene (the cross-region spine).
#[test]
fn chapter3_conkram_bridge_sequences_through_the_engine() {
    let Some(index) = open_index() else { return };
    let (conc_mf, conc) = load_man(&index, "conc");
    let (conc3_mf, conc3) = load_man(&index, "conc3");
    let (deroa_mf, deroa) = load_man(&index, "deroa");

    let soldier = gates(&conc_mf, &conc, 1); // C1=[0x007]
    let chain = gates(&conc_mf, &conc, 10); // C1=[0x3E5] C2=[0x3F9]
    let latch_3e5 = gates(&conc3_mf, &conc3, 10); // C1=[0x3E5]
    let descent = gates(&deroa_mf, &deroa, 4); // C1=[0x46D] C2=[0x3E1]

    let pass = |w: &World, g: &(Vec<u16>, Vec<u16>)| w.p2_record_gates_pass(&g.0, &g.1);
    let mut w = World::new();

    // Fresh arc: soldiers stand, the chain beat waits on 0x3F9, the
    // chitei2 descent waits on the Conkram bridge flag.
    assert!(pass(&w, &soldier), "conc soldier row present pre-disperse");
    assert!(!pass(&w, &chain), "conc P2[10] blocked (C2 needs 0x3F9)");
    assert!(pass(&w, &latch_3e5), "conc3 P2[10] available (0x3E5 clear)");
    assert!(
        !pass(&w, &descent),
        "deroa descent blocked (C2 needs 0x3E1)"
    );

    // Soldiers-disperse beat (0x007, concnow P0[34] / conc2 P0[21]).
    w.system_flag_set(0x7);
    assert!(
        !pass(&w, &soldier),
        "soldier rows latch shut once 0x007 set"
    );

    // conc3 P2[9] executes: sets 0x3F9 - the conc chain beat unlocks.
    w.system_flag_set(0x3F9);
    assert!(pass(&w, &chain), "conc P2[10] unlocked once 0x3F9 set");

    // conc3 P2[10] executes: self-latches 0x3E5 - both close.
    w.system_flag_set(0x3E5);
    assert!(!pass(&w, &chain), "conc P2[10] latches shut (C1 0x3E5)");
    assert!(!pass(&w, &latch_3e5), "conc3 P2[10] self-latches shut");

    // conc2 P2[12] executes: sets the 0x3E1 bridge - the deroa descent to
    // chitei2 opens.
    w.system_flag_set(0x3E1);
    assert!(pass(&w, &descent), "deroa descent open (C2 0x3E1 met)");

    // The descent beat's own one-shot latches it shut.
    w.system_flag_set(0x46D);
    assert!(
        !pass(&w, &descent),
        "deroa P2[4] self-latches shut (C1 0x46D)"
    );
}

/// The kor5 three-step chain (0x43A -> 0x436 -> 0x6C4) and the castle
/// arm-then-consume door flag 0x612 shared by kor/kor3/kor4.
#[test]
fn chapter3_castle_chain_and_door_arm_sequences() {
    let Some(index) = open_index() else { return };
    let (kor_mf, kor) = load_man(&index, "kor");
    let (kor3_mf, kor3) = load_man(&index, "kor3");
    let (kor5_mf, kor5) = load_man(&index, "kor5");

    let pass = |w: &World, g: &(Vec<u16>, Vec<u16>)| w.p2_record_gates_pass(&g.0, &g.1);

    // kor5: the three-step chain.
    let s1 = gates(&kor5_mf, &kor5, 3); // C1=[0x43A]
    let s2 = gates(&kor5_mf, &kor5, 4); // C1=[0x436] C2=[0x43A]
    let s3 = gates(&kor5_mf, &kor5, 5); // C1=[0x436]
    let s4 = gates(&kor5_mf, &kor5, 8); // C1=[0x6C4] C2=[0x436]

    let mut w = World::new();
    assert!(pass(&w, &s1), "chain step 1 open on a fresh arc");
    assert!(!pass(&w, &s2), "step 2 blocked (C2 needs 0x43A)");
    assert!(!pass(&w, &s4), "step 4 blocked (C2 needs 0x436)");
    w.system_flag_set(0x43A);
    assert!(!pass(&w, &s1), "step 1 self-latches shut");
    assert!(pass(&w, &s2), "step 2 unlocked (C2 0x43A)");
    assert!(pass(&w, &s3), "step 3 open (0x436 still clear)");
    w.system_flag_set(0x436);
    assert!(!pass(&w, &s2) && !pass(&w, &s3), "steps 2+3 latch shut");
    assert!(pass(&w, &s4), "step 4 unlocked (C2 0x436)");
    w.system_flag_set(0x6C4);
    assert!(!pass(&w, &s4), "step 4 self-latches shut");

    // The 0x612 door flag: doors across kor/kor3 consume the SAME arm flag,
    // and the arm is re-consumable (entry script re-sets, door re-clears).
    let kor_door = gates(&kor_mf, &kor, 13);
    let kor3_door = gates(&kor3_mf, &kor3, 7);
    let mut w = World::new();
    assert!(!pass(&w, &kor_door), "kor door blocked unarmed");
    assert!(!pass(&w, &kor3_door), "kor3 door blocked unarmed");
    w.system_flag_set(0x612);
    assert!(pass(&w, &kor_door), "kor door passes once armed");
    assert!(
        pass(&w, &kor3_door),
        "kor3 door passes on the same arm flag"
    );
    w.system_flag_clear(0x612);
    assert!(
        !pass(&w, &kor_door),
        "door blocks again after the record clears the arm back"
    );
}

/// The chitei2 underground families + the map03 hub co-writer relationship,
/// and the koin1 0x50A toggle pair (one record while set, the sibling while
/// clear).
#[test]
fn chapter3_chitei2_and_koin_toggle_sequences() {
    let Some(index) = open_index() else { return };
    let (chitei2_mf, chitei2) = load_man(&index, "chitei2");
    let (koin1_mf, koin1) = load_man(&index, "koin1");

    let pass = |w: &World, g: &(Vec<u16>, Vec<u16>)| w.p2_record_gates_pass(&g.0, &g.1);

    // 0x4C8 family: three pre-beat records, closed by the beat that either
    // chitei2 P1[0] or map03 P2[19] (the hub co-writer) executes.
    let pre14 = gates(&chitei2_mf, &chitei2, 14);
    let pre15 = gates(&chitei2_mf, &chitei2, 15);
    let pre19 = gates(&chitei2_mf, &chitei2, 19);
    let mut w = World::new();
    assert!(pass(&w, &pre14) && pass(&w, &pre15) && pass(&w, &pre19));
    w.system_flag_set(0x4C8);
    assert!(
        !pass(&w, &pre14) && !pass(&w, &pre15) && !pass(&w, &pre19),
        "the 0x4C8 family latches shut once the beat lands"
    );

    // 0x4C9/0x4C6: successor requires the 0x4C6 beat, then self-latches.
    let succ = gates(&chitei2_mf, &chitei2, 17);
    let mut w = World::new();
    assert!(!pass(&w, &succ), "P2[17] blocked (C2 needs 0x4C6)");
    w.system_flag_set(0x4C6);
    assert!(pass(&w, &succ), "P2[17] unlocked once 0x4C6 set");
    w.system_flag_set(0x4C9);
    assert!(!pass(&w, &succ), "P2[17] self-latches shut (C1 0x4C9)");

    // koin1 0x50A toggle pair: exactly one of the two records passes in
    // either flag state.
    let while_set = gates(&koin1_mf, &koin1, 9); // C2=[0x50A]
    let while_clear = gates(&koin1_mf, &koin1, 10); // C1=[0x50A]
    let mut w = World::new();
    assert!(!pass(&w, &while_set) && pass(&w, &while_clear));
    w.system_flag_set(0x50A);
    assert!(pass(&w, &while_set) && !pass(&w, &while_clear));
    w.system_flag_clear(0x50A);
    assert!(!pass(&w, &while_set) && pass(&w, &while_clear));
}

/// The real seeder ENTRY POINT (`install_gated_p2_record`, retail
/// `FUN_8003BDE0`) honors the chapter-3 gates against the actual scene MANs,
/// in both directions: the C2 door arm (kor P2[13]) and a C1 one-shot
/// (kor5 P2[3]).
#[test]
fn chapter3_seeder_entry_point_honors_the_disc_gates() {
    let Some(index) = open_index() else { return };
    let (kor_mf, kor) = load_man(&index, "kor");
    let (kor5_mf, kor5) = load_man(&index, "kor5");

    // C2 arm: kor door P2[13] refuses unarmed, spawns once 0x612 is set.
    let mut w = World::new();
    assert!(
        !w.install_gated_p2_record(&kor_mf, &kor, 13),
        "seeder refuses the kor door while 0x612 is unarmed"
    );
    let mut w = World::new();
    w.system_flag_set(0x612);
    assert!(
        w.install_gated_p2_record(&kor_mf, &kor, 13),
        "seeder spawns the kor door once 0x612 is armed"
    );

    // C1 one-shot: kor5 P2[3] spawns fresh, refuses once its latch is set.
    let mut w = World::new();
    assert!(
        w.install_gated_p2_record(&kor5_mf, &kor5, 3),
        "seeder spawns kor5 P2[3] when its C1 (0x43A) is clear"
    );
    let mut w = World::new();
    w.system_flag_set(0x43A);
    assert!(
        !w.install_gated_p2_record(&kor5_mf, &kor5, 3),
        "seeder refuses kor5 P2[3] once 0x43A (its C1) is set"
    );
}
