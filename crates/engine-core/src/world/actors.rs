//! Move-VM/actor-physics ticking, battle animation staging/commit/reactions, poses, party roster, battle/world-map entry, cutscene finish, and sprite requests.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Per-actor move-VM tick - clean port of `FUN_80021DF4` (lines
    /// `80022B94..80022BBC`).
    ///
    /// Two-phase: (1) pre-tick decrement the per-actor `wait_timer` by the
    /// global frame-time `delta`, (2) run the move VM through
    /// [`vm::move_vm::actor_tick`], which gates on the resulting timer and
    /// inspects the HALT flag after the call. Outcomes are recorded in
    /// [`World::move_outcomes`] so engines that want to react to per-actor
    /// halts / waits can read them after the world ticks.
    ///
    /// `delta` mirrors the retail product `_DAT_1f800393 * _DAT_1f80037D`
    /// (per-frame anim-speed scalars). Engines pass their own per-frame
    /// scalar; the default world tick uses `1` so a Wait of N consumes N
    /// frames.
    pub fn tick_move_vms_with_delta(&mut self, delta: u16) {
        self.move_outcomes.clear();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Pre-tick: decrement wait timer (retail does this unconditionally
            // before the gate).
            vm::move_vm::decrement_wait_timer(&mut self.actors[slot].move_state, delta);
            let outcome = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
            self.move_outcomes.push((slot as u8, outcome));
        }
    }

    /// Backwards-compatible wrapper using `delta = 1`.
    pub fn tick_move_vms(&mut self) {
        self.tick_move_vms_with_delta(1);
    }

    /// Per-actor physics tick - clean-room port driver for
    /// `engine-vm::actor_tick::tick_actor` (FUN_80021DF4). Runs
    /// [`vm::actor_tick::tick_actor`] once per active slot, then dispatches
    /// the emitted [`TickEvent`]s.
    ///
    /// At the moment the only event the engine reacts to is
    /// [`TickEvent::MoveVmKick`], which drives
    /// [`vm::move_buffer::cursor_advance`] against the actor's
    /// [`MoveBufferState`]. The cursor's record source is the per-scene
    /// MOVE pool installed via [`World::set_move_buffer_root`] (mirrors
    /// retail `_DAT_8007B888` / `_DAT_8007B840` / `_DAT_8007B75C`).
    ///
    /// The other event variants (audio cues, render submissions,
    /// unlink requests, keyframe pose writeback) are recorded in
    /// [`World::last_tick_events`] for engines that want to consume
    /// them but otherwise no-op. Wiring those is orthogonal to the
    /// move-buffer cursor.
    ///
    /// `frame_delta` matches the retail `DAT_1F800393` ramp scalar
    /// (idle = `1`). The default tick uses `1`.
    pub fn tick_actor_physics_with(&mut self, scalars: TickScalars, listener: &ListenerState) {
        self.last_tick_events.clear();
        let host = move_buffer_host::WorldMoveBufferView {
            move_buf: &self.move_buffer_root,
            move2_buf: &self.move2_buffer_root,
            alt_buf: &self.move_buffer_alt_root,
        };
        for (idx, actor) in self.actors.iter_mut().enumerate() {
            if !actor.active {
                continue;
            }
            let res = vm::actor_tick::tick_actor(&mut actor.physics, scalars, listener);
            if !res.events.is_empty() {
                // Drive the move-buffer cursor on any MoveVmKick event.
                let kicked = res
                    .events
                    .iter()
                    .any(|e| matches!(e, TickEvent::MoveVmKick));
                if kicked {
                    cursor_advance(&mut actor.move_buffer, &host, scalars.frame_delta);
                }
                self.last_tick_events.push((idx as u8, res));
            }
        }
    }

    /// Backwards-compatible wrapper using idle scalars and a default
    /// listener (no positional SFX integration yet).
    pub fn tick_actor_physics(&mut self) {
        let listener = ListenerState::unicast(0, 0, 0);
        self.tick_actor_physics_with(TickScalars::idle(), &listener);
    }

    /// Install the MOVE buffer pool root (retail `_DAT_8007B888`). The
    /// bytes are the MDT-shaped offset-table blob the scene-load path
    /// extracts from the slot-1 `Asset(0x05) = Move` descriptor. Pass
    /// an empty slice to clear it - the cursor's resolver will then
    /// return `None` for every requested id.
    pub fn set_move_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_buffer_root = bytes;
    }

    /// Install the MOVE2 buffer pool root (retail `_DAT_8007B840`).
    /// Selected when an actor's `cursor_requested` is `>= 0x400`.
    pub fn set_move2_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move2_buffer_root = bytes;
    }

    /// Install the alternate MOVE buffer pool root (retail
    /// `_DAT_8007B75C`). Selected when the actor's status flag word
    /// has [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] set.
    pub fn set_move_buffer_alt_root(&mut self, bytes: Vec<u8>) {
        self.move_buffer_alt_root = bytes;
    }

    /// Advance all active actor animations one frame. Mirrors the
    /// keyframe-table block in `FUN_80021DF4` (`0x80022ec4..0x80023040`)
    /// that walks `actor[+0x4C]` (anim pointer) when `actor[+0x22]`
    /// (factor) is non-zero. Called by [`World::tick`] after the move-VM
    /// pass.
    pub fn tick_actors(&mut self) {
        for actor in &mut self.actors {
            if !actor.active {
                continue;
            }
            if let Some(player) = &mut actor.active_animation {
                actor.pose_frame = Some(player.tick());
            }
        }
    }

    /// Advance the per-object battle animation of every actor carrying one,
    /// folding the result into `pose_frame`. The battle render path then
    /// deforms each actor's mesh through `tmd_to_vram_mesh_posed_rot`. Call once
    /// per battle frame (the field [`tick_actors`](Self::tick_actors) drives the
    /// ANM path instead). Unlike `tick_actors` this does not gate on `.active`,
    /// since battle-init actors keep their `tmd_binding` without the field
    /// `.active` flag.
    pub fn tick_battle_animations(&mut self) {
        // Commit any anim ids the SM staged this frame (idempotent - the
        // step_battle pre-step commit already handled last frame's stages).
        self.commit_staged_battle_anims();
        for i in 0..self.actors.len() {
            // Hit-reaction chaining first (the FUN_8004AD80 record-type-4 arm):
            // a finished knockdown re-stages the get-up entry while the actor
            // lives, and holds its final downed keyframe otherwise. Other
            // finished reactions fall through to the idle restore below.
            let reaction = {
                let a = &self.actors[i];
                match (a.battle_reaction, &a.battle_animation) {
                    (Some(key), Some(p)) if p.finished() => Some((key, a.battle.hp > 0)),
                    _ => None,
                }
            };
            match reaction {
                Some((4, true)) => {
                    // Knockdown finished on a living actor: play get-up (key 5).
                    if !self.queue_battle_reaction_key(i, 5) {
                        self.actors[i].battle_reaction = None;
                    }
                }
                Some((4, false)) => {
                    // Knockdown finished on a dead actor: hold the downed pose.
                }
                Some((_, _)) => {
                    // Flinch / get-up / block finished: resume idle.
                    self.actors[i].battle_reaction = None;
                }
                None => {}
            }
            // Staged-clip end - the engine's anim-end signal (retail: the
            // anim system's completion edge). Clear `ADVANCE_DONE` so the
            // attack chain's read gate opens for the next strike byte, and
            // converge the id pair back to idle `0` when the SM hasn't
            // staged a new id meanwhile; the idle restore below then
            // resumes the loop.
            let staged_done = {
                let a = &self.actors[i];
                match (a.battle_staged_anim, &a.battle_animation) {
                    (Some(id), Some(p)) if p.finished() => Some(id),
                    _ => None,
                }
            };
            if let Some(id) = staged_done {
                let a = &mut self.actors[i];
                a.battle_staged_anim = None;
                a.battle
                    .flag_bits
                    .clear(vm::battle_action::ActorFlags::ADVANCE_DONE);
                if a.battle.queued_anim == id {
                    a.battle.queued_anim = 0;
                    a.battle.current_anim = 0;
                }
            }
            // A finished one-shot action clip falls back to the idle loop -
            // except defeat, which holds its final (downed) keyframe.
            let restore_idle = {
                let a = &self.actors[i];
                a.battle_action_clips.is_some()
                    && a.battle_reaction.is_none()
                    && a.battle_pose != Some(vm::battle_action::Pose::Defeat as u8)
                    && matches!(&a.battle_animation, Some(p) if p.finished())
            };
            if restore_idle {
                self.apply_battle_pose(i, vm::battle_action::Pose::Idle as u8);
            }
            let actor = &mut self.actors[i];
            if let Some(player) = &mut actor.battle_animation {
                actor.pose_frame = Some(player.tick());
            }
        }
    }

    /// Commit every actor's staged battle anim id (`queued_anim` vs
    /// `current_anim`) through the retail anim-commit ladder. Engine port of
    /// the per-frame consumer that converges `+0x1D9` toward `+0x1DA`:
    ///
    /// - staged `0` converges and resumes the idle loop;
    /// - staged `q < 0x10` plays action-table entry `q` directly (the
    ///   equipment-spliced weapon swings live at `0xC..0xF`); `1` (the
    ///   walk/approach) loops, everything else plays one-shot;
    /// - staged `q >= 0x10` on an actor carrying an art bank materializes
    ///   bank record `q - 0x10` into dynamic slot `0x10`/`0x11` (ids `0x10`
    ///   and `0x1A` install at `0x11`) and **rewrites the staged id to the
    ///   slot number** - `legaia_engine_vm::anim_vm::resolve_staged_anim`;
    ///   without a bank (monsters) the id is a plain entry index;
    /// - an actor with no usable clip converges immediately and clears
    ///   `ADVANCE_DONE` (a zero-length swing), so clip-less hosts keep the
    ///   pre-animation pacing.
    ///
    /// Idempotent per frame (a converged pair is a no-op). Called by
    /// [`Self::step_battle`] (pre-step) and [`Self::tick_battle_animations`].
    // PORT: FUN_8004AD80 (staged-anim commit; the id -> slot/record ladder
    // lives in `legaia_engine_vm::anim_vm::resolve_staged_anim`).
    pub fn commit_staged_battle_anims(&mut self) {
        for i in 0..self.actors.len() {
            self.commit_staged_battle_anim(i);
        }
    }

    /// Single-actor arm of [`Self::commit_staged_battle_anims`]. Public so
    /// tests can drive one slot deterministically.
    pub fn commit_staged_battle_anim(&mut self, i: usize) {
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        use vm::battle_action::ActorFlags;
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        let q = actor.battle.queued_anim;
        if q == actor.battle.current_anim {
            return;
        }
        // Staged idle: converge and resume the loop. A staged clip in
        // flight is dropped (retail: the commit replaces the playing
        // record unconditionally).
        if q == 0 {
            let a = &mut self.actors[i];
            a.battle.current_anim = 0;
            a.battle_staged_anim = None;
            self.apply_battle_pose(i, vm::battle_action::Pose::Idle as u8);
            return;
        }
        // Resolve the clip + the committed id (post-rewrite).
        let (clip, committed) = match resolve_staged_anim(q) {
            StagedAnimTarget::ArtBank { record, slot } if actor.battle_art_bank.is_some() => {
                let clip = actor
                    .battle_art_bank
                    .as_ref()
                    .and_then(|b| b.get(record as usize))
                    .and_then(|c| c.clone());
                (clip, slot)
            }
            // Direct entries - and, for an actor without an art bank (a
            // monster), ids >= 0x10 too: monster anim ids are archive entry
            // indices across the whole range.
            _ => {
                let clip = actor
                    .battle_action_clips
                    .as_ref()
                    .and_then(|cl| cl.get(q as usize))
                    .and_then(|c| c.clone());
                (clip, q)
            }
        };
        let a = &mut self.actors[i];
        // The FUN_8004AD80 rewrite: both id fields hold the committed slot
        // number, so the SM's equality checks compare post-rewrite values.
        a.battle.queued_anim = committed;
        a.battle.current_anim = committed;
        // An in-flight hit reaction owns the player (same precedence as the
        // pose hook); the ids still converge above so the SM doesn't stall,
        // and the clip is treated as elapsed.
        if a.battle_reaction.is_some() {
            a.battle.flag_bits.clear(ActorFlags::ADVANCE_DONE);
            return;
        }
        // Id 1 is the walk/approach: it loops until the SM stages something
        // else (AttackShortStep clears it to 0 on arrival). Engine
        // assumption - the loop-vs-once bit retail derives from the record
        // kind isn't modelled on MonsterAnimation.
        let player = clip.as_ref().and_then(|c| {
            if committed == 1 {
                crate::battle_anim::MonsterAnimPlayer::new(c)
            } else {
                crate::battle_anim::MonsterAnimPlayer::new_one_shot(c)
            }
        });
        match player {
            Some(p) => {
                a.battle_animation = Some(p);
                a.battle_pose = None;
                // The marker keeps the SM's per-frame pose() requests from
                // stealing the player. A looping walk never finishes, so its
                // marker is released by the next staged id (AttackShortStep
                // clears the queue to 0 on arrival).
                a.battle_staged_anim = Some(committed);
            }
            None => {
                // No usable clip: a zero-length swing - fire the anim-end
                // signal immediately so the attack chain's read gate opens.
                a.battle.flag_bits.clear(ActorFlags::ADVANCE_DONE);
            }
        }
    }

    /// Queue the retail hit reaction on a damaged battle actor, mirroring the
    /// damage primitive `FUN_800402F4`: a surviving target with no get-up
    /// entry (action tag `5`) plays the light flinch (tag `2`, then straight
    /// back to idle); any other hit plays the knockdown (tag `4`), whose
    /// end-of-clip chain ([`Self::tick_battle_animations`], the
    /// `FUN_8004AD80` record-type-4 arm) re-stages the get-up while the actor
    /// lives and holds the downed keyframe when it dies. No-op for actors
    /// without installed action clips (or without the needed entries).
    // PORT: FUN_800402F4 (damage-arm reaction staging: `+0x1DA = +0x1EF` for
    // a surviving no-get-up target, else `+0x1DA = +0x1F1`; the `+0x1EF..
    // +0x1F3` tag->entry map is built by FUN_80054CB0 / FUN_80053CB8).
    pub fn queue_battle_reaction(&mut self, slot: usize, survives: bool) {
        let has_getup = self
            .battle_reaction_clip(slot, 5)
            .map(|c| c.frame_count > 0)
            .unwrap_or(false);
        let key = if survives && !has_getup { 2 } else { 4 };
        self.queue_battle_reaction_key(slot, key);
    }

    /// Look up actor `slot`'s action clip carrying action tag `key` (the
    /// retail `+0x1EF` map: tag -> entry, with the loader's tag-4 -> tag-2
    /// fallback applied by the caller). Player files store the reaction
    /// family identity-ordered; monster archives at arbitrary indices - so
    /// the lookup is by each clip's `action_id`, exactly like
    /// `FUN_80054CB0`'s first-byte scan.
    fn battle_reaction_clip(&self, slot: usize, key: u8) -> Option<MonsterAnimation> {
        let clips = self.actors.get(slot)?.battle_action_clips.as_ref()?;
        clips.iter().flatten().find(|c| c.action_id == key).cloned()
    }

    /// Start the reaction clip for `key` on actor `slot` (one-shot). Applies
    /// the retail tag-4 -> tag-2 fallback (`FUN_80054CB0` seeds `+0x1F1` from
    /// `+0x1EF` when no tag-4 entry exists). Returns `false` when no usable
    /// clip exists.
    fn queue_battle_reaction_key(&mut self, slot: usize, key: u8) -> bool {
        let clip = self.battle_reaction_clip(slot, key).or_else(|| {
            (key == 4)
                .then(|| self.battle_reaction_clip(slot, 2))
                .flatten()
        });
        let Some(clip) = clip else {
            return false;
        };
        let Some(player) = crate::battle_anim::MonsterAnimPlayer::new_one_shot(&clip) else {
            return false;
        };
        let Some(actor) = self.actors.get_mut(slot) else {
            return false;
        };
        actor.battle_animation = Some(player);
        actor.battle_reaction = Some(key);
        actor.battle_pose = None;
        true
    }

    /// Install the per-slot battle action clips for actor `slot` (see
    /// [`Actor::battle_action_clips`]). The battle-action SM's `pose()` host
    /// hook then switches `battle_animation` between the idle loop and the
    /// matching action clip. No-ops for out-of-range slots.
    pub fn set_actor_battle_action_clips(
        &mut self,
        slot: usize,
        clips: std::sync::Arc<Vec<Option<MonsterAnimation>>>,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_action_clips = Some(clips);
            actor.battle_pose = None;
            actor.battle_staged_anim = None;
        }
    }

    /// Install the per-character art-animation bank clips for actor `slot`
    /// (see [`Actor::battle_art_bank`]): index = bank record, content = the
    /// record's `"ME"`-archive keyframe stream expanded per assembled
    /// object. The staged-anim commit resolves ids `>= 0x10` through this
    /// bank exactly like retail `FUN_8004AD80`. No-ops for out-of-range
    /// slots.
    pub fn set_actor_battle_art_bank(
        &mut self,
        slot: usize,
        bank: std::sync::Arc<Vec<Option<MonsterAnimation>>>,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_art_bank = Some(bank);
        }
    }

    /// Switch actor `slot`'s battle animation for a battle-action SM pose
    /// request (the retail `FUN_801D5854(actor, pose_id)` call).
    ///
    /// Pose id → action-stream slot is an explicit engine interpretation
    /// grounded in the player files' slot census: the SM's pose-id space is
    /// `6` idle / `7` ready / `8` recover / `9` defeat, and in every player
    /// battle file slot 6 is EMPTY while slots 7/8/9 are populated (Terra,
    /// who barely fights, lacks exactly 7/8) and slot 0 is the proven idle
    /// loop. So: pose 6 plays slot 0 as a loop; poses 7/8/9 play their
    /// same-numbered slot as a one-shot (defeat holds its last frame via
    /// [`Self::tick_battle_animations`]); a missing slot falls back to idle.
    /// Re-requesting the actor's current pose keeps the playing clip.
    // REF: FUN_801D5854 - the SM's pose dispatch this hook answers; the
    // id->slot mapping is an engine interpretation, not a port of its body.
    pub fn apply_battle_pose(&mut self, slot: usize, pose_id: u8) {
        let Some(actor) = self.actors.get_mut(slot) else {
            return;
        };
        let Some(clips) = actor.battle_action_clips.clone() else {
            return;
        };
        // An in-flight hit reaction outranks the SM's per-frame pose calls
        // (retail's pose driver never touches the anim-id fields; the
        // reaction chain owns them until it completes).
        if actor.battle_reaction.is_some() {
            return;
        }
        // Same precedence for a staged one-shot (weapon swing / art clip):
        // the SM keeps calling `pose()` every step while the swing plays
        // (idle during the wait states, recover at the band end) - the
        // staged clip owns the player until it finishes
        // (`tick_battle_animations` clears the marker).
        if actor.battle_staged_anim.is_some() {
            return;
        }
        // Monster clip vectors are archive-order (retail resolves monster
        // actions by first-byte search, not by pose id), so only the idle
        // request maps for monster slots; party tables are identity-ordered
        // and accept the full pose set.
        if slot >= 3 && pose_id != vm::battle_action::Pose::Idle as u8 {
            return;
        }
        if actor.battle_pose == Some(pose_id) {
            return;
        }
        let idle_pose = vm::battle_action::Pose::Idle as u8;
        let clip_slot = if pose_id == idle_pose {
            0
        } else {
            pose_id as usize
        };
        let player = match clips.get(clip_slot).and_then(|c| c.as_ref()) {
            Some(clip) if clip_slot != 0 => {
                crate::battle_anim::MonsterAnimPlayer::new_one_shot(clip)
            }
            _ => clips
                .first()
                .and_then(|c| c.as_ref())
                .and_then(crate::battle_anim::MonsterAnimPlayer::new),
        };
        if let Some(player) = player {
            actor.battle_animation = Some(player);
            actor.battle_pose = Some(pose_id);
        }
    }

    /// Bind a battle animation player to actor `slot`, resetting its
    /// `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_battle_animation(
        &mut self,
        slot: usize,
        player: crate::battle_anim::MonsterAnimPlayer,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_animation = Some(player);
            actor.pose_frame = None;
        }
    }

    /// Bind an animation player to actor `slot`. Replaces any existing
    /// player and resets the playhead. No-ops for out-of-range slots.
    pub fn set_actor_animation(&mut self, slot: usize, player: AnimPlayer) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.active_animation = Some(player);
            actor.pose_frame = None;
        }
    }

    /// Bind actor `slot` to TMD index `tmd_idx` in `SceneResources::tmds`.
    /// Renderers use this binding to look up the right mesh when applying
    /// the actor's `pose_frame`. No-ops for out-of-range slots.
    pub fn set_actor_tmd_binding(&mut self, slot: usize, tmd_idx: usize) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.tmd_binding = Some(tmd_idx);
        }
    }

    /// Install the field player's idle/walk clip pair (built by the host from
    /// the PROT 0874 §1 locomotion bundle -
    /// [`crate::field_anim::FieldPlayerAnim`]). The field tick advances it
    /// after the locomotion step; `None` (the default) leaves the player on
    /// the static rest pose.
    pub fn set_field_player_anim(&mut self, anim: Option<crate::field_anim::FieldPlayerAnim>) {
        self.field_player_anim = anim;
    }

    /// One field-frame advance of the player's locomotion animation: pick
    /// idle vs walk off the movement flag the locomotion step just set, emit
    /// the active clip's frame into the player actor's `pose_frame`. Called
    /// by [`World::tick`]'s field branch right after
    /// [`World::step_field_locomotion`].
    pub(crate) fn tick_field_player_anim(&mut self) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let Some(anim) = &mut self.field_player_anim else {
            return;
        };
        let pose = anim.tick();
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.pose_frame = Some(pose);
        }
    }

    /// Run [`vm::move_vm::actor_tick`] for `slot` against the given `bytecode`
    /// with the supplied opcode `budget`. Returns the typed outcome -
    /// engines route `Halted` to their halt-handler, `EndOfBuffer` to "clear
    /// the move", `Pending` to a debug log.
    pub fn actor_tick_at(
        &mut self,
        slot: usize,
        bytecode: &[u16],
        budget: usize,
    ) -> vm::move_vm::ActorTickOutcome {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
        };
        let actor_state = unsafe {
            // SAFETY: same disjoint-field justification as `step_move_vm`.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let outcome = vm::move_vm::actor_tick(&mut host, actor_state, bytecode, budget);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        outcome
    }

    /// Resolve a battle/party ordinal (actor slot, HUD row, VRAM texture
    /// band) to the **roster slot** of the character occupying it, per
    /// [`Self::active_party`]. Identity when no composition is installed
    /// or the ordinal runs past it - the historical slot-`i`-is-character-`i`
    /// behaviour every synthetic test relies on.
    pub fn party_roster_slot(&self, member: usize) -> usize {
        self.active_party
            .get(member)
            .map(|&s| s as usize)
            .unwrap_or(member)
    }

    /// Install a present-party composition: `slots[i]` = roster slot for
    /// battle ordinal `i` (the engine mirror of retail's present-party
    /// list at `0x8007BD10`). The list caps at the 3 on-screen party
    /// positions (the runtime texture-band count). Sets
    /// [`Self::party_count`] to the resulting length and, for each ordinal
    /// whose mapped roster record exists, reseeds the party actor's HP /
    /// MP / liveness / SPD mirror from it - the same projection
    /// [`Self::load_party`] performs for the identity mapping. Ordinals
    /// past the roster keep their live mirrors (zeroed-roster / synthetic
    /// setups render the character with default equipment, exactly like
    /// the identity default).
    pub fn set_active_party(&mut self, slots: Vec<u8>) {
        let mut active = slots;
        active.truncate(3);
        for (member, &rslot) in active.iter().enumerate() {
            let Some(rec) = self.roster.members.get(rslot as usize) else {
                continue;
            };
            let hms = rec.hp_mp_sp();
            if let Some(a) = self.actors.get_mut(member) {
                a.active = true;
                a.battle.hp = hms.hp_cur;
                a.battle.max_hp = hms.hp_max;
                a.battle.mp = hms.mp_cur;
                a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            }
            if let Some(s) = self.battle_speed.get_mut(member) {
                *s = rec.live_stats().spd;
            }
        }
        if !active.is_empty() {
            self.party_count = active.len() as u8;
        }
        self.active_party = active;
    }

    /// Place the world into [`SceneMode::Battle`] and populate the actor
    /// pointer table with `party_count` party slots followed by
    /// `monster_count` monster slots, mirroring the layout
    /// `FUN_800520F0` produces (slots 0..2 = party, 3..7 = monsters; total
    /// caps at 8). Actors are seated at the retail stage seats
    /// ([`crate::battle_seats`]): the party at negative Z facing the
    /// monsters at positive Z, both rows selected by combatant count
    /// exactly like the setup `FUN_800513F0`.
    ///
    /// This is the engine-core analogue of the retail battle scene
    /// loader's "stamp the actor table from the scene record" pre-pass.
    /// Engines that drive the loader from real scene data (party data +
    /// monster archive) skip this helper and write the slots directly;
    /// it's the convenience path for tests + the asset-viewer's
    /// `battle-scene` subcommand.
    ///
    /// The battle-action state machine is seeded at
    /// [`legaia_engine_vm::battle_action::ActionState::Begin`].
    // PORT: FUN_800513F0 (battle setup: seat stamping from the SCUS tables)
    pub fn enter_battle(&mut self, party_count: u8, monster_count: u8) {
        self.mode = SceneMode::Battle;
        self.party_count = party_count.min(3);
        let monster_count = monster_count.min(5);
        let actor_count = ((self.party_count as usize) + (monster_count as usize)).min(MAX_ACTORS);
        for i in 0..(self.party_count as usize).min(actor_count) {
            let s = crate::battle_seats::party_seat(self.party_count, i);
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = s.x;
            actor.move_state.world_y = s.y;
            actor.move_state.world_z = s.z;
            actor.battle.liveness = 1;
        }
        for i in (self.party_count as usize)..actor_count {
            let s = crate::battle_seats::monster_seat(
                monster_count,
                i - self.party_count as usize,
                false,
            );
            let actor = self.spawn_actor(i);
            actor.move_state.world_x = s.x;
            actor.move_state.world_y = s.y;
            actor.move_state.world_z = s.z;
            actor.battle.liveness = 1;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        self.battle_end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch).
        self.effect_pool = vm::effect_vm::Pool::new();
    }

    /// Place the world into [`SceneMode::WorldMap`] and install a
    /// [`WorldMapController`] if one isn't already present. After this,
    /// [`World::tick`] drives the controller from the per-frame pad set
    /// via [`World::set_pad`] - scroll, azimuth, zoom, and the top-view
    /// debug toggle all respond to input through the engine tick rather
    /// than a host-side controller.
    ///
    /// Idempotent: re-entering world-map mode keeps the existing
    /// controller (and its accumulated camera state) instead of resetting
    /// it.
    pub fn enter_world_map(&mut self) {
        self.mode = SceneMode::WorldMap;
        if self.world_map_ctrl.is_none() {
            self.world_map_ctrl = Some(WorldMapController::new());
        }
    }

    /// Consume a pending field-VM FMV trigger and flip into the cutscene
    /// mode, mirroring retail's main mode dispatcher reading the
    /// next-game-mode global (`_DAT_8007B83C == 0x1A`, game mode 26) one
    /// frame after the field-VM op `0x4C 0xE2` writes it.
    ///
    /// Only fires from [`SceneMode::Field`] (the only mode that runs the
    /// field VM and so the only one that can set the trigger). The pending
    /// id is always drained; an id whose runtime FMV slot points at a
    /// cut/missing path ([`crate::cutscene::fmv_index_to_str_filename`]
    /// returns `None`) is a no-op transition - the field continues - which
    /// matches the engine's documented "treat a cut slot as a no-op" rule.
    pub(crate) fn maybe_enter_pending_cutscene(&mut self) {
        let Some(fmv_id) = self.pending_fmv_trigger.take() else {
            return;
        };
        if self.mode != SceneMode::Field {
            return;
        }
        if crate::cutscene::fmv_index_to_str_filename(fmv_id).is_some() {
            self.cutscene_return_mode = Some(self.mode);
            self.mode = SceneMode::Cutscene;
            self.active_fmv = Some(fmv_id);
        }
    }

    /// The FMV index currently playing in [`SceneMode::Cutscene`], or `None`
    /// when no STR FMV is active. Hosts poll this after [`World::tick`] to
    /// learn which `MV*.STR` to open.
    pub fn active_fmv(&self) -> Option<i16> {
        self.active_fmv
    }

    /// The retail `MV*.STR` path of the active cutscene FMV, or `None` when
    /// no STR FMV is active. Convenience over
    /// [`crate::cutscene::fmv_index_to_str_filename`].
    pub fn active_fmv_str_filename(&self) -> Option<&'static str> {
        self.active_fmv
            .and_then(crate::cutscene::fmv_index_to_str_filename)
    }

    /// End the active STR-FMV cutscene and return to the scene mode that was
    /// live when it started (the field, in the normal flow). Retail returns
    /// here when the cutscene/MDEC overlay finishes playback and unloads.
    ///
    /// The field VM resumes from where it paused - its program counter is
    /// already past the FMV op, so the next field tick continues the script.
    /// A no-op when no cutscene is active.
    pub fn finish_cutscene(&mut self) {
        if self.mode == SceneMode::Cutscene {
            self.mode = self.cutscene_return_mode.take().unwrap_or(SceneMode::Field);
            self.active_fmv = None;
        }
    }

    /// Build the per-frame sprite list for the renderer. One
    /// [`ActorSpriteRequest`] per active actor with a [`SpriteFrame`] set;
    /// the screen-space coordinates are derived from the actor's
    /// `move_state.world_x` / `move_state.world_z` (PSX field coords) by
    /// flattening to a top-down `(x, z)` view and adding the sprite's
    /// `anchor_y`. Engines that have a real camera projection pre-process
    /// the move_state coords before populating [`Actor::sprite_frame`] (or
    /// override this helper).
    ///
    /// Mirrors the retail `FUN_80021DF4` per-frame actor tick's "draw
    /// sprite at world position" pre-pass - the actual GPU upload happens
    /// in `legaia_engine_render` against the supplied atlas.
    pub fn collect_sprite_requests(&self) -> Vec<ActorSpriteRequest> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(slot, a)| {
                if !a.active {
                    return None;
                }
                let frame = a.sprite_frame?;
                let world_x = a.move_state.world_x as i32;
                let world_y = a.move_state.world_z as i32 + frame.anchor_y as i32;
                Some(ActorSpriteRequest {
                    actor_slot: slot as u8,
                    world_x,
                    world_y,
                    atlas_src: frame.atlas_src,
                    tint: frame.tint,
                })
            })
            .collect()
    }

    /// Set the sprite frame for the actor at `slot`. Idempotent - passing
    /// `None` removes the frame so the actor stops rendering as a sprite.
    pub fn set_actor_sprite(&mut self, slot: u8, frame: Option<SpriteFrame>) {
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.sprite_frame = frame;
        }
    }
}
