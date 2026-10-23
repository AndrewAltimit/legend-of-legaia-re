use super::*;
use legaia_engine_battle_vm::battle_cam_script as script;
use legaia_engine_core::world::World;

/// **Cross-host entry-azimuth pin, browser half.** The twin of the
/// native window's `battle_entry_yaw_tests` (`engine-shell`
/// `window/camera.rs`): both hosts must hand `drive` the *guarded*
/// azimuth, not the raw compass word. This host handed over the raw word,
/// and since the port's fixed follow camera publishes a constant `0`
/// while nobody orbits, that is the seat-axis azimuth on which the two
/// combatant rows project to the same screen X.
#[test]
fn web_an_on_axis_entry_azimuth_is_replaced() {
    for az in [0u16, 8, 160, 191, 2048, 2100, 4090] {
        let mut world = World::default();
        world.locomotion.camera_azimuth = az;
        assert_eq!(
            legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).entry_yaw,
            script::BATTLE_ENTRY_YAW_SAMPLE,
            "azimuth {az} frames both rows at the same screen X"
        );
    }
}

/// A real orbit sample survives untouched - the five captured retail
/// battle yaws.
#[test]
fn web_a_real_orbit_azimuth_survives() {
    for az in [224u16, 2632, 3136, 3808, 3882] {
        let mut world = World::default();
        world.locomotion.camera_azimuth = az;
        assert_eq!(
            legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).entry_yaw,
            f32::from(az)
        );
    }
}

/// **Cross-host heading pin, browser half.** The identical world recipe
/// as the native `the_camera_reads_the_battle_heading_not_the_field_heading`
/// (`engine-shell` `window/camera.rs` tests): an off-axis seat whose
/// field heading `+0x26` and battle heading `+0x46` deliberately differ.
/// Retail's framing subtracts `+0x46` (`FUN_801E295C` case `0x14` writes
/// it at `0x801E32EC..0x801E3318`), so both hosts must land on the same
/// yaw literal.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn web_camera_reads_the_battle_heading_not_the_field_heading() {
    const FIELD_26: u16 = 0x333;
    const BATTLE_46: u16 = 0x600;

    let mut world = World::default();
    world.mode = SceneMode::Battle;
    world.party.party_count = 1;
    let mut vahn = legaia_engine_core::world::Actor::default();
    vahn.active = true;
    vahn.move_state.world_x = 600;
    vahn.move_state.world_z = -775;
    vahn.move_state.render_26 = FIELD_26 as i16;
    vahn.battle.facing_angle = BATTLE_46;
    let mut tetsu = legaia_engine_core::world::Actor::default();
    tetsu.active = true;
    tetsu.move_state.world_x = -400;
    tetsu.move_state.world_z = 900;
    tetsu.battle_monster_id = Some(1);
    tetsu.tmd_binding = Some(0);
    world.actors = vec![vahn, tetsu];
    world.battle.command =
        Some(legaia_engine_battle_menus::battle_input::BattleCommandSession::new(0, 0));
    // The **arts input** picker is what arms retail's case-0 close-up;
    // the command chooser alone keeps the far framing (see
    // `script::phase_for_state`). Same recipe as the native mirror.
    world.battle.arts_menu = Some(legaia_engine_battle::battle_arts::BattleArtsSession::new(
        0,
        0,
        Vec::new(),
    ));

    let inputs = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world);
    assert_eq!(inputs.phase, script::BattleCamPhase::Submenu);
    let acting = inputs.acting.expect("acting actor");
    assert_eq!(acting.facing, i32::from(BATTLE_46), "actor[+0x46]");
    assert_ne!(acting.facing, i32::from(FIELD_26), "not the field +0x26");

    let mut slot = None;
    for f in 0..=6u64 {
        script::drive(&mut slot, true, inputs, f * 2, None);
    }
    let yaw = slot.as_ref().unwrap().pose().yaw.rem_euclid(4096.0);
    // The SAME literals the native mirror asserts.
    assert_eq!(yaw, 752.0, "0x8F0 - 0x600");
    assert_ne!(yaw, 1469.0, "0x8F0 - 0x333 (the field heading)");
}

/// **Cross-host presence pin, browser half.** The identical world recipe
/// as the native `the_formation_box_is_a_world_fact_not_a_render_fact`:
/// a live combatant seated off the origin whose mesh this host has NOT
/// bound. Retail's case-9 walk gates on `actor[+0x14c]` (`0x801D7000`)
/// and consults no mesh, so the box must contain it on both hosts.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn web_formation_box_is_a_world_fact_not_a_render_fact() {
    let mut world = World::default();
    world.mode = SceneMode::Battle;
    world.party.party_count = 1;
    let mut vahn = legaia_engine_core::world::Actor::default();
    vahn.active = true;
    vahn.move_state.world_z = -800;
    let mut tetsu = legaia_engine_core::world::Actor::default();
    tetsu.active = true;
    tetsu.move_state.world_x = 600;
    tetsu.move_state.world_z = 900;
    tetsu.battle_monster_id = Some(1);
    tetsu.tmd_binding = None;
    world.actors = vec![vahn, tetsu];

    let formation = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world).formation;
    assert_eq!(
        formation,
        Some(script::FormationBox {
            min: [0.0, -800.0],
            max: [600.0, 900.0],
        }),
        "the unbound-but-live monster is inside the framing box"
    );
    assert_ne!(
        formation,
        Some(script::FormationBox {
            min: [0.0, -800.0],
            max: [0.0, -800.0],
        })
    );
}

/// **Cross-host single-model pin, browser half.** The identical world
/// recipe as the native `native_pose_matches_the_shared_recipe`
/// (`engine-shell` `window/camera.rs` tests) - solo Vahn at the
/// tutorial seat, command menu open, one bound monster - driven through
/// THIS host's derivation + the shared drive must land on the same
/// literal measured close-up, so the two hosts' poses are equal to each
/// other by transitivity.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn web_pose_matches_the_native_recipe() {
    let mut world = World::default();
    world.mode = SceneMode::Battle;
    world.party.party_count = 1;
    let mut vahn = legaia_engine_core::world::Actor::default();
    vahn.active = true;
    vahn.move_state.world_x = 0;
    vahn.move_state.world_y = 0;
    vahn.move_state.world_z = -800;
    vahn.move_state.render_26 = 0;
    let mut tetsu = legaia_engine_core::world::Actor::default();
    tetsu.active = true;
    tetsu.move_state.world_z = 800;
    tetsu.battle_monster_id = Some(1);
    tetsu.tmd_binding = Some(0);
    world.actors = vec![vahn, tetsu];
    world.battle.command =
        Some(legaia_engine_battle_menus::battle_input::BattleCommandSession::new(0, 0));
    // The **arts input** picker is what arms retail's case-0 close-up;
    // the command chooser alone keeps the far framing (see
    // `script::phase_for_state`). Same recipe as the native mirror.
    world.battle.arts_menu = Some(legaia_engine_battle::battle_arts::BattleArtsSession::new(
        0,
        0,
        Vec::new(),
    ));

    let inputs = legaia_engine_core::battle_cam_inputs::battle_cam_inputs(&world);
    assert_eq!(inputs.phase, script::BattleCamPhase::Submenu);
    assert_eq!(
        inputs.acting,
        Some(script::BattleCamActor {
            facing: 0,
            world: [0.0, 0.0, -800.0],
            height: None,
        })
    );
    assert_eq!(
        inputs.formation,
        Some(script::FormationBox {
            min: [0.0, -800.0],
            max: [0.0, 800.0],
        })
    );
    let mut slot = None;
    for f in 0..=6u64 {
        script::drive(&mut slot, true, inputs, f * 2, None);
    }
    let pose = slot.as_ref().unwrap().pose();
    // The measured solo-Vahn submenu close-up - the SAME literals the
    // native mirror test asserts.
    assert_eq!(pose.pitch, 32.0);
    assert_eq!(pose.yaw.rem_euclid(4096.0), 2288.0);
    assert_eq!(pose.tr, [-512.0, 1152.0, 2457.0]);
    assert_eq!(pose.focus, [0.0, 0.0, -800.0]);
}
