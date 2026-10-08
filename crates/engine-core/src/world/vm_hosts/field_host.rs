//! The field VM's `FieldHost` bridge (`FieldHostImpl`) and the field-step
//! helpers it routes through. Split out of `vm_hosts.rs`.

use super::*;

/// Apply the op-`0x4C 0xC3` script-table teleport onto `ctx`: re-seat the
/// executing context at its **own** MAN record's placement header.
///
/// Retail resolves the record via `FUN_8003C8F0(ctx+0x50, 0)`. That helper's
/// `param_2` is a *partition base* selector, and the call passes `0`, so the
/// base is zero and `param_1` (the context's `+0x50` script id) indexes the
/// **flattened** `[P0..P1..P2]` record table directly. Since the placement
/// spawner writes `+0x50 = N0 + placement_index`, the record it lands on is
/// the context's own partition-1 placement record - NOT partition-0 record
/// `script_id`. [`crate::man_field_scripts::flat_record_span`] models that
/// index space; the partition-1 header shape then applies, which is what
/// `FUN_8003D0BC` (skip the `[u8 N][N*2]` name/locals field) plus the 4-byte
/// placement header `[model, anim, bx, bz]` expects.
///
/// It then writes (`overlay_0897_801de840.txt` case 3 of the nibble-C
/// dispatcher):
///
/// - `+0x14`/`+0x18` (`world_x`/`world_z`) = tile centre
///   `(b & 0x7F) * 0x80 + 0x40`, plus another `+0x40` when bit 7 is set;
/// - `+0x5C` ([`FieldCtx::move_id`]) = the header's anim byte;
/// - `+0x6A` = 8, `+0x72` = `0x1000`, `+0x62` = `0x15`;
/// - `+0x54` (wait accumulator), `+0x8E`, `+0x8B` = 0;
/// - flags word `&= 0x9EBFFAFE` (clears the halt bit `0x400` among others);
/// - `+0x8C`/`+0x8D` = tile column/row recomputed from the new world
///   position (signed `(w - 0x40) >> 7`).
///
/// - `+0x78` (the tint blend) = 0.
///
/// The `+0x9E` rebase and the `0x25` spawn-section re-run that follow are
/// [`FieldHostImpl::rerun_spawn_section`]. Not modelled (no [`FieldCtx`]
/// counterpart): the `+0x70`/`+0x9C`/`+0x2A` zeroes and the linked `+0x44`
/// struct's `+0x9A = 0xFFFF` write.
///
/// Returns `false` (ctx untouched) when the record cannot be resolved.
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

pub(in crate::world) fn apply_script_table_teleport(
    man_file: &legaia_asset::man_section::ManFile,
    man: &[u8],
    ctx: &mut FieldCtx,
) -> bool {
    let Some((script_start, pc0, _body_len)) =
        crate::man_field_scripts::flat_record_span(man_file, man, ctx.script_id as usize)
    else {
        return false;
    };
    // The 4-byte placement header sits right before the record's first
    // opcode (`pc0 = 1 + N*2 + 4`): [model, anim, bx, bz].
    let Some(hdr) = man.get(script_start + pc0 - 4..script_start + pc0) else {
        return false;
    };
    let (anim, bx, bz) = (hdr[1], hdr[2], hdr[3]);
    let center = |b: u8| -> u16 {
        let base = u16::from(b & 0x7F) * 0x80 + 0x40;
        if b & 0x80 != 0 { base + 0x40 } else { base }
    };
    ctx.world_x = center(bx);
    ctx.world_z = center(bz);
    ctx.move_id = u16::from(anim);
    ctx.field_6a = 8;
    ctx.field_72 = 0x1000;
    ctx.wait_accum = 0;
    ctx.field_8e = 0;
    ctx.field_78 = 0;
    ctx.local_flags = 0x15;
    ctx.flags &= 0x9EBF_FAFE;
    // Tile column/row from the fresh world position - the retail signed
    // `>> 7` keeps a negative-rounding fixup that is unreachable here (the
    // tile-centre values are always positive); mirrored for fidelity.
    let grid = |w: u16| -> u8 {
        let w = i32::from(w as i16);
        let v = if w - 0x40 < 0 { w + 0x3F } else { w - 0x40 };
        (v >> 7) as u8
    };
    ctx.npc_x = grid(ctx.world_x);
    // `+0x8D` (named for its op-0x23 facing role) carries the tile ROW here.
    ctx.npc_facing = grid(ctx.world_z);
    ctx.field_8b = 0;
    true
}

impl FieldHostImpl<'_> {
    /// The second half of op `4C C3`: re-run the re-seated context's spawn
    /// section.
    ///
    /// After the teleport retail rebases the context's script offset `+0x9E`
    /// onto its record's first opcode (`0x801E2798..0x801E27A4`), and when
    /// that opcode is `0x25` it raises the scene word `*(_DAT_801C6EA4) + 8`,
    /// runs the context through `FUN_8003CF7C` from there and drops the word
    /// again (`0x801E2800..0x801E282C`). `FUN_8003CF7C` executes ops until it
    /// has run a `0x21`, the PC stops moving, or the next byte is below the
    /// opcode band - the same slice the scene-entry install gives a
    /// placement (`FUN_8003A1E4`), so the section's story-flag dispatch picks
    /// the actor's seat again from the live flags. `nilboa` `P2[27]` sends
    /// the Fire Ravine boulders home this way and their sections, finding
    /// `0x457` set, put them back where they were pushed.
    ///
    /// The slice runs with the scene-entry pre-run's semantics
    /// ([`crate::world::FieldVmState::entry_prerun`]): a seat op seats this
    /// context and never the player. An op the section aims at another
    /// context is stepped over by its width; the caller writes the final
    /// position through to the actor.
    ///
    /// REF: FUN_8003CF7C, FUN_801DE840 (`0x801E2798..0x801E282C`)
    fn rerun_spawn_section(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        ctx: &mut FieldCtx,
    ) {
        if self.world.field_vm.respawn_rerun {
            return;
        }
        let Some((start, pc0, _len)) =
            crate::man_field_scripts::flat_record_span(man_file, man, ctx.script_id as usize)
        else {
            return;
        };
        let Some(bc) = man.get(start..) else {
            return;
        };
        if bc.get(pc0) != Some(&0x25) {
            return;
        }
        let prev_prerun = self.world.field_vm.entry_prerun;
        self.world.field_vm.entry_prerun = true;
        self.world.field_vm.respawn_rerun = true;
        let own = ctx.script_id;
        let mut pc = pc0;
        for _ in 0..256 {
            let Some(&op) = bc.get(pc) else {
                break;
            };
            if op & 0x7F < 0x20 {
                break;
            }
            let foreign = vm::field::peek_extended(bc, pc).is_some_and(|t| u16::from(t) != own);
            let next = if foreign {
                match legaia_asset::field_disasm::decode(bc, pc) {
                    Ok(insn) => pc + insn.size,
                    Err(_) => break,
                }
            } else {
                match field_step_routed(self, ctx, bc, pc) {
                    FieldStepResult::Advance { next_pc } => next_pc,
                    FieldStepResult::Yield { .. }
                    | FieldStepResult::Halt { .. }
                    | FieldStepResult::Pending { .. }
                    | FieldStepResult::Unknown { .. } => break,
                }
            };
            if op == 0x21 || next == pc {
                break;
            }
            pc = next;
        }
        self.world.field_vm.respawn_rerun = false;
        self.world.field_vm.entry_prerun = prev_prerun;
    }
}

/// `true` when the op at `pc` is `CC F8 40`: op `4C` nibble-4 sub-0 (the
/// `+0x72` write-or-ramp) aimed at the player anchor `0xF8`.
pub(in crate::world) fn is_player_scale_op(bytecode: &[u8], pc: usize) -> bool {
    bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&crate::field_env::PLAYER_ANCHOR_TARGET)
        && bytecode.get(pc + 2) == Some(&0x40)
}

/// `true` when the op at `pc` is `CC F8 C2`: op `4C` nibble-C sub-2 (the
/// `+0x42` byte write, `0x801E26F0..0x801E26FC`) aimed at the player anchor.
/// Five shipped sites raise or lower the player's object-effect gate this way.
pub(in crate::world) fn is_player_effect_gate_op(bytecode: &[u8], pc: usize) -> bool {
    bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&crate::field_env::PLAYER_ANCHOR_TARGET)
        && matches!(bytecode.get(pc + 2), Some(&(0xC2 | 0x45)))
}

/// Step one field-VM op, landing a player-aimed `+0x72` write on the player.
///
/// `FUN_8003C83C` resolves the extended target `0xF8` to the live player
/// object (`_DAT_8007C364`), so `CC F8 40 lo hi tlo thi` writes - or, with a
/// non-zero tick count, ramps - the **player's** `+0x72`: the pad step's speed
/// multiplier and `FUN_8001B964`'s render scale, where `0` is "do not draw".
/// Every runner that can meet the op (the system script, the cutscene
/// timeline, the placement channels, an inline talk, a prop run) calls this in
/// place of [`vm::field::step`], so the op runs on a stand-in player context
/// seeded from the live word and the result is written back, whichever
/// script issued it. Cutscenes hide the player this way (`CC F8 40 00 00 ..`
/// before a stand-in walks) and restore it with `CC F8 40 00 10 ..`.
///
/// Every other op passes straight through to [`vm::field::step`].
///
/// REF: FUN_8003C83C, FUN_801DE840 (the nibble-4 sub-0 arm `0x801E1174..0x801E11A8`)
pub(in crate::world) fn field_step_routed(
    host: &mut FieldHostImpl<'_>,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
) -> vm::field::StepResult {
    // `CC F8 C2 <b>`: the player's object-effect gate `+0x42`, on the same
    // stand-in context the scale op uses, seeded from and written back to the
    // world's player word. `CC F8 45 ..` (the player's look rotation) runs on
    // the same stand-in, which `look_key` reads as the player.
    if is_player_effect_gate_op(bytecode, pc) {
        let mut player_ctx = FieldCtx {
            script_id: u16::from(crate::field_env::PLAYER_ANCHOR_TARGET),
            flags: 0x0100_0000,
            field_42: host.world.field_vm.player_field_42,
            ..Default::default()
        };
        let r = vm::field::step(host, &mut player_ctx, bytecode, pc);
        host.world.field_vm.player_field_42 = player_ctx.field_42;
        return r;
    }
    let slot = host
        .world
        .player_actor_slot
        .map(usize::from)
        .filter(|&s| s < host.world.actors.len());
    let Some(slot) = slot.filter(|_| is_player_scale_op(bytecode, pc)) else {
        return vm::field::step(host, ctx, bytecode, pc);
    };
    let mut player_ctx = FieldCtx {
        script_id: u16::from(crate::field_env::PLAYER_ANCHOR_TARGET),
        flags: 0x0100_0000,
        field_72: host.world.actors[slot].move_state.field_72,
        ..Default::default()
    };
    let r = vm::field::step(host, &mut player_ctx, bytecode, pc);
    if let Some(a) = host.world.actors.get_mut(slot) {
        a.move_state.field_72 = player_ctx.field_72;
    }
    r
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
        ctx.flags & vm::field_player_clip::PARTY_BANK_FLAG != 0
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
    // `4C E3 <src>`: the executing context takes the source actor's
    // position `+0x14/+0x16/+0x18` and heading `+0x26`
    // (`0x801E3108..0x801E314C`). In the cross-context form
    // `CC <dst> E3 <src>` the destination is `<dst>` - a party placement
    // stepping onto the player's spot facing his way (`CC 09 E3 F8` in
    // `stone`). Headings copy in the engine's own space on both sides.
    // REF: FUN_801DE840 (0x801E3108..0x801E31B0), FUN_8003C83C
    fn op4c_n_e_sub_3_actor_sync_camera(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        let w = &mut *self.world;
        let src = if actor_id == crate::field_env::PLAYER_ANCHOR_TARGET {
            w.player_actor_slot
                .and_then(|s| w.actors.get(usize::from(s)))
                .map(|a| {
                    (
                        a.move_state.world_x,
                        a.move_state.world_z,
                        a.move_state.render_26,
                    )
                })
        } else {
            let view = w.channel_view();
            crate::field_channels::resolve_target(view, actor_id).map(|ci| {
                let ch = &view[ci];
                let own = (ch.ctx.world_x as i16, ch.ctx.world_z as i16);
                let slot = (!ch.object_bind)
                    .then(|| u8::try_from(ch.placement_index).ok())
                    .flatten();
                let (x, z) = slot
                    .and_then(|s| w.npcs.positions.get(&s).copied())
                    .unwrap_or(own);
                let h = slot
                    .and_then(|s| w.npcs.headings.get(&s).copied())
                    .unwrap_or(0x800);
                (x, z, h)
            })
        };
        let Some((x, z, heading)) = src else {
            return;
        };
        if ctx.script_id == u16::from(crate::field_env::PLAYER_ANCHOR_TARGET)
            && w.field_vm.executing_channel.is_none()
        {
            let y = w.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
            if let Some(a) = w
                .player_actor_slot
                .and_then(|s| w.actors.get_mut(usize::from(s)))
            {
                a.move_state.world_x = x;
                a.move_state.world_z = z;
                a.move_state.world_y = y;
                a.move_state.render_26 = heading;
            }
            return;
        }
        ctx.world_x = x as u16;
        ctx.world_z = z as u16;
        if let Some(slot) = w.field_vm.executing_channel {
            w.npcs.positions.insert(slot, (x, z));
            w.npcs.motions.remove(&slot);
            w.npcs.headings.insert(slot, heading);
        }
    }

    fn player_cflag(&mut self, bit: u8, set: bool) -> bool {
        self.world.field_player_cflag(bit, set)
    }

    fn player_set_model(&mut self, value: i16) -> bool {
        self.world.field_player_set_model(value)
    }

    // Op `0x38` simple path: the compass-LUT heading lands on the resolved
    // actor's `+0x26`. The extended `0xF8` (and a context that is the player)
    // turns the player; a placement channel stepping - its own script or a
    // poke on it - turns that placement; the inline talk runner turns the NPC
    // whose record it runs. Any other context turns itself: its own `+0x26`
    // takes the retail heading, which is what an object-bind context's draw
    // composes (`World::object_draw_turns`) - `town0c`'s exit rocks turn by
    // `38 83 00` in their bind prologue.
    // REF: FUN_801DE840 (case 0x38), FUN_8003C83C
    fn face_compass(&mut self, ctx: &mut FieldCtx, index: u8, player: bool) {
        let Some(heading) = crate::man_field_scripts::facing_index_to_engine_heading(index) else {
            return;
        };
        if !player {
            // Engine space is retail + 0x800.
            ctx.field_26 = (heading as u16).wrapping_sub(0x800) & 0x0FFF;
        }
        let channel = self.world.field_vm.executing_channel;
        if player
            || (channel.is_none()
                && ctx.script_id == u16::from(crate::field_env::PLAYER_ANCHOR_TARGET))
        {
            if let Some(slot) = self.world.player_actor_slot
                && let Some(actor) = self.world.actors.get_mut(slot as usize)
            {
                actor.move_state.render_26 = heading;
            }
            return;
        }
        if let Some(slot) = channel.or(self.world.dialog.stepping_inline_npc) {
            self.world.npcs.headings.insert(slot, heading);
        }
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

    // Op `0x4C` outer-nibble-4 subs `0xA..=0xD` - four scene globals written
    // or ramped from one script operand. Subs `0xA`/`0xB`/`0xC` are the three
    // the world-map frame pump `FUN_801D1344` forwards into the horizon
    // emitter gate, which is why the engine parks them on the world-map
    // controller: `_DAT_8007BCD0` (`sw v0,-0x4330(v1)` at `0x801E1648`),
    // `_DAT_8007BCD4` (`0x801E1688`) and `_DAT_8007BCD8` (`0x801E16C8`).
    // Sub `0xD` scales its operand by `_DAT_8008457C >> 12` before storing to
    // `_DAT_8007B910` (`0x801E1700..0x801E1720`); the engine has no consumer
    // for that slot, so it is dropped rather than parked somewhere a reader
    // would then have to be invented for.
    //
    // The ramp arms post the endpoint immediately for the same reason the
    // sub-9 ramp does - the gate reads a level, not a trajectory.
    // REF: FUN_801D1344 (the gate arm), FUN_801DE840 (these four arms)
    // Op `0x4C` nibble-4 sub-0 with a tick count, aimed at the player
    // ([`field_step_routed`] runs it on the player's stand-in context): the
    // arm hands `&ctx+0x72` to the generic ramp scheduler, kind 2, from the
    // live `+0x72` (`lhu a3,0x72(a0)`) to the operand over `ticks` frames
    // (`0x801E118C..0x801E11A8` -> `jal 0x8003C5F0` at `0x801E205C`). Other
    // contexts' `+0x72` ramps still drop: no port reader animates them.
    // REF: FUN_8003C5F0
    /// Op `4C 81` - the actor's draw tint (`+0x74` colour, `+0x78` blend).
    /// See `world::object_actor_height`.
    ///
    /// PORT: FUN_801DE840 (the nibble-8 sub-1 arm, `0x801E1FC4..0x801E2068`)
    fn op4c_n_8_sub_1_set_tint(
        &mut self,
        ctx: &mut FieldCtx,
        target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        let player = self.ctx_is_player(ctx);
        self.world
            .set_actor_tint(ctx, target, player, colour, blend, ticks);
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
        // Sub-8: a heading tween on a placement, from its live heading
        // (retail space) - the `FUN_8003C5F0` ramp the scheduler lerps.
        if sub == 8 {
            if let Some(HeadingOwner::Placement(slot)) = self.world.heading_owner(ctx) {
                use vm::ambient_motion::{RAMP_DEST_HEADING, Ramp, RampKind};
                let start = i32::from(self.world.npcs.heading(slot).wrapping_sub(0x800));
                let total = i32::from(ticks);
                self.world.npcs.heading_ramps.free_owner(u32::from(slot));
                self.world.npcs.heading_ramps.install(Ramp {
                    dest: RAMP_DEST_HEADING,
                    owner: u32::from(slot),
                    start,
                    end: i32::from(target),
                    total,
                    remaining: total,
                    kind: RampKind::U16,
                });
            }
            return;
        }
        // Sub-2 on a placed object's actor: the `+0x8E` tween the actor
        // tick's `0x20000000` height law turns into its Y (`chitei2`'s
        // falling boulder). See `world::object_actor_height`.
        if sub == 2
            && let Some(record) = self.world.field_vm.executing_object
        {
            self.world
                .schedule_object_slot_ramp(record, ctx.field_8e, target, ticks);
            return;
        }
        if sub != 0 || ctx.script_id != u16::from(crate::field_env::PLAYER_ANCHOR_TARGET) {
            return;
        }
        use vm::ambient_motion::{RAMP_DEST_SCALE, Ramp, RampKind};
        let total = i32::from(ticks);
        self.world.locomotion.player_scale_ramps.install(Ramp {
            dest: RAMP_DEST_SCALE,
            owner: u32::from(crate::field_env::PLAYER_ANCHOR_TARGET),
            start: i32::from(ctx.field_72),
            end: i32::from(target),
            total,
            remaining: total,
            kind: RampKind::U16,
        });
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
        // A field-VM-opened gold shop (op 0x49 sub-0 inline shop record) gates
        // the resume the same way name-entry does: Armed while the shop UI is
        // up, Done once the host closes it (`finish_field_shop`), so the VM
        // suspends across the shop and then advances past the merchant op.
        if self.world.shops.shop_armed {
            return if self.world.shops.shop_open {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        // A sub-7 casino prize exchange (`World::try_arm_prize_exchange`):
        // same shape as the gold shop - Armed while the exchange UI is up,
        // Done once the host closes it (`finish_prize_exchange`).
        if self.world.shops.prize_exchange_armed {
            return if self.world.shops.prize_exchange_open {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        // A sub-5 tile-board install (`World::try_install_tile_board`): Armed
        // while the board mode runs, Done once an event cell exits it, so the
        // script suspends across the whole board segment.
        if self.world.board.armed {
            return if self.world.board.grid.is_some() {
                Op49State::Armed
            } else {
                Op49State::Done
            };
        }
        if self.world.cutscene.prologue_naming_armed
            && self.world.cutscene.naming_owner == Some(self.world.op49_park_owner())
        {
            if self.world.name_entry_active() {
                Op49State::Armed
            } else {
                Op49State::Done
            }
        } else {
            // Every other sub-op parks on `_DAT_8007B450` until the
            // `FUN_801F159C`-class actor writes `1` there. That writer is
            // `World::tick_submode_screen`; without it these sub-ops re-armed
            // and halted on the same PC every frame, forever.
            //
            // The park is read only by the context that armed it
            // (`Op49ParkOwner`): retail's `_DAT_8007B450` is one global, but
            // the port steps the field script, the channels and the spawned
            // record contexts inside one `World::tick`, so a screen armed by
            // the field script must not park the cutscene timeline's own
            // op-`0x49` - which is the town01 name-entry hand-off.
            let owner = self.world.op49_park_owner();
            if self.world.field_vm.submode_screen.is_open_for(owner) {
                Op49State::Armed
            } else if self.world.field_vm.submode_screen.is_done_for(owner) {
                Op49State::Done
            } else {
                Op49State::Idle
            }
        }
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
        // Retail's `sw zero,-0x4bb0(s0)` on the Done edge.
        self.world.clear_op49_park();
        // The shop op's resume ran: drop the arm so a later op-0x49 can open
        // the next merchant. (Name-entry clears via its own pending flags.)
        self.world.shops.shop_armed = false;
        // Same for a finished prize exchange (sub-7).
        self.world.shops.prize_exchange_armed = false;
        // A finished tile-board segment resumes the same way.
        self.world.board.armed = false;
        // The submode screen's Done is one-shot: consume it so the next
        // op-0x49 opens a fresh screen rather than resuming instantly. Only
        // the context that armed the park may consume it - the name-entry
        // hand-off's own resume runs through this same hook, and clearing
        // another context's pending Done would strand it back on Idle and
        // re-open the screen it had just finished.
        let owner = self.world.op49_park_owner();
        if self.world.field_vm.submode_screen.owner == owner {
            self.world.field_vm.submode_screen.done = false;
        }
        // A finished naming prompt outside the opening is spent with its
        // resume, so the context's later op-0x49s park on their own screens.
        // The opening timeline keeps its latch: its later STATE_RESUMEs have
        // always resolved through it.
        if self.world.cutscene.prologue_naming_armed
            && !self.world.name_entry_active()
            && self.world.cutscene.naming_owner == Some(owner)
            && owner != crate::field_submode_screen::Op49ParkOwner::CutsceneTimeline
        {
            self.world.cutscene.prologue_naming_pending = false;
            self.world.cutscene.prologue_naming_armed = false;
            self.world.cutscene.naming_owner = None;
        }
    }
    fn op49_menu_request(&mut self, sub_op: u8, instr: &[u8]) {
        // Recognise + open an inline gold shop (sub-0); non-shop op-0x49 sub-0
        // payloads (inn / save prompts) fail the priced-record validation.
        if sub_op == 0 {
            self.world.try_arm_field_shop(instr);
        }
        // Sub-5 carries the inline 13-byte tile-board header: install the
        // board (retail arms `_DAT_8007b450` at this window and the board
        // consumer takes over the frame).
        if sub_op == 5 {
            self.world.try_install_tile_board(instr);
        }
        // Sub-3 is the name-entry hand-off wherever it executes: retail's
        // handler table maps it to `FUN_801F03F0` unconditionally, with no
        // test of how the record was reached. The opening install raises
        // the pending flag up front; a record reached any other way (a card
        // load replaying `town01` P2[3], the comparison corpus's resume)
        // raises it here, so `op49_invoke_setup` opens the screen on the
        // same Idle->arm edge.
        if sub_op == 3 {
            self.world.cutscene.prologue_naming_pending = true;
            self.world.cutscene.prologue_naming_armed = false;
            self.world.cutscene.naming_owner = Some(self.world.op49_park_owner());
            self.world.cutscene.naming_slot = instr.get(2).map_or(0, |&b| usize::from(b));
        }
        // Sub-7 is the casino prize-exchange counter (menu-overlay
        // sub-screen 0x20); the byte after the sub-op selects the prize
        // block (koin1 = 0, balden = 1).
        if sub_op == 7 {
            self.world.try_arm_prize_exchange(instr);
        }
        // Anything else is a submode sub-screen: open one on the driver actor
        // so the dispatcher runs it and, when it hands back, unparks this op.
        if let Some(slot) = crate::field_submode_screen::slot_for_op49_sub_op(sub_op) {
            self.world.open_field_submode_screen(slot, None);
            // Retail parks the operand *pointer*, and the screens read past
            // the sub-op byte through it (the start menu counts `+1..=3`), so
            // the payload has to travel with the arm.
            self.world.set_submode_board_entries(instr);
        }
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

    // Op-0x43 sub-2 three-actor talk setup - the talk-controller spawn.
    // Spec: `overlay_cutscene_dialogue_801d2d38.txt` (the 0897 static copy
    // of this VA is a garbled mid-function fragment). Gate = system flag
    // `0xD`, the talk-active lock this same function (re)sets on exit:
    //
    // - First arm (flag clear): retail collapses the story party list to
    //   its leader (count `0x80084594` = 1, ids `0x80084598..` =
    //   `[leader, 0, 0, 0]` from the leader byte `0x80084597`), clears the
    //   per-character talk flags `0x10/0x11/0x12`, sets flag
    //   `0x10 + leader`, and zeroes the dialog-busy byte `DAT_8007B648`
    //   (the engine's busy signal is `current_dialog`, left as-is). The
    //   fresh controller's SM state 0 (`FUN_801D27E0`) then captures the
    //   three participants' positions/headings into `0x800845E4` -
    //   mirrored here at arm time into the session record.
    // - Re-arm (flag set): restore the three participants' saved positions
    //   + headings from the table - retail's else-branch loop pairs saved
    //   record `i` with the new controller's actor `i`.
    //
    // Retail's controller countdown `+0x72` subtracts the scene-MAN header
    // pair (`FUN_8003D064(_DAT_8007B898 + 0x22)`) from `arg_byte`; the
    // session keeps the raw operand (no MAN header staged on this path).
    // PORT: FUN_801D2D38
    // REF: FUN_801D27E0 (controller SM; state 0 = the position capture)
    // REF: FUN_8003C83C (id resolve), FUN_8003D064 (MAN-header pair read)
    fn op43_three_actor_talk(&mut self, actor_ids: [u8; 3], arg_word: u16, arg_byte: u8) {
        // Instruction ids resolve through the actor-list walk (retail
        // `FUN_8003C83C`) to the placement slots the engine's field-NPC
        // state is keyed by; an unmatched id passes through raw (tests /
        // channel-less scenes).
        fn resolve(world: &World, id: u8) -> u8 {
            crate::field_channels::resolve_target(&world.field_vm.channels, id)
                .map(|ci| world.field_vm.channels[ci].placement_index as u8)
                .unwrap_or(id)
        }
        let w = &mut *self.world;
        let (saved, saved_party, saved_party_len, saved_leader) = if !w.system_flag_test(0xD) {
            // First arm: snapshot the pre-collapse story party (the
            // engine-side record `World::end_three_actor_talk` restores when
            // the talk lock drops - retail's post-talk membership comes from
            // the script's own party ops), then collapse it to its leader.
            let mut saved_party = [None; 4];
            let saved_party_len = w.party.party_actor_slots.len().min(4) as u8;
            for (dst, src) in saved_party
                .iter_mut()
                .zip(w.party.party_actor_slots.iter().copied())
            {
                *dst = src;
            }
            let saved_leader = w.party.party_leader_slot;
            let leader = w
                .party
                .party_leader_slot
                .or_else(|| w.party.party_actor_slots.first().copied().flatten())
                .unwrap_or(0);
            w.party.party_actor_slots = vec![Some(leader)];
            w.party.party_leader_slot = Some(leader);
            w.system_flag_clear(0x10);
            w.system_flag_clear(0x11);
            w.system_flag_clear(0x12);
            w.system_flag_set(0x10 + u16::from(leader));
            // Capture the participants' live positions for the paired
            // restore (retail: controller SM state 0).
            let saved = actor_ids.map(|id| {
                let slot = resolve(w, id);
                w.npcs
                    .positions
                    .get(&slot)
                    .map(|&pos| (pos, w.npcs.heading(slot)))
            });
            (saved, saved_party, saved_party_len, saved_leader)
        } else {
            // Re-arm during an active talk: restore saved positions onto
            // this instruction's participants, positionally. The party
            // snapshot carries over unchanged - retail's else branch never
            // re-collapses (`FUN_801D2D38` `801d2dd4` skips the count/ids
            // stores when the flag is up).
            let prior = w
                .dialog
                .three_actor_talk
                .as_ref()
                .copied()
                .unwrap_or_default();
            for (i, &id) in actor_ids.iter().enumerate() {
                if let Some((pos, heading)) = prior.saved[i] {
                    let slot = resolve(w, id);
                    w.npcs.positions.insert(slot, pos);
                    w.npcs.headings.insert(slot, heading);
                }
            }
            (
                prior.saved,
                prior.saved_party,
                prior.saved_party_len,
                prior.saved_leader,
            )
        };
        w.system_flag_set(0xD);
        w.dialog.three_actor_talk = Some(ThreeActorTalk {
            actor_ids,
            script_id: arg_word,
            duration: arg_byte,
            saved,
            saved_party,
            saved_party_len,
            saved_leader,
            // Every (re)arm spawns a fresh controller in retail, so the SM
            // starts at state 0; its presence-flag base is the instruction's
            // u16 (controller `+0x50`).
            swap: crate::cutscene_script_elements::LeaderSwap {
                phase: 0,
                counter: 0,
                flag_base: arg_word,
            },
        });
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
        // Sub-ops 1 (start) and 9 (start behind a load barrier) are the only
        // writers of the track id `_DAT_8007BAC8` (`0x801E012C`,
        // `0x801E0254`). The other sub-ops are control words - pause (2, 3),
        // re-attach (4, `0x801E0180`), volume, commit - that leave the id
        // alone, so none of them clears `current_bgm` either.
        // Pause bit 1 of `_DAT_8007B750`: raised by 2 / 3, ended by every
        // start, the re-attach and the commit.
        match sub_op {
            2 | 3 => self.world.audio.bgm_script_paused = true,
            1 | 9 | 4 | 0xA => self.world.audio.bgm_script_paused = false,
            _ => {}
        }
        if sub_op == 1 || sub_op == 9 {
            self.world.audio.current_bgm = Some(text_id);
            // Sub-op 9 also raises the script-owned start bit 0 of
            // `_DAT_8007B750` (`0x801E0260`): the poller holds the outgoing
            // track in the slot until the sub-op 0xA commit.
            if sub_op == 9 {
                self.world.audio.start_pending_commit = true;
            }
        } else if sub_op == 0xA {
            // The commit (`0x801E0264`) sets the release-ack bit 4, on which
            // the poller installs the incoming track and clears bit 0
            // (`0x8002472C`).
            self.world.audio.start_pending_commit = false;
        } else if sub_op == 7 {
            // Sub-7 stores the next fight's battle sound set `_DAT_8007B880`
            // and makes no sequencer call: `lbu v1,0x1(s6)` tests the
            // operand's low byte against `0xFF` (store `-1`), else the u16
            // operand is stored (`0x801E01DC..0x801E0208`).
            self.world.audio.battle_sound_set = if text_id & 0xFF == 0xFF {
                -1
            } else {
                i32::from(text_id)
            };
        } else if sub_op == 5 {
            // Sub-5 is the timed release: retail's handler is
            // `FUN_800267A8(0, s16_operand)` at `0x801E01B4` (the operand is
            // the same `FUN_8003CE9C` read every other sub-op takes), which
            // arms the sound-source auto-release for `operand` vsyncs. The
            // frame-begin tick (`FUN_800267FC`) counts it down and raises
            // `pending_sound_release` on expiry.
            // PORT: FUN_800267A8 (live wiring; the arm record itself is
            // `crate::scus_leaf_kernels::TimedSoundArm`)
            self.world.arm_sound_release(i32::from(text_id));
        }
        // Free-roam picker staging: drop an ENTRY-WINDOW pause. A scene's
        // entry script may park the just-started BGM for a story moment the
        // choreography repairs (town01 `P1[0]` `+0x61`: silent dawn while
        // flag 0x225 is clear, restarted by the opening records' sub-9
        // starts). A picker visit runs no choreography, so the pause would
        // hold forever - the "music dies a second into the scene" report.
        // Pauses issued after the window (dialog / cutscene beats the
        // player triggers) route normally.
        if sub_op == 2
            && self.world.field_vm.free_roam_staging
            && self
                .world
                .clock
                .display_frames
                .saturating_sub(self.world.field_vm.free_roam_entry_frame)
                < crate::world::FREE_ROAM_ENTRY_PAUSE_WINDOW
        {
            return;
        }
        self.world
            .pending_field_events
            .push(FieldEvent::Bgm { text_id, sub_op });
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

    // PORT: FUN_800430AC
    // REF: FUN_80042310
    // REF: FUN_80042EE0
    fn op4c_n5_sub2_take_item(&mut self, item_id: u8) {
        // Op 0x4C n5 sub-2 TAKE_ITEM `[4C, 52, item_id]`, the give-side mirror
        // of `give_item` above. Retail's arm at `0x801E1ABC` consumes one of
        // the id from the bag (`FUN_80042310(item_id, 1)`) and, **only when
        // the bag does not hold it**, falls back to taking it off whoever is
        // wearing it (`FUN_800430AC`). The fallback is why a script can
        // confiscate an equipped accessory at all.
        //
        // The ordering is the half that is easy to invert: `0x100` is the
        // consume primitive's *not-found* sentinel, so the `== 0x100` branch
        // is the fallback and not a success path. A bag hit must therefore
        // leave equipment alone - `take_item_prefers_the_bag_over_the_worn_copy`
        // pins that direction, because an implementation that unequips first
        // (or always) passes every other assertion here.
        //
        // `item_id == 0` is a bag miss by construction:
        // `RetailInventory::find_slot` (retail `FUN_80042EE0`) treats 0 as the
        // empty-slot sentinel and never matches it, so a zero operand goes
        // straight to the fallback, which then clears the first *empty*
        // accessory slot - a no-op the retail kernel reports as success.
        //
        // No `FieldEvent` is emitted. The give side has one because a host
        // renders an acquisition banner off it; nothing consumes a take yet,
        // and the observable this wire exists for is the world state - the bag
        // count and the accessory slot - which is what the tests read.
        let held = if item_id == 0 {
            None
        } else {
            self.world.party.inventory.get(&item_id).copied()
        };
        match held {
            Some(count) if count > 0 => {
                let left = count - 1;
                if left == 0 {
                    self.world.party.inventory.remove(&item_id);
                } else {
                    self.world.party.inventory.insert(item_id, left);
                }
            }
            _ => {
                crate::equipment::party_unequip_accessory_by_id(
                    &mut self.world.party.roster,
                    item_id,
                );
            }
        }
    }

    fn open_dialog(
        &mut self,
        text_id: u16,
        inline: &[u8],
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    ) {
        let inline_vec = inline.to_vec();
        self.world.dialog.current = Some(DialogRequest {
            text_id,
            inline: inline_vec.clone(),
            world_x,
            world_z,
            depth_id,
        });
        self.world
            .pending_field_events
            .push(FieldEvent::OpenDialog {
                text_id,
                inline: inline_vec,
                world_x,
                world_z,
                depth_id,
            });
    }

    /// Field-VM op 0x4C n5 sub-4 - dialog-advance poll.
    ///
    /// The retail dispatcher calls `FUN_801D65D8(0)` (dialog "advance one
    /// frame" query); a non-zero return halts the VM at `pc`, a zero
    /// return advances `pc += 2`. Our world tracks dialog activity via
    /// `current_dialog` (cleared by the engine after the user dismisses
    /// the box). When a dismiss button (Cross / Circle) was just-pressed
    /// this frame, drop the dialog request inline so the VM transitions
    /// without the host having to round-trip another event.
    ///
    /// Returns `true` while a dialog is showing and the user has *not*
    /// dismissed it this frame. Returns `false` when there's no active
    /// dialog or when the dismiss button just fired (clears the request
    /// and unblocks the VM in one step).
    fn op4c_n_5_sub_4_dialog_advance(&mut self, _ctx: &mut FieldCtx) -> bool {
        // When the inline-script field-VM runner owns the box, it advances /
        // dismisses it (see `World::drive_inline_dialogue`); the simplified
        // dialog-advance must not clear the box out from under it.
        if self.world.dialog.current.is_none() || self.world.dialog.inline.is_some() {
            return false;
        }
        // A carrier's spar menu owns the dialog input while it is up: navigate +
        // confirm the fight option (engages only then), vs the any-accept path.
        if self.world.carriers.menu.is_some() {
            self.world.handle_carrier_menu();
            return self.world.dialog.current.is_some();
        }
        let dismissed = (self.world.input.just_pressed(input::PadButton::Cross)
            || self.world.input.just_pressed(input::PadButton::Circle))
            && !self.world.dialog.input_consumed;
        if dismissed {
            self.world.dialog.input_consumed = true;
            self.world.dialog.current = None;
            self.world
                .pending_field_events
                .push(FieldEvent::DialogDismissed);
            // Accepting a scripted-encounter carrier's prompt engages it: this is
            // the dialogue-accept that advances the carrier SM (`FUN_801DA51C`)
            // to its scene-transition. The battle launches on the next
            // `tick_field_carriers`. (The tutorial fight is forced, so any
            // dismiss is the accept; the undecoded Yes/No box-selection logic
            // would gate this once pinned.)
            if let Some(idx) = self.world.carriers.pending_engage.take() {
                self.world.engage_field_carrier(idx);
            }
            return false;
        }
        true
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
        // Retail (`FUN_801DE840` op `0x3C`): a linear search of the
        // present-party list `0x80084598` for `count = DAT_80084594`
        // entries; when absent and `count < 4`, append, `count += 1`, and
        // bubble the new id down while its left neighbour is larger (the
        // list stays sorted by character id). A count of 1 afterwards makes
        // the new member the leader (`0x80084597` / `_DAT_8007B8F8`).
        //
        // That list is the one battle setup reads, so the engine's battle
        // composition (`party_count` / `active_party`) follows it here.
        // Without that, a mid-scene join - Noa's "Let me help you!" beat on
        // Mt. Rikuroa, one op before the Caruban fight - reached the battle
        // with the pre-join count and Vahn fought alone.
        let mut list = self.world.present_party_list();
        let accepted = if list.contains(&char_id) || list.len() >= 4 {
            false
        } else {
            list.push(char_id);
            let mut i = list.len() - 1;
            while i > 0 && list[i - 1] > list[i] {
                list.swap(i - 1, i);
                i -= 1;
            }
            true
        };
        if accepted {
            if list.len() == 1 {
                self.world.party.party_leader_slot = list.first().copied();
            }
            self.world.install_present_party_list(list);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::PartyAdd { char_id, accepted });
        accepted
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

    /// Op `0x43` sub-9 with a non-zero tick count - the three-axis eased
    /// move (`FUN_801DE698` allocating `0x801F2840`, tick `FUN_801DD4C4`).
    ///
    /// Retail passes `&target[+0x14]` as the start block, so the ease begins
    /// at the target's **live** position; the engine reads the same triple
    /// off whichever seat it resolves the ctx to.
    fn op43_sub9_tween(&mut self, ctx: &mut FieldCtx, x: u16, y: u16, z: u16, ticks: u16) {
        use crate::world::{EasedMoveTarget, FieldEasedMove};
        let is_player = self.ctx_is_player(ctx);
        let (target, start) = if is_player {
            let slot = self.world.player_actor_slot;
            let seat = slot
                .and_then(|s| self.world.actors.get(s as usize))
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.physics.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([ctx.world_x as i16, ctx.world_y as i16, ctx.world_z as i16]);
            (EasedMoveTarget::Player, seat)
        } else if let Some(placement) = self.world.field_vm.executing_channel {
            let seat = self
                .world
                .npcs
                .positions
                .get(&placement)
                .copied()
                .unwrap_or((ctx.world_x as i16, ctx.world_z as i16));
            (
                EasedMoveTarget::Placement(placement),
                [seat.0, ctx.world_y as i16, seat.1],
            )
        } else {
            // No addressable seat: retail would still allocate the actor and
            // write through a back-link the engine does not have. Dropping
            // the record is the honest outcome - a silent write to the wrong
            // actor would be worse than none.
            return;
        };
        self.world.field_vm.eased_moves.push(FieldEasedMove {
            target,
            target_flags: ctx.flags,
            ease: legaia_engine_vm::field_actor_timers::EasedMove::spawn(
                start,
                [x as i16, y as i16, z as i16],
                ticks as i16,
            ),
        });
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
        // Retail op-0x45 (`FUN_801DE084`) writes each masked param into a
        // PERSISTENT camera struct slot (`0x801C6EA8 + 0x02 + i*4`); a beat that
        // omits a slot keeps its prior value. So MERGE per-slot rather than
        // replacing the set: an opdeene beat that sets only slot 9 (H) - one of
        // its nine beats is exactly `[(9, 792)]` - must keep the focus / pitch /
        // eye-depth staged by the previous beat, not reset the shot to the
        // `cutscene_view` fall-back framing (lead-actor focus + default depth).
        // The persistent set is cleared on scene entry so cutscene shots don't
        // leak across scenes (see `enter_field_scene`).
        for p in params {
            if let Some(existing) = self
                .world
                .camera
                .state
                .params
                .iter_mut()
                .find(|e| e.slot == p.slot)
            {
                existing.value = p.value;
            } else {
                self.world.camera.state.params.push(*p);
            }
        }
        self.world.camera.state.apply_trigger = apply_trigger;
        self.world.camera.state.mode = mode;
        // A glide arms the mover for `apply_trigger` display frames; a snap
        // marks every live mover dead.
        // REF: FUN_801DE084
        self.world.camera.state.glide_frames = i32::from(apply_trigger);
        // The event still carries only THIS beat's params (the per-beat delta):
        // the `Camera` controller's `route_camera_events` applies them per-axis
        // onto its own persistent eye/look-at, matching the same retail model.
        self.world
            .pending_field_events
            .push(FieldEvent::CameraConfigure {
                params: params.to_vec(),
                apply_trigger,
                mode,
            });
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
        // Op `0x36` is the sound-cue op, not a fade (the `SceneFade` event
        // name predates the handler being read): retail decodes two s16
        // operands and, when bit 15 of the first is set, runs a five-way
        // sub-switch on its low bits. Two of those sub-ops are the pending-cue
        // ring's producer pair, and the engine models both.
        //
        // * sub-`0` - `FUN_80035B50(arg)` enqueues the cue, parks its slot at
        //   `gp+0x15A` and zeroes that slot's delay word.
        // * sub-`4` - `FUN_80035BAC(arg)` stores `arg` as the parked slot's
        //   delay, i.e. schedules the cue instead of firing it.
        //
        // Two gates ride on top of the sub-switch, both read off
        // `0x801E030C..0x801E0444` and both keyed on the side-band
        // request/acknowledge pair `_DAT_8007BABC` / `_DAT_8007BAA0`
        // (`World::audio.sound_stream`) plus the dev/dual-mode word
        // `_DAT_8007B868` (`World::audio.dual_mode_gate`, `0` in retail):
        //
        // * bit-15 **set**: `_DAT_8007B868 != 0` skips the whole sub-switch
        //   (`bnez v0,0x801DF898` at `0x801E031C`). Subs `0`, `1` and `2`
        //   then consult the pair; subs `3` and `4` do not.
        // * bit-15 **clear**: the pair is a *precondition* on the XA arm
        //   (`bne a0,v0,0x801DEE4C` at `0x801E040C`) which
        //   `_DAT_8007B868 != 0` bypasses (`bnez v0,0x801E0414`).
        //
        // The doc this port was written from said the gate covered subs
        // `0`/`2`/`3` and stopped at the bit-15-set arm; the bytes say subs
        // `0`/`1`/`2` and the bit-15-clear arm, with sub `3` ungated.
        //
        // Both producer calls also cross to the host's audio ring as
        // `SfxRingOp`s (`World::take_sfx_ring_ops`), which is what sounds the
        // cue; the pair above is the engine-core mirror the gates read.
        //
        // PORT: FUN_80035BAC (live wiring; the table itself is
        // `crate::scus_leaf_kernels::SfxCueDelays`)
        // REF: FUN_80035B50
        // REF: FUN_800243F0 (the driver that settles the pair)
        let dev_gate = self.world.audio.dual_mode_gate != 0;
        if op0_word & 0x8000 != 0 {
            if dev_gate {
                // Retail never reaches this: the arm is skipped whole and
                // the op advances.
                self.world
                    .pending_field_events
                    .push(FieldEvent::SceneFade { op0_word, op1_word });
                return SceneFadeResult::Done;
            }
            match op0_word & 0x7FFF {
                0 => {
                    if !self.world.audio.sound_stream.is_settled() {
                        return SceneFadeResult::Busy;
                    }
                    // The enqueue writes the slot the cursor names, parks it,
                    // then advances the cursor - so the parked slot stays the
                    // written one until the next enqueue.
                    let slot = self.world.audio.sfx_cue_cursor;
                    self.world.audio.sfx_cue_cursor = self.world.audio.sfx_cue_delays.park(slot);
                    self.world.audio.sfx_parked_slot = slot;
                    // `jal 0x80035B50` with `a0 = (s16)op1_word`
                    // (`0x801E0344..0x801E034C`): the cue id goes to the
                    // host ring.
                    self.world
                        .audio
                        .sfx_ring_ops
                        .push(crate::world::SfxRingOp::Push(op1_word as i16));
                }
                1 => {
                    if !self
                        .world
                        .audio
                        .sound_stream
                        .request(i32::from(op1_word as i16))
                    {
                        return SceneFadeResult::Busy;
                    }
                    // Synchronous host: the driver's latch lands in the same
                    // call, so a following sub-`2` barrier is satisfied on
                    // arrival.
                    self.world.audio.sound_stream.settle();
                }
                2 => {
                    if !self.world.audio.sound_stream.is_settled() {
                        return SceneFadeResult::Busy;
                    }
                }
                4 => {
                    let parked = self.world.audio.sfx_parked_slot;
                    self.world
                        .audio
                        .sfx_cue_delays
                        .set_delay(parked, op1_word as i16);
                    self.world
                        .audio
                        .sfx_ring_ops
                        .push(crate::world::SfxRingOp::SetLastDelay(op1_word as i16));
                }
                // Sub `3` (`FUN_801D8450`) is ungated: it stops the top two
                // voices, closes VAB slot 6 and clears the field-bank latch,
                // so the next field init reloads PROT 0876.
                3 => self.world.release_field_audio(),
                // Every sub `>= 5` advances unconditionally.
                _ => {}
            }
        } else if !dev_gate && !self.world.audio.sound_stream.is_settled() {
            return SceneFadeResult::Busy;
        } else if op0_word & 0x7FFF != 0 {
            // The CD-XA arm (`0x801E0420`): `FUN_8003D53C(arg >> 3, arg & 7,
            // sel)`, `arg` the second s16 operand. It lands on the field XA
            // queue both hosts drain (`World::push_field_xa_cue`).
            //
            // `sel & 0x7FFF == 0` is `FUN_80019794(arg >> 3)` instead - a
            // seek-ahead that issues no read (`crate::world::field_xa`), which
            // an engine with no drive completes on arrival.
            // REF: FUN_8003D53C, FUN_80019794
            let arg = op1_word as i16;
            self.world
                .push_field_xa_cue((arg >> 3) as u8, (arg & 7) as u8, op0_word);
        }
        self.world
            .pending_field_events
            .push(FieldEvent::SceneFade { op0_word, op1_word });
        SceneFadeResult::Done
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
        // Op `0x34` sub-0 is the **screen-effect colour tween**, and it is a
        // walk-out / walk-in pair rather than a value ramp. Reading the arm
        // at `0x801DFCD4..0x801DFEF8` off the field overlay:
        //
        // 1. If `_DAT_8007B62C` names a live effect actor, retire it
        //    (`+0x10 |= 8`) and spawn a tween that runs from the *previous*
        //    target colour down to black with a **one**-frame hold, using
        //    the blend and kind selectors as they stood.
        // 2. Recompute both selectors from the sub-op byte and latch the new
        //    target RGB into `_DAT_8007BCCD/CE/CF`.
        // 3. An all-zero target **clears** the effect: retail stores zero
        //    into `_DAT_8007B62C` and leaves without spawning anything.
        // 4. Otherwise spawn the walk-in tween, black -> target, hold `-1`.
        //
        // REF: FUN_801DE2B0 (the spawner, ported at
        // `crate::field_actor_kernels::tween_from_fade_template`)
        //
        // Both spawns fork on scratchpad global `_DAT_1F800394` bit 23
        // (`lui v1,0x80; and` at `0x801DFD1C` / `0x801DFEB0`): set, retail
        // spawns through `FUN_80024E80` instead of `FUN_801DE2B0`. Only the
        // clear arm is modelled here because no script the disc carries
        // raises that bit - neither a field-VM `0x2E` (`asset
        // field-op-census --only 2E`) nor a motion-VM `0x10`/`0x11` (none is
        // authored in any bank), pinned by
        // `crates/asset/tests/scratch_global_bit_writers_real.rs`.
        use crate::fade::FadeTemplate;
        use crate::field_actor_kernels::{ACTOR_FLAG_YIELD, tween_from_fade_template};

        if let Some(slot) = self.world.presentation.effect_tween_slot.take() {
            if let Some(a) = self.world.actors.get_mut(slot) {
                a.physics.status_flags |= ACTOR_FLAG_YIELD;
            }
            let walk_out = FadeTemplate {
                kind: self.world.presentation.effect_blend,
                duration: intensity,
                start_rgb: self.world.presentation.effect_target_rgb,
                end_rgb: [0; 3],
                mode: [0, 1, 0],
            };
            let kind = self.world.presentation.effect_kind;
            self.world
                .spawn_colour_tween(tween_from_fade_template(&walk_out, kind));
        }

        let blend: i16 = if op0 & 1 != 0 { 2 } else { 1 };
        let kind: i16 = if op0 & 2 != 0 {
            8
        } else if op0 & 4 != 0 {
            0
        } else {
            2
        };
        let target = [i16::from(rgb[0]), i16::from(rgb[1]), i16::from(rgb[2])];
        self.world.presentation.effect_blend = blend;
        self.world.presentation.effect_kind = kind;
        self.world.presentation.effect_target_rgb = target;

        self.world
            .pending_field_events
            .push(FieldEvent::ColorFade { op0, rgb });

        if target == [0; 3] {
            return;
        }

        // The one conditional on the operand: a pure-white target under
        // blend `2` shortens the ramp by an eighth (`sra v0,s1,3` /
        // `subu s1,s1,v0` at `0x801DFE60`). The shipped `0x41`-frame
        // instruction is what a capture sees as a 57-frame template.
        let duration = if blend == 2 && target == [0xFF; 3] {
            intensity - (intensity >> 3)
        } else {
            intensity
        };
        let walk_in = FadeTemplate {
            kind: blend,
            duration,
            start_rgb: [0; 3],
            end_rgb: target,
            mode: [0, crate::field_actor_kernels::TWEEN_HOLD_FOREVER, 0],
        };
        self.world.presentation.effect_tween_slot = self
            .world
            .spawn_colour_tween(tween_from_fade_template(&walk_in, kind));
    }

    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        // Op 0x34 sub-3 is the ambient-tree install, and retail has ONE of
        // them. The dispatcher arm at `0x801E00B0` is
        // `FUN_800252EC(bytecode[1] + 1, s5 + 0x14, s5 + 0x24)` where `s5` is
        // the executing script's context (`FUN_801DE840`'s third argument,
        // retargeted by the `0x80` cross-context prefix), and that call stages
        // the record through `FUN_80021B04` -> the move VM. The scene-entry
        // installer is not a second mechanism: `FUN_8003A1E4` runs this same
        // dispatcher for one frame slice per just-spawned placement, so a
        // load-slice install and a runtime install differ only in when the
        // arm is reached.
        //
        // The port therefore routes both to `spawn_ambient_record_at` - the
        // full `FUN_80021B04` port, with the op-`0x25` fan-out, the
        // self-modifying bytecode writes, the mode-3 CLUT-cell integrator,
        // the mode-4 VRAM-rect scroller and the VDF morph envelope. Routing
        // this arm at the older `SummonScene` pool instead left a runtime
        // install running a stripped copy of its own tree.
        //
        // Seat = the executing context, not the player: `s5 + 0x14` is the
        // ctx position and `s5 + 0x24` its render banks. A placement channel
        // seeds both from its MAN record, so a scripted install stages where
        // its actor stands.
        //
        // PORT: FUN_800252EC (op 0x34 sub-3 -> stager record `arg + 1`)
        let origin = [ctx.world_x as i16, ctx.world_y as i16, ctx.world_z as i16];
        let rot = [ctx.field_24, ctx.field_26 as i16, ctx.field_28];
        self.world
            .spawn_ambient_record_at(arg as usize + 1, origin, rot);
        self.world
            .pending_field_events
            .push(FieldEvent::EffectAnimTrigger { arg });
    }

    fn menu_ctrl_sub1(&mut self, op0: u8, payload: &[u8; 5]) {
        // Sub-op 0x12: the global multiply screen tint `DAT_8007BCB8/B9/BA =
        // payload[0..3]` (neutral 0x80), optionally ramped there over
        // `LE_u16(payload[3..5])` frames by the slot-job spawner
        // `FUN_8003C5F0`. This is the retail scene-fade primitive - every
        // field scene `P1[0]`'s 0x52F arrival arm fades in from black with
        // `4C 12 00 00 00 00 00` (instant black) + `4C 12 80 80 80 44 00`
        // (ramp to neutral over 68 frames).
        // REF: FUN_8003C5F0
        if op0 == 0x12 {
            let target = [
                (payload[0] as f32 / 128.0).min(2.0),
                (payload[1] as f32 / 128.0).min(2.0),
                (payload[2] as f32 / 128.0).min(2.0),
            ];
            let frames = u16::from_le_bytes([payload[3], payload[4]]);
            let current = self.world.presentation.tint.as_ref().map(|t| t.factor());
            self.world.presentation.tint = Some(crate::fade::SceneTintRamp::to_target(
                current, target, frames,
            ));
        }
        // Sub-op 0x13: the field clear colour (both draw environments'
        // `r0 / g0 / b0`), instant or ramped by `FUN_8003C5F0` over
        // `LE_u16(payload[3..5])` frames - see `ClearColourRamp`.
        // REF: FUN_8003C5F0
        if op0 == 0x13 {
            let end = [payload[0], payload[1], payload[2]];
            let frames = u16::from_le_bytes([payload[3], payload[4]]);
            let p = &mut self.world.presentation;
            if frames == 0 {
                p.clear_rgb = end;
                p.clear_ramp = None;
            } else {
                p.clear_ramp = Some(crate::world::ClearColourRamp {
                    start: p.clear_rgb,
                    end,
                    total: frames,
                    elapsed: 0,
                });
            }
        }
        self.world.pending_field_events.push(FieldEvent::MenuCtrl {
            op0,
            payload: *payload,
        });
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
        // The VM's `is_player` is the bit; retail's is an identity compare.
        let is_player = is_player && self.ctx_is_player(ctx);
        // Scene-entry spawn-prologue pre-run: the record seats ITS OWN actor
        // (the VM already wrote the channel ctx position; the pre-run's
        // write-through surfaces it). Never yank the player from here - a
        // prologue that flips its own ctx class bit would otherwise
        // teleport the player to the record's seat at scene load (seen as
        // `suimon`'s cold spawn landing in the off-map hide box).
        if self.world.field_vm.entry_prerun {
            let _ = ctx;
            return;
        }
        // **A spawned record's `0x23` belongs to its own channel, never to
        // the player.** Retail does not pick the player arm off a ctx class
        // bit: `0x801DEC7C bne s5,v0` compares the executing ctx **pointer**
        // against the player context `_DAT_8007C364` (`0x8007C348 + 0x1C`),
        // and only that identity reaches the camera re-centre `func_0x80017EC8`
        // (`0x801DEC84..0x801DECA8`); every other ctx falls to `0x801DECAC`,
        // the `+0x8C`/`+0x8D` facing + movement-init arm on **that** actor.
        // The port derives `is_player` from `ctx.flags & 0x1000000`, which a
        // spawned partition-2 record's context inherits - so without this arm
        // a scene-arrival record's `0x23` yanked the player wherever the
        // record seated its own actor, the hide box `(127,127)` included.
        // This is the same law [`Self::op4c_n5_sub1_npc_run`] already carries
        // for the `4C 51` form; the slice's write-through surfaces the move
        // into the placement-keyed NPC state.
        // REF: FUN_8003C83C (cross-context target resolve)
        if self.world.field_vm.in_spawned_record_slice
            && let Some(_slot) = self.world.field_vm.executing_channel
        {
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            return;
        }
        // Player path: also propagate to the active actor slot's
        // move_state so the renderer / collision layer sees the teleport.
        if is_player
            && let Some(slot) = self.world.player_actor_slot
            && let Some(actor) = self.world.actors.get_mut(slot as usize)
        {
            actor.move_state.world_x = world_x as i16;
            actor.move_state.world_z = world_z as i16;
        }
        // The player arm's camera re-centre, `FUN_80017EC8(op[1] & 0x7F,
        // op[2] & 0x7F, ..)` at `0x801DEC9C`: re-latch the region box on the
        // operand tile and re-plan the windowed static-object list. The
        // operand tile is `(world - 0x40) >> 7` of the seat (the half-tile bit
        // only adds `+0x40`).
        // REF: FUN_80017EC8
        if is_player {
            self.world.recentre_field_window(
                (i32::from(world_x) - 0x40) >> 7,
                (i32::from(world_z) - 0x40) >> 7,
            );
        }
        let _ = ctx;
        self.world.pending_field_events.push(FieldEvent::MoveTo {
            world_x,
            world_z,
            is_player,
        });
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
        // A cross-context ExecMove against an NPC channel (`A2 <id>
        // <move_id>`): retail is `FUN_80024E08(actor, id)` - the id lands in
        // the actor's anim slot and the anim-clock (`FUN_800204F8`) plays the
        // named clip (scene-bundle record `id - 1`, the same `+0x5C` id space
        // the placement anim byte seeds). Surface it as an anim cue so the
        // windowed host re-targets the NPC's clip player - this is what makes
        // Mei visibly WALK (clip 61) then idle (clip 60) through her town01
        // walk-on beat instead of sliding in a frozen pose.
        // REF: FUN_80024E08, FUN_800204F8
        //
        // Aimed at a `.MAP` placed object's actor (`A2 <record> <clip>`), the
        // same op re-points that prop's clip: `+0x5E = 0xFFFE` forces the anim
        // tick's re-point (`0x8002057C`), which zeroes the cursor, and the
        // tick then plays under the actor's own `+0x62` / `+0x6A` - the words
        // the record's preceding `AC`/`AB` and `4C 41` pokes just set.
        // `chitei2`'s rescue beat opens the drain pipe this way (P2[16]:
        // `CC 01 41 20`, `AC 01 07`, `AB 01 03`, `AC 01 01`, `A2 01 02` - clear
        // reverse, clamp, release the spawn hold, play clip 2 once).
        if let Some(record) = self.world.field_vm.executing_object {
            self.world
                .play_object_prop_clip(record, move_id, ctx.local_flags, ctx.field_6a);
        }
        if let Some(slot) = self.world.field_vm.executing_channel {
            self.world
                .npcs
                .anim_cues
                .insert(slot, (1, move_id, Vec::new()));
            let party_bank = ctx.flags & vm::field_player_clip::PARTY_BANK_FLAG != 0;
            self.world.bind_npc_scene_clip(
                slot,
                move_id,
                party_bank,
                ctx.local_flags,
                ctx.field_6a,
            );
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ExecMove { move_id });
    }

    /// Op `4C 50` - the actor model set. The state writes are the VM
    /// default's (`+0x64`, `+0x5C = 0`, the `0x1000` clear, the world-map
    /// `+0x60` mirror); the re-stage `FUN_80024E08` runs through
    /// `FUN_80020F88` - the actor starts drawing the named TMD - is the live
    /// model id a placement carries on [`World::field_npc_live_model`],
    /// which both play hosts already re-bind mid-scene from (the same seat
    /// motion op `0x0E` writes). The id stays raw: the `>= 0xF0` player-bank
    /// select is [`crate::model_bank::resolve_model_id`]'s, the `4C 50`
    /// arm's own `0x801E17AC..0x801E1824` select instruction for instruction.
    ///
    /// A placed object's bind context (an object-bind channel, its own op
    /// or a `CC <record> 50 ..` poke) lands on the live model its bind
    /// record draws with, [`World::object_live_models`] - the table op
    /// `0x0E` also writes, which both hosts redraw the record's object from.
    /// The credits walk on `map01` (`P2[40]`, `CC 08 50 20 00`) swaps Rim
    /// Elm's landmark, record 8, from its `.MAP` mesh to model `32`
    /// (`ending_vignette_rimelm_walkaway` holds `+0x64 = 37` over a bank base
    /// of `5`); without the route the overworld kept drawing the old village.
    /// An op aimed at the player (`CC F8 50 ..`, e.g. `jagaroom`'s costume
    /// swap) goes to [`World::field_player_set_model`] instead, through
    /// `FieldHost::player_set_model`.
    ///
    /// PORT: FUN_80024E08 (the model re-stage, through the live-model seat)
    fn op4c_n5_sub0_set_actor_model(&mut self, ctx: &mut FieldCtx, value: i16, _high: bool) {
        ctx.model_id = value as u16;
        ctx.move_id = 0;
        ctx.flags &= 0xffff_efff;
        if self.model_pool_is_world_map() {
            ctx.model_id_high = value as u16;
        }
        if let Some(record) = self.world.field_vm.executing_object {
            self.world
                .npcs
                .object_models
                .insert(usize::from(record), value);
        } else if let Some(slot) = self.world.field_vm.executing_channel {
            self.world.set_field_npc_live_model(slot, value);
        }
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

    /// Op `0x4C 0x82 <slot>` - full HP/MP restore of one party slot.
    ///
    /// Retail's inn / rest heal. There is no inn opcode and no native inn
    /// routine: the scene script composes a stay out of dialogue, a
    /// 2-option picker, an op-0x4E gold gate, an op-0x3A `ADD_MONEY` with
    /// the negative charge and the fades, then emits one of these per
    /// party slot. The charge and the restore are decoupled, which is why
    /// a free rest (a bed, an infirmary) is the same tail with the gate
    /// and debit dropped - and why `legaia_asset::inn_costs` finds the
    /// prices scripted per scene rather than in a table.
    ///
    /// Retail addresses the slot by *literal operand*, so a script that
    /// heals slots 0/1/2 heals exactly those records - it does not walk
    /// "every active member". The port keeps that: an operand past the end
    /// of the roster is a no-op, matching a write into a record the game
    /// never populated.
    fn op4c_n8_sub2_restore_party_slot(&mut self, slot: u8) {
        let Some(member) = self.world.party.roster.members.get_mut(slot as usize) else {
            return;
        };
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = hms.hp_max;
        hms.mp_cur = hms.mp_max;
        member.set_hp_mp_sp(hms);
        // The record is retail's only copy; the port's party actor mirrors
        // it, and a battle seats (and `save_party` writes back) from the
        // mirror - so a rest that left it stale healed nobody.
        self.world.mirror_roster_hp_mp(usize::from(slot));
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
    /// ([`legaia_engine_vm::field_light`]). `town01`'s `P1[0]` sets a white
    /// back colour; `koin3`'s cutscene records drop it to black and back.
    fn op4c_n8_sub_a_write_quad(&mut self, slots: [i16; 3], packed: u32) {
        self.world.presentation.field_light =
            legaia_engine_vm::field_light::FieldLight::from_op_4c_8a(slots, packed);
    }

    fn op4c_n8_sub_0_actor_allocator(&mut self, _ctx: &mut FieldCtx, count: u8, tail: &[u8]) {
        // In the spawned opening-cutscene context (target 0xF8) this op is the
        // inline-narration text-draw, not an actor spawn - the separate
        // `CutsceneNarration` presenter owns those pages. Suppress the spawn
        // side-effect while the cutscene timeline steps; the VM still advances
        // the PC past the page bytes on its own.
        if self.world.cutscene.in_timeline {
            return;
        }
        // Walk `count` variable-length records out of `tail` using the
        // retail packet-length rule (FUN_8003CA38, mirrored in
        // `legaia_engine_vm::field_helpers::packet_length`): bytes <= 0x1E
        // terminate a record; bytes whose top nibble is 0xC consume one
        // extra byte. The walker stops when the tail is exhausted - the
        // retail original would over-read into adjacent memory, which the
        // port refuses by construction.
        let mut records: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
        let mut cursor = 0usize;
        for _ in 0..count {
            if cursor >= tail.len() {
                break;
            }
            let len = vm::field_helpers::packet_length(&tail[cursor..]);
            records.push(tail[cursor..cursor + len].to_vec());
            // Skip the terminator byte itself (the byte <= 0x1E that
            // closed the record); if the walker ran off the end without
            // seeing one, `cursor + len == tail.len()` and the next
            // iteration's bounds check exits the loop.
            cursor += len + 1;
        }
        for record in &records {
            self.world.pending_actor_spawns.push(record.clone());
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ActorAllocate { records });
    }

    /// Op `0x4C` nibble-5 sub-1 - the retail "NPC run" primitive, which is a
    /// **teleport plus a move-anim start** (`FUN_80024E08` writes the target
    /// position outright and kicks the walk animation), not a glide.
    ///
    /// Dispatched **cross-context into the player channel** (`CC F8 51 …`) it
    /// is one of the two op forms retail uses to reposition the *player* - the
    /// other being the bare `0x23` MOVE_TO. Door records, scene-entry records
    /// and cutscene beats all use it (e.g. Rim Elm's in-house beat seats the
    /// player at its interior tile with `CC F8 51 61 0A …`), so dropping the
    /// player arm silently voids a whole class of doors and arrivals.
    ///
    /// The operand's `depth` byte carries the arrival facing in its low nibble
    /// (the dispatcher indexes the SCUS compass LUT at `0x80073F04`), the same
    /// LUT the paired `0x38` CAM_CFG uses.
    // PORT: FUN_801DE840 (nibble-5 sub-1: position write + heading LUT + move-anim)
    fn op4c_n5_sub1_npc_run(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        depth_byte: u8,
        move_id: u8,
        is_player: bool,
    ) {
        // The VM's `is_player` is the bit; retail's is an identity compare
        // (`0x801E1954`).
        let is_player = is_player && self.ctx_is_player(ctx);
        // Scene-entry spawn-prologue pre-run
        // ([`World::pre_run_field_channel_prologues`]): the record's own
        // `4C 51` is the actor's initial SEAT - retail's install pre-run
        // walks the actor there before the scene fades in, and the settled
        // retail position equals the op target exactly (town01 actor-list
        // pin). Write it through the channel ctx (including the
        // parked-sentinel despawn, `(127,127)` -> the off-map hide box) so
        // the pre-run's write-through + route invalidation see it. Scoped to
        // the pre-run: live channel stepping keeps its previous semantics.
        // The player arm is suppressed too - at install the op belongs to
        // the spawned actor, and a record whose ctx carries the player-class
        // bit must not yank the player at scene load (see `move_to`).
        if self.world.field_vm.entry_prerun {
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            // The case-5 sub-1 body stores the LUT heading into `+0x26`
            // (`0x801E1900`) along with the tile, on whichever story arm
            // the prologue took. The load-time facing seed reads the first
            // nibble a linear walk meets, which is another arm's when the
            // arms seat the actor differently: `town01` `P1[34]` stands at
            // `(98, 15)` facing index 0 on its `0x226` arm, and the seed had
            // turned it to the `(96, 58)` arm's index 7.
            if let Some(slot) = self.world.field_vm.executing_channel
                && let Some(heading) =
                    crate::man_field_scripts::facing_index_to_engine_heading(depth_byte & 0xF)
            {
                self.world.npcs.headings.insert(slot, heading);
            }
            return;
        }
        // A spawned partition-2 record's channel poke (the modal cutscene
        // timeline / a concurrent helper context driving a resolved
        // channel): SEAT the target exactly. Retail dispatches the run via
        // the move table and the settled position equals the op target -
        // the same pin as the entry pre-run. This is the town01 Mei
        // walk-on beat's `CC 46 51 11 1D 00 3C` (seat placement 34 at the
        // Vahn's-house door tile (17,29)); dropping it left Mei standing in
        // her own house across town, invisible to the conversation. The
        // (127,127) hide-box park writes through too - a record hiding an
        // actor is a despawn the render must see. `ctx` is the TARGET
        // channel's context here; the slice's write-through surfaces the
        // move into the placement-keyed NPC state.
        // REF: FUN_8003C83C (cross-context target resolve)
        if self.world.field_vm.in_spawned_record_slice
            && let Some(slot) = self.world.field_vm.executing_channel
        {
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            // Publish the seat NOW, not at the slice's end-of-frame
            // write-through: retail writes the actor's `+0x14`/`+0x18`
            // directly, so a `C7 <id> ..` walk later in the SAME slice starts
            // from here. Surfacing it only at slice end left the walk to read
            // the previous position - the off-map hide box for an actor a
            // cutscene places and then walks (`rugi`, `bylon`, `conc3`, ...),
            // which then glided across the whole map.
            self.world
                .npcs
                .positions
                .insert(slot, (world_x as i16, world_z as i16));
            self.world.npcs.motions.remove(&slot);
            if let Some(heading) =
                crate::man_field_scripts::facing_index_to_engine_heading(depth_byte & 0xF)
            {
                self.world.npcs.headings.insert(slot, heading);
            }
            return;
        }
        // The parked-sentinel tile (127,127) decodes to (0x3FC0, 0x3FC0):
        // a despawn, not a walk.
        if world_x == 0x3FC0 && world_z == 0x3FC0 {
            return;
        }
        if is_player {
            // The player arm also aims its move id at the player's clip
            // (`0x801E1954..0x801E1A3C`: clip base + pick + bind).
            self.world.field_player_script_clip(move_id);
            let y = self
                .world
                .sample_field_floor_height(i32::from(world_x as i16), i32::from(world_z as i16))
                as i16;
            if let Some(slot) = self.world.player_actor_slot
                && let Some(actor) = self.world.actors.get_mut(slot as usize)
            {
                actor.move_state.world_x = world_x as i16;
                actor.move_state.world_z = world_z as i16;
                actor.move_state.world_y = y;
                if let Some(heading) =
                    crate::man_field_scripts::facing_index_to_engine_heading(depth_byte & 0xF)
                {
                    actor.move_state.render_26 = heading;
                }
            }
            // The same re-centre the `0x23` player arm makes, from the `4C 51`
            // player arm: `FUN_80017EC8(op[1] & 0x7F, op[2] & 0x7F, ..)` at
            // `0x801E1A58`.
            // REF: FUN_80017EC8
            self.world.recentre_field_window(
                (i32::from(world_x) - 0x40) >> 7,
                (i32::from(world_z) - 0x40) >> 7,
            );
            self.world.pending_field_events.push(FieldEvent::MoveTo {
                world_x,
                world_z,
                is_player: true,
            });
            return;
        }
        // The talker's interaction prologue (the inline dialogue runner) or a
        // live channel stepping its own script: the same SEAT as above. The
        // case-5 sub-1 body stores the tile straight into the actor
        // (`sh v0,0x14(s5)` at `0x801E1880`, `sh v0,0x18(s5)` at
        // `0x801E1898`) and its LUT heading into `+0x26` (`0x801E1900`); no
        // walk kernel is armed, so the NPC appears on the tile rather than
        // gliding to it. Byte `+4` still picks the move clip.
        if let Some(slot) = self
            .world
            .dialog
            .stepping_inline_npc
            .or(self.world.field_vm.executing_channel)
        {
            ctx.world_x = world_x;
            ctx.world_z = world_z;
            if let Some(pos) = self.world.npcs.positions.get_mut(&slot) {
                *pos = (world_x as i16, world_z as i16);
                self.world.npcs.motions.remove(&slot);
                if let Some(heading) =
                    crate::man_field_scripts::facing_index_to_engine_heading(depth_byte & 0xF)
                {
                    self.world.npcs.headings.insert(slot, heading);
                }
                self.world.carry_npc_run_anim(slot, move_id);
            }
        }
    }

    // PORT: FUN_8003C8F0 - the `_DAT_8007B898` partition-table record
    // resolver (prefix-summed s16 counts at `+0x22` + flattened u24
    // record-offset walk) feeding the op-`0x4C 0xC3` script-table teleport;
    // the record walk is shared with
    // [`crate::man_field_scripts::partition_record_offset`], the descriptor
    // decode + ctx write-set live in [`apply_script_table_teleport`].
    fn op4c_n_c_sub_3_script_teleport(&mut self, ctx: &mut FieldCtx) {
        // Resolve against the scene's resident MAN (kept on the world while
        // per-actor channels run). Without one the trait's default no-op
        // semantics stand - there is no partition table to resolve against.
        let Some(man) = self.world.field_vm.channels_man.clone() else {
            return;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            return;
        };
        if !apply_script_table_teleport(&man_file, &man, ctx) {
            return;
        }
        self.rerun_spawn_section(&man_file, &man, ctx);
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
        // Synchronous actor allocator (see retail `FUN_801D77F4` body
        // dumped at `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d77f4.txt`).
        // The dispatcher packs the four args
        //   `[vdf_idx: u8, tmd_idx: i16, kind: i16, variant: i16]`
        // into the 7 bytes after `[0x4C, 0xD8]`; FUN_801D77F4 then writes
        // `actor[+0x3C] = kind` and `actor[+0x3E] = variant` on the
        // allocated slot, plus `actor[+0x48] = DAT_8007C018[tmd_idx]`
        // (TMD pointer) and `actor[+0x4C] = VDF_body_ptr`. We mirror
        // all four writes here.
        let kind = words[1] as u16;
        let variant = words[2] as u16;
        // Mirror retail's `actor[+0x4C] = VDF_body_ptr`: look up the
        // VDF record body bytes for the emitted event payload. `None` when
        // no VDF buffer is installed or the index is OOR; the shared
        // allocator handles the empty-record case (synchronous spawn
        // semantics). `spawn_field_actor` performs the identical resolve
        // and stores the record on the actor.
        let record_bytes: Vec<u8> = self
            .world
            .vdf_record_bytes(b1)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        match self
            .world
            .spawn_morph_weight_actor(words[0], b1, kind, variant)
        {
            Some(slot_idx) => {
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawned {
                        slot: slot_idx as u8,
                        kind,
                        variant,
                        record: record_bytes,
                    });
            }
            None => {
                // Pool-exhausted: mirrors the retail bail-silently branch
                // where FUN_80020DE0 returns 0.
                self.world
                    .pending_field_events
                    .push(FieldEvent::ActorSpawnFailed {
                        record: record_bytes,
                    });
            }
        }
    }
}
