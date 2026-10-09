//! Battle actor animation: the per-frame battle animation tick, staged clip
//! commits at clip boundaries, reaction queueing and the battle pose apply.
//! Split out of `actors.rs`; no logic change.

use super::*;

impl World {
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
            // The `>> 2` branch of the cursor advance is taken only by a
            // **Slowed** actor on idle: `andi v0,v0,0x1000` on `+0x16E` at
            // `0x800476E0`, then `+0x1D9 == 0` at `0x800476EC`. Every other
            // actor - idle included - advances on the `>> 1` branch.
            let slowed = self.raw_status_word(i as u8) & vm::battle_anim_rate::SLOW_STATUS_BIT != 0;
            let actor = &mut self.actors[i];
            if let Some(player) = &mut actor.battle_animation {
                player.set_tween_target(tween);
            }
            let mut looped_recommit = false;
            let frame = if let Some(player) = &mut actor.battle_animation {
                let before = player.current_frame();
                // Retail rate law (`FUN_80047430`): the cursor advance
                // scales by the per-actor anim-rate byte `+0x21D` - the
                // arts slow-motion channel - and a Slowed actor's idle runs
                // at half the shift (`>> 2` vs `>> 1`).
                let rate = actor.battle.anim_rate;
                let slowed_idle =
                    slowed && actor.battle.current_anim == 0 && actor.battle_reaction.is_none();
                let pose = player.tick_rated(rate, slowed_idle);
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
                looped_recommit = recommit;
                Some(after)
            } else {
                None
            };
            // The same re-commit runs the rest of the commit's resets: the
            // per-clip hit index (`sb zero,0x1f4` at `0x8004B064`) and, for
            // the acting actor, the battle camera's ramp / accumulator /
            // latch (`0x8004BF50..0x8004BF78`). So an idle loop restarts
            // `ctx[+0x87C]` every cycle: `player_steal_skeleton_banner`'s
            // history ring holds Vahn's idle wrapping 22 vsyncs before the
            // save, and its accumulator reads `176` - eight a vsync from that
            // wrap, not from the idle's first commit 58 vsyncs back.
            // PORT: FUN_80047430 (`0x80047B54`, the natural-end commit of a
            // looping clip)
            if looped_recommit {
                self.actors[i].battle.input_cursor = 0;
                self.note_active_clip_commit(i);
            }
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
        // A looping clip (the idle, the walk) is cut mid-cycle by the same
        // two tick paths as a one-shot (`0x800478EC..0x80047948`): `+0x1DC`
        // bit 0 commits at once, bit 1 only once the cursor frame is past the
        // entry's gate frame by more than two frames, and both refuse an
        // entry carrying the `+0x76` lock. The strike loop stages each byte
        // under bit 1 (`0x801E3758`) over the idle that `0x19`'s arrival
        // committed under bit 0 (`0x801E35C0`), so the first swing waits for
        // idle frame 3 - `player_steal_skeleton_pre` holds Vahn on idle at
        // cursor `0x20` with `0x0F` staged under bit 1. A byte staged under
        // neither bit waits for the cycle's natural end in retail; the
        // engine's looping player has no cycle edge to hand over on, so that
        // case keeps committing at once.
        // PORT: FUN_80047430 (`0x800478EC..0x80047948`, the looping-clip half)
        let bits = actor.battle.flag_bits;
        if !bits.has(vm::battle_action::ActorFlags::WINDUP_DONE)
            && bits.has(vm::battle_action::ActorFlags::ADVANCE_DONE)
            && let Some(p) = actor.battle_animation.as_ref()
            && p.is_looping()
        {
            let (frames, lock) = p
                .hit_source()
                .map_or(([0; 4], 0), |s| (s.event_frames, s.event_lock));
            if !vm::battle_action::event_commit_due(&frames, lock, p.current_frame()) {
                return;
            }
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
    /// Retail's acting slot `ctx[+0x13]` as the port holds it. Through the
    /// end-of-battle sequence it is the pose actor - the one field the
    /// sequencer's framing, the camera-ghost pass and the commit's camera
    /// reset all read (`noa_levelup_banner`: `ctx[+0x13] == 0`, the posing
    /// leader). The engine's acting mirror keeps the fight's last actor, so
    /// a win landed by another seat would otherwise hand that seat's idle
    /// loop the accumulator the battle-over script rides. An escape skips
    /// the framing (`0x8004E720`) and keeps the mirror.
    pub(in crate::world) fn retail_acting_slot(&self) -> u8 {
        match self.battle.victory {
            Some(seq) if seq.cause != vm::battle_action::BattleEndCause::Escaped => {
                seq.pose_actor as u8
            }
            _ => self.battle_ctx.active_actor,
        }
    }

    /// The commit's battle-camera reset: when the committing actor is the
    /// active one (`lbu v0,0x13(v1); bne s3,v0` then `sb zero,0x26e` /
    /// `sw zero,0x87c` / `sb zero,0x26f` at `0x8004BF50..0x8004BF78`), the
    /// framings that read the ramp / accumulator / latch run from this
    /// clip's own start. The port bumps a counter the camera watches.
    // REF: FUN_8004AD80
    pub(in crate::world) fn note_active_clip_commit(&mut self, i: usize) {
        if i == usize::from(self.retail_acting_slot()) {
            self.battle_ctx.active_clip_commits =
                self.battle_ctx.active_clip_commits.wrapping_add(1);
            self.battle_ctx.active_clip_commit_frame = self.clock.display_frames;
        }
    }

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
            actor
                .battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE | ActorFlags::WINDUP_DONE);
            // The re-commit runs the same tail as an install, the camera
            // reset included (`0x8004BF50..0x8004BF78`).
            self.note_active_clip_commit(i);
            return;
        }
        // The install path re-zeroes the battle camera's ramp / accumulator /
        // latch when the committing actor is the active one
        // (`lbu v0,0x13(v1); bne s3,v0` then `sb zero,0x26e` / `sw zero,0x87c`
        // / `sb zero,0x26f` at `0x8004BF50..0x8004BF78`), so every framing
        // that reads them - the summon close-up's swing, the per-art arms -
        // runs from the clip's own start rather than from the action's.
        // REF: FUN_8004AD80
        self.note_active_clip_commit(i);
        if i == usize::from(self.retail_acting_slot()) {
            self.battle_ctx.active_clip_installs =
                self.battle_ctx.active_clip_installs.wrapping_add(1);
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
        // `0x8004B064`) and releases the stage latches (bits 0 and 1 of
        // `+0x1DC`, the `andi 0xFC` / `0xF8` at the two commit paths): the
        // strike loop may now read the byte behind this one. Idle included.
        {
            let a = &mut self.actors[i];
            a.battle.input_cursor = 0;
            a.battle
                .flag_bits
                .clear(ActorFlags::ADVANCE_DONE | ActorFlags::WINDUP_DONE);
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
}
