//! Ambient NPC and object motion: seeding walkers, the per-frame ambient tick,
//! and the render-scale / tilt / effect outputs.

use super::*;

impl World {
    /// Point every not-yet-started ambient facing channel at its NPC's
    /// current render heading, converted back into the **retail** heading
    /// space the ambient ops work in (`retail = engine - 0x800`).
    ///
    /// Only channels that have not selected a variant yet (`live.is_none()`)
    /// are touched: once a stream is running its `+0x26` is the VM's own, and
    /// rewriting it would teleport a turn mid-ramp.
    pub(crate) fn resync_ambient_start_headings(&mut self) {
        for (slot, chan) in self.npcs.ambient.iter_mut() {
            if chan.live.is_some() {
                continue;
            }
            let engine = (self
                .npcs
                .headings
                .get(slot)
                .copied()
                .unwrap_or(super::super::SPAWN_HEADING)
                & 0x0FFF) as u16;
            chan.vm.heading = engine.wrapping_sub(0x800) & 0x0FFF;
        }
    }

    /// Re-seat every not-yet-started ambient channel on its NPC's current
    /// position. The channel install runs with the carriers, *before* the
    /// spawn-prologue pre-run relocates the story-parked and story-moved
    /// placements - and the `0x18` wander's AABB guard is absolute world
    /// space, so a channel left holding the raw MAN header tile retires its
    /// wander on the first tick and the villager never moves.
    ///
    /// Call after [`Self::pre_run_field_channel_prologues`].
    /// Capture alignment for the retail comparison's `play-window` child
    /// (`LEGAIA_SEAT_WALKERS`): stand the ambient walker whose flat MAN index
    /// (`+0x50`) is `flat` where a retail state holds it - `(x, z)` and the
    /// retail-space heading - on the frame it captures. Where a wanderer
    /// stands and which way it faces is its `rand()` picks and time since the
    /// entry, which no seed replays. The walker's motion channel is moved
    /// with it, so its next step starts from the seat.
    pub fn seed_ambient_walker(&mut self, flat: u16, x: i16, z: i16, heading: u16) {
        let Some(slot) = self
            .channel_view()
            .iter()
            .find(|c| !c.object_bind && c.ctx.script_id == flat)
            .and_then(|c| u8::try_from(c.placement_index).ok())
        else {
            return;
        };
        if !self.npcs.positions.contains_key(&slot) {
            return;
        }
        self.npcs.positions.insert(slot, (x, z));
        self.npcs.motions.remove(&slot);
        self.npcs
            .headings
            .insert(slot, (heading as i16).wrapping_add(0x800));
        if let Some(chan) = self.npcs.ambient.get_mut(&slot) {
            chan.vm.x = x;
            chan.vm.z = z;
            chan.vm.heading = heading;
        }
    }

    pub(crate) fn resync_ambient_start_positions(&mut self) {
        for (slot, chan) in self.npcs.ambient.iter_mut() {
            if chan.live.is_some() {
                continue;
            }
            if let Some(&(x, z)) = self.npcs.positions.get(slot) {
                chan.vm.x = x;
                chan.vm.z = z;
                // The anchor tile `+0x8C` / `+0x8D` re-seats with the
                // position: op `0x06` measures its box relative to it, so a
                // channel left holding the raw MAN header tile would bound a
                // relocated villager against where it used to stand.
                chan.vm.home_tile = (((x >> 7) & 0x7F) as u8, ((z >> 7) & 0x7F) as u8);
            }
        }
    }

    /// Seed the per-NPC **ambient facing** channels from the scene MAN's
    /// tail-section-1 motion streams - the idle turn-in-place behaviour of the
    /// second motion VM (`FUN_80038158` ops `0x04` / `0x0D`, ported at
    /// [`legaia_engine_motion_vm::ambient_motion`]). Without this a standing town NPC
    /// holds one heading forever where retail NPCs slowly look around.
    ///
    /// Binding resolution is the installer's: `FUN_8003A9D4` matches each
    /// record's `actor_id` byte against actor `+0x50`, and the placement
    /// spawner `FUN_8003A1E4` writes `+0x50 = N0 + placement_index` (`N0` =
    /// the MAN's partition-0 record count). The full variant table is carried
    /// per slot, not just the fresh-game one, because retail re-selects the
    /// live variant every tick against `DAT_80085758`
    /// ([`FieldNpcAmbient::select_variant`]).
    ///
    /// Each channel's VM starts from the heading the NPC is already standing
    /// in ([`crate::world::FieldNpcState::headings`], seeded by
    /// [`Self::seed_field_npc_facings`]), converted back into the **retail**
    /// heading space the ambient ops work in (`retail = engine - 0x800`), so
    /// an ambient turn starts where the spawn prologue left the actor.
    ///
    /// Call after [`Self::seed_field_npc_facings`].
    // PORT: FUN_80038158 (stream binding + variant table)
    // REF: FUN_8003A9D4 (binding installer), FUN_8003A1E4 (+0x50 = N0 + index)
    pub fn seed_field_npc_ambient(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
    ) {
        use legaia_scene_formats::man_motion;
        self.npcs.ambient.clear();
        let Some(n0) = man_file.partitions.first().map(|p| p.len()) else {
            return;
        };
        let records = man_motion::motion_records(man, man_file);
        // Streams bound to placed objects: an `actor_id` below `N0` names a
        // partition-0 record, the bind record a `.MAP` object's actor
        // carries in `+0x50`.
        self.npcs.object_ambient.clear();
        self.npcs.object_models.clear();
        for rec in &records {
            for b in &rec.bindings {
                let record = usize::from(b.actor_id);
                if record >= n0 || self.npcs.object_ambient.contains_key(&record) {
                    continue;
                }
                let variants: Vec<(u16, Vec<u8>)> = man_motion::stream_variants(man, rec)
                    .into_iter()
                    .filter_map(|v| {
                        man.get(v.code_offset..v.code_end)
                            .map(|code| (v.selector, code.to_vec()))
                    })
                    .filter(|(_, code)| !code.is_empty())
                    .collect();
                if variants.is_empty() {
                    continue;
                }
                let vm = legaia_engine_motion_vm::ambient_motion::AmbientMotion::new(
                    u32::from(b.actor_id),
                    0,
                );
                self.npcs.object_ambient.insert(
                    record,
                    FieldNpcAmbient {
                        variants,
                        live: None,
                        vm,
                        walks: false,
                        defers: b.enable & 1 != 0,
                    },
                );
            }
        }
        for p in man_file.actor_placements(man) {
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            let Some(bind_id) = n0.checked_add(p.index).and_then(|v| u8::try_from(v).ok()) else {
                continue;
            };
            if bind_id >= man_motion::ACTOR_PLAYER {
                continue; // collides with the 0xF8/0xFB special ids
            }
            let Some(rec) = records
                .iter()
                .find(|r| r.bindings.iter().any(|b| b.actor_id == bind_id))
            else {
                continue;
            };
            let variants: Vec<(u16, Vec<u8>)> = man_motion::stream_variants(man, rec)
                .into_iter()
                .filter_map(|v| {
                    man.get(v.code_offset..v.code_end)
                        .map(|code| (v.selector, code.to_vec()))
                })
                .filter(|(_, code)| !code.is_empty())
                .collect();
            if variants.is_empty() {
                continue;
            }
            // The ambient ops live in retail heading space; the engine's
            // render heading is the same compass rotated a half turn.
            let engine_heading = (self.npcs.heading(slot) & 0x0FFF) as u16;
            let retail_heading = engine_heading.wrapping_sub(0x800) & 0x0FFF;
            // Seat the channel where the spawn prologue left the actor: the
            // wander op's AABB guard is absolute, so a channel started at
            // the origin would retire its wander on the first tick.
            let (px, pz) = self.npcs.positions.get(&slot).copied().unwrap_or((0, 0));
            let mut vm = legaia_engine_motion_vm::ambient_motion::AmbientMotion::new(
                u32::from(slot),
                retail_heading,
            )
            .with_position(px, pz);
            // The seater's class bit: `FUN_8003A1E4` ORs `0x20000` into every
            // partition-1 placement it seats (`0x8003A3A4..0x8003A3B4`), so
            // the channel's `+0x10` word starts with it - the gate the
            // motion-pause kick (`Self::kick_field_npc_motion_pause`) tests.
            vm.actor_flags |= legaia_engine_field::motion_pause::MOVING_CLASS;
            // Per-actor RNG stream: retail draws from one global `rand()`,
            // so identical neighbours never step in lockstep. Deriving the
            // seed from the slot keeps that property and keeps a replay
            // deterministic.
            vm.rng = 0x9E37_79B9u32
                .wrapping_mul(u32::from(slot).wrapping_add(1))
                .wrapping_add(0x1234_5678);
            let walks = variants.iter().any(|(_, code)| stream_has_walk_op(code));
            let defers = rec
                .bindings
                .iter()
                .find(|b| b.actor_id == bind_id)
                .is_some_and(|b| b.enable & 1 != 0);
            self.npcs.ambient.insert(
                slot,
                FieldNpcAmbient {
                    variants,
                    live: None,
                    vm,
                    walks,
                    defers,
                },
            );
        }
    }

    /// Step every NPC's ambient facing channel one **actor game tick** and
    /// mirror the result into [`crate::world::FieldNpcState::headings`].
    ///
    /// `speed` is retail's `DAT_1F800393` ([`legaia_engine_vm::frame_clock::FrameClock::frame_step`]). The two ops
    /// respond to it differently and both readings are the retail law:
    /// `0x04`'s cursor is `addiu a0, a0, 1` - unit-per-tick, scalar-invariant,
    /// so its budget is denominated in *ticks*; `0x0D`'s wait cursor advances
    /// by the scalar, which is precisely what keeps it in lockstep with the
    /// ramp scheduler (`FUN_80036D80` decrements `remaining` by the same
    /// scalar, so op and ramp retire together).
    ///
    /// **The mirror is gated on the VM having actually moved the heading**,
    /// the same contract [`legaia_engine_motion_vm::motion_vm::MotionState`]'s
    /// `yaw_written` buys for the walk channel: an NPC parked in a `0x05`
    /// wait op must not re-stamp its stale ambient heading over a pose some
    /// other writer set - the interact "face the speaker" bearing, a scripted
    /// channel facing. Only a tick that changed the raw `+0x26` writes back.
    ///
    /// Retail's variant preamble re-selects the live variant every tick
    /// against `DAT_80085758`; a swap reseeds the record's cursor, so the
    /// port resets the VM's PC / cursor on a change rather than resuming the
    /// old variant's offset into new bytecode.
    // This is the host driver, not the port: both routines are ported in
    // `legaia_engine_motion_vm::ambient_motion`, whose module-level `PORT:` line
    // claims them, and this method only steps that VM and publishes the
    // result. So the tags here are cross-references - the same form
    // `world/frame_tick.rs`'s call site already uses for the pair.
    // REF: FUN_80038158 (facing channel drive), FUN_80036D80 (ramp pool)
    pub fn tick_field_npc_ambient(&mut self) {
        self.tick_object_ambient();
        if self.npcs.ambient.is_empty() {
            return;
        }
        let speed = self.clock.frame_step.max(1);
        // The walk half's collision service. Retail's two probes
        // (`FUN_801cf8ac` direct for the directional steps, `FUN_801d5a68`'s
        // three-point fan for the wander) both box-test against the
        // **player actor only** - not the wall grid, not other NPCs - so an
        // ambient walker's containment is its op's authored AABB and the
        // sole thing that can stop a step is the player standing in it.
        //
        // The opt-in liveliness (`Self::animate_field_npcs`) gates the
        // *mirror*, not the interpreter: with it off the stream still runs
        // its walk ops at their authored cadence, but neither the position
        // nor the walk-implied facing is published, so nothing on screen
        // moves. Suppressing the ops instead would stall the stream on its
        // first walking op (a blocked step re-runs forever without advancing
        // the PC) and a `0x07`/`0x08` story-flag write further down it would
        // then never fire.
        //
        // The VM's own position therefore drifts from the published seat
        // while the flag is off. That only shows if the flag is flipped
        // mid-scene, which no real entry path does - it is set once at boot
        // (`play-window`, opt-out `--no-live-npcs`).
        let live_walk = self.npcs.animate;
        let player = if live_walk {
            self.player_field_position()
        } else {
            None
        };
        let slots: Vec<u8> = self.npcs.ambient.keys().copied().collect();
        let mut globals_in = self.flags.story_flags;
        // Retail's engaged bit `+0x10 & 0x80000`. The engine raises the word
        // itself only on a touch post and reads a script context's
        // engagement through its predicate. A conversation is narrower than
        // retail here: it holds only the talker (retail's `+0x10 & 0x100`
        // on the actor whose record runs), not every deferring stream.
        let player_engaged = self.script_context_engages_player()
            || self
                .player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .is_some_and(|a| {
                    a.move_state.flags & legaia_engine_field::field_actor_program::PLAYER_ENGAGED
                        != 0
                });
        let talker = self.dialog.inline.as_ref().and_then(|id| id.npc_slot);
        for slot in slots {
            // Re-select against the live system-flag bank before stepping.
            let pick = self
                .npcs
                .ambient
                .get(&slot)
                .and_then(|c| c.select_variant(|f| self.system_flag_test(f)));
            let Some(pick) = pick else { continue };
            // The walker's own collision-exempt bits, off its live placement
            // context (op `0x31` sets them).
            let blocking = AmbientPlayerProbe {
                player,
                exempt: self.field_channel_flags(slot) & 3 != 0,
            };
            let seat = self.npcs.positions.get(&slot).copied();
            let chan_flags = self.field_channel_flags(slot);
            let Some(chan) = self.npcs.ambient.get_mut(&slot) else {
                continue;
            };
            // Retail has one position: the walk ops read and write the live
            // `+0x14` / `+0x18` that a script's `0x23` seat, a cross-context
            // walk or a teleport also write. While the mirror is live the
            // VM's copy equals the published seat unless some other writer
            // moved the actor, so adopt the seat - otherwise the next walk
            // step re-publishes the VM's stale coordinates and the actor
            // snaps back to where it wandered before the script placed it
            // (`conc3`'s cast, seated by `A3 2B 1A 1C`, was back in its
            // off-stage wander box a few hundred frames later and walked the
            // whole map to its mark). With the mirror off the two are
            // allowed to drift (see above), so the seat is not adopted.
            if live_walk && let Some((sx, sz)) = seat {
                chan.vm.x = sx;
                chan.vm.z = sz;
            }
            // Split the borrow across the struct's fields so the bytecode can
            // be read while the VM is stepped - no per-frame clone.
            let FieldNpcAmbient {
                variants,
                live,
                vm,
                defers,
                ..
            } = chan;
            // The stream's deferral gate (`FieldNpcAmbient::defers`): with
            // `+0x8A` bit 0 set the interpreter returns before its first op
            // while the player is engaged, the actor's own script or walk
            // kernel holds it (`+0x10 & 0x500`), or it stands on the off-map
            // park - the variant preamble included. The ramp pool is its own actor
            // and keeps running.
            // REF: FUN_80038158 (0x80038188..0x800381F4)
            let defer = *defers
                && (player_engaged
                    || talker == Some(slot)
                    || chan_flags & 0x500 != 0
                    || seat.is_some_and(|(x, z)| x >= 0x3F81 && z >= 0x3F81));
            if !defer && *live != Some(pick) {
                *live = Some(pick);
                vm.pc = 0;
                vm.cursor = 0;
            }
            let Some((_, code)) = variants.get(live.unwrap_or(pick)) else {
                continue;
            };
            let before = vm.heading;
            // Retail's `0x10` / `0x11` / `0x12` arms address the scratchpad
            // global flag word directly, so the channel carries a copy for
            // the tick and the drain below writes any change back. One
            // channel ticks at a time, so no two can race the word.
            vm.globals = globals_in;
            // Retail reaches this VM only through the field-actor driver
            // `FUN_8003BC08`, whose dispatch skips it on a set global freeze
            // (`_DAT_1F800394 & 0x400`) or a set `+0x10 & 8` on the actor -
            // both words the channel carries (`globals`, `actor_flags`). The
            // scene bracket guard is never held across a frame, and every
            // ambient channel has a stream bound, so those two inputs are
            // constant here. The ramp pool is its own actor and keeps
            // running either way.
            let suppressed = globals_in & legaia_engine_field::camera::CAMERA_HOLD_FLAG != 0;
            let plan = legaia_engine_motion_vm::motion_vm::field_actor_plan(
                legaia_engine_motion_vm::motion_vm::FieldActorInputs {
                    lifetime: vm.move_pair.unwrap_or(0),
                    flags: vm.actor_flags,
                    field_8e: 0,
                    path_target_present: false,
                    scripted_present: true,
                    ambient_gate: false,
                    frame_step: speed,
                    scene_guard_clear: true,
                    global_suppress: suppressed,
                },
            );
            if plan.dispatch.run_scripted && !defer {
                vm.tick_with(code, speed, &blocking);
            } else {
                vm.moved = false;
                vm.tick_ramps(speed);
            }
            let moved = vm.moved && live_walk;
            let (nx, nz) = (vm.x, vm.z);
            let anim = vm.requested_move;
            let globals_out = vm.globals;
            let scale_out = vm.scale;
            let tilt_out = (vm.pitch, vm.roll);
            let effects: Vec<_> = std::mem::take(&mut vm.effects);
            // A walk op's heading write is walk-direction-implied facing: it
            // only means anything alongside the step it accompanies. With the
            // walking suppressed it must be suppressed too, or the NPC pivots
            // on the spot through a motion it never performs - and in the
            // scene this matters most (`town01`) the fresh-game variants
            // author no turning at all, so that pivot would be pure artefact.
            // The `0x04` / `0x0D` ramps are ambient turning in their own
            // right and keep mirroring either way.
            let turned = vm.heading != before && (live_walk || !vm.walk_yaw);
            let engine_heading = vm.render_heading().wrapping_add(0x800) & 0x0FFF;
            if moved {
                // Retail's walk ops write the live `+0x14`/`+0x18`, which
                // every downstream probe reads: the NPC's own collision box,
                // the interact box, and the renderer's placement.
                self.npcs.positions.insert(slot, (nx, nz));
            }
            // The move-table consumer `FUN_800204F8`, which the same driver
            // dispatches after the scripted VM whenever `+0x5C > 0` and the
            // global freeze is down. It restarts the clip only on a changed
            // request ([`Self::carry_npc_run_anim`]'s `+0x5E` test), so it
            // runs every tick: a walker requests its walk anim on each step
            // and its standing move at the leg's end or when a step is
            // blocked, and a request the motion-pause kick
            // ([`Self::kick_field_npc_motion_pause`]) left is played unless
            // this tick's ops overwrote it first - retail's order. Gated with
            // the walk mirror: with the liveliness off no walk is published,
            // so no walk cycle may play in place either.
            if live_walk
                && !suppressed
                && let Some(id) = anim
            {
                self.carry_npc_run_anim(slot, id);
            }
            if turned {
                self.npcs.headings.insert(slot, engine_heading as i16);
            }
            // The other two of the actor's three authored angles. Retail's
            // per-actor render dispatcher hands `actor+0x24` whole to the
            // three-angle composer (`addiu a0,s0,0x24` / `jal 0x80026988` at
            // `0x8001af04` in `FUN_8001ADA4`), so the `0x15` / `0x16` tweens
            // are a draw input exactly as the `+0x26` heading is. Only a
            // non-zero pair is published: a slot that never tweened keeps no
            // entry, which is what lets both hosts' yaw-only fast path stay
            // the common case.
            if tilt_out != (0, 0) {
                self.npcs.tilts.insert(slot, tilt_out);
            } else {
                self.npcs.tilts.remove(&slot);
            }
            self.apply_ambient_motion_effects(slot, &effects);
            if globals_out != globals_in {
                self.flags.story_flags = globals_out;
                globals_in = globals_out;
            }
            // Retail has one actor record; the engine splits the field-VM
            // channel's copy of `+0x72` from the ambient channel's. Publish
            // the ambient write into the field-VM channel so
            // `World::field_npc_render_scale` - the one accessor both the
            // native window and the browser play page consult before drawing
            // an NPC - sees a `0x14` tween.
            if let Some(scale) = scale_out {
                self.publish_ambient_render_scale(slot, scale);
            }
        }
    }

    /// The live `(pitch, roll)` of a field NPC's actor draw - retail
    /// `actor+0x24` / `actor+0x28`, in the same 12-bit angle space as
    /// [`crate::world::FieldNpcState::headings`].
    ///
    /// `None` when the slot's scripted-motion channel has never tweened
    /// either angle, which is the ordinary case: the disc-wide census
    /// (`crates/engine-scene/tests/ambient_motion_op_census_disc.rs`) finds op
    /// `0x15` authored at zero sites and op `0x16` at 45, all in `juui1`. A
    /// host may therefore keep its cheap yaw-only model build for `None` and
    /// compose the full `Rx * Ry * Rz` (`FUN_80026988`) only here.
    ///
    /// REF: FUN_8001ADA4, FUN_80026988
    pub fn field_npc_tilt(&self, slot: u8) -> Option<(i16, i16)> {
        self.npcs.tilts.get(&slot).copied()
    }

    /// Publish an ambient channel's `actor+0x72` write into the field-VM
    /// channel both hosts already read through
    /// [`Self::field_npc_render_scale`].
    pub(in crate::world::field_movement) fn publish_ambient_render_scale(
        &mut self,
        slot: u8,
        scale: u16,
    ) {
        let idx = usize::from(slot);
        if let Some(c) = self
            .field_vm
            .channels
            .iter_mut()
            .find(|c| !c.object_bind && c.placement_index == idx)
        {
            c.ctx.field_72 = scale;
        }
    }

    /// Drain one ambient channel's per-tick side effects, in the order the
    /// VM queued them.
    ///
    /// Every variant with an engine mechanism is applied here:
    ///
    /// - [`AmbientEffect::ModelSwap`] records the new id on
    ///   [`crate::world::FieldNpcState::models`], which is the port's stand-in
    ///   for retail's `actor[+0x64]` store: retail's `FUN_80024E08` writes the
    ///   id onto the actor and reloads the mesh, while the port's hosts hold
    ///   the uploaded mesh themselves and read the id back through
    ///   [`World::field_npc_live_model`]. Nothing is resident to re-bind *to*
    ///   in the placements a host already uploaded - the disc census
    ///   (`ambient_motion_op_census_disc`) measures 215 authored sites over
    ///   four scenes and zero of them names a model some placement in the same
    ///   scene binds - so the bytes come from the scene's own model bank
    ///   ([`legaia_engine_scene::model_bank::SceneModelBank`]) instead.
    /// - [`AmbientEffect::MoveImage`] is applied: it is the same libgpu blit
    ///   the field VM's `4C 60` emitter queues, so it goes on the same
    ///   [`crate::world::AmbientFxState::script_vram_moves`] queue, which
    ///   both hosts already drain into their software VRAM
    ///   ([`Self::apply_script_vram_moves`]). The disc carries 24 `0x13`
    ///   sites, all in `edkorout`.
    ///
    /// [`AmbientEffect::SfxCue`] runs the enqueue half of `FUN_80035B50` -
    /// the same cursor / parked-slot / delay-table update the field VM's op
    /// `0x36` sub-`0` runs - so a scripted beat's cue parks the slot a
    /// following op-`0x36` sub-`4` delay write then targets - and queues the
    /// cue **id** as a [`legaia_engine_system::sound_state::SfxRingOp::Push`] for the host's audio
    /// ring, which both hosts drain and play.
    ///
    /// [`AmbientEffect::ModelSwap`]: legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect::ModelSwap
    /// [`AmbientEffect::MoveImage`]: legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect::MoveImage
    /// [`AmbientEffect::SfxCue`]: legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect::SfxCue
    // REF: FUN_80035B50 (the enqueue the `SfxCue` arm reproduces)
    pub(in crate::world::field_movement) fn apply_ambient_motion_effects(
        &mut self,
        slot: u8,
        effects: &[legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect],
    ) {
        use legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect as Fx;
        for fx in effects {
            match *fx {
                Fx::SystemFlagSet(idx) => self.system_flag_set(idx),
                Fx::SystemFlagClear(idx) => self.system_flag_clear(idx),
                Fx::Teleport { x, z, .. } => {
                    // Not gated on the liveliness toggle: retail's `0x0F` is
                    // scripted placement, not ambient walking, and the
                    // spawn-prologue parks depend on it landing.
                    self.npcs.positions.insert(slot, (x, z));
                }
                Fx::SfxCue(id) => {
                    let cursor = self.audio.sfx_cue_cursor;
                    self.audio.sfx_cue_cursor = self.audio.sfx_cue_delays.park(cursor);
                    self.audio.sfx_parked_slot = cursor;
                    // `jal 0x80035B50` at `0x80039178`: the id reaches the
                    // host ring, which plays it.
                    self.audio
                        .sfx_ring_ops
                        .push(legaia_engine_system::sound_state::SfxRingOp::Push(id));
                }
                Fx::MoveImage { rect, dx, dy } => {
                    // The same libgpu blit the field VM's `4C 60` emitter
                    // queues, so it goes on the same queue - which both hosts
                    // already drain into their software VRAM
                    // (`World::apply_script_vram_moves`). `dx` / `dy` are
                    // `MoveImage`'s destination ORIGIN, not a delta, which is
                    // also how the `4C 60` operand words read.
                    let clamp = |v: u16| i16::try_from(v).unwrap_or(i16::MAX);
                    self.queue_script_vram_move([
                        clamp(rect[0]),
                        clamp(rect[1]),
                        clamp(rect[2]),
                        clamp(rect[3]),
                        dx,
                        dy,
                    ]);
                }
                Fx::ModelSwap { bank, offset } => {
                    // Back to the raw operand space both pool consumers
                    // index, which is what a host resolves through
                    // `model_bank::resolve_model_id`. The VM already applied
                    // the `0xF0` split; re-adding the threshold on the
                    // special arm is its inverse, not a second decode.
                    use legaia_engine_motion_vm::ambient_motion_ops::ModelBank as VmBank;
                    let id = match bank {
                        VmBank::Scene => offset,
                        VmBank::Special => offset.wrapping_add(
                            legaia_engine_scene::model_bank::SPECIAL_MODEL_THRESHOLD as i16,
                        ),
                    };
                    self.npcs.models.insert(slot, id);
                }
                Fx::BitTargetFault => {}
            }
        }
    }

    /// Step the placed objects' scripted-motion streams
    /// ([`crate::world::FieldNpcState::object_ambient`]) one actor game tick.
    /// Only the model swap (op `0x0E`) has a placed-object consumer; the
    /// streams the disc binds to objects carry nothing else that draws.
    ///
    /// REF: FUN_80038158, FUN_8003BC08 (the same per-actor driver as the
    /// placements' streams)
    pub(in crate::world::field_movement) fn tick_object_ambient(&mut self) {
        if self.npcs.object_ambient.is_empty() {
            return;
        }
        let speed = self.clock.frame_step.max(1);
        let probe = AmbientPlayerProbe {
            player: None,
            exempt: true,
        };
        let suppressed =
            self.flags.story_flags & legaia_engine_field::camera::CAMERA_HOLD_FLAG != 0;
        let records: Vec<usize> = self.npcs.object_ambient.keys().copied().collect();
        for record in records {
            let pick = self
                .npcs
                .object_ambient
                .get(&record)
                .and_then(|c| c.select_variant(|f| self.system_flag_test(f)));
            let Some(pick) = pick else { continue };
            let Some(chan) = self.npcs.object_ambient.get_mut(&record) else {
                continue;
            };
            let FieldNpcAmbient {
                variants, live, vm, ..
            } = chan;
            if *live != Some(pick) {
                *live = Some(pick);
                vm.pc = 0;
                vm.cursor = 0;
            }
            let Some((_, code)) = variants.get(pick) else {
                continue;
            };
            if suppressed {
                continue;
            }
            vm.tick_with(code, speed, &probe);
            for fx in std::mem::take(&mut vm.effects) {
                if let legaia_engine_motion_vm::ambient_motion_ops::AmbientEffect::ModelSwap {
                    bank,
                    offset,
                } = fx
                {
                    use legaia_engine_motion_vm::ambient_motion_ops::ModelBank as VmBank;
                    let id = match bank {
                        VmBank::Scene => offset,
                        VmBank::Special => offset.wrapping_add(
                            legaia_engine_scene::model_bank::SPECIAL_MODEL_THRESHOLD as i16,
                        ),
                    };
                    self.npcs.object_models.insert(record, id);
                }
            }
        }
    }
}
