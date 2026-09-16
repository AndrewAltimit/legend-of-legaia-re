//! The scripted mesh re-bind (scripted-motion VM op `0x0E`) as a live world
//! field: the effect arm records the new model id per placement slot, and
//! `World::field_npc_live_model` is what each host reads it back through.
//!
//! The byte side - resolving that id to a TMD out of the scene's model bank -
//! is disc-gated and lives in `tests/model_rebind_live_disc.rs`.

use super::*;

/// Install a one-variant ambient channel on `slot` running `code`.
fn with_ambient(w: &mut World, slot: u8, code: Vec<u8>) {
    w.npcs.ambient.insert(
        slot,
        FieldNpcAmbient {
            walks: false,
            variants: vec![(legaia_asset::man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(u32::from(slot), 0),
        },
    );
}

#[test]
fn op_0x0e_records_the_new_model_on_the_world() {
    let mut w = World::new();
    // `[0E, lo, hi]` with operand 0x0021 - a scene-bank id (below 0xF0).
    with_ambient(&mut w, 5, vec![0x0E, 0x21, 0x00]);
    assert_eq!(w.field_npc_live_model(5), None, "nothing swapped yet");
    w.tick_field_npc_ambient();
    assert_eq!(
        w.field_npc_live_model(5),
        Some(0x21),
        "the effect arm recorded the re-bound id"
    );
}

#[test]
fn a_player_bank_operand_comes_back_in_the_raw_id_space() {
    let mut w = World::new();
    // Operand 0x00F2 takes the `>= 0xF0` arm, which the VM reports as bank
    // `Special` + offset 2. What a host resolves is the raw id, so the round
    // trip through the world has to give `0xF2` back and not `2`.
    with_ambient(&mut w, 7, vec![0x0E, 0xF2, 0x00]);
    w.tick_field_npc_ambient();
    assert_eq!(w.field_npc_live_model(7), Some(0xF2));
    let r = crate::model_bank::resolve_model_id(0xF2);
    assert_eq!(r.bank, crate::model_bank::ModelBank::Player);
    assert_eq!(r.index, 2);
    assert!(
        r.translucent,
        "the `>= 0xF0` arm raises the translucent bit"
    );
}

#[test]
fn the_installer_and_the_reader_are_the_same_slot_space() {
    let mut w = World::new();
    w.set_field_npc_live_model(3, 0x0C);
    assert_eq!(w.field_npc_live_model(3), Some(0x0C));
    assert_eq!(w.field_npc_live_model(4), None);
}
