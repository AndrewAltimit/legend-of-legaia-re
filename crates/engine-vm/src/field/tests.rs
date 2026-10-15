//! Unit tests for the field VM.

use super::*;

/// One observed `spawn_attached_light` invocation.
#[derive(Debug, Default)]
struct Op34Sub1Call {
    op0: u8,
    ext: Option<u8>,
    packed24: u32,
    /// `[half_w, half_h, height]`.
    extent_and_height: [i16; 3],
    script: Option<Vec<u8>>,
}

/// Recording host: tracks every host-state interaction so tests can
/// assert exact outcomes. Mirrors the actor-VM tests' approach.
#[derive(Default)]
struct TestHost {
    globals: u32,
    extras: u32,
    held_pad: u32,
    pad_compass_entry: Vec<Option<u32>>,
    frame_delta: u16,
    cam_cfg_table: Vec<u16>,                    // index = op0 & 0x0F
    exec_moves: Vec<(u16, u8)>,                 // (script_id, move_id)
    move_tos: Vec<(u16, u16, u16, bool)>,       // (script_id, world_x, world_z, is_player)
    dialogs: Vec<(u16, Vec<u8>, u16, u16, u8)>, // (text_id, inline, world_x, world_z, depth_id)
    bgm_calls: Vec<(u16, u8)>,                  // (text_id, sub_op)
    give_item_calls: Vec<u8>,                   // GIVE_ITEM item_id list
    money_deltas: Vec<i32>,
    equip_writes: Vec<(u8, u8)>, // (selector, item)
    party_added: Vec<u8>,
    party_removed: Vec<u8>,
    scripted_battles: Vec<(u8, u8)>, // op 0x3E `op0 < 100` / `0xFF` (op0, row)
    scene_transitions: Vec<u8>,
    minigame_door_warps: Vec<u8>, // op 0x3E `op0 >= 100` sub-ids
    named_scene_transitions: Vec<(String, u8, u8, u8)>, // (scene, entry_x, entry_z, dir)
    render_long: Vec<(u8, u8, u8, u8)>,
    render_short: Vec<(u8, u8, u8, u8)>,
    scene_regs: Vec<(u8, u8, u8)>,
    counter_calls: Vec<u8>,
    animations: Vec<(u16, u8, u8, Vec<u8>)>,
    party_leaders: Vec<u8>,
    camera_configs: Vec<(Vec<CameraParam>, u16, u8)>,
    camera_loads: Vec<Vec<u8>>,
    camera_saves: u32,
    camera_apply_args: Vec<(i16, u8)>,
    camera_applies: u32,
    scene_fade_calls: Vec<(u16, u16)>,
    scene_fade_busy: bool,
    // 0x4E inventory comparison: pairs[(page, sub_op)] -> (state, factor)
    inventory_pairs: std::collections::HashMap<(u8, u8), (i32, i32)>,
    // 0x49 STATE_RESUME: tristate (Idle/Armed/Done) + last arm record.
    op49_state_value: Op49State,
    /// `(sub_op, pc, field_90)` per arm - the sub-op is retail's park
    /// kind byte (`*_DAT_8007B450`), not a spare argument.
    op49_arms: Vec<(u8, usize, u32)>,
    op49_clears: u32,
    op49_setups: u32,
    // 0x34 sub-3 / 0x43 sub-8 callbacks.
    effect_anim_calls: Vec<(u16, u8)>, // (script_id, arg)
    face_resets: Vec<u16>,             // script_ids
    // 0x4E sub-10/11 party-bank value table.
    party_bank: std::collections::HashMap<u8, i32>,
    // 0x4C sub-1 menu sub-dispatcher recordings.
    menu_sub1_calls: Vec<(u8, [u8; 5])>,
    clone_actor_calls: Vec<(u8, u32, i16)>,
    // 0x4C sub-3 cleanups.
    menu_refresh_calls: u32,
    arrival_facing_calls: u32,
    subtile_refresh_calls: Vec<u8>,
    // 0x4C sub-3 sub-7 (player-coord copy).
    player_coords: Option<PlayerCoords>,
    inverted_y_writes: Vec<i16>,
    // 0x4C sub-3 sub-2 (party-state region clear).
    party_state_clears: u32,
    // 0x43 sub-7 / sub-12 + 0x4C sub-2.
    face_rotation_setups: Vec<(u8, u32, [u16; 4], i16)>,
    scripted_actor_allocs: Vec<(u8, u8, u8)>,
    party_view_swaps: Vec<u8>,
    // 0x43 sub-2/D/F/E.
    three_actor_talks: Vec<([u8; 3], u16, u8)>,
    actor_alloc_modes: Vec<(u8, u8, [u8; 4])>,
    mark_flag_8_calls: Vec<u16>,
    // 0x43 sub-3/4/5/6.
    camera_ramp_calls: Vec<(u8, [u8; 4], i16, i16)>,
    // 0x43 sub-9 / sub-0x10 / sub-0x11 / sub-0x15.
    sub9_tweens: Vec<(u16, u16, u16, u16)>,
    // High-byte default-route opcodes 0x5x/0x6x/0x7x.
    // The in-game "system flag bank" at DAT_80085758. Mirrors the
    // original's `idx >> 3` byte / `0x80 >> (idx & 7)` bit. Sized at
    // 8 KB so 16-bit indices never overflow the test slice.
    system_flags: Vec<u8>,
    sys_flag_writes: Vec<(u16, &'static str)>, // (idx, "set"|"clear")
    emitter_init_payloads: Vec<Vec<u8>>,
    emitter_5_words: Vec<[u16; 5]>,
    emitter_struct12_payloads: Vec<Vec<u8>>,
    // 0x43 sub-0x12/0x13/0x14.
    emitter_split_calls: Vec<Vec<crate::vram_rect_copy::RectCopyCall>>,
    emitter_func13_payloads: Vec<[u8; 13]>,
    emitter_4_words: Vec<[i16; 4]>,
    // 0x4C sub-3 sub-9 (camera query+conform+snap) / sub-E (snap+clamp) / sub-F.
    player_pos_refresh_calls: u32,
    player_render_resync_calls: u32,
    field_io_resync_calls: u32,
    // 0x4C sub-3 sub-0 / sub-1 (ambient-particle master gate).
    ambient_particle_gate_writes: Vec<bool>,
    // 0x4C outer-nibble-4 (ctx-slot ramp + global write).
    n4_ctx_ramps: Vec<(u8, i16, u16)>,
    n4_global_writes: Vec<(u8, i32, u16)>,
    // 0x4C outer-nibble-4 sub-6 / sub-7 paired-global gate.
    n4_global_pair_gated: bool,
    n4_global_pair_clears: u32,
    // 0x34 sub-2 capture-and-yield.
    op34_sub2_capture_match: bool, // gates whether the lookup hits
    op34_sub2_captures: Vec<(u8, usize)>, // (b1, captured_pc_offset)
    scripted_encounter_armed: bool, // gates the bare arm-encounter forward
    scripted_encounter_windows: Vec<Vec<u8>>, // record windows from 0x37/0x41
    // 0x34 sub-0 / sub-1.
    op34_sub0_calls: Vec<(u8, [u8; 3], i16)>, // (op0, rgb, intensity)
    op34_sub1_calls: Vec<Op34Sub1Call>,
    op34_sub1_spawns: bool, // report a spawn (consumes a following 0x40 block)
    // 0x4C outer-nibble-4 sub-5 (actor-field block).
    n4_sub5_immediate: Vec<(u8, i16, i16, i16)>, // (b1, w94, w96, w98)
    n4_sub5_ramp: Vec<(u8, i16, i16, i16, u16)>, // (b1, w94, w96, w98, ticks)
    // 0x4C outer-nibble-4 sub-9 (story-flag-driven dispatch).
    n4_sub9_state: Sub9State,
    n4_sub9_default_writes: Vec<i16>,
    n4_sub9_default_ramps: Vec<(i16, u16)>,
    n4_sub9_delta_calls: Vec<(i16, u16)>,
    // 0x4C outer-nibbles 5..F.
    n5_sub0_calls: Vec<(i16, bool)>, // (value, high)
    n6_sub0_calls: Vec<[i16; 6]>,
    #[allow(clippy::type_complexity)]
    n7_tile_calls: Vec<(u8, (u8, u8), (u8, u8), u8)>,
    n8_restored_slots: Vec<u8>,
    n8_b630_writes: Vec<u8>,
    n8_callback_regs: u32,
    n8_global_writes: Vec<(i16, u8, u8)>,
    n8_quad_writes: Vec<([i16; 3], u32)>,
    n8_sub9_writes: Vec<i16>,
    /// (col_start, row_start, col_end, row_end, value).
    n8_rect_tile_fills: Vec<(u8, u8, u8, u8, u8)>,
    n8_halt_acquires: Vec<u32>,
    n9_dde34_calls: Vec<(u8, u8, [i16; 3])>,
    n9_table_copies: Vec<[i16; 16]>,
    n9_callback_regs: u32,
    op4e_sub4_bios_rand_value: i32,
    op4e_sub4_bios_rand_calls: u32,
    op4e_char_levels: std::collections::HashMap<u8, i32>,
    slot_table_values: std::collections::HashMap<u8, i16>,
    n_c_subtile_broadcasts: Vec<(u8, u8)>,
    n_c_sound_triggers: Vec<(u8, u8)>,
    n_c_slot_writes: Vec<(u8, i16)>,
    n_c_slot_adjusts: Vec<(u8, i16, bool)>,
    n_c_b6ac_writes: Vec<u8>,
    n_c_sub_f_broadcasts: Vec<(i16, i16)>,
    n_c_sub9_globals_differ: bool,
    n_d_sub6_acks: u32,
    n_d_sub8_calls: Vec<(u8, [i16; 3])>,
    n_e_sub0_writes: Vec<u8>,
    n_e_sub9_clears: u32,
    n_e_sub_a_calls: u32,
    n_d_party_setups: Vec<(u32, u32, u32)>,
    n_d_collision_y_calls: u32,
    n_d_scene_byte_writes: Vec<u8>,
    n_e_fmv_triggers: Vec<i16>,
    n_e_d8280_calls: Vec<[i16; 3]>,
    n_e_capture_ddf48_calls: u32,
    n_e_ba66_writes: Vec<u8>,
    n_e_snapshot_84570_calls: u32,
    // Overlay-helper hooks.
    n_c_sub_1_flag_loops: u32,
    n_d_sub_2_channel_calls: Vec<u8>,
    n_d_sub_7_list_walk_regs: u32,
    n_d_sub_b_e57f0_calls: Vec<Vec<u8>>,
    n_d_sub_c_hit: bool,
    n_d_sub_c_calls: std::cell::RefCell<Vec<u8>>,
    n_d_sub_e_hit: bool,
    n_d_sub_e_calls: std::cell::RefCell<Vec<u8>>,
    n_e_sub_1_text_calls: Vec<(Vec<u8>, u16)>,
    // Five 0x4C nC sub-ops + two 0x4C nE sub-ops.
    n_c_sub_0_phase_draws: u32,
    n_c_sub_0_active: bool,
    n_c_sub_3_teleports: u32,
    n_c_sub_d_queries: u32,
    n_c_sub_d_mover_idle: bool,
    n_e_sub_4_outside: bool,
    n_e_sub_4_bboxes: std::cell::RefCell<Vec<[i16; 4]>>,
    n_e_sub_5_coin_deltas: Vec<i32>,
    n_e_sub_b_resolves: bool,
    n_e_sub_b_actor_ids: Vec<u8>,
    // 0x43 halt-acquire (sub-0/1/A/B).
    halt_acquire_predicate: bool,
    arc_target_halted: bool,
    halt_acquire_calls: Vec<(u8, usize, [i16; 3])>,
    // 0x4C n8 actor-allocator + nE camera + nD/n5 dialog.
    n_8_sub_1_set_model_calls: Vec<(u32, u16, u16)>, // (model_id, anim, tween)
    n8_sub6_reflection_installs: Vec<(u8, [i16; 6])>,
    n8_sub6_source_present: bool,
    n_8_sub_b_present_types: std::collections::HashSet<u8>,
    n_8_sub_d_known: bool,
    n_8_sub_d_queries: std::cell::RefCell<Vec<(u8, u8)>>,
    n_e_sub_3_camera_syncs: Vec<u8>,            // actor_id values
    n_e_sub_7_camera_animates: Vec<(u32, u16)>, // (target, duration)
    n_e_sub_8_camera_zooms: Vec<(i16, i16, i16, i16)>, // (zoom_x, zoom_y, zoom_z, mode)
    n_d_sub_0_prompts: Vec<(u16, u16)>,
    n_d_sub_0_pressed: bool,
    n_5_sub_3_dialog_waits: u32,
    n_5_sub_4_dialog_active: bool,
    n_5_sub_4_polls: u32,
    // 0x4C nD sub-4/5 VRAM STP set/clear.
    n_d_sub_4_vram_stp_set_calls: Vec<(u16, u16)>,
    n_d_sub_5_vram_stp_clear_calls: Vec<(u16, u16)>,
    // STATE_RESUME-entangled 0x4C n5/n6/n8 sub-cases.
    #[allow(clippy::type_complexity)]
    n_5_sub_1_npc_runs: Vec<(u16, u16, u8, u8, bool)>, // (x, z, depth, move_id, is_player)
    n_5_sub_2_take_items: std::cell::RefCell<Vec<u8>>,
    n_6_sub_61_emitter_calls: Vec<[u8; 14]>,
    n_8_sub_0_allocator_calls: Vec<(u8, Vec<u8>)>,
}

impl FieldFlagsHost for TestHost {
    fn global_flags(&self) -> u32 {
        self.globals
    }

    fn set_global_flags(&mut self, value: u32) {
        self.globals = value;
    }

    fn frame_delta(&self) -> u16 {
        self.frame_delta
    }

    fn extra_flags(&self) -> u32 {
        self.extras
    }

    fn held_pad(&self) -> u32 {
        self.held_pad
    }

    fn pad_compass_entry(&self, index: u8) -> Option<u32> {
        self.pad_compass_entry
            .get(index as usize)
            .copied()
            .flatten()
    }

    fn system_flag_set(&mut self, idx: u16) {
        self.ensure_sys_flag_capacity();
        let byte = (idx >> 3) as usize;
        let mask = 0x80u8 >> (idx & 7);
        self.system_flags[byte] |= mask;
        self.sys_flag_writes.push((idx, "set"));
    }

    fn system_flag_clear(&mut self, idx: u16) {
        self.ensure_sys_flag_capacity();
        let byte = (idx >> 3) as usize;
        let mask = 0x80u8 >> (idx & 7);
        self.system_flags[byte] &= !mask;
        self.sys_flag_writes.push((idx, "clear"));
    }

    fn system_flag_test(&self, idx: u16) -> bool {
        let byte = (idx >> 3) as usize;
        let mask = 0x80u8 >> (idx & 7);
        self.system_flags
            .get(byte)
            .map(|b| (*b & mask) != 0)
            .unwrap_or(false)
    }

    fn bios_rand(&mut self) -> i32 {
        self.op4e_sub4_bios_rand_calls += 1;
        self.op4e_sub4_bios_rand_value
    }

    fn slot_table_read(&self, slot: u8) -> i16 {
        self.slot_table_values.get(&slot).copied().unwrap_or(0)
    }

    fn slot_table_write(&mut self, slot: u8, value: i16) {
        self.n_c_slot_writes.push((slot, value));
    }

    fn slot_table_adjust(&mut self, slot: u8, delta: i16, subtract: bool) {
        self.n_c_slot_adjusts.push((slot, delta, subtract));
    }

    fn schedule_timed_flags(&mut self, ab: u32, cd: u32, ef: u32) {
        self.n_d_party_setups.push((ab, cd, ef));
    }

    fn snapshot_play_clock(&mut self) {
        self.n_e_snapshot_84570_calls += 1;
    }
}

impl FieldSceneHost for TestHost {
    fn scripted_battle(&mut self, op0: u8, row: u8) {
        self.scripted_battles.push((op0, row));
    }

    fn scene_transition(&mut self, map_id: u8) {
        self.scene_transitions.push(map_id);
    }

    fn minigame_door_warp(&mut self, sub_id: u8) {
        self.minigame_door_warps.push(sub_id);
    }

    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        self.named_scene_transitions
            .push((scene.to_string(), entry_x, entry_z, dir));
    }

    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.scene_regs.push((slot_10, slot_12, slot_14));
    }

    fn is_scripted_encounter_armed(&self) -> bool {
        self.scripted_encounter_armed
    }

    fn install_scripted_encounter(&mut self, window: &[u8]) {
        self.scripted_encounter_windows.push(window.to_vec());
    }

    fn set_door_of_light_destination(&mut self, value: i16, b3: u8, b4: u8) {
        self.n8_global_writes.push((value, b3, b4));
    }

    fn op4c_nd_subf_scene_ctrl_61_write(&mut self, value: u8) {
        self.n_d_scene_byte_writes.push(value);
    }

    fn trigger_fmv(&mut self, fmv_id: i16) {
        self.n_e_fmv_triggers.push(fmv_id);
    }

    fn reroll_encounter_step_counter(&mut self) {
        self.n_e_capture_ddf48_calls += 1;
    }

    fn bgm_swap_pending(&self) -> bool {
        self.n_c_sub9_globals_differ
    }

    fn reset_party_pack_load_state(&mut self) {
        self.n_e_sub9_clears += 1;
    }

    fn trigger_game_over(&mut self) {
        self.n_e_sub_a_calls += 1;
    }
}

impl FieldTerrainHost for TestHost {
    fn region_attributes_refresh_at_player(&mut self) {
        self.subtile_refresh_calls.push(0xD);
    }

    fn paint_collision_walls(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        self.n7_tile_calls.push((sub, x_range, z_range, mask));
    }

    fn rect_tile_fill(
        &mut self,
        col_start: u8,
        row_start: u8,
        col_end: u8,
        row_end: u8,
        value: u8,
    ) {
        self.n8_rect_tile_fills
            .push((col_start, row_start, col_end, row_end, value));
    }

    fn start_floor_ladder_oscillator(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        self.n9_dde34_calls.push((sub, b1, words));
    }

    fn install_floor_height_ladder(&mut self, words: [i16; 16]) {
        self.n9_table_copies.push(words);
    }

    fn retire_floor_ladder_oscillators(&mut self) {
        self.n9_callback_regs += 1;
    }

    fn refresh_collision_y(&mut self, _ctx: &mut FieldCtx) {
        self.n_d_collision_y_calls += 1;
    }

    fn reset_fog_region_enables(&mut self, _flags: &[u8]) {
        self.n_c_sub_1_flag_loops += 1;
    }

    fn outside_world_bbox(&self, _ctx: &FieldCtx, bbox: [i16; 4]) -> bool {
        self.n_e_sub_4_bboxes.borrow_mut().push(bbox);
        self.n_e_sub_4_outside
    }
}

impl FieldCameraHost for TestHost {
    fn view_window_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        self.render_long.push((b1, b2, b3, b4));
    }

    fn view_window_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        self.render_short.push((r, g, b, packed));
    }

    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        self.camera_configs
            .push((params.to_vec(), apply_trigger, mode));
    }

    fn camera_load(&mut self, payload: &[u8]) {
        self.camera_loads.push(payload.to_vec());
    }

    fn camera_save(&mut self) {
        self.camera_saves += 1;
    }

    fn camera_apply(&mut self, apply_trigger: i16, mode: u8) {
        self.camera_applies += 1;
        self.camera_apply_args.push((apply_trigger, mode));
    }

    fn camera_zone_query_at_player(&mut self) {
        self.subtile_refresh_calls.push(0x8);
    }

    fn spawn_camera_register_ramp(&mut self, sub_op: u8, zone: [u8; 4], start: i16, end: i16) {
        self.camera_ramp_calls.push((sub_op, zone, start, end));
    }

    fn camera_zone_query_conform_and_snap(&mut self, _ctx: &mut FieldCtx) {
        self.player_pos_refresh_calls += 1;
    }

    fn camera_snap_and_clamp(&mut self) {
        self.player_render_resync_calls += 1;
    }

    fn camera_zone_query_at_tile(&mut self, x: u8, z: u8) {
        self.n_c_subtile_broadcasts.push((x, z));
    }

    fn set_camera_focus_override(&mut self, x_global: i16, z_global: i16) {
        self.n_c_sub_f_broadcasts.push((x_global, z_global));
    }

    fn teleport_to_actor(&mut self, _ctx: &mut FieldCtx, actor_id: u8) {
        self.n_e_sub_3_camera_syncs.push(actor_id);
    }
}

impl FieldCameraMotionHost for TestHost {
    fn camera_offset_mode(&self) -> Sub9State {
        self.n4_sub9_state
    }

    fn write_camera_offset(&mut self, target: i16) {
        self.n4_sub9_default_writes.push(target);
    }

    fn ramp_camera_offset(&mut self, target: i16, ticks: u16) {
        self.n4_sub9_default_ramps.push((target, ticks));
    }

    fn write_camera_offset_delta(&mut self, target: i16, ticks: u16) {
        self.n4_sub9_delta_calls.push((target, ticks));
    }

    fn set_shake_amplitude(&mut self, value: u8) {
        self.n8_b630_writes.push(value);
    }

    fn retire_camera_movers(&mut self) {
        self.n_d_sub_7_list_walk_regs += 1;
    }

    fn camera_glide_live(&mut self) -> bool {
        self.n_c_sub_d_queries += 1;
        !self.n_c_sub_d_mover_idle
    }

    fn op4c_ne_sub7_ramp_73ee4(&mut self, target: u32, duration: u16) {
        self.n_e_sub_7_camera_animates.push((target, duration));
    }

    fn configure_crawl_roller(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
        self.n_e_sub_8_camera_zooms
            .push((zoom_x, zoom_y, zoom_z, mode));
    }
}

impl FieldScreenHost for TestHost {
    fn scene_fade(&mut self, op0: u16, op1: u16) -> SceneFadeResult {
        self.scene_fade_calls.push((op0, op1));
        if self.scene_fade_busy {
            SceneFadeResult::Busy
        } else {
            SceneFadeResult::Done
        }
    }

    fn set_scene_colour(&mut self, op0: u8, payload: &[u8; 5]) {
        self.menu_sub1_calls.push((op0, *payload));
    }

    fn spawn_sprite_widget(&mut self, payload: &[u8]) {
        self.emitter_init_payloads.push(payload.to_vec());
    }

    fn tween_mask_rect(&mut self, words: [u16; 5]) {
        self.emitter_5_words.push(words);
    }

    fn configure_letterbox(&mut self, payload: &[u8]) {
        self.emitter_struct12_payloads.push(payload.to_vec());
    }

    fn queue_vram_rect_copies(&mut self, calls: &[crate::vram_rect_copy::RectCopyCall]) {
        self.emitter_split_calls.push(calls.to_vec());
    }

    fn spawn_panel_widget(&mut self, payload: &[u8; 13]) {
        self.emitter_func13_payloads.push(*payload);
    }

    fn move_panel_widget(&mut self, words: [i16; 4]) {
        self.emitter_4_words.push(words);
    }

    fn set_ambient_particle_gate(&mut self, enabled: bool) {
        self.ambient_particle_gate_writes.push(enabled);
    }

    fn write_or_ramp_global_slot(&mut self, sub: u8, target: i32, ticks: u16) {
        self.n4_global_writes.push((sub, target, ticks));
    }

    fn event_vibration_off(&self) -> bool {
        self.n4_global_pair_gated
    }

    fn clear_rumble(&mut self) {
        self.n4_global_pair_clears += 1;
    }

    fn set_effect_colour(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        self.op34_sub0_calls.push((op0, rgb, intensity));
    }

    fn vram_move_image(&mut self, words: [i16; 6]) {
        self.n6_sub0_calls.push(words);
    }

    fn set_field_light(&mut self, slots: [i16; 3], packed: u32) {
        self.n8_quad_writes.push((slots, packed));
    }

    fn set_dither(&mut self, value: u8) {
        self.n_e_ba66_writes.push(value);
    }

    fn spawn_clut_blend_fade(&mut self, bytecode: &[u8]) {
        self.n_d_sub_b_e57f0_calls.push(bytecode.to_vec());
    }

    fn vram_stp_set(&mut self, x: u16, y: u16) {
        self.n_d_sub_4_vram_stp_set_calls.push((x, y));
    }

    fn vram_stp_clear(&mut self, x: u16, y: u16) {
        self.n_d_sub_5_vram_stp_clear_calls.push((x, y));
    }
}

impl FieldAudioHost for TestHost {
    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        self.bgm_calls.push((text_id, sub_op));
    }

    fn apply_item_effect(&mut self, b1: u8, b2: u8) {
        self.n_c_sound_triggers.push((b1, b2));
    }
}

impl FieldPartyInventoryHost for TestHost {
    fn give_item(&mut self, item_id: u8) {
        self.give_item_calls.push(item_id);
    }

    fn add_money(&mut self, delta: i32) {
        self.money_deltas.push(delta);
    }

    fn set_equip_slot(&mut self, selector: u8, item: u8) {
        self.equip_writes.push((selector, item));
    }

    fn party_add(&mut self, char_id: u8) -> bool {
        self.party_added.push(char_id);
        true
    }

    fn party_remove(&mut self, char_id: u8) {
        self.party_removed.push(char_id);
    }

    fn set_party_leader(&mut self, leader_id: u8) {
        self.party_leaders.push(leader_id);
    }

    fn char_hp_mp_pair(&self, page: u8, sub_op: u8) -> (i32, i32) {
        self.inventory_pairs
            .get(&(page, sub_op))
            .copied()
            .unwrap_or((0, 0))
    }

    fn party_bank_value(&self, sub_op: u8) -> i32 {
        self.party_bank.get(&sub_op).copied().unwrap_or(0)
    }

    fn rebuild_party_roster(&mut self) {
        self.menu_refresh_calls += 1;
    }

    fn clear_party_state_region(&mut self) {
        self.party_state_clears += 1;
    }

    fn set_camera_octant(&mut self, new_index: u8) {
        self.party_view_swaps.push(new_index);
    }

    fn copy_party_tmd_poses(&mut self) {
        self.field_io_resync_calls += 1;
    }

    fn restore_party_slot(&mut self, slot: u8) {
        self.n8_restored_slots.push(slot);
    }

    fn char_level(&self, page: u8) -> i32 {
        self.op4e_char_levels.get(&page).copied().unwrap_or(0)
    }

    fn op4c_nd_subc_item_held(&self, needle: u8) -> bool {
        self.n_d_sub_c_calls.borrow_mut().push(needle);
        self.n_d_sub_c_hit
    }

    fn op4c_nd_sube_item_worn(&self, needle: u8) -> bool {
        self.n_d_sub_e_calls.borrow_mut().push(needle);
        self.n_d_sub_e_hit
    }

    fn add_coins(&mut self, coin_delta: i32) {
        self.n_e_sub_5_coin_deltas.push(coin_delta);
    }

    fn op4c_n8_subb_spell_known(&self, type_byte: u8) -> bool {
        self.n_8_sub_b_present_types.contains(&type_byte)
    }

    fn op4c_n8_subd_skill_known(&self, char_idx: u8, skill: u8) -> bool {
        self.n_8_sub_d_queries.borrow_mut().push((char_idx, skill));
        self.n_8_sub_d_known
    }

    fn take_item(&mut self, item_id: u8) {
        self.n_5_sub_2_take_items.borrow_mut().push(item_id);
    }
}

impl FieldDialogMenuHost for TestHost {
    fn press_prompt(&mut self, x: u16, y: u16) -> bool {
        if !self.n_d_sub_0_pressed {
            self.n_d_sub_0_prompts.push((x, y));
        }
        self.n_d_sub_0_pressed
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        self.dialogs
            .push((text_id, inline.to_vec(), world_x, world_z, depth_id));
    }

    fn op49_state(&self) -> Op49State {
        self.op49_state_value
    }

    fn op49_arm(&mut self, sub_op: u8, pc: usize, field_90: u32) {
        self.op49_arms.push((sub_op, pc, field_90));
    }

    fn op49_clear(&mut self) {
        self.op49_clears += 1;
    }

    fn op49_invoke_setup(&mut self) {
        self.op49_setups += 1;
    }

    fn stage_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        self.three_actor_talks.push((actor_ids, arg_word, arg_byte));
    }

    fn set_dialog_auto_press(&mut self, value: i16) {
        self.n8_sub9_writes.push(value);
    }

    fn op4c_ne_sub0_state_write(&mut self, b1: u8) {
        self.n_e_sub0_writes.push(b1);
    }

    fn spawn_text_balloon(&mut self, text_buf: &[u8], script_id: u16) {
        self.n_e_sub_1_text_calls
            .push((text_buf.to_vec(), script_id));
    }

    fn op4c_n5_sub3_dialog_wait(&mut self, _ctx: &mut FieldCtx) {
        self.n_5_sub_3_dialog_waits += 1;
    }

    fn op4c_n5_sub4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        self.n_5_sub_4_polls += 1;
        self.n_5_sub_4_dialog_active
    }
}

impl FieldActorHost for TestHost {
    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        self.exec_moves.push((ctx.script_id, move_id));
    }

    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        self.move_tos
            .push((ctx.script_id, world_x, world_z, is_player));
    }

    fn compass_heading(&self, index: u8) -> Option<u16> {
        self.cam_cfg_table.get(index as usize).copied()
    }

    fn setup_animation(&mut self, ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        self.animations
            .push((ctx.script_id, count, base_id, frames.to_vec()));
    }

    fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
        self.face_resets.push(ctx.script_id);
    }

    fn actor_face_rotation_setup(
        &mut self,
        _ctx: &mut FieldCtx,
        face_id: u8,
        payload_4: u32,
        params: [u16; 4],
        target: i16,
    ) {
        self.face_rotation_setups
            .push((face_id, payload_4, params, target));
    }

    fn tween_actor_position(&mut self, _ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        self.sub9_tweens.push((x, y, z, ticks));
    }

    fn ramp_ctx_slot(&mut self, _ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        self.n4_ctx_ramps.push((sub, target, ticks));
    }

    fn write_actor_look(&mut self, _ctx: &mut FieldCtx, b1: u8, w94: i16, w96: i16, w98: i16) {
        self.n4_sub5_immediate.push((b1, w94, w96, w98));
    }

    fn ramp_actor_look(
        &mut self,
        _ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    ) {
        self.n4_sub5_ramp.push((b1, w94, w96, w98, ticks));
    }

    fn set_actor_model(&mut self, _ctx: &mut FieldCtx, value: i16, high: bool) {
        self.n5_sub0_calls.push((value, high));
    }

    fn op4c_nd_sub6_field_74_ack(&mut self) {
        self.n_d_sub6_acks += 1;
    }

    fn randomize_clip_phase(&mut self, _ctx: &mut FieldCtx) -> bool {
        self.n_c_sub_0_phase_draws += 1;
        self.n_c_sub_0_active
    }

    fn script_table_teleport(&mut self, _ctx: &mut FieldCtx) {
        self.n_c_sub_3_teleports += 1;
    }

    fn run_to_tile(
        &mut self,
        _ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        self.n_5_sub_1_npc_runs
            .push((world_x, world_z, depth_byte, move_id, is_player));
    }
}

impl FieldActorSpawnHost for TestHost {
    fn spawn_scene_record(&mut self, global_index: u8) {
        self.counter_calls.push(global_index);
    }

    fn clone_actor(&mut self, src_id: u8, tint_rgb: u32, fade_rate: i16) {
        self.clone_actor_calls.push((src_id, tint_rgb, fade_rate));
    }

    fn spawn_shutter_blackout(&mut self, b1: u8, b2: u8, b3: u8) {
        self.scripted_actor_allocs.push((b1, b2, b3));
    }

    fn op43_subdf_alloc_actor(&mut self, sub_op: u8, mode: u8, args: [u8; 4]) {
        self.actor_alloc_modes.push((sub_op, mode, args));
    }

    fn op43_sube_mark_actor_flag_8(&mut self, ctx: &mut FieldCtx) {
        self.mark_flag_8_calls.push(ctx.script_id);
    }

    fn op34_sub2_capture_pc(
        &mut self,
        _ctx: &FieldCtx,
        _ext: Option<u8>,
        block: Option<&[u8]>,
    ) -> bool {
        // The VM offers a block only when a `0x40` header follows; the
        // simulated lookup decides whether a light takes it.
        if self.op34_sub2_capture_match
            && let Some(b) = block
        {
            self.op34_sub2_captures.push((0x40, b.len()));
            true
        } else {
            false
        }
    }

    fn hsv_grade_resident_meshes(&mut self, words: [i16; 3]) {
        self.n_e_d8280_calls.push(words);
    }

    fn spawn_morph_actor(&mut self, b1: u8, words: [i16; 3]) {
        self.n_d_sub8_calls.push((b1, words));
    }

    fn spawn_placement_if_absent(&mut self, channel: u8) {
        self.n_d_sub_2_channel_calls.push(channel);
    }

    fn actor_guard(&mut self, actor_id: u8) -> Option<()> {
        self.n_e_sub_b_actor_ids.push(actor_id);
        if self.n_e_sub_b_resolves {
            Some(())
        } else {
            None
        }
    }

    fn allocate_child_actors(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        self.n_8_sub_0_allocator_calls.push((count, tail.to_vec()));
    }
}

impl FieldActorFxHost for TestHost {
    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        self.effect_anim_calls.push((ctx.script_id, arg));
    }

    fn spawn_attached_light(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        spawn: &crate::field_actor_billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        self.op34_sub1_calls.push(Op34Sub1Call {
            op0: spawn.op0,
            ext,
            packed24: spawn.packed_rgb,
            extent_and_height: [spawn.half_extent.0, spawn.half_extent.1, spawn.height],
            script: script.map(|s| s.iter().copied().take(3).collect()),
        });
        self.op34_sub1_spawns
    }

    fn retire_reflections(&mut self) {
        self.n8_callback_regs += 1;
    }

    fn set_actor_tint(
        &mut self,
        _ctx: &mut FieldCtx,
        _target: Option<u8>,
        model_id: u32,
        anim_frame: u16,
        tween_frames: u16,
    ) {
        self.n_8_sub_1_set_model_calls
            .push((model_id, anim_frame, tween_frames));
    }

    fn install_reflection(&mut self, _ctx: &mut FieldCtx, source_id: u8, words: [i16; 6]) -> bool {
        self.n8_sub6_reflection_installs.push((source_id, words));
        self.n8_sub6_source_present
    }

    fn spawn_clut_cell_fx(&mut self, _ctx: &mut FieldCtx, payload: [u8; 14]) {
        self.n_6_sub_61_emitter_calls.push(payload);
    }
}

impl FieldPlayerHost for TestHost {
    fn apply_arrival_facing(&mut self) {
        self.arrival_facing_calls += 1;
    }

    fn fetch_player_coords(&self, _ctx: &FieldCtx) -> Option<PlayerCoords> {
        self.player_coords
    }

    fn set_inverted_y_mirror(&mut self, _ctx: &mut FieldCtx, inverted_y: i16) {
        self.inverted_y_writes.push(inverted_y);
    }

    fn set_player_clip_override(&mut self, value: u8) {
        self.n_c_b6ac_writes.push(value);
    }
}

impl FieldHaltHost for TestHost {
    fn halt_acquire(&mut self, ctx: &mut FieldCtx, opcode_pc: u32) {
        ctx.saved_pc = opcode_pc;
        ctx.wait_accum = 0;
        ctx.flags |= 0x400;
        self.n8_halt_acquires.push(opcode_pc);
    }

    fn field_halt_acquire_predicate(&self, _ctx: &FieldCtx, _which: u8) -> bool {
        self.halt_acquire_predicate
    }

    fn arc_target_halted(&self, _ctx: &FieldCtx, _ext: Option<u8>) -> bool {
        self.arc_target_halted
    }

    fn field_halt_acquire_apply(
        &mut self,
        _ctx: &mut FieldCtx,
        which: u8,
        resume_pc: usize,
        coords: [i16; 3],
    ) {
        self.halt_acquire_calls.push((which, resume_pc, coords));
    }
}

impl TestHost {
    fn ensure_sys_flag_capacity(&mut self) {
        if self.system_flags.is_empty() {
            self.system_flags.resize(8192, 0);
        }
    }
}

fn run<H: FieldHost>(host: &mut H, bytecode: &[u8]) -> (FieldCtx, Vec<StepResult>) {
    let mut ctx = FieldCtx::default();
    let mut pc = 0usize;
    let mut trace = Vec::new();
    loop {
        let r = step(host, &mut ctx, bytecode, pc);
        trace.push(r.clone());
        match r {
            StepResult::Advance { next_pc } => {
                if next_pc >= bytecode.len() {
                    break;
                }
                pc = next_pc;
            }
            _ => break,
        }
    }
    (ctx, trace)
}

mod actor_camera_dialog_vram;
mod actor_ctrl_sysflag_integration;
mod camera_fade_inventory_state;
mod field_actions;
mod flow_control;
mod halt_acquire_and_overlay_helpers;
mod op4c_nibble4_and_op34;
mod op4c_nibbles_5_to_f;
