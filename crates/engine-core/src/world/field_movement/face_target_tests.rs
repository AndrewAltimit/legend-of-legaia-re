use super::*;

/// Talking to a field NPC turns it to face the player: the interaction
/// dispatch ([`World::trigger_field_interact`]) drives the ported `0x4C`
/// `FaceTarget` motion-VM leg through [`World::face_field_npc_toward`] and
/// settles the NPC's [`crate::world::FieldNpcState::headings`] entry onto the player
/// bearing, converging from whatever stale facing it held.
#[test]
fn interaction_start_turns_npc_to_face_player() {
    let mut w = World::new();
    // Player in slot 0, standing at (+100, 0) - due +X of the NPC.
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 100;
    w.actors[0].move_state.world_z = 0;
    // NPC placement slot 3 at the origin, facing the *opposite* way (0x800)
    // so the face leg has a full half-turn to converge.
    w.npcs.positions.insert(3, (0, 0));
    w.npcs.headings.insert(3, 0x800);

    w.trigger_field_interact(0x05, 3);

    // atan2(dx=100, dz=0) = +pi/2 -> 12-bit yaw 0x400 (X+); the one-shot
    // FaceTarget leg snaps straight onto it.
    assert_eq!(w.npcs.headings.get(&3), Some(&0x0400));
}

/// An NPC nothing has turned stands at the spawn default, retail
/// `+0x26 = 0` - engine `0x800`, the pose the hosts draw for an absent
/// heading. The talk snap saves that pose and the teardown puts it back,
/// rather than the opposite compass point an `unwrap_or(0)` read gave.
#[test]
fn talk_restore_of_an_unturned_npc_returns_the_spawn_heading() {
    let mut w = World::new();
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 100;
    w.actors[0].move_state.world_z = 0;
    w.npcs.positions.insert(3, (0, 0));
    assert_eq!(w.npcs.heading(3), crate::world::SPAWN_HEADING);
    w.face_field_npc_at_player(3);
    assert_eq!(w.npcs.headings.get(&3), Some(&0x0400));
    w.release_talk_facing();
    assert_eq!(w.npcs.heading(3), crate::world::SPAWN_HEADING);
}

/// A placement whose own context runs as the interaction timeline (a boss
/// stager) holds its talk pose until that timeline ends: the restore is
/// the dialog SM's teardown, and the SM is still engaged while the record
/// plays.
#[test]
fn interaction_timeline_holds_the_talk_pose_until_it_ends() {
    let mut w = World::new();
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 100;
    w.actors[0].move_state.world_z = 0;
    w.npcs.positions.insert(3, (0, 0));
    w.npcs.headings.insert(3, 0x800);
    w.face_field_npc_at_player(3);
    let mut tl = legaia_engine_field::cutscene_timeline::CutsceneTimeline::new(vec![0x21], 0);
    tl.interaction_slot = Some(3);
    w.cutscene.timeline = Some(tl);
    assert!(w.interaction_timeline_holds_talk_facing());
    // Another placement's timeline does not hold slot 3's pose.
    w.cutscene.timeline.as_mut().unwrap().interaction_slot = Some(4);
    assert!(!w.interaction_timeline_holds_talk_facing());
    w.cutscene.timeline.as_mut().unwrap().interaction_slot = Some(3);
    w.cutscene.timeline.as_mut().unwrap().done = true;
    assert!(!w.interaction_timeline_holds_talk_facing());
    w.release_talk_facing();
    assert_eq!(w.npcs.headings.get(&3), Some(&0x800));
}

/// The face driver rotates toward the bearing from an arbitrary start and
/// is a no-op for a slot with no surfaced position (the retail actor-list
/// miss never poses an actor).
#[test]
fn face_field_npc_toward_converges_and_skips_unplaced() {
    let mut w = World::new();
    // NPC at the origin, facing +X (0x400). Player is due -Z (0, -100):
    // atan2(dx=0, dz=-100) = pi -> yaw 0x800.
    w.npcs.positions.insert(2, (0, 0));
    w.npcs.headings.insert(2, 0x400);
    w.face_field_npc_toward(2, 0, -100);
    assert_eq!(w.npcs.headings.get(&2), Some(&0x0800));

    // A slot with no position is left untouched - no heading is invented.
    w.face_field_npc_toward(9, 100, 100);
    assert!(!w.npcs.headings.contains_key(&9));
}

fn talk_world_with_channel(flags: u32) -> World {
    let mut w = World::new();
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 100;
    w.actors[0].move_state.world_z = 0;
    w.npcs.positions.insert(3, (0, 0));
    w.npcs.headings.insert(3, 0x800);
    let ctx = legaia_engine_vm::field::FieldCtx {
        flags,
        ..Default::default()
    };
    w.field_vm
        .channels
        .push(legaia_engine_field::field_channels::FieldChannel {
            placement_index: 3,
            ctx,
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        });
    w
}

/// The dialog SM's exit restores `+0x5A` with no compare against the snapped
/// bearing (`0x80039CC8..0x80039CE8`): a turn the talk record gave its own
/// placement is undone when the talk ends.
#[test]
fn a_talk_turn_is_undone_by_the_restore() {
    let mut w = talk_world_with_channel(0x0002_0000);
    w.face_field_npc_at_player(3);
    assert_eq!(w.npcs.headings.get(&3), Some(&0x400));
    // The record turns its talker onto another compass point.
    w.npcs.headings.insert(3, 0xC00);
    w.release_talk_facing();
    assert_eq!(w.npcs.headings.get(&3), Some(&0x800));
}

/// `0x400000` on the talker at the end skips the restore, and the save is
/// taken anyway at the touch (`0x801D5BE4`), so a talk that drops the bit
/// (`32 16`) restores a placement the snap skipped.
#[test]
fn the_class_bit_at_the_end_decides_the_restore() {
    let mut w = talk_world_with_channel(0x0042_0000);
    w.face_field_npc_at_player(3);
    assert_eq!(
        w.npcs.headings.get(&3),
        Some(&0x800),
        "no snap under 0x400000"
    );
    w.npcs.headings.insert(3, 0xC00);
    w.release_talk_facing();
    assert_eq!(w.npcs.headings.get(&3), Some(&0xC00), "kept: the bit is up");

    let mut w = talk_world_with_channel(0x0042_0000);
    w.face_field_npc_at_player(3);
    w.npcs.headings.insert(3, 0xC00);
    w.field_vm.channels[0].ctx.flags &= !0x0040_0000;
    w.release_talk_facing();
    assert_eq!(
        w.npcs.headings.get(&3),
        Some(&0x800),
        "restored: the talk dropped it"
    );
}
