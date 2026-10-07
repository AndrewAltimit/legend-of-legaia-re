//! `FUN_801CF8AC`'s collision-exempt early-out: an ambient walker whose
//! placement context carries `+0x10 & 3` (op `0x31` bit 0 / 1) never stops
//! for the player; one without it does.

use super::*;

/// A field world with the player standing one probe ahead of an ambient
/// walker in slot 5, whose stream is a `+Z` directional step.
fn walker_world(flags: u32) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.npcs.animate = true;
    // `0x03`, LUT 4 (+Z), pace bits 0, three tiles.
    let code = vec![0x03, 4 << 4, 3];
    w.npcs.ambient.insert(
        5,
        FieldNpcAmbient {
            defers: false,
            walks: true,
            variants: vec![(legaia_asset::man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm: vm::ambient_motion::AmbientMotion::new(5, 0).with_position(1000, 2000),
        },
    );
    // The +Z probe is `(x, z + 64)`: stand the player there.
    w.actors[0].move_state.world_x = 1000;
    w.actors[0].move_state.world_z = 2064;
    w.field_vm
        .channels
        .push(crate::field_channels::FieldChannel {
            placement_index: 5,
            ctx: FieldCtx {
                flags,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        });
    w
}

fn walker_z(w: &World) -> i16 {
    w.npcs.ambient[&5].vm.z
}

#[test]
fn the_player_stops_an_ordinary_walker() {
    let mut w = walker_world(0);
    for _ in 0..4 {
        w.tick_field_npc_ambient();
    }
    assert_eq!(walker_z(&w), 2000, "the class arm refuses the step");
}

#[test]
fn a_collision_exempt_walker_walks_through_the_player() {
    for bit in [1u32, 2] {
        let mut w = walker_world(bit);
        for _ in 0..4 {
            w.tick_field_npc_ambient();
        }
        assert!(
            walker_z(&w) > 2000,
            "`+0x10 & {bit}` returns 0 before the box test (0x801CF8B8)"
        );
    }
}

/// `+0x8A` bit 0 (`FieldNpcAmbient::defers`): a stream that defers to the
/// player executes nothing while its own record runs - a talk on it, or
/// `+0x10 & 0x500` on its context (`FUN_80038158`, `0x800381BC..0x800381C8`);
/// one that does not keeps stepping.
#[test]
fn a_deferring_stream_waits_out_the_engagement() {
    // Stand the player off the walker's probe so only the gate can stop it.
    let fresh = |defers: bool, flags: u32| {
        let mut w = walker_world(flags);
        w.actors[0].move_state.world_z = 9000;
        w.npcs.ambient.get_mut(&5).unwrap().defers = defers;
        w
    };
    let talk = || crate::inline_dialogue::InlineDialogue::from_inline(vec![0x1F, b'K', 0]);
    let run = |w: &mut World| {
        for _ in 0..4 {
            w.tick_field_npc_ambient();
        }
    };
    let mut talking = fresh(true, 0);
    let mut id = talk();
    id.npc_slot = Some(5);
    talking.dialog.inline = Some(id);
    run(&mut talking);
    assert_eq!(
        walker_z(&talking),
        2000,
        "its own talk running: the stream holds"
    );

    let mut busy = fresh(true, 0x100);
    run(&mut busy);
    assert_eq!(
        walker_z(&busy),
        2000,
        "its own script running: the stream holds"
    );

    let mut idle = fresh(true, 0);
    run(&mut idle);
    assert!(walker_z(&idle) > 2000, "nothing holds it: it walks");

    let mut choreographed = fresh(false, 0x100);
    choreographed.dialog.inline = Some(talk());
    run(&mut choreographed);
    assert!(
        walker_z(&choreographed) > 2000,
        "a zero byte keeps choreographing"
    );
}
