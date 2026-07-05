use super::*;

#[test]
fn locomotion_moves_player_on_dpad() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 200;
    // Up -> +Z. speed = (8 * 0x1000 >> 12) * 1 = 8 -> +8 in 2-unit steps.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 208);
    assert_eq!(world.actors[0].move_state.world_x, 200);
}

#[test]
fn locomotion_diagonal_normalises_speed() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 400;
    world.actors[0].move_state.world_z = 400;
    // Up+Right -> Z+ and X+. speed = 8, diagonal -= 8>>2 = 6 -> +6 each.
    world.set_pad(input::PadButton::Up.mask() | input::PadButton::Right.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 406);
    assert_eq!(world.actors[0].move_state.world_x, 406);
}

#[test]
fn locomotion_stops_at_wall() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 250;
    // Block tile (col=1, row=3) - covers world z in [256,384) under the
    // retail biased derivation, the band the +Z walk crosses into at
    // z=256.
    world.paint_field_collision(1, (1, 2), (3, 4), 0);
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    // Player advances 250 -> 254, then the candidate 256 lands in the
    // blocked tile and is rejected. Without the wall it would reach 258.
    assert_eq!(world.actors[0].move_state.world_z, 254);
    assert_eq!(world.actors[0].move_state.world_x, 200);
}

#[test]
fn locomotion_follows_terrain_height_only_when_gated_on() {
    const STRIDE: usize = 0x80;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.reset_field_collision_grid();
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 200;
    world.actors[0].move_state.world_y = 999; // sentinel
    // Floor tier 3 -> -40 across the 2x2 block around tile (1,1), which the
    // +Z walk lands in (x=200, z=208 -> tile (1,1)).
    world.field_floor_height_lut[3] = -40;
    let base = STRIDE + 1;
    for &i in &[base, base + 1, base + STRIDE, base + STRIDE + 1] {
        world.field_collision_grid[i] = 0x03; // low nibble = tier 3, walkable
    }

    // Gate off (default): Y stays at the sentinel, flat-Y behaviour preserved.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 208);
    assert_eq!(world.actors[0].move_state.world_y, 999);

    // Gate on: the next step snaps Y to the sampled floor height.
    world.follow_terrain_height = true;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_y, -40);
}

#[test]
fn locomotion_gated_by_movement_disabled_flag() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    world.actors[0].move_state.flags |= 0x0008_0000; // encounter / cutscene owns player
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "no movement while disabled"
    );
}

#[test]
fn locomotion_gated_by_active_dialog() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    world.current_dialog = Some(DialogRequest {
        text_id: 1,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "dialog owns the frame"
    );
}

#[test]
fn locomotion_deterministic_across_identical_pad_stream() {
    fn drive(pads: &[u16]) -> (i16, i16) {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 300;
        world.actors[0].move_state.world_z = 300;
        // A couple of deterministic walls so collision rejection is in
        // the path being compared.
        world.paint_field_collision(1, (0, 3), (0, 3), 0);
        for &p in pads {
            world.set_pad(p);
            let _ = world.tick();
        }
        let ms = &world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    }
    let up = input::PadButton::Up.mask();
    let down = input::PadButton::Down.mask();
    let left = input::PadButton::Left.mask();
    let right = input::PadButton::Right.mask();
    let seq = [up, up | right, right, down, down | left, left, 0, up];
    assert_eq!(
        drive(&seq),
        drive(&seq),
        "identical pad stream is bit-identical"
    );
}

#[test]
fn cutscene_narration_roller_is_timer_driven_not_confirm_paced() {
    use crate::cutscene_narration::{DEFAULT_FRAMES_PER_PIXEL, RollerParams};
    let mut world = World::new();
    world.mode = SceneMode::Title; // isolate the top-of-tick narration advance
    world.open_cutscene_narration(vec!["Page 1".into(), "Page 2".into()]);
    let entered = |w: &World| {
        w.cutscene_narration
            .as_ref()
            .map(|n| n.current_index())
            .unwrap_or(usize::MAX)
    };
    assert_eq!(entered(&world), 0, "no line has entered yet");

    // The roller advances on the 60fps field sub-clock, not once per
    // `World::tick` (the sim runs at 100Hz): roughly `floor(3*ticks/5)`
    // roller-frames elapse per `ticks` World::ticks. Scale each crawl budget
    // by 100/60 (plus a small margin) to cover the same number of pixel steps.
    let ticks_for = |roller_frames: u32| roller_frames * 100 / 60 + 4;

    // A confirm press does NOT advance the crawl (retail `FUN_80037174` is
    // timer-driven; the intro skip goes through the hand-off packet).
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(entered(&world), 0);

    // The timer does: after one pixel step the first line enters.
    world.set_pad(0);
    for _ in 0..ticks_for(DEFAULT_FRAMES_PER_PIXEL) {
        let _ = world.tick();
    }
    assert_eq!(entered(&world), 1, "line 0 entered on the first pixel step");

    // Ticking through the whole crawl (2 entries + a full window traversal)
    // completes the block and clears the presenter, releasing the suspended
    // timeline.
    let p = RollerParams::DEFAULT;
    let roller_budget =
        (2 * p.line_step as u32 + (p.enter_y - p.exit_y) as u32 + 4) * p.frames_per_pixel;
    for _ in 0..ticks_for(roller_budget) {
        let _ = world.tick();
    }
    assert!(
        world.cutscene_narration.is_none(),
        "the crawl completes on its own timer"
    );
}

#[test]
fn locomotion_gated_while_cutscene_timeline_active() {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    // An opening-cutscene timeline owns the scene (establishing sweep). A
    // non-empty body so it is not immediately `done`.
    world.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x21, 0x2E, 0x1A], 0));
    assert!(world.cutscene_timeline_active());
    world.set_pad(input::PadButton::Up.mask());
    world.step_field_locomotion();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "pad-driven walk is locked while the cutscene timeline owns the scene"
    );
    // Once the timeline finishes, free-roam control returns.
    if let Some(tl) = world.cutscene_timeline.as_mut() {
        tl.done = true;
    }
    assert!(!world.cutscene_timeline_active());
    world.step_field_locomotion();
    assert_eq!(
        world.actors[0].move_state.world_z, 208,
        "locomotion resumes the frame the timeline drops"
    );
}

#[test]
fn world_tick_drives_per_actor_move_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.actors[0].active = true;
    // Move-VM bytecode: WORLD_SET (op 0x07) x=42, y=10, z=5, then HALT.
    world.set_move_bytecode(0, Some(vec![0x0007, 42, 10, 5, 0x0008]));
    let _ = world.tick();
    // First step is WORLD_SET; should write the position.
    assert_eq!(world.actors[0].move_state.world_x, 42);
    assert_eq!(world.actors[0].move_state.world_y, 10);
}

#[test]
fn world_tick_skips_move_vm_when_wait_timer_set() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.wait_timer = 5;
    world.set_move_bytecode(0, Some(vec![0x0007, 99, 99, 99, 0x0008]));
    let _ = world.tick();
    // Wait timer decremented, but move VM didn't run -> position unchanged.
    assert_eq!(world.actors[0].move_state.wait_timer, 4);
    assert_eq!(world.actors[0].move_state.world_x, 0);
}

#[test]
fn load_field_script_resets_pc_and_ctx() {
    let mut world = World::new();
    world.field_pc = 42;
    world.field_ctx.flags = 0xFFFF;
    world.load_field_script(vec![0xFF; 8]);
    assert_eq!(world.field_pc, 0);
    assert_eq!(world.field_ctx.flags, 0);
    assert_eq!(world.field_bytecode.len(), 8);
}
