//! Field-host bodies that move and face actors: compass facing, context ramps,
//! the op-`0x43` tween, `move_to`, `exec_move`, the NPC run and the actor-to-
//! actor sync.

use super::*;

impl FieldHostImpl<'_> {
    // `4C E3 <src>`: the executing context takes the source actor's
    // position `+0x14/+0x16/+0x18` and heading `+0x26`
    // (`0x801E3108..0x801E314C`). In the cross-context form
    // `CC <dst> E3 <src>` the destination is `<dst>` - a party placement
    // stepping onto the player's spot facing his way (`CC 09 E3 F8` in
    // `stone`). Headings copy in the engine's own space on both sides.
    // REF: FUN_801DE840 (0x801E3108..0x801E31B0), FUN_8003C83C
    pub(super) fn teleport_to_actor(&mut self, ctx: &mut FieldCtx, actor_id: u8) {
        let w = &mut *self.world;
        let src = if actor_id == legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET {
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
            legaia_engine_field::field_channels::resolve_target(view, actor_id).map(|ci| {
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
        if ctx.script_id == u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
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

    // Op `0x38` simple path: the compass-LUT heading lands on the resolved
    // actor's `+0x26`. The extended `0xF8` (and a context that is the player)
    // turns the player; a placement channel stepping - its own script or a
    // poke on it - turns that placement; the inline talk runner turns the NPC
    // whose record it runs. Any other context turns itself: its own `+0x26`
    // takes the retail heading, which is what an object-bind context's draw
    // composes (`World::object_draw_turns`) - `town0c`'s exit rocks turn by
    // `38 83 00` in their bind prologue.
    // REF: FUN_801DE840 (case 0x38), FUN_8003C83C
    pub(super) fn face_compass(&mut self, ctx: &mut FieldCtx, index: u8, player: bool) {
        let Some(heading) =
            legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(index)
        else {
            return;
        };
        if !player {
            // Engine space is retail + 0x800.
            ctx.field_26 = (heading as u16).wrapping_sub(0x800) & 0x0FFF;
        }
        let channel = self.world.field_vm.executing_channel;
        if player
            || (channel.is_none()
                && ctx.script_id == u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET))
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

    pub(super) fn ramp_ctx_slot(&mut self, ctx: &mut FieldCtx, sub: u8, target: i16, ticks: u16) {
        // Sub-8: a heading tween on a placement, from its live heading
        // (retail space) - the `FUN_8003C5F0` ramp the scheduler lerps.
        if sub == 8 {
            if let Some(HeadingOwner::Placement(slot)) = self.world.heading_owner(ctx) {
                use legaia_engine_motion_vm::ambient_motion::{RAMP_DEST_HEADING, Ramp, RampKind};
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
        if sub != 0
            || ctx.script_id != u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        {
            return;
        }
        use legaia_engine_motion_vm::ambient_motion::{RAMP_DEST_SCALE, Ramp, RampKind};
        let total = i32::from(ticks);
        self.world.locomotion.player_scale_ramps.install(Ramp {
            dest: RAMP_DEST_SCALE,
            owner: u32::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
            start: i32::from(ctx.field_72),
            end: i32::from(target),
            total,
            remaining: total,
            kind: RampKind::U16,
        });
    }

    /// Op `0x43` sub-9 with a non-zero tick count - the three-axis eased
    /// move (`FUN_801DE698` allocating `0x801F2840`, tick `FUN_801DD4C4`).
    ///
    /// Retail passes `&target[+0x14]` as the start block, so the ease begins
    /// at the target's **live** position; the engine reads the same triple
    /// off whichever seat it resolves the ctx to.
    pub(super) fn tween_actor_position(
        &mut self,
        ctx: &mut FieldCtx,
        x: u16,
        y: u16,
        z: u16,
        ticks: u16,
    ) {
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
            ease: legaia_engine_field::field_actor_timers::EasedMove::spawn(
                start,
                [x as i16, y as i16, z as i16],
                ticks as i16,
            ),
        });
    }

    pub(super) fn move_to(
        &mut self,
        ctx: &mut FieldCtx,
        world_x: u16,
        world_z: u16,
        is_player: bool,
    ) {
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
        // This is the same law [`Self::run_to_tile`] already carries
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
        // The seat is re-floored as the system script's `A3 F8` is
        // (`World::step_field_player_move_to`): a door's landing is a sub-area
        // of the same grid at its own elevation (`tower`'s floors).
        if is_player && let Some(slot) = self.world.player_actor_slot {
            let y = self
                .world
                .sample_field_floor_height(i32::from(world_x as i16), i32::from(world_z as i16))
                as i16;
            if let Some(actor) = self.world.actors.get_mut(slot as usize) {
                actor.move_state.world_x = world_x as i16;
                actor.move_state.world_z = world_z as i16;
                actor.move_state.world_y = y;
            }
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

    pub(super) fn exec_move(&mut self, ctx: &mut FieldCtx, move_id: u8) {
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
            let party_bank =
                ctx.flags & legaia_engine_field::field_player_clip::PARTY_BANK_FLAG != 0;
            if let Some(flags) = self.world.bind_npc_scene_clip(
                slot,
                move_id,
                party_bank,
                ctx.local_flags,
                ctx.field_6a,
            ) {
                ctx.local_flags = flags;
            }
        }
        self.world
            .pending_field_events
            .push(FieldEvent::ExecMove { move_id });
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
    pub(super) fn run_to_tile(
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
        // The actor words every arm writes ahead of the seat, on an NPC:
        // the depth byte's bit 7 sets or clears the party-bank bit
        // `0x01000000` of `+0x10` (`0x801E1828..0x801E1858`), the anim
        // control word `+0x62` drops hold / clamp / reverse (`andi 0xFF75`
        // at `0x801E1864`), and the NPC arm stores byte `+4` into `+0x5C`
        // with `+0x5E = -2` and binds it through `FUN_800204F8`
        // (`0x801E1A8C..0x801E1A9C`). So the op always leaves a looping
        // clip of the named move playing, out of the bank the depth byte
        // names - on the install pre-run and a cutscene's cross-context
        // poke as much as on a live channel. The player arm picks its own
        // clip (`field_player_script_clip` below) and keeps its words.
        // REF: FUN_800204F8
        if !is_player {
            let party = legaia_engine_field::field_player_clip::PARTY_BANK_FLAG;
            if depth_byte & 0x80 != 0 {
                ctx.flags |= party;
            } else {
                ctx.flags &= !party;
            }
            ctx.local_flags &= !(legaia_engine_scene::field_env::ANIM_HOLD
                | legaia_engine_scene::field_env::ANIM_CLAMP
                | legaia_engine_scene::field_env::ANIM_REVERSE);
            ctx.move_id = u16::from(move_id);
        }
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
                    legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                        depth_byte & 0xF,
                    )
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
                legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                    depth_byte & 0xF,
                )
            {
                self.world.npcs.headings.insert(slot, heading);
            }
            // The poke binds byte `+4` like any other NPC arm: a party
            // member a cutscene seats and then walks (`korout` `P2[3]`'s
            // `CC 09 51 .. 0A` / `CC 0A 51 .. 0F` ahead of their `C1`
            // legs) plays its walk clip instead of gliding in the idle.
            self.world.carry_npc_run_anim(slot, move_id);
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
                    legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                        depth_byte & 0xF,
                    )
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
                    legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(
                        depth_byte & 0xF,
                    )
                {
                    self.world.npcs.headings.insert(slot, heading);
                }
                self.world.carry_npc_run_anim(slot, move_id);
            }
        }
    }
}
