//! The field VM's `FieldHost` bridge (`FieldHostImpl`) and the field-step
//! helpers it routes through. Split out of `vm_hosts.rs`.

use super::*;

mod actor_motion;
mod actor_spawn;
mod op49_submode;
mod party_inventory;
mod screen_and_camera;
mod script_teleport;
mod sound_cues;
mod step_routing;
mod talk;

#[cfg(test)]
pub(in crate::world) use script_teleport::apply_script_table_teleport;
pub(in crate::world) use step_routing::*;

/// Whose drawn heading a field context's `+0x26` is.
pub(in crate::world) enum HeadingOwner {
    Player,
    Placement(u8),
}

impl World {
    /// Resolve the actor whose heading `ctx` carries: the player for the
    /// `0xF8` context outside a placement step; else the placement being
    /// stepped (its own script or a poke on it), else the NPC whose talk the
    /// inline runner runs (the runner's context carries no id of its own),
    /// else the placement channel whose context id it is.
    pub(in crate::world) fn heading_owner(&self, ctx: &FieldCtx) -> Option<HeadingOwner> {
        let channel = self.field_vm.executing_channel;
        if channel.is_none() && ctx.script_id == u16::from(crate::field_env::PLAYER_ANCHOR_TARGET) {
            return Some(HeadingOwner::Player);
        }
        channel
            .or(self.dialog.stepping_inline_npc)
            .or_else(|| {
                self.channel_view()
                    .iter()
                    .find(|c| !c.object_bind && c.ctx.script_id == ctx.script_id)
                    .and_then(|c| u8::try_from(c.placement_index).ok())
            })
            .map(HeadingOwner::Placement)
    }
}

pub(in crate::world) struct FieldHostImpl<'a> {
    pub(in crate::world) world: &'a mut World,
}

impl FieldHostImpl<'_> {
    /// Whether `ctx` is the **player** for an arm whose retail form compares
    /// the executing context against the player object `_DAT_8007C364`
    /// (`0x23` at `0x801DEC7C`, `4C 51` at `0x801E1954`) rather than testing
    /// a flag. The port has no context pointers, so it reads the party-bank
    /// bit `0x01000000` the player context carries - but a placement seated
    /// with a `>= 0xF0` party model carries that bit too (`FUN_8003A1E4` ORs
    /// it in at `0x8003A2DC..0x8003A3B4`, and scripts toggle it on every
    /// gesture). A context stepped as a placement channel - its own script,
    /// a cross-context poke on it, or the scene-entry pre-run - is never the
    /// player, whatever its bit says.
    ///
    /// REF: FUN_801DE840 (the player-identity compares), FUN_8003A1E4
    fn ctx_is_player(&self, ctx: &FieldCtx) -> bool {
        ctx.flags & legaia_engine_field::field_player_clip::PARTY_BANK_FLAG != 0
            && self.world.field_vm.executing_channel.is_none()
            && !self.world.field_vm.entry_prerun
    }

    /// Whose side buffer a `4C 45` writes: the player (an op on the player
    /// stand-in context, `CC F8 45 ..`), the placement channel stepping, or
    /// nobody - a placed object's context draws through the static bracket,
    /// which never reads the look.
    fn look_key(&self, ctx: &FieldCtx) -> Option<crate::actor_look::LookKey> {
        use crate::actor_look::LookKey;
        if ctx.script_id == u16::from(crate::field_env::PLAYER_ANCHOR_TARGET)
            || self.ctx_is_player(ctx)
        {
            return Some(LookKey::Player);
        }
        if self.world.field_vm.executing_object.is_some() {
            return None;
        }
        self.world.field_vm.executing_channel.map(LookKey::Npc)
    }
}

impl<'a> FieldHost for FieldHostImpl<'a> {
    fn op4c_n_e_sub_3_actor_sync_camera(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        Self::op4c_n_e_sub_3_actor_sync_camera(self, ctx, actor_id)
    }

    fn player_cflag(&mut self, bit: u8, set: bool) -> bool {
        self.world.field_player_cflag(bit, set)
    }

    fn player_set_model(&mut self, value: i16) -> bool {
        self.world.field_player_set_model(value)
    }

    fn face_compass(&mut self, ctx: &mut FieldCtx, index: u8, player: bool) {
        Self::face_compass(self, ctx, index, player)
    }

    fn global_flags(&self) -> u32 {
        self.world.flags.story_flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.world.flags.story_flags = value;
    }
    fn frame_delta(&self) -> u16 {
        // Default world ticks one logical frame per `tick()`. Engines that
        // run faster-than-frame can override this on a custom host wrapper.
        1
    }
    fn extra_flags(&self) -> u32 {
        self.world.flags.extra_flags
    }

    // Op-0x49 STATE_RESUME, scoped to the `town01` opening cutscene timeline.
    // The pinned name-entry handoff (P2[3] body `0x02c6`, `49 03 00`) suspends
    // the script here; the engine opens the name-entry overlay on the Idle->arm
    // edge and keeps the op Armed (parked) until the player commits, then Done
    // (resume). Any other `49 03 <slot>` does the same in the context that
    // ran it (`CutsceneState::naming_owner`); a context with no naming
    // prompt pending falls back to the default Idle.
    // REF: FUN_801F03F0 (name-entry overlay) / op49_invoke_setup func_0x80020de0
    // Op `0x4C` outer-nibble-4 sub-9 - the writer of the two globals
    // `crate::camera_ease` eases between, read off the three arms at
    // `0x801E1480..0x801E162C` in `overlay_world_map_801de840.txt`:
    //
    // | `_DAT_1F800394` | scene ctrl `+0x4A`     | `_DAT_8007BCAC`      |
    // |---|---|---|
    // | bit 25 (delta)  | `target`               | `target - player[+0x16]` |
    // | bit 24 (rel)    | `target + player[+0x16]` | `target`           |
    // | neither         | `target`               | untouched            |
    //
    // The first two arms both write **both** globals, and both land the
    // accumulator on the same value the per-frame easing would have walked
    // to (`ctrl[+0x4A] - player[+0x16]`); they are snaps, not a different
    // destination. The bit-24 arm has to post the accumulator itself because
    // that same bit is `FUN_801DA390`'s input lock (`0x801DA398`), so while
    // it is raised the easing returns before its first store.
    // REF: FUN_801DA390 (the easing), FUN_801D6704 (seeds the accumulator)
    fn op4c_n4_sub9_default_write(&mut self, target: i16) {
        self.world.camera.scene_offset = target;
    }
    fn op4c_n4_sub9_default_ramp(&mut self, target: i16, ticks: u16) {
        // Retail schedules a ramp over `ticks` frames through the register
        // ramp helper; the engine has one ramp mechanism and this is not it,
        // so the endpoint is posted immediately and the per-frame easing
        // supplies the approach. `ease_step` caps the move at 12 units a
        // frame either way, so the visible difference is the shape of the
        // last few frames, not the destination.
        let _ = ticks;
        self.world.camera.scene_offset = target;
    }
    fn op4c_n4_sub9_delta_write_or_ramp(&mut self, target: i16, ticks: u16) {
        let _ = ticks;
        self.world.camera.scene_offset = target;
        let footing = self.world.camera_ease_player_footing();
        self.world.camera.offset_ease = i32::from(target.wrapping_sub(footing));
    }
    fn op4c_n4_sub9_player_relative_write(&mut self, target: i16, ticks: u16) {
        let _ = ticks;
        let footing = self.world.camera_ease_player_footing();
        self.world.camera.scene_offset = target.wrapping_add(footing);
        self.world.camera.offset_ease = i32::from(target);
    }

    fn op4c_n_8_sub_1_set_tint(
        &mut self,
        ctx: &mut FieldCtx,
        target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        Self::op4c_n_8_sub_1_set_tint(self, ctx, target, colour, blend, ticks)
    }

    /// Op `4C 45` with `ticks == 0`: the side buffer's look object and
    /// angles, written at once (`0x801E12BC..0x801E12F4`). See
    /// [`crate::actor_look`].
    ///
    /// PORT: FUN_801DE840 (the nibble-4 sub-5 immediate arm)
    fn op4c_n4_sub5_write_immediate(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
    ) {
        if let Some(key) = self.look_key(ctx) {
            self.world.npcs.looks.write(key, b1, [w94, w96, w98]);
        }
    }

    /// Op `4C 45` with `ticks != 0`: the object at once, one ramp per angle
    /// that changes (`0x801E12F8..0x801E138C`).
    ///
    /// PORT: FUN_801DE840 (the nibble-4 sub-5 ramp arm)
    fn op4c_n4_sub5_ramp(
        &mut self,
        ctx: &mut FieldCtx,
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    ) {
        if let Some(key) = self.look_key(ctx) {
            self.world.npcs.looks.ramp(key, b1, [w94, w96, w98], ticks);
        }
    }

    // `4C 48`'s immediate arm: the context's `+0x26` is the actor's drawn
    // heading. nilboa's Delilas pair (`P1[4]` / `P1[5]`) stand at `0x300`
    // from their spawn prologue's `4C 48 00 03 00 00`.
    // REF: FUN_801DE840 (nibble-4 sub-8)
    fn op4c_n4_heading_write(&mut self, ctx: &mut FieldCtx, value: u16) {
        match self.world.heading_owner(ctx) {
            Some(HeadingOwner::Player) => {
                if let Some(a) = self
                    .world
                    .player_actor_slot
                    .and_then(|s| self.world.actors.get_mut(usize::from(s)))
                {
                    a.move_state.render_26 = (value as i16).wrapping_add(0x800);
                }
            }
            Some(HeadingOwner::Placement(slot)) => {
                self.world.npcs.heading_ramps.free_owner(u32::from(slot));
                self.world
                    .npcs
                    .headings
                    .insert(slot, (value as i16).wrapping_add(0x800));
            }
            None => {}
        }
    }

    fn op4c_nibble4_ctx_ramp(&mut self, ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        Self::op4c_nibble4_ctx_ramp(self, ctx, sub, target, ticks)
    }

    fn op4c_nibble4_global_write(&mut self, sub: u8, target: i32, ticks: u16) {
        let _ = ticks;
        let Some(ctrl) = self.world.world_map.ctrl.as_mut() else {
            return;
        };
        let v = target as u32;
        match sub {
            0xA => ctrl.horizon_params.0 = v,
            0xB => ctrl.horizon_params.1 = v,
            0xC => ctrl.horizon_params.2 = v,
            _ => {}
        }
    }

    fn op49_state(&self) -> Op49State {
        Self::op49_state(self)
    }
    fn op49_arm(&mut self, sub_op: u8, _pc: usize, _field_90: u32) {
        // Retail's `sw s6,-0x4bb0(s0)`: the park holds the operand pointer,
        // whose first byte is this sub-op. Recording it is what lets
        // `World::menu_entry_context_kind` answer with anything other than
        // the two sub-ops the port could previously infer from its own
        // dedicated host paths.
        self.world.record_op49_park(sub_op);
    }

    fn op49_clear(&mut self) {
        Self::op49_clear(self)
    }

    fn op49_menu_request(&mut self, sub_op: u8, instr: &[u8]) {
        Self::op49_menu_request(self, sub_op, instr)
    }
    fn op49_invoke_setup(&mut self) {
        if self.world.cutscene.prologue_naming_pending
            && !self.world.cutscene.prologue_naming_armed
            && self.world.cutscene.naming_owner == Some(self.world.op49_park_owner())
            && !self.world.name_entry_active()
        {
            // The slot is the operand's byte after the sub-op (retail's
            // char-record pointer `_DAT_8007B450 + 1`): `00` = Vahn at the
            // opening, `01` = Noa in `cave01`.
            self.world.open_name_entry(self.world.cutscene.naming_slot);
            self.world.cutscene.prologue_naming_armed = true;
        }
    }
    /// Op `0x42` mode 1's word is the **held pad** `_DAT_8007B850` in its
    /// packed form (`lw v0, -0x47B0(at)` at `0x801DFC08`): the d-pad in
    /// `0xF000`, Triangle / Circle / Cross / Square in `0x10` / `0x20` /
    /// `0x40` / `0x80` - the word the tile board and the world-map panels
    /// read. An earlier port kept a never-written `screen_mode` field here,
    /// so every mode-1 test missed; Rim Elm's "stand here and press Down"
    /// polls (`42 01 00 ..`, `town01` P2[12..14]) never saw the press.
    fn screen_mode(&self) -> u32 {
        u32::from(crate::world_map_panel_host::packed_pad(
            self.world.input.pad(),
        ))
    }

    /// The compass table at `0x801F28D0` (field overlay data): the
    /// `0xF000` d-pad value op `0x42` mode 1 compares the held pad against
    /// (`bne v0, v1` at `0x801DFC14`) - Down, Down+Left, Left, Left+Up,
    /// Up, Up+Right, Right, Right+Down.
    fn screen_mode_table(&self, index: u8) -> Option<u32> {
        const PAD_COMPASS: [u32; 8] = [
            0x4000, 0xC000, 0x8000, 0x9000, 0x1000, 0x3000, 0x2000, 0x6000,
        ];
        PAD_COMPASS.get(usize::from(index)).copied()
    }

    // Op-0x43 screen-effect widget sub-ops (the PROT-0900 mask / sprite /
    // panel / letterbox family, exercised by the ten ending scenes).
    // Each routes to the world's widget host; the Field / Cutscene tick
    // advances the widgets and publishes `World::presentation.fx_frame`.
    // REF: FUN_801F8004 / FUN_801F8D4C / FUN_801F88FC / FUN_801F8E6C /
    // FUN_801F8F28 (spawn + control APIs)
    fn op43_widget_sprite_spawn(&mut self, payload: &[u8]) {
        self.world.presentation.fx.sprite_spawn(payload);
    }
    fn op43_widget_mask_rect(&mut self, words: [u16; 5]) {
        self.world.presentation.fx.mask_rect(words);
    }
    fn op43_widget_letterbox(&mut self, payload: &[u8]) {
        self.world.presentation.fx.letterbox_config(payload);
    }
    fn op43_widget_panel_spawn(&mut self, payload: &[u8; 13]) {
        self.world.presentation.fx.panel_spawn(payload);
    }
    fn op43_widget_panel_move(&mut self, words: [i16; 4]) {
        self.world.presentation.fx.panel_move(words);
    }

    // Op-0x43 sub-3..6 camera-register zone-ramp spawn (retail
    // `FUN_8003C6A4` actor on the effect list). The record's
    // parameterization is the ported kernel; the world holds the spawned
    // records ([`crate::world::CameraRig::register_ramps`]) and ticks each one's
    // `FUN_80037018` handler per frame ([`World::tick_register_ramps`]).
    // REF: FUN_8003C6A4 (kernel PORT lives in crate::register_ramp)
    fn op43_camera_register_ramp(&mut self, sub_op: u8, zone: [u8; 4], start: i16, end: i16) {
        if let Some(ramp) = crate::register_ramp::spawn_register_ramp(sub_op, zone, start, end) {
            self.world.camera.register_ramps.push(ramp);
        }
    }

    fn op43_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        Self::op43_three_actor_talk(self, actor_ids, arg_word, arg_byte)
    }

    // Shared system flag bank - same fourth-flag-bank at `_DAT_80085758`
    // that move-VM ext sub-ops 0x13 / 0x14 / 0x1C / 0x1D query, plus the
    // 0x5x / 0x6x / 0x7x default-route opcodes.
    fn system_flag_set(&mut self, idx: u16) {
        self.world.system_flag_set(idx);
    }
    fn system_flag_clear(&mut self, idx: u16) {
        self.world.system_flag_clear(idx);
    }
    fn system_flag_test(&self, idx: u16) -> bool {
        self.world.system_flag_test(idx)
    }
    fn scene_transition(&mut self, map_id: u8) {
        // Record the request; SceneHost::tick drains it after the field
        // step returns so the bytecode swap doesn't invalidate the
        // borrow we're stepping through.
        self.world.pending_scene_transition = Some(map_id);
    }

    fn minigame_door_warp(&mut self, sub_id: u8) {
        // Retail's arm does two things this host can do inline - zero the
        // session-winnings accumulator `_DAT_80084440` and back the departure
        // scene up for the return trip (the backup is `FUN_80025980`'s half of
        // the same entry, and the engine keeps the field state resident, so
        // arming here is equivalent and keeps the round trip in one place).
        self.world.arm_minigame_warp();
        // The mode change itself is deferred like every other transition: the
        // field bytecode is still borrowed for this step, and entering a
        // minigame swaps the world's scene mode out from under it.
        self.world.minigames.pending_warp = Some(sub_id);
    }

    // PORT: FUN_8001FD44 (the name-based scene-change packet)
    //
    // Retail stages the destination by *name*. It saves the active buffer
    // `0x80084548` and the resolved-index word `0x80084540`, raises
    // `_DAT_1F800394 |= 0x40` (transition pending), zeroes `_DAT_8007BA98`,
    // copies the name into the staged buffer `0x8007050C` and calls
    // `FUN_8001D7F8`, which rewrites `0x80084540` with the destination's
    // index. Then it forks on `_DAT_8007B8C2` (`0x8001FDCC`):
    //
    // - `!= 0` (retail boots with it set): the old active name is backed up
    //   to `0x80084558` (the previous-scene buffer), the new name becomes the
    //   active one, and the index goes to `_DAT_8007B768` (`0x8001FDF4`);
    // - `== 0` (the dev arm, never taken on the disc): the name goes to
    //   `0x800915C8`, the saved name is restored into the active buffer, and
    //   the index goes to `gp+0x688`.
    //
    // Both arms restore `0x80084540` from the saved copy and spawn the
    // transition streaming actor (descriptor `0x80070734`, handler
    // `FUN_80021934`), writing `_DAT_8007B828 = 0x7FFF` when the pool is full.
    // The engine has no name buffers or streaming actor: the packet is this
    // deferred triple plus the arrival facing, which `SceneHost::tick` drains
    // where the retail arm's streaming actor would load the destination.
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        // Named scene-change (op 0x3F): the destination name is inline, so no
        // map-id resolver is needed. Recorded for SceneHost::tick to drain,
        // the same deferral as the map-id path above (the bytecode swap can't
        // run while we're stepping through it). `dir` is the arrival-facing
        // compass selector the seat applies on the far side.
        self.world.pending_named_scene_transition =
            Some((scene.to_string(), entry_x, entry_z, dir));
    }

    fn is_scripted_encounter_armed(&self) -> bool {
        self.world.encounters.scripted_armed
    }

    fn install_scripted_encounter(&mut self, window: &[u8]) {
        // Queue the record window for the field-step driver to install after
        // the VM borrow ends (we can't mutate the encounter session while the
        // field bytecode is still borrowed).
        self.world.encounters.pending_scripted = Some(window.to_vec());
    }

    // PORT: FUN_8003C7EC
    fn op4c_n_e_sub_a_call_c7ec(&mut self) {
        // Field-VM op `4C EA` - the scripted game-over trigger. Retail
        // stores master mode 0x16 (22 = CARD INIT, the CARD/CONTINUE
        // title flow) + `_DAT_8007BB00 = 1` + clears `DAT_8007B750` bit 1,
        // then pauses the BGM (`FUN_800266E0(0x8007052C)` - the same
        // primitive as BGM sub-op 2). The engine's CARD/CONTINUE flow is
        // its defeat state: raise `game_over` and route a BGM pause.
        self.world.game_over = true;
        self.world.pending_field_events.push(FieldEvent::Bgm {
            text_id: 0,
            sub_op: 2,
        });
    }

    // Field-VM op `4C EB`: "run the next op only if this actor exists".
    // Retail resolves the id through `FUN_8003C83C`, the actor-list walk:
    // `0xF8` short-circuits to the player object, any other id matches a
    // live context's `+0x50` script id - the scene's channels here. The
    // trait default (always a miss) skipped every guarded op, so koin3's
    // entry script never raised the bit-3 flag on the dance hall's props.
    // REF: FUN_8003C83C
    fn op4c_n_e_sub_b_actor_jump(&mut self, actor_id: u8) -> Option<()> {
        let found = if actor_id == 0xF8 {
            self.world.player_actor_slot.is_some()
        } else {
            crate::field_channels::resolve_target(self.world.channel_view(), actor_id).is_some()
        };
        found.then_some(())
    }

    // Field-VM op `4C EC`: `_DAT_8007B5FC = FUN_801DDF48()` (`0x801E34F8`
    // `jal`, store in the `j` delay slot at `0x801E3508`) - reroll the
    // encounter step counter. No shipped script issues a clean `4C EC`
    // (field-op census), so this is reachable only from modded bytecode.
    // REF: FUN_801DDF48 (ported as region_encounter::encounter_counter_reroll)
    fn op4c_n_e_sub_c_capture_ddf48(&mut self) {
        self.world.reroll_encounter_step_counter();
    }

    fn op4c_n_e_sub_1_text_actor(&mut self, text_buf: &[u8], script_id: u16) {
        // Field-VM op `4C E1` - spawn the single-line text balloon
        // (`FUN_8003C764`). Replacing the Option is the retail
        // predecessor-kill (the handler's first lines kill any live
        // sibling balloon). The X centering waits for a host-side font
        // measurement; the tick lives in `World::tick`
        // (`crate::text_balloon::TextBalloon::tick`).
        let _ = script_id;
        self.world.cutscene.text_balloon = Some(crate::text_balloon::TextBalloon::spawn(text_buf));
    }

    fn op4c_n_e_sub2_fmv_trigger(&mut self, fmv_id: i16) {
        // Field-VM op `0x4C 0xE2` - retail handler at 0x801E30E4 writes
        // the resolved s16 to `_DAT_8007BA78` (FMV index) and pokes
        // `_DAT_8007B83C = 0x1A` (next game mode = 26 = StrInit). We
        // record the request here so the SceneHost / engine driver can
        // pop it after the field step returns and switch its scene
        // mode without invalidating the field-VM borrow.
        self.world.cutscene.pending_fmv_trigger = Some(fmv_id);
        self.world
            .pending_field_events
            .push(FieldEvent::FmvTrigger { fmv_id });
    }

    fn bgm(&mut self, text_id: u16, sub_op: u8) {
        Self::bgm(self, text_id, sub_op)
    }

    fn give_item(&mut self, item_id: u8) {
        // Op 0x39 GIVE_ITEM: add one of `item_id` to the inventory, capacity-
        // checked like the retail add-by-id primitive FUN_800421D4(item_id, 1).
        let slot = self.world.party.inventory.entry(item_id).or_insert(0);
        *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
        self.world
            .pending_field_events
            .push(FieldEvent::GiveItem { item_id });
    }

    fn op4c_n5_sub2_take_item(&mut self, item_id: u8) {
        Self::op4c_n5_sub2_take_item(self, item_id)
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        Self::open_dialog(self, text_id, inline, world_x, world_z, depth_id)
    }

    fn op4c_n_5_sub_4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        Self::op4c_n_5_sub_4_dialog_advance(self, _ctx)
    }

    /// Field-VM op `0x4C` outer-nibble-7 - rectangular collision-grid wall
    /// paint. Writes the high-nibble wall bits of the per-scene collision
    /// grid (`*(_DAT_1F8003EC) + 0x4000`), the same grid
    /// [`World::step_field_locomotion`] reads. The VM dispatcher has
    /// already turned the op operands into half-open tile ranges; we just
    /// apply the per-byte mutation. See [`World::paint_field_collision`].
    fn op4c_n7_tile_flag_bulk(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        self.world
            .paint_field_collision(sub, x_range, z_range, mask);
    }

    fn add_money(&mut self, delta: i32) {
        let new_total = (self.world.party.money as i64 + delta as i64).clamp(0, 9_999_999) as i32;
        self.world.party.money = new_total;
        self.world
            .pending_field_events
            .push(FieldEvent::AddMoney { delta });
    }

    /// Op `0x4E` party-bank read - the read side of the two purses `add_money`
    /// and the casino cash-out write. Sub-op 10 is party gold
    /// (`_DAT_8008459C`), sub-op 11 the casino coin bank (`_DAT_800845A4`);
    /// the VM normalises the 7-byte (sub-3 / sub-9) and 9-byte (sub-10 /
    /// sub-11) encoded forms onto that pair before calling.
    ///
    /// Without it every scripted gold gate reads an empty purse, so each of
    /// them takes its can't-afford branch: an inn stay, a paid tour, a train
    /// ticket and a casino coin purchase are all one `0x4E` sub-3 compare
    /// against a script literal (`legaia_asset::inn_costs`). That is the
    /// whole affordability check - retail has no inn routine and no cost
    /// table, so this read is what makes the charge reachable at all.
    fn party_bank_value(&self, sub_op: u8) -> i32 {
        match sub_op {
            11 => self.world.minigames.casino_coins.min(i32::MAX as u32) as i32,
            _ => self.world.party.money,
        }
    }

    fn set_item_count(&mut self, slot_byte: u8, count: u8) {
        if count == 0 {
            self.world.party.inventory.remove(&slot_byte);
        } else {
            self.world.party.inventory.insert(slot_byte, count);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetItemCount { slot_byte, count });
    }

    fn party_add(&mut self, char_id: u8) -> bool {
        Self::party_add(self, char_id)
    }

    fn party_remove(&mut self, char_id: u8) {
        // Retail op `0x3D`: find the id, shift the tail down, `count -= 1`;
        // the leader re-points at the new head when the count drops to 1
        // or the removed id was the leader.
        let mut list = self.world.present_party_list();
        if let Some(pos) = list.iter().position(|&id| id == char_id) {
            list.remove(pos);
            let leader_gone =
                matches!(self.world.party.party_leader_slot, Some(id) if id == char_id);
            if list.len() == 1 || leader_gone {
                self.world.party.party_leader_slot = list.first().copied();
            }
            self.world.install_present_party_list(list);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyRemove { char_id });
    }

    fn scripted_battle(&mut self, op0: u8, row: u8) {
        // `3E <op0> <row>` with `op0 < 100` or `op0 == 0xFF` - the
        // scripted-battle install. Retail's arm installs the per-scene MAN
        // formation-table row `row` as the SYSTEM entity's encounter record
        // (`sys_ctx[+0x8A] = 1`, `sys_ctx[+0x94] = formation_table +
        // row*stride + 1`) and requests the battle mode switch;
        // `FUN_801DA51C`'s confirm state then copies the row into the battle
        // formation cell. `op0` is never read past the `0xFF` / `< 100` test,
        // so `3E 00 02` (town0b), `3E 00 03` (stone) and `3E 01 00`
        // (jagaroom) install their rows exactly as garmel's Zeto beat's
        // `3E FF 09` does. Talking to an NPC never reaches this op: that is
        // the interaction probe's [`World::trigger_field_interact`].
        let _ = op0;
        self.world.trigger_scripted_battle(row);
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

    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneRegisterWrite {
                slot_10,
                slot_12,
                slot_14,
            });
    }

    fn op4c_n9_sub_f_retire_ladder_oscillators(&mut self) {
        // `4C 9F` is `FUN_8003CF40(_DAT_8007C34C, LAB_801DA930)`, a **retire
        // sweep** over the `0x801F27EC` handler - so it cancels every live
        // floor-height-ladder oscillator this scene spawned (sub-`0..2`),
        // and registers nothing. Retiring the engine's records is the whole
        // of that half; the VM advances past the op either way.
        self.world.terrain.floor_tier_bobs.clear();
        self.world.retire_floor_ladder_oscillators();
    }

    // `4C 86` / `4C 87` - the reflection controller's install and teardown,
    // consecutive slots of the nibble-8 sub-table.
    // REF: FUN_801E573C, FUN_8003CF40
    fn op4c_n8_sub6_install_reflection(
        &mut self,
        ctx: &mut FieldCtx,
        source_id: u8,
        words: [i16; 6],
    ) -> bool {
        // Retail's `a0` is the executing context itself - `FUN_801DE840`'s
        // third argument - so the script that issues the op is the mirror
        // image, and the operand byte names what it reflects.
        let ctx_is_player = self.ctx_is_player(ctx);
        self.world
            .spawn_reflection_controller(ctx_is_player, source_id, words)
            .is_some()
    }

    fn op4c_n8_sub7_retire_reflections(&mut self) {
        self.world.retire_reflection_controllers();
    }

    // -- the three frame-delta timer templates ---------------------------
    //
    // Each spawner allocates one pool actor from a plain template and fills
    // in its clock; the engine keeps the record on `World` instead and steps
    // it in `World::tick_field_timer_actors`. Kernels live in
    // `legaia_engine_vm::field_actor_timers`.

    /// Op `0x43` sub-`0xC` - the cinematic bar emitter (`FUN_801DE754`
    /// allocating `0x801F2858`, tick `FUN_801DD784`).
    ///
    /// The three operand bytes are the phase durations in ticks: bars close,
    /// hold, bars open. Retail allocates a fresh actor per call and the two
    /// would then both emit; the engine keeps one, because a second envelope
    /// over the first is a script defect rather than an effect.
    fn op43_alloc_scripted_actor(&mut self, b1: u8, b2: u8, b3: u8) {
        self.world.presentation.cinematic_bars =
            Some(legaia_engine_vm::field_actor_timers::ShutterBars::spawn(
                i16::from(b1),
                i16::from(b2),
                i16::from(b3),
            ));
        self.world.presentation.cinematic_bar = 0;
    }

    fn op43_sub9_tween(&mut self, ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        Self::op43_sub9_tween(self, ctx, x, y, z, ticks)
    }

    /// Op `0x4C` nibble-9 sub-`0xE` - install all sixteen rungs of the scene
    /// floor-height ladder at once.
    ///
    /// The paired install of the animator below, and the op that identifies
    /// the whole nibble-9 family: retail writes `-words[i]` into
    /// `0x1F800314 + 0x48 + i*2` (and `words[i]` into the MAN-header mirror
    /// at `_DAT_8007B898 + 2`), which is the same array
    /// [`crate::scene::SceneHost`] seeds from the MAN header's sixteen
    /// **negated** shorts on scene entry. `jou`'s entry script installs the
    /// linear ramp `i * 0x20` here and then sets rungs `4..` oscillating -
    /// the undulating organic floor.
    ///
    /// Both stores are in the arm's one loop (`0x801E24F8..0x801E2538`:
    /// `sh v0, 2(MAN + i*2)` then `sh -v0, 0x48(0x1F800314 + i*2)`), so the
    /// MAN-header ladder the camera composer swaps in around its floor
    /// sample follows the install too. Only the oscillators write the live
    /// rungs alone. Updating the live rungs without the mirror left the
    /// follow camera framing `concnow`'s pre-install ladder while the player
    /// walked the installed one - the eye sank under the raised floor.
    fn op4c_n9_sub_e_table_copy(&mut self, words: [i16; 16]) {
        let terrain = &mut self.world.terrain;
        for (i, w) in words.into_iter().enumerate() {
            terrain.floor_height_lut[i] = w.wrapping_neg();
            terrain.floor_height_lut_static[i] = w.wrapping_neg();
        }
    }

    /// Op `0x4C` nibble-9 sub-`0..2` - one rung of the scene floor-height
    /// ladder starts oscillating (`FUN_801DDE34` allocating `0x801F27EC`,
    /// tick `FUN_801DA930`).
    ///
    /// `b1` is the rung (`0..16`, the low nibble of a collision byte);
    /// `words` are the half-period, the amplitude and the burst-arm word.
    /// Retail seeds both the position and the rest height from the rung's
    /// current value, which is `World::terrain.floor_height_lut` here.
    fn op4c_n9_sub0_2_dde34(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        let seed = self
            .world
            .terrain
            .floor_height_lut
            .get(b1 as usize)
            .copied()
            .unwrap_or(0);
        self.world.terrain.floor_tier_bobs.push(
            legaia_engine_vm::field_actor_timers::FloorTierBob::spawn(
                u16::from(b1),
                sub,
                words[0],
                words[1],
                words[2],
                seed,
            ),
        );
    }

    fn op44_spawn_scene_record(&mut self, global_index: u8) {
        // Record the spawn request for SceneHost::tick to resolve against the
        // current scene MAN (the partition math needs the MAN header, which
        // the VM borrow precludes touching here). Retail spawns the record as
        // a sibling context immediately; the one-frame deferral is invisible
        // because a fresh context first runs on the NEXT frame slice anyway.
        // A bounded queue (not a single slot) so a second spawn issued while
        // another record executes is not dropped - retail's context table
        // holds several concurrent spawned records.
        // REF: FUN_8003BDE0
        if self.world.field_vm.pending_record_spawns.len() < crate::world::SPAWNED_CONTEXT_SLOTS {
            self.world.field_vm.pending_record_spawns.push(global_index);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SpawnRecord { global_index });
    }

    fn setup_animation(&mut self, _ctx: &mut FieldCtx, count: u8, base_id: u8, frames: &[u8]) {
        // Op `0x4B` on a placement's own context: its VDF morph lanes
        // (`World::arm_npc_morph`), not a clip - the arm writes the `+0xB0`
        // sub-entry bytes and the `+0xB8` / `+0xC8` ramp velocities and
        // touches neither `+0x5C` nor the clip pointer.
        // PORT: FUN_801DE840 (the op-`0x4B` arm)
        let owner = match (
            self.world.field_vm.executing_channel,
            self.world.field_vm.executing_object,
        ) {
            (Some(slot), _) => Some(crate::world::MorphOwner::Placement(slot)),
            (None, Some(record)) => Some(crate::world::MorphOwner::Object(record)),
            (None, None) => None,
        };
        if let Some(owner) = owner {
            self.world.arm_field_morph(owner, count, base_id, frames);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SetupAnimation {
                count,
                base_id,
                frames: frames.to_vec(),
            });
    }

    fn set_party_leader(&mut self, leader_id: u8) {
        self.world.party.party_leader_slot = Some(leader_id);
        self.world
            .pending_field_events
            .push(FieldEvent::SetPartyLeader { leader_id });
    }

    fn camera_configure(&mut self, params: &[CameraParam], apply_trigger: u16, mode: u8) {
        Self::camera_configure(self, params, apply_trigger, mode)
    }

    fn op4c_n_c_sub_d_camera_mover_live(&mut self) -> bool {
        self.world.camera.state.glide_frames > 0
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

    fn scene_fade(&mut self, op0_word: u16, op1_word: u16) -> SceneFadeResult {
        Self::scene_fade(self, op0_word, op1_word)
    }

    // Op `0x34` sub-1: the attached light (`FUN_801E5668`), seated on the
    // actor the op runs against. See `world/field_script_actors.rs`.
    fn op34_sub1_spawn_attached(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        spawn: &legaia_engine_vm::field_actor_billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        self.world.spawn_field_attached_light(ext, spawn, script)
    }

    // Op `0x43` sub-0/1/A/B: the scripted arc (`FUN_801D25EC`). An actor the
    // engine cannot place (the scene system context) gets no arc; its halt
    // stays raised exactly as before the arc channel existed.
    fn op43_arc_target_halted(&self, _ctx: &FieldCtx, ext: Option<u8>) -> bool {
        // The arm's acquire refuses an actor still mid arc
        // (`0x801DF3A4..0x801DF3CC`); the VM retries the op next frame.
        self.world.script_arc_target_halted(ext)
    }

    fn op43_arc_jump(
        &mut self,
        _ctx: &FieldCtx,
        ext: Option<u8>,
        req: &legaia_engine_vm::field_ledge_hop_arc::ScriptArcRequest,
    ) {
        // The watcher's release context: the arced NPC's own channel, or -
        // for a player arc - the channel that ran the op, which retail halts
        // with the player (`0x801DF3F0..0x801DF408`).
        let release = self.world.field_vm.executing_channel;
        if let Some(actor) = self.world.resolve_script_actor(ext) {
            self.world.start_field_script_arc(actor, req, release);
        }
    }

    fn op34_sub0_color_intensity_setup(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        Self::op34_sub0_color_intensity_setup(self, op0, rgb, intensity)
    }

    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        Self::effect_anim_trigger(self, ctx, arg)
    }

    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        Self::menu_ctrl_sub1(self, op0, payload)
    }

    // Sub-op 0x14: the actor clone. The whole body is
    // `World::spawn_actor_clone` (retail `FUN_801D835C` plus the arm's own
    // `FUN_8003C83C` resolve); the clone then ticks itself out through
    // `World::tick_handler_actors`.
    // REF: FUN_801D835C
    fn menu_ctrl_clone_actor(&mut self, src_id: u8, tint_rgb: u32, fade_rate: i16) {
        self.world.spawn_actor_clone(src_id, tint_rgb, fade_rate);
    }

    fn menu_refresh(&mut self) {
        self.world
            .pending_field_events
            .push(FieldEvent::MenuRefresh);
    }

    // REF: FUN_801DE840 (`4C 3A`, 0x801E10DC..0x801E10F4)
    fn apply_arrival_facing(&mut self) {
        self.world.apply_arrival_facing();
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

    fn region_attributes_refresh_at_player(&mut self) {
        self.world
            .push_camera_zone_request(CameraZoneRequest::RefreshAttributes);
    }

    // Op `0x4C` outer-nibble-3 sub-0 / sub-1: the ambient-particle master
    // gate `_DAT_8007B854`. Retail keeps it in one word that the emitter
    // re-reads every frame; the port keeps a copy on each live emitter
    // element, so the single write fans out over the channel here. Both hosts
    // run the channel off `World::tick_cutscene_elements`, so this one sink
    // serves native and browser alike.
    fn set_ambient_particle_gate(&mut self, enabled: bool) {
        self.world.set_ambient_particles_enabled(enabled);
    }

    fn move_to(&mut self, ctx: &mut FieldCtx, world_x: u16, world_z: u16, is_player: bool) {
        Self::move_to(self, ctx, world_x, world_z, is_player)
    }

    // `4C 37` (nibble 3 sub 7): copy the player's position and heading onto
    // the executing context - how a cutscene brings a party actor in at the
    // player (`bylon` `P2[9]` `CC 48 37`, then `C7 48 ..` walks it off). The
    // arm skips the player's own context - by context identity, not by the
    // party-bank bit `0x01000000` a party actor's `4C 50 F1` model select
    // raises (placement 37 there carries it). The default hook answered
    // `None`, so the actor kept its parked seat and the walk started at the
    // off-map hide box.
    // REF: FUN_801DE840 (nibble-3 sub-7)
    fn fetch_player_coords(&self, ctx: &FieldCtx) -> Option<vm::field::PlayerCoords> {
        if ctx.script_id == 0xF8 {
            return None;
        }
        let slot = self.world.player_actor_slot?;
        let ms = &self.world.actors.get(slot as usize)?.move_state;
        Some(vm::field::PlayerCoords {
            world_x: ms.world_x as u16,
            world_y: ms.world_y as u16,
            world_z: ms.world_z as u16,
            field_26: ms.render_26 as u16,
        })
    }

    fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
        Self::exec_move(self, ctx, move_id)
    }

    fn op4c_n5_sub0_set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, _high: bool) {
        Self::op4c_n5_sub0_set_actor_model(self, ctx, value, _high)
    }

    /// Op `4C CE <value>` - store the player clip override `_DAT_8007B6AC`
    /// (`lbu v1, 1(s6); sw v1, -0x4954(v0)` at `0x801E2A24..0x801E2A30`),
    /// which the settle tail and the player clip arms read.
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xE` arm)
    fn op4c_n_c_sub_e_set_b6ac(&mut self, value: u8) {
        self.world.locomotion.clip_override = u32::from(value);
    }

    /// Op `4C CA <slot> <u16>` - store into the script-counter slot table
    /// `0x801C6460` (`sh v0, 0(v1)` at `0x801E2954`). See
    /// [`crate::world::FieldVmState::slot_table`].
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xA` arm)
    fn op4c_n_c_sub_a_set_slot(&mut self, slot: u8, value: i16) {
        self.world.field_vm.slot_table[usize::from(slot)] = value;
    }

    /// Op `4C CB` / `4C CC <slot> <u16>` - add to / subtract from a slot
    /// (`lhu` / `addu` or `subu` / `sh` at `0x801E2988..0x801E29DC`): a
    /// 16-bit wrapping update. The VM has already substituted the frame tick
    /// for a `0xFFFF` literal.
    ///
    /// REF: FUN_801DE840 (the nibble-C sub-`0xB` / sub-`0xC` arms)
    fn op4c_n_c_sub_bc_adjust_slot(&mut self, slot: u8, delta: i16, subtract: bool) {
        let cell = &mut self.world.field_vm.slot_table[usize::from(slot)];
        *cell = if subtract {
            cell.wrapping_sub(delta)
        } else {
            cell.wrapping_add(delta)
        };
    }

    /// Op `0x4E` sub-ops `5..=8` - the signed slot read (`0x801E0B0C`).
    fn slot_table_read(&self, slot: u8) -> i16 {
        self.world.field_vm.slot_table[usize::from(slot)]
    }

    /// Op `4C C1` - re-derive every fog region's enable byte from its story
    /// flag: for each record of the `_DAT_80073ED8` table (count
    /// `DAT_80073EDC`, stride `0xB`), `FUN_8003CE64(rec[9] | rec[10] << 8)`
    /// and `rec[0] = flag set ? 0 : 1` (`0x801E2674..0x801E26EC`). The fog
    /// spawner ends its region search on the first box holding the tile and
    /// spawns nothing when that record's byte 0 is clear, so this is what
    /// keeps a region's fog off once its story beat has passed: retail's
    /// `retock` inn (flag `0x51C`), `rikuroa` (`0x007`) and `garmel`
    /// (`0x007`) all hold the reset result, with an empty pool where every
    /// covering region is off.
    ///
    /// PORT: FUN_801DE840 (the nibble-C sub-`1` arm)
    fn op4c_n_c_sub_1_flag_loop_reset(&mut self, _flags: &[u8]) {
        let enables: Vec<bool> = self
            .world
            .fog
            .regions
            .iter()
            .map(|r| !self.world.system_flag_test(r.flag_index))
            .collect();
        for (r, on) in self.world.fog.regions.iter_mut().zip(enables) {
            r.enabled = on;
        }
    }

    /// Op `0x4C 0x61` - scripted CLUT-cell effect (one-shot cell write /
    /// cross-fade spawn). Decodes the 14-byte operand payload and queues the
    /// effect on the world; [`World::step_clut_fx`] applies it against the
    /// host's software VRAM on the retail game-tick cadence.
    ///
    /// PORT: FUN_801E4C58
    fn op4c_n6_sub_61_emitter(&mut self, _ctx: &mut FieldCtx, payload: [u8; 14]) {
        self.world.spawn_clut_cell_fx(&payload);
    }

    /// Op `0x4C 0xDB` - spawn the single-source CLUT blend fade
    /// (`FUN_801E57F0` -> handler `FUN_801E4D8C`). `bytecode` starts at the
    /// `0xDB` byte, the record pointer retail parks at actor `+0x90`; the
    /// fade runs on [`World::step_clut_fx`]'s game-tick bank against the
    /// host's software VRAM.
    ///
    /// REF: FUN_801E57F0
    fn op4c_n_d_sub_b_call_e57f0(&mut self, bytecode: &[u8]) {
        self.world.spawn_clut_blend_fx(bytecode);
    }

    /// Op `0x4C 0xE5` - the casino coin bank's script delta (a cabinet's
    /// fee, the dome's entry fee, a prize price). The arm and its missing
    /// lower clamp are documented on [`crate::casino_coin_bank`].
    fn op4c_n_e_sub_5_add_coins(&mut self, coin_delta: i32) {
        self.world.add_script_coins(coin_delta);
    }

    /// Op `0x4C 0x60` - literal-operand VRAM `MoveImage`. The six words are
    /// `[src_x, src_y, w, h, dst_x, dst_y]`; retail's handler arm hands them
    /// straight to the libgpu `MoveImage` wrapper. Queued on the world;
    /// [`World::apply_script_vram_moves`] drains the queue against the
    /// host's software VRAM. Retail's known user: the one-shot face-frame
    /// stamps onto the player texture atlas (town01's opening record stamps
    /// the Noa blink/mouth cells - see `docs/formats/character-mesh.md`).
    ///
    /// PORT: FUN_80058490 (consumer: `jal` at 0x801E1B84)
    /// REF: FUN_801DE840 (sub-0x60 handler arm 0x801E1B28..0x801E1B90),
    /// FUN_8003CE9C (misaligned-u16 operand reads)
    fn op4c_n6_sub0_emitter6(&mut self, words: [i16; 6]) {
        self.world.queue_script_vram_move(words);
    }

    /// Op `4C D4` - set the mask bit on a 16x1 VRAM run's non-zero words.
    /// `teien` `P1[0]` runs it over every CLUT of rows 505..507 before it
    /// installs the HSV cycler (`34 30 06`) that darkens them for the night
    /// garden; without the bit, the entries the cycler takes to black read
    /// `0x0000` and the hedge texels behind them go transparent.
    fn op4c_n_d_sub_4_vram_stp_set(&mut self, vram_x: u16, vram_y: u16) {
        self.world
            .ambient
            .script_vram_stp
            .push((vram_x, vram_y, true));
    }

    /// Op `4C D5` - clear the mask bit on a 16x1 VRAM run (every word but
    /// `0x8000`).
    fn op4c_n_d_sub_5_vram_stp_clear(&mut self, vram_x: u16, vram_y: u16) {
        self.world
            .ambient
            .script_vram_stp
            .push((vram_x, vram_y, false));
    }

    /// Op `0x43` sub-`0x12` - the GP0 `0x80` VRAM rectangle copy, after the
    /// VM has resolved the arm's two-page split into one or two
    /// `FUN_800468A4` calls. Queued on the world the same way the `4C 60`
    /// `MoveImage` family is; [`World::apply_vram_rect_copies`] runs each
    /// call through the retail enqueue and executes the resulting packet
    /// against the host's software VRAM.
    ///
    /// REF: FUN_800468a4 (the enqueue the drain runs each call through)
    fn op43_vram_rect_copy(&mut self, calls: &[vm::vram_rect_copy::RectCopyCall]) {
        self.world.queue_vram_rect_copies(calls);
    }

    fn op4c_n8_sub2_restore_party_slot(&mut self, slot: u8) {
        Self::op4c_n8_sub2_restore_party_slot(self, slot)
    }

    /// `[4C, 0x84, amplitude]` - the screen-shake amplitude `_DAT_8007B630`.
    ///
    /// This opcode is the global's only retail writer and the global is the
    /// only input to `FUN_801D9D30`, so the field script is the sole source
    /// of a camera shake. The world holds it for the camera to read; see
    /// [`crate::world::CameraRig::shake_amplitude`].
    fn op4c_n8_sub4_set_b630(&mut self, value: u8) {
        self.world.camera.shake_amplitude = value;
    }

    /// `[4C, 0x89, lo, hi]` - the pager's automatic-press countdown
    /// `_DAT_80073F00`, which a waiting box page counts down and then
    /// presses through ([`crate::dialog::OwnedDialogPanel::tick_at_auto`]).
    fn op4c_n8_sub9_set_73f00(&mut self, value: i16) {
        self.world.dialog.auto_press = value;
    }

    /// `[4C, 0x8A, ...]` - the field light: the angle trio
    /// `_DAT_8007B780..84` and the back colour `_DAT_8007B788` the
    /// light-source TMD rows shade through
    /// ([`legaia_render_kernels::field_light`]). `town01`'s `P1[0]` sets a white
    /// back colour; `koin3`'s cutscene records drop it to black and back.
    fn op4c_n8_sub_a_write_quad(&mut self, slots: [i16; 3], packed: u32) {
        self.world.presentation.field_light =
            legaia_render_kernels::field_light::FieldLight::from_op_4c_8a(slots, packed);
    }

    fn op4c_n8_sub_0_actor_allocator(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        Self::op4c_n8_sub_0_actor_allocator(self, _ctx, count, tail)
    }

    fn op4c_n5_sub1_npc_run(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        Self::op4c_n5_sub1_npc_run(self, ctx, world_x, world_z, depth_byte, move_id, is_player)
    }

    fn op4c_n_c_sub_3_script_teleport(&mut self, ctx: &mut FieldCtx) {
        Self::op4c_n_c_sub_3_script_teleport(self, ctx)
    }

    // Op 0x4C nibble-D sub-3 - arm the scripted countdown timer. The three
    // operands are the installer's four global writes; the world keeps them
    // as one `EscapeTimer` plus the packed flag word, and `World::tick`
    // drains it once per retail frame.
    // REF: FUN_801DE840 case 0xD sub 3, FUN_801D2EBC (the drain)
    fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
        self.world.schedule_timed_flags(ab, cd, ef);
    }

    fn op4c_n_d_sub8_call_d77f4(&mut self, b1: u8, words: [i16; 3]) {
        Self::op4c_n_d_sub8_call_d77f4(self, b1, words)
    }
}
