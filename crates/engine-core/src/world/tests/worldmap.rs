use super::*;

#[test]
fn enter_world_map_installs_controller() {
    let mut world = World::default();
    assert!(world.world_map_ctrl.is_none());
    world.enter_world_map();
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.world_map_ctrl.is_some());
    // Idempotent: re-entry keeps the existing controller + state.
    world.world_map_ctrl.as_mut().unwrap().camera_x = 42;
    world.enter_world_map();
    assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, 42);
}

#[test]
fn world_tick_drives_world_map_from_pad() {
    // A pad installed via set_pad() before tick() flows into the
    // world-map controller through World::tick's WorldMap arm. This is
    // the A1 keystone: input changes per-frame World state through the
    // tick path, not via a host-side controller.
    let mut world = World::default();
    world.enter_world_map();
    world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;

    // Frame 1: the toggle combo (0x4A held, edge includes 0x40) flips
    // the view into top-view.
    world.set_pad(0x4A);
    let _ = world.tick();
    assert!(world.world_map_ctrl.as_ref().unwrap().is_top_view());

    // Frame 2: in top-view, the left-scroll bit (0x1000) moves the
    // camera. Releasing the toggle bits first so this frame is a clean
    // scroll, not another toggle.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(0x1000);
    let _ = world.tick();
    assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, -8);
}

#[test]
fn world_map_tick_is_deterministic_across_identical_pad_streams() {
    let pad_stream = [0x4Au16, 0x0000, 0x1000, 0x0020, 0x0002];
    let drive = |stream: &[u16]| {
        let mut world = World::default();
        world.enter_world_map();
        world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;
        for &pad in stream {
            world.set_pad(pad);
            let _ = world.tick();
        }
        let c = world.world_map_ctrl.unwrap();
        (c.view_mode, c.camera_x, c.camera_z, c.azimuth, c.zoom)
    };
    assert_eq!(drive(&pad_stream), drive(&pad_stream));
}

/// With no overworld entities installed, the world-map tick is camera-only:
/// the encounter state never advances even when encounters are enabled.
#[test]
fn world_map_without_entities_never_encounters() {
    let mut world = World::default();
    world.enter_world_map();
    world.set_world_map_encounter(true, 0, 7, 64);
    // No install_world_map_entities call.
    for _ in 0..10 {
        let _ = world.tick();
    }
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.pending_world_map_encounter.is_none());
}

/// Walking the overworld player across tiles rolls the region-keyed encounter
/// (the `FUN_801D9E1C` port) and flips Field-less straight into a battle that
/// returns to the world map.
#[test]
fn world_map_region_walk_triggers_battle() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    use crate::region_encounter::{EncounterRegion, RegionEncounterTable};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    // Frame the camera at a quarter turn (azimuth 1024) so the camera-relative
    // remap maps a held Right cleanly to world +X (keeps this test's "walk +X
    // across tiles" intent readable; at the default azimuth 0 Right maps to -Z).
    if let Some(ctrl) = world.world_map_ctrl.as_mut() {
        ctrl.azimuth = 1024;
    }
    world.install_field_player(0); // player_actor_slot = 0, actor active
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);

    // Formation 5 spawns one weak monster (id 100).
    world
        .formation_table
        .insert(FormationDef::new(5, vec![FormationSlot::new(100)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Test Slug", 20, 4));
    world.set_monster_catalog(cat);

    // One region covering tiles (0,0)..(20,20), high rate, rolling formation 5.
    let mut table = RegionEncounterTable::new("test");
    table.regions.push(EncounterRegion {
        tile_x_min: 0,
        tile_z_min: 0,
        tile_x_max: 20,
        tile_z_max: 20,
        rate_increment: 255,
        formation_base: 5,
        formation_count: 1,
    });
    world.set_world_map_regions(table);

    // Hold Right; the player walks +X, crossing 128-unit tiles. Each crossing
    // rolls the region; the high rate triggers within a couple of tiles.
    world.set_pad(input::PadButton::Right.mask());
    let mut entered_battle = false;
    for _ in 0..200 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            entered_battle = true;
            break;
        }
    }
    assert!(
        entered_battle,
        "walking the overworld triggers a region encounter"
    );
    assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
}

/// The overworld player is bounded by the scene's walkability grid, exactly
/// like the field: the retail world-map-walk overlay's locomotion is the same
/// `FUN_801d01b0` + `FUN_801cfe4c` against the same `_DAT_1f8003ec + 0x4000`
/// grid. With every tile walled the player cannot move in any direction.
#[test]
fn world_map_locomotion_blocked_by_full_wall_grid() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Wall every tile (sub=1 sets all four sub-cell bits across the grid).
    world.paint_field_collision(1, (0, 0x80), (0, 0x80), 0);
    world.actors[0].move_state.world_x = 400;
    world.actors[0].move_state.world_z = 400;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_x, 400,
        "walled in: no X move"
    );
    assert_eq!(
        world.actors[0].move_state.world_z, 400,
        "walled in: no Z move"
    );
}

/// With no walls, the overworld player walks freely. At the default walk-mode
/// camera azimuth (`0`) the camera sits on `+X` looking `-X`, so "screen up"
/// (away from the camera) walks the player `-X` - the camera-relative remap,
/// not a raw `+Z`.
#[test]
fn world_map_locomotion_walks_when_clear() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.reset_field_collision_grid(); // present but all-walkable
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 250;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    // speed 8 -> four 2-unit steps, all clear: at azimuth 0 the camera sits on
    // +X looking back, so "screen up" walks -X; x: 200 -> 192, z unchanged.
    assert_eq!(world.actors[0].move_state.world_x, 192);
    assert_eq!(world.actors[0].move_state.world_z, 250);
}

/// The camera-relative remap rotates the held d-pad through the overworld
/// camera azimuth. Spot-check the cardinal framings against the
/// `world_map_camera_mvp` geometry (eye at `center + (d·cosθ, _, d·sinθ)`):
/// at azimuth 0 the camera is on `+X`, so "screen up" walks `-X`; a 3/4-turn
/// azimuth puts it on `-Z`, so "screen up" walks `+Z`.
#[test]
fn world_map_camera_relative_bits_rotates_with_azimuth() {
    use crate::world::world_map_camera_relative_bits;
    // Expectations are the camera-verified screen axes (screen-up -> world
    // (-cosθ, -sinθ), screen-right -> world (sinθ, -cosθ)); the engine-shell
    // projection test confirms these move the right way on screen.
    // No input -> no bits.
    assert_eq!(world_map_camera_relative_bits(0, 0, 0), 0);
    // Azimuth 0: camera on +X, so Up (screen up) -> X- (0x8000), Right -> Z- (0x4000).
    assert_eq!(world_map_camera_relative_bits(0, 0, 1), 0x8000);
    assert_eq!(world_map_camera_relative_bits(0, 1, 0), 0x4000);
    // Azimuth 1024 (quarter turn): Up -> Z- (0x4000).
    assert_eq!(world_map_camera_relative_bits(1024, 0, 1), 0x4000);
    // Azimuth 2048 (half turn): Up -> X+ (0x2000).
    assert_eq!(world_map_camera_relative_bits(2048, 0, 1), 0x2000);
    // Azimuth 3072 (3/4 turn): Up -> Z+ (0x1000), Right -> X- (0x8000).
    assert_eq!(world_map_camera_relative_bits(3072, 0, 1), 0x1000);
    assert_eq!(world_map_camera_relative_bits(3072, 1, 0), 0x8000);
    // A diagonal framing (1/8 turn) maps a single screen press to two world
    // axes (the player walks diagonally).
    let diag = world_map_camera_relative_bits(512, 0, 1);
    assert_eq!(
        diag.count_ones(),
        2,
        "rotated framing -> diagonal world move"
    );
}

/// A camera-only world map (no entities, no region tracker) never encounters,
/// even while the player walks.
#[test]
fn world_map_without_regions_or_entities_never_encounters() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..500 {
        let _ = world.tick();
    }
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.pending_world_map_encounter.is_none());
}

/// An installed overworld entity whose shared countdown reaches zero (with
/// encounters enabled) fires an encounter that resolves into a battle, and
/// the battle is tagged to return to the overworld - not the field.
#[test]
fn world_map_encounter_flips_to_battle_returning_to_world_map() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    // A capable lone party member.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // Formation 7 spawns one weak monster (id 100); register its stats.
    world
        .formation_table
        .insert(FormationDef::new(7, vec![FormationSlot::new(100)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Test Slug", 20, 4));
    world.set_monster_catalog(cat);
    // One entity; encounters enabled with the countdown already at zero so
    // the first Idle step fires immediately.
    world.install_world_map_entities(1);
    world.set_world_map_encounter(true, 0, 7, 64);

    // Tick once: the entity SM fires the encounter and the world flips into
    // battle, tagged to return to the overworld.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
    assert!(world.field_return.is_some());

    // Drive the fight to completion; it must return to the world map, not
    // the field.
    let mut returned = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            returned = true;
            break;
        }
    }
    assert!(returned, "the overworld battle must resolve");
    assert_eq!(
        world.mode,
        SceneMode::WorldMap,
        "an overworld encounter returns to the world map"
    );
}

/// A stationary player next to an idle overworld entity triggers an
/// interaction (surfaced as a `FieldInteract` event), and a moving player
/// does not.
#[test]
fn world_map_idle_entity_interacts_only_when_player_stationary() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities(1);
    // Encounters disabled so only the interaction path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);

    // Player moving (d-pad direction held): no interaction.
    world.set_pad(crate::input::PadButton::Up.mask());
    let _ = world.tick();
    assert!(
        !world
            .pending_field_events
            .iter()
            .any(|e| matches!(e, FieldEvent::FieldInteract { .. })),
        "a walking player does not interact"
    );

    // Player stationary: the idle entity interacts.
    world.set_pad(0);
    let _ = world.tick();
    let interacted = world
        .drain_field_events()
        .iter()
        .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 0, .. }));
    assert!(interacted, "a stationary player interacts with the entity");
}

/// An encounter-zone entity spawns its OWN formation, not the map-wide
/// shared one.
#[test]
fn world_map_encounter_zone_uses_its_own_formation() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // Register both the zone's formation (9) and a decoy shared one (7).
    world
        .formation_table
        .insert(FormationDef::new(9, vec![FormationSlot::new(100)]));
    world
        .formation_table
        .insert(FormationDef::new(7, vec![FormationSlot::new(101)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Zone Slug", 20, 4));
    cat.insert(MonsterDef::new(101, "Decoy", 20, 4));
    world.set_monster_catalog(cat);
    // Entity 0 is an encounter zone for formation 9; shared formation is 7.
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::EncounterZone {
        formation_id: 9,
    }]);
    world.set_world_map_encounter(true, 0, 7, 64);

    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(
        world.active_formation.as_ref().map(|f| f.formation_id),
        Some(9),
        "the zone's own formation spawns, not the shared one"
    );
}

/// Engaging a portal entity surfaces a `WorldMapTransition` carrying the
/// portal's target map id.
#[test]
fn world_map_portal_engage_surfaces_target_map() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Portal {
        target_map: 5,
    }]);
    // Encounters off so only the transition path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);

    world.engage_world_map_entity(0);
    let _ = world.tick();
    let transitioned = world.drain_field_events().into_iter().any(|e| {
        matches!(
            e,
            FieldEvent::WorldMapTransition {
                target_map: 5,
                slot: 0
            }
        )
    });
    assert!(transitioned, "the portal surfaces its target map");
}

/// Walking the overworld player onto a portal entity's tile auto-engages it
/// (no host `engage_world_map_entity` call) and surfaces its target map.
#[test]
fn world_map_walking_onto_portal_auto_engages() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Encounters off so only the transition path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);
    // A portal at tile (3,3) -> world (3*128 + 64 = 448, 448).
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Portal { target_map: 9 },
        (448, 448),
    )]);

    // Player starts two tiles to the -X side, on the same row as the portal.
    world.actors[0].move_state.world_x = 448 - 256;
    world.actors[0].move_state.world_z = 448;

    // Hold the d-pad direction that walks +X at the default azimuth (0): the
    // camera sits on +X, so "screen down" walks +X toward the portal (see the
    // camera-relative remap).
    world.set_pad(input::PadButton::Down.mask());
    let mut transitioned = false;
    for _ in 0..200 {
        let _ = world.tick();
        if world.drain_field_events().into_iter().any(|e| {
            matches!(
                e,
                FieldEvent::WorldMapTransition {
                    target_map: 9,
                    slot: 0
                }
            )
        }) {
            transitioned = true;
            break;
        }
    }
    assert!(
        transitioned,
        "walking onto the portal tile auto-fires its transition"
    );
}

/// Auto-engage is portal-only: walking onto an NPC entity's tile does NOT fire
/// a transition (NPCs are talk-to, not walk-onto).
#[test]
fn world_map_walking_onto_npc_does_not_transition() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Npc {
            interact_id: 4,
            text_id: None,
            inline: Vec::new(),
        },
        (448, 448),
    )]);
    world.actors[0].move_state.world_x = 448;
    world.actors[0].move_state.world_z = 448; // standing on the NPC tile
    world.set_pad(0);
    let _ = world.tick();
    let transitioned = world
        .drain_field_events()
        .into_iter()
        .any(|e| matches!(e, FieldEvent::WorldMapTransition { .. }));
    assert!(
        !transitioned,
        "an NPC is not auto-engaged by walking onto its tile"
    );
}

/// Placed overworld entities surface as render markers: one per installed
/// position, paired with its kind, at the player's walking plane.
#[test]
fn world_map_entity_markers_pair_position_and_kind() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Put the player on a known plane so the marker `y` is deterministic.
    world.actors[0].move_state.world_y = -200;
    world.install_world_map_entities_at(vec![
        (WorldMapEntityConfig::Portal { target_map: 9 }, (448, 320)),
        (
            WorldMapEntityConfig::Npc {
                interact_id: 4,
                text_id: None,
                inline: Vec::new(),
            },
            (640, 128),
        ),
        (
            WorldMapEntityConfig::EncounterZone { formation_id: 2 },
            (-64, 512),
        ),
    ]);

    let markers = world.world_map_entity_markers();
    assert_eq!(markers.len(), 3);
    // Position x/z come straight from the placement; y is the player plane.
    assert_eq!(markers[0].world_pos, [448.0, -200.0, 320.0]);
    assert_eq!(markers[0].kind, WorldMapEntityKind::Portal);
    assert_eq!(markers[1].world_pos, [640.0, -200.0, 128.0]);
    assert_eq!(markers[1].kind, WorldMapEntityKind::Npc);
    assert_eq!(markers[2].world_pos, [-64.0, -200.0, 512.0]);
    assert_eq!(markers[2].kind, WorldMapEntityKind::EncounterZone);
}

/// The player surfaces as an overworld marker at its actor position; with no
/// player actor installed there is no marker.
#[test]
fn world_map_player_marker_tracks_player_actor() {
    let mut world = World::default();
    world.enter_world_map();
    assert!(
        world.world_map_player_marker().is_none(),
        "no player actor -> no marker"
    );
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_y = -64;
    world.actors[0].move_state.world_z = 256;
    let m = world
        .world_map_player_marker()
        .expect("player marker present");
    assert_eq!(m.world_pos, [320.0, -64.0, 256.0]);
}

/// Walking on the overworld records a heading the player marker exposes (the
/// world-map walk sets `render_26` itself, since it uses the camera-relative
/// bits rather than the field heading decoder).
#[test]
fn world_map_walking_sets_player_marker_facing() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    world.reset_field_collision_grid(); // all-walkable so the step commits
    world.actors[0].move_state.world_x = 200; // away from the -X boundary
    let start_x = world.actors[0].move_state.world_x;
    // At the default azimuth the camera sits on +X, so "screen up" walks -X
    // (dx=-1, dz=0) -> atan2(-1, 0) = -TAU/4 -> heading 3072.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    let m = world
        .world_map_player_marker()
        .expect("player marker present");
    assert_eq!(m.facing, 3072, "walking -X faces heading 3072");
    assert!(
        world.actors[0].move_state.world_x < start_x,
        "the player advanced -X (start {start_x} -> {})",
        world.actors[0].move_state.world_x
    );
}

/// Config-only installs (no disc placements) produce no markers, so a
/// camera-only or synthetic world map draws nothing.
#[test]
fn world_map_entity_markers_empty_without_positions() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
        interact_id: 1,
        text_id: None,
        inline: Vec::new(),
    }]);
    assert!(world.world_map_entity_markers().is_empty());
}

/// An NPC-config entity surfaces its configured interaction id.
#[test]
fn world_map_npc_config_surfaces_interact_id() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
        interact_id: 7,
        text_id: None,
        inline: Vec::new(),
    }]);
    world.set_world_map_encounter(false, 50, 0, 64);
    // Stationary player: the idle entity interacts.
    world.set_pad(0);
    let _ = world.tick();
    let interacted = world
        .drain_field_events()
        .into_iter()
        .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 7, .. }));
    assert!(interacted, "the NPC surfaces its configured interact id");
}

/// Talking to an adjacent NPC that carries inline dialog text opens its MES
/// message on a confirm press (sets `current_dialog` + emits `OpenDialog`); a
/// later confirm/cancel press dismisses it (emits `DialogDismissed`).
#[test]
fn world_map_npc_talk_to_opens_and_dismisses_dialogue() {
    let cross = crate::input::PadButton::Cross.mask();
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    // Inline dialog bytes in the field-VM box format: a one-byte prologue
    // then a `0x1F`-lead text segment ("Hi").
    let inline = vec![0x00u8, 0x1F, b'H', b'i', 0x00];
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Npc {
            interact_id: 4,
            text_id: Some(0x12),
            inline: inline.clone(),
        },
        (576, 448), // one tile east of the player (448 >> 7 == 3, 576 >> 7 == 4)
    )]);
    world.actors[0].move_state.world_x = 448;
    world.actors[0].move_state.world_z = 448;

    // Settle a frame with no input so the next Cross press is a clean edge.
    world.set_pad(0);
    let _ = world.tick();
    assert!(world.current_dialog.is_none(), "no box before talking");

    // Confirm press next to the NPC opens its dialogue, carrying the inline
    // text through (the host renders it via `OwnedDialogPanel::from_inline_dialog`).
    world.set_pad(cross);
    let _ = world.tick();
    assert_eq!(
        world.current_dialog.as_ref().map(|d| d.inline.clone()),
        Some(inline.clone()),
        "talk-to opens the NPC's inline dialogue text"
    );
    assert!(
        world
            .drain_field_events()
            .into_iter()
            .any(|e| matches!(e, FieldEvent::OpenDialog { ref inline, .. } if !inline.is_empty())),
        "talk-to emits OpenDialog carrying the inline text for the host to render"
    );

    // Cross held across the frame boundary is not a fresh edge (edges advance
    // on `set_pad`): the box stays up.
    world.set_pad(cross);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "no dismiss without a new edge"
    );

    // Release then press again to dismiss.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(cross);
    let _ = world.tick();
    assert!(world.current_dialog.is_none(), "confirm dismisses the box");
    assert!(
        world
            .drain_field_events()
            .into_iter()
            .any(|e| matches!(e, FieldEvent::DialogDismissed)),
        "dismiss emits DialogDismissed"
    );
}
