//! NPC facing, ambient motion, scripted NPC / actor motion and the walk-touch check.
//! Split out of `field_movement.rs`.

use super::*;

impl World {
    /// Turn the player toward field NPC `npc_slot` (retail's face-the-NPC
    /// step after a successful interact probe: `func_0x80019b28` computes
    /// the 12-bit angle from the touched actor to the player and stores it
    /// in the player's `+0x26`). The engine computes the same angle with
    /// float `atan2` in its own heading convention (`0` = Z+) rather than
    /// retail's arctan LUT at `0x8006f4c8`, so it is shape-faithful, not
    /// bit-exact - the value only feeds the heading marker and the next
    /// probe's 45° sector quantisation.
    ///
    /// REF: FUN_80019b28
    pub(crate) fn face_field_npc(&mut self, npc_slot: u8) {
        let Some(&(nx, nz)) = self.npcs.positions.get(&npc_slot) else {
            return;
        };
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() {
            return;
        }
        let ms = &mut self.actors[slot].move_state;
        let (dx, dz) = (
            (nx as i32 - ms.world_x as i32) as f32,
            (nz as i32 - ms.world_z as i32) as f32,
        );
        if dx == 0.0 && dz == 0.0 {
            return;
        }
        ms.render_26 = engine_bearing(dx, dz);
    }

    /// Turn field NPC `slot` to face the player, and remember the heading it
    /// was standing with - the talk-time half of retail's save / snap /
    /// restore triple.
    ///
    /// The live actor flag word (`+0x10`) of placement `slot`'s field-VM
    /// channel - what the spawn prologue's `0x31` / `0x32` CFLAG ops left set
    /// once [`Self::pre_run_field_channel_prologues`] ran the record's
    /// story-true branch. `0` when no channel is installed for the slot.
    ///
    /// It must be the **live** context rather than a static decode of the
    /// record: some placements author both `31 16` and `32 16` in one
    /// prologue on different story branches, so only the executed branch is
    /// the truth.
    ///
    /// REF: FUN_8003A1E4 (per-placement context), FUN_801DE840 (op 0x31)
    pub fn field_channel_flags(&self, slot: u8) -> u32 {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == slot as usize)
            .map_or(0, |c| c.ctx.flags)
    }

    /// Gated on the placement's live class word - see the body.
    ///
    /// **Retail snaps; it does not ramp.** The write is a single `sh` in the
    /// per-actor dialog SM: `FUN_80039B7C` reaches `0x80039F48` when it moves
    /// its context to state 1 (the script has yielded and the box is about to
    /// open), tests the addressed actor's class word
    /// (`flags & 0x420000 == 0x20000` - moving-class set, `0x400000` clear, so
    /// static props never turn), calls the bearing resolver `FUN_80019B28` and
    /// stores the result straight into `+0x26` at `0x80039F80`. There is no
    /// budget word, no `+0x54` ramp cursor and no per-frame re-entry: one
    /// instruction, one frame.
    ///
    /// That distinguishes it from the motion VM's `0x4C` `FaceTarget` leg
    /// ([`Self::face_field_npc_toward`]), which really does ramp over a frame
    /// budget and is the *scripted* poser - the two functions write the same
    /// field and the last writer of the frame draws.
    ///
    /// Note also which endpoints retail passes. `0x4C` calls
    /// `FUN_80019B28(self.z, self.x, tgt.z, tgt.x)` and adds `0x800`; this
    /// site calls it `(player.z, player.x, self.z, self.x)` with **no**
    /// `0x800`. Swapping the endpoints is the half-turn, so the two agree - a
    /// port that copied the `0x4C` convention here *and* kept the `0x800`
    /// would face the NPC exactly backwards.
    ///
    /// The previous heading goes into [`crate::world::FieldNpcState::facing_save`], which
    /// [`Self::release_talk_facing`] writes back when the conversation ends.
    /// No-op for a slot with no surfaced position (retail's actor-list miss)
    /// and while a save is already outstanding, so a nested interaction cannot
    /// overwrite the authored heading with a talk pose.
    ///
    /// The engine resolves the bearing with float `atan2` in its own heading
    /// convention (`0` = Z+) rather than retail's arctan LUT at `0x8006F4C8`,
    /// so it is shape-faithful rather than bit-exact.
    ///
    /// PORT: FUN_80039B7C (the `0x80039F48..0x80039F80` face-the-player arm)
    /// REF: FUN_80019B28, FUN_801D5B5C, FUN_8003A1E4, FUN_80020DE0
    pub fn face_field_npc_at_player(&mut self, slot: u8) {
        // Retail's class gate, which the paragraphs above describe and this
        // arm used to skip: the snap runs only for `flags & 0x420000 ==
        // 0x20000`. `FUN_8003A1E4` ORs `0x20000` into every partition-1
        // placement and the allocator template leaves `0x400000` clear
        // (`FUN_80020DE0`), so on the disc the only discriminator is the
        // opt-out - a spawn prologue `31 16` (`CFLAG_SET` bit 22) marking a
        // placement that must keep its authored pose. The engine does not
        // mirror the `0x20000` OR, so the test reduces to "bit 22 clear".
        //
        // koin6's two cribbed babies carry it, which is why they used to
        // rotate ~51 degrees out of their cribs to look at the player.
        if self.field_channel_flags(slot) & 0x0040_0000 != 0 {
            return;
        }
        let Some(&(nx, nz)) = self.npcs.positions.get(&slot) else {
            return;
        };
        let Some(pslot) = self.player_actor_slot else {
            return;
        };
        let Some(pactor) = self.actors.get(pslot as usize) else {
            return;
        };
        let (px, pz) = (pactor.move_state.world_x, pactor.move_state.world_z);
        let (dx, dz) = (
            (px as i32 - nx as i32) as f32,
            (pz as i32 - nz as i32) as f32,
        );
        if dx == 0.0 && dz == 0.0 {
            return;
        }
        if self.npcs.facing_save.is_none() {
            let prev = self.npcs.heading(slot);
            self.npcs.facing_save = Some((slot, prev));
        }
        self.npcs.headings.insert(slot, engine_bearing(dx, dz));
    }

    /// Write an addressed NPC's pre-talk heading back, once the conversation
    /// that turned it has ended - retail's `+0x5A` restore, which the dialog
    /// SM's teardown performs at `0x80039CE8` / `0x80039EBC` (`lhu v0,0x5a` /
    /// `sh v0,0x26`) on the way out.
    ///
    /// A no-op with no save outstanding, and - deliberately - a no-op for a
    /// slot whose heading has been rewritten since the snap. That covers the
    /// case retail covers with its `0x400000` class test in the other
    /// direction: an NPC that *walked* during the conversation (a scripted
    /// interaction leg, an ambient wander step) has earned a new
    /// heading, and restoring the pre-talk one would teleport its facing.
    ///
    /// PORT: FUN_80039B7C (the `+0x5A` -> `+0x26` interaction-end restore)
    pub fn release_talk_facing(&mut self) {
        let Some((slot, prev)) = self.npcs.facing_save.take() else {
            return;
        };
        // Only restore when the talk pose is still the live heading.
        let posed = match (
            self.npcs.positions.get(&slot),
            self.player_actor_slot
                .and_then(|p| self.actors.get(p as usize)),
        ) {
            (Some(&(nx, nz)), Some(pactor)) => {
                let (px, pz) = (pactor.move_state.world_x, pactor.move_state.world_z);
                let (dx, dz) = (
                    (px as i32 - nx as i32) as f32,
                    (pz as i32 - nz as i32) as f32,
                );
                (dx != 0.0 || dz != 0.0).then(|| engine_bearing(dx, dz))
            }
            _ => None,
        };
        if posed.is_some() && self.npcs.headings.get(&slot).copied() != posed {
            return;
        }
        self.npcs.headings.insert(slot, prev);
    }

    /// Seed each placed field NPC's **initial facing** from its MAN spawn
    /// prologue, so a never-walked NPC stands with its retail heading instead
    /// of the unrotated default.
    ///
    /// Retail applies these at scene load: the placement installer
    /// `FUN_8003A1E4` pre-runs the record's `0x24`/`0x25`-marked prologue
    /// through the field VM, and the prologue's `0x4C 0x51` / `0x38`
    /// (simple-path) ops write the actor's `+0x26` heading from the
    /// 8-direction LUT at SCUS `0x80073F04`
    /// ([`crate::man_field_scripts::placement_initial_facing`]). The engine
    /// derives the same LUT index statically per placement and stores the
    /// converted 12-bit engine heading (`0` = Z+;
    /// [`crate::man_field_scripts::facing_index_to_engine_heading`]) in
    /// [`crate::world::FieldNpcState::headings`] - the map every NPC draw reads. A later
    /// walk overwrites the slot exactly as retail's per-step facing writes
    /// overwrite `+0x26`, and an already-present heading (a scripted channel
    /// move that ran first) is kept.
    ///
    /// Call after [`Self::install_field_carriers_from_man`] (whose inner
    /// install clears `field_npc_headings`).
    // PORT: FUN_8003A1E4 (spawn prologue pre-run -> +0x26 facing writes)
    // REF: FUN_801DE840, FUN_801d01b0 (heading-space convention)
    pub fn seed_field_npc_facings(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        for p in man_file.actor_placements(man) {
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            let Some(idx) = crate::man_field_scripts::placement_initial_facing(man_file, man, &p)
            else {
                continue;
            };
            let Some(heading) = crate::man_field_scripts::facing_index_to_engine_heading(idx)
            else {
                continue;
            };
            self.npcs.headings.entry(slot).or_insert(heading);
        }
        // The ambient facing channels are installed by
        // `install_field_carriers_from_man`, which runs BEFORE this pass - so
        // at install time the headings map is still empty and every channel
        // started from compass zero. Re-seed the start heading of every
        // channel that has not ticked yet, so an ambient turn begins where
        // the spawn prologue actually left the actor.
        self.resync_ambient_start_headings();
    }

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
                .unwrap_or(super::SPAWN_HEADING)
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
    /// [`legaia_engine_vm::ambient_motion`]). Without this a standing town NPC
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
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        use legaia_asset::man_motion;
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
                let vm = vm::ambient_motion::AmbientMotion::new(u32::from(b.actor_id), 0);
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
            let mut vm = vm::ambient_motion::AmbientMotion::new(u32::from(slot), retail_heading)
                .with_position(px, pz);
            // The seater's class bit: `FUN_8003A1E4` ORs `0x20000` into every
            // partition-1 placement it seats (`0x8003A3A4..0x8003A3B4`), so
            // the channel's `+0x10` word starts with it - the gate the
            // motion-pause kick (`Self::kick_field_npc_motion_pause`) tests.
            vm.actor_flags |= vm::motion_pause::MOVING_CLASS;
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
    /// `speed` is retail's `DAT_1F800393` ([`crate::world::FrameClock::frame_step`]). The two ops
    /// respond to it differently and both readings are the retail law:
    /// `0x04`'s cursor is `addiu a0, a0, 1` - unit-per-tick, scalar-invariant,
    /// so its budget is denominated in *ticks*; `0x0D`'s wait cursor advances
    /// by the scalar, which is precisely what keeps it in lockstep with the
    /// ramp scheduler (`FUN_80036D80` decrements `remaining` by the same
    /// scalar, so op and ramp retire together).
    ///
    /// **The mirror is gated on the VM having actually moved the heading**,
    /// the same contract [`legaia_engine_vm::motion_vm::MotionState`]'s
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
    // `legaia_engine_vm::ambient_motion`, whose module-level `PORT:` line
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
                    a.move_state.flags & crate::field_actor_program::PLAYER_ENGAGED != 0
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
            let suppressed = globals_in & crate::world::CAMERA_HOLD_FLAG != 0;
            let plan = vm::motion_vm::field_actor_plan(vm::motion_vm::FieldActorInputs {
                lifetime: vm.move_pair.unwrap_or(0),
                flags: vm.actor_flags,
                field_8e: 0,
                path_target_present: false,
                scripted_present: true,
                ambient_gate: false,
                frame_step: speed,
                scene_guard_clear: true,
                global_suppress: suppressed,
            });
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
    /// (`crates/engine-core/tests/ambient_motion_op_census_disc.rs`) finds op
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
    pub(super) fn publish_ambient_render_scale(&mut self, slot: u8, scale: u16) {
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
    ///   ([`crate::model_bank::SceneModelBank`]) instead.
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
    /// cue **id** as a [`crate::world::SfxRingOp::Push`] for the host's audio
    /// ring, which both hosts drain and play.
    ///
    /// [`AmbientEffect::ModelSwap`]: legaia_engine_vm::ambient_motion_ops::AmbientEffect::ModelSwap
    /// [`AmbientEffect::MoveImage`]: legaia_engine_vm::ambient_motion_ops::AmbientEffect::MoveImage
    /// [`AmbientEffect::SfxCue`]: legaia_engine_vm::ambient_motion_ops::AmbientEffect::SfxCue
    // REF: FUN_80035B50 (the enqueue the `SfxCue` arm reproduces)
    pub(super) fn apply_ambient_motion_effects(
        &mut self,
        slot: u8,
        effects: &[legaia_engine_vm::ambient_motion_ops::AmbientEffect],
    ) {
        use legaia_engine_vm::ambient_motion_ops::AmbientEffect as Fx;
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
                        .push(crate::world::SfxRingOp::Push(id));
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
                    use legaia_engine_vm::ambient_motion_ops::ModelBank as VmBank;
                    let id = match bank {
                        VmBank::Scene => offset,
                        VmBank::Special => {
                            offset.wrapping_add(crate::model_bank::SPECIAL_MODEL_THRESHOLD as i16)
                        }
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
    pub(super) fn tick_object_ambient(&mut self) {
        if self.npcs.object_ambient.is_empty() {
            return;
        }
        let speed = self.clock.frame_step.max(1);
        let probe = AmbientPlayerProbe {
            player: None,
            exempt: true,
        };
        let suppressed = self.flags.story_flags & crate::world::CAMERA_HOLD_FLAG != 0;
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
                if let vm::ambient_motion_ops::AmbientEffect::ModelSwap { bank, offset } = fx {
                    use legaia_engine_vm::ambient_motion_ops::ModelBank as VmBank;
                    let id = match bank {
                        VmBank::Scene => offset,
                        VmBank::Special => {
                            offset.wrapping_add(crate::model_bank::SPECIAL_MODEL_THRESHOLD as i16)
                        }
                    };
                    self.npcs.object_models.insert(record, id);
                }
            }
        }
    }

    /// The live scene-bank model id each placed object's stream swapped in
    /// (op `0x0E`), keyed by bind record. Both hosts draw a placed object
    /// whose bind record is here with `env_pack[id]` instead of its `.MAP`
    /// pack slot; an id at or past `0xF0` (the player bank) is never a
    /// placed object's and is ignored.
    pub fn object_live_models(&self) -> &std::collections::BTreeMap<usize, i16> {
        &self.npcs.object_models
    }

    /// The look rotation (field-VM `4C 45`) an animated actor draws with
    /// this frame, or `None` while it turns nothing. Both play hosts fold it
    /// into the actor's pose with [`crate::actor_look::apply_look`] before
    /// they skin the mesh - the player's rig and every posed NPC.
    // REF: FUN_8001B964 (0x8001BB40..0x8001BB88)
    pub fn actor_look(
        &self,
        key: crate::actor_look::LookKey,
    ) -> Option<crate::actor_look::ActorLook> {
        self.npcs.looks.turning(key)
    }

    /// Capture alignment: set a placed object's live model where a motion
    /// stream drives it (a record outside
    /// [`crate::world::FieldNpcState::object_ambient`] is left alone). The
    /// retail comparison's image child only.
    pub fn seed_object_live_model(&mut self, record: usize, model: i16) {
        if self.npcs.object_ambient.contains_key(&record) {
            self.npcs.object_models.insert(record, model);
        }
    }

    /// The live model id the scripted-motion VM's op `0x0E` re-bound this
    /// placement slot to, or `None` while the actor still draws its spawn
    /// mesh.
    ///
    /// The **one** question a host asks per slot per scene load. Both hosts
    /// ask it: the native window's `upload_assets` and the browser's
    /// `play_npc_live_model` export, each feeding the id to
    /// [`crate::model_bank::SceneModelBank::tmd_bytes`] to materialise the
    /// mesh. Retail needs no equivalent - it reloads the mesh inside
    /// `FUN_80024E08` - but the port's mesh lives on the host, so the id has
    /// to cross that boundary.
    pub fn field_npc_live_model(&self, slot: u8) -> Option<i16> {
        self.npcs.models.get(&slot).copied()
    }

    /// The clip id a placement slot's actor carries once its spawn prologue
    /// has run - retail's `actor[+0x5C]`, the word the per-actor anim tick
    /// `FUN_800204F8` binds as `record = id - 1`. The MAN placement header
    /// seeds it, but a prologue can rewrite it before the first drawn frame:
    /// every save crystal ships header anim `0` and its record sets `22`
    /// (the locomotion bundle's savepoint clip, record 21) - retail's
    /// `conc_field_card_boot` capture holds `0x16` at the crystal actor's
    /// `+0x5C` and draw kind `1` (posed). Read at the header anim byte alone,
    /// the crystal's three objects draw unposed on the origin.
    ///
    /// `None` when no channel owns the slot or the channel's id is zero (a
    /// model re-stage clears it); the host then keeps the header anim.
    // REF: FUN_800204F8 (clip bind off actor+0x5C), FUN_8003A1E4 (prologue)
    pub fn field_npc_live_anim(&self, slot: usize) -> Option<u8> {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == slot)
            .and_then(|c| u8::try_from(c.ctx.move_id).ok())
            .filter(|&id| id != 0)
    }

    /// Install a live model id on a slot, as op `0x0E` does. For a host or a
    /// test that drives the re-bind directly.
    pub fn set_field_npc_live_model(&mut self, slot: u8, id: i16) {
        self.npcs.models.insert(slot, id);
    }

    /// The player actor's live field position, or `None` when no player
    /// actor is seated (a headless scene inspection).
    pub(crate) fn player_field_position(&self) -> Option<(i16, i16)> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        if !a.active {
            return None;
        }
        Some((a.move_state.world_x, a.move_state.world_z))
    }

    /// Start a field NPC walking to world `(tx, tz)` through the motion VM -
    /// the engine's start-motion kernel for the MAN-placed actor set. Mirrors
    /// the retail start shape: write the walk target onto the actor and reset
    /// the glide state so the per-frame motion stepper picks it up (retail's
    /// `FUN_800358c0` writes the target into the actor `+0xA`/`+0xC` + subobj
    /// mirrors and clears the `+0x20` glide cursor; the per-frame consumer is
    /// the motion VM `FUN_8003774C`, ported in
    /// [`legaia_engine_vm::motion_vm`]). Returns `false` (and does nothing)
    /// when `slot` is not an installed field NPC - the retail actor-list
    /// search miss, which returns 0.
    ///
    /// Every leg started here is *scripted*: it runs even while
    /// [`crate::world::FieldNpcState::animate`] is off and even during a dialogue
    /// (the interaction partner executing its own prologue walk), and ends
    /// where it lands.
    ///
    /// REF: FUN_800358c0, FUN_8003774C
    pub fn start_field_npc_motion(&mut self, slot: u8, tx: i16, tz: i16) -> bool {
        let Some(&(cx, cz)) = self.npcs.positions.get(&slot) else {
            return false;
        };
        // Faithful glide speed: the placement's own `0x4C 0x51` motion-op
        // base step (retail `FUN_8003774C` `4 << bits`), derived at scene load
        // into `field_npc_glide_speeds`; the stand-in `FIELD_NPC_MOTION_SPEED`
        // is the fallback for a placement with no decodable motion leg.
        let speed = self
            .npcs
            .glide_speeds
            .get(&slot)
            .copied()
            .unwrap_or(FIELD_NPC_MOTION_SPEED);
        self.npcs.motions.insert(
            slot,
            FieldNpcMotion {
                state: vm::motion_vm::MotionState {
                    world_x: cx,
                    world_y: 0,
                    world_z: cz,
                    speed,
                    // Seed from the facing the NPC is standing in (retail
                    // reads the live `+0x26`), so a leg that never moves - or
                    // one whose first frame is blocked - keeps it instead of
                    // snapping to the compass origin.
                    yaw: (self.npcs.heading(slot) & 0x0FFF) as u16,
                    ..Default::default()
                },
                target: (tx, tz),
            },
        );
        true
    }

    /// Attach the `4C 51` record's byte-`+4` **move-anim id** to a just-started
    /// NPC glide leg. Retail's run dispatch writes that byte to the actor's
    /// `+0x5C` anim slot (consumed by the anim-stream stepper `FUN_800204F8`),
    /// so the walk plays its named move clip instead of gliding in a frozen
    /// pose. The engine surfaces it as a [`crate::world::FieldNpcState::anim_cues`] entry -
    /// the same shape the cross-context `A2` ExecMove raises - keyed by the
    /// placement slot. A zero id carries no clip (retail's `+0x5C = 0` is the
    /// "no move-anim" sentinel, not clip `-1`).
    ///
    /// A request for the move the slot is already playing raises nothing:
    /// the consumer restarts a clip only when `+0x5C` differs from the
    /// playing `+0x5E` (`0x80020570..0x800205A8`), and a cue here would
    /// restart it at frame `0` on every host. That matters because the
    /// ambient walk ops request their walk move on every step, so without
    /// the test a walker's cycle never got past its first frames.
    ///
    /// REF: FUN_80024E08, FUN_800204F8 (actor `+0x5C` anim-slot consumer)
    pub(crate) fn carry_npc_run_anim(&mut self, slot: u8, move_id: u8) {
        if move_id != 0 && self.npcs.clip_current.get(&slot) != Some(&move_id) {
            self.npcs.anim_cues.insert(slot, (1, move_id, Vec::new()));
        }
    }

    /// Turn a stationary field NPC to face world `(tx, tz)` - the retail
    /// "face the speaker" cinematic pose. This runs one shot of the ported
    /// motion VM's `0x4C` `FaceTarget` op (the yaw-rotate leg of
    /// `FUN_8003774C`, [`legaia_engine_vm::motion_vm`]) seeded from the NPC's
    /// current heading and settles the resulting 12-bit yaw straight into
    /// [`crate::world::FieldNpcState::headings`] - the map every NPC draw reads. It is the
    /// runtime driver the retail dialog engine invokes when the player talks
    /// to an actor (a `FaceTarget` leg whose budget is small enough to snap in
    /// one step), and is a no-op for a slot with no surfaced position (the
    /// retail actor-list miss returns 0 and never poses the actor).
    ///
    /// REF: FUN_8003774C (0x4C FaceTarget), FUN_80019B28 (bearing)
    pub fn face_field_npc_toward(&mut self, slot: u8, tx: i16, tz: i16) {
        let Some(&(cx, cz)) = self.npcs.positions.get(&slot) else {
            return;
        };
        // Seed the one-shot VM state from the NPC's current facing so the leg
        // rotates *from* where it stands (a full match for retail's actor
        // `+0x26` seed) and mask into the 12-bit yaw space the op expects.
        let cur_yaw = (self.npcs.heading(slot) & 0x0FFF) as u16;
        let mut state = vm::motion_vm::MotionState {
            world_x: cx,
            world_z: cz,
            // Budget of 1 (below) against this speed makes the FaceTarget leg
            // settle onto the exact bearing in a single step - the dialog
            // "snap to face the speaker" the retail engine performs on talk.
            speed: 0x0400,
            yaw: cur_yaw,
            ..Default::default()
        };
        let target = vm::motion_vm::MotionTarget {
            x: tx,
            y: 0,
            z: tz,
            id: 0,
        };
        // `0x4C` FaceTarget, sub-mode `0x85` (rotate yaw), budget `0x0001`,
        // target byte `0xF8` (self); no high bit -> the body starts at +1.
        const FACE_TARGET_PROGRAM: [u8; 5] = [0x4C, 0x85, 0x01, 0x00, 0xF8];
        let _ = vm::motion_vm::step(&mut state, target, &FACE_TARGET_PROGRAM);
        self.npcs.headings.insert(slot, (state.yaw & 0x0FFF) as i16);
    }

    /// Step every in-flight field-NPC walk leg one frame through the ported
    /// motion VM, writing each NPC's new position back into
    /// [`crate::world::FieldNpcState::positions`] - so the moving NPC's
    /// ±40-unit collision box ([`Self::field_actor_dir_blocked`]) and its
    /// interact box ([`Self::field_interact_probe_slot`]) follow the live
    /// position, exactly as retail probes the live `+0x14`/`+0x18` rather
    /// than the spawn anchor.
    ///
    /// Every leg here is **scripted**: a cutscene timeline's cross-context
    /// walk or an actor-VM `start_motion`. Each one ends where it lands, and none is gated by
    /// [`crate::world::FieldNpcState::animate`] - a script started it. There
    /// is no `0x4C 0x51` leg: that op is an instant seat wherever it runs,
    /// and a placement's own `0x4C 0x51` ops are instant
    /// story-branch **seats** that the scene-entry pre-run already applied,
    /// and a villager's ambient wandering is its tail-section-1 stream
    /// (`World::tick_field_npc_ambient`), so nothing walks between seats.
    ///
    /// REF: FUN_8003774C
    pub(crate) fn tick_field_npc_motions(&mut self) {
        self.tick_field_npc_face_legs();
        let slots: Vec<u8> = self.npcs.motions.keys().copied().collect();
        for slot in slots {
            let Some(motion) = self.npcs.motions.get_mut(&slot) else {
                continue;
            };
            let target = vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let result = vm::motion_vm::step(&mut motion.state, target, &FIELD_NPC_MOTION_PROGRAM);
            let pos = (motion.state.world_x, motion.state.world_z);
            // The walker's heading is the one the `0x47` step itself snapped
            // onto the eight-point compass - written **once per leg** at the
            // first moving frame and again on a step-direction change, the
            // retail write pattern (runtime-pinned: a walk leg holds one
            // heading for its whole run). Gating the copy on the VM's own
            // write keeps a heading some other writer posed (the interact
            // face-the-speaker bearing) standing while a leg idles unmoved.
            if motion.state.yaw_written {
                self.npcs.headings.insert(slot, motion.state.yaw as i16);
            }
            self.npcs.positions.insert(slot, pos);
            if result == vm::motion_vm::StepResult::Done {
                self.npcs.motions.remove(&slot);
            }
        }
    }

    /// The locomotion's per-step **walk-touch dispatch**: when the player's
    /// body stands inside a walk-touch placement's static contact box
    /// (±[`FIELD_PROP_BOX_HALF`]), post that placement's event - no button
    /// press, the same dispatch path the button-gated interact uses
    /// ([`Self::trigger_field_interact`]) plus the decoded script effect:
    ///
    /// - [`WalkTouchEvent::Warp`] → arm the **mode-24 minigame door-warp**
    ///   (the effect of the record's `0x3E` op, staged exactly as the
    ///   field-VM arm stages it - see [`crate::minigame_entry`]);
    /// - [`WalkTouchEvent::PlayerMoveTo`] → snap the player to the decoded
    ///   world coords (the record's cross-context `0x23` into the player
    ///   channel) and surface a [`FieldEvent::MoveTo`].
    ///
    /// Retail posts the touch event (`FUN_801d5b5c`) on every contact step,
    /// gated by the player's `+0x10 & 0x80000` engaged flag until the dialog
    /// SM teardown clears it; the engine latches one post per contact
    /// ([`crate::world::FieldPropState::active_walk_touch`]) instead. The full post kernel (engaged
    /// flag, facing save/restore, touch counters) is not modelled.
    ///
    /// REF: FUN_801d5b5c, FUN_801cfc40
    pub(super) fn check_field_walk_touch(&mut self) {
        if self.props.walk_touch.is_empty() {
            self.props.active_walk_touch = None;
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if !self.props.arrival_exempt.is_empty() && self.dialog.inline.is_none() {
            self.walk_off_sealed_arrival(slot);
        }
        let (px, pz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        // Contact fires from the SAME probe points that block movement
        // (retail: `FUN_801cfe4c`'s three `FUN_801cfc40` calls both refuse
        // the step and link/post the touched actor) - so a **solid** door
        // object still fires its walk-touch while the player stands pressed
        // against its box, 64+ units short of the centre. The stand-inside
        // test is kept as well (a landing seated inside a box, nav drivers).
        let mut points: Vec<(i32, i32)> = vec![(px as i32, pz as i32)];
        for dir in Self::dirs_of_bits(self.locomotion.last_move_dir_bits) {
            for &(dx, dz) in &FIELD_ACTOR_PROBES[dir] {
                points.push((px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32));
            }
        }
        // The arrival bracket lifts once the player is off the platform: no
        // probe point in any direction reaches the partner's contact box.
        // Not while the ride's own record is still running it: a lift parks
        // the player on tile (0, 0) and tours the floors before the arrival
        // `MoveTo`, and retail's `B2` comes only after the walk-off.
        if !self.props.arrival_exempt.is_empty() && self.dialog.inline.is_none() {
            let reach: Vec<(i32, i32)> =
                std::iter::once((px as i32, pz as i32))
                    .chain(FIELD_ACTOR_PROBES.iter().flatten().map(|&(dx, dz)| {
                        (px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32)
                    }))
                    .collect();
            self.props.arrival_exempt.retain(|&(_, (wx, wz))| {
                reach.iter().any(|&(qx, qz)| {
                    (qx - wx as i32).abs() < FIELD_PROP_BOX_HALF
                        && (qz - wz as i32).abs() < FIELD_PROP_BOX_HALF
                })
            });
        }
        let exempt = &self.props.arrival_exempt;
        let hit = self
            .props
            .walk_touch
            .iter()
            .filter(|(s, _)| !exempt.iter().any(|(e, _)| e == *s))
            .find(|(_, ((wx, wz), _))| {
                points.iter().any(|&(qx, qz)| {
                    (qx - *wx as i32).abs() < FIELD_PROP_BOX_HALF
                        && (qz - *wz as i32).abs() < FIELD_PROP_BOX_HALF
                })
            })
            .map(|(&s, &(_, event))| (s, event));
        let Some((touch_slot, event)) = hit else {
            self.props.active_walk_touch = None;
            return;
        };
        if self.props.active_walk_touch == Some(touch_slot) {
            return; // still inside the same contact - already posted
        }
        self.props.active_walk_touch = Some(touch_slot);
        // A door record is a field-VM script, not a constant: its opening
        // `SysFlag.Test` chain picks which arm runs (teleport into the
        // interior vs. spawn the story beat). Retail resumes the record on
        // contact, so the arm is chosen against the *live* flags - re-resolve
        // here rather than reusing the load-time structural decode. Falls back
        // to that decode only when the record can't be re-walked: a record
        // walked to its idle loop with no teleport on the taken arm (the
        // `tower` rapid elevator while its switch `0x1C6` is off) moves
        // nobody - its static first `MoveTo` belongs to the other arm.
        // REF: FUN_801d5b5c (contact resumes the object's script)
        let resolved = self
            .props
            .walk_touch_records
            .get(&touch_slot)
            .copied()
            .and_then(|record| {
                let man = self.field_vm.channels_man.clone()?;
                let man_file = legaia_asset::man_section::parse(&man).ok()?;
                let flags = self.flags.system_flags.clone();
                let test = |idx: u16| -> bool {
                    let byte = usize::from(idx >> 3);
                    byte < flags.len() && flags[byte] & (0x80u8 >> (idx & 7)) != 0
                };
                crate::man_field_scripts::resolve_walk_touch_arm(&man_file, &man, record, &test)
            })
            .unwrap_or(Some(event));
        // Post through the same dispatch path the button-gated interact uses.
        self.trigger_field_interact(0, touch_slot);
        let Some(event) = resolved else {
            return;
        };
        match event {
            // A `Warp` event is, by construction, **only** ever a mode-24
            // minigame sub-id: `is_genuine_warp` gates the decoded `0x3E`'s
            // `op0` to `100..=106`, and every carrier of that id space is a
            // venue cabinet (see [`crate::minigame_entry`]).
            //
            // Body contact must NOT execute it. Retail's contact kernel
            // `FUN_801d5b5c` **resumes the placement's script**; it does not
            // shortcut to that script's terminal warp. A koin1 cabinet's
            // script is a coin compare into a confirm dialogue, and only the
            // taken arm reaches the `0x3E` - which is why brushing a slot
            // machine in retail costs nothing and opens nothing.
            //
            // Executing the *decoded* effect here instead skipped the compare
            // and the confirm, and made the whole casino floor a trap: the
            // contact box is +/-`FIELD_PROP_BOX_HALF` around the placement
            // and koin1's casino NPCs stand inside their neighbouring
            // cabinets' boxes, so walking up to an NPC to talk entered a
            // minigame. No shipped host has a player-reachable exit from one
            // (the browser play page does not even draw them), so the entry
            // read as a freeze: field stopped, BGM kept playing.
            //
            // The entry stays on the button probe
            // ([`Self::field_interact_probe_slot`]) - retail's actual
            // trigger - which runs the record and lets its own `0x3E` arm
            // ([`crate::world::vm_hosts`]) do the warp when the script gets
            // there. Contact still posts the interact above, which is the
            // script resume `FUN_801d5b5c` does model.
            WalkTouchEvent::Warp { .. } => {}
            WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            } => {
                // The retail op-0x23 player arm rewrites X/Z (`+0x14`/`+0x18`)
                // and re-seats the actor on the floor; the paired op-0x38
                // cross-context CAM_CFG writes the arrival heading (`+0x26`).
                // A door's interior is a *sub-area of the same collision grid*
                // at its own elevation, so the floor must be resampled at the
                // landing - otherwise the player keeps the doorstep's outdoor
                // height until the next locomotion frame nudges it.
                let y = self.sample_field_floor_height(world_x as i32, world_z as i32) as i16;
                if let Some(p) = self.player_actor_slot
                    && let Some(actor) = self.actors.get_mut(p as usize)
                {
                    actor.move_state.world_x = world_x;
                    actor.move_state.world_z = world_z;
                    actor.move_state.world_y = y;
                    if let Some(heading) = facing {
                        actor.move_state.render_26 = heading;
                    }
                }
                self.pending_field_events.push(FieldEvent::MoveTo {
                    world_x: world_x as u16,
                    world_z: world_z as u16,
                    is_player: true,
                });
                self.arm_arrival_bracket(touch_slot);
            }
            // Boss-stager contact: the `trigger_field_interact` call above
            // already ran the placement's record ([`crate::world::World::
            // run_boss_stager_record`]); the event carries no extra effect.
            WalkTouchEvent::StagerBeat => {}
            // The record's taken arm is an op-`0x44` SPAWN_RECORD: queue the
            // referenced record so `SceneHost::tick` installs it as a spawned
            // field-VM context (the same drain the in-script op-`0x44` uses).
            // This is the arm a story-gated door takes once its flag is set -
            // the in-house beat, not a bare reposition.
            WalkTouchEvent::SpawnRecord { flat_index } => {
                if let Ok(idx) = u8::try_from(flat_index) {
                    self.field_vm.pending_record_spawns.push(idx);
                }
            }
        }
    }

    /// Arm the arrival bracket for a teleport the walk-touch bind `slot`'s
    /// record just made: every bind whose record the teleporting record
    /// names in a cross-context `B1 <obj> 00`
    /// ([`crate::man_field_scripts::record_exempted_objects`]) is skipped by
    /// the touch dispatch and the collision probe until the player is off
    /// its contact box. The engine moves the player by the record's
    /// `MoveTo` alone (the ride's `A2 F8` walk-off clip is not run), so the
    /// player steps off the platform themselves; the bracket keeps that step
    /// from re-firing the partner, as retail's exemption does across the
    /// scripted walk-off.
    ///
    /// REF: FUN_801CF754, FUN_801CF9F4, FUN_801d5b5c
    pub(super) fn arm_arrival_bracket(&mut self, slot: u8) {
        let Some(&record) = self.props.walk_touch_records.get(&slot) else {
            return;
        };
        let Some(man) = self.field_vm.channels_man.clone() else {
            return;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            return;
        };
        let targets = crate::man_field_scripts::record_exempted_objects(&man_file, &man, record);
        // A ride that sets the player down inside another door's contact
        // box walks the player out of it before letting go (`balden`'s
        // elevator cars, P0[7] / P0[14]: `CC F8 51` runs the player to the
        // partner car, then `A2 F8 01` / `A2 F8 02` walk it out through the
        // partner's door, with no `B1` bracket). The engine runs neither
        // clip as motion, so a landing on the partner's centre would post
        // its touch on the first step in any direction and ride straight
        // back; the partner is exempt until the player has stepped off it,
        // as a bracketed one is.
        let landing = self
            .player_actor_slot
            .and_then(|p| self.actors.get(p as usize))
            .map(|a| {
                (
                    i32::from(a.move_state.world_x),
                    i32::from(a.move_state.world_z),
                )
            });
        let partners: Vec<(u8, (i16, i16))> = self
            .props
            .walk_touch_records
            .iter()
            .filter(|&(&s, _)| s != slot)
            .filter_map(|(&s, &r)| self.props.walk_touch.get(&s).map(|&(pos, _)| (s, r, pos)))
            .filter(|&(_, r, (wx, wz))| {
                targets.iter().any(|&t| usize::from(t) == r)
                    || landing.is_some_and(|(lx, lz)| {
                        (lx - i32::from(wx)).abs() < FIELD_PROP_BOX_HALF
                            && (lz - i32::from(wz)).abs() < FIELD_PROP_BOX_HALF
                    })
            })
            .map(|(s, _, pos)| (s, pos))
            .collect();
        for p in partners {
            if !self.props.arrival_exempt.contains(&p) {
                self.props.arrival_exempt.push(p);
            }
        }
    }

    /// The ride's walk-off, for an arrival the walls seal. A lift record's
    /// tail turns the player (`B8 F8 80 00`) and plays the walk-off clips
    /// (`A2 F8 01`, `A2 F8 02`) across the bracketed platform before its
    /// `B2`; the engine runs neither clip as motion, so a landing set in the
    /// wall niche behind a platform - `tower`'s rapid lift down arrives at
    /// (1856, 9280), walled on every side but its own lift - would hold the
    /// player for good. When every direction from the player reads wall and
    /// the ride's own record has finished, carry the player straight through
    /// the nearest bracketed platform to the first open spot past its reach,
    /// which is where the scripted walk-off leaves it.
    ///
    /// REF: FUN_801DE840 (cross-context `0x22` ExecMove into the player)
    pub(super) fn walk_off_sealed_arrival(&mut self, slot: usize) {
        let (px, pz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        let Some(&(_, platform)) = self
            .props
            .arrival_exempt
            .iter()
            .min_by_key(|&&(_, (x, z))| {
                (i32::from(x) - i32::from(px)).abs() + (i32::from(z) - i32::from(pz)).abs()
            })
        else {
            return;
        };
        let Some((x, z)) = self.sealed_arrival_walk_off((px, pz), platform) else {
            return;
        };
        let y = self.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = x;
        ms.world_z = z;
        ms.world_y = y;
    }

    /// Where [`Self::walk_off_sealed_arrival`] carries a player standing at
    /// `from` across the bracketed platform centred on `platform`: `None`
    /// when `from` has an open side (the player steps off by itself) or no
    /// open spot lies within 384 units past the platform's centre.
    pub fn sealed_arrival_walk_off(
        &self,
        from: (i16, i16),
        platform: (i16, i16),
    ) -> Option<(i16, i16)> {
        let (px, pz) = from;
        if !(0..4).all(|d| self.field_dir_blocked(px, pz, d)) {
            return None;
        }
        let (dx, dz) = (
            i32::from(platform.0) - i32::from(px),
            i32::from(platform.1) - i32::from(pz),
        );
        let len = dx.abs().max(dz.abs());
        if len == 0 {
            return None;
        }
        // Past the platform's centre, in 16-unit steps, to the first spot
        // the walls leave open on some side.
        (1..=24).find_map(|k| {
            let t = len + 16 * k;
            let x = i16::try_from(i32::from(px) + dx * t / len).ok()?;
            let z = i16::try_from(i32::from(pz) + dz * t / len).ok()?;
            (!self.field_tile_is_wall(x, z) && (0..4).any(|d| !self.field_dir_blocked(x, z, d)))
                .then_some((x, z))
        })
    }

    /// Whether a prop collider is under the arrival bracket
    /// ([`crate::world::FieldPropState::arrival_exempt`]): its static
    /// contact centre is a bracketed bind's.
    pub(crate) fn collider_arrival_exempt(&self, c: &FieldPropCollider) -> bool {
        !c.moving_box
            && self
                .props
                .arrival_exempt
                .iter()
                .any(|&(_, (x, z))| (i32::from(x), i32::from(z)) == c.center)
    }

    /// Post a player-NPC contact into the motion VM's one-slot touch mailbox
    /// (`DAT_80073F1C`), the sibling of [`Self::check_field_walk_touch`].
    ///
    /// Retail's producer is the actor box test `FUN_801CFC40`, which the
    /// per-axis collision `FUN_801CFE4C` calls three times per step: an
    /// overlap both refuses the step and tail-calls `FUN_8003D038` with the
    /// touched actor's `+0x50`. The consumer is the ambient motion VM's
    /// `0x05` wait arm (`FUN_80038158` at `0x8003882C`), which ends the wait
    /// on the frame the post names its own actor - so bumping into a town
    /// NPC breaks it out of its idle pause instead of leaving it parked.
    ///
    /// The `0x801C6470` arena the guard reads is assembled from the live
    /// channels' op-`0x17` records rather than from
    /// [`crate::world::FieldNpcState::default_moves`] (the static harvest): retail reads
    /// the arena, and a stream that has not run its `0x17` yet still holds
    /// the [`DEFAULT_MOVE_UNSET`](legaia_engine_vm::ambient_motion::DEFAULT_MOVE_UNSET)
    /// sentinel there, which suppresses the post.
    ///
    /// REF: FUN_801cfc40, FUN_8003d038, FUN_80038158
    pub(crate) fn post_ambient_motion_touch(&mut self) {
        if self.npcs.ambient.is_empty() {
            return;
        }
        let Some((px, pz)) = self.player_field_position() else {
            return;
        };
        // The same probe fan the NPC collision test walks
        // (`Self::field_npc_dir_blocked`), plus the stand-inside point.
        let mut points: Vec<(i32, i32)> = vec![(px as i32, pz as i32)];
        for dir in Self::dirs_of_bits(self.locomotion.last_move_dir_bits) {
            for &(dx, dz) in &FIELD_ACTOR_PROBES[dir] {
                points.push((px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32));
            }
        }
        let hit = self.npcs.ambient.keys().copied().find(|slot| {
            let Some(&(ax, az)) = self.npcs.positions.get(slot) else {
                return false;
            };
            points.iter().any(|&(qx, qz)| {
                (qx - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (qz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        });
        let Some(slot) = hit else { return };
        let stride = vm::motion_vm::BIND_RECORD_STRIDE;
        let slots = usize::from(*self.npcs.ambient.keys().next_back().unwrap_or(&0)) + 1;
        let mut arena = vec![vm::ambient_motion::DEFAULT_MOVE_UNSET; slots * stride];
        for (&s, chan) in &self.npcs.ambient {
            arena[usize::from(s) * stride] = chan.vm.default_move[0];
        }
        let Some(posted) = vm::motion_vm::post_touch(&arena, usize::from(slot)) else {
            return; // suppressed by the record's class byte
        };
        if let Some(chan) = self.npcs.ambient.get_mut(&slot) {
            chan.vm.pending_touch = Some(posted);
        }
    }

    /// Actor-VM glide start (`MotionAt` / `EffectMotion` → `start_motion`,
    /// retail `FUN_800358c0`): record the target on the actor and install a
    /// motion-VM leg gliding the actor's sprite position
    /// (`move_state.world_x` / `world_y`) toward it, stepped once per tick by
    /// [`Self::tick_actor_motions`]. The retail kernel writes the target into
    /// the actor `+0xA`/`+0xC` and its subobj mirrors and clears the `+0x20`
    /// glide cursor; the per-frame glide is the motion-VM pursue step.
    ///
    /// REF: FUN_800358c0
    pub(crate) fn start_actor_motion(&mut self, actor_id: u8, target: ActorVmPosition) {
        let Some(actor) = self.actors.get(actor_id as usize) else {
            return;
        };
        if !actor.active {
            return;
        }
        let (cx, cy) = (actor.move_state.world_x, actor.move_state.world_y);
        self.actors[actor_id as usize].motion_target = Some(target);
        self.move_vm.actor_motions.insert(
            actor_id,
            FieldNpcMotion {
                state: vm::motion_vm::MotionState {
                    world_x: cx,
                    // The sprite-actor glide runs in the actor VM's packed
                    // (x, y) plane; the motion VM's XZ pursue step maps
                    // y → z here.
                    world_z: cy,
                    speed: FIELD_NPC_MOTION_SPEED,
                    ..Default::default()
                },
                target: (target.x, target.y),
            },
        );
    }

    /// Step every actor-VM glide ([`Self::start_actor_motion`]) one frame
    /// through the motion VM, writing back into the actor's `move_state`.
    /// Finished or stale (despawned-actor) glides are dropped.
    ///
    /// REF: FUN_8003774C
    pub(crate) fn tick_actor_motions(&mut self) {
        if self.move_vm.actor_motions.is_empty() {
            return;
        }
        let slots: Vec<u8> = self.move_vm.actor_motions.keys().copied().collect();
        for slot in slots {
            let alive = self
                .actors
                .get(slot as usize)
                .is_some_and(|actor| actor.active);
            if !alive {
                self.move_vm.actor_motions.remove(&slot);
                continue;
            }
            let Some(motion) = self.move_vm.actor_motions.get_mut(&slot) else {
                continue;
            };
            let target = vm::motion_vm::MotionTarget {
                x: motion.target.0,
                y: 0,
                z: motion.target.1,
                id: 0,
            };
            let result = vm::motion_vm::step(&mut motion.state, target, &FIELD_NPC_MOTION_PROGRAM);
            let (nx, ny) = (motion.state.world_x, motion.state.world_z);
            let actor = &mut self.actors[slot as usize];
            actor.move_state.world_x = nx;
            actor.move_state.world_y = ny;
            if result == vm::motion_vm::StepResult::Done {
                self.move_vm.actor_motions.remove(&slot);
            }
        }
    }
}
