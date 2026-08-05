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
    /// This loop is the engine's form of the retail actor-list iterator
    /// `FUN_8002519C` - the walker `FUN_80016444` runs over each of the
    /// five `_DAT_8007C34C..0x36C` list heads: per node it either `jalr`s
    /// the node's own tick fn (`node[+0x0C]`) or, for standard actors whose
    /// fn is `FUN_80021DF4`, runs the inline physics tick, with flag bit
    /// `0x200` as the "already ticked this frame" dedupe. The engine keeps
    /// one pool with an `active` flag instead of five lists, and the
    /// special-fn nodes are the dedicated ticks `World::tick` sequences
    /// around this loop, so the dedupe bit has no counterpart. Node layout
    /// and observed tick fns: `docs/subsystems/world-map.md`
    /// ("per-frame render-pass iterator").
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
    // PORT: FUN_8002519c (list-walk tick dispatch; pool-not-lists divergence
    //                     documented above)
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

    /// Default-listener wrapper (no positional SFX integration yet) carrying
    /// the **live cadence** into the dispatcher scalars.
    ///
    /// `frame_delta` is retail's `DAT_1F800393` - the vsyncs one game tick
    /// spans - not a constant `1`. [`World::tick`] fires this once every
    /// [`World::frame_step`] vsyncs, so the two together conserve
    /// vsyncs-per-second: the dispatcher integrates the same total delta over
    /// the same wall-clock span, just in fewer, larger steps. That is exactly
    /// retail's own trade, and it is why duration-based parity (the camera
    /// mover, every `t = min(t + dt, d)` accumulator) is untouched by it.
    ///
    /// REF: FUN_80016B6C
    pub fn tick_actor_physics(&mut self) {
        let listener = ListenerState::unicast(0, 0, 0);
        let cadence = vm::actor_tick::FrameCadence::from_raw(self.frame_step);
        self.tick_actor_physics_with(TickScalars::for_cadence(cadence, 1), &listener);
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
            let frame = if let Some(player) = &mut actor.battle_animation {
                let before = player.current_frame();
                // Retail rate law (`FUN_80047430`): the cursor advance
                // scales by the per-actor anim-rate byte `+0x21D` - the
                // arts slow-motion channel - and the idle branch runs at
                // half the action-clip shift (`>> 2` vs `>> 1`).
                let rate = actor.battle.anim_rate;
                let idle = actor.battle.current_anim == 0 && actor.battle_reaction.is_none();
                let pose = player.tick_rated(rate, idle);
                let after = player.current_frame();
                let clip_tag = player.action_id();
                // History-ring push (retail `FUN_80047430`
                // `0x80047E58..0x80048060`): slot 0 takes this frame's pose
                // + position; the arts after-image walk samples it. The
                // ring-id gate: party ghosts only on the committed dynamic
                // slot `0x11` (the Super / Miracle SpecialStarter dash);
                // monster ring ids are `clip_tag + 0x10`, so any non-idle
                // tag is eligible.
                let ghost_eligible = if i < 3 {
                    actor.battle.current_anim == vm::anim_vm::DYNAMIC_ART_SLOT_B
                } else {
                    clip_tag != 0
                };
                actor.battle_pose_history.push_front(BattleGhostFrame {
                    pose: pose.clone(),
                    pos: [
                        i32::from(actor.move_state.world_x),
                        i32::from(actor.move_state.world_y),
                        i32::from(actor.move_state.world_z),
                    ],
                    ghost_eligible,
                });
                actor
                    .battle_pose_history
                    .truncate(crate::battle_afterimage::HISTORY_DEPTH);
                actor.pose_frame = Some(pose);
                if after < before {
                    // Looping clip wrapped: refire the effect script next
                    // cycle (engine cadence choice - see the cursor's docs).
                    actor.battle_effect_cursor = 0;
                }
                Some(after)
            } else {
                None
            };
            // Per-frame effect-script walk for the committed record - the
            // engine seat of retail's `FUN_80047430` -> `FUN_801DEA50` call
            // pair (frame argument = the node's 12.4 anim cursor in whole
            // keyframes, which is `MonsterAnimPlayer::current_frame`).
            if let Some(frame) = frame {
                self.step_actor_effect_script(i, frame);
            }
        }
        // The move-FX streak counter walk (retail `FUN_801E09F8` phase 1):
        // `ctx[+0x6C6]` falls 4 per frame, shrinking the trail's half-width
        // and scheduling the afterimage -> ribbon emitter handoff.
        self.move_fx_streak.tick_counter();
    }

    /// Walk actor `i`'s committed effect script for one frame and queue the
    /// resulting spawn requests (drained via
    /// [`World::drain_battle_effect_spawns`]). The engine seat of the retail
    /// per-frame call `FUN_80047430` -> `FUN_801DEA50`: the block is the
    /// committed clip's disc entry head, the cursor persists at
    /// [`Actor::battle_effect_cursor`], the facing comes from the SM's
    /// bearing writes (`BattleActor::facing_angle`), and the move-power map
    /// is the installed [`crate::move_power::MovePowerCatalog`]'s id-index
    /// map when present.
    ///
    /// The terminator's context writes land in [`Self::move_fx_streak`] -
    /// the `ctx[+0x1014]` / `+0x6C6` / `+0x1144` block the afterimage streak
    /// projects from ([`crate::action_effect_script::MoveFxStreak`]).
    // REF: FUN_80047430 (the retail caller this substitutes for)
    fn step_actor_effect_script(&mut self, i: usize, frame: i16) {
        use crate::action_effect_script as fx;
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        let Some(script) = actor.battle_effect_script.as_ref() else {
            return;
        };
        if actor.battle_effect_cursor >= fx::MAX_CURSOR {
            return;
        }
        let frame = u8::try_from(frame.max(0)).unwrap_or(u8::MAX);
        let script_actor = fx::EffectScriptActor {
            cursor: actor.battle_effect_cursor,
            facing: actor.battle.facing_angle,
            world: (
                i32::from(actor.move_state.world_x),
                i32::from(actor.move_state.world_y),
                i32::from(actor.move_state.world_z),
            ),
            // Retail scales offsets by the render node's mesh-header scale
            // (`actor[+0x22C][+0x72]`); the engine actor carries no render
            // node, so the q12 unit stands in (see `fx::scale_offset`).
            scale: 1 << 12,
            scope: actor.battle.active_target,
            action: actor.battle.params.first().copied().unwrap_or(0),
            suppressed: actor
                .battle
                .flag_bits
                .has(vm::battle_action::ActorFlags::FX_SUPPRESSED),
        };
        // The catalog's map is based at 0x801F4E63 (`map[move_id]`); the
        // stepper's terminator reads the 0x801F4E64-based view (`map[action
        // - 1]`), so skip the first byte - same bytes, reconciled bases.
        let map = self
            .move_power
            .as_ref()
            .and_then(|cat| cat.id_index_map_bytes().get(1..))
            .unwrap_or(&[]);
        let step = fx::step_effect_script(
            crate::action_effect_script::retail_rotation_lut(),
            script,
            script_actor,
            frame,
            map,
        );
        let cursor = step.cursor;
        for s in &step.spawns {
            self.battle_effect_spawns
                .push(crate::battle_events::BattleEffectSpawn {
                    actor_slot: i as u8,
                    effect: s.effect & !fx::EFFECT_DIRECT_BIT,
                    direct: s.direct,
                    at: s.at,
                    facing: script_actor.facing,
                });
        }
        // Terminator sink: install the staged move-power record's `+0x04`
        // word and the launch position into the move-FX streak block. The
        // record id the terminator resolves indexes the same table
        // `MovePowerCatalog` holds, so the `+0x6C6` word is that record's
        // `counter_init()`.
        if step.homing_band.is_some() {
            let counter = step
                .move_power_offset
                .map(|off| (off / fx::MOVE_POWER_STRIDE) as u8)
                .and_then(|id| self.move_power.as_ref()?.record_for_move_id(id))
                .map(|rec| rec.counter_init());
            self.move_fx_streak.install(&step, counter);
        }
        if let Some(actor) = self.actors.get_mut(i) {
            actor.battle_effect_cursor = cursor;
        }
    }

    /// The move-FX streak block the effect script's terminator installs -
    /// retail's `ctx[+0x1014]` / `+0x6C6` / `+0x24E` / `+0x1144` quartet.
    /// The render layer projects the afterimage streak from it; `is_armed()`
    /// is `false` until a terminator has run.
    pub fn move_fx_streak(&self) -> crate::action_effect_script::MoveFxStreak {
        self.move_fx_streak
    }

    /// Plan this frame's arts after-image ghosts - the engine seat of the
    /// retail per-actor walk `FUN_80049348` (see
    /// [`crate::battle_afterimage`]). For every battle actor with a pose
    /// history, sample the two rate-scheduled ring depths, keep the
    /// ghost-eligible ones, and resolve each ghost's flat additive colour
    /// (per-character base from the SCUS `0x80076908` table via the
    /// present-party ordinal; monsters share the `0x80076914` word; each
    /// drawn ghost decays by `0x101010`). Hosts draw each returned pose as
    /// a flat-coloured additive copy of the actor's mesh, behind the live
    /// body (retail pushes the ghost `0x50` OT buckets deeper).
    // REF: FUN_80049348 (the walk; kernel in `crate::battle_afterimage`)
    pub fn battle_ghost_draws(&self) -> Vec<BattleGhostDraw> {
        use crate::battle_afterimage as ai;
        let mut out = Vec::new();
        for (i, actor) in self.actors.iter().enumerate() {
            if actor.battle_pose_history.is_empty() {
                continue;
            }
            let monster = i >= 3;
            let base = if monster {
                ai::GHOST_COLOR_MONSTER
            } else {
                *ai::GHOST_COLOR_PARTY
                    .get(self.party_roster_slot(i))
                    .unwrap_or(&ai::GHOST_COLOR_MONSTER)
            };
            let hist = &actor.battle_pose_history;
            let plans = ai::plan_ghosts(actor.battle.anim_rate.get(), monster, base, |depth| {
                hist.get(depth.saturating_sub(1))
                    .map(|f| f.ghost_eligible)
                    .unwrap_or(false)
            });
            for p in plans {
                let Some(f) = hist.get(p.depth.saturating_sub(1)) else {
                    continue;
                };
                out.push(BattleGhostDraw {
                    actor_slot: i as u8,
                    pos: f.pos,
                    pose: f.pose.clone(),
                    color: p.color,
                });
            }
        }
        out
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
        // `+0x1DB = +0x1DA` (`FUN_8004AD80` `0x8004AEB0..0x8004AEB8`), taken
        // BEFORE the art-bank rewrite below turns an id >= 0x10 into its
        // dynamic slot number - so the latch keeps the RAW staged id, which
        // is the id space both battle-camera dispatch tables index.
        self.actors[i].battle.latched_anim = q;
        // The arts slow-motion arms (`FUN_8004AD80`; kernel
        // `legaia_engine_vm::battle_anim_rate`). Order is retail's: the
        // unconditional decay first (`0x8004B080` - a non-normal actor's
        // committing clip rises to half speed), then the staged-id arms.
        // The SpecialStarter (`0x1A`) freezes every slot and puts the
        // acting actor at quarter speed; an art constant (`>= 0x1B`) drops
        // the whole battle to half speed (quarter under an armed
        // `ctx[+0x243]`). The restore back to normal is the SM's Done arm
        // (`FUN_801E93C8` via `battle_gauge_rearm::rearm_gauge`).
        {
            use vm::battle_anim_rate as rl;
            let decayed = rl::commit_rate_decay(self.actors[i].battle.anim_rate);
            self.actors[i].battle.anim_rate = decayed;
            let marker = self.battle_ctx.gauge_rearm_latch != 0;
            match rl::staged_commit_rate_effect(q, i < 3, marker) {
                rl::CommitRateEffect::StarterFreeze => {
                    for a in self.actors.iter_mut() {
                        a.battle.anim_rate = rl::AnimRate(rl::RATE_FROZEN);
                    }
                    self.actors[i].battle.anim_rate = rl::AnimRate(rl::RATE_QUARTER);
                }
                rl::CommitRateEffect::StrikeSlow { rate } => {
                    for a in self.actors.iter_mut() {
                        a.battle.anim_rate = rl::AnimRate(rate);
                    }
                }
                rl::CommitRateEffect::None => {}
            }
        }
        let actor = &self.actors[i];
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
        // Retail keeps ONE staged-anim channel. The hit reaction is written
        // into the same `actor[+0x1DA]` byte the action SM stages into
        // (`FUN_800402F4` `0x80042118` knockdown / `0x80042124` flinch), and
        // this commit copies `+0x1DA` into `+0x1DB` unconditionally
        // (`FUN_8004AD80` `0x8004AEB0..0x8004AEB8`) - there is no reaction
        // guard anywhere on that path, and even the knockdown -> get-up chain
        // runs by writing `+0x1DA = +0x1F2` (`0x8004B690`). So a freshly
        // staged record REPLACES an in-flight reaction; it is not swallowed
        // by it. Swallowing it left a hit party member playing knockdown /
        // get-up through its own attack turn - walking to the target and back
        // lying on the ground, with the approach clip and every weapon swing
        // dropped. Dropping the latch here also stops the end-of-clip get-up
        // chain in `tick_battle_animations` from stealing the clip back.
        a.battle_reaction = None;
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
                // Anim record committed: install its effect script and zero
                // the effect-script cursor (retail FUN_8004AD80,
                // `sb zero,0x1f5` right after the record install).
                a.battle_effect_script = clip
                    .as_ref()
                    .map(|c| c.effect_script.clone())
                    .filter(|s| !s.is_empty());
                a.battle_effect_cursor = 0;
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
        // Reaction record committed: swap in its effect script + zero the
        // cursor (retail FUN_8004AD80, `sb zero,0x1f5` on every commit).
        actor.battle_effect_script = Some(clip.effect_script).filter(|s| !s.is_empty());
        actor.battle_effect_cursor = 0;
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
        // An in-flight hit reaction outranks the SM's per-frame pose calls.
        // This channel is the PORT's own idle-restore hook, not retail's
        // staged-anim byte: retail has a single `+0x1DA` stage that the
        // reaction and the SM both write (see `commit_staged_battle_anim`),
        // so there is nothing here to be faithful to - and without the guard
        // the per-frame `pose(Idle)` the attack band issues would cancel
        // every reaction on the frame after it starts.
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
        let selected = match clips.get(clip_slot).and_then(|c| c.as_ref()) {
            Some(clip) if clip_slot != 0 => {
                crate::battle_anim::MonsterAnimPlayer::new_one_shot(clip).map(|p| (p, clip))
            }
            _ => clips.first().and_then(|c| c.as_ref()).and_then(|clip| {
                crate::battle_anim::MonsterAnimPlayer::new(clip).map(|p| (p, clip))
            }),
        };
        if let Some((player, clip)) = selected {
            // Anim record swapped: install its effect script and zero the
            // effect-script cursor (retail FUN_8004AD80, `sb zero,0x1f5`).
            let script = Some(clip.effect_script.clone()).filter(|s| !s.is_empty());
            actor.battle_animation = Some(player);
            actor.battle_pose = Some(pose_id);
            actor.battle_effect_script = script;
            actor.battle_effect_cursor = 0;
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

    /// Frame count to size a **cross-context** clip cursor with when the scene
    /// ANM bundle cannot name the poked clip
    /// ([`crate::field_env::PropAnimBank::bind_actor_clip`]): the live
    /// locomotion clip's length if a host has installed a player clip player,
    /// else [`crate::field_env::PLAYER_CLIP_STANDIN_FRAMES`]. It sets how long
    /// the script's end-latch spin waits, so it only has to be finite and of
    /// the right order - the frames the player actually *sees* are the host
    /// clip player's, which is a different object.
    pub(crate) fn player_clip_frames_hint(&self) -> u16 {
        self.field_player_anim
            .as_ref()
            .map(|a| {
                let clip = if a.walking { &a.walk } else { &a.idle };
                clip.frame_count() as u16
            })
            .filter(|n| *n > 0)
            .unwrap_or(crate::field_env::PLAYER_CLIP_STANDIN_FRAMES)
    }

    /// Is the player running this frame?
    ///
    /// Retail (`FUN_801d01b0` at `0x801D0358..0x801D03A0`) computes it as the
    /// **exclusive or** of two things:
    ///
    /// - the run button - held pad `_DAT_8007B850` AND the mask config word
    ///   `[0x800846DC]`;
    /// - the Field Move option word `[0x800846CC]` (= `0x80084140 + 0x58c`,
    ///   the pause menu's Walk / Run row).
    ///
    /// The XOR is what the paired branches encode: from the button-held side
    /// (`bnez` at `0x801D0370` → `0x801D0390`) a set option jumps PAST the
    /// `$s4 = 0xc` store, and from the button-clear side it falls INTO it. So
    /// the option picks the default and the button inverts it - hold to run
    /// when Walk is selected, hold to walk when Run is.
    pub fn field_run_active(&self) -> bool {
        self.field_run_button_held != self.field_move_run_default
    }

    /// The frame's base step - retail's `$s4` before the `+0x72` multiply at
    /// `0x801D056C`.
    ///
    /// Ported from the selector at `0x801D0334..0x801D03E0`, in the order
    /// retail tests it: forced-slow wins outright (its arm `j`s past
    /// everything else), otherwise run vs walk. The debug-turbo arm
    /// ([`crate::world::config::FIELD_BASE_STEP_DEBUG_TURBO`]) is recorded but
    /// never taken - see its doc comment for the three gates.
    ///
    /// PORT: FUN_801d01b0 (base-step selector)
    pub fn field_base_step(&self) -> i32 {
        if self.field_forced_slow {
            return crate::world::config::FIELD_BASE_STEP_FORCED_SLOW;
        }
        if self.field_run_active() {
            return crate::world::config::FIELD_BASE_STEP_RUN;
        }
        crate::world::config::FIELD_BASE_STEP
    }

    /// Recompute [`World::field_actor_moving`] by diffing every tracked
    /// actor's live field position against last frame's, and fold the
    /// player's own bit into its locomotion animation.
    ///
    /// This is the source-agnostic half of the locomotion animation. The pad
    /// and nav-walk paths raise
    /// [`crate::field_anim::FieldPlayerAnim::moved_this_frame`] directly
    /// (they know they moved before they commit, and a *wall-blocked* pad
    /// step still walks in place the way retail does, which a position diff
    /// cannot see). Everything else that moves an actor - a motion-VM patrol
    /// leg, a cutscene `MoveTo`, a channel-driven walk-on - commits a
    /// position and nothing more, so without this pass those actors slide
    /// along in their idle pose.
    ///
    /// Called once per field tick, immediately before
    /// [`Self::tick_field_player_anim`], so it sees every position any earlier
    /// step in the frame committed.
    ///
    /// A slot appearing for the first time (an actor seated mid-scene by a
    /// timeline, or the frame after a scene load) seeds the snapshot and is
    /// NOT reported as moving - its arrival is a placement, not a step.
    pub(crate) fn detect_field_actor_motion(&mut self) {
        self.field_actor_moving.clear();
        let player_slot = self.player_actor_slot;
        // The player reads from its move_state (the locomotion commits
        // there); NPCs read from the live placement-position map.
        let player_pos = player_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let mut seen: std::collections::HashSet<u8> =
            std::collections::HashSet::with_capacity(self.field_npc_positions.len() + 1);
        let mut moved_player = false;
        if let (Some(slot), Some(pos)) = (player_slot, player_pos) {
            seen.insert(slot);
            match self.field_motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    moved_player = true;
                    self.field_actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        for (&slot, &pos) in &self.field_npc_positions {
            if Some(slot) == player_slot {
                // The player is tracked off its move_state above; a stale
                // mirror of it here must not double-report.
                continue;
            }
            seen.insert(slot);
            match self.field_motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    self.field_actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        // Drop slots that are no longer tracked (scene actors torn down), so
        // a later scene reusing the slot number starts from a fresh seed
        // instead of diffing against a dead actor's last position.
        self.field_motion_prev.retain(|slot, _| seen.contains(slot));
        if moved_player && let Some(anim) = &mut self.field_player_anim {
            anim.moved_this_frame = true;
        }
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
            field_record_words: None,
            child_spawns: Vec::new(),
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
        self.battle_monster_flee_attempted = false;
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
            // Seated facing: the party faces the monster row (+Z = heading
            // 0 in the FUN_80019B28 convention). Overwritten by the SM's
            // per-action bearing writes once actions run.
            actor.battle.facing_angle = 0;
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
            // Monsters face the party row (-Z = heading 0x800).
            actor.battle.facing_angle = 0x800;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        self.battle_end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch).
        self.effect_pool = vm::effect_vm::Pool::new();
        // Sparring fight: arm the tutorial prompt machine, the engine's stand-in
        // for retail paging stage overlay 967 in at battle load.
        self.battle_tutorial = None;
        self.battle_tutorial_boxes.clear();
        self.battle_flow = crate::battle_flow::BattleFlowState::Idle;
        if self.battle_tutorial_pending {
            self.arm_battle_tutorial();
        }
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
    ///
    /// TODO(return scenes): retail's master dispatch (`FUN_801CEA3C`) does
    /// NOT return to the trigger scene for mid-game FMVs - it copies a
    /// CDNAME label from the seven-entry list at `0x801CE8AC` into the
    /// next-scene name global `0x80084548` (+ spawn/door word `0x80084540`),
    /// e.g. `town01` triggers fmv 1 and lands in `town0b`. The per-id map is
    /// [`crate::cutscene::fmv_post_play_return_scene`]; wiring the actual
    /// scene transition here is pending (hosts currently drive transitions
    /// themselves after playback).
    // REF: FUN_801CEA3C
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

    /// Allocate a field actor in the auto-spawn slot range
    /// ([`FIELD_SPAWN_START_SLOT`]..), resolving its mesh from the global
    /// TMD pool (`tmd_idx`) and its spawn record from the VDF buffer
    /// (`vdf_idx`), and stamping the `kind`/`variant` classifier. Returns
    /// the allocated slot index, or `None` when the pool is exhausted.
    ///
    /// Shared by the field-VM synchronous actor allocator (the `0x4C 0xD8`
    /// path, retail `FUN_801D77F4`) and the tile-board install
    /// ([`World::try_install_tile_board`]); both resolve a template id
    /// through the same global-TMD + VDF-buffer path. Slots below
    /// [`FIELD_SPAWN_START_SLOT`] are skipped so party / scripted actors
    /// stay out of the auto-allocation range. Unresolved indices leave the
    /// actor with an empty `tmd_ref` / `spawn_record` (the synchronous
    /// spawn still succeeds), matching the retail bail-through.
    pub(crate) fn spawn_field_actor(
        &mut self,
        tmd_idx: i16,
        vdf_idx: u8,
        kind: u16,
        variant: u16,
    ) -> Option<usize> {
        let tmd_ref = self.global_tmd(tmd_idx).cloned();
        let record_bytes: Vec<u8> = self
            .vdf_record_bytes(vdf_idx)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let start = FIELD_SPAWN_START_SLOT as usize;
        let slot_idx = self
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)?;
        let actor = &mut self.actors[slot_idx];
        actor.active = true;
        actor.kind = kind;
        actor.variant = variant;
        actor.tmd_ref = tmd_ref;
        actor.spawn_record = if record_bytes.is_empty() {
            None
        } else {
            Some(record_bytes)
        };
        Some(slot_idx)
    }
}
