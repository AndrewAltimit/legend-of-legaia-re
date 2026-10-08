//! Object render queries and the field-channel prologue runners.
//! Split out of `narration.rs`.

use super::*;

impl World {
    /// Step every live per-actor field-VM channel one frame slice.
    ///
    /// Mirrors the retail per-actor script ticker (`FUN_80039B7C`, the
    /// `+0x9C == 0` branch): each context runs ops until it yields, parks on
    /// a conditional hold, **or executes a `0x21` NOP** - the NOP is the
    /// per-frame pacing point in retail (`if (bVar1 == 0x21) break`), which is
    /// why placement idle loops are written `21 21 26 FE FF`. A parked channel
    /// (flag-test `Halt`) retries the same PC next frame; a cross-context poke
    /// (an extended op naming another channel's script id) resolves the target
    /// context and runs against it, parking the caller if the target is
    /// halted - the retail synchronisation primitive between the cutscene
    /// timeline and its vignette actors.
    ///
    /// Channels are seeded two ways. A cutscene timeline spawns the full set
    /// ([`Self::install_cutscene_timeline_record`]) so its cross-context pokes
    /// land on real per-actor contexts (the opening prologue's vignettes). On
    /// **ordinary free-roam scene entry** the scene loader seeds the same set
    /// ([`Self::seed_field_channels`], the non-cutscene half of retail's
    /// `FUN_8003AEB0` spawn loop) so each placement's own init opcodes run -
    /// scripted initial facings, idle/`WAIT`-loop cadence, local-flag setup.
    ///
    /// After stepping, each channel whose context position changed writes
    /// through to [`crate::world::FieldNpcState::positions`] so the field render / probes
    /// follow the scripted move.
    /// The render scale a placement channel's actor draws at - retail's
    /// `actor[+0x72]` fixed-point scalar (`0x1000` = 1.0), seeded by the
    /// allocator (`actor_free`) and rewritten by the spawn prologue's
    /// `4C 40` writes. The per-actor render dispatcher (`FUN_8001ADA4`)
    /// composes it into the GTE model matrix, so **scale 0 = invisible**:
    /// the invisible interaction-trigger records draw their dev-gizmo
    /// marker mesh at scale 0 in retail. Both hosts' NPC draw loops read
    /// this and skip zero-scale actors. `None` when no channel owns the
    /// placement (draw at unit scale).
    // REF: FUN_8001ADA4 (scale-vector compose, disasm 8001b240..8001b28c)
    // REF: FUN_80020de0 (actor_free: +0x72 = 0x1000 at birth)
    pub fn field_npc_render_scale(&self, placement_index: usize) -> Option<u16> {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == placement_index)
            .map(|c| c.ctx.field_72)
    }

    /// Flat partition-0 record indices whose **object-bind channel** is
    /// story-hidden after its spawn prologue ran: parked at the off-map hide
    /// box (`0x23 MoveTo 0x7F,0x7F` behind a `SysFlag.Test`, e.g. town01's
    /// gate rocks `P0[18..21]` until flag `0x147` sets) or zero render scale
    /// (`0x4C 0x40 0x00`). The placed-object draw resolvers consult this via
    /// [`crate::field_env::retain_visible_placed_draws`] - the `.MAP` table
    /// alone says where a placed object *can* stand, the bind record's
    /// prologue says whether it currently *does*.
    // REF: FUN_8003A55C (bind-time prologue pre-run seats/parks the actor)
    pub fn hidden_object_records(&self) -> std::collections::HashSet<usize> {
        let hide = crate::world::FIELD_OFFMAP_HIDE_XZ as u16;
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind)
            .filter(|c| (c.ctx.world_x == hide && c.ctx.world_z == hide) || c.ctx.field_72 == 0)
            .map(|c| c.placement_index)
            .collect()
    }

    /// Flat partition-0 record index -> render scale for every **object-bind
    /// channel** whose spawn prologue left `actor[+0x72]` at a non-zero,
    /// non-unit value. Retail's per-actor render dispatcher composes that word
    /// into the model matrix for every placed object it draws
    /// (`FUN_8001ADA4` case 5: `lhu v1,0x72(s0)` / `li v0,0x1000` /
    /// `beq v1,v0` at `0x8001B240`, else `ScaleMatrix` over `(s, s, s)` at
    /// `0x8001B288`), so a prop the prologue shrinks draws shrunk. town01's
    /// horizon backdrop (partition-0 record 26, a `17920 x 9600` plane at
    /// `(3264, 6744)`) is the case that needs it: its actor carries `0x400`
    /// in a retail capture, and at unit scale the plane stands between the
    /// plaza camera and the player. Zero-scale objects are
    /// [`Self::hidden_object_records`]' business and are not listed here.
    // REF: FUN_8001ADA4 (scale-vector compose, disasm 8001b240..8001b28c)
    pub fn object_render_scales(&self) -> std::collections::HashMap<usize, u16> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && c.ctx.field_72 != 0 && c.ctx.field_72 != 0x1000)
            .map(|c| (c.placement_index, c.ctx.field_72))
            .collect()
    }

    /// Flat partition-0 record indices whose **object-bind channel** carries
    /// the actor tick's floor-follow law: `+0x10 & 0x20200` up and
    /// `0x20000000` down. The field actor tick `FUN_8003BC08` rewrites such an
    /// actor's Y `+0x16` with the floor sample under it (`FUN_80019278`, the
    /// `jal` at `0x8003BC98`) on every tick the visibility cull leaves it in
    /// view, so the record's own `y_off` - the lift the `.MAP` sweep
    /// `FUN_8003A55C` seats it with (`lut[nibble] + y_off`, `0x8003A640`) -
    /// lasts only until the actor is first seen. A prologue raises the class
    /// bit with `31 11` (`CFlag.Set` bit 17): `rikuroa`'s `P0[0]`, bound to
    /// most of the summit's props, does, and its sky panorama (pack 37,
    /// `y_off` `2080`) draws at the `-480` floor tier in every retail capture,
    /// which puts the cliff ring across the top of the frame.
    ///
    /// The `0x2000` glide variant converges on the same floor and is listed
    /// too; the `0x20000000` law (`-(+0x8E)`) is
    /// [`Self::object_draw_displacements`]' business.
    // REF: FUN_8003BC08 (height arm, 0x8003BC44..0x8003BCF4), FUN_80019278
    pub fn object_floor_follow_records(&self) -> std::collections::HashSet<usize> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| {
                c.object_bind && c.ctx.flags & 0x2000_0000 == 0 && c.ctx.flags & 0x0002_0200 != 0
            })
            .map(|c| c.placement_index)
            .collect()
    }

    /// The draw Y a placed object bound to `record` takes at world `(x, z)`:
    /// the floor sample under it when the record follows the floor
    /// ([`Self::object_floor_follow_records`]), else `None` (the `.MAP`
    /// sweep's `lut[nibble] + y_off` stands).
    // REF: FUN_8003BC08, FUN_80019278
    pub fn object_floor_follow_y(
        &self,
        follow: &std::collections::HashSet<usize>,
        record: usize,
        x: i32,
        z: i32,
    ) -> Option<i32> {
        follow
            .contains(&record)
            .then(|| self.sample_field_floor_height(x, z))
    }

    /// Flat partition-0 record index -> how far a script has moved that
    /// **object-bind channel's** actor from where its bind seated it, as
    /// `[dx, dy, dz]` in retail world units (Y-down). Retail draws a placed
    /// object at its actor's live `+0x14 / +0x16 / +0x18`, not at the `.MAP`
    /// record, so a script that seats one with `A3 <id> <tx> <tz>` or lifts it
    /// with op `4C 42` (`+0x8E`, mirrored into world Y while `+0x10 &
    /// 0x20000000` is up) moves the drawn mesh. `chitei2`'s collapse is the
    /// case that needs it: partition-0 records 28..30 are the boulder, born
    /// 700 units up at a parking tile (`31 1D` + `4C 42 BC 02`), and the
    /// boulder beat (P2[17]) seats them at the foot of the escape stairs and
    /// ramps them down to the floor.
    ///
    /// The bind seats the actor at Y `0`, so `dy` is the actor's Y itself;
    /// hosts add the displacement to the placement's own transform. Parked
    /// objects ([`Self::hidden_object_records`]) and unmoved ones are not
    /// listed.
    // REF: FUN_8001ADA4 (case 5 draws at the actor position), FUN_8003A55C
    pub fn object_draw_displacements(&self) -> std::collections::HashMap<usize, [i32; 3]> {
        let hide = crate::world::FIELD_OFFMAP_HIDE_XZ as u16;
        // A record bound at several tiles seats its one channel at the first
        // bind (`spawn_object_channels` skips the repeats).
        let mut seeds: std::collections::HashMap<usize, (i16, i16)> = Default::default();
        for &(record, pos) in &self.field_vm.object_channel_binds {
            seeds.entry(record).or_insert(pos);
        }
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && !(c.ctx.world_x == hide && c.ctx.world_z == hide))
            .filter_map(|c| {
                let (sx, sz) = *seeds.get(&c.placement_index)?;
                let d = [
                    i32::from(c.ctx.world_x as i16) - i32::from(sx),
                    i32::from(c.ctx.world_y as i16),
                    i32::from(c.ctx.world_z as i16) - i32::from(sz),
                ];
                (d != [0, 0, 0]).then_some((c.placement_index, d))
            })
            .collect()
    }

    /// Retail's spawn-install prologue pre-run: `FUN_8003A1E4` runs each
    /// just-spawned placement context through the field VM at scene load, so
    /// the record's story-flag-tested opening ops execute BEFORE the first
    /// rendered frame - the `0x23 MoveTo` park to the off-map sentinel tile
    /// (`0x7F,0x7F`) for actors the current story state despawns, and the
    /// story-relocation `MoveTo`s that seat an actor away from its MAN header
    /// tile. Runtime-pinned against the retail town01 actor list: a share of
    /// the placements stand parked or relocated from frame one, positions the
    /// raw placement header does not carry.
    ///
    /// The engine mirror: run ONE frame slice per channel (the NOP-break
    /// slice of `FUN_80039B7C` - a spawn prologue is written `test / MoveTo / 21`-idle, so its repositioning
    /// lands in the first slice), *unconditionally* - this is load-time
    /// behaviour, not the opt-in free-roam liveliness - but only for a record
    /// whose first opcode is `0x24`/`0x25`, the install loop's own entry gate.
    /// Everything that slice runs is the record's **spawn** section (it stops
    /// at the section's raw `0x21`), so a story-flag write there is a write
    /// every MAN-loading entry performs - `kor5` `P1[2]`'s `SET 0x619` at
    /// `+0x1A` is one. A same-scene reload that does not re-load the MAN
    /// (`FUN_801D6704` passes `a0 = loader-mask & 4` to `FUN_8003AEB0`, which
    /// skips the partition-1 spawn loop at `0x8003B8A0` when it is zero) runs
    /// none of it. Position writes are
    /// surfaced for every repositioned slot, and a slot the prologue parks
    /// drops its glide pace and any in-flight leg. A `4C 51` seat also writes
    /// its LUT heading, on the arm the prologue actually took - over the
    /// first-nibble guess [`Self::seed_field_npc_facings`] made at load.
    ///
    /// Call at scene entry after the carrier/channel install; the resulting
    /// positions snapshot into [`crate::world::FieldNpcState::entry_positions`], the state
    /// a cutscene teardown restores to.
    ///
    /// This load-frame slice is the only time the engine steps a placement
    /// channel's own script. Retail's per-actor tick `FUN_8003BC08` runs a
    /// context (`FUN_80039B7C`) only while its `+0x10 & 0x100` is up, and
    /// the three writers of that bit are the touch post `FUN_801D5B5C`, the
    /// op-`0x44` record spawner `FUN_8003BDE0` (a new partition-2 context -
    /// the engine's helper contexts) and the system SM `FUN_801DA51C`. The
    /// spawn install `FUN_8003A1E4` raises `0x01020000`, not `0x100`, and a
    /// cross-context op runs on the caller's slice against the target's
    /// context without raising it. So a placement a cutscene pokes, walks or
    /// places does not run its talk body: `dolk2`'s Noa (`P1[2]`) and the
    /// `town01` opening's `P1[10]` / `P1[11]` stay parked after their spawn
    /// section. A touch runs the engaged context as the interaction timeline
    /// ([`crate::cutscene_timeline::CutsceneTimeline::interaction_slot`]).
    // PORT: FUN_8003A1E4 (spawn-prologue pre-run -> initial actor positions)
    // REF: FUN_8003AEB0, FUN_80039B7C, FUN_8003BC08
    pub fn pre_run_field_channel_prologues(&mut self) {
        self.field_vm.entry_prerun = true;
        self.step_field_channel_prologues();
        self.field_vm.entry_prerun = false;
        self.npcs.entry_positions = self.npcs.positions.clone();
        // The ambient motion channels installed with the carriers still hold
        // the raw MAN header tiles; re-seat them on the story-true positions
        // this pre-run just resolved. The `0x18` wander's containment box is
        // absolute world space, so a stale seat silently retires it.
        self.resync_ambient_start_positions();
        // ... and on the headings its seats wrote.
        self.resync_ambient_start_headings();
    }

    pub(super) fn step_field_channel_prologues(&mut self) {
        if self.field_vm.channels.is_empty() {
            return;
        }
        let Some(man) = self.field_vm.channels_man.clone() else {
            return;
        };
        let mut channels = std::mem::take(&mut self.field_vm.channels);
        // Host hooks resolve cross-context ids against the channel set while
        // one of these is executing; the live vector is moved out for the
        // borrow, so they read this copy (`World::channel_view`).
        self.field_vm.stepping_view = channels.clone();
        let pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        for i in 0..channels.len() {
            if channels[i].done {
                continue;
            }
            // Object-bind channels are cross-context poke targets only: the
            // engine drives their interaction bodies through the
            // touch/interact dispatch, not autonomous stepping (their
            // bind-time `0x24`/`0x25` prologue already ran at install,
            // mirroring `FUN_8003A55C`).
            if channels[i].object_bind {
                continue;
            }
            if man.len() <= channels[i].record_offset {
                channels[i].done = true;
                continue;
            }
            // A halted channel is SUSPENDED - the halt bit persists until an
            // explicit un-halt (the op-0x32 CFLAG_CLR bit-10 carve-out, the
            // one cross-context op allowed against a halted target). This is
            // the timeline's freeze/unfreeze choreography primitive: a
            // `4C 85` halt-acquire suspends the actor, `B3 <id> 0A` verifies
            // the suspension, `B2 <id> 0A` resumes it for its next beat.
            // (Autonomous frame pacing uses the `0x21` NOP break instead of
            // yields, so suspension never races normal idling.)
            if channels[i].ctx.is_halted() {
                continue;
            }
            // The load-frame slice is gated on the record's FIRST opcode:
            // `FUN_8003A1E4` enters its run loop only when the byte at the
            // freshly seated `+0x9E` is `0x24` or `0x25` (`addiu v0,v1,-0x24;
            // sltiu v0,v0,0x2` at `0x8003A480`), so a placement opening on
            // anything else executes nothing inside the load frame.
            if !matches!(
                man.get(channels[i].record_offset + channels[i].pc),
                Some(0x24 | 0x25)
            ) {
                continue;
            }
            let mut budget = FIELD_CHANNEL_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = channels[i].pc;
                let record_offset = channels[i].record_offset;
                let bc = &man[record_offset..];
                let Some(&opcode_byte) = bc.get(pc) else {
                    channels[i].done = true;
                    break;
                };
                // Cross-context poke: resolve the extended target to another
                // channel and run the op against that context.
                let ext = vm::field::peek_extended(bc, pc);
                let target = ext.and_then(|t| {
                    let ci = crate::field_channels::resolve_target(&channels, t)?;
                    (ci != i).then_some(ci)
                });
                let self_target = ext.is_some_and(|t| {
                    crate::field_channels::resolve_target(&channels, t) == Some(i)
                });
                self.field_vm.executing_object = target
                    .filter(|&ci| channels[ci].object_bind)
                    .map(|ci| channels[ci].ctx.script_id);
                self.field_vm.executing_channel = match target {
                    // Object-bind targets carry a flat record index, not a
                    // placement slot - no placement-keyed attribution.
                    Some(ci) if channels[ci].object_bind => None,
                    Some(ci) => Some(channels[ci].placement_index as u8),
                    // An extended op aimed at something that is not a
                    // channel (the player `0xF8`, the system `0xFB`) is not
                    // this placement's to receive.
                    None if ext.is_some() && !self_target => None,
                    None => Some(channels[i].placement_index as u8),
                };
                // A face-at acquire on a placement - the record's own actor
                // (`4C 85 <lo> <hi> <bind>`, `rikuroa`'s Noa turning toward
                // the placement she was seated beside) or another one
                // (`CC <id> 85 ..`): the target's walk kernel runs the leg.
                // It is armed here and stepped from the next field tick,
                // once the seats this pre-run writes have surfaced; the op
                // itself still runs below, so the pre-run's control flow -
                // an own-context acquire parks the section, as the record's
                // own halt does in retail - is unchanged.
                // REF: FUN_801DE840 (0x801E2148..0x801E21DC), FUN_8003774C (the 0x4C arm)
                if let Some(slot) = self.field_vm.executing_channel
                    && let Some(ramp) = crate::inline_dialogue::TalkFaceRamp::from_own_acquire(
                        bc, pc,
                    )
                    .or_else(|| {
                        crate::inline_dialogue::TalkFaceRamp::from_npc_acquire(bc, pc)
                            .filter(|_| target.is_some())
                            .map(|(_, r)| r)
                    })
                {
                    self.npcs.rotate_legs.remove(&slot);
                    self.npcs.face_legs.insert(slot, ramp);
                }
                let result = {
                    let mut host = FieldHostImpl { world: self };
                    match target {
                        Some(ci) => {
                            // Two disjoint &mut contexts out of the vec.
                            let (lo, hi) = channels.split_at_mut(i.max(ci));
                            let (target_ctx, caller_ctx) = if ci < i {
                                (&mut lo[ci].ctx, &mut hi[0].ctx)
                            } else {
                                (&mut hi[0].ctx, &mut lo[i].ctx)
                            };
                            let bc = &man[record_offset..];
                            vm::field::step_with_caller(
                                &mut host, target_ctx, caller_ctx, false, bc, pc,
                            )
                        }
                        None => {
                            let bc = &man[record_offset..];
                            field_step_routed(&mut host, &mut channels[i].ctx, bc, pc)
                        }
                    }
                };
                self.field_vm.executing_channel = None;
                self.field_vm.executing_object = None;
                match result {
                    FieldStepResult::Advance { next_pc } => {
                        let stalled = next_pc == pc;
                        channels[i].pc = next_pc;
                        // Retail frame-slice pacing: a NOP ends the slice
                        // after executing (FUN_80039B7C `bVar1 == 0x21`), and
                        // a non-advancing PC ends it defensively.
                        if opcode_byte & 0x7F == 0x21 || stalled {
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => {
                        channels[i].pc = resume_pc;
                        break;
                    }
                    FieldStepResult::Halt { final_pc } => {
                        // Parked (conditional hold / halted cross-target):
                        // retry the same PC next frame.
                        channels[i].pc = final_pc;
                        break;
                    }
                    FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                        // An op this port can't advance past inside a channel
                        // script: retire the channel (it stays resolvable as a
                        // cross-context target).
                        channels[i].pc = pc;
                        channels[i].done = true;
                        break;
                    }
                }
            }
        }
        // Write the spawn prologue's moves through to the field NPC
        // render/probe state, unconditionally: the executed branch is the
        // story truth for the slot's initial position. A park also drops the
        // slot's glide pace and any in-flight leg - a despawned actor has
        // nothing left to walk.
        for (c, pre) in channels.iter().zip(pre_pos) {
            if c.object_bind {
                // Flat-record-keyed context; the NPC surfaces are
                // placement-keyed.
                continue;
            }
            let (nx, nz) = (c.ctx.world_x, c.ctx.world_z);
            if (nx, nz) == pre {
                continue;
            }
            let slot = c.placement_index as u8;
            let (nx, nz) = (nx as i16, nz as i16);
            let hide = crate::world::FIELD_OFFMAP_HIDE_XZ;
            if (nx, nz) == (hide, hide) {
                self.npcs.glide_speeds.remove(&slot);
                self.npcs.motions.remove(&slot);
            }
            self.npcs.positions.insert(slot, (nx, nz));
        }
        self.field_vm.channels = channels;
        self.field_vm.stepping_view.clear();
    }

    /// Seed the per-actor field-VM channels for **ordinary free-roam** scene
    /// entry: one context per MAN partition-1 placement, exactly as a cutscene
    /// install seeds them, but without a timeline driving cross-context pokes.
    /// The scene loader calls this after the placement-derived carrier / NPC
    /// install so each placement's spawn section runs through
    /// [`Self::pre_run_field_channel_prologues`] - the non-cutscene half of
    /// retail's `FUN_8003AEB0` spawn loop.
    ///
    /// Cutscene scenes (`opdeene` and friends) re-seed the set through
    /// [`Self::install_cutscene_timeline_record`] afterwards, which simply
    /// replaces this set, so the two paths compose. A scene with no placements
    /// seeds an empty set and the pre-run no-ops - but the
    /// MAN is retained regardless: object-bind channels
    /// ([`Self::seed_object_channels`]) and walk-on partition-2 installs
    /// ([`Self::install_gated_p2_record`]) execute out of the same buffer, and
    /// retail binds objects (`FUN_8003A55C`) whether or not partition 1 placed
    /// any actors.
    // PORT: FUN_8003AEB0 (the per-record spawn loop, free-roam entry)
    // REF: FUN_8003A1E4 (per-record context spawn)
    pub fn seed_field_channels(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        self.field_vm.pending_engagements.clear();
        self.field_vm.channels = crate::field_channels::spawn_channels(man_file, man);
        self.field_vm.channels_man = if man.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(man.to_vec()))
        };
        // A new scene's binds are installed by `seed_object_channels` after
        // the trigger tables resolve; drop the previous scene's.
        self.field_vm.object_channel_binds.clear();
        self.npcs.anim_cues.clear();
        self.npcs.clip_current.clear();
        self.npcs.clip_cursors.clear();
        self.npcs.clip_rate_live.clear();
        self.npcs.clip_bones.clear();
    }

    /// Append the `.MAP` **object-bind** channels (retail scene-init
    /// `FUN_8003A55C`): one context per bound object, script id = the gate-0
    /// trigger's **flat** MAN record index (`actor[+0x50] = trigger[2]`).
    /// This is what makes partition-0 records resolvable cross-context
    /// targets through the `FUN_8003C83C` walk - the `town01` Mei walk-on
    /// beat pokes the Vahn's-house door object as channel `0x01`.
    ///
    /// Mirrors the retail bind-time prologue pre-run: a bound record whose
    /// first opcode is `0x24`/`0x25` runs through the field VM until it
    /// yields, stalls, or reaches a dialog byte (the `FUN_8003A55C` inline
    /// loop), so the object context carries its init state (the door-angle
    /// `4C 41` ramp seed) before the first poke.
    ///
    /// Binds are remembered on [`crate::world::FieldVmState::object_channel_binds`] so a
    /// cutscene-timeline install that has to respawn the channel set
    /// re-appends them. Call after [`Self::seed_field_channels`]; no-op when
    /// that seeded nothing (a scene without a MAN).
    // REF: FUN_8003A55C (object bind: +0x50 flat script id + prologue pre-run)
    pub fn seed_object_channels(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        binds: &[(usize, (i16, i16))],
    ) {
        self.field_vm.object_channel_binds = binds.to_vec();
        let same_man = self
            .field_vm
            .channels_man
            .as_deref()
            .is_some_and(|m| m.as_slice() == man);
        if !same_man {
            return;
        }
        let mut obj = crate::field_channels::spawn_object_channels(man_file, man, binds);
        self.pre_run_object_channel_prologues(&mut obj, man);
        self.field_vm.channels.extend(obj);
    }

    /// The `FUN_8003A55C` bind-time prologue pre-run: for each object-bind
    /// channel whose first opcode is `0x24`/`0x25`, step the field VM until
    /// the op was a `0x21` NOP, the PC stalls, a dialog byte is reached, or
    /// the context yields/halts - the retail inline loop's exact stop set.
    pub(super) fn pre_run_object_channel_prologues(
        &mut self,
        channels: &mut [crate::field_channels::FieldChannel],
        man: &[u8],
    ) {
        for c in channels.iter_mut() {
            let Some(bc) = man.get(c.record_offset..) else {
                c.done = true;
                continue;
            };
            let Some(&first) = bc.get(c.pc) else {
                c.done = true;
                continue;
            };
            if first != 0x24 && first != 0x25 {
                continue;
            }
            let mut budget = FIELD_CHANNEL_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = c.pc;
                let Some(&op) = bc.get(pc) else {
                    break;
                };
                if op & 0x7F < 0x20 {
                    // Dialog byte: the retail pre-run loop stops here.
                    break;
                }
                // The bind's own context is the one executing: an op-`0x4B`
                // here arms the bound object's morph lanes.
                self.field_vm.executing_object = Some(c.ctx.script_id);
                let result = {
                    let mut host = FieldHostImpl { world: self };
                    field_step_routed(&mut host, &mut c.ctx, bc, pc)
                };
                self.field_vm.executing_object = None;
                match result {
                    FieldStepResult::Advance { next_pc } => {
                        if next_pc == pc {
                            break;
                        }
                        c.pc = next_pc;
                        if op & 0x7F == 0x21 {
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => {
                        c.pc = resume_pc;
                        break;
                    }
                    FieldStepResult::Halt { final_pc } => {
                        c.pc = final_pc;
                        break;
                    }
                    FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                        c.pc = pc;
                        break;
                    }
                }
            }
        }
    }
}
