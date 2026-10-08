//! Move-VM/actor-physics ticking, battle animation staging/commit/reactions, poses, party roster, battle/world-map entry, cutscene finish, and sprite requests.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

/// The committed anim id (`+0x1D9`) the effect stepper's code substitution
/// keys on - the dynamic art slot the anim commit remaps an art clip onto.
const ACTION_FX_ART_SLOT: u8 = 0x11;
/// The table-form code an art's code-`0` record becomes (`li s1,0x9` at
/// `0x801DF094`).
const ACTION_FX_ART_CODE: u8 = 9;

impl World {
    /// Per-actor move-VM tick - clean port of `FUN_80021DF4` (lines
    /// `80022B94..80022BBC`).
    ///
    /// Two-phase: (1) pre-tick decrement the per-actor `wait_timer` by the
    /// global frame-time `delta`, (2) run the move VM through
    /// [`vm::move_vm::actor_tick`], which gates on the resulting timer and
    /// inspects the HALT flag after the call. Outcomes are recorded in
    /// [`crate::world::MoveVmGlobals::outcomes`] so engines that want to react to per-actor
    /// halts / waits can read them after the world ticks.
    ///
    /// `delta` mirrors the retail product `_DAT_1f800393 * _DAT_1f80037D`
    /// (per-frame anim-speed scalars). Engines pass their own per-frame
    /// scalar; the default world tick uses `1` so a Wait of N consumes N
    /// frames.
    pub fn tick_move_vms_with_delta(&mut self, delta: u16) {
        self.move_vm.outcomes.clear();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let bc = self.move_vm.bytecode.get(slot).cloned().unwrap_or_default();
            if bc.is_empty() {
                continue;
            }
            // Pre-tick: decrement wait timer (retail does this unconditionally
            // before the gate).
            vm::move_vm::decrement_wait_timer(&mut self.actors[slot].move_state, delta);
            let outcome = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
            self.move_vm.outcomes.push((slot as u8, outcome));
        }
    }

    /// Backwards-compatible wrapper using `delta = 1`.
    pub fn tick_move_vms(&mut self) {
        self.tick_move_vms_with_delta(1);
    }

    /// Per-actor physics tick - port driver for
    /// `engine-vm::actor_tick::tick_actor` (FUN_80021DF4). Runs
    /// [`vm::actor_tick::tick_actor`] once per active slot, then dispatches
    /// the emitted [`TickEvent`]s.
    ///
    /// This loop is the engine's form of the retail actor-list iterator
    /// `FUN_8002519C` - the walker `FUN_80016444` runs over each of the
    /// five `_DAT_8007C34C..0x36C` list heads. Per node it first snapshots
    /// the previous position (`+0x14` -> `+0x1C`, `+0x18` -> `+0x20`,
    /// `0x800251D4..0x800251F0`); a live node (`+0x10 & 8` clear) then gets
    /// `jalr node[+0x0C]` (`0x800252B4`) - for a standard actor that is
    /// `FUN_80021DF4`, reached through the same pointer as every other
    /// handler. A retired node (`& 8`) instead gets a one-time teardown
    /// guarded by bit `0x02000000` (not `0x200`): `FUN_80024DFC`,
    /// `FUN_800204A4`, and - when its handler is `FUN_80021DF4` - the frees of
    /// `+0xA8` / `+0x4C` and `FUN_800250D4` (`0x800251F4..0x80025294`). An
    /// earlier version of this doc described an inline physics tick with a
    /// `0x200` "ticked this frame" dedupe, which is not these bytes. The
    /// engine keeps one pool with an `active` flag instead of five lists, and
    /// the special-fn nodes are the dedicated ticks `World::tick` sequences
    /// around this loop. Neither the previous-position snapshot nor the
    /// retire teardown is modelled here. Node layout
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
    /// [`crate::world::MoveVmGlobals::last_tick_events`] for engines that want to consume
    /// them but otherwise no-op. Wiring those is orthogonal to the
    /// move-buffer cursor.
    ///
    /// `frame_delta` matches the retail `DAT_1F800393` ramp scalar
    /// (idle = `1`). The default tick uses `1`.
    // PORT: FUN_8002519c (list-walk tick dispatch; pool-not-lists divergence
    //                     documented above)
    pub fn tick_actor_physics_with(&mut self, scalars: TickScalars, listener: &ListenerState) {
        self.move_vm.last_tick_events.clear();
        let host = move_buffer_host::WorldMoveBufferView {
            move_buf: &self.move_vm.buffer_root,
            move2_buf: &self.move_vm.buffer2_root,
            alt_buf: &self.move_vm.buffer_alt_root,
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
                self.move_vm.last_tick_events.push((idx as u8, res));
            }
        }
    }

    /// Default-listener wrapper (no positional SFX integration yet) carrying
    /// the **live cadence** into the dispatcher scalars.
    ///
    /// `frame_delta` is retail's `DAT_1F800393` - the vsyncs one game tick
    /// spans - not a constant `1`. [`World::tick`] fires this once every
    /// [`crate::world::FrameClock::frame_step`] vsyncs, so the two together conserve
    /// vsyncs-per-second: the dispatcher integrates the same total delta over
    /// the same wall-clock span, just in fewer, larger steps. That is exactly
    /// retail's own trade, and it is why duration-based parity (the camera
    /// mover, every `t = min(t + dt, d)` accumulator) is untouched by it.
    ///
    /// REF: FUN_80016B6C
    pub fn tick_actor_physics(&mut self) {
        let listener = ListenerState::unicast(0, 0, 0);
        let cadence = vm::actor_tick::FrameCadence::from_raw(self.clock.frame_step);
        self.tick_actor_physics_with(TickScalars::for_cadence(cadence, 1), &listener);
    }

    /// Install the MOVE buffer pool root (retail `_DAT_8007B888`). The
    /// bytes are the MDT-shaped offset-table blob the scene-load path
    /// extracts from the slot-1 `Asset(0x05) = Move` descriptor. Pass
    /// an empty slice to clear it - the cursor's resolver will then
    /// return `None` for every requested id.
    pub fn set_move_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer_root = bytes;
    }

    /// Install the MOVE2 buffer pool root (retail `_DAT_8007B840`).
    /// Selected when an actor's `cursor_requested` is `>= 0x400`.
    pub fn set_move2_buffer_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer2_root = bytes;
    }

    /// Install the alternate MOVE buffer pool root (retail
    /// `_DAT_8007B75C`). Selected when the actor's status flag word
    /// has [`vm::move_buffer::STATUS_FLAG_ALT_POOL`] set.
    pub fn set_move_buffer_alt_root(&mut self, bytes: Vec<u8>) {
        self.move_vm.buffer_alt_root = bytes;
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
        // Per-clip impact freeze/tint arms + the tint's neutral decay
        // (`FUN_8004CE2C` pass 2 / `FUN_80050120` arm 0) - before the
        // cursor advance so an in-window freeze stops THIS frame's step,
        // like retail's tick order (the maintenance sweep runs off the
        // XA-template tick ahead of the anim node advance).
        self.tick_battle_impact_fx();
        self.tick_battle_ambient();
        // The same pass's Mystic Shield break (`0x8004D534..0x8004D668`).
        if self.mode == SceneMode::Battle {
            self.tick_mystic_shield_break();
        }
        for i in 0..self.actors.len() {
            // Hit-reaction chaining first: a finished reaction clip takes the
            // natural-end path and the next commit's reaction arms - the
            // `FUN_8004AD80` clip-tag ladder's staged entry (get-up behind a
            // knockdown, a downed party member's `7` -> `8` chain) and the
            // monster-death arm (`world::battle::clip_ladder`).
            let finished = {
                let a = &self.actors[i];
                match (a.battle_reaction, &a.battle_animation) {
                    (Some(tag), Some(p)) if p.finished() => Some(tag),
                    _ => None,
                }
            };
            if let Some(tag) = finished {
                self.finish_battle_reaction(i, tag);
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
            // The same id staged again behind itself - a monster stream that
            // repeats a byte (`[5, 5, 0]`) - reads as "nothing new" on the id
            // pair alone; the stage latch is what says a byte is waiting.
            // Retail's natural-end path calls `FUN_8004AD80` unconditionally
            // (`0x80047B30..0x80047B58`), and with `+0x1DA == +0x1D9` that is
            // the re-commit: the clip replays from its first keyframe with
            // its hit index zeroed. Converging to idle instead dropped the
            // second swing - and with it the parked-cursor hit that lands the
            // accumulated total on live HP, leaving the bar drained and the
            // SM parked in `0x51` for good.
            // PORT: FUN_80047430 (`0x80047B30..0x80047B58`, the natural-end commit)
            let restaged = staged_done.is_some_and(|id| {
                let b = &self.actors[i].battle;
                b.queued_anim == id
                    && b.current_anim == id
                    && b.flag_bits.has(vm::battle_action::ActorFlags::ADVANCE_DONE)
            });
            if restaged {
                self.commit_staged_battle_anim_at_boundary(i);
            } else if let Some(id) = staged_done {
                let a = &mut self.actors[i];
                a.battle_staged_anim = None;
                a.battle
                    .flag_bits
                    .clear(vm::battle_action::ActorFlags::ADVANCE_DONE);
                if a.battle.queued_anim == id {
                    a.battle.queued_anim = 0;
                    a.battle.current_anim = 0;
                } else {
                    // A byte was staged behind this clip (the retail one-ahead
                    // stream): the natural-end commit installs it right here,
                    // in the same tick, exactly as `FUN_8004AD80` is called
                    // from the tick's natural-end path (`0x80047B54`). Leaving
                    // it to the next frame's pair-inequality commit would let
                    // the SM overwrite the queued byte first.
                    self.commit_staged_battle_anim_at_boundary(i);
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
            // The decoder's last-frame tween reads the clip queued behind
            // this one; recompute it before the step, like the draw that
            // follows the tick in retail.
            let tween = self.battle_tween_target(i);
            let seru_staged = self.battle_ctx.multi_cast_gate != 0;
            let actor = &mut self.actors[i];
            if let Some(player) = &mut actor.battle_animation {
                player.set_tween_target(tween);
            }
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
                // The natural end's displacement: the actor steps along its
                // facing by the committed entry's `+0x0E` - the distance the
                // last frame's tween carried the body in model Z - unless the
                // root latch (`+0x1DC` bit 3) is up, or the actor is down with
                // no Seru staged (`0x80047A68..0x80047B2C`; the `+0x228` byte
                // it also tests has no store in the dump corpus and is taken
                // as clear).
                // PORT: FUN_80047430 (`0x80047A68..0x80047B2C`)
                let natural_end = player.take_natural_end();
                if natural_end {
                    let step = player.end_root_step();
                    let latched = actor
                        .battle
                        .flag_bits
                        .has(vm::battle_action::ActorFlags::FX_SUPPRESSED);
                    if step != 0 && !latched && (actor.battle.hp != 0 || seru_staged) {
                        let (sin, cos) =
                            vm::battle_action::motion::trig12(actor.battle.facing_angle);
                        let (dx, dz) = vm::battle_action::motion::end_root_step(sin, cos, step);
                        let ms = &mut actor.move_state;
                        ms.world_x = ms.world_x.wrapping_add(dx as i16);
                        ms.world_z = ms.world_z.wrapping_add(dz as i16);
                    }
                }
                // History-ring push (retail `FUN_80047430`
                // `0x80047E58..0x80048060`): slot 0 takes this frame's pose
                // + position; the arts after-image walk samples it. The
                // ring id is retail's own: a party seat stamps the
                // committed dynamic slot `+0x1D9` (`0x80047FCC`), a monster
                // seat stamps the committed record's `+0x77` byte `+ 0x10`,
                // or `0x11` when the record's `+0x87` solo byte is `1`
                // (`0x80048044..0x80048060`); the walk draws ids `>= 0x11`.
                // Both are read off the playing clip's own disc bytes - the
                // engine's staging / reaction state is not consulted, because
                // retail never consults it (gating on "is a clip staged"
                // ghosted every monster on every frame of its approach walk
                // and its idle loop after it: the permanent yellow halo).
                let clip_key = player.attach_key();
                let ring_id = if actor.battle_monster_id.is_none() {
                    crate::battle_afterimage::party_ring_id(actor.battle.current_anim)
                } else {
                    crate::battle_afterimage::monster_ring_id(clip_key, player.solo_flag())
                };
                let ghost_eligible = crate::battle_afterimage::ghost_eligible(ring_id);
                actor.battle_pose_history.push_front(BattleGhostFrame {
                    pose: pose.clone(),
                    pos: [
                        i32::from(actor.move_state.world_x),
                        i32::from(actor.move_state.world_y),
                        i32::from(actor.move_state.world_z),
                    ],
                    ghost_eligible,
                    clip_key,
                });
                actor
                    .battle_pose_history
                    .truncate(crate::battle_afterimage::HISTORY_DEPTH);
                actor.pose_frame = Some(pose);
                // The loop-window rewind (`0x800477EC..0x80047878`) re-arms
                // the effect script only for a party seat playing the art
                // slot `0x11` whose latched id `+0x1DB` is `0x2B` or above
                // (`sltiu v0,s1,0x3` / `li v0,0x11` / `sltiu v0,v0,0x2b`,
                // then `sb zero,0x1f5` / `0x1f6` / `0x1f4`); any other clip
                // fires its effect records once per commit. Re-arming every
                // wrap re-spawned a looping cast clip's record every cycle:
                // `freed_summon_mid_cast` drew six or seven overlapping ray
                // bursts where retail holds one.
                //
                // A looping clip's *natural end* is a different path: retail
                // has no loop counter there and re-commits the still-queued
                // `+0x1DA` (`FUN_80047430` -> `FUN_8004AD80`), and every
                // commit zeroes the cursor (`sb zero,0x1f5` at `0x8004B060`),
                // so a walk re-fires its footfall records every cycle.
                let rearm_window = after < before
                    && i < 3
                    && actor.battle.current_anim == 0x11
                    && actor.battle.latched_anim >= 0x2B;
                let recommit = natural_end && player.is_looping();
                if rearm_window || recommit {
                    actor.battle_effect_cursor = 0;
                }
                Some(after)
            } else {
                None
            };
            // `+0x1F7`, written for every node right after its cursor
            // advance (`0x80047E28..0x80047E54`).
            // PORT: FUN_80047430 (`0x80047E1C..0x80047E54`, the juggle window)
            self.actors[i].battle_juggle_window = Self::juggle_window_open(&self.actors[i]);
            // Per-frame effect-script walk for the committed record - the
            // engine seat of retail's `FUN_80047430` -> `FUN_801DEA50` call
            // pair (frame argument = the node's 12.4 anim cursor in whole
            // keyframes, which is `MonsterAnimPlayer::current_frame`).
            if let Some(frame) = frame {
                self.step_actor_effect_script(i, frame);
                // ... and its third call, the animation cue track
                // (`FUN_800508DC`, same arguments).
                self.step_actor_anim_cues(i, frame);
            }
        }
        // The move-FX streak counter walk (retail `FUN_801E09F8` phase 1):
        // `ctx[+0x6C6]` falls 4 per frame, shrinking the trail's half-width
        // and scheduling the afterimage -> ribbon emitter handoff.
        self.casting.move_fx_streak.tick_counter();
        self.tick_homing_slots();
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
    /// The terminator's context writes land in [`crate::world::CastFxState::move_fx_streak`] -
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
            // `FUN_801DF570`'s inputs: this actor's live pair and its
            // target's seat pair. A scope byte naming no seated actor (the
            // `8` / `9` whole-side codes) leaves the clamp without a
            // separation to measure.
            approach: self
                .actors
                .get(usize::from(actor.battle.active_target))
                .map(|_| {
                    let (ref_x, ref_z) =
                        self.battle_seat_of(usize::from(actor.battle.active_target));
                    vm::battle_approach::ApproachPose {
                        x: actor.move_state.world_x,
                        z: actor.move_state.world_z,
                        ref_x,
                        ref_z,
                    }
                }),
        };
        // The catalog's map is based at 0x801F4E63 (`map[move_id]`); the
        // stepper's terminator reads the 0x801F4E64-based view (`map[action
        // - 1]`), so skip the first byte - same bytes, reconciled bases.
        let map = self
            .tables
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
        // The table arm's code substitution (`0x801DF054..0x801DF094`): code
        // `0` reads as `9` while the context's **active** actor (`ctx[+0x13]`,
        // not the stepped one) has the dynamic art slot `0x11` committed in
        // `+0x1D9`. Every later read of the code - the `0x801F6418` CLUT map,
        // the `4` / `6` scale arms, the prototype table `0x801F6324` - takes
        // the substituted one, so an art's ray burst (code `9`, the purple
        // pool `9` mesh) replaces the plain swing's (code `0`, the red pool
        // `8` twin) - `battle_melee_hit_spark`'s Somersault.
        // PORT: FUN_801DEA50 (`0x801DF054..0x801DF094`)
        let art_slot_active = self
            .actors
            .get(usize::from(self.battle_ctx.active_actor))
            .is_some_and(|a| a.battle.current_anim == ACTION_FX_ART_SLOT);
        for s in &step.spawns {
            let mut effect = s.effect & !fx::EFFECT_DIRECT_BIT;
            if !s.direct && effect == 0 && art_slot_active {
                effect = ACTION_FX_ART_CODE;
            }
            self.battle
                .effect_spawns
                .push(crate::battle_events::BattleEffectSpawn {
                    actor_slot: i as u8,
                    effect,
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
        if let Some(band) = step.homing_band {
            // `ctx[+0x1014]` is the table base plus `index * 26`: the
            // record by table index, not by move id.
            let record = step
                .move_power_offset
                .map(|off| off / fx::MOVE_POWER_STRIDE);
            let counter = record
                .and_then(|idx| self.tables.move_power.as_ref()?.record_at_index(idx))
                .map(|rec| rec.counter_init());
            self.casting.move_fx_streak.install(&step, counter);
            if let Some(launch) = step.launch {
                self.seed_homing_slots(i, band, record, launch);
            }
        }
        if let Some(actor) = self.actors.get_mut(i) {
            actor.battle_effect_cursor = cursor;
        }
    }

    /// Walk actor `i`'s committed clip's **animation cue track** for one
    /// frame - the engine seat of retail's `FUN_80047430` -> `FUN_800508DC`
    /// call (`0x800478E4` / `0x80047C34`, the same `(slot, entry, frame)`
    /// arguments the effect-script stepper takes). The track is the entry's
    /// `+0x54` `(frame, cue)` run, read off the committed entry head
    /// ([`Actor::battle_effect_script`]); the cursor is
    /// [`Actor::battle_anim_cue_cursor`].
    ///
    /// Every fired cue goes through the battle sound funnel `FUN_8004FE5C`
    /// ([`crate::sfx_cue::route_sfx_cue`]) with the actor's **retail**
    /// actor-table index as the category: the `>= 0x100` party arm starts a
    /// CD-XA clip ([`AudioState::battle_xa_cues`]), everything else lands in
    /// the SFX ring both hosts drain ([`World::take_sfx_ring_ops`]) - a
    /// runtime-bank id (`>= 0x200`) after its `bse.dat` row's `+4` category
    /// has been written. That category is the actor's render-node `+0x80`
    /// byte ([`Self::battle_sound_category`]), the literal `2` for a party
    /// cue `>= 0xA7`.
    ///
    /// This is what an ordinary swing, a footstep, a knockdown and a
    /// monster's attack sound like: none of those sounds is emitted by the
    /// battle-action SM or the melee kernel.
    // REF: FUN_80047430 (the retail caller this substitutes for)
    fn step_actor_anim_cues(&mut self, i: usize, frame: i16) {
        use crate::anim_cue::{AnimCueActor, AnimCueEmit, AnimCueSlot, walk_anim_cues};
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        let Some(head) = actor.battle_effect_script.as_ref() else {
            return;
        };
        let track: Vec<AnimCueSlot> = (0..usize::from(crate::anim_cue::ANIM_CUE_TRACK_LEN))
            .map_while(|k| {
                let o = crate::anim_cue::ANIM_CUE_TRACK_OFFSET + k * 4;
                let b = head.get(o..o + 4)?;
                Some(AnimCueSlot {
                    frame: u16::from_le_bytes([b[0], b[1]]),
                    cue: u16::from_le_bytes([b[2], b[3]]),
                })
            })
            .collect();
        if track.first().is_none_or(|s| s.cue == 0) {
            return;
        }
        let cursor = actor.battle_anim_cue_cursor;
        let anim_id = head.get(0x77).copied().unwrap_or(0);
        let category = self.retail_actor_category(i as u8);
        let party = category < 3;
        let char_id = if party {
            self.party_roster_slot(i) as u8 + 1
        } else {
            0
        };
        // Record `+0xF8 & 0x2000` - bit `32 + 13` of the ability bitfield
        // at `+0xF4`.
        let voice_muted = party
            && self
                .party
                .roster
                .members
                .get(self.party_roster_slot(i))
                .is_some_and(|r| r.ability_bits()[5] & 0x20 != 0);
        let cue_actor = AnimCueActor {
            slot: category,
            char_id,
            anim_id,
            voice_muted,
            cd_busy: self.audio.battle_xa_busy_frames > 0,
        };
        let key = u8::try_from(frame.max(0)).unwrap_or(u8::MAX);
        let walk = walk_anim_cues(&cue_actor, &track, cursor, key, &mut || {
            self.next_rand() as i32
        });
        if walk.cursor_committed
            && let Some(a) = self.actors.get_mut(i)
        {
            a.battle_anim_cue_cursor = walk.cursor;
        }
        for emit in walk.emits {
            match emit {
                AnimCueEmit::Route { id, slot } => self.route_battle_cue(id, slot),
                AnimCueEmit::Dispatch { id } => {
                    if let Some(rid) = crate::anim_cue::dispatch_ring_id(id) {
                        self.audio.sfx_ring_ops.push(SfxRingOp::Push(rid as i16));
                    }
                }
                AnimCueEmit::Suppressed { .. } => {}
            }
        }
    }

    /// One call into the battle sound funnel `FUN_8004FE5C(id, category)`
    /// ([`crate::sfx_cue::route_sfx_cue`]), with its two outputs placed where
    /// both hosts read them: the CD-XA leg on
    /// [`AudioState::battle_xa_cues`], the ring id on the SFX ring ops - after
    /// the runtime row's `+4` category write the funnel makes first.
    // REF: FUN_8004FE5C
    pub(in crate::world) fn route_battle_cue(&mut self, id: u16, category: u8) {
        let cats: Vec<u8> = (0..8u8).map(|c| self.battle_sound_category(c)).collect();
        let element_of = |c: u8| cats.get(usize::from(c)).copied().unwrap_or(7);
        let durations = self.audio.xa_cue_durations.as_deref();
        let xa_duration_raw = |n: u32| {
            durations
                .and_then(|t| t.get(n as usize).copied())
                .unwrap_or(0)
        };
        let src = crate::sfx_cue::SfxCueSources {
            element_of: &element_of,
            xa_duration_raw: &xa_duration_raw,
            side_band_streaming: false,
            cd_read_busy: self.audio.battle_xa_busy_frames > 0,
        };
        let mut ring = crate::sfx_cue::SfxCueRing::default();
        let out = crate::sfx_cue::route_sfx_cue(&mut ring, u32::from(id), category, &src);
        if let Some(xa) = out.xa
            && xa.duration_sectors > 0
        {
            self.push_battle_xa_cue(xa);
        }
        if let Some(rid) = out.enqueued {
            if let Some(cat) = out.element_write {
                self.write_battle_sfx_category(rid, cat);
            }
            self.audio.sfx_ring_ops.push(SfxRingOp::Push(rid as i16));
        }
    }

    /// The byte the cue funnel writes into a runtime row's `+4` category for
    /// a cue fired by **retail** actor-table index `category` - the actor's
    /// render node `+0x80` byte (`*(0x801C9370[cat] + 0x22C) + 0x80`). It is
    /// a VAB slot, not an element: battle init `FUN_800513F0` seeds every
    /// node with `7` (`0x80051548`), and the battle scene loader
    /// `FUN_800520F0` re-seeds the monsters - `7` for every present monster
    /// (`0x8005225C`, slot 7 then takes `monster.snd` bank `min id - 1`),
    /// then `8` for the monsters carrying the largest id when the formation
    /// mixes ids (`0x8005234C`, slot 8 takes bank `max id - 1`). Slots 7 / 8
    /// are the battle's two `monster.snd` banks (`docs/formats/sfx-table.md`).
    // REF: FUN_800513F0, FUN_800520F0
    /// The `monster.snd` banks this battle opens, as `(VAB slot, bank
    /// index)`: slot `7` takes bank `min id - 1`, and - when the formation
    /// mixes monster ids - slot `8` takes bank `max id - 1`. Empty outside
    /// battle or with no monster seated. Both hosts stage these while the
    /// battle is on screen, which is what makes the monster-side runtime cues
    /// ([`Self::battle_sound_category`]) audible.
    ///
    /// Read off the battle scene loader `FUN_800520F0`: the first pass
    /// (`0x80052218..0x80052288`) keeps the smallest non-zero id of the four
    /// `DAT_8007BD0C` bytes and calls `FUN_8003E104(min - 1, 7, ..)`
    /// (`0x80052288..0x80052294`); the second (`0x800522B8..0x80052308`)
    /// keeps the largest and counts ids that differ from it on the way, and
    /// only a non-zero count reaches `FUN_8003E104(max - 1, 8, ..)`
    /// (`0x80052360..0x8005236C`). `FUN_8003E104` indexes the archive's
    /// sector table with that bank number directly.
    // REF: FUN_800520F0, FUN_8003E104
    pub fn battle_monster_sound_banks(&self) -> Vec<(u8, u16)> {
        let ids: Vec<u16> = self
            .battle_monster_slots()
            .into_iter()
            .map(|(_, id, _)| id)
            .filter(|&id| id != 0)
            .collect();
        let (Some(&min), Some(&max)) = (ids.iter().min(), ids.iter().max()) else {
            return Vec::new();
        };
        let mut out = vec![(7u8, min - 1)];
        if min != max {
            out.push((8, max - 1));
        }
        out
    }

    pub(in crate::world) fn battle_sound_category(&self, category: u8) -> u8 {
        if category < 3 {
            return 7;
        }
        let ids: Vec<u16> = self
            .battle_monster_slots()
            .into_iter()
            .map(|(_, id, _)| id)
            .collect();
        let slot = usize::from(self.engine_slot_of_retail_category(category));
        let Some(id) = self.actors.get(slot).and_then(|a| a.battle_monster_id) else {
            return 7;
        };
        let max = ids.iter().copied().max().unwrap_or(id);
        let mixed = ids.iter().any(|&x| x != ids[0]);
        if mixed && id == max { 8 } else { 7 }
    }

    /// The move-FX streak block the effect script's terminator installs -
    /// retail's `ctx[+0x1014]` / `+0x6C6` / `+0x24E` / `+0x1144` quartet.
    /// The render layer projects the afterimage streak from it; `is_armed()`
    /// is `false` until a terminator has run.
    pub fn move_fx_streak(&self) -> crate::action_effect_script::MoveFxStreak {
        self.casting.move_fx_streak
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
            let monster = actor.battle_monster_id.is_some();
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

    /// Arm the retail **impact tint triple** on `target`: `+0x04 = impact
    /// table[selector - 1]`, `+0x21F = selector`, `+0x0C = 0x1000` - the
    /// writes both impact arms perform when a hit lands. The melee / arts
    /// routine `FUN_801EC3E4` (`0x801EE3D4..0x801EE43C`) reads the selector
    /// off the acting actor's action record `+0x7A`
    /// ([`crate::battle_anim::MonsterAnimPlayer::impact_class`]) and gates it
    /// `0 < sel < 6` (`sltiu v0,v0,0x6` at `0x801EE3E0` - a class past the
    /// table skips all three writes; callers apply that gate, see
    /// [`crate::move_power::IMPACT_CLASS_LIMIT`]). The monster special-attack arm
    /// `FUN_801E09F8` (`0x801E15AC..0x801E15EC`, at each arm's impact phase)
    /// reads the move-power record's `+0x0A` and stamps unguarded (its
    /// selector ladder ends at `5`, so that byte never carries a `6`); a
    /// zero selector arms nothing in either (`beq v0,zero` past the writes).
    /// The `+0x7A` byte is the hit routine's whole **status / impact
    /// selector**, and the disc does carry `6` on it (the melee-path Curse
    /// arm at `0x801EE690`: a 1-in-4 `+0x16E |= 0x1000` roll and no tint) -
    /// that is what the `sltiu` gate is for. A selector past the table
    /// with no gate leaves the colour word alone here.
    ///
    /// The tint then decays through [`Self::tick_battle_impact_fx`]'s
    /// presentation SM (`FUN_80050120` arm 0): colour eases to neutral, the
    /// blend drains, the selector retires. Retail captures of the armed
    /// state: `battle_gimard_tail_fire_a` / `_b` (Vahn at `+0x21F = 1`,
    /// `+0x0C = 0x1000`, the red word eight lane-units apart between the two
    /// frames).
    // PORT: FUN_801EC3E4 (the `+0x7A` impact-tint arm only; the damage /
    // status body is the battle loop's)
    // REF: FUN_801E09F8 (the move-power sibling arm - same three writes)
    pub fn arm_impact_tint(&mut self, target: usize, selector: u8) {
        if selector == 0 {
            return;
        }
        let word = self
            .tables
            .move_power
            .as_ref()
            .and_then(|t| t.impact_table())
            .and_then(|t| t.get(usize::from(selector - 1)).copied());
        let Some(a) = self.actors.get_mut(target) else {
            return;
        };
        if let Some(word) = word {
            a.battle.render_color = word;
        }
        a.battle.impact_state = selector;
        a.battle.render_blend = vm::battle_formulas::TINT_BLEND_FULL;
    }

    /// Per-frame **battle ambient ramp**: the trailing block of
    /// `FUN_80050120` (`0x800505B0..0x8005083C`). The packed base
    /// `ctx[+0x890]` steps down while the summon close-up holds
    /// `ctx[+0x243]` and back up while it is clear
    /// (`legaia_engine_vm::battle_ground_grid::ambient_base_step`), and the
    /// pass stores the base the grid's near and far colours derive from
    /// unless it skips on the floor. One call per vsync, so `dt = 1`.
    // PORT: FUN_80050120 (the trailing ambient / far-colour block; the
    // backdrop pair's `+0x78` / `+0x56` ramp is not modelled)
    fn tick_battle_ambient(&mut self) {
        use vm::battle_ground_grid as grid;
        if self.mode != SceneMode::Battle {
            return;
        }
        let ctx = &mut self.battle_ctx;
        ctx.ambient_base = grid::ambient_base_step(ctx.ambient_base, ctx.gauge_rearm_latch != 0, 1);
        let rgb = grid::ambient_base_rgb(ctx.ambient_base);
        // `ctx[+0x278]` is one retail byte the port carries in two places:
        // the summon band's own `summon_staging_a` (set at `0x32 -> 0x33`,
        // `0x801E49F8`; cleared at the `0x34` exit) and the slot-B module's
        // scratch copy from `0x35` on. The band's `1` is what freezes the
        // ambient once the base reaches the floor through `0x33` / `0x34`.
        let ctx_278 = ctx.summon_staging_a | self.casting.module_ctx_278;
        // The backdrop pair's own ramp runs ahead of the ambient one in the
        // same pass (`0x80050600..0x80050714`), on the same two bytes.
        self.battle.backdrop_cue = grid::backdrop_cue_step(
            self.battle.backdrop_cue,
            ctx.gauge_rearm_latch,
            ctx_278,
            self.battle.stage_outdoor,
            1,
        );
        if !grid::ambient_store_skipped(rgb, ctx.gauge_rearm_latch, ctx_278) {
            self.battle.ambient_stored = rgb;
        }
    }

    /// The backdrop pair's depth-cue weight this frame, `1.0 = 0x1000`, or
    /// `None` when `FUN_80050120` has taken the pair off the draw (a weight
    /// of exactly `0x1000` switches `+0x56` to `0`, `0x80050850..0x80050880`).
    /// The pull is toward black: the records' colour word `+0x74` is `0`.
    /// Both battle hosts cue their backdrop draw with it.
    pub fn battle_backdrop_cue(&self) -> Option<f32> {
        let w = self.battle.backdrop_cue;
        (w != vm::battle_ground_grid::BACKDROP_CUE_FULL).then(|| f32::from(w) / 4096.0)
    }

    /// The battle ambient base the ground grid is coloured from this frame,
    /// 8 bits a channel: near colour `battle_ground_grid::battle_ambient_colour`
    /// of it, far colour `battle_ground_grid::grid_far_colour` of it. Settles
    /// on `0x80` and dims toward `0x20` through a summon close-up.
    pub fn battle_ambient_base(&self) -> [u8; 3] {
        self.battle.ambient_stored
    }

    /// Per-frame **presentation tint + impact freeze** maintenance: the
    /// per-actor arms of `FUN_80050120` (kernel
    /// `legaia_engine_vm::battle_formulas::tint_sm_step`) followed by
    /// `FUN_8004CE2C` pass 2's per-clip impact arms (kernel
    /// `legaia_engine_vm::battle_impact_fx`).
    ///
    /// The SM runs on every seated battle actor and dispatches on the
    /// render flag `+0x21C` exactly as retail's jump table does: `0` eases
    /// the colour word to neutral, then drains the `+0x0C` blend, then
    /// retires the `+0x21F` selector (one phase per frame, in that order);
    /// `1`/`3`/`4`/`6..=10` ease toward their fixed colours with the full
    /// blend stamped; `2` runs the defeat / capture fade; `5` and every
    /// out-of-table value (the cursor's `200`, the summon-hide `0xFF`)
    /// leave the words alone. The ease is unconditional on flag `0` -
    /// retail has no "was it armed" test, so a colour word any writer left
    /// off-neutral (the retired target cursor's dim word included) eases
    /// back at the same rate.
    ///
    /// Runs before the per-clip arms so an in-window arm's rewrite owns the
    /// frame - the retail order (`FUN_80046A20` calls `FUN_8004CE2C`, then
    /// the presentation tick, then the next frame's arms re-stamp).
    // PORT: FUN_8004CE2C (pass 2 - per-clip impact arms; pass 4 is
    // `crate::battle_status_clut`)
    // REF: FUN_80050120 (the per-actor arms, ported as `tint_sm_step`; this is
    // the per-frame walk over the actor table that drives them)
    /// The defeat fade's **sink** (`FUN_80050120` arm 2,
    /// `0x80050360..0x800504A8`): a monster seat (`3..=6`) fading out on
    /// render flag `2` sinks into the stage floor, `+0x36 += (size_class *
    /// dt) >> 2` a battle frame - `size_class` the record's `+0x1F` - while
    /// its node colour (`node[+0x74]`; the port reads its fading colour
    /// word) is non-zero. Gated off by a Seru absorb staged for the action
    /// (`ctx[+0x269]`, which raises the body for the absorb instead), a
    /// captured actor (`+0x225`) and a scripted fight (`ctx[+0x287]`) whose
    /// formation carries no second monster (`gp+0x9F5` = `0x8007BD0D` zero;
    /// the port reads "more than one monster seated").
    ///
    /// That last gate is also the one that raises the **lone-monster defeat
    /// latch** `ctx[+0x288]` (`sb s4,0x288(v1)`, `s4 = 1`, at `0x800504E8`):
    /// the scripted lone monster dies in place instead of sinking, and the
    /// latch is what lets the action SM's state-`0x20` reaction hold out
    /// without waiting for its fade (`BattleActionCtx::lone_defeat_latch`).
    ///
    /// The post-strike death re-frame forks on the height this moves
    /// (`target[+0x36] != 0` takes the ramped shot):
    /// `player_steal_skeleton_banner` reads its killed skeleton `183` down.
    ///
    /// PORT: FUN_80050120 (arm 2's monster sink and its `ctx[+0x288]` latch)
    fn tick_battle_defeat_sink(&mut self) {
        if self.battle_ctx.multi_cast_gate != 0 {
            return;
        }
        let lone_scripted =
            self.battle_ctx.scripted_fight != 0 && self.battle_monster_slots().len() <= 1;
        let first = self.party.party_count as usize;
        for slot in first..(first + 4).min(self.actors.len()) {
            let a = &self.actors[slot];
            if !a.active
                || a.battle_monster_id.is_none()
                || a.battle.render_flag != vm::battle_formulas::STATE_DEFEAT_FADE
                || a.battle.capture_state != 0
                || a.battle.render_color & 0x00FF_FFFF == 0
            {
                continue;
            }
            if lone_scripted {
                // `0x80050454..0x8005045C` skips the sink; `0x800504BC..
                // 0x800504E8` raises the latch.
                self.battle_ctx.lone_defeat_latch = 1;
                continue;
            }
            // `(size * dt) >> 2` a battle frame of `dt` vsyncs; the engine
            // ticks once a vsync.
            let sink = i16::from(self.battle_size_class_of(slot as u8)) >> 2;
            let ms = &mut self.actors[slot].move_state;
            ms.world_y = ms.world_y.wrapping_add(sink);
        }
    }

    /// The burning-body emitter at the tail of the anim decode
    /// `FUN_8004998C` (`0x8004A5FC..0x8004A8D8`): every body with a non-zero
    /// `+0x21F` selector spawns one effect-pool sprite per `0x10` of the
    /// frame's accumulator `ctx[+0x328]`, at a random object of its current
    /// pose jittered by its size - the fire a red selector-`1` body sheds
    /// ([`vm::battle_impact_fx::burn_effect`]): PROT 0903's arm-9 Gimard
    /// (`gimard_burning_attack` holds two of its effect-`0x0B` puffs at the
    /// creature's mouth) and a Tail-Fire-struck party member.
    ///
    /// The accumulator is the frame driver's: low nibble kept, `+8` a frame
    /// (`FUN_80046A20`, `0x8004713C..0x80047160`, `DAT_1F800393 << 3` at one
    /// step a tick).
    ///
    /// Not ported: selector `2`'s screen-shake globals (`_DAT_8007B92C` /
    /// `_DAT_8007B930`, `gp+0xA30..0xA34`, `0x8004A838..0x8004A8BC`).
    ///
    /// PORT: FUN_8004998C (`0x8004A5FC..0x8004A8D8`, the selector emit loop)
    pub(in crate::world) fn emit_battle_burn_sprites(&mut self) {
        use crate::action_effect_script::RotationLut;
        use vm::battle_impact_fx as ifx;
        let acc = (self.battle.burn_emit_accum & 0xF) + 8;
        self.battle.burn_emit_accum = acc;
        let emits = acc / ifx::BURN_EMIT_QUANTUM;
        if emits == 0 {
            return;
        }
        for i in 0..self.actors.len() {
            let a = &self.actors[i];
            let selector = a.battle.impact_state;
            if !a.active || selector == 0 {
                continue;
            }
            let Some(objects) = a
                .pose_frame
                .as_ref()
                .map(|p| p.bone_outputs.iter().map(|(t, _)| *t).collect::<Vec<_>>())
                .filter(|o| !o.is_empty())
            else {
                continue;
            };
            let Some(plan) = self.battle_actor_draw_plan(i, None, 4.0, false) else {
                continue;
            };
            let a = &self.actors[i];
            let base = [
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            ];
            let facing = a.battle.facing_angle;
            let red = plan.tint.colour as u8;
            let lut = crate::action_effect_script::retail_rotation_lut();
            for _ in 0..emits {
                let idx = self.next_rand() as usize % objects.len();
                let rands = [
                    self.next_rand() as i32,
                    self.next_rand() as i32,
                    self.next_rand() as i32,
                ];
                let p = ifx::burn_emit_point(
                    base,
                    facing,
                    objects[idx],
                    plan.radius,
                    rands,
                    |a| lut.b(i32::from(a)),
                    |a| lut.a(i32::from(a)),
                );
                if p[1] <= 0
                    && let Some(fx) = ifx::burn_effect(selector, red)
                {
                    self.try_spawn_effect(fx, p, facing & 0xFFF);
                }
            }
        }
    }

    fn tick_battle_impact_fx(&mut self) {
        use vm::battle_formulas::{FadeInputs, TintWords, tint_sm_step};
        use vm::battle_impact_fx as ifx;
        // The tag-`0x67` ribbon is a per-frame call in retail: re-derived
        // below every tick, so it drops the frame the window closes.
        self.battle.clip_ribbon = None;
        if self.mode != SceneMode::Battle {
            return;
        }
        // Whether each actor's reaction channel holds its knockdown entry
        // `+0x1F1` (which is the flinch entry on an actor without one).
        let on_knockdown: Vec<bool> = (0..self.actors.len())
            .map(|i| {
                let entry = self.actors[i].battle_reaction_entry;
                entry.is_some() && entry == self.battle_reaction_map(i).map(|m| m[2])
            })
            .collect();
        for (a, on_knockdown) in self.actors.iter_mut().zip(on_knockdown) {
            // `+0x22C == 0` (no battle record) skips the slot.
            if !a.active {
                continue;
            }
            let party = a.battle_monster_id.is_none();
            let b = &mut a.battle;
            let words = TintWords {
                color: b.render_color,
                blend: b.render_blend,
                selector: b.impact_state,
            };
            // The arm-2 gate compares the committed anim against the cached
            // knockdown entry `+0x1F1`; the engine plays a knockdown through
            // the reaction channel without re-pointing `current_anim`, so
            // the reaction channel's committed entry stands in for that
            // equality.
            let fade = FadeInputs {
                party,
                committed_anim: b.current_anim,
                knockdown_entry: if on_knockdown { b.current_anim } else { 0xFF },
                captured: b.capture_state != 0,
            };
            let (next, fx) = tint_sm_step(b.render_flag, words, fade, 1);
            b.render_color = next.color;
            b.render_blend = next.blend;
            b.impact_state = next.selector;
            if fx.mode_semi_transparent {
                // Arm 2 ORs `0x81000000` into the mode word `+0x08`
                // (`0x80050230..0x80050244`): the fading body draws additive.
                b.flag_word |= 0x8100_0000;
            }
            if fx.party_fade_done {
                // `0x80050344..0x80050354`: state 0, staged anim 0, the
                // `+0x1DC` fade-done bit, blend `0x800` (already in `next`).
                b.render_flag = 0;
                b.queued_anim = 0;
                b.flag_bits =
                    vm::battle_action::ActorFlags(vm::battle_action::ActorFlags::WINDUP_DONE);
            }
        }
        self.tick_battle_defeat_sink();
        self.emit_battle_burn_sprites();
        // The per-clip arms: the acting actor's committed record key + cursor
        // window select the writes onto it and its target.
        let acting = self.battle_ctx.active_actor as usize;
        let Some(actor) = self.actors.get(acting) else {
            return;
        };
        let Some(player) = &actor.battle_animation else {
            return;
        };
        let key = player.attach_key();
        let cursor = player.cursor_sixteenths();
        let target = actor.battle.active_target as usize;
        if actor.battle_monster_id.is_some() {
            // The monster arm (tag `0x3B`, `0x8004D2DC..0x8004D32C`).
            if let Some((own, tgt)) = ifx::monster_render_arm(key, actor.battle.hit_count_bound) {
                if let Some(t) = self.actors.get_mut(target) {
                    t.battle.render_flag = tgt;
                }
                if let Some(a) = self.actors.get_mut(acting) {
                    a.battle.render_flag = own;
                }
            }
            return;
        }
        let char_id = self.party_roster_slot(acting) as u8 + 1;
        let hit_index = actor.battle.input_cursor;
        if let Some(sel) = ifx::clip_impact_acting_selector(char_id, key)
            && let Some(a) = self.actors.get_mut(acting)
        {
            a.battle.impact_state = sel;
        }
        // Vahn's tag-`0x2B` arm writes the acting actor's `+0x21C`.
        if let Some(flag) = ifx::vahn_render_arm(char_id, key, cursor)
            && let Some(a) = self.actors.get_mut(acting)
        {
            a.battle.render_flag = flag;
        }
        // Noa's tag-`0x29` / `0x2D` status arm.
        let first_monster = self
            .battle_monster_slots()
            .into_iter()
            .find(|&(_, _, slot)| slot == 0)
            .map_or(0, |(_, id, _)| id as u8);
        let scripted = self.battle.scripted_fight;
        if ifx::noa_status_arm(char_id, key, hit_index, scripted, first_monster, || {
            self.next_rand()
        }) && let Some(t) = self.actors.get_mut(target)
        {
            t.battle.field_flags |= ifx::NOA_STATUS_BITS;
        }
        let Some(w) = ifx::clip_impact(char_id, key, cursor) else {
            return;
        };
        // `w.effect_at_target` is tag `0x67`'s per-frame
        // `FUN_801E1D98(&target[+0x3C], 0xC)` (`addiu a0,s1,0x3c` in the
        // delay slot at `0x8004D220`, `li a1,0xc` at `0x8004D224`): the
        // chained streak ribbon anchored on the target's seat vector.
        // `+0x3C..+0x43` is the spawn node's seat copied verbatim by the
        // battle setup (`0x8005158C..0x80051598`); the engine keeps its
        // `x`/`z` as `BattleActor::seat` and every retail seat row has
        // `y = 0` (`crate::battle_seats`), so the seat's Y is `0`.
        if w.effect_at_target
            && let Some(t) = self.actors.get(target)
        {
            let (sx, sz) = t
                .battle
                .seat
                .unwrap_or((t.move_state.world_x, t.move_state.world_z));
            self.battle.clip_ribbon = Some(super::ClipRibbon {
                seat: [sx, 0, sz],
                trail_id: ifx::GALA_EFFECT_ARG,
            });
        }
        let tint = self
            .tables
            .move_power
            .as_ref()
            .and_then(|t| t.impact_table())
            .map(|t| t[usize::from(w.impact_selector - 1)]);
        let Some(t) = self.actors.get_mut(target) else {
            return;
        };
        if w.freeze_target {
            t.battle.anim_rate = vm::battle_anim_rate::AnimRate(vm::battle_anim_rate::RATE_FROZEN);
        }
        if let Some(word) = tint {
            t.battle.render_color = word;
        }
        // The selector + full blend arm even without disc data (the freeze
        // is data-free; the tint word just has nothing to carry). Every tint
        // arm stamps `+0x0C = 0x1000` (`sw v0,0xc(s1)` at `0x8004D180` /
        // `0x8004D1DC` / `0x8004D234` / `0x8004D294`).
        t.battle.impact_state = w.impact_selector;
        t.battle.render_blend = vm::battle_formulas::TINT_BLEND_FULL;
    }

    /// The acting actor's impact-effect class for a landing hit - the
    /// `+0x7A` byte of its committed action record, read off the playing
    /// clip; `0` when nothing is playing (a synthetic battle) or the clip
    /// carries no class.
    pub(in crate::world) fn attacker_impact_class(&self, attacker: usize) -> u8 {
        self.actors
            .get(attacker)
            .and_then(|a| a.battle_animation.as_ref())
            .map(|p| p.impact_class())
            .unwrap_or(0)
    }

    /// Plan this frame's weapon-trail sweeps - the engine seat of the
    /// retail trigger `FUN_8005112C` + sweep driver `FUN_80048310`
    /// (kernels in `legaia_engine_vm::battle_trail`; geometry emission in
    /// `legaia_engine_ui::battle_trail`).
    ///
    /// For each party battle actor whose committed clip's `+0x77`
    /// identity byte matches its character's trigger row, sample the pose
    /// ring at even depths (one sweep step per two frames - the retail
    /// `2 * rate` cursor rewind under the per-frame `rate` advance),
    /// stopping at the clip boundary (ring `clip_key` mismatch), the
    /// 16-step budget, or a pose without the trigger's control points.
    /// A sweep below two steps draws nothing (`FUN_80048310`'s
    /// `slti 0x2` gate).
    // PORT: FUN_8005112C (trigger; table in `engine-vm::battle_trail`)
    // REF: FUN_80048310 (sweep capture - the ring sampling here replaces
    // the rewound re-decode; see `engine-vm::battle_trail` module docs)
    pub fn battle_weapon_trail_draws(&self) -> Vec<BattleWeaponTrailDraw> {
        use vm::battle_trail as wt;
        let mut out = Vec::new();
        for (i, actor) in self.actors.iter().enumerate() {
            // Party-only: retail's trigger gates on seat < 3; the engine
            // keys party-ness on identity, not slot (monsters may sit
            // anywhere - the slot-gate trap).
            if actor.battle_monster_id.is_some() {
                continue;
            }
            let Some(player) = &actor.battle_animation else {
                continue;
            };
            let key = player.attach_key();
            // Retail char id space: `DAT_8007BD10[seat]` = roster ordinal
            // + 1 (1 = Vahn, 2 = Noa, 3 = Gala).
            let char_id = self.party_roster_slot(i) as u8 + 1;
            let Some(trig) = wt::trail_trigger(char_id, key) else {
                continue;
            };
            let hist = &actor.battle_pose_history;
            let mut steps = Vec::new();
            'sweep: for k in 0..wt::MAX_SWEEP_STEPS {
                let Some(f) = hist.get(k * wt::SWEEP_FRAMES_PER_STEP) else {
                    break;
                };
                if f.clip_key != key {
                    break;
                }
                let mut pts = [[0i16; 3]; wt::TRAIL_POINTS];
                for (p, pt) in pts.iter_mut().enumerate() {
                    match f.pose.bone_outputs.get(trig.base_part + p) {
                        Some((t, _rot)) => *pt = *t,
                        None => break 'sweep,
                    }
                }
                steps.push(pts);
            }
            if steps.len() >= 2 {
                out.push(BattleWeaponTrailDraw {
                    actor_slot: i as u8,
                    steps,
                    rgb: trig.rgb,
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
    /// The clip the decoder tweens `slot`'s last frame into - the port of
    /// the next-entry rule's queued arm (`FUN_8004998C`
    /// `0x80049A7C..0x80049BD0`). Retail reads `+0x1DA`; the engine keeps
    /// that byte on two channels, so this resolves the entry the engine will
    /// actually install at the natural end: the reaction channel's staged
    /// entry (idle when `+0x1DC` bit 2 is up or nothing is staged), a byte
    /// staged behind a playing swing, the swing itself on a re-commit, and
    /// otherwise the idle a finished one-shot falls back to. A looping clip
    /// re-queues itself.
    ///
    /// The gate is retail's: HP `+0x14C` non-zero and the queued id below
    /// `0x10`, else the last frame blends toward itself with no Z term. A
    /// monster whose queued stream has a different part count also blends
    /// toward itself, but keeps the Z term (`0x80049B9C` branches into the
    /// `+0xE` arm with `a1 = t0`). The Z term is the **committed** entry's
    /// `+0x0E`; the `+0x228` byte that suppresses it has no store in the
    /// dump corpus and is taken as clear.
    // PORT: FUN_8004998C (`0x80049A7C..0x80049BD0`, the queued-clip arm of
    // the next-entry rule)
    pub(in crate::world) fn battle_tween_target(
        &self,
        slot: usize,
    ) -> Option<crate::battle_anim::TweenTarget> {
        use crate::battle_anim::TweenTarget;
        use vm::battle_action::ActorFlags;
        let a = self.actors.get(slot)?;
        let player = a.battle_animation.as_ref()?;
        let queued: u8 = if a.battle_reaction.is_some() {
            if a.battle.flag_bits.has(ActorFlags::EXIT) {
                0
            } else {
                a.battle_reaction_next.unwrap_or(0)
            }
        } else if let Some(id) = a.battle_staged_anim {
            if player.is_looping() || a.battle.queued_anim != id {
                a.battle.queued_anim
            } else if a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE) {
                id
            } else {
                0
            }
        } else if player.is_looping() {
            a.battle.current_anim
        } else {
            0
        };
        if a.battle.hp == 0 || queued >= 0x10 {
            return Some(TweenTarget {
                frame0: None,
                z_bias: 0,
            });
        }
        let z_bias = player.end_root_step();
        let frame0 = if player.is_looping() && queued == a.battle.current_anim {
            Some(player.first_frame().to_vec())
        } else {
            a.battle_action_clips
                .as_ref()
                .and_then(|cl| cl.get(usize::from(queued)))
                .and_then(|c| c.as_ref())
                .and_then(|c| c.frames.first().cloned())
        };
        // Retail's monster arm checks the queued stream's part count on the
        // non-idle path only; the player also refuses a mismatched frame
        // for either seat, which a party table never produces.
        let frame0 = frame0.filter(|f| f.len() == player.part_count());
        Some(TweenTarget { frame0, z_bias })
    }

    pub fn commit_staged_battle_anims(&mut self) {
        for i in 0..self.actors.len() {
            self.commit_staged_battle_anim(i);
        }
    }

    /// Single-actor arm of [`Self::commit_staged_battle_anims`]. Public so
    /// tests can drive one slot deterministically.
    ///
    /// Retail commits only at a **clip boundary**: `FUN_8004AD80` is called
    /// from the anim tick's natural-end path and its bit-1 event-path cut
    /// (and directly on a bit-0 "commit now"), never merely because
    /// `+0x1DA != +0x1D9`. So while a staged one-shot clip is still in
    /// flight, a byte staged behind it waits here - the natural end
    /// ([`Self::tick_battle_animations`]) or the event cut
    /// (`World::tick_battle_hit_events`) then calls
    /// [`Self::commit_staged_battle_anim_at_boundary`]. A looping player
    /// (the walk) is exempt: retail replays it by re-committing at every
    /// cycle end, and the engine's player has no cycle edge to hand over
    /// on, so a byte staged over the walk commits at once (the pre-boundary
    /// pacing).
    pub fn commit_staged_battle_anim(&mut self, i: usize) {
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        if actor.battle.queued_anim == actor.battle.current_anim {
            return;
        }
        let in_flight = actor.battle_staged_anim.is_some()
            && actor
                .battle_animation
                .as_ref()
                .is_some_and(|p| !p.finished() && !p.is_looping());
        if in_flight {
            if let Some(p) = actor.battle_animation.as_ref() {
                log::trace!(
                    "battle anim: slot {i} staged {:#04x} waits for the boundary of {:?} (frame {} / {}, loop cycles left {})",
                    actor.battle.queued_anim,
                    actor.battle_staged_anim,
                    p.current_frame(),
                    p.frame_count(),
                    p.loop_cycles_remaining()
                );
            }
            return;
        }
        self.commit_staged_battle_anim_at_boundary(i);
    }

    /// The commit body, run at a clip boundary (see
    /// [`Self::commit_staged_battle_anim`]). With the staged byte equal to
    /// the committed id this is retail's **re-commit** of a still-queued
    /// clip: the same entry is re-installed with its cursor and hit index
    /// zeroed (`0x8004B064..0x8004B068`) - the port rewinds the in-flight
    /// player in place. Either way the stage latch `+0x1DC` bit 1
    /// (`ADVANCE_DONE`) clears, which is what lets the strike loop read its
    /// next byte.
    // PORT: FUN_8004AD80 (the commit body; boundary selection is the tick's)
    pub(in crate::world) fn commit_staged_battle_anim_at_boundary(&mut self, i: usize) {
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        use vm::battle_action::ActorFlags;
        let Some(actor) = self.actors.get_mut(i) else {
            return;
        };
        let q = actor.battle.queued_anim;
        // The commit prologue's Arts-banner cancel (`0x8004ADBC..0x8004ADE8`,
        // ahead of every other write in the routine, idle re-commit included):
        // a banner in flight whose own actor commits again moves into the
        // `5..=8` retire band and restarts its clock.
        // REF: FUN_8004AD80
        if let Some((stage, level)) = vm::battle_action::banner_cancel_on_commit(
            self.battle_ctx.arts_banner_stage,
            i as u8,
            self.battle_ctx.active_actor,
        ) {
            self.battle_ctx.arts_banner_stage = stage;
            self.battle_ctx.arts_banner_level = level;
        }
        // The art-name label's close (`0x8004AE70..0x8004AEAC`): a party seat
        // (`sltiu v0,s3,0x3`) whose outgoing clip sits on dynamic slot
        // `0x11` - every art constant and the SpecialStarter install there -
        // under an attack command (`+0x1DE == 3`), and whose latched id
        // `+0x1DB` is not the SpecialStarter `0x1A`, destroys widget `0x21`,
        // the label. Ahead of the latch below and of the re-commit return,
        // as in retail. The art-constant arm re-opens it on this same commit
        // when the incoming id names another art.
        // REF: FUN_8004AD80
        {
            let b = &self.actors[i].battle;
            if self.actors[i].battle_monster_id.is_none()
                && b.current_anim == vm::anim_vm::DYNAMIC_ART_SLOT_B
                && b.action_category == vm::battle_action::ActionCategory::Attack.as_byte()
                && b.latched_anim != 0x1A
            {
                self.battle.move_label_closed = true;
            }
        }
        let actor = &mut self.actors[i];
        if q == actor.battle.current_anim {
            if let Some(p) = actor.battle_animation.as_mut() {
                p.rewind();
            }
            actor.battle_effect_cursor = 0;
            actor.battle_anim_cue_cursor = 0;
            actor.battle.input_cursor = 0;
            actor.battle.flag_bits.clear(ActorFlags::ADVANCE_DONE);
            return;
        }
        // The install path re-zeroes the battle camera's ramp / accumulator /
        // latch when the committing actor is the active one
        // (`lbu v0,0x13(v1); bne s3,v0` then `sb zero,0x26e` / `sw zero,0x87c`
        // / `sb zero,0x26f` at `0x8004BF50..0x8004BF78`), so every framing
        // that reads them - the summon close-up's swing, the per-art arms -
        // runs from the clip's own start rather than from the action's.
        // REF: FUN_8004AD80
        if i == usize::from(self.battle_ctx.active_actor) {
            self.battle_ctx.active_clip_commits =
                self.battle_ctx.active_clip_commits.wrapping_add(1);
            self.battle_ctx.active_clip_commit_frame = self.clock.display_frames;
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
        // (`FUN_801E93C8` via `battle_gauge_rearm::restore_anim_rates`).
        {
            use vm::battle_anim_rate as rl;
            let decayed = rl::commit_rate_decay(self.actors[i].battle.anim_rate);
            self.actors[i].battle.anim_rate = decayed;
            let marker = self.battle_ctx.gauge_rearm_latch != 0;
            // Party-ness is the actor's identity, not its slot: the engine
            // seats monsters wherever the formation put them (retail's
            // 0..2 / 3..7 split is `battle_monster_id` here).
            let is_party = self.actors[i].battle_monster_id.is_none();
            match rl::staged_commit_rate_effect(q, is_party, marker) {
                rl::CommitRateEffect::StarterFreeze => {
                    for a in self.actors.iter_mut() {
                        a.battle.anim_rate = rl::AnimRate(rl::RATE_FROZEN);
                    }
                    self.actors[i].battle.anim_rate = rl::AnimRate(rl::RATE_QUARTER);
                    // The same arm raises the Arts announcement banner. Retail
                    // reaches it only for a party seat, which is the engine's
                    // `battle_monster_id == None` (`is_party` above), and the
                    // rate arm has already established both that and the
                    // staged id. The middle pick is the queue-builder's own
                    // side array `0x801F6990[ctx[+0x15] - 1]`, which the
                    // engine models per actor as `starter_marks` - so the
                    // banner the player sees comes off the very marks the
                    // build loop and the Super tail-replace left behind.
                    // REF: FUN_8004AD80 (`0x8004B754..0x8004BB44`)
                    if is_party {
                        let seat = self
                            .battle_ctx
                            .arts_banner_seat_flags
                            .get(i)
                            .copied()
                            .unwrap_or(0)
                            != 0;
                        let b = &self.actors[i].battle;
                        let pick = usize::from(b.strike_index)
                            .checked_sub(1)
                            .and_then(|k| b.starter_marks.and_then(|m| m.get(k).copied()))
                            .map(|w| w as u8);
                        let (stage, level) =
                            vm::battle_action::banner_on_starter_commit(seat, pick);
                        self.battle_ctx.arts_banner_stage = stage;
                        self.battle_ctx.arts_banner_level = level;
                    }
                }
                rl::CommitRateEffect::StrikeSlow { rate } => {
                    for a in self.actors.iter_mut() {
                        a.battle.anim_rate = rl::AnimRate(rate);
                    }
                }
                rl::CommitRateEffect::None => {}
            }
        }
        // Every commit zeroes the per-clip hit index (`sb zero,0x1f4` at
        // `0x8004B064`) and releases the stage latch (bit 1 of `+0x1DC`,
        // the `andi 0xFC` / `0xF8` at the two commit paths): the strike loop
        // may now read the byte behind this one. Idle included.
        {
            let a = &mut self.actors[i];
            a.battle.input_cursor = 0;
            a.battle.flag_bits.clear(ActorFlags::ADVANCE_DONE);
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
        // The art-constant arm (`staged >= 0x1B`, `0x8004BB5C..0x8004BC40`)
        // places and opens the name label (`FUN_8004C650`, then
        // `FUN_801D8DE8(0x4C, 0)`) for a party seat.
        if q >= 0x1B && actor.battle_monster_id.is_none() {
            self.battle.move_label_closed = false;
        }
        let actor = &self.actors[i];
        // Resolve the clip + the committed id (post-rewrite).
        let (clip, committed) = match resolve_staged_anim(q) {
            StagedAnimTarget::ArtBank { record, slot } if actor.battle_art_bank.is_some() => {
                let clip = actor
                    .battle_art_bank
                    .as_ref()
                    .and_then(|b| b.get(record as usize))
                    .and_then(|c| c.clone());
                if clip.is_none() {
                    log::warn!(
                        "battle actor {i}: staged art id {q:#04x} -> bank record {record} \
                         carries no clip (zero-length commit)"
                    );
                }
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
        // The entry's solo / freeze byte (`+0x87`): a non-zero value is
        // handed to `FUN_8004E13C` right after the loop-window seed
        // (`0x8004BE18..0x8004BE2C`), which stores it into battle ctx
        // `+0x243` (`sb s0,0x243` at `0x8004E2C0`) - the marker the art
        // constant's rate arm reads (`ctx[+0x243]` armed -> quarter speed)
        // and the SM's Done arm clears. The routine's other half, the
        // per-actor pause flag `+0x21C` on every non-acting, non-target
        // slot, has no engine field; the SpecialStarter's freeze covers the
        // visible case through the rate arm above.
        // The value-2 arm (`0x8004E254..0x8004E2B4`): when the byte is `2`,
        // the previous `+0x243` was not, and the acting seat `ctx[+0x13]` is
        // a party one, it re-seeds the camera's yaw counter `ctx[+0x6DA] =
        // (rand() % 2) * 0x800 + 0x280` and zeroes the framing style
        // `ctx[+0xD]` - the swing camera's per-swing side. The counter is the
        // camera's (`BattleCamInputs::swing_reseed`); the style byte is the
        // live one both hosts feed it from.
        // PORT: FUN_8004E13C (the `+0x243` store and the value-2 re-seed;
        // the `+0x21C` sweep is not modelled)
        if let Some(v) = clip
            .as_ref()
            .and_then(|c| c.entry_solo_flag())
            .filter(|&v| v != 0)
        {
            if v == 2
                && self.battle_ctx.gauge_rearm_latch != 2
                && self.battle_ctx.active_actor < self.party.party_count
            {
                let coin = (vm::battle_formulas::world_rand(&mut self.rng_state) & 1) as u8;
                self.battle_ctx.swing_yaw_seeds = self.battle_ctx.swing_yaw_seeds.wrapping_add(1);
                self.battle_ctx.swing_yaw_coin = coin;
                self.battle_ctx.camera_variant = 0;
            }
            self.battle_ctx.gauge_rearm_latch = v;
        }
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
        a.battle_reaction_entry = None;
        a.battle_reaction_next = None;
        // The walk (action tag 1) loops until the SM stages something else
        // (AttackShortStep clears it to 0 on arrival). Keyed on the clip's
        // own tag, not its id: a party file's walk is entry 1, but a
        // monster's is wherever its record put the tag-1 entry, and the SM
        // stages that index (`monster_action_by_tag`). Engine assumption -
        // the loop-vs-once bit retail derives from the record kind isn't
        // modelled on MonsterAnimation.
        let player = clip.as_ref().and_then(|c| {
            if c.action_id == vm::battle_action::WALK_TAG {
                crate::battle_anim::MonsterAnimPlayer::new(c)
            } else {
                crate::battle_anim::MonsterAnimPlayer::new_one_shot(c)
            }
        });
        match player {
            Some(p) => {
                log::debug!(
                    "battle anim: slot {i} commits {committed:#04x} (staged {q:#04x}): {} frames, loop window {:?}, events {:?}, power {:?}, lock {:?}, speed {}",
                    p.frame_count(),
                    clip.as_ref().and_then(|c| c.entry_loop_window()),
                    clip.as_ref().and_then(|c| c.entry_event_frames()),
                    clip.as_ref().and_then(|c| c.entry_power_run()),
                    clip.as_ref().and_then(|c| c.entry_event_commit_lock()),
                    p.root_speed()
                );
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
                a.battle_anim_cue_cursor = 0;
            }
            None => {
                // No usable clip: a zero-length swing - nothing plays, the
                // latch above is already clear so the attack chain's read
                // gate is open; the live loop resolves the byte's hits at
                // stage time (`World::resolve_zero_length_clip_hits`).
            }
        }
    }

    /// Queue the retail hit reaction on a damaged battle actor, mirroring the
    /// damage primitive `FUN_800402F4`: a surviving target with no get-up
    /// entry plays the light flinch (`+0x1EF`, then straight back to idle);
    /// any other hit plays the knockdown (`+0x1F1`, which falls back to the
    /// flinch entry on an actor without one), whose natural end runs the
    /// commit's clip-tag ladder (`world::battle::clip_ladder`): the get-up
    /// while the actor lives, a downed party member's `7` -> `8` chain, a
    /// dead monster's death arm. No-op for actors without installed action
    /// clips (or without the needed entries).
    // PORT: FUN_800402F4 (damage-arm reaction staging: `+0x1DA = +0x1EF` for
    // a surviving no-get-up target, else `+0x1DA = +0x1F1`; the `+0x1EF..
    // +0x1F3` tag->entry map is built by FUN_80054CB0 / FUN_80053CB8).
    pub fn queue_battle_reaction(&mut self, slot: usize, survives: bool) {
        if let Some(entry) = self.battle_reaction_entry_for(slot, survives) {
            self.commit_battle_reaction_entry(slot, entry);
        }
    }

    /// The reaction entry [`Self::queue_battle_reaction`] would commit on
    /// actor `slot` - the `+0x1EF` flinch for a survivor without a get-up
    /// entry, the `+0x1F1` knockdown otherwise. `None` without clips.
    pub(in crate::world) fn battle_reaction_entry_for(
        &self,
        slot: usize,
        survives: bool,
    ) -> Option<u8> {
        let map = self.battle_reaction_map(slot)?;
        let has_getup = map[3] != 0
            && self
                .actors
                .get(slot)
                .and_then(|a| a.battle_action_clips.as_ref())
                .and_then(|c| c.get(usize::from(map[3])))
                .and_then(|c| c.as_ref())
                .is_some_and(|c| c.frame_count > 0);
        Some(if survives && !has_getup {
            map[0]
        } else {
            map[2]
        })
    }

    /// The action **tag** of every installed action clip of the monster in
    /// `slot`, in entry order - the `+0x4C` table the battle action SM's
    /// `FUN_80050E2C` lookups scan
    /// (`legaia_engine_vm::battle_action::monster_action_by_tag`).
    ///
    /// The clips are installed positionally
    /// (`legaia_asset::monster_archive::animations_by_entry`), so a clip's
    /// index is its entry index and its `action_id` is the entry's first
    /// byte. A hole - an entry with no decodable stream - has no byte to
    /// report and reads as [`legaia_asset::monster_archive::NO_ACTION_ENTRY`],
    /// which no searched tag equals; PROT 0867 carries no holes, so the list
    /// is the record's own. `None` for a party slot, an empty slot, or a
    /// monster whose clips were never installed.
    pub(in crate::world) fn battle_monster_action_tags(&self, slot: u8) -> Option<Vec<u8>> {
        let actor = self.actors.get(usize::from(slot))?;
        actor.battle_monster_id?;
        let clips = actor.battle_action_clips.as_ref()?;
        Some(
            clips
                .iter()
                .map(|c| {
                    c.as_ref()
                        .map_or(legaia_asset::monster_archive::NO_ACTION_ENTRY, |c| {
                            c.action_id
                        })
                })
                .collect(),
        )
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
    /// Pose id → action-stream slot is an engine interpretation: retail's
    /// `FUN_801D5854(actor, 6..9)` selects a camera / presentation program
    /// and writes no anim field. Pose 6 plays slot 0, the idle loop. Poses 7
    /// and 8 play it too: the same-numbered entries are the **downed**
    /// chain - the commit's clip-tag ladder stages entry 7 behind a dead
    /// party member's knockdown and entry 8 behind that
    /// (`world::battle::clip_ladder`), three catalogued states read a dead
    /// party member on entry 8, and none of the action SM's literal `+0x1DA`
    /// stores names 7 or 8 - so a living attacker never plays them. Pose 9
    /// plays entry 9 as a one-shot (the SM does stage id 9,
    /// `0x801E4A14`), holding its last frame via
    /// [`Self::tick_battle_animations`]; a missing slot falls back to idle.
    /// Re-requesting the actor's current pose keeps the playing clip.
    // REF: FUN_801D5854 - the SM's pose dispatch this hook answers; the
    // id->slot mapping is an engine interpretation, not a port of its body.
    pub fn apply_battle_pose(&mut self, slot: usize, pose_id: u8) {
        use vm::battle_action::Pose;
        // Ready / recover play the idle loop (doc above): fold them into the
        // idle request so the loop is not restarted when the SM switches
        // between them.
        let pose_id = if pose_id == Pose::Ready as u8 || pose_id == Pose::Recover as u8 {
            Pose::Idle as u8
        } else {
            pose_id
        };
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
        let clip_slot = if pose_id == Pose::Defeat as u8 {
            pose_id as usize
        } else {
            0
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
            actor.battle_anim_cue_cursor = 0;
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

    /// Whether a host's per-actor mesh pass draws actor `slot`: it must carry
    /// a TMD binding **and** be active (spawned). [`Self::init_scene_animations`]
    /// pre-binds every slot `K` to scene TMD `K` so a later spawn finds its
    /// mesh, which leaves the never-spawned slots bound, inactive and parked
    /// at the origin. Retail has no such actors - `FUN_8001E890` registers the
    /// scene TMDs in the pointer table without allocating any, and the render
    /// dispatcher walks only the allocated list - so drawing them paints every
    /// scene-pack mesh at world `(0, 0, 0)`.
    ///
    /// `synthetic_battle_camera` is the one exception the native window keeps:
    /// a battle with no stage dome frames every bound body round the origin.
    // REF: FUN_8001E890 (scene TMD registration - no actor allocation)
    // REF: FUN_8001D140 (the render dispatcher's allocated-list walk)
    pub fn actor_slot_drawn(&self, slot: usize, synthetic_battle_camera: bool) -> bool {
        self.actors
            .get(slot)
            .is_some_and(|a| a.tmd_binding.is_some() && (a.active || synthetic_battle_camera))
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
        self.locomotion.player_anim = anim;
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
        self.locomotion
            .player_anim
            .as_ref()
            .map(|a| a.active_frame_count() as u16)
            .filter(|n| *n > 0)
            .unwrap_or(crate::field_env::PLAYER_CLIP_STANDIN_FRAMES)
    }

    /// The size the player's figure draws at, as a factor of its mesh:
    /// the player's `+0x72` over `0x1000`.
    ///
    /// The animated renderer `FUN_8001B964` scales the actor matrix by
    /// `+0x72` whenever it is not `0x1000` (`0x8001BA6C..0x8001BAA4`, the
    /// three stores into the scale vector `0x1F800348` and `ScaleMatrix`), so
    /// the word the pad step reads as its speed multiplier is also the
    /// figure's size. Each kingdom's entry script sets it to `0xC00`, which is
    /// why the overworld player stands at three quarters of its town height.
    /// Both play hosts fold this into the player's draw.
    ///
    /// A `0` word is retail's "do not draw": `FUN_8001B964` tests it first
    /// (`lhu v0,0x72(s0)` / `beq v0,zero,0x8001BE20` at `0x8001B998..0x8001B9A0`)
    /// and returns without emitting a packet. Cutscenes hide the player that
    /// way while a stand-in walks (`CC F8 40 00 00 00 00`, see
    /// [`Self::player_hidden`]), so the scale is `0.0` and both hosts skip the
    /// draw. `1.0` with no player actor.
    ///
    /// REF: FUN_8001B964
    pub fn player_render_scale(&self) -> f32 {
        f32::from(self.player_scale_word()) / 4096.0
    }

    /// `true` while the player's `+0x72` is `0` - retail's animated renderer
    /// skips the actor outright (`FUN_8001B964` at `0x8001B9A0`). Both play
    /// hosts drop the player's draw (textured and colour halves) on it.
    ///
    /// REF: FUN_8001B964
    pub fn player_hidden(&self) -> bool {
        self.player_scale_word() == 0
    }

    /// Un-hide the player a script context left hidden because the **port**
    /// ended it before it reached its own restore.
    ///
    /// Cutscenes and talks hide the player with `CC F8 40 00 00 ..` and show
    /// it again with `CC F8 40 00 10 ..` (op `4C` nibble-4 sub-0 aimed at the
    /// player anchor `0xF8`; see [`Self::player_render_scale`]). Retail's
    /// runner (`FUN_80039B7C`) only lets go of a context on the context's own
    /// terminal bytes, so a record that hides the player always reaches the
    /// restore it carries. The port's runners also end a context on their
    /// anti-hang nets - a frame cap, a park timeout, a loop with no box, an
    /// op they cannot advance - and a context cut that way left `+0x72` at
    /// `0`: the player undrawn **and** unable to move, because the same word
    /// is the pad step's speed multiplier, until the next scene entry
    /// re-seated it.
    ///
    /// The rescue is keyed on the record's own bytes, so it never invents a
    /// restore: it fires only while the player is hidden, no ramp is in
    /// flight, and the record holds a non-zero `CC F8 40` write at or after
    /// the PC the run stopped on (`end_pc`) that the run never executed
    /// (`visited`). The player takes that op's target value - the value the
    /// script would have written. A record with no pending restore leaves the
    /// player hidden, as retail would. Returns whether it restored.
    ///
    /// REF: FUN_80039B7C, FUN_8001B964
    pub(crate) fn restore_owed_player_scale(
        &mut self,
        bytecode: &[u8],
        end_pc: usize,
        visited: &[bool],
    ) -> bool {
        if !self.player_hidden() || self.locomotion.player_scale_ramps.active() != 0 {
            return false;
        }
        let owed = (end_pc..bytecode.len().saturating_sub(4)).find_map(|p| {
            if visited.get(p).copied().unwrap_or(false)
                || !crate::world::vm_hosts::is_player_scale_op(bytecode, p)
            {
                return None;
            }
            let v = u16::from_le_bytes([bytecode[p + 3], bytecode[p + 4]]);
            (v != 0).then_some(v)
        });
        let Some(value) = owed else {
            return false;
        };
        let Some(a) = self
            .player_actor_slot
            .and_then(|s| self.actors.get_mut(s as usize))
        else {
            return false;
        };
        log::info!(
            "script context ended before its player restore; +0x72 0 -> {value:#06x} \
             (end pc {end_pc:#06x})"
        );
        a.move_state.field_72 = value;
        true
    }

    /// The player's live `+0x72`, `0x1000` with no player actor.
    fn player_scale_word(&self) -> u16 {
        self.player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map_or(0x1000, |a| a.move_state.field_72)
    }

    /// One frame of the player's `+0x72` ramps - the slots
    /// [`crate::world::FieldLocomotion::player_scale_ramps`] holds, lerped by
    /// retail's ramp ticker (`end + (start - end) * remaining / total`,
    /// `remaining` down by the frame step) and stored as a halfword (kind 2).
    /// A cutscene's `CC F8 40 00 10 64 00` grows the player back from hidden
    /// over 100 frames this way.
    ///
    /// REF: FUN_80036D80
    pub(crate) fn tick_player_scale_ramp(&mut self) {
        if self.locomotion.player_scale_ramps.active() == 0 {
            return;
        }
        let speed = self.move_vm.ramp_ratio.max(1);
        let writes = self.locomotion.player_scale_ramps.tick(speed);
        let Some(slot) = self.player_actor_slot.map(usize::from) else {
            return;
        };
        if let Some(a) = self.actors.get_mut(slot) {
            for w in writes {
                a.move_state.field_72 = w.value as u16;
            }
        }
    }

    /// Is the player running this frame?
    ///
    /// Retail (`FUN_801d01b0` at `0x801D0358..0x801D03A0`) computes it as the
    /// **exclusive or** of two things:
    ///
    /// - the run button - held pad `_DAT_8007B850` AND the mask config word
    ///   `[0x800846DC]`, which is `0x48` = Cross | R1;
    /// - the Field Move option word `[0x800846CC]` (= `0x80084140 + 0x58c`,
    ///   the pause menu's Walk / Run row).
    ///
    /// The XOR is what the paired branches encode: from the button-held side
    /// (`bnez` at `0x801D0370` → `0x801D0390`) a set option jumps PAST the
    /// `$s4 = 0xc` store, and from the button-clear side it falls INTO it. So
    /// the option picks the default and the button inverts it - hold to run
    /// when Walk is selected, hold to walk when Run is.
    pub fn field_run_active(&self) -> bool {
        self.locomotion.run_button_held != self.locomotion.run_default
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
    /// The forced-slow arm's byte `_DAT_8007B6A8` is the per-scene MAN flag
    /// [`crate::world::PartyState::scene_save_allowed`] (`FUN_8003AEB0` copies
    /// `MAN[1] & 1` into it), set on exactly the three kingdom world maps - so
    /// the overworld player always takes the slow step and never runs, which
    /// is what the retail overworld states measure (`10` units per `dt = 3`
    /// frame: `(5 * 0xC00) >> 12 = 3`, times 3, rounded up by the 2-unit
    /// stepper).
    ///
    /// PORT: FUN_801d01b0 (base-step selector)
    pub fn field_base_step(&self) -> i32 {
        if self.locomotion.forced_slow || self.party.scene_save_allowed {
            return crate::world::config::FIELD_BASE_STEP_FORCED_SLOW;
        }
        if self.field_run_active() {
            return crate::world::config::FIELD_BASE_STEP_RUN;
        }
        crate::world::config::FIELD_BASE_STEP
    }

    /// Recompute [`crate::world::FieldLocomotion::actor_moving`] by diffing every tracked
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
        self.locomotion.actor_moving.clear();
        let player_slot = self.player_actor_slot;
        // The player reads from its move_state (the locomotion commits
        // there); NPCs read from the live placement-position map.
        let player_pos = player_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let mut seen: std::collections::HashSet<u8> =
            std::collections::HashSet::with_capacity(self.npcs.positions.len() + 1);
        let mut moved_player = false;
        if let (Some(slot), Some(pos)) = (player_slot, player_pos) {
            seen.insert(slot);
            match self.locomotion.motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    moved_player = true;
                    self.locomotion.actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        for (&slot, &pos) in &self.npcs.positions {
            if Some(slot) == player_slot {
                // The player is tracked off its move_state above; a stale
                // mirror of it here must not double-report.
                continue;
            }
            seen.insert(slot);
            match self.locomotion.motion_prev.insert(slot, pos) {
                Some(prev) if prev != pos => {
                    self.locomotion.actor_moving.insert(slot);
                }
                _ => {}
            }
        }
        // Drop slots that are no longer tracked (scene actors torn down), so
        // a later scene reusing the slot number starts from a fresh seed
        // instead of diffing against a dead actor's last position.
        self.locomotion
            .motion_prev
            .retain(|slot, _| seen.contains(slot));
        if moved_player && let Some(anim) = &mut self.locomotion.player_anim {
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
        let Some(anim) = &mut self.locomotion.player_anim else {
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
            && let Some(buf) = self.move_vm.bytecode.get_mut(slot)
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
    /// [`crate::world::PartyState::active_party`]. Identity when no composition is installed
    /// or the ordinal runs past it - the historical slot-`i`-is-character-`i`
    /// behaviour every synthetic test relies on.
    pub fn party_roster_slot(&self, member: usize) -> usize {
        self.party
            .active_party
            .get(member)
            .map(|&s| s as usize)
            .unwrap_or(member)
    }

    /// The field party list - retail's `0x80084598` member ids for
    /// `DAT_80084594` entries - as the field VM's party ops see it.
    ///
    /// [`crate::world::PartyState::party_actor_slots`] carries it once a
    /// save or a party op has installed it. Before that (a New Game, or a
    /// save whose composition is the roster's identity order) the list is
    /// the installed battle composition: `active_party` when set, else the
    /// identity `0..party_count`. A list the party ops emptied stays empty
    /// ([`crate::world::PartyState::field_list_emptied`]).
    pub fn present_party_list(&self) -> Vec<u8> {
        if self.party.party_actor_slots.is_empty() && self.party.field_list_emptied {
            return Vec::new();
        }
        if !self.party.party_actor_slots.is_empty() {
            return self
                .party
                .party_actor_slots
                .iter()
                .flatten()
                .copied()
                .collect();
        }
        if !self.party.active_party.is_empty() {
            return self.party.active_party.clone();
        }
        (0..self.party.party_count.min(4)).collect()
    }

    /// Install `list` as the field party list and the battle composition
    /// together, as retail's party ops write the one list both read.
    /// An empty list clears the field list and leaves the battle
    /// composition as it was (no party of zero is ever fought with).
    pub fn install_present_party_list(&mut self, list: Vec<u8>) {
        self.party.party_actor_slots = list.iter().take(4).map(|&id| Some(id)).collect();
        self.party.field_list_emptied = list.is_empty();
        if !list.is_empty() {
            self.set_active_party(list);
        }
    }

    /// Install a present-party composition: `slots[i]` = roster slot for
    /// battle ordinal `i` (the engine mirror of retail's present-party
    /// list at `0x8007BD10`). The list caps at the 3 on-screen party
    /// positions (the runtime texture-band count). Sets
    /// [`crate::world::PartyState::party_count`] to the resulting length and, for each ordinal
    /// whose mapped roster record exists, reseeds the party actor's HP /
    /// MP / liveness / SPD mirror from it - the same projection
    /// [`Self::load_party`] performs for the identity mapping. Ordinals
    /// past the roster keep their live mirrors (zeroed-roster / synthetic
    /// setups render the character with default equipment, exactly like
    /// the identity default).
    pub fn set_active_party(&mut self, slots: Vec<u8>) {
        let mut active = slots;
        active.truncate(3);
        // Retail's New Game seeds all four live records from the SCUS
        // template (`0x80084708 + n*0x414`, the seed routine's four-iteration
        // loop), so a member who joins later already has a level-1 record.
        // The engine's New Game roster is Vahn alone; a join naming a slot it
        // lacks takes that slot's template row here, or the member would
        // fight with 0 / 0 HP and the battle could never see the party wiped.
        // Every slot up to the highest named one is filled, as retail's are:
        // a roster grown to reach slot 2 must not leave a zeroed slot 1.
        if let Some(tpl) = self.tables.starting_party.clone() {
            let top = active.iter().copied().max().unwrap_or(0);
            let missing: Vec<u8> = (0..=top)
                .filter(|&r| {
                    self.party
                        .roster
                        .members
                        .get(usize::from(r))
                        .is_none_or(|m| m.hp_mp_sp().hp_max == 0)
                })
                .collect();
            if !missing.is_empty() {
                self.seed_party_members(&tpl, &missing);
            }
        }
        for (member, &rslot) in active.iter().enumerate() {
            let Some(rec) = self.party.roster.members.get(rslot as usize) else {
                continue;
            };
            let hms = rec.hp_mp_sp();
            let activate = self.party_mirror_activates(member);
            if let Some(a) = self.actors.get_mut(member) {
                if activate {
                    a.active = true;
                }
                a.battle.hp = hms.hp_cur;
                a.battle.max_hp = hms.hp_max;
                a.battle.mp = hms.mp_cur;
                a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            }
            if let Some(s) = self.battle.speed.get_mut(member) {
                *s = rec.live_stats().spd;
            }
        }
        if !active.is_empty() {
            self.party.party_count = active.len() as u8;
        }
        self.party.active_party = active;
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
        self.battle.entry_serial = self.battle.entry_serial.wrapping_add(1);
        self.battle.monster_flee_attempted = false;
        // The battle scene setup re-seeds object-effect row 0
        // (`0x80055DDC..0x80055DF8`).
        // REF: FUN_80055B6C
        self.object_effect.reseed_for_battle();
        // The magic-level-up queue is a per-battle oracle record, not a host
        // hand-off: the banner the level-up raises is the battle message
        // banner (`raise_magic_level_banner`, screen element `0x65`), which
        // both hosts draw through `battle_hud::battle_banner_message`. No
        // host drains the queue, so it is bounded here - one battle's events
        // at most - instead of growing for the whole session.
        self.seru.magic_level_ups.clear();
        self.party.party_count = party_count.min(3);
        let monster_count = monster_count.min(5);
        let actor_count =
            ((self.party.party_count as usize) + (monster_count as usize)).min(MAX_ACTORS);
        for i in 0..(self.party.party_count as usize).min(actor_count) {
            let s = crate::battle_seats::party_seat(self.party.party_count, i);
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
        for i in (self.party.party_count as usize)..actor_count {
            let s = crate::battle_seats::monster_seat(
                monster_count,
                i - self.party.party_count as usize,
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
        // Every battle row past this fight's layout starts empty. The target
        // rows, the validator and the round walk all read slots
        // `party_count..party_count + 5` and count one as present by its
        // battle stats, so a slot the last fight (or the field) left carrying
        // stats seated a ghost enemy: a one-member party after a larger
        // layout faced its real monster plus stale rows that never die.
        // Retail's battle loader builds the actor table fresh per fight.
        for actor in self.actors.iter_mut().take(8).skip(actor_count) {
            actor.battle = Default::default();
            actor.battle_monster_id = None;
            actor.battle_element = None;
        }
        // Reset the battle ctx and seed at Begin via the public byte API to
        // avoid pulling battle_action::ActionState into world.rs imports.
        self.battle_ctx = vm::battle_action::BattleActionCtx::new();
        self.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
        // Battle init's ambient seed (`0x80051C70..0x80051C84`): the floor,
        // which the per-frame ramp lifts to the settled `0x80` while no cast
        // holds `ctx[+0x243]` - the fight's floor fades in from dark.
        self.battle_ctx.ambient_base = vm::battle_ground_grid::AMBIENT_BASE_FLOOR;
        self.battle.ambient_stored =
            vm::battle_ground_grid::ambient_base_rgb(vm::battle_ground_grid::AMBIENT_BASE_FLOOR);
        // Battle init spawns the backdrop records fresh: `+0x78` starts at 0.
        self.battle.backdrop_cue = 0;
        self.battle.end = None;
        // Effect pool is reused across scenes - reset to a fresh instance
        // (per-battle the head/free-list rebuilds from scratch). This is
        // retail's battle-loader init call (stage `0xE`, `0x80052670`): a
        // fresh pool is exactly the state `FUN_801DE914(0x1000, 0xA00)`
        // leaves, so `Pool::init_head` carries `REPLACED-BY` naming this line.
        // The rest of the battle-effect state goes with it - the mode switch
        // into battle runs the same actor-pool reset (`FUN_8001E1B4`) the
        // exit does.
        // REF: FUN_801DE914
        self.teardown_battle_effects();
        // Sparring fight: resolve the battle-stage id exactly as retail's
        // battle-entry tail does - default 0, and raise it to the tutorial
        // stage only when the disc's one-shot arm flag is set, consuming the
        // flag. `battle_tutorial_pending` is the separate debug force
        // (`World::prime_battle_tutorial`); both are evaluated so a forced
        // fight still consumes an armed flag rather than leaving it to fire
        // again on the next battle.
        self.battle.tutorial = None;
        self.battle.tutorial_boxes.clear();
        self.battle.flow = crate::battle_flow::BattleFlowState::Idle;
        self.battle.round_flow = crate::battle_round::RoundFlow::default();
        self.battle.commit_log_launch = None;
        self.battle.intro_names_frames = 0;
        // The per-fighter Auto flags and parked queues are battle state; the
        // disc inputs beside them are scene state and stay.
        self.battle.auto_combo.flags = [false; 3];
        self.battle.auto_combo.pending = false;
        self.battle.auto_combo.queues = Default::default();
        // `ctx[+0x289]` and the rest of the side-band state start at zero with
        // the rest of the battle context, as do the stage modules' own words.
        self.battle.sideband = Default::default();
        self.battle.arrival = Default::default();
        self.battle.form_transition = Default::default();
        // Battle init registers a fresh backdrop pair; any rebind is gone.
        self.battle.backdrop_rebound = false;
        self.battle.vram_moves.clear();
        self.battle.vram_loads = Default::default();
        self.battle.stage_camera = None;
        self.battle.stage_banner = None;
        // The entity SM's battle-entry tail writes the stage id: `0` in the
        // delay slot, raised to the tutorial stage by `arm_battle_tutorial`
        // when its arm fired (`0x801DA698..0x801DA6B0`). Battle init's
        // per-formation override is `enter_battle_from_formation`'s.
        self.battle.stage_id = 0;
        let armed_by_disc = self.take_battle_tutorial_arm();
        if self.battle.tutorial_pending || armed_by_disc {
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
        if self.world_map.ctrl.is_none() {
            self.world_map.ctrl = Some(WorldMapController::new());
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
        let Some(fmv_id) = self.cutscene.pending_fmv_trigger.take() else {
            return;
        };
        if self.mode != SceneMode::Field {
            return;
        }
        if crate::cutscene::fmv_index_to_str_filename(fmv_id).is_some() {
            self.cutscene.return_mode = Some(self.mode);
            self.mode = SceneMode::Cutscene;
            self.cutscene.active_fmv = Some(fmv_id);
        }
    }

    /// The FMV index currently playing in [`SceneMode::Cutscene`], or `None`
    /// when no STR FMV is active. Hosts poll this after [`World::tick`] to
    /// learn which `MV*.STR` to open.
    pub fn active_fmv(&self) -> Option<i16> {
        self.cutscene.active_fmv
    }

    /// The retail `MV*.STR` path of the active cutscene FMV, or `None` when
    /// no STR FMV is active. Convenience over
    /// [`crate::cutscene::fmv_index_to_str_filename`].
    pub fn active_fmv_str_filename(&self) -> Option<&'static str> {
        self.cutscene
            .active_fmv
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
    /// Retail's master dispatch (`FUN_801CEA3C`) does NOT return to the
    /// trigger scene for mid-game FMVs - it copies a CDNAME label from the
    /// seven-entry list at `0x801CE8AC` into the next-scene name global
    /// `0x80084548` (+ spawn/door word `0x80084540`), e.g. `town01` triggers
    /// fmv 1 and lands in `town0b`. That transfer needs the host's asset
    /// index, so this parks the finished id in [`crate::world::CutsceneState::finished_fmv`] and
    /// [`crate::scene::SceneHost::apply_pending_fmv_handoff`] performs it -
    /// one drain, whichever host polls.
    // REF: FUN_801CEA3C
    pub fn finish_cutscene(&mut self) {
        if self.mode == SceneMode::Cutscene {
            self.mode = self.cutscene.return_mode.take().unwrap_or(SceneMode::Field);
            self.cutscene.finished_fmv = self.cutscene.active_fmv;
            self.cutscene.active_fmv = None;
        }
    }

    /// Drain the id parked by [`World::finish_cutscene`]. `None` when no FMV
    /// has finished since the last drain.
    ///
    /// The world half of the post-play hand-off: a host with no scene loader
    /// (a headless world test, the `sim-trace` emitter) can consume the edge
    /// without one, and the scene host's
    /// [`apply_pending_fmv_handoff`](crate::scene::SceneHost::apply_pending_fmv_handoff)
    /// is the only production caller. `take` semantics are what stop two
    /// hosts - or one host polling twice - from transferring control twice.
    pub fn take_finished_fmv(&mut self) -> Option<i16> {
        self.cutscene.finished_fmv.take()
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

    /// Seat the **actor clone** field-VM op `0x4C` sub-1 sub-op `0x14` asks
    /// for: a fading, tinted copy of `src_id`'s transform on a pool slot
    /// whose handler is the clip-fraction fade.
    ///
    /// PORT: FUN_801D835C (the helper; the plan kernel is
    /// [`crate::field_actor_clone::clone_plan`])
    /// REF: FUN_8003C83C (the id resolve), FUN_80020DE0 (the allocation),
    /// REF: FUN_801D820C (the tick [`Self::tick_handler_actors`] then runs)
    ///
    /// `src_id` is resolved in the cross-context target space: `0xF8` is the
    /// player anchor, any other id is matched against the scene's script
    /// channels and read at its **live** position
    /// (`World::npcs.positions`, which the walk legs update) rather than at
    /// its MAN spawn point. An unresolvable id seats nothing, which is
    /// retail's `beqz s5` skip; so does an exhausted pool.
    ///
    /// Returns the seated slot.
    pub fn spawn_actor_clone(&mut self, src_id: u8, modulation: u32, rate: i16) -> Option<usize> {
        let src = self.clone_source(src_id)?;
        let plan = crate::field_actor_clone::clone_plan(src, modulation, rate);
        let start = FIELD_SPAWN_START_SLOT as usize;
        let slot_idx = self
            .actors
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, a)| !a.active)
            .map(|(i, _)| i)?;
        // Retail's `FUN_80020DE0` hands back a zeroed node stamped from the
        // descriptor, so the clone starts from a default record rather than
        // from whatever the slot last held.
        let mut actor = Actor {
            active: true,
            handler: crate::actor_handler::ActorHandler::ClipFade,
            state_54: crate::field_actor_clone::CLONE_DESCRIPTOR_INITIAL_STATE,
            ..Actor::default()
        };
        actor.physics.world_x = plan.pos.0;
        actor.physics.world_y = plan.pos.1;
        actor.physics.world_z = plan.pos.2;
        actor.physics.motion_x = plan.rot.0;
        actor.physics.motion_y = plan.rot.1;
        actor.physics.motion_z = plan.rot.2;
        actor.physics.timer = plan.rate;
        actor.physics.focal_envelope = plan.fraction;
        actor.move_state.world_x = plan.pos.0;
        actor.move_state.world_y = plan.pos.1;
        actor.move_state.world_z = plan.pos.2;
        actor.move_state.render_24 = plan.rot.0;
        actor.move_state.render_26 = plan.rot.1;
        actor.move_state.render_28 = plan.rot.2;
        actor.modulation_rgb = Some(crate::field_actor_clone::modulation_rgb(plan.modulation));
        self.actors[slot_idx] = actor;
        Some(slot_idx)
    }

    /// Seat the **reflection controller** the field VM's `4C 86` installs:
    /// a pool actor that mirrors one addressable actor's pose onto another
    /// across an axis-aligned plane, while the mirrored actor stands inside
    /// a tile rect.
    ///
    /// PORT: FUN_801E573C (the plan kernel is
    /// [`legaia_engine_vm::field_actor_reflect::spawn_controller`])
    /// REF: FUN_8003C83C (the arm's id resolve), FUN_80020DE0 (the allocation),
    /// REF: FUN_801E5154 (the tick [`Self::tick_handler_actors`] then runs)
    ///
    /// `ctx_is_player` is the executing context's `+0x10 & 0x01000000`, the
    /// same bit the eased-move arm reads; [`crate::world::FieldVmState::executing_channel`]
    /// resolves retail's `a0` - the script's own actor, which becomes the
    /// image - and the bit only stands in when no channel is executing. The
    /// `source_id` byte resolves in the cross-context target space (`0xF8` is
    /// the player), and an id that names nothing seats nothing, which is the
    /// arm's own `beqz s7` skip.
    ///
    /// Returns the seated slot.
    pub fn spawn_reflection_controller(
        &mut self,
        ctx_is_player: bool,
        source_id: u8,
        words: [i16; 6],
    ) -> Option<usize> {
        use crate::world::EasedMoveTarget;
        // Retail's `a0` is the executing context STRUCT, and a talk record's
        // context is the record's own actor even when its `+0x10` player bit
        // is up (the bit says who raised the record, not whose pose the
        // context carries). Every shipped `4C 86` sits in such a record and
        // names `0xF8`, so reading the bit as "the context is the player"
        // pairs the player with itself and the tick teleports them onto the
        // mirror line - the `other1` / `ropeway2` cold-spawn regression. The
        // record's placement is the image; the player bit only decides the
        // seat when there is no executing channel at all.
        let destination = match (self.field_vm.executing_channel, ctx_is_player) {
            (Some(placement), _) => EasedMoveTarget::Placement(placement),
            (None, true) => EasedMoveTarget::Player,
            (None, false) => return None,
        };
        let source = if source_id == 0xF8 {
            EasedMoveTarget::Player
        } else {
            let view = self.channel_view();
            let ci = crate::field_channels::resolve_target(view, source_id)?;
            EasedMoveTarget::Placement(view[ci].placement_index as u8)
        };
        if source == destination {
            // A pair whose two ends are one actor would write that actor's
            // own mirrored pose back onto it every frame; no retail record
            // forms one, and seating it can only strand the player.
            return None;
        }
        let slot = self.spawn_handler_actor(crate::actor_handler::ActorHandler::Reflection)?;
        let controller = legaia_engine_vm::field_actor_reflect::spawn_controller(words);
        let a = &mut self.actors[slot];
        a.state_54 = legaia_engine_vm::field_actor_reflect::REFLECT_INITIAL_STATE;
        a.reflection = Some(crate::world::ReflectionLink {
            destination,
            source,
            controller,
        });
        Some(slot)
    }

    /// The scene's channel set as a cross-context resolve should see it: the
    /// live vector, or - while a stepping pass has moved it out to execute one
    /// of its channels - the copy that pass took
    /// ([`crate::world::FieldVmState::stepping_view`]).
    ///
    /// Retail's resolver (`FUN_8003C83C`) walks the whole actor list whatever
    /// is executing, and a `4C 86` / `4C 14` / talk op that names another
    /// actor resolves it from inside a running script by construction. Read
    /// against the live vector alone, every such id resolved to nothing during
    /// a step: `conc2` seated one of its three entry-time mirrors (the one
    /// naming the player, which bypasses the walk) where a retail capture of
    /// the same entry seats all three.
    // REF: FUN_8003C83C
    pub fn channel_view(&self) -> &[crate::field_channels::FieldChannel] {
        if self.field_vm.channels.is_empty() {
            &self.field_vm.stepping_view
        } else {
            &self.field_vm.channels
        }
    }

    /// The source-actor fields the clone helper reads, resolved from a
    /// cross-context target byte.
    // REF: FUN_8003C83C
    fn clone_source(&self, src_id: u8) -> Option<crate::field_actor_clone::CloneSource> {
        use crate::field_actor_clone::CloneSource;
        if src_id == 0xF8 {
            let slot = self.player_actor_slot? as usize;
            let a = self.actors.get(slot)?;
            return Some(CloneSource {
                pos: (
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ),
                rot: (
                    a.move_state.render_24,
                    a.move_state.render_26,
                    a.move_state.render_28,
                ),
                ..CloneSource::default()
            });
        }
        let view = self.channel_view();
        let ci = crate::field_channels::resolve_target(view, src_id)?;
        let ch = &view[ci];
        let placement = ch.placement_index as u8;
        let (x, z) = self
            .npcs
            .positions
            .get(&placement)
            .copied()
            .unwrap_or((ch.ctx.world_x as i16, ch.ctx.world_z as i16));
        Some(CloneSource {
            pos: (x, ch.ctx.world_y as i16, z),
            rot: (ch.ctx.field_24, ch.ctx.field_26 as i16, ch.ctx.field_28),
            ..CloneSource::default()
        })
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
        // A retired occupant's handler, reflection link and move state must
        // not leak into the new actor (retail's allocator rewrites them).
        actor.init_allocated();
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

    /// Step the **Arts announcement banner** one battle frame.
    ///
    /// PORT: FUN_801E2524 (driver; the kernel is
    /// [`legaia_engine_vm::battle_action::step_flash_ramp`])
    ///
    /// Retail calls the ramp unconditionally from the battle draw tick
    /// (`FUN_800480D8`, `jal 0x801E2524` at `0x80048140`) and lets the stage
    /// byte gate it. The engine splits simulation from presentation, so the
    /// *step* runs here - inside the battle frame tick, where every host
    /// reaches it - and the quads come off the resulting state at draw time
    /// ([`Self::battle_arts_banner_quads`]). Returns `true` when the frame
    /// drew something, which is what a test can assert without a renderer.
    pub fn tick_arts_banner(&mut self, frame_delta: u8) -> bool {
        use legaia_engine_vm::battle_action as fr;
        let frame = fr::step_flash_ramp(
            self.battle_ctx.arts_banner_stage,
            self.battle_ctx.arts_banner_level,
            frame_delta,
        );
        if let Some(stage) = frame.stage_out {
            self.battle_ctx.arts_banner_stage = stage;
        }
        if let Some(level) = frame.level_out {
            self.battle_ctx.arts_banner_level = level;
        }
        !frame.layers.is_empty()
    }

    /// This frame's banner quads, in retail emit order - the shared read both
    /// hosts build their screen primitives from
    /// (`legaia_engine_ui::battle_numerals::arts_banner_prims`).
    ///
    /// Retail re-reads `ctx[+0x28C]` per layer *inside* the emitter, so a
    /// layer emitted later in the frame still sees the pre-walk value; the
    /// step above writes the walked value back, so this read has to recompute
    /// the layer set from the pre-walk clock rather than reuse the stepped
    /// one. Empty outside battle and on an idle banner.
    pub fn battle_arts_banner_quads(&self) -> Vec<legaia_engine_vm::battle_action::FlashQuad> {
        use legaia_engine_vm::battle_action as fr;
        if self.mode != SceneMode::Battle {
            return Vec::new();
        }
        let level = self.battle_ctx.arts_banner_level;
        // `frame_delta = 0`: the layer set for this clock, with no walk.
        let frame = fr::step_flash_ramp(self.battle_ctx.arts_banner_stage, level, 0);
        frame
            .layers
            .iter()
            .filter_map(|l| fr::flash_quads(l, level))
            .flatten()
            .collect()
    }

    /// The field VM's `0x4C 0xD8` allocator, whole: [`Self::spawn_field_actor`]
    /// plus the tail the port used to stop short of - the `actor+0x90`
    /// rest-pose snapshot, the `+0x3C`/`+0x3E` envelope rates and the
    /// `+0x0C` handler identity that makes the slot a morph actor at all.
    ///
    /// PORT: FUN_801D77F4
    ///
    /// The two immediates the instruction carries are the envelope's rise
    /// and fall rates in that order (`sh s5,0x3c` / `sh s6,0x3e` at
    /// `0x801D79A4`), and the weight, direction and render-mode halfwords
    /// are zeroed, so a fresh morph actor starts at the rest pose and rises.
    /// A spawn that resolves no morph block or no mesh seats no morph state
    /// and behaves exactly like the plain allocator - retail's own
    /// bail-through, where the snapshot allocation is a zero-byte request
    /// and the copy loop never runs.
    ///
    /// The tile-board install keeps calling [`Self::spawn_field_actor`]: its
    /// actors are not this opcode's, and the template id it passes as a VDF
    /// index would otherwise seat a morph block that retail never installs.
    pub(crate) fn spawn_morph_weight_actor(
        &mut self,
        tmd_idx: i16,
        vdf_idx: u8,
        up_rate: u16,
        down_rate: u16,
    ) -> Option<usize> {
        let slot_idx = self.spawn_field_actor(tmd_idx, vdf_idx, up_rate, down_rate)?;
        // The operand is a scene-bank index, not a raw pool slot: the arm
        // adds `*(u16*)0x8007B6F8` before the call (see
        // [`Self::field_pool_tmd`]). Resolving it against the pool the
        // engine seeds with the battle effect library bound jagaroom's and
        // garmel's morph actors to effect models and left balden's unbound.
        self.actors[slot_idx].tmd_ref = self.field_pool_tmd(tmd_idx).cloned();
        let block = match self.actors[slot_idx].spawn_record.clone() {
            Some(b) if b.len() >= 4 => b,
            _ => return Some(slot_idx),
        };
        let Some(gtmd) = self.actors[slot_idx].tmd_ref.as_ref().map(Arc::clone) else {
            return Some(slot_idx);
        };
        let groups = Self::tmd_group_vertex_bytes(&gtmd.tmd);
        let refs: Vec<&[u8]> = groups.iter().map(Vec::as_slice).collect();
        let rest_pose = crate::morph_weight_apply::rest_pose_snapshot(&block, &refs);
        let actor = &mut self.actors[slot_idx];
        actor.handler = crate::actor_handler::ActorHandler::MorphWeights;
        actor.morph_weights = Some(crate::morph_weight_apply::MorphWeightActor {
            block,
            rest_pose,
            envelope: crate::morph_weight_apply::MorphWeightEnvelope {
                weight: 0,
                up_rate: up_rate as i16,
                down_rate: down_rate as i16,
                descending: false,
            },
        });
        Some(slot_idx)
    }

    /// Every TMD object's vertex block as the 8-byte GTE vertices retail's
    /// object table points at (`[i16 x][i16 y][i16 z][i16 pad]`).
    fn tmd_group_vertex_bytes(tmd: &legaia_tmd::Tmd) -> Vec<Vec<u8>> {
        tmd.objects
            .iter()
            .map(|o| {
                let mut out = Vec::with_capacity(o.vertices.len() * 8);
                for v in &o.vertices {
                    out.extend_from_slice(&v.x.to_le_bytes());
                    out.extend_from_slice(&v.y.to_le_bytes());
                    out.extend_from_slice(&v.z.to_le_bytes());
                    out.extend_from_slice(&v._pad.to_le_bytes());
                }
                out
            })
            .collect()
    }

    /// Actor slots carrying live morph-weight state, with each one's current
    /// blend weight - the host-side change detector: re-pose and re-upload a
    /// slot whose weight has moved since the last upload.
    pub fn morph_weight_actor_weights(&self) -> Vec<(u8, i16)> {
        self.actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.active)
            .filter_map(|(i, a)| {
                let m = a.morph_weights.as_ref()?;
                Some((u8::try_from(i).ok()?, m.envelope.weight))
            })
            .collect()
    }

    /// The **one** engine-side morph kernel both hosts draw through: actor
    /// `slot`'s mesh with its rest pose restored and its morph deltas
    /// re-blended at the live `+0x6E` weight, returned as a posed TMD
    /// alongside the raw bytes a VRAM-mesh build needs and the weight it was
    /// posed at.
    ///
    /// This is [`crate::morph_weight_apply::apply_morph_weights`] run over
    /// the engine's own vertex representation, so neither host owns any part
    /// of the blend. `None` when the slot carries no morph state, no mesh,
    /// or a block that names nothing the mesh has.
    pub fn morph_weight_posed_tmd(
        &self,
        slot: usize,
    ) -> Option<(legaia_tmd::Tmd, Arc<Vec<u8>>, i16)> {
        let actor = self.actors.get(slot)?;
        if !actor.active {
            return None;
        }
        let m = actor.morph_weights.as_ref()?;
        let gtmd = actor.tmd_ref.as_ref()?;
        let mut groups = Self::tmd_group_vertex_bytes(&gtmd.tmd);
        let weight = m.envelope.weight;
        if crate::morph_weight_apply::apply_morph_weights(
            &m.block,
            &mut groups,
            &m.rest_pose,
            weight,
        ) == 0
        {
            return None;
        }
        let mut posed = gtmd.tmd.clone();
        for (obj, bytes) in posed.objects.iter_mut().zip(groups.iter()) {
            for (i, v) in obj.vertices.iter_mut().enumerate() {
                let o = i * 8;
                if o + 6 > bytes.len() {
                    break;
                }
                v.x = i16::from_le_bytes([bytes[o], bytes[o + 1]]);
                v.y = i16::from_le_bytes([bytes[o + 2], bytes[o + 3]]);
                v.z = i16::from_le_bytes([bytes[o + 4], bytes[o + 5]]);
            }
        }
        Some((posed, Arc::new(gtmd.raw.clone()), weight))
    }
}
