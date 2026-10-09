//! The field camera's `World` seam.
//!
//! The camera - presets, the zone follow composer and ease, the script
//! mover, the event router, the follow knobs - lives in
//! [`legaia_engine_field::camera`] and is re-exported here at its old paths.
//! It reaches the world only through [`CameraWorld`]; this module is where
//! `World` implements it, each method a forward to the field or method the
//! camera used to read directly.

pub use legaia_engine_field::camera::*;

use crate::field_events::FieldEvent;
use crate::world::World;

impl CameraWorld for World {
    fn scene_mode(&self) -> crate::mode::SceneMode {
        self.mode
    }

    fn player_world_pos(&self) -> Option<(i32, i32, i32)> {
        let a = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))?;
        Some((
            i32::from(a.move_state.world_x),
            i32::from(a.move_state.world_y),
            i32::from(a.move_state.world_z),
        ))
    }

    fn active_actor_pos(&self, slot: usize) -> Option<(i32, i32, i32)> {
        let a = self.actors.get(slot).filter(|a| a.active)?;
        Some((
            i32::from(a.move_state.world_x),
            i32::from(a.move_state.world_y),
            i32::from(a.move_state.world_z),
        ))
    }

    fn sample_field_floor_height_static(&self, world_x: i32, world_z: i32) -> i32 {
        World::sample_field_floor_height_static(self, world_x, world_z)
    }

    fn map_region_block(&self) -> &[u8] {
        &self.terrain.map_region_block
    }

    fn zone_table(&self) -> &[u8] {
        &self.terrain.zone_table
    }

    fn collision_grid(&self) -> &[u8] {
        &self.terrain.collision_grid
    }

    fn story_flags(&self) -> u32 {
        self.flags.story_flags
    }

    fn camera_registers(&self) -> &crate::register_ramp::CameraRegisterFile {
        &self.camera.registers
    }

    fn camera_shake_amplitude(&self) -> u8 {
        self.camera.shake_amplitude
    }

    fn scene_save_allowed(&self) -> bool {
        self.party.scene_save_allowed
    }

    fn display_frames(&self) -> u64 {
        self.clock.display_frames
    }

    fn cutscene_timeline_active(&self) -> bool {
        World::cutscene_timeline_active(self)
    }

    fn script_arc_follow_camera(&self) -> bool {
        World::script_arc_follow_camera(self)
    }

    fn camera_zone_requery_per_frame(&self) -> bool {
        World::camera_zone_requery_per_frame(self)
    }

    fn field_player_movement_locked(&self) -> bool {
        World::field_player_movement_locked(self)
    }

    fn ledge_hop_active(&self) -> bool {
        self.locomotion.ledge_hop.is_some()
    }

    fn player_script_arc_live(&self) -> bool {
        World::player_script_arc_live(self)
    }

    fn take_camera_zone_requests(&mut self) -> Vec<CameraZoneRequest> {
        World::take_camera_zone_requests(self)
    }

    fn drain_field_events(&mut self) -> Vec<FieldEvent> {
        World::drain_field_events(self)
    }

    fn requeue_field_events(&mut self, events: Vec<FieldEvent>) {
        self.pending_field_events.extend(events);
    }

    fn set_npc_cull_view(&mut self, view: Option<FieldCullView>) {
        self.npcs.cull_view = view;
    }

    fn set_fog_view_window(&mut self, window: [i8; 4]) {
        self.fog.view_window = Some(window);
    }

    fn rng_state(&self) -> u32 {
        self.rng_state
    }

    fn set_rng_state(&mut self, state: u32) {
        self.rng_state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SceneMode;
    use legaia_engine_vm::Position as ActorVmPosition;

    fn world_with_actor_at(slot: u8, x: i16, z: i16) -> World {
        let mut w = World::default();
        let actor = w.spawn_actor(slot as usize);
        actor.default_pos = ActorVmPosition::new(x, 0);
        actor.move_state.world_x = x;
        actor.move_state.world_y = 0;
        actor.move_state.world_z = z;
        w
    }

    #[test]
    fn follow_mode_tracks_actor_xz() {
        let w = world_with_actor_at(0, 100, 200);
        let mut c = Camera::default();
        c.tick(&w);
        // look_at = (100, height, 200).
        assert_eq!(c.look_at, [100.0, 80.0, 200.0]);
        // eye = (100, height, 200 - distance) when yaw == 0.
        assert_eq!(c.eye, [100.0, 80.0, 200.0 - 200.0]);
    }

    #[test]
    fn follow_mode_tracks_player_after_locomotion() {
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.install_field_player(0);
        w.actors[0].move_state.world_x = 100;
        w.actors[0].move_state.world_z = 100;
        let mut c = Camera::default();
        c.follow_slot = 0;
        // Walk +Z one frame (speed 8) then advance the camera.
        w.set_pad(crate::input::PadButton::Up.mask());
        let _ = w.tick();
        c.tick(&w);
        assert_eq!(w.actors[0].move_state.world_z, 108);
        // Camera look-at Z tracks the moved player.
        assert_eq!(c.look_at[2], 108.0);
    }

    #[test]
    fn follow_mode_yaw_offsets_eye() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera::default();
        c.yaw = std::f32::consts::FRAC_PI_2;
        c.tick(&w);
        // yaw=π/2 -> sin=1, cos=0 -> eye_x = -distance, eye_z = 0.
        assert!((c.eye[0] + 200.0).abs() < 1e-3, "eye_x={}", c.eye[0]);
        assert!(c.eye[2].abs() < 1e-3, "eye_z={}", c.eye[2]);
    }

    #[test]
    fn static_mode_does_not_move_eye() {
        let mut c = Camera::default();
        c.mode = CameraMode::Static;
        c.eye = [1.0, 2.0, 3.0];
        c.look_at = [4.0, 5.0, 6.0];
        let w = world_with_actor_at(0, 99, 99);
        c.tick(&w);
        assert_eq!(c.eye, [1.0, 2.0, 3.0]);
        assert_eq!(c.look_at, [4.0, 5.0, 6.0]);
    }

    #[test]
    fn route_camera_events_consumes_camera_variants() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        // Decoded op-0x45 slots: 0 = pitch, 1 = yaw, 6/7/8 = focus.
        // 1024 (12-bit) = quarter turn = TAU/4.
        w.pending_field_events = vec![
            FieldEvent::CameraConfigure {
                params: vec![
                    CameraParam {
                        slot: 0,
                        value: 512,
                    }, // pitch 1/8 turn
                    CameraParam {
                        slot: 1,
                        value: 1024,
                    }, // yaw 1/4 turn
                    CameraParam {
                        slot: 6,
                        value: (-100i16) as u16,
                    },
                    CameraParam { slot: 7, value: 40 },
                    CameraParam {
                        slot: 8,
                        value: (-200i16) as u16,
                    },
                ],
                apply_trigger: 0,
                mode: 0,
            },
            FieldEvent::CameraApply {
                apply_trigger: 0,
                mode: 0,
            },
            FieldEvent::Bgm {
                text_id: 1,
                sub_op: 1,
            },
        ];
        let mut c = Camera::default();
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 2);
        use std::f32::consts::TAU;
        assert!((c.pitch - TAU / 8.0).abs() < 1e-3, "slot 0 -> pitch");
        assert!((c.yaw - TAU / 4.0).abs() < 1e-3, "slot 1 -> yaw");
        // Focus (6/7/8) -> look_at with X/Z negated back to world space.
        assert_eq!(c.look_at, [100.0, 40.0, 200.0]);
        // Non-camera event preserved.
        assert_eq!(w.pending_field_events.len(), 1);
        match &w.pending_field_events[0] {
            FieldEvent::Bgm { sub_op, .. } => assert_eq!(*sub_op, 1),
            other => panic!("expected Bgm, got {other:?}"),
        }
    }

    #[test]
    fn camera_configure_focus_slots_apply_per_axis() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        // A beat that supplies focus X (slot 6) and Z (slot 8) but NOT Y
        // (slot 7) - opdeene's opening beats omit slot 7 entirely. The look-at
        // must pan X/Z while keeping the prior Y, not stay frozen.
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            params: vec![
                CameraParam {
                    slot: 6,
                    value: (-100i16) as u16,
                },
                CameraParam {
                    slot: 8,
                    value: (-200i16) as u16,
                },
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        // Prior look-at (e.g. the scene-centre Y a shell falls back to).
        let mut c = Camera::default();
        c.look_at = [1.0, 55.0, 2.0];
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 1);
        assert_eq!(
            c.look_at,
            [100.0, 55.0, 200.0],
            "X/Z retarget from slots 6/8; Y kept from the prior look-at (slot 7 absent)"
        );
    }

    /// A snap beat (`apply == 0`) writes every masked slot straight into the
    /// retail globals, and an absent slot holds. The focus lands in retail's
    /// STORED convention (negated X/Z) - the trace channel compares against
    /// that word, not against a world-space point.
    #[test]
    fn snap_beat_writes_all_ten_globals_in_retail_convention() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World::default();
        let p = |slot: u8, value: i16| CameraParam {
            slot,
            value: value as u16,
        };
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            // Pitch/yaw, the full eye-space trio, focus X/Z (no slot 7), H.
            params: vec![
                p(0, 240),
                p(1, -455),
                p(3, 280),
                p(4, 5462),
                p(5, 832),
                p(6, -8568),
                p(8, -8944),
                p(9, 776),
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert_eq!(
            c.globals.angles(),
            [240, -455, 0],
            "pitch/yaw set, roll held"
        );
        assert_eq!(c.globals.tr_eye(), [280, 5462, 832], "eye-space trio");
        assert_eq!(
            c.globals.focus_stored(),
            [-8568, 0, -8944],
            "focus stored negated in X/Z; absent slot 7 holds its prior 0"
        );
        assert_eq!(c.globals.focus_world(), [8568, 0, 8944], "world focus");
        assert_eq!(c.globals.h(), 776);
        assert!(c.mover.is_none(), "a snap cancels any glide in flight");
    }

    /// Slot 2 is the roll angle, and retail authors it - `juui2`'s opening
    /// beat stages `-660` units (-58 deg) alongside pitch, yaw, the eye trio,
    /// focus X/Z and H. The controller surfaces it as [`Camera::roll`]
    /// alongside pitch and yaw (the render hosts decode the staged slot
    /// themselves, the same way they do for those two), and a camera that
    /// drops the term frames that shot upright.
    #[test]
    fn camera_configure_slot_two_sets_the_roll() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World::default();
        let p = |slot: u8, value: i16| CameraParam {
            slot,
            value: value as u16,
        };
        // The `juui2` P2[0] beat, verbatim (entry 597, pc 0x000A).
        w.pending_field_events = vec![FieldEvent::CameraConfigure {
            params: vec![
                p(0, -643),
                p(1, -1480),
                p(2, -660),
                p(3, -19),
                p(4, 521),
                p(5, 4537),
                p(6, -3522),
                p(8, -13904),
                p(9, 280),
            ],
            apply_trigger: 0,
            mode: 0,
        }];
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert_eq!(c.globals.angles(), [-643, -1480, -660], "all three angles");
        let want = -660.0 * std::f32::consts::TAU / 4096.0;
        assert!(
            (c.roll - want).abs() < 1e-4,
            "slot 2 -> Camera::roll: {} vs {want}",
            c.roll
        );
        // Free-roam clears it with the rest of the scripted pose, so a rolled
        // cutscene cannot leave the field camera tilted.
        let field = World {
            mode: crate::mode::SceneMode::Field,
            ..World::default()
        };
        c.reset_for_free_roam(&field);
        assert_eq!(c.roll, 0.0);
    }

    /// A glide beat (`apply != 0`) arms the mover instead of snapping, and the
    /// globals interpolate toward the target over the beat's duration in
    /// display frames, arriving exactly.
    #[test]
    fn glide_beat_arms_the_mover_and_arrives_exactly() {
        use legaia_engine_vm::field::CameraParam;
        let mut w = World {
            // Slot 5 (eye-back depth) from the field reset 16420 -> 17420.
            pending_field_events: vec![FieldEvent::CameraConfigure {
                params: vec![CameraParam {
                    slot: 5,
                    value: 17420,
                }],
                apply_trigger: 100,
                mode: 1, // linear
            }],
            ..World::default()
        };
        let mut c = Camera::default();
        c.route_camera_events(&mut w);
        assert!(c.mover.is_some(), "apply != 0 arms a glide, does not snap");
        assert_eq!(
            c.globals.tr_eye()[2],
            16420,
            "arming alone does not move the global"
        );

        // Advance 50 of the 100 display frames - halfway on a linear curve.
        w.clock.display_frames = 50;
        c.tick(&w);
        let mid = c.globals.tr_eye()[2];
        assert!(
            (16420..17420).contains(&mid),
            "midpoint {mid} interpolates between start and target"
        );

        // Run out the duration: exact arrival, and the one-shot mover retires.
        w.clock.display_frames = 100;
        c.tick(&w);
        assert_eq!(c.globals.tr_eye()[2], 17420, "glide arrives exactly");
        assert!(c.mover.is_none(), "the mover is one-shot");
    }

    /// Op-`0x45` LOAD carries one 18-byte camera-region record, and the
    /// router hands it to the camera-config loader (`FUN_801DBC20`): the
    /// parameter block takes the record's split, the follow camera keeps
    /// the frame (retail's arm loads and returns - no mode change).
    #[test]
    fn route_camera_load_splits_the_record_into_the_parameter_block() {
        let mut w = World::default();
        let mut payload = vec![0u8; crate::field_regions::ZONE_RECORD_STRIDE];
        payload[5] = 0x1A; // mode 1, strength 0xA
        payload[6] = 0x21;
        payload[10..12].copy_from_slice(&(-160i16).to_le_bytes());
        payload[12..14].copy_from_slice(&0x1B8i16.to_le_bytes());
        payload[16..18].copy_from_slice(&0x200i16.to_le_bytes());
        w.pending_field_events = vec![FieldEvent::CameraLoad {
            payload: payload.clone(),
        }];
        let mut c = Camera::default();
        let n = c.route_camera_events(&mut w);
        assert_eq!(n, 1);
        assert_eq!(c.zone.config.mode, 0x1A);
        assert_eq!(c.zone.config.b608, 0x21);
        assert_eq!(c.zone.config.yaw, -160);
        assert_eq!(c.zone.config.pitch, 0x1B8);
        assert_eq!(c.zone.config.h, 0x200);
        assert_eq!(
            c.zone.loaded_record.as_ref().map(|r| r.to_vec()),
            Some(payload)
        );
        assert_eq!(c.mode, CameraMode::Follow, "LOAD does not seize the camera");
        // A short payload is ignored rather than mis-split.
        w.pending_field_events = vec![FieldEvent::CameraLoad {
            payload: vec![0u8; 12],
        }];
        c.route_camera_events(&mut w);
        assert_eq!(c.zone.config.mode, 0x1A);
    }

    /// A terrain-bearing field world drives the zone camera: the globals
    /// leave the field reset for the composed shot, snap in on arrival, and
    /// the compass reports the live yaw's negation on a host that renders
    /// the follow view.
    #[test]
    fn zone_camera_composes_from_terrain_and_snaps_on_arrival() {
        use crate::field_regions::ZONE_RECORD_STRIDE;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.spawn_actor(0);
        w.player_actor_slot = Some(0);
        w.actors[0].move_state.world_x = 0x1040;
        w.actors[0].move_state.world_z = 0x2040;
        // One kind-1 record covering the whole map: mode 1 strength 0,
        // yaw -160, pitch 450, depth 0x3000, H 512.
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 1;
        rec[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
        rec[5] = 0x10;
        rec[6] = 0x10;
        rec[7] = 0x30;
        rec[8] = 0x00;
        rec[9] = 0x20;
        rec[10..12].copy_from_slice(&(-160i16).to_le_bytes());
        rec[12..14].copy_from_slice(&450i16.to_le_bytes());
        rec[14..16].copy_from_slice(&0x3000i16.to_le_bytes());
        rec[16..18].copy_from_slice(&512i16.to_le_bytes());
        let mut zone_table = vec![1u8];
        zone_table.extend_from_slice(&rec);
        w.load_field_region_tables(&[], &zone_table);

        let mut c = Camera::default();
        c.render_yaw_bias = crate::camera_view::retail_field_render_yaw_bias();
        c.reset_globals_for_scene_entry();
        w.tick();
        c.tick(&w);
        assert!(c.zone.active);
        assert_eq!(c.zone.loaded_record, Some(rec));
        assert_eq!(c.globals.angles()[0], 450, "snapped pitch");
        assert_eq!(c.globals.angles()[1] as i16, -160, "snapped yaw");
        assert_eq!(c.globals.h(), 512);
        assert_eq!(c.globals.tr_eye()[2], 0x3000);
        assert_eq!(c.zone_follow_yaw_units(), Some(-160));
        // Compass = -yaw = +160 units.
        assert_eq!(c.compass_azimuth_units(), 160);

        // Walk one step: the ease keeps the settled pose exactly.
        w.actors[0].move_state.world_x += 2;
        w.tick();
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 450);
        assert_eq!(c.globals.angles()[1] as i16, -160);

        // A scene with NO covering record composes the miss defaults.
        let mut w2 = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w2.spawn_actor(0);
        w2.player_actor_slot = Some(0);
        w2.load_field_region_tables(&[], &[0u8]);
        let mut c2 = Camera::default();
        c2.reset_globals_for_scene_entry();
        w2.tick();
        c2.tick(&w2);
        assert_eq!(c2.zone.loaded_record, None);
        assert_eq!(
            c2.zone.config,
            crate::camera_zone::CameraZoneConfig::ZONE_MISS
        );
        assert_eq!(c2.globals.angles()[0], 0x1B8);
        assert_eq!(c2.globals.angles()[1], 0);
        assert_eq!(c2.globals.h(), 0x300);
    }

    /// A mode-5 fixed shot keeps its own focus across the free-roam
    /// writeback: retail's `FUN_801DB510` pins the focus onto the player only
    /// outside mode 5 (`0x801DB724..0x801DB734` -> `0x801DB820`), eases a
    /// mode-5 focus from where the previous frame left it, and leaves it
    /// alone on a frame the player did not move.
    #[test]
    fn zone_camera_mode5_focus_eases_from_its_own_value_not_the_player() {
        use crate::field_regions::ZONE_RECORD_STRIDE;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.spawn_actor(0);
        w.player_actor_slot = Some(0);
        w.actors[0].move_state.world_x = 0x1040;
        w.actors[0].move_state.world_z = 0x2040;
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 1;
        rec[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
        rec[5] = 0x10;
        rec[16..18].copy_from_slice(&512i16.to_le_bytes());
        let mut zone_table = vec![1u8];
        zone_table.extend_from_slice(&rec);
        w.load_field_region_tables(&[], &zone_table);
        let mut c = Camera::default();
        c.reset_globals_for_scene_entry();
        w.tick();
        c.tick(&w);
        assert!(c.zone.active);

        // Turn the resident block into a fixed shot on tile (4, 6), ease
        // code 1 (`>> 5`), and snap it in: the focus sits on the anchor.
        c.zone.config.mode = 0x50;
        c.zone.config.b60b = 0x10;
        c.zone.config.anchor_x = 4;
        c.zone.config.anchor_z = 6;
        c.zone.snap_pending = true;
        w.tick();
        c.tick(&w);
        let anchor = [-(4 << 7) - 0x40, -(6 << 7) - 0x40];
        assert_eq!([c.globals.0[6], c.globals.0[8]], anchor, "snapped");

        // Standing still: the focus holds on the anchor, not the player.
        w.tick();
        c.tick(&w);
        assert_eq!([c.globals.0[6], c.globals.0[8]], anchor, "held");

        // Walking: the focus eases from the anchor toward the anchor - it
        // stays put - rather than restarting from the player's position.
        w.actors[0].move_state.world_x += 2;
        w.tick();
        c.tick(&w);
        assert_eq!([c.globals.0[6], c.globals.0[8]], anchor, "eased in place");
    }

    /// A whole-map zone record world, snapped in: the fixture for the ease's
    /// three gates.
    fn gated_zone_world() -> (World, Camera) {
        use crate::field_regions::ZONE_RECORD_STRIDE;
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        w.spawn_actor(0);
        w.player_actor_slot = Some(0);
        w.actors[0].move_state.world_x = 0x1040;
        w.actors[0].move_state.world_z = 0x2040;
        let mut rec = [0u8; ZONE_RECORD_STRIDE];
        rec[0] = 1;
        rec[1..5].copy_from_slice(&[0, 0, 0x7F, 0x7F]);
        rec[5] = 0x10;
        rec[6] = 0x10;
        rec[7] = 0x30;
        rec[9] = 0x20;
        rec[10..12].copy_from_slice(&(-160i16).to_le_bytes());
        rec[12..14].copy_from_slice(&450i16.to_le_bytes());
        rec[14..16].copy_from_slice(&0x3000i16.to_le_bytes());
        rec[16..18].copy_from_slice(&512i16.to_le_bytes());
        let mut zone_table = vec![1u8];
        zone_table.extend_from_slice(&rec);
        w.load_field_region_tables(&[], &zone_table);
        let mut c = Camera::default();
        c.reset_globals_for_scene_entry();
        w.tick();
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 450, "snapped");
        (w, c)
    }

    /// `FUN_801DB510`'s stationary test: a standing player leaves the camera
    /// where it is, unless scratchpad `0x1F800394 & 0x40000` (`2E 12`)
    /// forces the compose-and-ease (`0x801DB578..0x801DB5A4`).
    #[test]
    fn a_standing_player_eases_only_under_the_force_bit() {
        let (mut w, mut c) = gated_zone_world();
        c.zone.config.pitch = 600;
        w.tick();
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 450, "unmoved: no ease");
        w.flags.story_flags |= CAMERA_FORCE_EASE_FLAG;
        w.tick();
        c.tick(&w);
        let p = c.globals.angles()[0];
        assert!(p > 450 && p <= 600, "forced: eases toward 600, got {p}");
    }

    /// `FUN_801D1344`'s gate in front of the ease (branch at `0x801D17DC`):
    /// a movement-locked player (`+0x10 & 0x80000`) gets no compose, ease,
    /// focus pin or clamp - the camera holds where the lock found it -
    /// unless `0x1F800394 & 0x10000` lets the ease through.
    #[test]
    fn a_movement_locked_player_holds_the_camera() {
        let (mut w, mut c) = gated_zone_world();
        c.zone.config.pitch = 600;
        let focus = [c.globals.0[6], c.globals.0[8]];
        w.actors[0].move_state.flags |= 0x0008_0000;
        w.actors[0].move_state.world_x += 64;
        w.tick();
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 450, "locked: no ease");
        assert_eq!([c.globals.0[6], c.globals.0[8]], focus, "locked: no pin");
        w.flags.story_flags |= CAMERA_LOCKED_EASE_FLAG;
        w.actors[0].move_state.world_x += 64;
        w.tick();
        c.tick(&w);
        let p = c.globals.angles()[0];
        assert!(
            p > 450 && p <= 600,
            "let through: eases toward 600, got {p}"
        );
        assert_ne!([c.globals.0[6], c.globals.0[8]], focus, "let through: pins");
    }

    /// The focus is written on the ease's legs only: a standing player
    /// keeps a focus that is not on it (`0x801DB5A4` branches past the pin
    /// leg `0x801DB820`), which is what a seat landing a retail state's
    /// stale focus relies on.
    #[test]
    fn a_standing_player_keeps_a_focus_off_the_player() {
        let (mut w, mut c) = gated_zone_world();
        c.zone.seat_focus_after_snap([-0x1000, -0x2400]);
        c.zone.snap_pending = true;
        w.tick();
        c.tick(&w);
        assert_eq!([c.globals.0[6], c.globals.0[8]], [-0x1000, -0x2400]);
        w.tick();
        c.tick(&w);
        assert_eq!([c.globals.0[6], c.globals.0[8]], [-0x1000, -0x2400]);
        w.actors[0].move_state.world_x += 2;
        w.tick();
        c.tick(&w);
        assert_ne!([c.globals.0[6], c.globals.0[8]], [-0x1000, -0x2400]);
    }

    /// The hold gates (`0x801DB550` / `0x801DB564`): with the follow switch
    /// off or scratchpad `0x1F800394 & 0x400` up, a moving player gets the
    /// pin leg - the focus on the player, no compose, no ease.
    #[test]
    fn the_hold_gates_pin_the_focus_and_skip_the_ease() {
        for switch_off in [false, true] {
            let (mut w, mut c) = gated_zone_world();
            c.zone.config.pitch = 600;
            if switch_off {
                c.zone.follow_enabled = false;
            } else {
                w.flags.story_flags |= CAMERA_HOLD_FLAG;
            }
            w.actors[0].move_state.world_x += 2;
            w.tick();
            c.tick(&w);
            assert_eq!(c.globals.angles()[0], 450, "held: no ease");
            let x = i32::from(w.actors[0].move_state.world_x);
            let z = i32::from(w.actors[0].move_state.world_z);
            let clamped = crate::camera_zone::clamp_focus(
                [-x, -z],
                c.zone.config.mode_nibble(),
                c.zone.attrs.kind != 0,
                c.zone.attrs.box_bytes,
                c.zone.view_window,
                w.party.scene_save_allowed,
                [0, 0],
            );
            assert_eq!([c.globals.0[6], c.globals.0[8]], clamped, "pinned");
        }
    }

    /// Op-`0x45` APPLY hands the camera back to the follow shot
    /// (`0x801DF210`: compose, clamp, `FUN_801DE084`). A scripted shot staged
    /// before it gives way to the composed zone pose on the same frame, the
    /// camera stays in `Follow`, and a looping record re-running the APPLY
    /// every frame keeps the pose there rather than freezing the camera.
    #[test]
    fn camera_apply_snaps_back_to_the_composed_follow_shot() {
        use legaia_engine_vm::field::CameraParam;
        let (mut w, mut c) = gated_zone_world();
        let composed = c.globals;
        // A scripted CONFIGURE takes the shot: pitch 100, H 900.
        w.pending_field_events.push(FieldEvent::CameraConfigure {
            params: vec![
                CameraParam {
                    slot: 0,
                    value: 100,
                },
                CameraParam {
                    slot: 9,
                    value: 900,
                },
            ],
            apply_trigger: 0,
            mode: 0,
        });
        c.route_camera_events(&mut w);
        c.tick(&w);
        assert_eq!(c.globals.angles()[0], 100, "the scripted shot holds");
        assert_eq!(c.globals.h(), 900);
        for _ in 0..3 {
            w.pending_field_events.push(FieldEvent::CameraApply {
                apply_trigger: 0,
                mode: 0,
            });
            c.route_camera_events(&mut w);
            assert_eq!(
                c.mode,
                CameraMode::Follow,
                "APPLY is not a cinematic commit"
            );
            w.tick();
            c.tick(&w);
            assert_eq!(c.globals, composed, "APPLY snaps to the follow shot");
            assert!(c.zone.active);
        }
    }

    /// A non-zero APPLY trigger glides to the composed shot over that many
    /// frames instead of snapping (`FUN_801DE084` -> `FUN_801DD310`).
    #[test]
    fn camera_apply_with_a_trigger_glides_to_the_follow_shot() {
        use legaia_engine_vm::field::CameraParam;
        let (mut w, mut c) = gated_zone_world();
        let composed = c.globals;
        w.pending_field_events.push(FieldEvent::CameraConfigure {
            params: vec![CameraParam {
                slot: 0,
                value: 100,
            }],
            apply_trigger: 0,
            mode: 0,
        });
        c.route_camera_events(&mut w);
        w.pending_field_events.push(FieldEvent::CameraApply {
            apply_trigger: 8,
            mode: 1,
        });
        c.route_camera_events(&mut w);
        assert!(c.mover.is_some(), "a glide is armed");
        w.tick();
        c.tick(&w);
        let p = c.globals.angles()[0];
        assert!(p > 100 && p < 450, "mid-glide pitch, got {p}");
        for _ in 0..16 {
            w.tick();
            c.tick(&w);
        }
        assert_eq!(c.globals, composed, "the glide lands on the follow shot");
    }

    #[test]
    fn reset_for_free_roam_clears_leaked_cinematic_yaw() {
        // A cutscene left the camera Cinematic at a ~180deg yaw (the state that
        // inverts the field d-pad remap). Free-roam field (no active timeline)
        // must snap it back to the follow default so `field_camera_azimuth`
        // quantises to quadrant 0.
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        assert!(!w.cutscene_timeline_active(), "no timeline installed");
        let mut c = Camera::default();
        c.mode = CameraMode::Cinematic;
        c.yaw = std::f32::consts::PI;
        c.pitch = 0.5;
        c.reset_for_free_roam(&w);
        assert_eq!(c.mode, CameraMode::Follow);
        assert_eq!(c.yaw, 0.0);
        assert_eq!(c.pitch, 0.0);
    }

    #[test]
    fn reset_for_free_roam_noop_outside_field() {
        // Only free-roam field resets; other modes keep whatever the scene
        // configured (e.g. a menu / battle / world-map camera).
        for mode in [SceneMode::Menu, SceneMode::Battle, SceneMode::WorldMap] {
            let w = World {
                mode,
                ..World::default()
            };
            let mut c = Camera::default();
            c.mode = CameraMode::Cinematic;
            c.yaw = std::f32::consts::PI;
            c.reset_for_free_roam(&w);
            assert_eq!(c.mode, CameraMode::Cinematic, "mode {mode:?} untouched");
            assert_eq!(c.yaw, std::f32::consts::PI, "mode {mode:?} yaw kept");
        }
    }

    #[test]
    fn distance_preset_scales_follow_eye_only() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera::default();
        c.distance = CameraDistance::Far;
        c.tick(&w);
        // Eye pulled back by the preset scale; look-at unchanged.
        assert!((c.eye[2] + 200.0 * CameraDistance::Far.scale()).abs() < 1e-3);
        assert_eq!(c.look_at, [0.0, 80.0, 0.0]);
        // Retail preset is the identity (the historical framing).
        let mut r = Camera::default();
        r.tick(&w);
        assert_eq!(r.eye, [0.0, 80.0, -200.0]);
    }

    #[test]
    fn manual_orbit_rotates_follow_eye_and_compass_together() {
        let w = world_with_actor_at(0, 0, 0);
        let mut c = Camera::default();
        c.manual_orbit = std::f32::consts::FRAC_PI_2;
        c.tick(&w);
        // Quarter-turn orbit: eye swings to -X (same as a scripted
        // yaw = pi/2 - see `follow_mode_yaw_offsets_eye`).
        assert!((c.eye[0] + 200.0).abs() < 1e-3, "eye_x={}", c.eye[0]);
        assert!(c.eye[2].abs() < 1e-3, "eye_z={}", c.eye[2]);
        // And the compass azimuth follows: pi/2 = 1024 units, so the
        // d-pad remap quantises to quadrant 1 (screen-up walks +X).
        assert_eq!(c.compass_azimuth_units(), 1024);
    }

    #[test]
    fn reset_for_free_roam_preserves_manual_orbit_and_distance() {
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let mut c = Camera::default();
        c.mode = CameraMode::Cinematic;
        c.yaw = std::f32::consts::PI;
        c.manual_orbit = 0.5;
        c.distance = CameraDistance::Farther;
        c.reset_for_free_roam(&w);
        assert_eq!(c.yaw, 0.0, "scripted yaw resets");
        assert_eq!(c.manual_orbit, 0.5, "player orbit intent is kept");
        assert_eq!(c.distance, CameraDistance::Farther, "preset is kept");
    }

    #[test]
    fn reset_for_free_roam_preserves_tilt_and_zoom() {
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let mut c = Camera::default();
        c.mode = CameraMode::Cinematic;
        c.pitch = 0.5;
        c.manual_tilt = 0.3;
        c.manual_zoom = 1.7;
        c.reset_for_free_roam(&w);
        assert_eq!(c.pitch, 0.0, "scripted pitch resets");
        assert_eq!(c.manual_tilt, 0.3, "player tilt intent is kept");
        assert_eq!(c.manual_zoom, 1.7, "player zoom intent is kept");
    }

    /// The three knobs take a gesture only while the follow camera owns the
    /// frame: a running cutscene keeps the camera where the script put it,
    /// and the gesture is dropped rather than banked.
    #[test]
    fn follow_knobs_are_locked_while_a_cutscene_owns_the_camera() {
        let mut w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let mut c = Camera::default();
        assert!(c.follow_knobs_live(&w));
        assert!(c.orbit_by(&w, 0.25));
        assert!(c.tilt_by(&w, 0.1));
        assert!(c.zoom_by(&w, 1.5));
        assert!((c.manual_orbit - 0.25).abs() < 1e-6);
        assert!((c.manual_tilt - 0.1).abs() < 1e-6);
        assert!((c.manual_zoom - 1.5).abs() < 1e-6);

        w.cutscene.timeline = Some(crate::cutscene_timeline::CutsceneTimeline::new(vec![0], 0));
        assert!(w.cutscene_timeline_active());
        assert!(!c.follow_knobs_live(&w));
        assert!(!c.orbit_by(&w, 1.0));
        assert!(!c.tilt_by(&w, 1.0));
        assert!(!c.zoom_by(&w, 2.0));
        assert!((c.manual_orbit - 0.25).abs() < 1e-6, "orbit untouched");
        assert!((c.manual_tilt - 0.1).abs() < 1e-6, "tilt untouched");
        assert!((c.manual_zoom - 1.5).abs() < 1e-6, "zoom untouched");

        // Off the field (world map / battle / menu) the knobs are not live
        // either - those modes have cameras of their own.
        for mode in [SceneMode::Menu, SceneMode::Battle, SceneMode::WorldMap] {
            let w = World {
                mode,
                ..World::default()
            };
            assert!(!c.follow_knobs_live(&w), "mode {mode:?}");
            assert!(!c.zoom_by(&w, 2.0), "mode {mode:?}");
        }
    }

    /// Tilt and zoom clamp at the knob bounds and never accumulate beyond
    /// them; orbit wraps.
    #[test]
    fn follow_knobs_clamp_and_wrap() {
        let w = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        let mut c = Camera::default();
        for _ in 0..100 {
            c.tilt_by(&w, 0.5);
            c.zoom_by(&w, 1.5);
        }
        assert_eq!(c.manual_tilt, follow_knobs::TILT_LIMIT);
        assert_eq!(c.manual_zoom, follow_knobs::ZOOM_MAX);
        for _ in 0..100 {
            c.tilt_by(&w, -0.5);
            c.zoom_by(&w, 0.5);
        }
        assert_eq!(c.manual_tilt, -follow_knobs::TILT_LIMIT);
        assert_eq!(c.manual_zoom, follow_knobs::ZOOM_MIN);
        assert!(!c.zoom_by(&w, 0.0), "a non-positive factor is refused");
        assert!(!c.zoom_by(&w, -1.0));
        c.orbit_by(&w, -0.5);
        assert!(c.manual_orbit > 0.0 && c.manual_orbit < std::f32::consts::TAU);
        c.reset_follow_knobs();
        assert_eq!(
            (c.manual_orbit, c.manual_tilt, c.manual_zoom),
            (0.0, 0.0, 1.0)
        );
    }
}
