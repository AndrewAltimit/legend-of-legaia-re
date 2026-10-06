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

/// A stream bound to a placed object (a partition-0 record) swaps that
/// record's live model, the table both hosts draw the record's first
/// placement through - `koin3`'s video wall cycles its panels this way.
#[test]
fn an_object_stream_swaps_its_records_model() {
    let mut w = World::new();
    w.npcs.object_ambient.insert(
        5,
        FieldNpcAmbient {
            walks: false,
            // 0E 16 00 (model 22), wait 12, 0E 1B 00 (model 27), loop.
            variants: vec![(
                legaia_asset::man_motion::SELECTOR_DEFAULT,
                vec![0x0E, 0x16, 0x00, 0x05, 0x0C, 0x0E, 0x1B, 0x00, 0x01],
            )],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(5, 0),
        },
    );
    assert!(w.object_live_models().is_empty());
    w.tick_field_npc_ambient();
    assert_eq!(w.object_live_models().get(&5), Some(&0x16));
    // A record no stream drives is never seeded.
    w.seed_object_live_model(9, 3);
    assert_eq!(w.object_live_models().get(&9), None);
    w.seed_object_live_model(5, 0x1E);
    assert_eq!(w.object_live_models().get(&5), Some(&0x1E));
}
