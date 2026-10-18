//! NPC facing: face an NPC, face it at the player or toward a point, the talk-
//! facing hold and release, and the scene-entry facing seed.

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
        self.field_channel_flags_and_8e(slot).0
    }

    /// [`Self::field_channel_flags`] with the context's `+0x8E`, the height
    /// the `0x20000000` arm of `FUN_8003BC08` mirrors into `+0x16`.
    pub fn field_channel_flags_and_8e(&self, slot: u8) -> (u32, i16) {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == slot as usize)
            .map_or((0, 0), |c| (c.ctx.flags, c.ctx.field_8e))
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
    /// Whether the outstanding talk-pose save belongs to a placement whose
    /// own context runs as the interaction timeline
    /// (`CutsceneTimeline::interaction_slot`, a boss stager): the dialog SM
    /// that snapped it is still engaged, so the restore waits for the
    /// record's `0x21`.
    pub fn interaction_timeline_holds_talk_facing(&self) -> bool {
        let Some((slot, _)) = self.npcs.facing_save else {
            return false;
        };
        self.cutscene
            .timeline
            .as_ref()
            .is_some_and(|tl| !tl.done && tl.interaction_slot == Some(slot))
    }

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
    /// ([`legaia_engine_field::man_field_scripts::placement_initial_facing`]). The engine
    /// derives the same LUT index statically per placement and stores the
    /// converted 12-bit engine heading (`0` = Z+;
    /// [`legaia_engine_field::man_field_scripts::facing_index_to_engine_heading`]) in
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
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
    ) {
        for p in man_file.actor_placements(man) {
            let Ok(slot) = u8::try_from(p.index) else {
                continue;
            };
            let Some(idx) =
                legaia_engine_field::man_field_scripts::placement_initial_facing(man_file, man, &p)
            else {
                continue;
            };
            let Some(heading) =
                legaia_engine_field::man_field_scripts::facing_index_to_engine_heading(idx)
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

    /// Turn a stationary field NPC to face world `(tx, tz)` - the retail
    /// "face the speaker" cinematic pose. This runs one shot of the ported
    /// motion VM's `0x4C` `FaceTarget` op (the yaw-rotate leg of
    /// `FUN_8003774C`, [`legaia_engine_motion_vm::motion_vm`]) seeded from the NPC's
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
        let mut state = legaia_engine_motion_vm::motion_vm::MotionState {
            world_x: cx,
            world_z: cz,
            // Budget of 1 (below) against this speed makes the FaceTarget leg
            // settle onto the exact bearing in a single step - the dialog
            // "snap to face the speaker" the retail engine performs on talk.
            speed: 0x0400,
            yaw: cur_yaw,
            ..Default::default()
        };
        let target = legaia_engine_motion_vm::motion_vm::MotionTarget {
            x: tx,
            y: 0,
            z: tz,
            id: 0,
        };
        // `0x4C` FaceTarget, sub-mode `0x85` (rotate yaw), budget `0x0001`,
        // target byte `0xF8` (self); no high bit -> the body starts at +1.
        const FACE_TARGET_PROGRAM: [u8; 5] = [0x4C, 0x85, 0x01, 0x00, 0xF8];
        let _ = legaia_engine_motion_vm::motion_vm::step(&mut state, target, &FACE_TARGET_PROGRAM);
        self.npcs.headings.insert(slot, (state.yaw & 0x0FFF) as i16);
    }
}
