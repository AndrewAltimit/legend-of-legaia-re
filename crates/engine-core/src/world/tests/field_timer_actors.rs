//! The three field-overlay frame-delta timer templates, driven through the
//! real field VM: the cinematic bar wipe (`43 0C` / `FUN_801DD784`), the
//! eased move (`43 09` with a tick count / `FUN_801DD4C4`) and the
//! floor-height-ladder oscillator (`4C 90` / `FUN_801DA930`).
//!
//! Every script below is the byte sequence a scene MAN actually carries -
//! the operand triples are quoted from the on-disc sites named in each test.

use super::*;

/// Give the world the frame cadence the field tick reads (`DAT_1F800393`).
fn field_world() -> World {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_frame_step = 1;
    world
}

/// `43 0C 10 08 10` is the exact instruction `balden` / `balden2` P1[4..61]
/// and `town0c` P1[24] carry (11 / 11 / 7 sites at real opcode boundaries):
/// close over 16 ticks, hold 8, open over 16. The other on-disc flavour is
/// `18 08 A0` (`deroa` P2[9], `station3` P2[0]) - a slow reveal.
///
/// The property the disassembly states, end to end through the VM: the bars
/// grow to the full `0x73`, cover the screen, come back, and the record
/// retires itself.
#[test]
fn op_43_0c_runs_the_camera_shutter_wipe_to_full_and_back() {
    let mut world = field_world();
    world.load_field_script(vec![0x43, 0x0C, 0x10, 0x08, 0x10]);
    let mut heights = Vec::new();
    for _ in 0..64 {
        let _ = world.tick();
        heights.push(world.cinematic_bar);
    }
    let peak = *heights.iter().max().unwrap();
    assert_eq!(
        peak,
        legaia_engine_vm::field_actor_timers::SHUTTER_FULL_BAR,
        "the envelope must reach the full 0x73: {heights:?}"
    );
    assert_eq!(
        *heights.last().unwrap(),
        0,
        "and the record must retire: {heights:?}"
    );
    assert!(world.cinematic_bars.is_none());
}

/// At full envelope the two bars cover a 224-line screen twice over - the
/// beat is a blackout, not a 2.35:1 crop. Asserted on the emitted rects so
/// the reading cannot drift back.
#[test]
fn the_full_envelope_blacks_out_the_whole_screen() {
    use legaia_engine_vm::field_actor_timers as t;
    let [top, bottom] = t::shutter_bar_rects(t::SHUTTER_FULL_BAR, t::SHUTTER_RETAIL_SCREEN_H);
    assert!(bottom.y0 <= top.y1, "{top:?} / {bottom:?} leave a gap");
}

/// `jou`'s scene-entry script installs the ladder with `4C 9E` and then sets
/// rung after rung oscillating with `4C 90 <rung> 50 00 16 00 <phase> 80` -
/// period `0x50`, amplitude `0x16`, and an arm word whose burst count
/// phase-offsets each rung by ten steps. This drives that pair and asserts
/// the rung actually leaves the height the install gave it.
#[test]
fn op_4c_90_oscillates_a_floor_height_rung_the_ladder_installed() {
    let mut world = field_world();
    // `4C 9E` with the linear ramp `i * 0x20` (jou P1[0] `+0x003C`), then
    // `4C 90` on rung 4 (jou P1[0] `+0x0190`).
    let mut script = vec![0x4C, 0x9E];
    for i in 0..16u16 {
        script.extend_from_slice(&(i * 0x20).to_le_bytes());
    }
    script.extend_from_slice(&[0x4C, 0x90, 0x04, 0x50, 0x00, 0x16, 0x00, 0x00, 0x80]);
    world.load_field_script(script);

    let _ = world.tick(); // 4C 9E
    assert_eq!(
        world.field_floor_height_lut[4],
        -(4 * 0x20),
        "the install writes the negated ladder, matching the MAN-header seed"
    );
    let installed = world.field_floor_height_lut[4];

    let _ = world.tick(); // 4C 90 spawns
    assert_eq!(world.floor_tier_bobs.len(), 1);
    let mut seen = Vec::new();
    for _ in 0..96 {
        let _ = world.tick();
        seen.push(world.field_floor_height_lut[4]);
    }
    let hi = *seen.iter().max().unwrap();
    let lo = *seen.iter().min().unwrap();
    assert!(
        hi > installed,
        "the rung must rise off {installed}: span {lo}..{hi}"
    );
    assert!(lo < hi, "and swing back: span {lo}..{hi}");
    // Untouched rungs stay where the install put them.
    assert_eq!(world.field_floor_height_lut[5], -(5 * 0x20));
}

/// `4C 9F` is a retire sweep over the same handler, so it cancels every
/// live rung oscillator - it registers nothing.
#[test]
fn op_4c_9f_cancels_the_running_rung_oscillators() {
    // Two rungs armed, no sweep: both records stand.
    let mut spawn_only = field_world();
    let mut script = vec![0x4C, 0x90, 0x04, 0x50, 0x00, 0x16, 0x00, 0x00, 0x80];
    script.extend_from_slice(&[0x4C, 0x90, 0x05, 0x50, 0x00, 0x16, 0x00, 0x0A, 0x80]);
    spawn_only.load_field_script(script.clone());
    for _ in 0..4 {
        let _ = spawn_only.tick();
    }
    assert_eq!(spawn_only.floor_tier_bobs.len(), 2);

    // The same script with the sweep appended leaves none - which is the
    // contrast that makes the assertion non-vacuous.
    let mut swept = field_world();
    script.extend_from_slice(&[0x4C, 0x9F]);
    swept.load_field_script(script);
    for _ in 0..4 {
        let _ = swept.tick();
    }
    assert!(
        swept.floor_tier_bobs.is_empty(),
        "4C 9F sweeps the handler class"
    );
}

/// `43 09` with a non-zero tick count eases the ctx's actor to the target
/// triple over that many ticks, arriving exactly on the end value.
#[test]
fn op_43_09_with_ticks_eases_the_player_to_the_target() {
    let mut world = field_world();
    world.player_actor_slot = Some(1);
    {
        let a = world.spawn_actor(1);
        a.move_state.world_x = 0;
        a.move_state.world_z = 0;
    }
    // `[43, 09, x:i16, y:i16, z:i16, ticks:i16]`. `load_field_script` resets
    // the ctx, so the player class bit goes on afterwards.
    world.load_field_script(vec![
        0x43, 0x09, 0x00, 0x01, 0xFF, 0xFF, 0x00, 0x02, 0x08, 0x00,
    ]);
    world.field_ctx.flags |= 0x0100_0000;
    let _ = world.tick();
    assert_eq!(world.eased_moves.len(), 1, "the tween record spawned");

    let mut xs = Vec::new();
    for _ in 0..8 {
        let _ = world.tick();
        xs.push(world.actors[1].move_state.world_x);
    }
    assert_eq!(xs.last().copied(), Some(0x0100), "arrives at the end value");
    assert_eq!(world.actors[1].move_state.world_z, 0x0200, "Z arrives too");
    // Quadratic, not linear: the halfway sample is nearer the start than the
    // midpoint would be.
    assert!(
        xs[3] < 0x0080,
        "t^2/d^2 keeps the halfway sample below the midpoint: {xs:?}"
    );
    assert!(
        world.eased_moves.is_empty(),
        "the record retires on arrival"
    );
}

/// A `-1` axis in the operand stream disables that axis - the same
/// `0xFFFF` sentinel the zero-tick immediate branch skips. The Y operand
/// above is `0xFFFF`, so the actor's Y must be untouched by the tween.
#[test]
fn op_43_09_leaves_a_minus_one_axis_alone() {
    let mut world = field_world();
    world.player_actor_slot = Some(1);
    {
        let a = world.spawn_actor(1);
        a.physics.world_y = 0x0777;
    }
    world.load_field_script(vec![
        0x43, 0x09, 0x00, 0x01, 0xFF, 0xFF, 0x00, 0x02, 0x04, 0x00,
    ]);
    world.field_ctx.flags |= 0x0100_0000;
    for _ in 0..8 {
        let _ = world.tick();
    }
    assert_eq!(world.actors[1].physics.world_y, 0x0777);
}

/// The MAN loader's retire sweep drops every live timer record: its first
/// arm is `FUN_8003CF40` against `LAB_801DA930`, and a rung bob left running
/// across a scene change would keep writing into the next scene's ladder.
#[test]
fn the_man_load_sweep_drops_every_live_timer_record() {
    let mut world = field_world();
    world.load_field_script(vec![
        0x43, 0x0C, 0x10, 0x08, 0x10, 0x4C, 0x90, 0x04, 0x50, 0x00, 0x16, 0x00, 0x00, 0x80,
    ]);
    let _ = world.tick();
    let _ = world.tick();
    assert!(world.cinematic_bars.is_some());
    assert_eq!(world.floor_tier_bobs.len(), 1);
    let _ = world.man_load_actor_reset();
    assert!(world.cinematic_bars.is_none());
    assert_eq!(world.cinematic_bar, 0);
    assert!(world.floor_tier_bobs.is_empty());
    assert!(world.eased_moves.is_empty());
}

/// The `+0x8E` inverted-Y mirror reaches a consumer.
///
/// `FUN_801DD4C4` stores `-Y` into the target's `+0x8E` when the target
/// carries `0x20000000` (`0x801DD6A4..0x801DD6B8`), and `FUN_8003BC08`'s
/// height arm reads it back negated into `+0x16` **instead of** sampling the
/// ground (`0x8003BC4C..0x8003BC64` branches past both floor arms). So the
/// property is not "the byte is stored" - it is that an armed mirror wins
/// over the terrain follow, and that the eased Y is what survives.
///
/// The script is `43 09` with a live Y axis and a tick count; the world runs
/// with `follow_terrain_height` on and a floor the player is nowhere near, so
/// an unmirrored move would be dragged onto it every frame.
#[test]
fn op_43_09_mirror_flag_holds_the_eased_y_against_the_terrain_follow() {
    use legaia_engine_vm::field_actor_timers::EASE_TARGET_INVERT_Y;

    let script = vec![0x43, 0x09, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x08, 0x00];

    // Control: no mirror flag. The terrain follow owns Y, and the tween's own
    // Y write (into the physics seat) does not reach `move_state`.
    let mut plain = field_world();
    plain.player_actor_slot = Some(1);
    plain.follow_terrain_height = true;
    plain.spawn_actor(1);
    plain.load_field_script(script.clone());
    plain.field_ctx.flags |= 0x0100_0000;
    for _ in 0..4 {
        let _ = plain.tick();
    }
    assert_eq!(plain.field_eased_mirror_y, None, "no flag, no latch");
    let floor = plain.sample_field_floor_height(
        plain.actors[1].move_state.world_x as i32,
        plain.actors[1].move_state.world_z as i32,
    ) as i16;
    assert_eq!(
        plain.actors[1].move_state.world_y, floor,
        "without the mirror the follow owns Y"
    );

    // With the flag: the latch carries `-Y` and the height arm writes `Y`.
    let mut mirrored = field_world();
    mirrored.player_actor_slot = Some(1);
    mirrored.follow_terrain_height = true;
    mirrored.spawn_actor(1);
    mirrored.load_field_script(script);
    mirrored.field_ctx.flags |= 0x0100_0000 | EASE_TARGET_INVERT_Y;
    for _ in 0..4 {
        let _ = mirrored.tick();
    }
    let latch = mirrored
        .field_eased_mirror_y
        .expect("the mirror flag arms the latch");
    let eased_y = mirrored.actors[1].physics.world_y;
    assert_eq!(latch, eased_y.wrapping_neg(), "the latch is the negated Y");
    assert_eq!(
        mirrored.actors[1].move_state.world_y, eased_y,
        "the height arm writes -(+0x8E) = the eased Y, not the floor"
    );

    // ...and the ease really moved Y off the floor, so the assertion above is
    // not accidentally comparing two zeros.
    assert_ne!(eased_y, 0, "the Y axis is live in this script");

    // The latch drops with the record: nothing left to hold Y once the move
    // has retired, so the follow takes over again.
    for _ in 0..16 {
        let _ = mirrored.tick();
    }
    assert!(mirrored.eased_moves.is_empty(), "the tween retired");
    assert_eq!(mirrored.field_eased_mirror_y, None, "the latch retired too");
}
