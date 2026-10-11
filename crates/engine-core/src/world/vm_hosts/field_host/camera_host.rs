//! `FieldCameraHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldCameraHost for FieldHostImpl<'a> {
    fn op4c_n_e_sub_3_actor_sync_camera(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        Self::op4c_n_e_sub_3_actor_sync_camera(self, ctx, actor_id)
    }

    // Op-0x43 sub-3..6 camera-register zone-ramp spawn (retail
    // `FUN_8003C6A4` actor on the effect list). The record's
    // parameterization is the ported kernel; the world holds the spawned
    // records ([`legaia_engine_field::camera_rig::CameraRig::register_ramps`]) and ticks each one's
    // `FUN_80037018` handler per frame ([`World::tick_register_ramps`]).
    // REF: FUN_8003C6A4 (kernel PORT lives in legaia_engine_field::register_ramp)
    fn op43_camera_register_ramp(&mut self, sub_op: u8, zone: [u8; 4], start: i16, end: i16) {
        if let Some(ramp) =
            legaia_engine_field::register_ramp::spawn_register_ramp(sub_op, zone, start, end)
        {
            self.world.camera.register_ramps.push(ramp);
        }
    }

    fn view_window_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::ViewWindowLong { b1, b2, b3, b4 });
    }

    fn view_window_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::ViewWindowShort { r, g, b, packed });
    }

    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        Self::camera_configure(self, params, apply_trigger, mode)
    }

    fn camera_load(&mut self, payload: &[u8]) {
        self.world.camera.state.loaded_payload = payload.to_vec();
        self.world
            .pending_field_events
            .push(FieldEvent::CameraLoad {
                payload: payload.to_vec(),
            });
    }

    fn camera_save(&mut self) {
        // Snapshot what we have currently - engines that model real camera
        // matrices can override this on a custom host wrapper. For now we
        // write a placeholder so save/load round-trip behaves.
        self.world.camera.state.saved = self.world.camera.state.loaded_payload.clone();
        self.world.pending_field_events.push(FieldEvent::CameraSave);
    }

    fn camera_apply(&mut self, apply_trigger: i16, mode: u8) {
        // APPLY composes the follow pose and glides to it over the trigger's
        // frames, or snaps (killing the mover) on a zero trigger.
        self.world.camera.state.glide_frames = i32::from(apply_trigger.max(0));
        self.world
            .pending_field_events
            .push(FieldEvent::CameraApply {
                apply_trigger,
                mode,
            });
    }

    // The five camera-zone arms of op `0x4C` outer-nibble 3 / C. They queue
    // on the world because the camera globals live on the host-owned
    // `Camera`; `Camera::tick` drains the queue for both hosts. See
    // `crate::world::camera_hooks`.
    fn camera_zone_query_at_player(&mut self) {
        self.world
            .push_camera_zone_request(CameraZoneRequest::QueryAtPlayer);
    }

    fn camera_zone_query_at_tile(&mut self, x: u8, z: u8) {
        self.world
            .push_camera_zone_request(CameraZoneRequest::QueryAtTile { x, z });
    }

    fn camera_zone_query_conform_and_snap(&mut self, _ctx: &mut FieldCtx) {
        // The arm's middle call, `FUN_80019278(player)` -> `player[+0x16]`:
        // re-conform the footing to the floor under the tile the script just
        // moved the player to, before the camera composes from it. The query
        // and the snap are the queued half.
        if let Some(slot) = self.world.player_actor_slot
            && let Some(a) = self.world.actors.get(slot as usize)
        {
            let (x, z) = (
                i32::from(a.move_state.world_x),
                i32::from(a.move_state.world_z),
            );
            let y = self.world.sample_field_floor_height(x, z) as i16;
            if let Some(a) = self.world.actors.get_mut(slot as usize) {
                a.move_state.world_y = y;
            }
        }
        self.world
            .push_camera_zone_request(CameraZoneRequest::QueryConformAndSnap);
    }

    fn camera_snap_and_clamp(&mut self) {
        self.world
            .push_camera_zone_request(CameraZoneRequest::SnapAndClamp);
    }
}
