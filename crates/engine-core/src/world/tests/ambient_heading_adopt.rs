//! The ambient stream's heading is the actor's one `+0x26`: a talk snap, its
//! restore or a script turn writes the published heading, and the stream's
//! next turn starts from it (`World::tick_field_npc_ambient`).

use super::*;

/// A stream that only waits, on `slot`, with the walk mirror live.
fn waiting_stream(w: &mut World, slot: u8) {
    w.npcs.animate = true;
    w.npcs.ambient.insert(
        slot,
        FieldNpcAmbient {
            defers: false,
            walks: false,
            variants: vec![(
                legaia_scene_formats::man_motion::SELECTOR_DEFAULT,
                vec![0x05, 0x7F, 0x01],
            )],
            live: None,
            vm: legaia_engine_motion_vm::ambient_motion::AmbientMotion::new(u32::from(slot), 0),
        },
    );
}

#[test]
fn the_stream_takes_a_heading_another_writer_published() {
    let mut w = World::new();
    waiting_stream(&mut w, 4);
    w.npcs.ambient.get_mut(&4).unwrap().vm.heading = 0x800;
    // The talk's restore put back `0xE80` (retail space): engine `0x680`.
    w.npcs.headings.insert(4, 0x680);
    w.tick_field_npc_ambient();
    assert_eq!(w.npcs.ambient[&4].vm.heading, 0xE80);
}

#[test]
fn a_ramps_raw_heading_is_kept_when_it_publishes_as_itself() {
    let mut w = World::new();
    waiting_stream(&mut w, 4);
    // A `0x0D` ramp holds `+0x26` out of range; its masked value is what
    // the world published.
    w.npcs.ambient.get_mut(&4).unwrap().vm.heading = 0x1100;
    w.npcs.headings.insert(4, 0x900);
    w.tick_field_npc_ambient();
    assert_eq!(w.npcs.ambient[&4].vm.heading, 0x1100);
}
