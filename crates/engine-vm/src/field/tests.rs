//! Unit tests for the field VM. Split out of `field.rs`.

use super::*;

/// One observed `op34_sub1_spawn_or_skip` invocation.
#[derive(Debug, Default)]
struct Op34Sub1Call {
    op0: u8,
    packed24: u32,
    pos: [i16; 3],
    capture_flag: u8,
    captured_payload: Vec<u8>,
}

/// Recording host: tracks every host-state interaction so tests can
/// assert exact outcomes. Mirrors the actor-VM tests' approach.
#[derive(Default)]
struct TestHost {
    globals: u32,
    extras: u32,
    screen_mode: u32,
    screen_mode_table: Vec<Option<u32>>,
    frame_delta: u16,
    cam_cfg_table: Vec<u16>,                    // index = op0 & 0x0F
    exec_moves: Vec<(u16, u8)>,                 // (script_id, move_id)
    move_tos: Vec<(u16, u16, u16, bool)>,       // (script_id, world_x, world_z, is_player)
    dialogs: Vec<(u16, Vec<u8>, u16, u16, u8)>, // (text_id, inline, world_x, world_z, depth_id)
    bgm_calls: Vec<(u16, u8)>,                  // (text_id, sub_op)
    give_item_calls: Vec<u8>,                   // GIVE_ITEM item_id list
    money_deltas: Vec<i32>,
    item_writes: Vec<(u8, u8)>, // (slot_byte, count)
    party_added: Vec<u8>,
    party_removed: Vec<u8>,
    interacts: Vec<(u8, u8)>,
    scene_transitions: Vec<u8>,
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
    camera_applies: u32,
    scene_fade_calls: Vec<(u16, u16)>,
    scene_fade_busy: bool,
    // 0x4E inventory comparison: pairs[(page, sub_op)] -> (state, factor)
    inventory_pairs: std::collections::HashMap<(u8, u8), (i32, i32)>,
    // 0x49 STATE_RESUME: tristate (Idle/Armed/Done) + last arm record.
    op49_state_value: Op49State,
    op49_arms: Vec<(usize, u32)>,
    op49_clears: u32,
    op49_setups: u32,
    // 0x34 sub-3 / 0x43 sub-8 callbacks.
    effect_anim_calls: Vec<(u16, u8)>, // (script_id, arg)
    face_resets: Vec<u16>,             // script_ids
    // 0x4E sub-10/11 party-bank value table.
    party_bank: std::collections::HashMap<u8, i32>,
    // 0x4C sub-1 menu sub-dispatcher recordings.
    menu_sub1_calls: Vec<(u8, [u8; 5])>,
    // 0x4C sub-3 cleanups.
    menu_refresh_calls: u32,
    depth_copy_calls: u32,
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
    sound_ramp_calls: Vec<(u8, [u8; 4], u16, u16)>,
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
    emitter_split_calls: Vec<([i16; 6], bool)>,
    emitter_func13_payloads: Vec<[u8; 13]>,
    emitter_4_words: Vec<[i16; 4]>,
    // 0x4C sub-3 sub-9 / sub-E / sub-F.
    player_pos_refresh_calls: u32,
    player_render_resync_calls: u32,
    field_io_resync_calls: u32,
    // 0x4C sub-3 sub-0 / sub-1 (field input lock).
    field_input_lock_writes: Vec<bool>,
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
    op34_sub1_capture_delta: Option<usize>, // override the default 13
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
    n8_party_mirrors: Vec<u8>,
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
    // Round 16 - overlay-helper hooks.
    n_c_sub_1_flag_loops: u32,
    n_d_sub_1_jump_target: Option<usize>,
    n_d_sub_2_channel_calls: Vec<u8>,
    n_d_sub_7_list_walk_regs: u32,
    n_d_sub_b_e57f0_calls: Vec<Vec<u8>>,
    n_d_sub_c_jump_target: Option<usize>,
    n_d_sub_c_calls: Vec<u8>,
    n_d_sub_e_jump_target: Option<usize>,
    n_d_sub_e_calls: Vec<u8>,
    n_e_sub_1_text_calls: Vec<(Vec<u8>, u16)>,
    // Round 17 - five new 0x4C nC sub-ops + two 0x4C nE sub-ops.
    n_c_sub_0_move_cancels: u32,
    n_c_sub_0_active: bool,
    n_c_sub_3_teleports: u32,
    n_c_sub_d_allocs: u32,
    n_c_party_flag_bits: std::collections::HashMap<u16, bool>,
    n_e_sub_4_outside: bool,
    n_e_sub_4_bboxes: std::cell::RefCell<Vec<[i16; 4]>>,
    n_e_sub_5_xp_deltas: Vec<i32>,
    n_e_sub_b_resolves: bool,
    n_e_sub_b_actor_ids: Vec<u8>,
    // 0x43 halt-acquire (sub-0/1/A/B).
    halt_acquire_predicate: bool,
    halt_acquire_calls: Vec<(u8, usize, [i16; 3])>,
    // Round 18 - 0x4C n8 actor-allocator + nE camera + nD/n5 dialog.
    n_8_sub_1_set_model_calls: Vec<(u32, u16, u16)>, // (model_id, anim, tween)
    n_8_sub_6_actor_set_rotation_calls: Vec<(u8, [i16; 3], [i16; 3])>,
    n_8_sub_6_actor_present: bool,
    n_8_sub_b_present_types: std::collections::HashSet<u8>,
    n_8_sub_d_search_result: ActorSearchResult,
    n_8_sub_d_queries: std::cell::RefCell<Vec<(u8, u8)>>,
    n_e_sub_3_camera_syncs: Vec<u8>,            // actor_id values
    n_e_sub_7_camera_animates: Vec<(u32, u16)>, // (target, duration)
    n_e_sub_8_camera_zooms: Vec<(i16, i16, i16, i16)>, // (zoom_x, zoom_y, zoom_z, mode)
    n_d_sub_0_se_triggers: Vec<(u16, u16)>,
    n_5_sub_3_dialog_waits: u32,
    n_5_sub_4_dialog_active: bool,
    n_5_sub_4_polls: u32,
    // Round 19 - 0x4C nD sub-4/5 VRAM STP set/clear.
    n_d_sub_4_vram_stp_set_calls: Vec<(u16, u16)>,
    n_d_sub_5_vram_stp_clear_calls: Vec<(u16, u16)>,
    // Round 20 - STATE_RESUME-entangled 0x4C n5/n6/n8 sub-cases.
    #[allow(clippy::type_complexity)]
    n_5_sub_1_npc_runs: Vec<(u16, u16, u8, u8, bool)>, // (x, z, depth, move_id, is_player)
    n_5_sub_2_menu_state: std::collections::HashMap<u8, bool>,
    n_5_sub_2_polls: std::cell::RefCell<Vec<u8>>,
    n_6_sub_61_emitter_calls: Vec<[u8; 14]>,
    n_8_sub_0_allocator_calls: Vec<(u8, Vec<u8>)>,
}

impl FieldHost for TestHost {
    fn global_flags(&self) -> u32 {
        self.globals
    }
    fn set_global_flags(&mut self, value: u32) {
        self.globals = value;
    }
    fn frame_delta(&self) -> u16 {
        self.frame_delta
    }
    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        self.exec_moves.push((ctx.script_id, move_id));
    }
    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        self.move_tos
            .push((ctx.script_id, world_x, world_z, is_player));
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
    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        self.bgm_calls.push((text_id, sub_op));
    }
    fn cam_cfg_lookup(&self, index: u8) -> Option<u16> {
        self.cam_cfg_table.get(index as usize).copied()
    }
    fn give_item(&mut self, item_id: u8) {
        self.give_item_calls.push(item_id);
    }
    fn add_money(&mut self, delta: i32) {
        self.money_deltas.push(delta);
    }
    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        self.item_writes.push((slot_byte, count));
    }
    fn party_add(&mut self, char_id: u8) -> bool {
        self.party_added.push(char_id);
        true
    }
    fn party_remove(&mut self, char_id: u8) {
        self.party_removed.push(char_id);
    }
    fn extra_flags(&self) -> u32 {
        self.extras
    }
    fn screen_mode(&self) -> u32 {
        self.screen_mode
    }
    fn screen_mode_table(&self, index: u8) -> Option<u32> {
        self.screen_mode_table
            .get(index as usize)
            .copied()
            .flatten()
    }
    fn field_interact(&mut self, interact_id: u8, slot: u8) {
        self.interacts.push((interact_id, slot));
    }
    fn scene_transition(&mut self, map_id: u8) {
        self.scene_transitions.push(map_id);
    }
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        self.named_scene_transitions
            .push((scene.to_string(), entry_x, entry_z, dir));
    }
    fn render_cfg_long(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) {
        self.render_long.push((b1, b2, b3, b4));
    }
    fn render_cfg_short(&mut self, r: u8, g: u8, b: u8, packed: u8) {
        self.render_short.push((r, g, b, packed));
    }
    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.scene_regs.push((slot_10, slot_12, slot_14));
    }
    fn counter_update(&mut self, op0: u8) {
        self.counter_calls.push(op0);
    }
    fn setup_animation(&mut self, ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        self.animations
            .push((ctx.script_id, count, base_id, frames.to_vec()));
    }
    fn set_party_leader(&mut self, leader_id: u8) {
        self.party_leaders.push(leader_id);
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
    fn camera_apply(&mut self) {
        self.camera_applies += 1;
    }
    fn scene_fade(&mut self, op0: u16, op1: u16) -> SceneFadeResult {
        self.scene_fade_calls.push((op0, op1));
        if self.scene_fade_busy {
            SceneFadeResult::Busy
        } else {
            SceneFadeResult::Done
        }
    }
    fn inventory_compare_pair(&self, page: u8, sub_op: u8) -> (i32, i32) {
        self.inventory_pairs
            .get(&(page, sub_op))
            .copied()
            .unwrap_or((0, 0))
    }
    fn op49_state(&self) -> Op49State {
        self.op49_state_value
    }
    fn op49_arm(&mut self, pc: usize, field_90: u32) {
        self.op49_arms.push((pc, field_90));
    }
    fn op49_clear(&mut self) {
        self.op49_clears += 1;
    }
    fn op49_invoke_setup(&mut self) {
        self.op49_setups += 1;
    }
    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        self.effect_anim_calls.push((ctx.script_id, arg));
    }
    fn actor_face_reset(&mut self, ctx: &mut FieldCtx) {
        self.face_resets.push(ctx.script_id);
    }
    fn party_bank_value(&self, sub_op: u8) -> i32 {
        self.party_bank.get(&sub_op).copied().unwrap_or(0)
    }
    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        self.menu_sub1_calls.push((op0, *payload));
    }
    fn menu_refresh(&mut self) {
        self.menu_refresh_calls += 1;
    }
    fn copy_dialog_depth_to_player(&mut self) {
        self.depth_copy_calls += 1;
    }
    fn player_subtile_refresh(&mut self, sub_op: u8) {
        self.subtile_refresh_calls.push(sub_op);
    }
    fn fetch_player_coords(&self, _ctx: &FieldCtx) -> Option<PlayerCoords> {
        self.player_coords
    }
    fn set_inverted_y_mirror(&mut self, _ctx: &mut FieldCtx, inverted_y: i16) {
        self.inverted_y_writes.push(inverted_y);
    }
    fn clear_party_state_region(&mut self) {
        self.party_state_clears += 1;
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
    fn op43_alloc_scripted_actor(&mut self, b1: u8, b2: u8, b3: u8) {
        self.scripted_actor_allocs.push((b1, b2, b3));
    }
    fn party_view_swap(&mut self, new_index: u8) {
        self.party_view_swaps.push(new_index);
    }
    fn op43_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        self.three_actor_talks.push((actor_ids, arg_word, arg_byte));
    }
    fn op43_alloc_actor_with_mode(&mut self, sub_op: u8, mode: u8, args: [u8; 4]) {
        self.actor_alloc_modes.push((sub_op, mode, args));
    }
    fn op43_mark_actor_flag_8(&mut self, ctx: &mut FieldCtx) {
        self.mark_flag_8_calls.push(ctx.script_id);
    }
    fn op43_sound_register_ramp(&mut self, sub_op: u8, bytes: [u8; 4], ticks: u16, curve: u16) {
        self.sound_ramp_calls.push((sub_op, bytes, ticks, curve));
    }
    fn op43_sub9_tween(&mut self, _ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        self.sub9_tweens.push((x, y, z, ticks));
    }
    fn op43_widget_sprite_spawn(&mut self, payload: &[u8]) {
        self.emitter_init_payloads.push(payload.to_vec());
    }
    fn op43_widget_mask_rect(&mut self, words: [u16; 5]) {
        self.emitter_5_words.push(words);
    }
    fn op43_widget_letterbox(&mut self, payload: &[u8]) {
        self.emitter_struct12_payloads.push(payload.to_vec());
    }
    fn op43_vram_rect_copy(&mut self, words: [i16; 6], did_split: bool) {
        self.emitter_split_calls.push((words, did_split));
    }
    fn op43_widget_panel_spawn(&mut self, payload: &[u8; 13]) {
        self.emitter_func13_payloads.push(*payload);
    }
    fn op43_widget_panel_move(&mut self, words: [i16; 4]) {
        self.emitter_4_words.push(words);
    }
    fn player_position_refresh_with_collision_y(&mut self, _ctx: &mut FieldCtx) {
        self.player_pos_refresh_calls += 1;
    }
    fn player_render_resync(&mut self) {
        self.player_render_resync_calls += 1;
    }
    fn field_io_resync(&mut self) {
        self.field_io_resync_calls += 1;
    }
    fn set_field_input_lock(&mut self, locked: bool) {
        self.field_input_lock_writes.push(locked);
    }
    fn op4c_nibble4_ctx_ramp(&mut self, _ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        self.n4_ctx_ramps.push((sub, target, ticks));
    }
    fn op4c_nibble4_global_write(&mut self, sub: u8, target: i32, ticks: u16) {
        self.n4_global_writes.push((sub, target, ticks));
    }
    fn op4c_nibble4_global_pair_gate(&self) -> bool {
        self.n4_global_pair_gated
    }
    fn op4c_nibble4_global_pair_clear(&mut self) {
        self.n4_global_pair_clears += 1;
    }
    fn op34_capture_pc_for_existing_actor(
        &mut self,
        _ctx: &FieldCtx,
        b1: u8,
        captured_pc_offset: usize,
    ) -> bool {
        // Match the original's gates: only capture when b1 == 0x40 AND
        // the simulated lookup hits. Hosts model the lookup themselves.
        if self.op34_sub2_capture_match && b1 == 0x40 {
            self.op34_sub2_captures.push((b1, captured_pc_offset));
            true
        } else {
            false
        }
    }
    fn is_scripted_encounter_armed(&self) -> bool {
        self.scripted_encounter_armed
    }
    fn install_scripted_encounter(&mut self, window: &[u8]) {
        self.scripted_encounter_windows.push(window.to_vec());
    }
    fn op34_sub0_color_intensity_setup(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        self.op34_sub0_calls.push((op0, rgb, intensity));
    }
    fn op34_sub1_spawn_or_skip(
        &mut self,
        _ctx: &FieldCtx,
        op0: u8,
        packed24: u32,
        pos: [i16; 3],
        capture_flag: u8,
        captured_pc_payload: &[u8],
    ) -> usize {
        self.op34_sub1_calls.push(Op34Sub1Call {
            op0,
            packed24,
            pos,
            capture_flag,
            captured_payload: captured_pc_payload.to_vec(),
        });
        self.op34_sub1_capture_delta.unwrap_or(13)
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
    fn op4c_n4_sub5_write_immediate(
        &mut self,
        _ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
    ) {
        self.n4_sub5_immediate.push((b1, w94, w96, w98));
    }
    fn op4c_n4_sub5_ramp(
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
    fn op4c_n4_sub9_state(&self) -> Sub9State {
        self.n4_sub9_state
    }
    fn op4c_n4_sub9_default_write(&mut self, target: i16) {
        self.n4_sub9_default_writes.push(target);
    }
    fn op4c_n4_sub9_default_ramp(&mut self, target: i16, ticks: u16) {
        self.n4_sub9_default_ramps.push((target, ticks));
    }
    fn op4c_n4_sub9_delta_write_or_ramp(&mut self, target: i16, ticks: u16) {
        self.n4_sub9_delta_calls.push((target, ticks));
    }
    fn op4c_n5_sub0_set_actor_model(&mut self, _ctx: &mut FieldCtx, value: i16, high: bool) {
        self.n5_sub0_calls.push((value, high));
    }
    fn op4c_n6_sub0_emitter6(&mut self, words: [i16; 6]) {
        self.n6_sub0_calls.push(words);
    }
    fn op4c_n7_tile_flag_bulk(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        self.n7_tile_calls.push((sub, x_range, z_range, mask));
    }
    fn op4c_n8_sub2_party_page_mirror(&mut self, page: u8) {
        self.n8_party_mirrors.push(page);
    }
    fn op4c_n8_sub4_set_b630(&mut self, value: u8) {
        self.n8_b630_writes.push(value);
    }
    fn op4c_n_8_sub_3_rect_tile_fill(
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
    fn op4c_n8_sub7_register_callback(&mut self) {
        self.n8_callback_regs += 1;
    }
    fn op4c_n8_sub8_write_globals(&mut self, value: i16, b3: u8, b4: u8) {
        self.n8_global_writes.push((value, b3, b4));
    }
    fn op4c_n8_sub_a_write_quad(&mut self, slots: [i16; 3], packed: u32) {
        self.n8_quad_writes.push((slots, packed));
    }
    fn op4c_n8_sub9_set_73f00(&mut self, value: i16) {
        self.n8_sub9_writes.push(value);
    }
    fn op4c_n9_sub0_2_dde34(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        self.n9_dde34_calls.push((sub, b1, words));
    }
    fn op4c_n9_sub_e_table_copy(&mut self, words: [i16; 16]) {
        self.n9_table_copies.push(words);
    }
    fn op4c_n9_sub_f_register_callback(&mut self) {
        self.n9_callback_regs += 1;
    }
    fn op4e_sub4_bios_rand(&mut self) -> i32 {
        self.op4e_sub4_bios_rand_calls += 1;
        self.op4e_sub4_bios_rand_value
    }
    fn op4c_n_c_sub4_subtile_broadcast(&mut self, x: u8, z: u8) {
        self.n_c_subtile_broadcasts.push((x, z));
    }
    fn op4c_n_c_sub7_sound_trigger(&mut self, b1: u8, b2: u8) {
        self.n_c_sound_triggers.push((b1, b2));
    }
    fn op4c_n_c_sub_a_set_slot(&mut self, slot: u8, value: i16) {
        self.n_c_slot_writes.push((slot, value));
    }
    fn op4c_n_c_sub_bc_adjust_slot(&mut self, slot: u8, delta: i16, subtract: bool) {
        self.n_c_slot_adjusts.push((slot, delta, subtract));
    }
    fn op4c_n_c_sub_e_set_b6ac(&mut self, value: u8) {
        self.n_c_b6ac_writes.push(value);
    }
    fn op4c_n_c_sub_f_position_broadcast(&mut self, x_global: i16, z_global: i16) {
        self.n_c_sub_f_broadcasts.push((x_global, z_global));
    }
    fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
        self.n_d_party_setups.push((ab, cd, ef));
    }
    fn op4c_n_d_sub_a_collision_y_refresh(&mut self, _ctx: &mut FieldCtx) {
        self.n_d_collision_y_calls += 1;
    }
    fn op4c_n_d_sub_f_scene_byte_write(&mut self, value: u8) {
        self.n_d_scene_byte_writes.push(value);
    }
    fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
        self.n_e_fmv_triggers.push(fmv_id);
    }
    fn op4c_n_e_sub6_call_d8280(&mut self, words: [i16; 3]) {
        self.n_e_d8280_calls.push(words);
    }
    fn op4c_n_e_sub_c_capture_ddf48(&mut self) {
        self.n_e_capture_ddf48_calls += 1;
    }
    fn op4c_n_e_sub_d_set_ba66(&mut self, value: u8) {
        self.n_e_ba66_writes.push(value);
    }
    fn op4c_n_e_sub_e_snapshot_84570(&mut self) {
        self.n_e_snapshot_84570_calls += 1;
    }
    fn op4c_n8_halt_acquire(&mut self, ctx: &mut FieldCtx, opcode_pc: u32) {
        ctx.saved_pc = opcode_pc;
        ctx.wait_accum = 0;
        ctx.flags |= 0x400;
        self.n8_halt_acquires.push(opcode_pc);
    }
    fn op4c_n_c_sub9_globals_differ(&self) -> bool {
        self.n_c_sub9_globals_differ
    }
    fn op4c_n_d_sub6_field74_mutate_ack(&mut self) {
        self.n_d_sub6_acks += 1;
    }
    fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
        self.n_d_sub8_calls.push((b1, words));
    }
    fn op4c_n_e_sub0_state_write(&mut self, b1: u8) {
        self.n_e_sub0_writes.push(b1);
    }
    fn op4c_n_e_sub9_clear_b9c4(&mut self) {
        self.n_e_sub9_clears += 1;
    }
    fn op4c_n_e_sub_a_call_c7ec(&mut self) {
        self.n_e_sub_a_calls += 1;
    }
    fn op4c_n_e_sub_1_text_actor(&mut self, text_buf: &[u8], script_id: u16) {
        self.n_e_sub_1_text_calls
            .push((text_buf.to_vec(), script_id));
    }
    fn op4c_n_c_sub_1_flag_loop_reset(&mut self, _flags: &[u8]) {
        self.n_c_sub_1_flag_loops += 1;
    }
    fn op4c_n_d_sub_1_list_lookup_jump(&mut self, _ctx: &FieldCtx) -> Option<usize> {
        self.n_d_sub_1_jump_target
    }
    fn op4c_n_d_sub_2_channel_spawn(&mut self, channel: u8) {
        self.n_d_sub_2_channel_calls.push(channel);
    }
    fn op4c_n_d_sub_7_register_list_walk(&mut self) {
        self.n_d_sub_7_list_walk_regs += 1;
    }
    fn op4c_n_d_sub_b_call_e57f0(&mut self, bytecode: &[u8]) {
        self.n_d_sub_b_e57f0_calls.push(bytecode.to_vec());
    }
    fn op4c_n_d_sub_c_party_search_set(&mut self, needle: u8) -> Option<usize> {
        self.n_d_sub_c_calls.push(needle);
        self.n_d_sub_c_jump_target
    }
    fn op4c_n_d_sub_e_party_search_query(&mut self, needle: u8) -> Option<usize> {
        self.n_d_sub_e_calls.push(needle);
        self.n_d_sub_e_jump_target
    }
    fn op4c_n_d_sub_4_vram_stp_set(&mut self, x: u16, y: u16) {
        self.n_d_sub_4_vram_stp_set_calls.push((x, y));
    }
    fn op4c_n_d_sub_5_vram_stp_clear(&mut self, x: u16, y: u16) {
        self.n_d_sub_5_vram_stp_clear_calls.push((x, y));
    }
    fn op4c_n_c_sub_0_move_cancel(&mut self, _ctx: &mut FieldCtx) -> bool {
        self.n_c_sub_0_move_cancels += 1;
        self.n_c_sub_0_active
    }
    fn op4c_n_c_sub_3_script_teleport(&mut self, _ctx: &mut FieldCtx) {
        self.n_c_sub_3_teleports += 1;
    }
    fn op4c_n_c_sub_d_script_alloc(&mut self) {
        self.n_c_sub_d_allocs += 1;
    }
    fn op4c_n_c_party_flag_test(&self, flag_idx: u16) -> bool {
        *self.n_c_party_flag_bits.get(&flag_idx).unwrap_or(&false)
    }
    fn op4c_n_e_sub_4_bbox_outside(&self, _ctx: &FieldCtx, bbox: [i16; 4]) -> bool {
        self.n_e_sub_4_bboxes.borrow_mut().push(bbox);
        self.n_e_sub_4_outside
    }
    fn op4c_n_e_sub_5_add_xp(&mut self, xp_delta: i32) {
        self.n_e_sub_5_xp_deltas.push(xp_delta);
    }
    fn op4c_n_e_sub_b_actor_jump(&mut self, actor_id: u8) -> Option<()> {
        self.n_e_sub_b_actor_ids.push(actor_id);
        if self.n_e_sub_b_resolves {
            Some(())
        } else {
            None
        }
    }
    fn field_halt_acquire_predicate(&self, _ctx: &FieldCtx, _which: u8) -> bool {
        self.halt_acquire_predicate
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
    fn op4c_n_8_sub_1_set_model_anim(
        &mut self,
        _ctx: &mut FieldCtx,
        model_id: u32,
        anim_frame: u16,
        tween_frames: u16,
    ) {
        self.n_8_sub_1_set_model_calls
            .push((model_id, anim_frame, tween_frames));
    }
    fn op4c_n_8_sub_6_actor_set_rotation(
        &mut self,
        _ctx: &mut FieldCtx,
        actor_id: u8,
        position: [i16; 3],
        rotation: [i16; 3],
    ) -> bool {
        self.n_8_sub_6_actor_set_rotation_calls
            .push((actor_id, position, rotation));
        self.n_8_sub_6_actor_present
    }
    fn op4c_n_8_sub_b_actor_type_present(&self, type_byte: u8) -> bool {
        self.n_8_sub_b_present_types.contains(&type_byte)
    }
    fn op4c_n_8_sub_d_actor_search(&self, char_idx: u8, marker: u8) -> ActorSearchResult {
        self.n_8_sub_d_queries.borrow_mut().push((char_idx, marker));
        self.n_8_sub_d_search_result
    }
    fn op4c_n_e_sub_3_actor_sync_camera(&mut self, _ctx: &mut FieldCtx, actor_id: u8) {
        self.n_e_sub_3_camera_syncs.push(actor_id);
    }
    fn op4c_n_e_sub_7_camera_animate(&mut self, target: u32, duration: u16) {
        self.n_e_sub_7_camera_animates.push((target, duration));
    }
    fn op4c_n_e_sub_8_camera_zoom(&mut self, zoom_x: i16, zoom_y: i16, zoom_z: i16, mode: i16) {
        self.n_e_sub_8_camera_zooms
            .push((zoom_x, zoom_y, zoom_z, mode));
    }
    fn op4c_n_d_sub_0_field_se_trigger(&mut self, a: u16, b: u16) {
        self.n_d_sub_0_se_triggers.push((a, b));
    }
    fn op4c_n_5_sub_3_dialog_wait(&mut self, _ctx: &mut FieldCtx) {
        self.n_5_sub_3_dialog_waits += 1;
    }
    fn op4c_n_5_sub_4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        self.n_5_sub_4_polls += 1;
        self.n_5_sub_4_dialog_active
    }
    fn op4c_n5_sub1_npc_run(
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
    fn op4c_n5_sub2_menu_activation(&mut self, menu_id: u8) -> bool {
        self.n_5_sub_2_polls.borrow_mut().push(menu_id);
        self.n_5_sub_2_menu_state
            .get(&menu_id)
            .copied()
            .unwrap_or(false)
    }
    fn op4c_n6_sub_61_emitter(&mut self, _ctx: &mut FieldCtx, payload: [u8; 14]) {
        self.n_6_sub_61_emitter_calls.push(payload);
    }
    fn op4c_n8_sub_0_actor_allocator(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        self.n_8_sub_0_allocator_calls.push((count, tail.to_vec()));
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

mod actor_camera_dialog_rounds;
mod actor_ctrl_sysflag_integration;
mod camera_fade_inventory_state;
mod field_actions;
mod flow_control;
mod halt_acquire_and_overlay_helpers;
mod op4c_nibble4_and_op34;
mod op4c_nibbles_5_to_f;
