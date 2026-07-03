use super::*;

// ---------------------------------------------------------------------------
// Field-NPC motion (motion-VM wiring) + prop walk-touch dispatch
// ---------------------------------------------------------------------------

#[test]
fn field_npc_patrol_route_walks_through_motion_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_npc_positions.insert(1, (1000, 1000));
    world
        .field_npc_routes
        .insert(1, vec![(1300, 1000), (1000, 1000)]);

    // Baseline: with `animate_field_npcs` off the NPC rests at its anchor.
    for _ in 0..20 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_positions.get(&1), Some(&(1000, 1000)));

    // Flag on: the motion VM walks the NPC toward waypoint 0 at the per-frame
    // speed (8 units), reaches it, then patrols back toward waypoint 1.
    world.animate_field_npcs = true;
    let _ = world.tick();
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1008, 1000)),
        "one tick = one motion-VM step of FIELD_NPC_MOTION_SPEED units"
    );
    for _ in 0..37 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1300, 1000)),
        "the leg clamps at the waypoint (300 units / 8 per frame)"
    );
    for _ in 0..5 {
        let _ = world.tick();
    }
    let &(x, _) = world.field_npc_positions.get(&1).unwrap();
    assert!(x < 1300, "patrol loops: the NPC heads back to waypoint 1");
}

#[test]
fn moving_field_npc_collision_box_follows_live_position() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.solid_field_npcs = true;
    world.animate_field_npcs = true;
    world.field_npc_positions.insert(1, (1000, 1000));
    world.field_npc_routes.insert(1, vec![(1300, 1000)]);

    // Anchor blocks before the walk: the X+ probe from 102 out lands 38
    // inside the strict ±40 box (104 out reads exactly 40 = clear).
    assert!(world.field_actor_dir_blocked(1000 - 102, 1000, 3));

    // Walk the NPC to its waypoint (one-shot route).
    for _ in 0..60 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_positions.get(&1), Some(&(1300, 1000)));
    assert!(
        world.field_npc_motions.is_empty(),
        "a one-waypoint route rests after arrival (no restart churn)"
    );

    // The ±40 moving-actor box follows the LIVE position: the abandoned
    // anchor no longer blocks, the new position does.
    assert!(!world.field_actor_dir_blocked(1000 - 102, 1000, 3));
    assert!(world.field_actor_dir_blocked(1300 - 102, 1000, 3));
}

#[test]
fn autonomous_legs_pause_during_dialogue_scripted_legs_run() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.animate_field_npcs = true;
    world.field_npc_positions.insert(1, (1000, 1000));
    world.field_npc_routes.insert(1, vec![(1300, 1000)]);
    world.field_npc_positions.insert(2, (2000, 2000));

    // A dialogue is up: the autonomous patrol must not start (retail's
    // interaction motion-pause), but a script-started leg (the interaction
    // partner's own prologue walk) keeps stepping.
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline: vec![],
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    assert!(world.start_field_npc_motion(2, 2080, 2000));
    for _ in 0..10 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1000, 1000)),
        "autonomous patrol paused while the box is up"
    );
    assert_eq!(
        world.field_npc_positions.get(&2),
        Some(&(2080, 2000)),
        "scripted leg runs through the dialogue"
    );

    // Box dismissed: the patrol resumes.
    world.current_dialog = None;
    for _ in 0..10 {
        let _ = world.tick();
    }
    let &(x, _) = world.field_npc_positions.get(&1).unwrap();
    assert!(x > 1000, "patrol resumes once the dialogue clears");
}

#[test]
fn start_field_npc_motion_requires_installed_slot() {
    // The retail start kernel's actor-list search miss returns 0: a slot
    // with no installed placement starts nothing.
    let mut world = World::new();
    assert!(!world.start_field_npc_motion(9, 100, 100));
    assert!(world.field_npc_motions.is_empty());
}

#[test]
fn walk_touch_warp_posts_once_per_contact_and_queues_transition() {
    use crate::man_field_scripts::WalkTouchEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 1000;
    world.actors[0].move_state.world_z = 2000;
    world
        .field_walk_touch
        .insert(5, ((1200, 2000), WalkTouchEvent::Warp { target_map: 3 }));

    // Baseline: standing outside the ±80 contact box posts nothing.
    let _ = world.drain_field_events();
    for _ in 0..3 {
        let _ = world.tick();
    }
    assert!(world.pending_scene_transition.is_none());
    assert!(world.drain_field_events().is_empty());

    // Hold screen-right (camera azimuth 0: world X+) into the placement.
    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..25 {
        let _ = world.tick();
    }
    assert_eq!(
        world.pending_scene_transition,
        Some(3),
        "the door-warp queues through the same path the 0x3E op uses"
    );
    let events = world.drain_field_events();
    let touches: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, FieldEvent::FieldInteract { slot: 5, .. }))
        .collect();
    assert_eq!(touches.len(), 1, "one post per contact (edge latch)");

    // Still inside the box: no re-post while the contact persists.
    for _ in 0..5 {
        let _ = world.tick();
    }
    assert!(
        world
            .drain_field_events()
            .iter()
            .all(|e| !matches!(e, FieldEvent::FieldInteract { .. })),
        "sustained contact does not re-post"
    );
}

#[test]
fn walk_touch_player_moveto_teleports_player() {
    use crate::man_field_scripts::WalkTouchEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 1000;
    world.actors[0].move_state.world_z = 2000;
    world.field_walk_touch.insert(
        7,
        (
            (1150, 2000),
            WalkTouchEvent::PlayerMoveTo {
                world_x: 5000,
                world_z: 6000,
            },
        ),
    );

    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..30 {
        let _ = world.tick();
        // The snap is the contact tick's last write; stop before the next
        // walk tick moves the player again.
        if world.actors[0].move_state.world_x >= 4000 {
            break;
        }
    }
    let ms = &world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (5000, 6000),
        "touching the placement snaps the player to the decoded coords"
    );
    assert!(
        world.drain_field_events().iter().any(|e| matches!(
            e,
            FieldEvent::MoveTo {
                world_x: 5000,
                world_z: 6000,
                is_player: true
            }
        )),
        "the teleport surfaces as a player MoveTo event"
    );
    // One more walking tick: the touch dispatch re-runs (it lives on the
    // locomotion step) with the player now far outside the contact box, so
    // the edge latch releases for the next approach.
    let _ = world.tick();
    assert!(
        world.active_walk_touch.is_none(),
        "the teleport leaves the contact box, releasing the latch"
    );
}

#[test]
fn interaction_prologue_npc_run_walks_the_interacted_npc() {
    // A synthetic interaction record: prologue = one `0x4C 0x51` NPC run to
    // tile (12, 10), then a text segment. Driving the interact through the
    // opt-in field-VM runner must start the NPC's walk leg (the host hook
    // routing the op to the interacted placement slot) and the field ticks
    // must converge the NPC on the decoded tile-centre world position.
    let target_x = 12i16 * 0x80 + 0x40;
    let target_z = 10i16 * 0x80 + 0x40;
    let mut body = vec![0x4C, 0x51, 12, 10, 0, 5];
    let first_segment = body.len();
    body.extend_from_slice(&[0x1F, b'h', b'i', 0x00, 0x00]);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world
        .field_npc_positions
        .insert(3, (target_x - 80, target_z));
    world
        .field_npc_dialog
        .insert(3, body[first_segment..].to_vec());
    world.field_npc_dialog_prologue.insert(
        3,
        crate::man_field_scripts::InlineDialogPrologue {
            body,
            entry_pc: 0,
            first_segment,
        },
    );

    world.trigger_field_interact(0, 3);
    for _ in 0..15 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&3),
        Some(&(target_x, target_z)),
        "the prologue's 0x4C 0x51 walked the interacted NPC to its tile"
    );
}
