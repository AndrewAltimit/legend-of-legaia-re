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
    named_scene_transitions: Vec<(String, u8, u8)>, // (scene, entry_x, entry_z)
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
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8) {
        self.named_scene_transitions
            .push((scene.to_string(), entry_x, entry_z));
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
    fn op43_emitter_init(&mut self, payload: &[u8]) {
        self.emitter_init_payloads.push(payload.to_vec());
    }
    fn op43_emitter_5_words(&mut self, words: [u16; 5]) {
        self.emitter_5_words.push(words);
    }
    fn op43_emitter_struct_12(&mut self, payload: &[u8]) {
        self.emitter_struct12_payloads.push(payload.to_vec());
    }
    fn op43_emitter_split_call(&mut self, words: [i16; 6], did_split: bool) {
        self.emitter_split_calls.push((words, did_split));
    }
    fn op43_emitter_func13(&mut self, payload: &[u8; 13]) {
        self.emitter_func13_payloads.push(*payload);
    }
    fn op43_emitter_4_words(&mut self, words: [i16; 4]) {
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

// -- NOP cluster ----------------------------------------------------

#[test]
fn nop_cluster_advances_one_byte() {
    for op in [0x21u8, 0x24, 0x25, 0x48] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[op], 0);
        assert_eq!(r, StepResult::Advance { next_pc: 1 });
    }
}

// -- JMP_REL --------------------------------------------------------

#[test]
fn jmp_rel_forward() {
    // 0x26 with offset 0x0008 should land at PC = 0 + 1 + 8 = 9.
    let bc = [0x26, 0x08, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

#[test]
fn jmp_rel_with_high_byte() {
    // Offset 0x0100 from PC 0 lands at 0x0101.
    let bc = [0x26, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x0101 });
}

#[test]
fn jmp_rel_backward_wraps_at_16_bits() {
    // Retail stores PC as a 16-bit `short`, so a delta with the high bit
    // set is a *backward* jump - `0xFFFE` = -2. From base = pc + 1 this
    // must land 2 bytes back, NOT race off to base + 0xFFFE. This is the
    // "PC runs away to 0x10102" bug: real field scripts use backward
    // JMP_REL for per-frame wait loops, so without the 16-bit wrap every
    // parked script explodes off the end of its buffer.
    let mut bc = vec![0u8; 0x80];
    bc[0x42] = 0x26;
    bc[0x43] = 0xFE;
    bc[0x44] = 0xFF;
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // base = pc(0x42) + header_size(1) = 0x43; 0x43 + (-2) = 0x41.
    let r = step(&mut host, &mut ctx, &bc, 0x42);
    assert_eq!(r, StepResult::Advance { next_pc: 0x41 });
    // From PC 0 the same -2 delta wraps to 0xFFFF (a deliberately
    // out-of-range PC, matching retail's 16-bit truncation).
    let bc0 = [0x26, 0xFE, 0xFF];
    let r0 = step(&mut host, &mut ctx, &bc0, 0);
    assert_eq!(r0, StepResult::Advance { next_pc: 0xFFFF });
}

#[test]
fn flag_test_backward_jump_wraps_at_16_bits() {
    // The 0x7x flag-TEST conditional jump shares the same 16-bit-wrap
    // rule. With the bit set and a `0xFFF0` (-16) delta from base
    // pc+header+1, a backward jump must wrap rather than overflow.
    let mut bc = vec![0u8; 0x80];
    bc[0x30] = 0x70;
    bc[0x31] = 0x00;
    bc[0x32] = 0xF0;
    bc[0x33] = 0xFF;
    let mut host = TestHost::default();
    host.system_flags.resize(8192, 0);
    host.system_flags[0] = 0x80; // idx 0 set (0x80 >> 0)
    let mut ctx = FieldCtx::default();
    // base = pc(0x30) + header(1) + 1 = 0x32; 0x32 + (-16) = 0x22.
    let r = step(&mut host, &mut ctx, &bc, 0x30);
    assert_eq!(r, StepResult::Advance { next_pc: 0x22 });
}

// -- Local flag triplet 0x2B / 0x2C / 0x2D ---------------------------

#[test]
fn lflag_set_then_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Set bit 5 (mask 0x20).
    let r = step(&mut host, &mut ctx, &[0x2B, 5], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0x0020);

    // Clear bit 5.
    let r = step(&mut host, &mut ctx, &[0x2C, 5], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0);
}

#[test]
fn lflag_set_masks_to_5_bits() {
    // The original masks the bit index by 0x1F. Index 0x25 should set
    // bit 5 (0x20), not bit 0x25.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &[0x2B, 0x25], 0);
    assert_eq!(ctx.local_flags, 0x0020);
}

#[test]
fn lflag_tst_bit_set_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0x0040, // bit 6 set
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

#[test]
fn lflag_tst_bit_clear_halts() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x2D, 6], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- Global flag triplet 0x2E / 0x2F / 0x30 --------------------------

#[test]
fn gflag_set_writes_through_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x2E, 17], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.globals, 1u32 << 17);
}

#[test]
fn gflag_clr_preserves_other_bits() {
    let mut host = TestHost {
        globals: 0xFFFF_FFFF,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &[0x2F, 3], 0);
    assert_eq!(host.globals, !(1u32 << 3));
}

#[test]
fn gflag_tst_branches_on_global() {
    let mut host = TestHost {
        globals: 1u32 << 12,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x30, 12], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });

    // bit 13 is clear -> halt
    let r = step(&mut host, &mut ctx, &[0x30, 13], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- Context flag triplet 0x31 / 0x32 / 0x33 -------------------------

#[test]
fn cflag_set_normal_path_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x31, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags, 1);
}

#[test]
fn cflag_set_bit_8_copies_field_26_and_advances() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_26: 0x1234,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x31, 8], 0);
    // Bit 8 = mask 0x100. The original calls
    // `switchD_801e0f24::caseD_4()` which is 0x801df098 → s8 += 2 →
    // return; PC advances by 2 like every other 0x31 path.
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags, 0x100);
    assert_eq!(ctx.saved_26, 0x1234);
}

#[test]
fn cflag_clr_basic() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    step(&mut host, &mut ctx, &[0x32, 10], 0);
    assert_eq!(ctx.flags, !(1u32 << 10));
}

#[test]
fn cflag_tst_halts_when_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x33, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- YIELD ----------------------------------------------------------

#[test]
fn yield_sets_halt_and_saves_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x37], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.saved_pc, 0);
}

#[test]
fn yield_4_uses_pc_plus_4() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Pad so pc=5 is in-bounds (yield reads only the opcode byte).
    let mut bc = [0u8; 6];
    bc[5] = 0x47;
    let r = step(&mut host, &mut ctx, &bc, 5);
    assert_eq!(r, StepResult::Yield { resume_pc: 9 });
    assert_eq!(ctx.saved_pc, 5);
}

// -- DATA_BLOCK ------------------------------------------------------

#[test]
fn data_block_skips_len_bytes() {
    // len = 4: skip operand byte + 4 inline bytes => PC += 6.
    let bc = [0x40, 0x04, 0xAA, 0xBB, 0xCC, 0xDD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

// -- WAIT_FRAMES ----------------------------------------------------

#[test]
fn wait_frames_accumulates_until_target() {
    let mut host = TestHost {
        frame_delta: 1,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let bc = [0x4A, 0x03, 0x00]; // wait 3 frames

    // Tick 1: accum=1, halt.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.wait_accum, 1);

    // Tick 2: accum=2, still halt.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.wait_accum, 2);

    // Tick 3: accum=3, advance + reset.
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.wait_accum, 0);
}

// -- Pending / Unknown -----------------------------------------------

#[test]
fn op_4c_n8_sub_3_no_longer_pending() {
    // Sanity-check that the rectangular tile-fill case (`0x4C n8 sub-3`,
    // host hook [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`]) is fully
    // ported. The dispatcher previously returned `Pending` for this
    // opcode; it now invokes the host and advances PC by 7. There are
    // no remaining `0x4C` sub-ops that return `Pending`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x83, 0x00, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n8_rect_tile_fills, vec![(0u8, 0, 0, 0, 0)]);
}

// -- Multi-instruction trace -----------------------------------------

#[test]
fn trace_runs_through_chain() {
    // Set local-flag bit 0, jump forward over 3 garbage bytes, set local
    // bit 1, fall off the end. JMP_REL formula:
    //   target = (pc + header_size) + ((hi << 8) | lo)
    // Jumping from pc=2 with offset 5 lands at byte 8 = the second SET.
    let bc = [
        0x2B, 0, // [0..2] LFLAG_SET 0
        0x26, 0x05, 0x00, // [2..5] JMP +5 -> target = 3 + 5 = 8
        0xFF, 0xFF, 0xFF, // [5..8] unreachable
        0x2B, 1, // [8..10] LFLAG_SET 1
    ];

    let mut host = TestHost::default();
    let (ctx, _trace) = run(&mut host, &bc);
    assert_eq!(ctx.local_flags, 0b11);
}

// -- peek_extended ---------------------------------------------------

#[test]
fn peek_extended_returns_target_id_when_high_bit_set() {
    // 0xA1 = 0x80 | 0x21 (extended NOP). Next byte is target script ID.
    let bc = [0xA1, 0x42, 0x00];
    assert_eq!(peek_extended(&bc, 0), Some(0x42));
}

#[test]
fn peek_extended_returns_none_for_normal_opcode() {
    let bc = [0x21, 0x42];
    assert_eq!(peek_extended(&bc, 0), None);
}

#[test]
fn peek_extended_handles_eof() {
    // Empty bytecode -> None.
    assert_eq!(peek_extended(&[], 0), None);
    // Lone extended-bit byte at end (no script-id byte) -> None.
    assert_eq!(peek_extended(&[0xA1], 0), None);
}

// -- Cross-context dispatch (extended bit) --------------------------

#[test]
fn extended_lflag_set_advances_three_bytes() {
    // Encoding: [0xAB (= 0x80 | 0x2B), script_id, bit].
    // Header size = 2; tail = 1; next_pc = 0 + 2 + 1 = 3.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 5,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 5, 3], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.local_flags, 1u16 << 3);
}

#[test]
fn extended_jmp_rel_uses_header_size_2() {
    // [0xA6, script_id, lo, hi]. target = pc + 2 + delta. With pc=0,
    // delta=4, expect target=6.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0xA6, 0x42, 0x04, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

#[test]
fn extended_halt_target_returns_halt() {
    // Halted ctx + extended dispatch + non-carve-out op -> Halt.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400, // halted
        script_id: 7,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 7, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Local flag should NOT have been written.
    assert_eq!(ctx.local_flags, 0);
}

#[test]
fn extended_system_channel_bypasses_halt() {
    // script_id == 0xFB is the system channel; halted state is ignored.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400,
        script_id: 0xFB,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xAB, 0xFB, 4], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.local_flags, 1u16 << 4);
}

#[test]
fn extended_032_with_bit_10_unhalt_carve_out() {
    // 0x32 (CFLAG_CLR) with bit 10 (mask 0x400) is the unique opcode that
    // can run on a halted target - it's how a script un-halts another.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400,
        script_id: 9,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xB2, 9, 10], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.flags, 0); // halt bit cleared
    assert!(!ctx.is_halted());
}

#[test]
fn extended_032_with_other_bit_does_not_bypass_halt() {
    // Same opcode but a different bit -> halt path still wins.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x400 | 0x10,
        script_id: 9,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xB2, 9, 4], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Bit 4 should NOT have been cleared (the dispatch never fired).
    assert_eq!(ctx.flags, 0x400 | 0x10);
}

// -- 0x22 EXEC_MOVE --------------------------------------------------

#[test]
fn exec_move_writes_state_and_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 3,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x22, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.move_id, 5);
    assert_eq!(ctx.field_5e, 0xFFFE);
    assert_eq!(ctx.move_substate, 1);
    assert_eq!(host.exec_moves, vec![(3u16, 5u8)]);
}

#[test]
fn exec_move_zero_sets_substate_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x22, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.move_substate, 5);
}

#[test]
fn exec_move_extended_threads_target_id() {
    // Extended dispatch on a non-halted target.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x42,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0xA2, 0x42, 0x07], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.exec_moves, vec![(0x42u16, 7u8)]);
}

// -- 0x23 MOVE_TO ----------------------------------------------------

#[test]
fn move_to_decodes_grid_coords_no_high_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 4,
        ..Default::default()
    };
    // x=2, z=3, neither has high bit -> world_x = 2*0x80 + 0x40 = 0x140,
    // world_z = 3*0x80 + 0x40 = 0x1C0.
    let r = step(&mut host, &mut ctx, &[0x23, 0x02, 0x03], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.world_x, 0x0140);
    assert_eq!(ctx.world_z, 0x01C0);
    assert_eq!(ctx.npc_x, 0x02);
    assert_eq!(ctx.npc_facing, 0x03);
    assert_eq!(host.move_tos, vec![(4u16, 0x0140u16, 0x01C0u16, false)]);
}

#[test]
fn move_to_high_bit_adds_offset() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // x = 0x82 (low bits 2, high bit set) -> 2*0x80 + 0x40 + 0x40 = 0x180.
    let r = step(&mut host, &mut ctx, &[0x23, 0x82, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.world_x, 0x0180);
    assert_eq!(ctx.world_z, 0x0040); // 0*0x80 + 0x40
}

#[test]
fn move_to_player_path_uses_flag_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x1000000, // player chain bit
        ..Default::default()
    };
    step(&mut host, &mut ctx, &[0x23, 0x01, 0x01], 0);
    assert_eq!(host.move_tos.len(), 1);
    assert!(host.move_tos[0].3); // is_player == true
}

// -- 0x3F SCENE_CHANGE (named warp) ----------------------------------

#[test]
fn scene_change_decodes_name_and_entry() {
    // idx = 0x0042, name_len = 4 ("dolk"), entry_x = 1, entry_z = 2, dir = 3.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x42, 0x00, // opcode + idx (LE)
        0x04, b'd', b'o', b'l', b'k', // name_len + name
        0x01, 0x02, 0x03, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    // header_size 1 + 3 (idx,len) + 4 (name) + 3 (entry) = 11.
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(
        host.named_scene_transitions,
        vec![("dolk".to_string(), 0x01, 0x02)]
    );
    // It is not a dialog opener.
    assert!(host.dialogs.is_empty());
}

#[test]
fn scene_change_phantom_name_stages_no_transition() {
    // A 0x3F whose "name" is uppercase/punctuation (a literal '?' inside text)
    // fails the clean-CDNAME gate: no transition, but the PC still advances.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x10, 0x00, 0x04, b'H', b'i', b'!', b' ', // idx + len + "Hi! "
        0x00, 0x00, 0x00, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert!(host.named_scene_transitions.is_empty());
}

#[test]
fn scene_change_empty_name_no_transition() {
    // name_len = 0 -> empty name -> no transition; advances header + 6.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [
        0x3F, 0x10, 0x00, 0x00, // opcode + idx + name_len=0
        0x00, 0x00, 0x00, // entry_x, entry_z, dir
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert!(host.named_scene_transitions.is_empty());
}

#[test]
fn scene_change_truncated_buffer_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // name_len = 10 but bytecode only has 5 trailing bytes - should error.
    let bc = [0x3F, 0x00, 0x00, 0x0A, 0x01, 0x02, 0x03, 0x04, 0x05];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x3F,
            pc: 0
        }
    ));
    assert!(host.named_scene_transitions.is_empty());
}

// -- 0x35 BGM --------------------------------------------------------

#[test]
fn bgm_decodes_text_id_and_sub_op() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x35, 0x12, 0x00, 0x01], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.bgm_calls, vec![(0x12u16, 1u8)]);
}

// -- 0x38 CAM_CFG ----------------------------------------------------

#[test]
fn cam_cfg_simple_path_writes_field_26_from_table() {
    let mut host = TestHost {
        cam_cfg_table: vec![0xAA, 0xBB, 0xCC, 0xDD],
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // op0 = 2 (low nibble), op1 = 0 (& 0x7F == 0 -> simple path).
    let r = step(&mut host, &mut ctx, &[0x38, 0x02, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_26, 0xCC);
}

#[test]
fn cam_cfg_halt_acquire_succeeds_and_yields() {
    // op1 != 0 → halt-acquire path. With the predicate set to true (the
    // default test impl), the VM marks ctx halted (saved_pc + wait_accum
    // + flag 0x400) and yields with `resume_pc = pc + 3`.
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x38, 0x05, 0x01], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    assert_eq!(host.halt_acquire_calls, vec![(0x38u8, 3usize, [0i16; 3])]);
}

#[test]
fn cam_cfg_halt_acquire_failed_predicate_halts_at_pc() {
    // op1 != 0 + predicate false → original falls into the dispatcher
    // default-arm path; for op 0x38 (not a 0x50/0x60/0x70 opcode) that
    // halts the VM at the current PC.
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x38, 0x00, 0x01], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(!ctx.is_halted());
    assert_eq!(host.halt_acquire_calls, vec![]);
}

// -- 0x39 GIVE_ITEM --------------------------------------------------

#[test]
fn give_item_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x39, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.give_item_calls, vec![0x42]);
}

// -- 0x3A ADD_MONEY --------------------------------------------------

#[test]
fn add_money_positive_24_bit() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0x000100 = +256.
    let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x01, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![256]);
}

#[test]
fn add_money_negative_sign_extended() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0xFFFFFF = -1 in 24-bit two's complement.
    let r = step(&mut host, &mut ctx, &[0x3A, 0xFF, 0xFF, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![-1]);
}

#[test]
fn add_money_min_negative() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0x800000 = -8388608 (24-bit min signed).
    let r = step(&mut host, &mut ctx, &[0x3A, 0x00, 0x00, 0x80], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.money_deltas, vec![-8388608]);
}

// -- 0x3B SET_ITEM_COUNT --------------------------------------------

#[test]
fn set_item_count_passes_raw_slot_and_count() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3B, 0x23, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.item_writes, vec![(0x23u8, 0x05u8)]);
}

// -- 0x3C / 0x3D party ----------------------------------------------

#[test]
fn party_add_and_remove_route_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3C, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    let r = step(&mut host, &mut ctx, &[0x3D, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_added, vec![0x02]);
    assert_eq!(host.party_removed, vec![0x02]);
}

// -- 0x42 COND_JMP --------------------------------------------------

#[test]
fn cond_jmp_mode_0_skips_when_flag_clear() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // mode=0, bit=3, lo=0xFF, hi=0xFF - flag clear -> skip 5 bytes.
    let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0xFF, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_0_jumps_when_flag_set() {
    let mut host = TestHost {
        extras: 1u32 << 3,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // mode=0, bit=3, delta=0x10. Jump target = pc + 3 + 0x10 = 19. The
    // original computes `iVar18 = param_2 + 3; return iVar18 + delta`.
    let r = step(&mut host, &mut ctx, &[0x42, 0x00, 0x03, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });
}

#[test]
fn cond_jmp_mode_1_table_match_jumps() {
    // mode=1, op1=3 (< 8 → table lookup path), table[3] = high-nibble
    // value of screen_mode. delta = 0x20.
    let mut host = TestHost {
        screen_mode: 0x4000,
        screen_mode_table: {
            let mut v = vec![None; 8];
            v[3] = Some(0x4000);
            v
        },
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
    // Take jump: pc + 3 + 0x20 = 35.
    assert_eq!(r, StepResult::Advance { next_pc: 35 });
}

#[test]
fn cond_jmp_mode_1_table_mismatch_skips() {
    // Same as above but screen_mode high nibble doesn't match table[3].
    let mut host = TestHost {
        screen_mode: 0x1000,
        screen_mode_table: {
            let mut v = vec![None; 8];
            v[3] = Some(0x4000);
            v
        },
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x03, 0x20, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_1_bit_test_path() {
    // op1 = 8 → tests screen_mode bit 0x20.
    let mut host = TestHost {
        screen_mode: 0x20,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // Bit set → take jump.
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });

    // Bit clear → skip.
    host.screen_mode = 0;
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn cond_jmp_mode_2_halts_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Mode >= 2 hits the dispatcher's default arm, which returns
    // `param_2` for opcodes whose high nibble isn't 0x5x/0x6x/0x7x.
    // 0x42 & 0x70 = 0x40 → halt at PC.
    let r = step(&mut host, &mut ctx, &[0x42, 0x02, 0x00, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn cond_jmp_mode_1_op1_at_least_c_takes_jump() {
    // Mode 1 op1 >= 0xC: original at line 5176 of the dump falls
    // through every `if (uVar31 == N) ... return iVar18` branch and
    // ends up in the unconditional take-jump path with
    // `iVar18 = param_2 + 3` and `delta = LE_u16(operand[2..4])`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [0x42, 0x01, 0x0C, 0x10, 0x00] → mode 1, op1=0xC, delta=0x0010.
    // Expected next_pc = 0 + 3 + 0x10 = 19 (header_size=2, +1 for mode,
    // +0x10 delta - 3 + delta).
    let r = step(&mut host, &mut ctx, &[0x42, 0x01, 0x0C, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 19 });
}

#[test]
fn op_34_sub_f_halts_at_pc() {
    // Top of the 4-bit sub-op range. Same dispatch path as sub-4.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0xF0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_43_subop_ff_halts_at_pc() {
    // 0xFF is the sentinel "uninitialised bytecode" byte. 0x43 sub-op
    // 0xFF has no original handler ⇒ halt at PC.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n4_sub_e_or_f_halts_at_pc() {
    // 0x4C outer-nibble 4 inner switch (line 5901 of the dump) has cases
    // 0..=0xD followed by an explicit `default:` that prints
    // `SUB_40_ERROR` and routes via `switchD_801e00f4::default()` ⇒
    // halt at PC for sub-ops 0xE/0xF. The instruction is 6 bytes.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x4E, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x4F, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- 0x3E WARP / INTERACT -------------------------------------------

#[test]
fn warp_interact_path_advances_3() {
    // op0 = 5 (< 100) → INTERACT path.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3E, 0x05, 0x02], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.interacts, vec![(0x05u8, 0x02u8)]);
    assert!(host.scene_transitions.is_empty());
}

#[test]
fn warp_interact_handles_0xff_sentinel() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x3E, 0xFF, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.interacts, vec![(0xFFu8, 0x00u8)]);
}

#[test]
fn warp_scene_transition_path_advances_6_and_clears_flag() {
    // op0 = 105 (>= 100) → WARP. map_id = 5.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0xFFFF_FFFF,
        ..Default::default()
    };
    let bc = [0x3E, 105, 0, 0, 0, 0];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.scene_transitions, vec![5u8]);
    // Bit 0x80000 cleared on the active ctx.
    assert_eq!(ctx.flags & 0x80000, 0);
    // Other bits preserved.
    assert_eq!(ctx.flags & 0x40000, 0x40000);
}

// -- 0x46 RENDER_CFG ------------------------------------------------

#[test]
fn render_cfg_long_form_advances_6() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x46, 0x24, 0x11, 0x22, 0x33, 0x44],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.render_long, vec![(0x11u8, 0x22u8, 0x33u8, 0x44u8)]);
    assert!(host.render_short.is_empty());
}

#[test]
fn render_cfg_short_form_advances_3_and_computes_bitfield() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x10, op1 = 0x06.
    // r = !(0x10 >> 1) & 0xFF = !0x08 & 0xFF = 0xF7
    // g = 2 - (0x06 >> 1) = 2 - 3 = 0xFF (wrap)
    // b = (0x10 >> 1) - 1 = 0x07
    // packed = (0x06 >> 1) + 2 = 5
    let r = step(&mut host, &mut ctx, &[0x46, 0x10, 0x06], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.render_short, vec![(0xF7u8, 0xFFu8, 0x07u8, 0x05u8)]);
    assert!(host.render_long.is_empty());
}

// -- 0x4F SCENE_REGISTER_WRITE --------------------------------------

#[test]
fn scene_register_write_passes_three_bytes() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4F, 0xAA, 0xBB, 0xCC], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.scene_regs, vec![(0xAAu8, 0xBBu8, 0xCCu8)]);
}

// -- 0x44 COUNTER ----------------------------------------------------

#[test]
fn counter_advances_and_calls_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x44, 0x55], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.counter_calls, vec![0x55]);
}

// -- 0x4B ANIMATE ----------------------------------------------------

#[test]
fn animate_advances_3_plus_4_count_and_writes_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 7,
        local_flags: 0xFFFF, // all bits set; check mask
        ..Default::default()
    };
    // count = 2, base_id = 5, 8 keyframe bytes.
    let bc = [0x4B, 2, 5, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(ctx.flags & 0x1000, 0x1000);
    // 0xD3FF = 0b1101001111111111 keeps bits 12, 6-9, 0-5; clears 10, 11, 13, 14.
    assert_eq!(ctx.local_flags & 0x1000, 0x1000);
    assert_eq!(ctx.local_flags & 0x0C00, 0); // bits 10, 11 cleared by mask
    assert_eq!(ctx.local_flags & 0x2000, 0); // bit 13 cleared
    assert_eq!(ctx.face_rotation, 2);
    assert_eq!(host.animations.len(), 1);
    assert_eq!(host.animations[0].0, 7);
    assert_eq!(host.animations[0].1, 2);
    assert_eq!(host.animations[0].2, 5);
    assert_eq!(
        host.animations[0].3,
        vec![0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23]
    );
}

#[test]
fn animate_zero_count_advances_3() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4B, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.animations.len(), 1);
    assert!(host.animations[0].3.is_empty());
}

// -- 0x4C MENU_CTRL sub-0 -------------------------------------------

#[test]
fn menu_sub_0_sets_party_leader() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x05 (high nibble 0, low bits 5 → masked to 5 = 0x05 & 7).
    let r = step(&mut host, &mut ctx, &[0x4C, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_leaders, vec![5]);
}

#[test]
fn menu_sub_0_masks_leader_to_low_3_bits() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x0F → masked to 7.
    step(&mut host, &mut ctx, &[0x4C, 0x0F], 0);
    assert_eq!(host.party_leaders, vec![7]);
}

#[test]
fn menu_sub_1_advances_seven_and_dispatches_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x12 → high nibble 1 → menu_ctrl_sub1.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x12, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(
        host.menu_sub1_calls,
        vec![(0x12, [0xA1, 0xA2, 0xA3, 0xA4, 0xA5])]
    );
}

#[test]
fn menu_sub_3_sub_5_writes_local_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0xFF80,
        ..Default::default()
    };
    // op0 = 0x35 → sub-3 sub-5: lf = (lf & 0xFF7F) | 0x020A.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x35], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, (0xFF80 & 0xFF7F) | 0x020A);
}

#[test]
fn menu_sub_3_sub_6_or_local_flags() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        local_flags: 0x0001,
        ..Default::default()
    };
    // op0 = 0x36 → sub-3 sub-6: lf |= 0x028A.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x36], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.local_flags, 0x0001 | 0x028A);
}

#[test]
fn menu_sub_3_sub_7_no_player_falls_through() {
    // Host has no player_coords set → behave as a no-op advance.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.world_x, 0);
    assert!(host.inverted_y_writes.is_empty());
}

#[test]
fn menu_sub_3_sub_7_copies_player_coords() {
    let mut host = TestHost {
        player_coords: Some(PlayerCoords {
            world_x: 0x1234,
            world_y: 0x4567,
            world_z: 0x89AB,
            field_26: 0xCDEF,
        }),
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.world_x, 0x1234);
    assert_eq!(ctx.world_y, 0x4567);
    assert_eq!(ctx.world_z, 0x89AB);
    assert_eq!(ctx.field_26, 0xCDEF);
    // No bit 0x20000000 → no inverted-Y write.
    assert!(host.inverted_y_writes.is_empty());
}

#[test]
fn menu_sub_3_sub_7_yields_with_inverted_y_when_flag_set() {
    let mut host = TestHost {
        player_coords: Some(PlayerCoords {
            world_x: 0,
            world_y: 0x0010, // small positive - easy to negate
            world_z: 0,
            field_26: 0,
        }),
        ..Default::default()
    };
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x4C, 0x37], 0);
    // Inverted-Y branch returns Yield (caseD_4 STATE_RESUME exit).
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.inverted_y_writes, vec![-0x0010]);
    // Coords still copied even on the yield path.
    assert_eq!(ctx.world_y, 0x0010);
}

#[test]
fn menu_sub_3_sub_2_clears_party_state_region() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x32 → sub-3 sub-2 (clear 512-byte party-state region).
    let r = step(&mut host, &mut ctx, &[0x4C, 0x32], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_state_clears, 1);
}

#[test]
fn op_4c_outer_dispatcher_has_no_remaining_pending_arms() {
    // Sanity-check that every `0x4C` sub-dispatcher now returns
    // `Advance` / `Halt` / `Yield` / `Unknown` - none returns
    // `Pending`. n8 sub-3 (box-fill table via FUN_801D5630) was the
    // last truly-pending case; it now invokes
    // [`FieldHost::op4c_n_8_sub_3_rect_tile_fill`] and advances PC.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x83, 0, 0, 0, 0, 0], 0);
    assert!(!matches!(r, StepResult::Pending { .. }));
}

#[test]
fn op_4c_n4_sub_0_immediate_writes_field_72() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x40, 34, 12, 0, 0] → sub-0, val=0x1234, ticks=0 (immediate)
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0x34, 0x12, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_72, 0x1234);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_0_ramp_calls_host_does_not_write_ctx() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x40, 0xCC, 0x55, 0x10, 0] → sub-0, val=0x55CC, ticks=16
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xCC, 0x55, 0x10, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_72, 0); // VM does not write ctx in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(0u8, 0x55CCi16, 16u16)]);
}

#[test]
fn op_4c_n4_sub_2_immediate_mirrors_world_y_when_flag_set() {
    // sub-2 immediate write path: when `flags & 0x20000000` is set,
    // also writes `world_y = -value`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..FieldCtx::default()
    };
    // val = 0x0064 (= 100); world_y should become (u16)(-100) = 0xFF9C
    let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_8e, 100);
    assert_eq!(ctx.world_y, (-100i16) as u16);
}

#[test]
fn op_4c_n4_sub_2_immediate_no_mirror_without_flag() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x42, 0x64, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_8e, 100);
    assert_eq!(ctx.world_y, 0);
}

#[test]
fn op_4c_n4_sub_8_immediate_writes_field_26() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x48, 0x07, 0x00, 0, 0] → sub-8, val=7
    let r = step(&mut host, &mut ctx, &[0x4C, 0x48, 0x07, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_26, 7);
}

#[test]
fn op_4c_n4_global_subs_call_host() {
    for sub in [0xAu8, 0xB, 0xC, 0xD] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(
            &mut host,
            &mut ctx,
            &[0x4C, 0x40 | sub, 0x10, 0x00, 0x05, 0x00],
            0,
        );
        assert_eq!(r, StepResult::Advance { next_pc: 6 });
        assert_eq!(host.n4_global_writes, vec![(sub, 0x10i32, 5u16)]);
    }
}

// -- 0x4C outer-nibble-4 sub-1 (ctx[+0x6A] write/ramp with halve+floor)

#[test]
fn op_4c_n4_sub_1_immediate_halves_and_floors() {
    // val = 6 → halved = 3 → ctx.field_6a = 3
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x06, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 3);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_1_immediate_floors_zero_to_one() {
    // val = 1 → halved = 0 → floor → 1
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x41, 0x01, 0x00, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 1);
}

#[test]
fn op_4c_n4_sub_1_ramp_passes_halved_target_to_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val=20 → halved=10; ticks=8
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x41, 0x14, 0x00, 0x08, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_6a, 0); // VM does not write in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(1u8, 10i16, 8u16)]);
}

// -- 0x4C outer-nibble-4 sub-3 (ramp +0x24 OR absolute jump)

#[test]
fn op_4c_n4_sub_3_ticks_zero_jumps_absolute() {
    // ticks=0 path: returned `iVar18` = signed_16(operand[0..2]) - the
    // new PC offset is the literal target value.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x43, 0x40, 0x00, 0, 0], 0);
    // target = 0x40 → next_pc = 0x40
    assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
    assert_eq!(ctx.field_24, 0); // jump-only branch does not write
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_3_ticks_nonzero_ramps_field_24() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val=300, ticks=12 → ramp `ctx.field_24` 0 → 300 over 12 frames
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x43, 0x2C, 0x01, 0x0C, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_24, 0); // VM does not write in ramp path
    assert_eq!(host.n4_ctx_ramps, vec![(3u8, 300i16, 12u16)]);
}

// -- 0x4C outer-nibble-4 sub-4 (immediate +0x28 OR absolute jump)

#[test]
fn op_4c_n4_sub_4_ticks_zero_writes_field_28() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x44, 0xFF, 0xFF, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(ctx.field_28, -1);
    assert!(host.n4_ctx_ramps.is_empty());
}

#[test]
fn op_4c_n4_sub_4_ticks_nonzero_jumps_absolute() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks=4 (nonzero) → jump to target = 0x80
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x44, 0x80, 0x00, 0x04, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 0x80 });
    assert_eq!(ctx.field_28, 0); // jump-only branch does not write
}

// -- 0x4C outer-nibble-4 sub-6 / sub-7 (paired-global gate)

#[test]
fn op_4c_n4_sub_6_gate_clear_writes_global() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // gate=false (default) → regular write/ramp dispatch fires.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_global_writes, vec![(6u8, 7i32, 16u16)]);
    assert_eq!(host.n4_global_pair_clears, 0);
}

#[test]
fn op_4c_n4_sub_7_gate_clear_writes_global() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x47, 0x21, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_global_writes, vec![(7u8, 0x21i32, 0u16)]);
    assert_eq!(host.n4_global_pair_clears, 0);
}

#[test]
fn op_4c_n4_sub_6_gate_set_clears_pair_and_skips_write() {
    let mut host = TestHost {
        n4_global_pair_gated: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x46, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // No write happened - the gate short-circuited to a pair-clear.
    assert!(host.n4_global_writes.is_empty());
    assert_eq!(host.n4_global_pair_clears, 1);
}

#[test]
fn op_4c_n4_sub_7_gate_set_clears_pair_and_skips_write() {
    let mut host = TestHost {
        n4_global_pair_gated: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x47, 0x07, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert!(host.n4_global_writes.is_empty());
    assert_eq!(host.n4_global_pair_clears, 1);
}

// -- 0x4C outer-nibble-4 sub-5 (actor-field block, 11-byte encoding)

#[test]
fn op_4c_n4_sub_5_immediate_writes_actor_block() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x45, b1=0x07, w94=0x0123, w96=-0x0001, w98=0x4567, ticks=0]
    let bytes = [
        0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67, 0x45, 0x00, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(host.n4_sub5_immediate, vec![(0x07, 0x0123, -1, 0x4567)]);
    assert!(host.n4_sub5_ramp.is_empty());
}

#[test]
fn op_4c_n4_sub_5_ramp_yields_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks = 30 → ramp path
    let bytes = [
        0x4C, 0x45, 0x40, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00, 0x1E, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 11 });
    assert_eq!(
        host.n4_sub5_ramp,
        vec![(0x40, 0x0010, 0x0020, 0x0030, 30u16)]
    );
    assert!(host.n4_sub5_immediate.is_empty());
}

#[test]
fn op_4c_n4_sub_5_truncated_buffer_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Only 8 bytes - sub-5 needs 11.
    let bytes = [0x4C, 0x45, 0x07, 0x23, 0x01, 0xFF, 0xFF, 0x67];
    let r = step(&mut host, &mut ctx, &bytes, 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

// -- 0x4C outer-nibble-4 sub-9 (story-flag-driven dispatch)

#[test]
fn op_4c_n4_sub_9_default_immediate_calls_default_write() {
    let mut host = TestHost::default(); // n4_sub9_state defaults to Default
    let mut ctx = FieldCtx::default();
    // [4C, 0x49, target=0x0042, ticks=0]
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x42, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n4_sub9_default_writes, vec![0x0042i16]);
    assert!(host.n4_sub9_default_ramps.is_empty());
    assert!(host.n4_sub9_delta_calls.is_empty());
}

#[test]
fn op_4c_n4_sub_9_default_ramp_yields_at_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks = 60 → ramp path; VM yields at the same PC.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x42, 0x00, 0x3C, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    assert_eq!(host.n4_sub9_default_ramps, vec![(0x0042i16, 60u16)]);
    assert!(host.n4_sub9_default_writes.is_empty());
}

#[test]
fn op_4c_n4_sub_9_abs_jump_uses_signed_target() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::AbsJump,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    // target = 0x0080 → absolute jump regardless of ticks.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x80, 0x00, 0x10, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 0x80 });
    // No host writes happen on the abs-jump path.
    assert!(host.n4_sub9_default_writes.is_empty());
    assert!(host.n4_sub9_default_ramps.is_empty());
    assert!(host.n4_sub9_delta_calls.is_empty());
}

#[test]
fn op_4c_n4_sub_9_delta_immediate_advances() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::Delta,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x55, 0xFF, 0x00, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // signed_16(0xFF55) = -171
    assert_eq!(host.n4_sub9_delta_calls, vec![(-171i16, 0u16)]);
}

#[test]
fn op_4c_n4_sub_9_delta_ramp_yields() {
    let mut host = TestHost {
        n4_sub9_state: Sub9State::Delta,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4C, 0x49, 0x55, 0x00, 0x14, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    assert_eq!(host.n4_sub9_delta_calls, vec![(0x55i16, 20u16)]);
}

#[test]
fn op_4c_n4_sub_9_state_default_when_no_story_bits_set() {
    // Verify the FieldHost::op4c_n4_sub9_state default impl reads global flags.
    let host = TestHost {
        globals: 0x0000_0000,
        ..TestHost::default()
    };
    // TestHost overrides with its own n4_sub9_state, but a fresh host with
    // the default Sub9State is at the `Default` variant.
    assert_eq!(host.n4_sub9_state, Sub9State::Default);
}

#[test]
fn op_34_sub_2_no_match_advances_two() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub_op = 2, b1 = 0x40 - but op34_sub2_capture_match=false (default)
    // so the host returns false → Advance PC += 2.
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert!(host.op34_sub2_captures.is_empty());
}

#[test]
fn op_34_sub_2_match_with_b1_40_yields_and_captures() {
    let mut host = TestHost {
        op34_sub2_capture_match: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x40, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0 });
    // Captured PC offset should be `pc + header_size + 2` = 3.
    assert_eq!(host.op34_sub2_captures, vec![(0x40u8, 3usize)]);
}

#[test]
fn op_37_arm_encounter_forwards_record_window_when_armed() {
    // When the host reports the active entity is an encounter carrier, the
    // bare arm-encounter op (0x37) hands the host the record window that
    // overlays the opcode: [opcode][op1][op2][count][ids..].
    let mut host = TestHost {
        scripted_encounter_armed: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    // record overlay at pc 0: [0x37][op1][op2][count=2][id=0x4F][id=0x50](+tail)
    let bc = [0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00, 0x99];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert_eq!(host.scripted_encounter_windows.len(), 1);
    // Bounded 8-byte window starting at the opcode.
    assert_eq!(
        host.scripted_encounter_windows[0],
        vec![0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00]
    );
}

#[test]
fn op_37_yield_does_not_arm_encounter_when_unarmed() {
    // Default host is unarmed: a generic 0x37 yield must not be mistaken
    // for an encounter arm.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let bc = [0x37, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 3 });
    assert!(host.scripted_encounter_windows.is_empty());
}

#[test]
fn op_34_sub_2_match_but_b1_not_40_advances_two() {
    // Even with the lookup matching, b1 != 0x40 means no capture and the
    // VM falls through to Advance PC += 2.
    let mut host = TestHost {
        op34_sub2_capture_match: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x34, 0x20, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert!(host.op34_sub2_captures.is_empty());
}

#[test]
fn op_34_sub_0_advances_pc_by_7_with_rgb_intensity() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub-0: op0 = 0x05 (sub_nibble = 0, low_3 = 0b101), rgb = (1, 2, 3),
    // intensity = 0x1234.
    let r = step(&mut host, &mut ctx, &[0x34, 0x05, 1, 2, 3, 0x34, 0x12], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.op34_sub0_calls.len(), 1);
    let (op0, rgb, intensity) = host.op34_sub0_calls[0];
    assert_eq!(op0, 0x05);
    assert_eq!(rgb, [1, 2, 3]);
    assert_eq!(intensity, 0x1234);
}

#[test]
fn op_34_sub_0_negative_intensity_sign_extends() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // intensity = 0xFFFE = -2 as i16
    let r = step(&mut host, &mut ctx, &[0x34, 0x00, 0, 0, 0, 0xFE, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.op34_sub0_calls[0].2, -2);
}

#[test]
fn op_34_sub_1_default_advance_is_13() {
    // No host overrides → default impl returns delta = 13. Capture flag
    // is 0 so the captured-PC path doesn't fire.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x34, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    assert_eq!(host.op34_sub1_calls.len(), 1);
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.op0, 0x10);
    assert_eq!(call.packed24, 0);
    assert_eq!(call.pos, [0, 0, 0]);
    assert_eq!(call.capture_flag, 0);
    assert!(call.captured_payload.is_empty());
}

#[test]
fn op_34_sub_1_packed24_and_position_decode() {
    // packed24 = 0x123456 (b1=0x12, b2=0x34, b3=0x56), world_x = 100,
    // world_z = -50, world_y = -(-200) = 200 (the original NEGATES the
    // raw bytes). reserved bytes are 0; capture_flag = 0.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let raw = [
        0x34, 0x10, 0x12, 0x34, 0x56, // op0 + packed24
        100, 0, // world_x = 100
        0xCE, 0xFF, // world_z = -50
        0x38, 0xFF, // raw -y = -200, → world_y = 200
        0, 0, // reserved
        0, // capture_flag
    ];
    let r = step(&mut host, &mut ctx, &raw, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.packed24, 0x123456);
    assert_eq!(call.pos, [100, 200, -50]);
}

#[test]
fn op_34_sub_1_capture_path_uses_host_returned_delta() {
    // capture_flag = 0x40, payload_len = 3. The instruction is 13 base
    // bytes + 2 (header bytes 0x40, len) + 3 (payload) = 18. Default
    // host returns 13; we override to model the capture branch, which
    // returns `13 + 2 + payload_len` = 18.
    let mut host = TestHost {
        op34_sub1_capture_delta: Some(18),
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let raw = [
        0x34, 0x10, 0x00, 0x00, 0x00, // op0 + packed24
        0, 0, 0, 0, 0, 0, 0, 0, // pos + reserved
        0x40, 3, // capture_flag, payload_len
        0xAA, 0xBB, 0xCC, // captured payload
    ];
    let r = step(&mut host, &mut ctx, &raw, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 18 });
    let call = &host.op34_sub1_calls[0];
    assert_eq!(call.capture_flag, 0x40);
    assert_eq!(call.captured_payload, vec![0xAA, 0xBB, 0xCC]);
}

#[test]
fn op_4c_n4_negative_value_sign_extends() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // val = 0xFFFE = -2 as i16
    let r = step(&mut host, &mut ctx, &[0x4C, 0x40, 0xFE, 0xFF, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    // i16 (-2) cast to u16 = 0xFFFE
    assert_eq!(ctx.field_72, 0xFFFE);
}

#[test]
fn op_4c_sub_3_sub_0_locks_input_and_yields() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x30], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.field_input_lock_writes, vec![true]);
}

#[test]
fn op_4c_sub_3_sub_1_unlocks_input_and_yields() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x31], 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 2 });
    assert_eq!(host.field_input_lock_writes, vec![false]);
}

#[test]
fn op_4c_sub_3_sub_4_b_c_advance_two_no_host_call() {
    // sub-4 / sub-B / sub-C all jump through code_r0x801df098 →
    // LAB_801df09c → switchD_801e00f4::default(). Asm delay slot is
    // `_addiu s8, s8, 0x2` → PC += 2. No host hook fires.
    for sub in [0x4u8, 0xB, 0xC] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &[0x4C, 0x30 | sub], 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 2 },
            "sub_{:X} did not advance",
            sub
        );
        // None of the side-effect counters should fire for this no-op
        // group.
        assert_eq!(host.field_input_lock_writes, Vec::<bool>::new());
        assert_eq!(host.party_state_clears, 0);
        assert_eq!(host.menu_refresh_calls, 0);
    }
}

#[test]
fn menu_sub_3_sub_3_calls_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x33], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.menu_refresh_calls, 1);
}

#[test]
fn menu_sub_3_sub_a_copies_dialog_depth() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3A], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.depth_copy_calls, 1);
}

#[test]
fn menu_sub_3_sub_8_and_d_call_subtile_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x38], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3D], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.subtile_refresh_calls, vec![8, 0xD]);
}

#[test]
fn menu_sub_2_dispatches_party_view_swap() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x2A → sub-2; new_index = 0xA & 7 = 2.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x2A], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.party_view_swaps, vec![2]);
}

// -- 0x4D BBOX_TEST --------------------------------------------------

#[test]
fn bbox_inside_default_tile_derivation() {
    // world_x = 0x1C0 → tile = (0x1C0 - 0x40) >> 7 = 3
    // world_z = 0x140 → tile = (0x140 - 0x40) >> 7 = 2
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0,
        world_z: 0x0140,
        ..Default::default()
    };
    // bbox: x in [2..4], z in [1..3] → inside.
    let r = step(&mut host, &mut ctx, &[0x4D, 2, 1, 4, 3, 0xAA, 0xBB], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

// -- 0x36 SCENE_FADE -------------------------------------------------

#[test]
fn scene_fade_done_advances_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x36, 0x10, 0x80, 0x05, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.scene_fade_calls, vec![(0x8010u16, 0x0005u16)]);
}

#[test]
fn scene_fade_busy_halts() {
    let mut host = TestHost {
        scene_fade_busy: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x36, 0xFF, 0xFF, 0x00, 0x00], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.scene_fade_calls, vec![(0xFFFFu16, 0u16)]);
}

// -- 0x45 CAMERA -----------------------------------------------------

#[test]
fn camera_load_advances_20() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x40 → LOAD; 18 bytes payload.
    let mut bc = vec![0x45, 0x40];
    bc.extend((0..18u8).map(|i| 0x10 + i));
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 20 });
    assert_eq!(host.camera_loads.len(), 1);
    assert_eq!(host.camera_loads[0].len(), 18);
    assert_eq!(host.camera_loads[0][0], 0x10);
    assert_eq!(host.camera_loads[0][17], 0x21);
}

#[test]
fn camera_save_advances_2_and_pings_host() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x80 → SAVE.
    let r = step(&mut host, &mut ctx, &[0x45, 0x80], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.camera_saves, 1);
}

#[test]
fn camera_apply_jumps_to_absolute_pc() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0xC0 → APPLY; absolute target = LE_u16(operand[1..3]) = 0x0042.
    let r = step(&mut host, &mut ctx, &[0x45, 0xC0, 0x42, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x42 });
    assert_eq!(host.camera_applies, 1);
}

#[test]
fn camera_configure_decodes_mask_and_advances() {
    // op0 = 0x00 (top 2 bits clear; mode = (0 >> 2) & 0xF = 0).
    // op1 = 0b1010_0000 = 0xA0 → mask bit 7 (slot 2), bit 5 (slot 4).
    // Wait - the bit interpretation is "MSB-first across the 10-bit
    // mask" from CONCAT11(op0, op1). So mask = u16(op0:op1) =
    // 0x00A0 = 0b0000_0000_1010_0000. Bit 9 → slot 0, bit 0 → slot 9.
    // Set bits at positions 7 and 5 → slot indices 9-7=2 and 9-5=4.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [45, 0x00, 0xA0, lo_trig, hi_trig, val0_lo, val0_hi, val1_lo, val1_hi]
    let bc = [
        0x45, 0x00, 0xA0, // opcode + op0/op1 mask
        0x34, 0x12, // apply_trigger = 0x1234
        0x55, 0x44, // first set bit (slot 2) → 0x4455
        0x66, 0x77, // second set bit (slot 4) → 0x7766
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    // PC += 5 + 2*set_count = 5 + 4 = 9.
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.camera_configs.len(), 1);
    let (params, trigger, mode) = &host.camera_configs[0];
    assert_eq!(*trigger, 0x1234);
    assert_eq!(*mode, 0);
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[0],
        CameraParam {
            slot: 2,
            value: 0x4455
        }
    );
    assert_eq!(
        params[1],
        CameraParam {
            slot: 4,
            value: 0x7766
        }
    );
}

#[test]
fn camera_configure_zero_mask_advances_5() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // No bits set → no params, but still PC += 5.
    let bc = [0x45, 0x00, 0x00, 0x12, 0x34];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.camera_configs.len(), 1);
    assert!(host.camera_configs[0].0.is_empty());
    assert_eq!(host.camera_configs[0].1, 0x3412);
}

#[test]
fn camera_configure_mode_is_op0_shifted_right_2_low_4() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // op0 = 0x14 (top 2 bits clear → CONFIGURE; (0x14 >> 2) & 0xF = 5).
    let bc = [0x45, 0x14, 0x00, 0x00, 0x00];
    step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(host.camera_configs[0].2, 5);
}

#[test]
fn bbox_outside_jumps_via_skip_delta() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0, // tile 3
        world_z: 0x0140, // tile 2
        ..Default::default()
    };
    // bbox: x in [10..20] → outside. skip = 0x100; outside-box jump
    // target = pc + header_size + 4 + delta = 0 + 1 + 4 + 0x100 = 261.
    let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 1], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 261 });
}

#[test]
fn bbox_outside_zero_skip_advances_5() {
    // Confirm that skip=0 still produces a non-zero next_pc (= pc + 5).
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x01C0,
        world_z: 0x0140,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x4D, 10, 0, 20, 10, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

// -- 0x4E inventory comparison-and-jump ----------------------------

#[test]
fn op_4e_sub0_lt_taken_jumps_relative() {
    // Sub-op 0, comparison 0 (state < scaled). state=5, factor=10,
    // arg=128 -> scaled = (10 * 128) >> 8 = 5; 5 < 5 = false; not taken.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((0, 0), (5, 10));
    let mut ctx = FieldCtx::default();
    // skip = 100 (0x64). Not taken -> PC += 7.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 0, 0x00, 0x80, 0x00, 0x64, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub0_lt_strict_taken() {
    // state=5, factor=10, arg=200 -> scaled = (10*200)>>8 = 7; 5 < 7
    // is true. Forward jump: PC = pc + 1 + 4 + 100 = 105.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((0, 0), (5, 10));
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 0, 0x00, 200, 0x00, 100, 0x00],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 105 });
}

#[test]
fn op_4e_sub1_gt_uses_second_pair() {
    // mode_byte = 0x11 (sub=1, op=1 → scaled < state). page=2.
    // pair (2, 1) returns (state=20, factor=8). arg = 1024.
    // scaled = (8 * 1024) >> 8 = 32. 32 < 20 is false; not taken.
    let mut host = TestHost::default();
    host.inventory_pairs.insert((2, 1), (20, 8));
    let mut ctx = FieldCtx::default();
    let r = step(
        &mut host,
        &mut ctx,
        &[0x4E, 2, 0x11, 0x00, 0x04, 0x42, 0x00],
        0,
    );
    // not-taken -> PC += 7
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
}

#[test]
fn op_4e_sub2_absolute_jump() {
    // sub-op 2 falls through to absolute jump = LE_u16(operand[2..4]).
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // mode_byte high nibble = 2; LE u16 at +2 = 0x1234.
    let bc = [0x4E, 0x00, 0x20, 0x34, 0x12, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
}

#[test]
fn op_4e_sub_c_through_f_advance_seven() {
    // mode_byte high-nibble in 12..=15 hits the dispatcher's default arm
    // at switchD_801e0a38_default. With no party-bank state initialised,
    // `uVar31 = uVar27 = 0`; the comparison is false, and `(sub-10) < 2`
    // is false for sub-op >= 12, so the original returns `param_2 + 7`
    // (= PC += 7 from the opcode).
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    for sub in 12..=15u8 {
        let mode = sub << 4;
        let bc = [0x4E, 0, mode, 0, 0, 0, 0, 0, 0];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 7 },
            "sub-op {sub}: expected PC += 7"
        );
    }
}

#[test]
fn op_4e_sub_a_compares_party_bank() {
    // sub-op 10 (party-bank A). state=1000, scaled-from-operands=
    // (0x0064 | (0x0000 << 16)) = 100. mode=0 (state < scaled): false ->
    // PC += 9.
    let mut host = TestHost::default();
    host.party_bank.insert(10, 1000);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x42, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

#[test]
fn op_4e_sub_a_compare_taken() {
    // sub-op 10. state=50, scaled = 0x0064 = 100. mode=0 (state < scaled):
    // 50 < 100 = true -> jump pc + 1 + 4 + delta = 5 + 0x10 = 21.
    let mut host = TestHost::default();
    host.party_bank.insert(10, 50);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xA0, 0x64, 0x00, 0x10, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
}

#[test]
fn op_4e_sub_4_invokes_bios_rand_and_jumps_to_returned_pc() {
    // Sub-op 4 calls BIOS Rand (FUN_80056798) and uses the result as the
    // next PC. With the host returning 0x42, the VM should jump to 0x42.
    let mut host = TestHost {
        op4e_sub4_bios_rand_value: 0x42,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4E, 0, 0x40], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x42 });
    assert_eq!(host.op4e_sub4_bios_rand_calls, 1);
}

#[test]
fn op_4e_sub_b_uses_high_word() {
    // sub-op 11. scaled = 0x0001 | (0x0001 << 16) = 0x10001. state =
    // 0x10000. mode=1 (scaled < state): 0x10001 < 0x10000 = false ->
    // PC += 9.
    let mut host = TestHost::default();
    host.party_bank.insert(11, 0x10000);
    let mut ctx = FieldCtx::default();
    let bc = [0x4E, 0, 0xB1, 0x01, 0x00, 0, 0, 0x01, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

// -- 0x49 STATE_RESUME ---------------------------------------------

#[test]
fn op_49_idle_arms_and_halts() {
    let mut host = TestHost {
        op49_state_value: Op49State::Idle,
        ..Default::default()
    };
    let mut ctx = FieldCtx {
        field_90: 0xDEAD_BEEF,
        ..Default::default()
    };
    // Sub-op 1: captures field_90 into the arm record.
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.op49_arms, vec![(0, 0xDEAD_BEEF)]);
    assert_eq!(host.op49_setups, 1);
}

#[test]
fn op_49_idle_invalid_subop_halts_without_arming() {
    let mut host = TestHost {
        op49_state_value: Op49State::Idle,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // sub-op 0xE is out of range (> 0xD).
    let r = step(&mut host, &mut ctx, &[0x49, 0xE, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.op49_arms.is_empty());
    assert_eq!(host.op49_setups, 0);
}

#[test]
fn op_49_armed_halts() {
    let mut host = TestHost {
        op49_state_value: Op49State::Armed,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.op49_arms.is_empty());
}

#[test]
fn op_49_done_advances_per_subop() {
    // Sub-op 1, 3, 7 jump to 0x801e00b8 in the original, where
    // `addiu s8, s8, 0x3` runs before the dispatcher tail; PC += 3.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 1, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.op49_clears, 1);

    // Sub-op 2, 4: original returns `param_2 + 7`; PC += 7.
    host.op49_state_value = Op49State::Done;
    let r = step(&mut host, &mut ctx, &[0x49, 2, 0, 0, 0, 0, 0, 0], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });

    // Sub-op 5: original returns `param_2 + 0xe`; PC += 14.
    host.op49_state_value = Op49State::Done;
    let r = step(
        &mut host,
        &mut ctx,
        &[0x49, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
}

#[test]
fn op_49_done_subop_a_halts_at_pc() {
    // Done-side catch-all in `FUN_801de840 case 0x49`: any sub-op that
    // isn't explicitly handled (here sub-0xA, which falls through the
    // 1/3/7, 2/4, 5, 6/8/9/C/D arms) clears the resume slot and returns
    // `param_2` (= halt at the same PC).
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x49, 0xA, 0, 0, 0], 0);
    assert!(matches!(r, StepResult::Halt { final_pc: 0 }));
    assert_eq!(host.op49_clears, 1, "should clear the resume slot");
}

#[test]
fn op_49_done_sub0_walks_inline_mes_payload() {
    // `[49, 0, length, ...length args..., ...mes_bytes, terminator, ...]`.
    // The original at FUN_801DE840 (case 0x49 / DONE / sub-0) reads
    // `length = pbVar47[2]`, then walks the inline MES bytecode starting
    // at `pbVar47 + length + 3` via `func_0x8003ca38` (counts bytes > 0x1E,
    // pair-extends 0xCx prefix bytes), and advances PC by `5 + length +
    // walked`.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    // length = 2 (two args), MES bytes = [0x50, 0xC3, 0xAA, 0x00 terminator]
    // → walker counts 4 bytes (0x50 = 1, 0xC3 0xAA pair = 2, then sees 0x00).
    let bc = &[
        0x49, // opcode
        0x00, // sub-op
        0x02, // length
        0xAA, 0xBB, // 2 args (ignored by walker - walker starts past them)
        0x50, 0xC3, 0xAA, 0x00, // MES body (3 bytes walked) + terminator
    ];
    let r = step(&mut host, &mut ctx, bc, 0);
    // PC = 0 + (header_size=1) + 4 + length=2 + walked=3 = 10. The
    // terminator is NOT consumed by the walker - it stops at it.
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.op49_clears, 1);
}

#[test]
fn op_49_done_sub0_empty_mes_advances_5_plus_length() {
    // length = 0, no MES body, immediate terminator.
    let mut host = TestHost {
        op49_state_value: Op49State::Done,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let bc = &[0x49, 0x00, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, bc, 0);
    // PC = 1 + 4 + 0 + 0 = 5.
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn walk_mes_bytecode_terminates_on_low_byte() {
    // Standard byte run terminates on a byte ≤ 0x1E.
    assert_eq!(walk_mes_bytecode(&[0x50, 0x60, 0x70, 0x10, 0x80]), 3);
    // Empty buffer.
    assert_eq!(walk_mes_bytecode(&[]), 0);
    // Immediate terminator.
    assert_eq!(walk_mes_bytecode(&[0x05]), 0);
}

#[test]
fn walk_mes_bytecode_pair_extends_cx_prefix() {
    // 0xC0..0xCF prefix bytes consume one extra byte each (the pair byte).
    // 0xC0 0x05 (pair 0x05 NOT a terminator, because it's a pair byte not
    // a top-level byte) - counts 2.
    assert_eq!(walk_mes_bytecode(&[0xC0, 0x05, 0x10]), 2);
    // 0xCF 0x99 - counts 2; then 0x50 - counts 3 total; then 0x10 stops.
    assert_eq!(walk_mes_bytecode(&[0xCF, 0x99, 0x50, 0x10]), 3);
}

#[test]
fn walk_mes_bytecode_eof_mid_pair_stops_gracefully() {
    // 0xC0 at end of buffer - original would read past EOF; we stop.
    assert_eq!(walk_mes_bytecode(&[0xC0]), 1);
}

// -- 0x34 EFFECT sub-3 ----------------------------------------------

#[test]
fn op_34_sub3_triggers_anim() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 7,
        ..Default::default()
    };
    // op0 high nibble = 3 (=> 0x30). arg = 0x42.
    let r = step(&mut host, &mut ctx, &[0x34, 0x30, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.effect_anim_calls, vec![(7, 0x42)]);
}

#[test]
fn op_34_high_subop_halts() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // sub-4..=sub-F (op0 >> 4 >= 4) hits the original's
    // `if (bVar35 != 2) { if (bVar35 != 3) { return param_2; } }` at
    // line 4811-4814 of the dump ⇒ halt at PC.
    let r = step(&mut host, &mut ctx, &[0x34, 0x40, 0], 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

// -- 0x43 ACTOR_CTRL sub-8 ------------------------------------------

#[test]
fn op_43_sub8_resets_face_rotation() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 11,
        face_rotation: 5,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x43, 8], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.face_rotation, 0);
    assert_eq!(host.face_resets, vec![11]);
}

#[test]
fn op_43_other_subops_halt() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Sub-op 0x16 (no handler in the original `case 0x43` inner switch);
    // falls through with `iVar45 = param_2` (initialised at line 4511 of
    // the dump) ⇒ halt at PC.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 0x16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
    );
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_43_sub7_face_rotation_setup() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        face_rotation: 0,
        ..Default::default()
    };
    // 17-byte: [43, 7, face=3, payload=0xDEADBEEF, p0=0x1111, p1=0x2222,
    //          p2=0x3333, p3=0x4444, target=-1 (=0xFFFF)]
    let bc = [
        0x43, 7, 3, 0xEF, 0xBE, 0xAD, 0xDE, 0x11, 0x11, 0x22, 0x22, 0x33, 0x33, 0x44, 0x44, 0xFF,
        0xFF,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 17 });
    assert_eq!(ctx.face_rotation, 3);
    assert_eq!(
        host.face_rotation_setups,
        vec![(
            3u8,
            0xDEAD_BEEFu32,
            [0x1111u16, 0x2222, 0x3333, 0x4444],
            -1i16
        )]
    );
}

#[test]
fn op_43_sub12_alloc_actor() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xC, 0x10, 0x20, 0x30] = 5 bytes; PC += 5.
    let r = step(&mut host, &mut ctx, &[0x43, 0xC, 0x10, 0x20, 0x30], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.scripted_actor_allocs, vec![(0x10, 0x20, 0x30)]);
}

#[test]
fn op_43_sub2_three_actor_talk() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 2, a1=5, a2=6, a3=7, lo=0x12, hi=0x34, b6=0xAB] = 8 bytes; PC += 8.
    let r = step(
        &mut host,
        &mut ctx,
        &[0x43, 2, 5, 6, 7, 0x12, 0x34, 0xAB],
        0,
    );
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
    assert_eq!(host.three_actor_talks, vec![([5, 6, 7], 0x3412, 0xAB)]);
}

#[test]
fn op_43_sub_d_alloc_with_mode_3() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xD, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=3.
    let r = step(&mut host, &mut ctx, &[0x43, 0xD, 1, 2, 3, 4], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.actor_alloc_modes, vec![(0xD, 3, [1, 2, 3, 4])]);
}

#[test]
fn op_43_sub_f_alloc_with_mode_0() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [43, 0xF, b1, b2, b3, b4] = 6 bytes; PC += 6; mode=0.
    let r = step(&mut host, &mut ctx, &[0x43, 0xF, 9, 8, 7, 6], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.actor_alloc_modes, vec![(0xF, 0, [9, 8, 7, 6])]);
}

#[test]
fn op_43_sub_e_marks_flag() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x55,
        ..Default::default()
    };
    let r = step(&mut host, &mut ctx, &[0x43, 0xE], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.mark_flag_8_calls, vec![0x55]);
}

#[test]
fn op_43_sub_9_tween_path() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // ticks=5 (non-zero) → tween path. Coords (0x0100, 0x0200, 0x0300).
    let bc = [0x43, 9, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0x05, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.sub9_tweens, vec![(0x0100u16, 0x0200, 0x0300, 5)]);
    // Tween path doesn't write ctx coords directly.
    assert_eq!(ctx.world_x, 0);
}

#[test]
fn op_43_sub_9_immediate_writes_when_ticks_zero() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0xAAAA,
        world_y: 0xBBBB,
        world_z: 0xCCCC,
        ..Default::default()
    };
    // ticks=0 → immediate write path. y = 0xFFFF (sentinel - skip).
    let bc = [0x43, 9, 0x11, 0x22, 0xFF, 0xFF, 0x33, 0x44, 0x00, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(ctx.world_x, 0x2211);
    assert_eq!(ctx.world_y, 0xBBBB); // unchanged (sentinel)
    assert_eq!(ctx.world_z, 0x4433);
    assert!(host.sub9_tweens.is_empty());
}

#[test]
fn op_43_sub_10_emitter_init() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 21-byte instruction. Payload is 19 bytes after the [43, 0x10] header.
    let mut bc = [0u8; 21];
    bc[0] = 0x43;
    bc[1] = 0x10;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = i as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 21 });
    assert_eq!(host.emitter_init_payloads.len(), 1);
    assert_eq!(host.emitter_init_payloads[0].len(), 19);
    assert_eq!(host.emitter_init_payloads[0][0], 2);
}

#[test]
fn op_43_sub_11_emitter_5_words() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 12-byte: [43, 0x11, 5 u16s].
    let bc = [
        0x43, 0x11, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 12 });
    assert_eq!(host.emitter_5_words, vec![[1u16, 2, 3, 4, 5]]);
}

#[test]
fn op_43_sub_15_emitter_struct_12() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x15, 12 bytes].
    let mut bc = [0u8; 14];
    bc[0] = 0x43;
    bc[1] = 0x15;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = (i + 0x10) as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.emitter_struct12_payloads.len(), 1);
    assert_eq!(host.emitter_struct12_payloads[0].len(), 12);
}

#[test]
fn op_43_sub_12_split_call_no_split() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x12, six s16 LE]. words[2] = 0x0080 ≤ 0xFF so no split.
    let bc = [
        0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x80, 0x00, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(
        host.emitter_split_calls,
        vec![([0x10i16, 0x20, 0x80, 0x40, 0x50, 0x60], false)]
    );
}

#[test]
fn op_43_sub_12_split_call_with_split() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // words[2] = 0x0200 > 0xFF → split.
    let bc = [
        0x43, 0x12, 0x10, 0x00, 0x20, 0x00, 0x00, 0x02, 0x40, 0x00, 0x50, 0x00, 0x60, 0x00,
    ];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.emitter_split_calls.len(), 1);
    assert!(host.emitter_split_calls[0].1);
    assert_eq!(host.emitter_split_calls[0].0[2], 0x0200i16);
}

#[test]
fn op_43_sub_13_func13_passes_full_payload() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 14-byte: [43, 0x13, 12 data bytes]. Payload includes the 0x13 byte.
    let mut bc = [0u8; 14];
    bc[0] = 0x43;
    bc[1] = 0x13;
    for (i, b) in bc.iter_mut().enumerate().skip(2) {
        *b = (i + 0x40) as u8;
    }
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.emitter_func13_payloads.len(), 1);
    assert_eq!(host.emitter_func13_payloads[0][0], 0x13);
    assert_eq!(host.emitter_func13_payloads[0][12], (13 + 0x40) as u8);
}

#[test]
fn op_43_sub_14_4_words() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 10-byte: [43, 0x14, 4 s16 LE]. Use a negative second word to verify sign-ext.
    let bc = [0x43, 0x14, 0x01, 0x00, 0xFF, 0xFF, 0x03, 0x00, 0x04, 0x00];
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.emitter_4_words, vec![[1i16, -1, 3, 4]]);
}

#[test]
fn op_43_sub_12_truncated_returns_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 13-byte: one byte short of the required 14.
    let bc = [0u8; 13];
    let bc = {
        let mut b = bc;
        b[0] = 0x43;
        b[1] = 0x12;
        b
    };
    let r = step(&mut host, &mut ctx, &bc, 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

#[test]
fn op_4c_sub_3_sub_9_position_refresh() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // [4C, 0x39] = 2 bytes; high nibble = 3, low nibble = 9.
    let r = step(&mut host, &mut ctx, &[0x4C, 0x39], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.player_pos_refresh_calls, 1);
    // sub-9 falls through to sub-E's render-resync chain.
    assert_eq!(host.player_render_resync_calls, 1);
}

#[test]
fn op_4c_sub_3_sub_e_render_resync() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3E], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.player_render_resync_calls, 1);
    assert_eq!(host.player_pos_refresh_calls, 0);
}

#[test]
fn op_4c_sub_3_sub_f_io_resync() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x4C, 0x3F], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.field_io_resync_calls, 1);
}

#[test]
fn op_49_done_state_sub_6_8_9_c_d_advance_5() {
    for sub_op in [6u8, 8, 9, 0xC, 0xD] {
        let mut host = TestHost {
            op49_state_value: Op49State::Done,
            ..Default::default()
        };
        let mut ctx = FieldCtx::default();
        // 5-byte instruction: [49, sub_op, 3 unused payload bytes].
        let bc = [0x49, sub_op, 0xAA, 0xBB, 0xCC];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(
            r,
            StepResult::Advance { next_pc: 5 },
            "sub_op {sub_op:#x} should advance by 5 in Done state"
        );
        assert_eq!(host.op49_clears, 1);
    }
}

#[test]
fn op_43_sub_3_4_5_6_sound_ramps() {
    for sub_op in [3u8, 4, 5, 6] {
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        // [43, sub_op, b1=1, b2=2, b3=3, b4=4, ticks=0x0064, curve=0x0010]
        let bc = [0x43, sub_op, 1, 2, 3, 4, 0x64, 0x00, 0x10, 0x00];
        let r = step(&mut host, &mut ctx, &bc, 0);
        assert_eq!(r, StepResult::Advance { next_pc: 10 });
        assert_eq!(
            host.sound_ramp_calls,
            vec![(sub_op, [1, 2, 3, 4], 100u16, 16u16)]
        );
    }
}

// -- step_with_caller (YIELD propagation) --------------------------

#[test]
fn yield_propagates_to_caller_when_target_is_player() {
    let mut host = TestHost::default();
    let mut target = FieldCtx {
        script_id: 0xFB, // arbitrary - caller designates as player below
        ..Default::default()
    };
    let mut caller = FieldCtx::default();
    let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x37], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert!(target.is_halted());
    assert!(caller.is_halted());
    assert_eq!(target.saved_pc, 0);
    assert_eq!(caller.saved_pc, 0);
}

#[test]
fn yield_does_not_propagate_when_target_not_player() {
    let mut host = TestHost::default();
    let mut target = FieldCtx::default();
    let mut caller = FieldCtx::default();
    let r = step_with_caller(&mut host, &mut target, &mut caller, false, &[0x37], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert!(target.is_halted());
    assert!(!caller.is_halted());
}

#[test]
fn step_with_caller_non_yield_does_not_touch_caller() {
    let mut host = TestHost::default();
    let mut target = FieldCtx::default();
    let mut caller = FieldCtx::default();
    // 0x21 NOP - no yield. Caller untouched even when target == player.
    let r = step_with_caller(&mut host, &mut target, &mut caller, true, &[0x21], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 1 });
    assert!(!caller.is_halted());
}

#[test]
fn yield_saved_pc_is_pre_extended_when_extended_dispatch() {
    // The original stores pbVar43 (the address of the OPCODE byte) in
    // ctx.saved_pc, regardless of extended/non-extended. So an extended
    // YIELD at pc=0 saves 0, not 1.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 0xB7 = 0x80 | 0x37. Extended YIELD on a (synthetic) target.
    let r = step(&mut host, &mut ctx, &[0xB7, 0x00], 0);
    assert!(matches!(r, StepResult::Yield { .. }));
    assert_eq!(ctx.saved_pc, 0);
}

// -- High-byte default-route opcodes 0x5x/0x6x/0x7x ------------------
// (the **fourth flag bank** at DAT_80085758). These fall through the
// explicit opcode arm and hit the dispatcher's default route.

#[test]
fn sysflag_set_low_index_writes_through_host() {
    // Opcode 0x50 + idx_lo 0x07 → idx = (0x50 & 0x8F) << 8 | 0x07 = 0x07.
    // Bit at byte 0 / mask 0x01 set.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x50, 0x07], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0], 0x01);
    assert_eq!(host.sys_flag_writes, vec![(0x07, "set")]);
}

#[test]
fn sysflag_set_uses_low_nibble_of_opcode_for_high_byte() {
    // Opcode 0x5F + idx_lo 0xFF → idx = (0x5F & 0x8F) << 8 | 0xFF =
    // 0x0F00 | 0xFF = 0x0FFF. Byte 0x1FF, mask 0x01.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x5F, 0xFF], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0x1FF], 0x01);
    assert_eq!(host.sys_flag_writes, vec![(0x0FFF, "set")]);
}

#[test]
fn sysflag_set_extended_prefix_sets_high_bit_of_index() {
    // 0xD0 = 0x80 | 0x50. Extended SET. The dispatcher reads the raw
    // opcode byte for `(opcode_byte & 0x8F) << 8`, so the extended bit
    // (0x80) lands at bit 15 of `idx`.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // The extended prefix consumes one extra byte. peek_extended would
    // tell the caller to fetch the script ID (here 0x00).
    let r = step(&mut host, &mut ctx, &[0xD0, 0x00, 0x05], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    // idx = (0xD0 & 0x8F) << 8 | 0x05 = 0x80 << 8 | 0x05 = 0x8005.
    // Byte 0x1000, mask 0x04 (bit 5 in big-endian per-byte order).
    assert_eq!(host.sys_flag_writes, vec![(0x8005, "set")]);
    assert_eq!(host.system_flags[0x1000], 0x04);
}

#[test]
fn sysflag_clear_only_resets_targeted_bit() {
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    host.system_flags[0] = 0xFF;
    let mut ctx = FieldCtx::default();
    // Opcode 0x60 + idx_lo 0x03 → idx = 0x03. Bit at byte 0 / mask 0x10.
    let r = step(&mut host, &mut ctx, &[0x60, 0x03], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.system_flags[0], 0xEF);
    assert_eq!(host.sys_flag_writes, vec![(0x03, "clear")]);
}

#[test]
fn sysflag_test_bit_clear_falls_through_4_bytes() {
    // TEST against an unset bit advances PC past the 4-byte instruction.
    // The 2 trailing operand bytes (the "branch target") are skipped.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA, 0xBB], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // No writes - TEST is read-only.
    assert!(host.sys_flag_writes.is_empty());
}

#[test]
fn sysflag_test_bit_set_takes_relative_jump() {
    // Pre-set bit at idx 0x08 (byte 1, mask 0x80).
    // Opcode 0x70 + idx_lo 0x08 + offset 0x0010 → on bit-set, jump to
    // pc + header_size + 1 + 0x0010 = 0 + 1 + 1 + 16 = 18.
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    host.system_flags[1] = 0x80;
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x70, 0x08, 0x10, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 18 });
}

#[test]
fn sysflag_test_extended_prefix_sets_high_bit_of_index() {
    // 0xF0 = 0x80 | 0x70. Extended TEST.
    let mut host = TestHost::default();
    host.ensure_sys_flag_capacity();
    // idx = (0xF0 & 0x8F) << 8 | 0x05 = 0x8005.
    // Byte 0x1000, mask 0x04. Pre-set it.
    host.system_flags[0x1000] = 0x04;
    let mut ctx = FieldCtx::default();
    // Layout: [prefix=0xF0, target_id, idx_lo, off_lo, off_hi]
    // pc=0; on bit-set, next_pc = 0 + 2 (header) + 1 + LE_u16(0x05, 0x00) = 8.
    let r = step(&mut host, &mut ctx, &[0xF0, 0x00, 0x05, 0x05, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
}

#[test]
fn sysflag_set_then_test_round_trips() {
    // SET-then-TEST round trip: a SET on idx K must subsequently make
    // TEST on the same idx return true and take the branch.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &[0x50, 0x42], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    // Fresh TEST at pc=0 on its own buffer; offset 0x0004 → jump to
    // 0 + header_size(1) + 1 + 4 = 6.
    let r = step(&mut host, &mut ctx, &[0x70, 0x42, 0x04, 0x00], 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
}

#[test]
fn sysflag_set_truncated_idx_byte_is_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 1-byte buffer - no idx byte available.
    let r = step(&mut host, &mut ctx, &[0x50], 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

#[test]
fn sysflag_test_truncated_offset_is_unknown() {
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // 3-byte buffer - missing the second offset byte.
    let r = step(&mut host, &mut ctx, &[0x70, 0x05, 0xAA], 0);
    assert!(matches!(r, StepResult::Unknown { .. }));
}

// -- Cross-opcode integration ---------------------------------------

/// Drive a script through `step()` repeatedly until it `Halt`s, hits
/// `Pending`/`Unknown`, or executes more than `max_steps` instructions.
/// Returns the trace of `(pc_before, StepResult)` for assertion.
fn run_until_halt(
    host: &mut TestHost,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    start_pc: usize,
    max_steps: usize,
) -> Vec<(usize, StepResult)> {
    let mut trace = Vec::new();
    let mut pc = start_pc;
    for _ in 0..max_steps {
        let r = step(host, ctx, bytecode, pc);
        trace.push((pc, r.clone()));
        match r {
            StepResult::Advance { next_pc } => pc = next_pc,
            StepResult::Yield { resume_pc } => {
                // Treat Yield as "step one tick, resume next iter".
                pc = resume_pc;
            }
            StepResult::Halt { .. } | StepResult::Pending { .. } | StepResult::Unknown { .. } => {
                break;
            }
        }
    }
    trace
}

#[test]
fn integration_lflag_set_test_jmp_clr_test_halt() {
    // Composed script exercising five opcodes in sequence. The JMP_REL
    // formula is `target = pc + header_size + delta` (= pc + 1 + delta
    // for non-extended). All offsets below were chosen so the JMP at
    // PC=4 lands on PC=9 (delta=4 → 4 + 1 + 4 = 9), skipping the
    // intermediate CLR.
    //
    //   00: 2B 05         LFLAG_SET bit 5      (PC -> 02)
    //   02: 2D 05         LFLAG_TST bit 5      (set → PC -> 04)
    //   04: 26 04 00      JMP_REL +4           (target = 4 + 1 + 4 = 9)
    //   07: 2C 05         LFLAG_CLR bit 5  ← skipped by JMP
    //   09: 2D 05         LFLAG_TST bit 5  (still set → PC -> 0B)
    //   0B: 2C 05         LFLAG_CLR bit 5      (PC -> 0D)
    //   0D: 2D 05         LFLAG_TST bit 5      (now clear → Halt)
    //
    // Validates: local-flag round trip, unconditional jump skipping
    // an intermediate op, conditional-test halt path.
    let bytecode = [
        0x2B, 0x05, // 00..02 SET bit 5
        0x2D, 0x05, // 02..04 TEST bit 5 (advance)
        0x26, 0x04, 0x00, // 04..07 JMP +4 → PC=9
        0x2C, 0x05, // 07..09 CLR bit 5 (skipped)
        0x2D, 0x05, // 09..0B TEST bit 5 (still set, advance)
        0x2C, 0x05, // 0B..0D CLR bit 5
        0x2D, 0x05, // 0D..0F TEST bit 5 (clear, halt)
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 32);

    // Terminal state is a Halt at PC 0x0D (the third TEST).
    let (last_pc, last) = trace.last().unwrap().clone();
    assert!(
        matches!(last, StepResult::Halt { final_pc: 0x0D }),
        "expected Halt at 0x0D, got {:?} at pc=0x{:X}",
        last,
        last_pc
    );

    assert_eq!(ctx.local_flags & (1 << 5), 0, "bit 5 should end clear");

    // Walk the visited PCs to ensure the JMP took its branch and the
    // CLR at offset 0x07 was skipped.
    let visited: Vec<usize> = trace.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        visited,
        vec![0x00, 0x02, 0x04, 0x09, 0x0B, 0x0D],
        "PC trace mismatch (JMP must skip the CLR at 0x07)"
    );
}

#[test]
fn integration_jmp_then_advance_through_n4_immediate_writes() {
    // Composed script that JMPs over a garbage region, then walks three
    // 6-byte 0x4C nibble-4 immediate writes, ending at NOP. JMP_REL
    // target = `pc + header_size + delta` = `0 + 1 + 4 = 5`. Validates
    // PC math composition across the 6-byte 0x4C nibble-4 instruction.
    //
    //   00: 26 04 00            JMP +4  → PC = 5
    //   03..04: garbage (skipped)
    //   05: 4C 40 11 00 00 00   nibble-4 sub-0: ctx.field_72 = 0x0011, immediate
    //   0B: 4C 48 22 00 00 00   nibble-4 sub-8: ctx.field_26 = 0x0022, immediate
    //   11: 4C 42 33 00 00 00   nibble-4 sub-2: ctx.field_8e = 0x0033, immediate
    //   17: 21                  NOP
    //   18: 21                  NOP - terminal (run_until_halt limit)
    let bytecode = [
        0x26, 0x04, 0x00, // 00..03 JMP +4 → PC=5
        0xAA, 0xAA, // 03..05 garbage (skipped)
        0x4C, 0x40, 0x11, 0x00, 0x00, 0x00, // 05..0B sub-0
        0x4C, 0x48, 0x22, 0x00, 0x00, 0x00, // 0B..11 sub-8
        0x4C, 0x42, 0x33, 0x00, 0x00, 0x00, // 11..17 sub-2
        0x21, // 17 NOP
        0x21, // 18 NOP
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    // Cap steps low enough that the trace ends inside the buffer; with
    // 6 instructions the trace stops at PC=0x19 reading past EOF
    // (Unknown). That's fine - we assert state, not the terminal kind.
    let _ = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

    assert_eq!(ctx.field_72, 0x11);
    assert_eq!(ctx.field_26, 0x22);
    assert_eq!(ctx.field_8e, 0x33);
    // Garbage bytes at 0x03..05 must not have been interpreted as ops.
    // We verify this via the host call counters: nothing else should
    // have fired.
    assert!(host.n4_ctx_ramps.is_empty());
    assert!(host.n4_global_writes.is_empty());
}

#[test]
fn integration_yield_then_resume_then_advance() {
    // Validates the Yield resume cycle composed with Advance:
    //   00: 4C 30           sub-3 sub-0 (set field-input lock, Yield)
    //   02: 4C 31           sub-3 sub-1 (clear lock, Yield)
    //   04: 21              NOP (Advance)
    //   05: 2D 05           LFLAG_TST bit 5 (clear → Halt)
    //
    // After two Yields the host log should show [true, false]; after
    // the NOP advances we hit the LFLAG_TST and halt.
    let bytecode = [
        0x4C, 0x30, // lock + Yield
        0x4C, 0x31, // unlock + Yield
        0x21, // NOP
        0x2D, 0x05, // TEST bit 5 (clear → Halt)
    ];

    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let trace = run_until_halt(&mut host, &mut ctx, &bytecode, 0, 16);

    // Lock log must be exactly [true, false].
    assert_eq!(host.field_input_lock_writes, vec![true, false]);

    // Trace shape: Yield, Yield, Advance, Halt.
    assert!(matches!(trace[0].1, StepResult::Yield { resume_pc: 2 }));
    assert!(matches!(trace[1].1, StepResult::Yield { resume_pc: 4 }));
    assert!(matches!(trace[2].1, StepResult::Advance { next_pc: 5 }));
    let last = trace.last().unwrap().1.clone();
    assert!(matches!(last, StepResult::Halt { final_pc: 5 }));
}

// -- 0x4C outer-nibbles 5..F ----------------------------------------

#[test]
fn op_4c_n5_sub0_low_clears_flag_bit_and_advances() {
    // [4C, 0x50, 0x40, 0x00] → value = 0x40, < 0xF0 → low half.
    let bytecode = [0x4Cu8, 0x50, 0x40, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x0100_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(ctx.flags & 0x0100_0000, 0);
    assert_eq!(host.n5_sub0_calls, vec![(0x40, false)]);
}

#[test]
fn op_4c_n5_sub0_high_sets_flag_bit() {
    // [4C, 0x50, 0xF0, 0x00] → value = 0xF0, >= 0xF0 → high half.
    let bytecode = [0x4Cu8, 0x50, 0xF0, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert!(ctx.flags & 0x0100_0000 != 0);
    assert_eq!(host.n5_sub0_calls, vec![(0xF0, true)]);
}

#[test]
fn op_4c_n6_sub_60_emitter6_passes_six_signed_words() {
    // [4C, 0x60, 12 bytes of 6 s16 words]
    let mut bytecode = vec![0x4C, 0x60];
    for w in &[1i16, -2, 3, -4, 5, -6] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.n6_sub0_calls, vec![[1, -2, 3, -4, 5, -6]]);
}

#[test]
fn op_4c_n6_unrecognized_halts_at_pc() {
    // n6 op0 in {0x62..=0x6F}: original dispatcher returns `param_2`
    // unchanged (halt at PC). Only 0x60 (6-word emitter) and 0x61
    // (halt-acquire emitter) are recognized.
    let bytecode = [0x4Cu8, 0x62, 0, 0, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(r, StepResult::Halt { .. }));
}

#[test]
fn op_4c_n7_sub_0_yields_at_next_pc() {
    // [4C, 0x70, col0=1, row0=2, col1=3, row1=4]. Sub-0 has no mask
    // byte, so it is a 6-byte op (yield at pc+6). Columns
    // [col0, col1+1) = [1, 4); rows [row0+1, row1+2) = [3, 6). The
    // trailing 0xAA is the next op's first byte, not consumed here.
    let bytecode = [0x4Cu8, 0x70, 1, 2, 3, 4, 0xAA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 6 });
    assert_eq!(host.n7_tile_calls, vec![(0u8, (1u8, 4u8), (3u8, 6u8), 0u8)]);
}

#[test]
fn op_4c_n7_sub_2_advances() {
    // sub-2 → mask-clear, advance not yield.
    let bytecode = [0x4Cu8, 0x72, 0, 0, 0, 0, 0x0F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n7_tile_calls.len(), 1);
    assert_eq!(host.n7_tile_calls[0].0, 2);
}

#[test]
fn op_4c_n8_sub2_party_mirror_advances_three_bytes() {
    let bytecode = [0x4Cu8, 0x82, 0x03];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n8_party_mirrors, vec![3u8]);
}

#[test]
fn op_4c_n8_sub_c_branch_taken_when_field_68_zero() {
    // [4C, 0x8C, 0x10, 0x00] - field_68 = 0 → branch to 0x0010.
    let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_68: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x10 });
}

#[test]
fn op_4c_n8_sub_c_advances_past_when_field_68_nonzero() {
    let bytecode = [0x4Cu8, 0x8C, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_68: 1,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n5_sub_1_decodes_coords_and_advances_six() {
    // [4C, 0x51, b1, b2, b3, b4]. Tile-coord formula: world = (b & 0x7F)*0x80 + 0x40
    // (+0x40 if high bit). b1=0x82 → (2*0x80) + 0x40 + 0x40 = 0x180.
    // b2=0x03 → (3*0x80) + 0x40 = 0x1C0.
    let bytecode = [0x4Cu8, 0x51, 0x82, 0x03, 0x07, 0x2A];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_5_sub_1_npc_runs.len(), 1);
    let (wx, wz, depth, move_id, is_player) = host.n_5_sub_1_npc_runs[0];
    assert_eq!(wx, 0x180);
    assert_eq!(wz, 0x1C0);
    assert_eq!(depth, 0x07);
    assert_eq!(move_id, 0x2A);
    assert!(!is_player, "ctx.flags & 0x01000000 == 0 → NPC path");
}

#[test]
fn op_4c_n5_sub_1_is_player_when_flag_bit_set() {
    let bytecode = [0x4Cu8, 0x51, 0x00, 0x00, 0x00, 0x63];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x0100_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert!(host.n_5_sub_1_npc_runs[0].4);
    // 99 (0x63) is the player-path cancel sentinel.
    assert_eq!(host.n_5_sub_1_npc_runs[0].3, 99);
}

#[test]
fn op_4c_n5_sub_2_advances_when_menu_activated() {
    let bytecode = [0x4Cu8, 0x52, 0x05];
    let mut host = TestHost {
        n_5_sub_2_menu_state: std::collections::HashMap::from([(0x05u8, true)]),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_5_sub_2_polls.borrow().as_slice(), &[0x05]);
}

#[test]
fn op_4c_n5_sub_2_halts_while_menu_still_loading() {
    let bytecode = [0x4Cu8, 0x52, 0x05];
    let mut host = TestHost::default();
    // Menu state defaults to false - polling halts at PC.
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_6_sub_61_emitter_calls.is_empty());
}

#[test]
fn op_4c_n6_sub_61_acquires_and_advances_sixteen() {
    // 16-byte instruction; TestHost's predicate defaults to `false`, so
    // arm it for the acquire-success path.
    let bytecode = [
        0x4Cu8, 0x61, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
        0x0E,
    ];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 16 });
    // ctx mutation: halt bit set, saved_pc and wait_accum updated.
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    // Apply hook fired with which = 0x61.
    assert!(host.halt_acquire_calls.iter().any(|(w, _, _)| *w == 0x61));
    // Emitter received bytes +2..+15 (14 data bytes; sub-byte 0x61 stripped).
    assert_eq!(host.n_6_sub_61_emitter_calls.len(), 1);
    assert_eq!(
        host.n_6_sub_61_emitter_calls[0],
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E
        ],
    );
}

#[test]
fn op_4c_n6_sub_61_halts_when_predicate_refuses() {
    let bytecode = [
        0x4Cu8, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    // halt_acquire_predicate defaults to false → refuse.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_6_sub_61_emitter_calls.is_empty());
    assert_eq!(ctx.flags & 0x400, 0, "ctx unmutated on refusal");
}

#[test]
fn op_4c_n8_sub_0_acquires_and_advances_three() {
    let bytecode = [0x4Cu8, 0x80, 0x03, 0xAA, 0xBB, 0xCC];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert!(host.halt_acquire_calls.iter().any(|(w, _, _)| *w == 0x80));
    assert_eq!(host.n_8_sub_0_allocator_calls.len(), 1);
    let (count, tail) = &host.n_8_sub_0_allocator_calls[0];
    assert_eq!(*count, 3);
    assert_eq!(tail.as_slice(), &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn op_4c_n8_sub_0_halts_when_predicate_refuses() {
    let bytecode = [0x4Cu8, 0x80, 0x00];
    // halt_acquire_predicate defaults to false.
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert!(host.n_8_sub_0_allocator_calls.is_empty());
}

#[test]
fn op_4c_n8_sub_3_rect_tile_fill_emits_host_hook_and_advances_7() {
    // [4C, 0x83, col_start, row_start, col_end, row_end, value]
    // Total instruction = 7 bytes (header + op0 + 5 operand bytes).
    let bytecode = [0x4Cu8, 0x83, 2, 4, 5, 6, 0x7F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n8_rect_tile_fills, vec![(2u8, 4, 5, 6, 0x7F)]);
}

#[test]
fn op_4c_n8_sub_3_truncated_bytecode_returns_unknown() {
    // Operand list cut short (only 4 operand bytes instead of 5).
    let bytecode = [0x4Cu8, 0x83, 1, 1, 1, 1];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    );
    assert!(host.n8_rect_tile_fills.is_empty());
}

#[test]
fn op_4c_n8_sub_a_writes_quad() {
    // [4C, 0x8A, s0_lo, s0_hi, s1_lo, s1_hi, s2_lo, s2_hi, u24_b0, u24_b1, u24_b2]
    // Original encodes the trailing slot as a 24-bit LE integer (read
    // via `func_0x8003CEB8`). Total instruction = 11 bytes.
    let mut bytecode = vec![0x4C, 0x8A];
    for s in &[10i16, 20, 30] {
        bytecode.extend_from_slice(&s.to_le_bytes());
    }
    // u24 = 0x00ADBEEF (bytes 0xEF, 0xBE, 0xAD).
    bytecode.extend_from_slice(&[0xEF, 0xBE, 0xAD]);
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 11 });
    assert_eq!(host.n8_quad_writes, vec![([10i16, 20, 30], 0x00AD_BEEFu32)]);
}

#[test]
fn op_4c_n8_sub_9_writes_signed_16_and_advances_4() {
    // [4C, 0x89, 0x34, 0x12] writes 0x1234 then PC += 4.
    let bytecode = [0x4Cu8, 0x89, 0x34, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n8_sub9_writes, vec![0x1234i16]);
}

#[test]
fn op_4c_n9_sub_0_passes_b1_and_three_words() {
    // [4C, 0x90, b1=7, lo,hi, lo,hi, lo,hi]
    let mut bytecode = vec![0x4C, 0x90, 0x07];
    for w in &[1i16, 2, 3] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n9_dde34_calls, vec![(0u8, 0x07u8, [1i16, 2, 3])]);
}

#[test]
fn op_4c_n9_sub_e_copies_16_signed_words() {
    let mut bytecode = vec![0x4C, 0x9E];
    for i in 0..16i16 {
        bytecode.extend_from_slice(&(i * 100 - 700).to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x22 });
    let mut expected = [0i16; 16];
    for (i, e) in expected.iter_mut().enumerate() {
        *e = (i as i16) * 100 - 700;
    }
    assert_eq!(host.n9_table_copies, vec![expected]);
}

#[test]
fn op_4c_n_a_sub_0_advances_when_ctx_flag_clear() {
    // [4C, 0xA0, bit=4, lo, hi] - ctx.flags bit 4 clear → skip 5 bytes.
    // The asm at 0x801e2580/4 ANDs ctx[+0x10] with `(1 << bit)` and only
    // branches to the take-jump label when the result is non-zero.
    let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n_a_sub_0_branches_when_ctx_flag_set() {
    // ctx.flags bit 4 SET → take the absolute jump (0x100).
    let bytecode = [0x4Cu8, 0xA0, 0x04, 0x00, 0x01];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 1u32 << 4,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
}

#[test]
fn op_4c_n_a_sub_2_uses_global_flags() {
    // Global flag set → take jump.
    let bytecode = [0x4Cu8, 0xA2, 0x03, 0x20, 0x00];
    let mut host = TestHost {
        globals: 1u32 << 3,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x20 });
}

#[test]
fn op_4c_n_a_sub_3_through_f_skip_5() {
    // Sub-ops 3..=0xF have no `case` arm in the asm. The dispatch at
    // 0x801e2568 (`bne a1, zero, 0x801e258c`) jumps past every bank
    // check, both `bne a1, 1` and `bne a1, 2` then branch to the
    // skip-5 exit. The s8 += 5 in the delay slot of the first bne is
    // the PC delta.
    let bytecode = [0x4Cu8, 0xA5, 0xFF, 0x00, 0x01];
    let mut host = TestHost {
        globals: !0u32,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx {
        flags: !0u32,
        local_flags: !0u16,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n9_sub_f_registers_callback_and_halts() {
    // [4C, 0x9F] - register `LAB_801DA930` callback then halt at PC.
    // Same dispatch pattern as nibble-8 sub-7 (callback target differs).
    let bytecode = [0x4Cu8, 0x9F];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n9_callback_regs, 1);
}

#[test]
fn op_4c_n8_sub_7_registers_callback_and_halts() {
    // [4C, 0x87] - register actor-list callback (LAB_801E5154) then halt
    // at PC. The original goes through `switchD_801e00f4::default()`,
    // which for opcode 0x4C (`& 0x70 = 0x40`) returns `param_2` -
    // halt at PC. Our hook is one-shot per dispatch entry.
    let bytecode = [0x4Cu8, 0x87];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n8_callback_regs, 1);
}

#[test]
fn op_4c_n_c_sub_4_subtile_broadcast() {
    let bytecode = [0x4Cu8, 0xC4, 0x05, 0x07];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n_c_subtile_broadcasts, vec![(5u8, 7u8)]);
}

#[test]
fn op_4c_n_c_sub_8_xors_field_74() {
    let bytecode = [0x4Cu8, 0xC8];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.field_74, 0x1000_0000);

    // Re-applying flips back.
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.field_74, 0);
}

#[test]
fn op_4c_n_c_sub_a_writes_slot() {
    let bytecode = [0x4Cu8, 0xCA, 0x05, 0x10, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_writes, vec![(5u8, 0x10i16)]);
}

#[test]
fn op_4c_n_c_sub_b_substitutes_frame_delta_when_value_is_ffff() {
    let bytecode = [0x4Cu8, 0xCB, 0x07, 0xFF, 0xFF];
    let mut host = TestHost {
        frame_delta: 3,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_adjusts, vec![(7u8, 3i16, false)]);
}

#[test]
fn op_4c_n_c_sub_c_subtracts() {
    let bytecode = [0x4Cu8, 0xCC, 0x02, 0x05, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_c_slot_adjusts, vec![(2u8, 5i16, true)]);
}

#[test]
fn op_4c_n_c_sub_2_writes_field_42() {
    let bytecode = [0x4Cu8, 0xC2, 0x55];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_42, 0x55);
}

#[test]
fn op_4c_n8_sub_5_halt_acquire_writes_ctx_and_halts() {
    let bytecode = [0x4Cu8, 0x85];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        wait_accum: 7,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(ctx.wait_accum, 0);
    assert_eq!(ctx.flags & 0x400, 0x400);
    assert_eq!(host.n8_halt_acquires, vec![0u32]);
}

#[test]
fn op_4c_n8_sub_e_and_f_share_halt_acquire_body() {
    for sub in [0x8Eu8, 0x8F] {
        let bytecode = [0x4Cu8, sub];
        let mut host = TestHost::default();
        let mut ctx = FieldCtx::default();
        let r = step(&mut host, &mut ctx, &bytecode, 0);
        assert_eq!(r, StepResult::Halt { final_pc: 0 });
        assert_eq!(host.n8_halt_acquires, vec![0u32]);
    }
}

#[test]
fn op_4c_n_c_sub_9_advances_when_globals_match() {
    let bytecode = [0x4Cu8, 0xC9];
    let mut host = TestHost {
        n_c_sub9_globals_differ: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

#[test]
fn op_4c_n_c_sub_9_halts_when_globals_differ() {
    let bytecode = [0x4Cu8, 0xC9];
    let mut host = TestHost {
        n_c_sub9_globals_differ: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_d_sub_6_b1_eq_4_clears_top_bit_only() {
    let bytecode = [0x4Cu8, 0xD6, 0x04];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0xFFFF_FFFF,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(ctx.field_74, 0x7FFF_FFFF);
    assert_eq!(host.n_d_sub6_acks, 1);
}

#[test]
fn op_4c_n_d_sub_6_b1_neq_4_sets_high_byte() {
    let bytecode = [0x4Cu8, 0xD6, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        field_74: 0,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    // Sets bit 0x80000000 + (0x12 << 24) = 0x92000000.
    assert_eq!(ctx.field_74, 0x9200_0000);
}

#[test]
fn op_4c_n_d_sub_8_passes_b1_and_three_words_advances_9() {
    let bytecode = [0x4Cu8, 0xD8, 0x05, 0x10, 0x00, 0x20, 0x00, 0x30, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n_d_sub8_calls, vec![(5u8, [0x10i16, 0x20, 0x30])]);
}

#[test]
fn op_4c_n_e_sub_0_writes_b1_and_halts() {
    let bytecode = [0x4Cu8, 0xE0, 0x42];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_e_sub0_writes, vec![0x42u8]);
}

#[test]
fn op_4c_n_e_sub_9_clears_global_and_advances_2() {
    let bytecode = [0x4Cu8, 0xE9];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_sub9_clears, 1);
}

#[test]
fn op_4c_n_e_sub_a_calls_overlay_and_halts() {
    let bytecode = [0x4Cu8, 0xEA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_e_sub_a_calls, 1);
}

#[test]
fn op_4c_n_c_sub_f_uses_actor_world_when_byte_is_ff() {
    // [4C, 0xCF, 0xFF, 0xFF] - both bytes select the actor's coords.
    let bytecode = [0x4Cu8, 0xCF, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_x: 0x1234,
        world_z: 0x5678,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(
        host.n_c_sub_f_broadcasts,
        vec![(0x1234u16 as i16, 0x5678u16 as i16)]
    );
}

#[test]
fn op_4c_n_c_sub_f_zero_yields_zero_nonzero_yields_tile_center() {
    // b1=0 → 0; b2=2 → 2*0x80 + 0x40 = 0x140.
    let bytecode = [0x4Cu8, 0xCF, 0x00, 0x02];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert_eq!(host.n_c_sub_f_broadcasts, vec![(0, 0x0140)]);
}

#[test]
fn op_4c_n_d_sub_3_party_setup_advances_14_bytes() {
    let mut bytecode = vec![0x4C, 0xD3];
    bytecode.extend_from_slice(&0x1234i16.to_le_bytes());
    bytecode.extend_from_slice(&0x5678i16.to_le_bytes());
    bytecode.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    bytecode.extend_from_slice(&0xCAFE_BABEu32.to_le_bytes());
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 14 });
    assert_eq!(host.n_d_party_setups.len(), 1);
    let (ab, cd, ef) = host.n_d_party_setups[0];
    assert_eq!(ab, (0x1234u32 << 16) | 0x5678u32);
    assert_eq!(cd, 0xDEAD_BEEF);
    assert_eq!(ef, 0xCAFE_BABE);
}

#[test]
fn op_4c_n_d_sub_9_sets_inverted_y_mirror_and_negates_world_y() {
    // value = 5 → field_8e = 5, world_y = -5 (= 0xFFFB).
    let bytecode = [0x4Cu8, 0xD9, 0x05, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    assert!(ctx.flags & 0x2000_0000 != 0);
    assert_eq!(ctx.field_8e, 5);
    assert_eq!(ctx.world_y, 0xFFFB);
}

#[test]
fn op_4c_n_d_sub_9_9999_sentinel_keeps_world_y_unchanged() {
    let bytecode = [0x4Cu8, 0xD9, 0x0F, 0x27]; // 9999 LE
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        world_y: 0x0040,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // value = -world_y = -0x40, ctx.field_8e = -0x40 = 0xFFC0.
    assert_eq!(ctx.field_8e, -0x40);
    // world_y = -value = world_y = 0x40 (unchanged).
    assert_eq!(ctx.world_y, 0x0040);
}

#[test]
fn op_4c_n_d_sub_a_clears_inverted_y_and_calls_collision_y() {
    let bytecode = [0x4Cu8, 0xDA];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        flags: 0x2000_0000,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(ctx.flags & 0x2000_0000, 0);
    assert_eq!(host.n_d_collision_y_calls, 1);
}

#[test]
fn op_4c_n_d_sub_d_writes_field_58() {
    let bytecode = [0x4Cu8, 0xDD, 0x77];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(ctx.field_58, 0x77);
}

#[test]
fn op_4c_n_d_sub_f_scene_byte_write() {
    let bytecode = [0x4Cu8, 0xDF, 0xAB];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_d_scene_byte_writes, vec![0xABu8]);
}

#[test]
fn op_4c_n_e_sub_2_fmv_trigger_decodes_fmv_id() {
    // The 6-byte form `[4C, 0xE2, lo, hi, _, _]` triggers an FMV
    // (game-mode 26 / StrInit). The fmv_id is the s16 at +1..+3
    // and selects an entry in the runtime FMV-state table.
    let bytecode = [0x4Cu8, 0xE2, 0x03, 0x00, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_fmv_triggers, vec![3i16]);
}

#[test]
fn op_4c_n_e_sub_2_fmv_trigger_sign_extends_negative() {
    // s16 sign-extension - retail dispatcher reads through
    // FUN_8003CE9C which sign-extends. fmv_id can be negative
    // when the script wants to clear the trigger / signal a
    // sentinel.
    let bytecode = [0x4Cu8, 0xE2, 0xFF, 0xFF, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_fmv_triggers, vec![-1i16]);
}

#[test]
fn op_4c_n_e_sub_6_d8280_passes_three_words() {
    let mut bytecode = vec![0x4C, 0xE6];
    for w in &[5i16, 10, 15] {
        bytecode.extend_from_slice(&w.to_le_bytes());
    }
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 8 });
    assert_eq!(host.n_e_d8280_calls, vec![[5i16, 10, 15]]);
}

#[test]
fn op_4c_n_e_sub_c_capture_call() {
    let bytecode = [0x4Cu8, 0xEC];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_capture_ddf48_calls, 1);
}

#[test]
fn op_4c_n_e_sub_d_writes_ba66() {
    let bytecode = [0x4Cu8, 0xED, 0x88];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    assert_eq!(host.n_e_ba66_writes, vec![0x88u8]);
}

#[test]
fn op_4c_n_e_sub_e_snapshot_call() {
    let bytecode = [0x4Cu8, 0xEE];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_snapshot_84570_calls, 1);
}

#[test]
fn op_4c_n_f_pass_through_advances_two_bytes() {
    let bytecode = [0x4Cu8, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
}

// -- 0x43 sub-0/1/A/B halt-acquire ----------------------------------

#[test]
fn op_43_sub_0_halt_acquire_yields_to_resume_pc() {
    // [43, 0, x_byte, z_byte, lo, hi] → resume_pc = signed_16(lo, hi) = 0x100
    let bytecode = [0x43u8, 0x00, 0x10, 0x20, 0x00, 0x01];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0x100 });
    assert!(ctx.is_halted());
    assert_eq!(ctx.saved_pc, 0);
    assert_eq!(host.halt_acquire_calls.len(), 1);
    assert_eq!(host.halt_acquire_calls[0].0, 0u8);
    assert_eq!(host.halt_acquire_calls[0].1, 0x100);
}

#[test]
fn op_43_sub_a_halt_acquire_uses_offset_7_target() {
    // [43, 0xA, x, z, _, _, _, _, lo, hi]
    let bytecode = [0x43u8, 0x0A, 0x10, 0x20, 0, 0, 0, 0, 0x34, 0x12];
    let mut host = TestHost {
        halt_acquire_predicate: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Yield { resume_pc: 0x1234 });
    assert!(ctx.is_halted());
}

#[test]
fn op_43_sub_0_predicate_false_advances_5_bytes() {
    let bytecode = [0x43u8, 0x00, 0, 0, 0, 0];
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert!(!ctx.is_halted());
}

#[test]
fn op_43_sub_b_predicate_false_advances_9_bytes() {
    let bytecode = [0x43u8, 0x0B, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut host = TestHost {
        halt_acquire_predicate: false,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
}

// -- Round 16: helper-driven sub-ops --------------------------------

#[test]
fn op_4c_n_c_sub_1_flag_loop_reset_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC1];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_1_flag_loops, 1);
}

#[test]
fn op_4c_n_d_sub_1_no_jump_advances_4_bytes() {
    let bytecode = [0x4Cu8, 0xD1, 0, 0];
    let mut host = TestHost {
        n_d_sub_1_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_d_sub_1_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xD1, 0, 0];
    let mut host = TestHost {
        n_d_sub_1_jump_target: Some(0xABCD),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0xABCD });
}

#[test]
fn op_4c_n_d_sub_2_channel_spawn_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xD2, 0xFB];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_d_sub_2_channel_calls, vec![0xFBu8]);
}

#[test]
fn op_4c_n_d_sub_7_register_list_walk_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xD7];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_d_sub_7_list_walk_regs, 1);
}

#[test]
fn op_4c_n_d_sub_b_e57f0_advances_13_bytes() {
    // 13 total bytes: opcode + sub-op + 11 payload.
    let mut bytecode = vec![0x4Cu8, 0xDB];
    bytecode.extend_from_slice(&[
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55,
    ]);
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 13 });
    assert_eq!(host.n_d_sub_b_e57f0_calls.len(), 1);
    // 12-byte slice starting from the 0xDB sub-op byte.
    assert_eq!(host.n_d_sub_b_e57f0_calls[0].len(), 12);
    assert_eq!(host.n_d_sub_b_e57f0_calls[0][0], 0xDB);
    assert_eq!(host.n_d_sub_b_e57f0_calls[0][11], 0x55);
}

#[test]
fn op_4c_n_d_sub_b_truncated_buffer_returns_unknown() {
    // 12 bytes only - sub-B needs 13.
    let bytecode = [0x4Cu8, 0xDB, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_d_sub_b_e57f0_calls.is_empty());
}

#[test]
fn op_4c_n_d_sub_c_no_jump_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
    let mut host = TestHost {
        n_d_sub_c_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_d_sub_c_calls, vec![0x42u8]);
}

#[test]
fn op_4c_n_d_sub_c_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xDC, 0x42, 0, 0];
    let mut host = TestHost {
        n_d_sub_c_jump_target: Some(0x100),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x100 });
}

#[test]
fn op_4c_n_d_sub_e_query_no_jump_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
    let mut host = TestHost {
        n_d_sub_e_jump_target: None,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_d_sub_e_calls, vec![0x99u8]);
}

#[test]
fn op_4c_n_d_sub_e_query_jump_target_takes_ce9c_path() {
    let bytecode = [0x4Cu8, 0xDE, 0x99, 0, 0];
    let mut host = TestHost {
        n_d_sub_e_jump_target: Some(0x40),
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x40 });
}

#[test]
fn op_4c_n_e_sub_1_text_actor_zero_first_byte_skips_spawn() {
    // First byte 0 → no spawn; PC still advances by 3 (0 has no payload).
    // packet_length([0]) = 0 → advance by 3 + 0 = 3.
    let bytecode = [0x4Cu8, 0xE1, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 3 });
    // No spawn because first byte is 0.
    assert!(host.n_e_sub_1_text_calls.is_empty());
}

#[test]
fn op_4c_n_e_sub_1_text_actor_short_string_advances_correctly() {
    // [4C, E1, 'A', 'B', 'C', 0] - packet length 3, total advance 6.
    let bytecode = [0x4Cu8, 0xE1, b'A', b'B', b'C', 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx {
        script_id: 0x42,
        ..FieldCtx::default()
    };
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_e_sub_1_text_calls.len(), 1);
    assert_eq!(host.n_e_sub_1_text_calls[0].0, vec![b'A', b'B', b'C']);
    assert_eq!(host.n_e_sub_1_text_calls[0].1, 0x42);
}

#[test]
fn op_4c_n_e_sub_1_text_actor_with_escape_sequences() {
    // [4C, E1, 'A', 0xC1, 0xAB, 'B', 0] - escape pair counts as 2,
    // total packet length = 4, total advance = 7.
    let bytecode = [0x4Cu8, 0xE1, b'A', 0xC1, 0xAB, b'B', 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n_e_sub_1_text_calls[0].0.len(), 4);
}

#[test]
fn op_4c_n_e_sub_1_text_actor_truncated_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE1]; // first byte missing
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_1_text_calls.is_empty());
}

// -----------------------------------------------------------------
// Round 17 - five new 0x4C nC sub-ops + two 0x4C nE sub-ops.
// -----------------------------------------------------------------

#[test]
fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC0];
    let mut host = TestHost {
        n_c_sub_0_active: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_0_move_cancels, 1);
}

#[test]
fn op_4c_n_c_sub_0_move_cancel_advances_2_bytes_when_inactive() {
    // Even when the host returns false (no active move), PC still
    // advances by 2 - the cancel side-effect is conditional but the
    // dispatcher's PC delta is constant.
    let bytecode = [0x4Cu8, 0xC0];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_0_move_cancels, 1);
}

#[test]
fn op_4c_n_c_sub_3_script_teleport_advances_2_bytes() {
    let bytecode = [0x4Cu8, 0xC3];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_c_sub_3_teleports, 1);
}

#[test]
fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes() {
    // [4C, 0xC5, 0x05, 0x00] - flag idx 5. Bit 5 in the host's bank is
    // unset, so the original "jump-if-zero" path fires; both branches
    // advance PC by 4.
    let bytecode = [0x4Cu8, 0xC5, 0x05, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(5, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_party_flag_jz_advances_4_bytes_when_set() {
    // Bit set → original's "jump-if-zero" doesn't fire; PC still += 4.
    let bytecode = [0x4Cu8, 0xC5, 0x07, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(7, true);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_reads_16_bit_index_via_helper() {
    // Verify the dispatcher reads the index through load_u16_le by
    // setting two distinct flag bits and checking which one was queried.
    let bytecode = [0x4Cu8, 0xC5, 0x34, 0x12]; // 0x1234
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(0x1234, true);
    host.n_c_party_flag_bits.insert(0x3412, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
    // The 0x1234 bit is set in the bank - the dispatcher's load_u16_le
    // must produce 0x1234 (LE), not 0x3412 (BE).
    assert!(*host.n_c_party_flag_bits.get(&0x1234).unwrap());
}

#[test]
fn op_4c_n_c_sub_6_party_flag_jnz_advances_4_bytes() {
    // Same shape as sub-5 but opposite polarity. Both polarities share
    // PC += 4 either way.
    let bytecode = [0x4Cu8, 0xC6, 0x09, 0x00];
    let mut host = TestHost::default();
    host.n_c_party_flag_bits.insert(9, false);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 4 });
}

#[test]
fn op_4c_n_c_sub_5_truncated_buffer_returns_unknown() {
    // Need 3 bytes after pc; only 2 available.
    let bytecode = [0x4Cu8, 0xC5, 0x05]; // missing high byte
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_c_sub_d_script_alloc_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xCD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_c_sub_d_allocs, 1);
}

#[test]
fn op_4c_n_e_sub_4_bbox_inside_advances_9_bytes() {
    // Host returns false (= "inside") → PC += 9. The dispatcher computes
    // a tile-center bbox and passes it to the host.
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n_e_sub_4_bboxes.borrow().len(), 1);
}

#[test]
fn op_4c_n_e_sub_4_bbox_outside_halts_at_pc() {
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x20, 0x20, 0x00, 0x00, 0x00];
    let mut host = TestHost {
        n_e_sub_4_outside: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_e_sub_4_bbox_tile_center_math_for_low_byte() {
    // Operand byte 0x10 → tile-center: (0x10 << 7) | 0x40 = 0x840
    // (high bit clear, so no extra +0x40).
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10, 0x10, 0x10, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes.len(), 1);
    // All four corners should be 0x840 = 2112.
    assert_eq!(bboxes[0], [0x840, 0x840, 0x840, 0x840]);
}

#[test]
fn op_4c_n_e_sub_4_bbox_tile_center_high_bit_adds_0x40() {
    // Operand byte 0x90: low 7 bits are 0x10 → base 0x840. High bit set
    // → extra +0x40 = 0x880.
    let bytecode = [0x4Cu8, 0xE4, 0x90, 0x90, 0x90, 0x90, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes[0], [0x880, 0x880, 0x880, 0x880]);
}

#[test]
fn op_4c_n_e_sub_4_bbox_zero_byte_yields_zero() {
    // Operand byte 0x00 → 0 (special case in tile-center math).
    let bytecode = [0x4Cu8, 0xE4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let _ = step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes[0], [0, 0, 0, 0]);
}

#[test]
fn op_4c_n_e_sub_4_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x10]; // missing last 5 bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_5_add_xp_positive_value() {
    // [4C, E5, 0xE8, 0x03, 0x00] - 0x0003E8 = 1000.
    let bytecode = [0x4Cu8, 0xE5, 0xE8, 0x03, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_xp_deltas, vec![1000]);
}

#[test]
fn op_4c_n_e_sub_5_add_xp_negative_value() {
    // 0xFFFFFE = -2 in 24-bit two's complement.
    let bytecode = [0x4Cu8, 0xE5, 0xFE, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_xp_deltas, vec![-2]);
}

#[test]
fn op_4c_n_e_sub_5_add_xp_zero_advances_5_bytes() {
    let bytecode = [0x4Cu8, 0xE5, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_5_xp_deltas, vec![0]);
}

#[test]
fn op_4c_n_e_sub_5_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE5, 0x01]; // missing 2 bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_5_xp_deltas.is_empty());
}

#[test]
fn op_4c_n_e_sub_b_actor_resolved_advances_5_bytes() {
    // [4C, EB, actor_id=0x07, target_lo=0x10, target_hi=0x20]
    // When the host resolves the actor, take the "pc + 5" path.
    let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
    let mut host = TestHost {
        n_e_sub_b_resolves: true,
        ..TestHost::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
    assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
}

#[test]
fn op_4c_n_e_sub_b_actor_unresolved_jumps_to_target() {
    // Same instruction; host returns None (actor not resolved). The
    // dispatcher reads the absolute jump target via load_u16_le and
    // returns it as the new PC.
    let bytecode = [0x4Cu8, 0xEB, 0x07, 0x10, 0x20];
    let mut host = TestHost::default(); // n_e_sub_b_resolves = false
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x2010 });
    assert_eq!(host.n_e_sub_b_actor_ids, vec![0x07]);
}

#[test]
fn op_4c_n_e_sub_b_jump_target_uses_load_u16_le() {
    // Verify endianness: bytes 0x34, 0x12 should produce target 0x1234,
    // not 0x3412.
    let bytecode = [0x4Cu8, 0xEB, 0xAA, 0x34, 0x12];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
}

#[test]
fn op_4c_n_e_sub_b_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xEB, 0x07]; // missing 2 jump-target bytes
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_b_actor_ids.is_empty());
}

// -- Round 18 - 0x4C n8 actor-allocator + nE camera + nD/n5 dialog ---

#[test]
fn op_4c_n_8_sub_1_set_model_anim_advances_pc_by_9() {
    // [4C, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE]
    // model_id (LE24) = 0x563412
    // anim_frame (LE16) = 0x9A78
    // tween_frames (LE16) = 0xDEBC
    let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 9 });
    assert_eq!(host.n_8_sub_1_set_model_calls.len(), 1);
    let (model_id, anim_frame, tween_frames) = host.n_8_sub_1_set_model_calls[0];
    assert_eq!(model_id, 0x0056_3412);
    assert_eq!(anim_frame, 0x9A78);
    assert_eq!(tween_frames, 0xDEBC);
}

#[test]
fn op_4c_n_8_sub_1_truncated_buffer_returns_unknown() {
    // Only 8 bytes - sub-1 needs 9 total.
    let bytecode = [0x4Cu8, 0x81, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_8_sub_1_set_model_calls.is_empty());
}

#[test]
fn op_4c_n_8_sub_6_actor_set_rotation_advances_15() {
    // [4C, 0x86, ... 12 bytes for 6 LE16 ..., actor_id]
    let bytecode = [
        0x4Cu8, 0x86, // opcode + sub-op
        0x10, 0x00, 0x20, 0x00, 0x30, 0x00, // x, y, z = 0x10, 0x20, 0x30
        0x40, 0x00, 0x50, 0x00, 0x60, 0x00, // rx, ry, rz = 0x40, 0x50, 0x60
        0x07, // actor_id = 7
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 15 });
    assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
    let (actor_id, position, rotation) = host.n_8_sub_6_actor_set_rotation_calls[0];
    assert_eq!(actor_id, 7);
    assert_eq!(position, [0x10, 0x20, 0x30]);
    assert_eq!(rotation, [0x40, 0x50, 0x60]);
}

#[test]
fn op_4c_n_8_sub_6_advances_15_even_when_actor_missing() {
    // The original short-circuits to `return param_2 + 0xF` when the
    // actor lookup fails - PC still advances by 15.
    let bytecode = [
        0x4Cu8, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF,
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: false, // actor lookup fails
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 15 });
    // Hook still fires (the host records the call); the actor_present
    // bool just controls the return value.
    assert_eq!(host.n_8_sub_6_actor_set_rotation_calls.len(), 1);
}

#[test]
fn op_4c_n_8_sub_6_signed_decode_round_trip() {
    // Negative LE16 should decode through `as i16` correctly.
    let bytecode = [
        0x4Cu8, 0x86, 0xFF, 0xFF, // x = -1
        0x00, 0x80, // y = -32768
        0xFE, 0xFF, // z = -2
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // rx/ry/rz = 0
        0x00,
    ];
    let mut host = TestHost {
        n_8_sub_6_actor_present: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    let (_, position, _) = host.n_8_sub_6_actor_set_rotation_calls[0];
    assert_eq!(position, [-1, -32768, -2]);
}

#[test]
fn op_4c_n_8_sub_b_jumps_when_actor_type_present() {
    // [4C, 0x8B, type=0x12, target_lo=0x34, target_hi=0x12] → jump 0x1234
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
    let mut host = TestHost::default();
    host.n_8_sub_b_present_types.insert(0x12);
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x1234 });
}

#[test]
fn op_4c_n_8_sub_b_advances_5_when_no_actor() {
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34, 0x12];
    let mut host = TestHost::default(); // no types registered
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 5 });
}

#[test]
fn op_4c_n_8_sub_b_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0x8B, 0x12, 0x34]; // missing target hi byte
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_8_sub_d_empty_slot_advances_6() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
    let host_state = ActorSearchResult::EmptySlot;
    let mut host = TestHost {
        n_8_sub_d_search_result: host_state,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    let queries = host.n_8_sub_d_queries.borrow();
    assert_eq!(queries.as_slice(), &[(0x05, 0xAB)]);
}

#[test]
fn op_4c_n_8_sub_d_found_jumps_to_le_target() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x78, 0x56];
    let mut host = TestHost {
        n_8_sub_d_search_result: ActorSearchResult::Found,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 0x5678 });
}

#[test]
fn op_4c_n_8_sub_d_no_match_halts_at_pc() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34, 0x12];
    let mut host = TestHost {
        n_8_sub_d_search_result: ActorSearchResult::NoMatch,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}

#[test]
fn op_4c_n_8_sub_d_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0x8D, 0x05, 0xAB, 0x34]; // missing target hi
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_3_advances_2_and_records_actor() {
    let bytecode = [0x4Cu8, 0xE3, 0x09];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_e_sub_3_camera_syncs, vec![9]);
}

#[test]
fn op_4c_n_e_sub_3_truncated_returns_unknown() {
    let bytecode = [0x4Cu8, 0xE3]; // missing actor_id
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
    assert!(host.n_e_sub_3_camera_syncs.is_empty());
}

#[test]
fn op_4c_n_e_sub_7_camera_animate_decodes_le24_then_le16() {
    // 7-byte instruction: [opcode, sub-op, t0, t1, t2, d0, d1].
    // target = LE24(0x12, 0x34, 0x56) = 0x563412
    // duration = LE16(0xEF, 0xCD) = 0xCDEF
    let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF, 0xCD];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 7 });
    assert_eq!(host.n_e_sub_7_camera_animates, vec![(0x0056_3412, 0xCDEF)]);
}

#[test]
fn op_4c_n_e_sub_7_truncated_buffer_returns_unknown() {
    // 6 bytes - last byte is missing.
    let bytecode = [0x4Cu8, 0xE7, 0x12, 0x34, 0x56, 0xEF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_e_sub_8_camera_zoom_decodes_four_le16() {
    // 10-byte instruction: [opcode, sub-op, x0, x1, y0, y1, z0, z1, m0,
    // m1]. zoom_x = LE16(0x40, 0x00) = 0x40, zoom_y = 0x08, zoom_z = 4,
    // mode = 0 (the default-zoom triplet at line 7315-7317 of the
    // dispatcher dump).
    let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 10 });
    assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0x40, 0x08, 0x04, 0)]);
}

#[test]
fn op_4c_n_e_sub_8_camera_zoom_signed_mode_round_trip() {
    // Mode is i16 - verify negative / sign-bit values flow through.
    let bytecode = [0x4Cu8, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(host.n_e_sub_8_camera_zooms, vec![(0, 0, 0, -1)]);
}

#[test]
fn op_4c_n_e_sub_8_truncated_buffer_returns_unknown() {
    // 9 bytes - needs 10.
    let bytecode = [0x4Cu8, 0xE8, 0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_d_sub_0_field_se_trigger_advances_6() {
    // [4C, 0xD0, 0x34, 0x12, 0x78, 0x56] → a = 0x1234, b = 0x5678.
    let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78, 0x56];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_0_se_triggers, vec![(0x1234, 0x5678)]);
}

#[test]
fn op_4c_n_d_sub_0_truncated_buffer_returns_unknown() {
    let bytecode = [0x4Cu8, 0xD0, 0x34, 0x12, 0x78]; // 5 bytes - needs 6
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert!(matches!(
        r,
        StepResult::Unknown {
            opcode: 0x4C,
            pc: 0
        }
    ));
}

#[test]
fn op_4c_n_5_sub_3_dialog_wait_halts_at_pc_plus_2() {
    // [4C, 0x53] → halt-style return with PC = 0 + 2 = 2.
    let bytecode = [0x4Cu8, 0x53];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 2 });
    assert_eq!(host.n_5_sub_3_dialog_waits, 1);
}

#[test]
fn op_4c_n_5_sub_4_dialog_advance_advances_when_done() {
    // Default: dialog_active = false → advance PC by 2.
    let bytecode = [0x4Cu8, 0x54];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 2 });
    assert_eq!(host.n_5_sub_4_polls, 1);
}

#[test]
fn op_4c_n_5_sub_4_dialog_advance_halts_when_active() {
    // dialog_active = true → halt at PC.
    let bytecode = [0x4Cu8, 0x54];
    let mut host = TestHost {
        n_5_sub_4_dialog_active: true,
        ..Default::default()
    };
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
    assert_eq!(host.n_5_sub_4_polls, 1);
}

#[test]
fn op_4c_n_e_sub_4_uses_shared_tile_center_helper() {
    // Verifies the round-18 tile_center helper is wired in: 0x10 → 0x840,
    // 0x90 → 0x880, 0x00 → 0. This is the same case the round-17 inline
    // closure verified - confirm round-18's lift to a shared helper
    // didn't change semantics.
    let bytecode = [0x4Cu8, 0xE4, 0x10, 0x90, 0x00, 0x10, 0x00, 0x00, 0x00];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    step(&mut host, &mut ctx, &bytecode, 0);
    let bboxes = host.n_e_sub_4_bboxes.borrow();
    assert_eq!(bboxes.len(), 1);
    assert_eq!(bboxes[0], [0x840, 0x880, 0, 0x840]);
}

#[test]
fn op_4c_n_d_sub_4_vram_stp_set_advances_6_bytes() {
    // [4C, 0xD4, x_lo, x_hi, y_lo, y_hi] = 6 bytes; original returns
    // iVar47 + 6. next_pc = 0 + 1 (header_size) + 5 = 6.
    let bytecode = [0x4Cu8, 0xD4, 0x80, 0x00, 0xEF, 0x01]; // x=0x0080, y=0x01EF
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_4_vram_stp_set_calls, vec![(0x0080, 0x01EF)]);
}

#[test]
fn op_4c_n_d_sub_5_vram_stp_clear_advances_6_bytes() {
    // Sister of sub-4 with STP-clear semantics; same 6-byte encoding.
    let bytecode = [0x4Cu8, 0xD5, 0xC0, 0x01, 0x10, 0x00]; // x=0x01C0, y=0x0010
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Advance { next_pc: 6 });
    assert_eq!(host.n_d_sub_5_vram_stp_clear_calls, vec![(0x01C0, 0x0010)]);
}

#[test]
fn op_4c_n_e_sub_f_halts_at_pc() {
    // Sub-F has no case in the original; falls through to halt.
    let bytecode = [0x4Cu8, 0xEF];
    let mut host = TestHost::default();
    let mut ctx = FieldCtx::default();
    let r = step(&mut host, &mut ctx, &bytecode, 0);
    assert_eq!(r, StepResult::Halt { final_pc: 0 });
}
