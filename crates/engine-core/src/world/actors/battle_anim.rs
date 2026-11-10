//! Battle actor animation: the per-frame battle animation tick, staged clip
//! commits at clip boundaries, reaction queueing and the battle pose apply.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// Slots the per-actor pose ring keeps: retail's 32-frame ring
/// ([`legaia_engine_battle::battle_afterimage::HISTORY_DEPTH`]) at up to
/// four vsyncs a frame, since the per-vsync cadence pushes a slot a vsync
/// (`World::ghost_ring_slots_per_frame`).
/// Display frames past a `+0x87` commit at which the dealt track coin is
/// read (`World::dealt_track_coin`): a replay's commit lands within a few
/// frames of retail's, and an art's next `+0x87` commit is tens of frames on.
const DEALT_COIN_LOOKAHEAD: u64 = 8;
const BATTLE_POSE_HISTORY_SLOTS: usize = legaia_engine_battle::battle_afterimage::HISTORY_DEPTH * 4;

impl World {
    /// The engine slots behind retail's seven battle table seats (party
    /// `0..=2`, monsters from `3`), in seat order, for a walk retail bounds at
    /// seven: a fifth monster sits at seat `7`, outside every such walk.
    pub(in crate::world) fn battle_table_seats(&self) -> Vec<usize> {
        let seats = legaia_engine_battle_vm::battle_slot::SeatMap::new(self.party.party_count);
        (0..7u8)
            .filter_map(|r| seats.engine(r).map(usize::from))
            .filter(|&e| e < self.actors.len())
            .collect()
    }

    /// What a frame of seated clip cursors moves with them, re-derived the
    /// way the pass that poses the bodies derives it: every body pair
    /// `+0x3C..+0x40` (the live pair plus the new pose's centroid) and the
    /// Mystic Shield, which sits on the first monster seat's pair.
    pub fn settle_seated_clips(&mut self) {
        self.refresh_battle_body_pairs();
        if self.mode == SceneMode::Battle {
            self.tick_dohati_growth();
            self.tick_mystic_shield();
        }
    }

    /// Put a party member at zero HP on its downed kneel - entry `8`, which
    /// re-commits itself at every natural end
    /// ([`legaia_engine_battle_loop::clip_ladder::PARTY_DOWNED_LOOP_ENTRY`]) - when
    /// it is not there already. A seat that lands a member's HP at zero
    /// skips the knockdown chain (`4 -> 7 -> 8`) that leads there, and a
    /// capture of a downed member holds `+0x1D9 = +0x1DA = 8`. Returns
    /// whether the member kneels. A no-op for a monster or a living member.
    pub fn seat_downed_kneel(&mut self, slot: usize) -> bool {
        use legaia_engine_battle_loop::clip_ladder::PARTY_DOWNED_LOOP_ENTRY;
        let Some(a) = self.actors.get(slot) else {
            return false;
        };
        if a.battle_monster_id.is_some() || a.battle.hp != 0 {
            return false;
        }
        if a.battle_reaction == Some(PARTY_DOWNED_LOOP_ENTRY) {
            return true;
        }
        let kneels = self.commit_battle_reaction_entry(slot, PARTY_DOWNED_LOOP_ENTRY);
        if kneels {
            // The kneel's pose centroid moves the body pair `+0x3C..+0x40`
            // the framings and the range law read.
            self.refresh_battle_body_pairs();
        }
        kneels
    }

    /// Carry a body still falling on its knockdown on to the get-up the
    /// capture shows it rising on: when slot `slot` plays its knockdown
    /// entry `+0x1F1` on the reaction channel and the capture's `+0x1D9`
    /// names its get-up entry `+0x1F2`, run the knockdown's natural end - the
    /// anim tick's end-of-clip path and the commit after it, which for a
    /// monster at HP `0` is the death arm (`+0x21C = 2`, the spoils, and with
    /// a Seru staged in `ctx[+0x269]` the get-up `0x8004B688..0x8004B6A0`).
    /// The reaction starts on the hit, which the replay lands on its own
    /// clock, so the knockdown the replay struck may still be falling where
    /// retail's has ended (`rim_elm_gimard_seru_capture_before`: retail's
    /// Gimard sixteen keyframes into the get-up, the replay's two short of
    /// the knockdown's end). A no-op on any other pairing.
    ///
    /// REF: FUN_80047430 (`0x80047B30..0x80047B58`), FUN_8004AD80
    fn seat_knockdown_end(&mut self, slot: usize, clip: u8) {
        let Some(map) = self.battle_reaction_map(slot) else {
            return;
        };
        let (knockdown, getup) = (map[2], map[3]);
        let Some(a) = self.actors.get(slot) else {
            return;
        };
        if getup == 0
            || clip != getup
            || knockdown == getup
            || a.battle_reaction != Some(4)
            || a.battle_reaction_entry != Some(knockdown)
        {
            return;
        }
        self.finish_battle_reaction(slot, 4);
        // The passes retail ran since then ran the presentation tick's arm 2
        // on the body each time (`0x80050230..0x80050244`): blend `0x1000`
        // and `0x81000000` in the mode word, the additive draw the capture
        // shows. Its lanes hold while the get-up plays (the arm's fade gate
        // compares the committed entry against the knockdown).
        if let Some(a) = self.actors.get_mut(slot)
            && a.battle.render_flag == legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE
        {
            a.battle.render_blend = legaia_engine_battle_vm::battle_formulas::TINT_BLEND_FULL;
            a.battle.flag_word |= 0x8100_0000;
        }
    }

    /// Land battle slot `slot`'s playing clip on the node cursor `+0x68` a
    /// capture held (12.4 sixteenths of a keyframe), re-posing it there -
    /// when the slot plays the clip the capture's `+0x1D9` names. Returns
    /// whether it landed. A capture seat: the replay reaches the capture's
    /// phase on its own clock, so an idle loop stands at whatever phase that
    /// clock left it rather than retail's. `cursor` may sit below zero (the
    /// displayed frame from before the loop's last wrap).
    pub fn seat_battle_clip_cursor(&mut self, slot: usize, clip: u8, cursor: i32) -> bool {
        // A party member the capture holds downed plays entry `8`, the kneel
        // that re-commits itself at every natural end
        // ([`legaia_engine_battle_loop::clip_ladder::PARTY_DOWNED_LOOP_ENTRY`]).
        // A seat lands its HP at zero without the fall that led there, so
        // the body would stand on its idle: put it on the kneel first.
        let downed = clip == legaia_engine_battle_loop::clip_ladder::PARTY_DOWNED_LOOP_ENTRY
            && self
                .actors
                .get(slot)
                .is_some_and(|a| a.battle_monster_id.is_none() && a.battle.hp == 0);
        if downed && !self.seat_downed_kneel(slot) {
            return false;
        }
        self.seat_knockdown_end(slot, clip);
        let Some(a) = self.actors.get_mut(slot) else {
            return false;
        };
        // A struck body plays its reaction entry on the reaction channel
        // (`+0x1D9` holds the committed entry, `World::commit_battle_reaction_entry`):
        // the capture's `+0x1D9` names it, not the staged-anim id.
        let reacting = !downed && a.battle_reaction_entry == Some(clip);
        if !downed && !reacting && a.battle.current_anim != clip {
            return false;
        }
        let Some(player) = a.battle_animation.as_mut() else {
            return false;
        };
        // Only a looping clip runs free of the phase the seed placed: a
        // one-shot (a swing, a reaction) started with its action, which the
        // seed already put where the capture's was. A looping clip wraps
        // (retail's re-commit zeroes the cursor at the stream's end), and so
        // does the self-re-committing kneel.
        //
        // A reaction is the one one-shot seated: it starts on the hit, which
        // the replay lands on its own clock, and the frame on screen is two
        // game frames older than the RAM's cursor - a knockdown moves the
        // whole body in that time (`battle_gimard_tail_fire_b`'s Vahn, still
        // upright mid-fall on the displayed frame at `72` less two frames'
        // advance, sprawled on the RAM's).
        if !player.is_looping() && !downed && !reacting {
            return false;
        }
        let total = (player.frame_count() * 16).max(1) as i32;
        let cursor = cursor.rem_euclid(total);
        a.pose_frame = Some(player.seat_cursor(cursor as u16));
        step_model_table(
            &mut a.battle_model_table,
            &mut a.battle_drawn_objects,
            player.variant_track(),
            player.current_frame(),
        );
        true
    }

    /// Pose battle slot `slot` on action clip `clip` at node cursor `cursor`
    /// (12.4 sixteenths of a keyframe) whatever clip it plays: the frame a
    /// capture's display shows, when the anim node committed another clip
    /// between that frame and the RAM's. The playing clip is swapped for
    /// the shown one with its cursor seated, the staged-anim and action
    /// state left as they are. A capture seat for the frame it is taken on,
    /// not a commit: no clip-tag ladder, no root step, no effect script.
    /// Returns whether it landed (`false` with no such clip).
    pub fn seat_displayed_battle_clip(&mut self, slot: usize, clip: u8, cursor: i32) -> bool {
        let Some(a) = self.actors.get_mut(slot) else {
            return false;
        };
        let Some(anim) = a
            .battle_action_clips
            .as_ref()
            .and_then(|c| c.get(usize::from(clip)))
            .and_then(|c| c.as_ref())
        else {
            return false;
        };
        let Some(mut player) = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(anim)
        else {
            return false;
        };
        let total = (player.frame_count() * 16).max(1) as i32;
        a.pose_frame = Some(player.seat_cursor(cursor.rem_euclid(total) as u16));
        step_model_table(
            &mut a.battle_model_table,
            &mut a.battle_drawn_objects,
            player.variant_track(),
            player.current_frame(),
        );
        a.battle_animation = Some(player);
        true
    }

    /// Advance the per-object battle animation of every actor carrying one,
    /// folding the result into `pose_frame`. The battle render path then
    /// deforms each actor's mesh through `tmd_to_vram_mesh_posed_rot`. Call once
    /// per battle frame (the field [`tick_actors`](Self::tick_actors) drives the
    /// ANM path instead). Unlike `tick_actors` this does not gate on `.active`,
    /// since battle-init actors keep their `tmd_binding` without the field
    /// `.active` flag.
    pub fn tick_battle_animations(&mut self) {
        self.tick_battle_animations_walk(false);
    }

    /// [`Self::tick_battle_animations`], with each actor's hit walk and
    /// event-path commit (`World::tick_battle_hit_events_seat`) run inside
    /// its own slice of the walk when `hits` is set - retail's order, where
    /// both sit in the node's own tick `FUN_80047430` right after its
    /// advance (`0x8004787C..0x80047A44`). Returns the clip commits each
    /// actor's own slice landed (advance, natural end, staged commit, its
    /// own hit walk), per actor, against the hit walks' commits on other
    /// seats.
    pub(in crate::world) fn tick_battle_animations_walk(&mut self, hits: bool) -> Vec<bool> {
        let start: Vec<u32> = self.actors.iter().map(|a| a.battle_clip_commits).collect();
        let mut foreign = vec![0u32; self.actors.len()];
        if self.battle.anim_slice_commits.len() != self.actors.len() {
            self.battle.anim_slice_commits = start.clone();
        }
        // The vsyncs this pass spans (`World::battle_anim_pass_vsyncs`):
        // `1` on the per-vsync cadence and from a direct call.
        let vsyncs = self.battle.anim_pass_vsyncs.max(1);
        // Commit any anim ids the SM staged this frame (idempotent - the
        // step_battle pre-step commit already handled last frame's stages).
        // On retail's cadence the commit follows the advance instead: every
        // commit `FUN_80047430` makes (the bit-0 commit-now, the bit-1 event
        // cut, the natural end) sits after the cursor advance
        // (`0x800478EC..0x80047A44`), and `FUN_8004AD80` zeroes the cursor
        // (`sh zero,0x68(s5)` at `0x8004B068`), so a clip the SM staged
        // starts the next pass at its first keyframe.
        let commit_after = self.toggles.battle_sm_frame_cadence;
        if !commit_after {
            self.commit_staged_battle_anims();
        }
        // Per-clip impact freeze/tint arms + the tint's neutral decay
        // (`FUN_8004CE2C` pass 2 / `FUN_80050120` arm 0) - before the
        // cursor advance so an in-window freeze stops THIS frame's step,
        // like retail's tick order (the maintenance sweep runs off the
        // XA-template tick ahead of the anim node advance). Their counters
        // are written in vsyncs, so a pass runs one per vsync it spans.
        for _ in 0..vsyncs {
            self.tick_battle_impact_fx();
        }
        // The same pass's Mystic Shield pose and break
        // (`0x8004D534..0x8004D674`): once a pass - it counts nothing, it
        // seats the sphere and tests the break.
        if self.mode == SceneMode::Battle {
            self.tick_dohati_growth();
            self.tick_mystic_shield();
        }
        let mut fresh = vec![false; self.actors.len()];
        for i in 0..self.actors.len() {
            fresh[i] = self.actors[i].battle_clip_commits
                != self.battle.anim_slice_commits.get(i).copied().unwrap_or(0);
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
                    && b.flag_bits
                        .has(legaia_engine_battle_vm::battle_action::ActorFlags::ADVANCE_DONE)
            });
            if restaged {
                self.commit_staged_battle_anim_at_boundary(i);
            } else if let Some(id) = staged_done {
                let a = &mut self.actors[i];
                a.battle_staged_anim = None;
                a.battle
                    .flag_bits
                    .clear(legaia_engine_battle_vm::battle_action::ActorFlags::ADVANCE_DONE);
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
            self.restore_idle_after_action_clip(i);
            // A Stone or Numb body on its idle stands frozen: the tick skips
            // the whole advance (`0x80047638..0x800476D4`, jumping to the
            // epilogue at `0x800480B0`).
            // PORT: FUN_80047430 (`0x80047638..0x800476D4`, the frozen idle)
            let frozen_idle = self.raw_status_word(i as u8)
                & legaia_engine_battle_vm::battle_anim_rate::FROZEN_IDLE_STATUS_BITS
                != 0
                && self.actors[i].battle.current_anim == 0
                && self.actors[i].battle_reaction.is_none();
            if frozen_idle {
                self.finish_battle_node_slice(i, commit_after, hits, &mut foreign);
                continue;
            }
            // A seat past the party with a zero colour word does not tick at
            // all (`lw v0,0x74(s5)`, the low 24 bits, `beq v0,zero,0x800480B0`
            // at `0x800474D4..0x800474E0`): no advance, no commit, no clip
            // cue. A row parked under the hide holds its clip with no sound
            // (Palma's flinch), and a creature hidden on its seat holds its
            // first frames until a module shows it (Terra's arms 3..6).
            // PORT: FUN_80047430 (`0x800474D4..0x800474E0`, the zero-colour skip)
            if (self.actors[i].battle_monster_id.is_some()
                || self.casting.summon_actor_slot == Some(i as u8))
                && self.actors[i]
                    .battle_node_colour_zero
                    .unwrap_or_else(|| self.battle_node_colour_is_zero(i))
            {
                continue;
            }
            self.stamp_rage_delegation(i);
            // The decoder's last-frame tween reads the clip queued behind
            // this one; recompute it before the step, like the draw that
            // follows the tick in retail.
            let tween = self.battle_tween_target(i);
            let seru_staged = self.battle_ctx.absorbed_seru != 0;
            static CREATURE_DIAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            let creature_diag = *CREATURE_DIAG
                .get_or_init(|| std::env::var_os("LEGAIA_DIAG_CREATURE_ANIM").is_some())
                && self.casting.summon_actor_slot == Some(i as u8);
            let creature_channel = self
                .casting
                .creature_channel_object
                .filter(|_| self.casting.summon_actor_slot == Some(i as u8));
            let module_phase = self.casting.module_phase;
            let mirrors_loop = self.mirrors_loop_counter(i);
            // A clip committed on this seat since its own slice of the last
            // walk (the SM's pass, a cast module's seat write, another
            // seat's hit walk) is, in retail, only staged there - `+0x1DA`
            // with `+0x1DC` bit 0 (the damage arm, `FUN_800402F4`
            // `0x80042118..0x80042170`; the module bump) - and this seat's
            // own tick advances first and then commits it, zeroing the
            // cursor (`sh zero,0x68` at `0x8004B068`), so the clip starts the
            // next pass on its first keyframe. The engine installed it
            // already, so this pass's advance is skipped.
            let fresh_from_hit = commit_after && fresh.get(i).copied().unwrap_or(false);
            // The `>> 2` branch of the cursor advance is taken only by a
            // **Slowed** actor on idle: `andi v0,v0,0x1000` on `+0x16E` at
            // `0x800476E0`, then `+0x1D9 == 0` at `0x800476EC`. Every other
            // actor - idle included - advances on the `>> 1` branch.
            let slowed = self.raw_status_word(i as u8)
                & legaia_engine_battle_vm::battle_anim_rate::SLOW_STATUS_BIT
                != 0;
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
                let rate = if fresh_from_hit {
                    legaia_engine_battle_vm::battle_anim_rate::AnimRate(
                        legaia_engine_battle_vm::battle_anim_rate::RATE_FROZEN,
                    )
                } else {
                    actor.battle.anim_rate
                };
                let slowed_idle =
                    slowed && actor.battle.current_anim == 0 && actor.battle_reaction.is_none();
                let mut pose = player.tick_rated_vsyncs(rate, slowed_idle, vsyncs);
                // The decoder fills the pose pool `ctx+0x6F4` as far as the
                // stream's part count and the draw reads an entry per mesh
                // object: a clip with fewer parts than the model leaves its
                // extra objects on the last pose written (Ozma's clip 3 has 15
                // parts over 21 objects, and a RAM image's entries 15..20 hold
                // still across the clip).
                if let Some(prev) = actor.pose_frame.as_ref() {
                    let n = pose.bone_outputs.len();
                    if let Some(rest) = prev.bone_outputs.get(n..) {
                        pose.bone_outputs.extend_from_slice(rest);
                    }
                }
                // The channel a module's table store moved an object onto
                // places that object, and the object's own channel stands past
                // the stream.
                if let Some((channel, object)) = creature_channel
                    && let Some(&placed) = pose.bone_outputs.get(usize::from(channel))
                    && let Some(slot) = pose.bone_outputs.get_mut(usize::from(object))
                {
                    *slot = placed;
                }
                let after = player.current_frame();
                // The window arm's counter store (`actor+0x21B = +0x176 >> 4`,
                // `0x800477E8` / `0x8004783C`) - mirrored for the summon
                // creature, whose `+0x21B` the cast modules read.
                // PORT: FUN_80047430 (`0x80047768..0x8004783C`, the `+0x21B` store)
                if player.take_loop_store() && mirrors_loop {
                    actor.battle.hit_count_bound = player.loop_cycles_remaining();
                }
                // `LEGAIA_DIAG_CREATURE_ANIM=1`: one line per tick for the
                // summon creature's clip, rate and frame.
                if creature_diag {
                    eprintln!(
                        "[creature-anim] slot={i} phase={} cur={} queued={} rate={} frame={after} pos={:?} colour={:#x} tint={} facing={}",
                        module_phase,
                        actor.battle.current_anim,
                        actor.battle.queued_anim,
                        rate.get(),
                        [
                            actor.move_state.world_x,
                            actor.move_state.world_y,
                            actor.move_state.world_z
                        ],
                        actor.battle.render_color,
                        actor.battle.render_flag,
                        actor.battle.facing_angle,
                    );
                }
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
                        .has(legaia_engine_battle_vm::battle_action::ActorFlags::FX_SUPPRESSED);
                    if step != 0 && !latched && (actor.battle.hp != 0 || seru_staged) {
                        let (sin, cos) =
                            legaia_engine_system::trig::trig12(actor.battle.facing_angle);
                        let (dx, dz) =
                            legaia_engine_battle_vm::battle_action::motion::end_root_step(
                                sin, cos, step,
                            );
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
                    legaia_engine_battle::battle_afterimage::party_ring_id(
                        actor.battle.current_anim,
                    )
                } else {
                    legaia_engine_battle::battle_afterimage::monster_ring_id(
                        clip_key,
                        player.solo_flag(),
                    )
                };
                let ghost_eligible =
                    legaia_engine_battle::battle_afterimage::ghost_eligible(ring_id);
                actor.battle_pose_history.push_front(BattleGhostFrame {
                    pose: pose.clone(),
                    pos: [
                        i32::from(actor.move_state.world_x),
                        i32::from(actor.move_state.world_y),
                        i32::from(actor.move_state.world_z),
                    ],
                    ghost_eligible,
                    clip_key,
                    variant_track: player.variant_track(),
                    frame: after,
                });
                actor
                    .battle_pose_history
                    .truncate(BATTLE_POSE_HISTORY_SLOTS);
                actor.pose_frame = Some(pose);
                // The mesh-variant swap, run off the same node cursor right
                // after the facial animator at the end of the tick
                // (`FUN_80047430` `0x80048010..0x80048030`, frame argument
                // `(node[+0x68] << 16) >> 20`).
                step_model_table(
                    &mut actor.battle_model_table,
                    &mut actor.battle_drawn_objects,
                    player.variant_track(),
                    after,
                );
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
                // The natural-end re-commit re-seeds the window
                // (`0x8004BDEC..0x8004BE0C` sits on every commit's path).
                if recommit {
                    // The commit zeroes the node cursor (`sh zero,0x68` at
                    // `0x8004B068`, on every path through `FUN_8004AD80`), so
                    // the advance's overshoot past the clip's end is dropped
                    // and the next cycle starts its next pass on keyframe 0.
                    // `tunnelc_freed_water_crystals_mid_cast`'s Vahn wraps his
                    // rate-7 idle from `138` to `0`; carried over, the engine's
                    // stood `10` ahead and its knockdown ran ahead with it.
                    // PORT: FUN_8004AD80 (`0x8004B068`, the cursor zero on the
                    // natural-end re-commit)
                    player.rewind();
                    player.reseed_loop_window();
                    if mirrors_loop {
                        actor.battle.hit_count_bound = player.entry_loop_count();
                    }
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
                // The commit's rate decay runs on this path too
                // (`0x8004B080..0x8004B090`): a seat at any rate but `8` loops
                // at `4`, so a summon creature seated at `2` plays its idle at
                // twice the pace from its first wrap (capture: Aluru's arm 9).
                self.actors[i].battle.anim_rate =
                    legaia_engine_battle_vm::battle_anim_rate::commit_rate_decay(
                        self.actors[i].battle.anim_rate,
                    );
                self.actors[i].battle.input_cursor = 0;
                // The commit's cue-track cursor seed (`sb zero,0x1f6` at
                // `0x8004AFD0`, then the juggled re-hit's `1`): a walk's
                // second and later cycles re-fire their footfall cues. The
                // juggle byte is still the last pass's here, as at retail's
                // commit (`+0x1F7` is written after it, `0x80047E28`).
                let cue_cursor = self.battle_reaction_map(i).map_or(0, |map| {
                    let a = &self.actors[i];
                    let entry = self.battle_current_anim(i);
                    let first_cue = a
                        .battle_effect_script
                        .as_ref()
                        .and_then(|s| s.get(0x56..0x58))
                        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                    legaia_engine_battle_loop::clip_ladder::recommit_cue_cursor(
                        map,
                        entry,
                        entry,
                        a.battle_juggle_window,
                        a.battle_monster_id.is_none(),
                        first_cue,
                    )
                });
                self.actors[i].battle_anim_cue_cursor = cue_cursor;
                self.note_active_clip_commit(i);
                // The natural-end commit installs whatever `+0x1DA` holds:
                // a byte staged over the summon creature's loop waits here
                // ([`Self::summon_seat_waits_for_cycle`]).
                // So does a byte whose bit-0 cut the loop's `+0x76` lock
                // refused ([`Self::loop_cut_locked`]).
                // And the Spirit band's stage
                // ([`Self::spirit_stage_waits_for_cycle`]).
                if (self.summon_seat_waits_for_cycle(i)
                    || self.loop_cut_locked(i)
                    || self.spirit_stage_waits_for_cycle(i))
                    && self.actors[i].battle.queued_anim != self.actors[i].battle.current_anim
                {
                    self.commit_staged_battle_anim_at_boundary(i);
                    // This tick's pose is the old clip wrapped to frame 0, which
                    // retail's commit never decodes. A new clip shorter than the
                    // model takes its extra objects from the pose it keeps, so
                    // keep the one before the wrap (Ozma's shell halves stay
                    // folded on the shoulders through its 15-part clips).
                    let a = &mut self.actors[i];
                    if let Some(before) = a.battle_pose_history.get(1).map(|f| f.pose.clone()) {
                        a.pose_frame = Some(before);
                    }
                }
            }
            // `+0x1F7`, written for every node right after its cursor
            // advance (`0x80047E28..0x80047E54`).
            // PORT: FUN_80047430 (`0x80047E1C..0x80047E54`, the juggle window)
            self.actors[i].battle_juggle_window = Self::juggle_window_open(&self.actors[i]);
            // Per-frame effect-script walk for the committed record - the
            // engine seat of retail's `FUN_80047430` -> `FUN_801DEA50` call
            // pair (frame argument = the node's 12.4 anim cursor in whole
            // keyframes, which is `MonsterAnimPlayer::current_frame`).
            // Retail's walks run in the order hit, effect script, cue track
            // (`0x800478A0`, `0x800478B8`, `0x800478E4`). A blocked hit steps its
            // attacker's cue cursor over the impact cue (`0x801EE6EC`), so the
            // hit goes first where the slice has no commit to make ahead of it:
            // an impact cue on the hit's own keyframe would otherwise sound on a
            // block (Nova's clip 8).
            let hit_first =
                hits && frame.is_some() && !(commit_after && self.battle_commit_may_land(i));
            let commits = self.actors[i].battle_clip_commits;
            if hit_first {
                self.walk_battle_node_hits(i, &mut foreign);
            }
            // A hit that committed its own attacker's next clip leaves the walk
            // to the post-commit re-walk.
            let cut = self.actors[i].battle_clip_commits != commits;
            if let Some(frame) = frame.filter(|_| !cut) {
                self.step_actor_effect_script(i, frame);
                // ... and its third call, the animation cue track
                // (`FUN_800508DC`, same arguments).
                self.step_actor_anim_cues(i, frame);
            }
            self.finish_battle_node_slice(i, commit_after, hits && !hit_first, &mut foreign);
        }
        // The cast tick's effect-child drive. On retail's cadence it runs
        // with the pass's draw instead (`World::tick_battle_anim_pass`).
        if !commit_after {
            self.tick_battle_cast_drive(vsyncs);
        }
        self.latch_battle_node_colours();
        self.actors
            .iter()
            .enumerate()
            .map(|(k, a)| {
                let moved = a
                    .battle_clip_commits
                    .wrapping_sub(start.get(k).copied().unwrap_or(a.battle_clip_commits));
                moved > foreign.get(k).copied().unwrap_or(0)
            })
            .collect()
    }

    /// The render half of the pass for each battle body's node colour word:
    /// the dispatcher `FUN_8001ADA4` calls the draw tick `FUN_800480D8` only
    /// for a node at view depth `0xA1` or deeper (`slti v0,v0,0xa1` at
    /// `0x8001AE9C` / `0x8001AEE8`), and only the draw tick runs the tint pass
    /// that writes `+0x74`. A body the camera leaves behind keeps the word it
    /// last drew with, so a monster a module hides there ticks on. Water
    /// Crystals' arm 2 hides Freed (`+0x04 = 0`, `+0x21C = 0xFF`) under a cut
    /// to the victim; a retail recording's watch on Freed's node holds its
    /// last `+0x74` store from arm 1 to arm 5's show, and its idle cursor
    /// walks on throughout, where the hide alone froze the engine's body.
    // PORT: FUN_8001ADA4 (`0x8001AE94..0x8001AEF4`, the near reject ahead of the draw tick)
    // REF: FUN_800480D8 (the draw tick whose tint pass writes `+0x74`)
    pub(in crate::world) fn latch_battle_node_colours(&mut self) {
        if self.mode != SceneMode::Battle {
            return;
        }
        let pose = (self.battle.camera.is_some() || self.battle.stage_camera.is_some())
            .then(|| self.battle_cam_pose());
        for i in 0..self.actors.len() {
            let drawn_deep = self
                .battle_actor_draw_plan(
                    i,
                    pose.as_ref(),
                    legaia_engine_camera::projection::BATTLE_WORLD_SCALE,
                    false,
                )
                .map(|p| p.view_z >= legaia_engine_battle_vm::battle_actor_tick::NEAR_REJECT_DEPTH);
            if drawn_deep == Some(true) {
                let zero = self.battle_node_colour_is_zero(i);
                self.actors[i].battle_node_colour_zero = Some(zero);
            }
        }
    }

    /// The tail of actor `i`'s slice of the anim walk on retail's cadence:
    /// its staged commit, then (with `hits`) its hit walk and event-path
    /// commit. Retail makes every commit of a node inside that node's own
    /// tick, after its advance, and the actor list walks the nodes in seat
    /// order (`FUN_8002519C` calling each node's `+0x0C` handler
    /// `FUN_80047430`; `minigame_muscle_dome_retail_skeleton_hyper_arts`
    /// lists Vahn's node ahead of the Skeleton's). So a commit's
    /// battle-wide rate arm reaches a later seat in the same pass: Vahn's
    /// event-path cut into the SpecialStarter `0x1A` freezes every slot
    /// (`0x8004B728..0x8004B750`) before the Skeleton's advance that pass,
    /// and its reaction holds keyframe 3 (cursor `48`, the capture's
    /// history ring) where cutting after the whole walk let it reach
    /// keyframe 4. `foreign` counts, per seat, the commits a hit walk
    /// landed on a seat other than the walker.
    // PORT: FUN_80047430 (the per-node hit walk and commits after the advance)
    fn finish_battle_node_slice(
        &mut self,
        i: usize,
        commit_after: bool,
        hits: bool,
        foreign: &mut [u32],
    ) {
        if commit_after {
            let reaction_done = self.finish_reaction_at_natural_end(i);
            self.commit_staged_battle_anim(i);
            if reaction_done {
                self.restore_idle_after_action_clip(i);
            }
        }
        if hits {
            self.walk_battle_node_hits(i, foreign);
        }
        if let (Some(a), Some(c)) = (
            self.actors.get(i),
            self.battle.anim_slice_commits.get_mut(i),
        ) {
            *c = a.battle_clip_commits;
        }
        // The root-motion term this seat's tick steps: the entry it holds
        // once its own commits are made (`BattleState::anim_slice_roots`).
        if commit_after {
            if self.battle.anim_slice_roots.len() != self.actors.len() {
                self.battle.anim_slice_roots = vec![(None, 0); self.actors.len()];
            }
            let root = (
                self.battle_playing_root_motion(i),
                self.actors[i].battle.facing_angle,
            );
            self.battle.anim_slice_roots[i] = root;
        }
    }

    /// A reaction clip whose cursor crossed its end on this pass takes the
    /// natural-end path now, in the slice of the advance that finished it: the
    /// node tick commits after the advance (`0x80047A48..0x80047B58`), so the
    /// next entry stands on its first keyframe the pass the old clip ended, and
    /// state `0x20`'s reaction hold reads the idle on the pass after. The check
    /// at the head of the next slice held every reaction a pass longer on its
    /// last frame, and each strike's Done band with it.
    ///
    /// PORT: FUN_80047430 (`0x80047A48..0x80047B58`, the natural end)
    fn finish_reaction_at_natural_end(&mut self, i: usize) -> bool {
        let finished =
            self.actors
                .get(i)
                .and_then(|a| match (a.battle_reaction, &a.battle_animation) {
                    (Some(tag), Some(p)) if p.finished() => Some(tag),
                    _ => None,
                });
        let Some(tag) = finished else {
            return false;
        };
        self.finish_battle_reaction(i, tag);
        true
    }

    /// A finished one-shot falls back to the idle loop, except defeat, which
    /// holds its final (downed) keyframe.
    fn restore_idle_after_action_clip(&mut self, i: usize) {
        let restore = self.actors.get(i).is_some_and(|a| {
            a.battle_action_clips.is_some()
                && a.battle_reaction.is_none()
                && a.battle_pose != Some(legaia_engine_battle_vm::battle_action::Pose::Defeat as u8)
                && matches!(&a.battle_animation, Some(p) if p.finished())
        });
        if restore {
            self.apply_battle_pose(i, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
        }
    }

    /// [`Self::commit_staged_battle_anim`] could install a clip on this seat now:
    /// a staged clip differs from the playing one and no one-shot holds it off.
    fn battle_commit_may_land(&self, i: usize) -> bool {
        self.actors.get(i).is_some_and(|a| {
            a.battle.queued_anim != a.battle.current_anim
                && !(a.battle_staged_anim.is_some()
                    && a.battle_animation
                        .as_ref()
                        .is_some_and(|p| !p.finished() && !p.is_looping()))
        })
    }

    /// One seat's hit walk inside its slice; a commit it made on another seat
    /// is that seat's foreign commit, not its own slice's.
    fn walk_battle_node_hits(&mut self, i: usize, foreign: &mut [u32]) {
        if let Some(before) = self.tick_battle_hit_events_seat(i) {
            for (k, b) in before.into_iter().enumerate() {
                if k != i
                    && let (Some(a), Some(f)) = (self.actors.get(k), foreign.get_mut(k))
                {
                    *f += a.battle_clip_commits.wrapping_sub(b);
                }
            }
        }
    }

    /// The cast tick's effect-child drive (`FUN_801E09F8`, after its census):
    /// the move-FX streak's counter walk and the homing slots, once per
    /// vsync of `vsyncs`. The cast tick is the draw's (`jal 0x801E09F8` in
    /// the battle draw tick `FUN_800480D8`), so on retail's cadence it runs
    /// at the tail of the anim pass (`World::tick_battle_anim_pass`); per
    /// vsync it stays at the anim tick's tail too.
    pub(in crate::world) fn tick_battle_cast_drive(&mut self, vsyncs: u8) {
        // On retail's cadence the drive is one call a battle frame with the
        // frame step folded into every counter and step
        // (`HomingSlots::step_pass`), not `step` one-vsync calls: the
        // counters fall `4 * step` and are tested once, so a re-armed list
        // spawn or a landing waits for the next pass.
        if self.toggles.battle_sm_frame_cadence {
            if vsyncs > 0 {
                let streak_before = self.casting.move_fx_streak.counter_word;
                self.casting.move_fx_streak.tick_counter_by(vsyncs);
                self.tick_homing_slots_pass(vsyncs, streak_before);
            }
            return;
        }
        for _ in 0..vsyncs {
            // The move-FX streak counter walk (retail `FUN_801E09F8` phase
            // 1): `ctx[+0x6C6]` falls 4 per frame, shrinking the trail's
            // half-width and scheduling the afterimage -> ribbon emitter
            // handoff.
            self.casting.move_fx_streak.tick_counter();
            self.tick_homing_slots();
        }
    }

    /// The track coin a capture's strike deal holds for a commit now
    /// (`BattleState::capture_track_coin`): the value retail's recording
    /// stood on [`DEALT_COIN_LOOKAHEAD`] display frames on, so a commit a
    /// few frames ahead of retail's still reads retail's draw. `None` outside
    /// that oracle; the draw on the world stream stands.
    fn dealt_track_coin(&self) -> Option<u8> {
        let at = self.clock.display_frames + DEALT_COIN_LOOKAHEAD;
        self.battle
            .capture_track_coin
            .iter()
            .take_while(|&&(f, _)| f <= at)
            .last()
            .map(|&(_, c)| c)
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
        // A creature byte staged with the `+0x1DC` bump (PROT 0907's arm 8,
        // `0x801F7A44..0x801F7A5C`) takes bit 0, which cuts the playing clip
        // at once (`0x800478F4`) - unless that clip's entry carries the
        // `+0x76` lock (`0x80047940..0x80047948`), which leaves the byte to
        // the natural end like a bare `+0x1DA` store.
        let cut_now = bits.has(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE)
            && actor
                .battle_animation
                .as_ref()
                .and_then(|p| p.hit_source())
                .is_none_or(|s| s.event_lock == 0);
        if (self.summon_seat_waits_for_cycle(i)
            || self.loop_cut_locked(i)
            || self.spirit_stage_waits_for_cycle(i))
            && !cut_now
            && actor
                .battle_animation
                .as_ref()
                .is_some_and(|p| p.is_looping())
        {
            return;
        }
        if !bits.has(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE)
            && bits.has(legaia_engine_battle_vm::battle_action::ActorFlags::ADVANCE_DONE)
            && let Some(p) = actor.battle_animation.as_ref()
            && p.is_looping()
        {
            let (frames, lock) = p
                .hit_source()
                .map_or(([0; 4], 0), |s| (s.event_frames, s.event_lock));
            if !legaia_engine_battle_vm::battle_action::event_commit_due(
                &frames,
                Self::committed_four_slot_beat(actor),
                lock,
                p.current_frame(),
            ) {
                return;
            }
        }
        self.commit_staged_battle_anim_at_boundary(i);
    }

    /// Whether actor `i`'s byte is staged under the `+0x1DC` bit-0 "commit
    /// now" over a looping clip whose entry carries the `+0x76` lock: the
    /// tick's cut refuses it (`lbu v0,0x76(s3); bne v0,zero,0x80047a48` at
    /// `0x80047940..0x80047948`), so the byte waits for the loop's natural
    /// end (`0x80047B54`). The approach poll's arrival stages idle under bit
    /// 0 (`0x801E35BC..0x801E35C8`); over a locked walk, retail plays the
    /// walk on into the strike loop (`battle_retona_king_frog_strike` reads
    /// `0x1E` on clip `1` with the accumulator `168` into its cycle).
    pub(in crate::world) fn loop_cut_locked(&self, i: usize) -> bool {
        let Some(a) = self.actors.get(i) else {
            return false;
        };
        a.battle
            .flag_bits
            .has(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE)
            && a.battle_animation.as_ref().is_some_and(|p| {
                p.is_looping() && p.hit_source().is_some_and(|s| s.event_lock != 0)
            })
    }

    /// The commit body, run at a clip boundary (see
    /// [`Self::commit_staged_battle_anim`]). With the staged byte equal to
    /// the committed id this is retail's **re-commit** of a still-queued
    /// clip: the same entry is re-installed with its cursor and hit index
    /// zeroed (`0x8004B064..0x8004B068`) - the port rewinds the in-flight
    /// player in place. Either way the stage latch `+0x1DC` bit 1
    /// (`ADVANCE_DONE`) clears, which is what lets the strike loop read its
    /// next byte.
    /// Whether actor `i`'s `+0x21B` follows its clip's loop counter: the
    /// summon creature's, which the cast modules read (PROT 0904's arm 7,
    /// PROT 0907's arm 10). The same byte on the other seats carries
    /// readers of its own (the magic hit loop's bound, the battle camera's
    /// arm selector) that the port still feeds separately, so the mirror
    /// stays on the creature.
    pub(in crate::world) fn mirrors_loop_counter(&self, i: usize) -> bool {
        self.casting.summon_actor_slot == Some(i as u8)
    }

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
            Some(seq)
                if seq.cause != legaia_engine_battle_vm::battle_action::BattleEndCause::Escaped =>
            {
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
        // The commit's tail: a strike clip the acting actor commits in the
        // strike loop stands it on the edge of its reach.
        // REF: FUN_8004AD80 (`0x8004BF7C..0x8004C118`)
        if i == usize::from(self.battle_ctx.active_actor)
            && self.battle_ctx.action_state
                == legaia_engine_battle_vm::battle_action::ActionState::AttackChain.as_byte()
        {
            self.commit_reach_snap(i);
        }
    }

    // PORT: FUN_8004AD80 (the commit body; boundary selection is the tick's)
    pub(in crate::world) fn commit_staged_battle_anim_at_boundary(&mut self, i: usize) {
        use legaia_engine_battle_vm::battle_action::ActorFlags;
        use vm::anim_vm::{StagedAnimTarget, resolve_staged_anim};
        let Some(actor) = self.actors.get_mut(i) else {
            return;
        };
        let q = actor.battle.queued_anim;
        // The commit prologue's Arts-banner cancel (`0x8004ADBC..0x8004ADE8`,
        // ahead of every other write in the routine, idle re-commit included):
        // a banner in flight whose own actor commits again moves into the
        // `5..=8` retire band and restarts its clock.
        // REF: FUN_8004AD80
        if let Some((stage, level)) =
            legaia_engine_battle_vm::battle_action::banner_cancel_on_commit(
                self.battle_ctx.arts_banner_stage,
                i as u8,
                self.battle_ctx.active_actor,
            )
        {
            self.battle_ctx.arts_banner_stage = stage;
            self.battle_ctx.arts_banner_level = level;
        }
        // The sustained-cue teardown (`lbu v1,0x1db(s1)` / `lbu v0,0x1da(s1)`
        // / `beq` / `jal 0x80017910` at `0x8004AE54..0x8004AE68`): a commit
        // whose staged clip differs from the latched one keys off the held
        // sustained SFX run, whichever actor keyed it. This is what ends a
        // looping attack sound when its clip gives way to the next; a
        // re-commit of the same clip leaves it sounding.
        // REF: FUN_80017910
        if self.actors[i].battle.latched_anim != q {
            self.audio.sfx_ring_ops.push(SfxRingOp::ReleaseSustained);
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
                && b.action_category
                    == legaia_engine_battle_vm::battle_action::ActionCategory::Attack.as_byte()
                && b.latched_anim != 0x1A
            {
                self.battle.move_label_closed = true;
            }
        }
        // The monster move-name label (`0x8004AEBC..0x8004AF88`), also ahead
        // of the re-commit return: a monster seat's outgoing clip whose entry
        // `+0x77` is non-zero destroys widget `0x21`, and an incoming clip
        // whose `+0x77` is non-zero opens element `0x4C` on that spell id's
        // name. Every commit runs it, the hand-off to idle included.
        // REF: FUN_8004AD80
        if self.actors[i].battle_monster_id.is_some() {
            let key = |clip: u8| {
                self.actors[i]
                    .battle_action_clips
                    .as_ref()
                    .and_then(|cl| cl.get(clip as usize))
                    .and_then(|c| c.as_ref())
                    .map_or(0, |c| c.attach_key)
            };
            let outgoing = key(self.actors[i].battle.current_anim);
            let incoming = key(q);
            if outgoing != 0 {
                self.battle.monster_move_label = None;
            }
            if incoming != 0 {
                self.battle.monster_move_label = Some(incoming);
            }
        }
        let mirrors_loop = self.mirrors_loop_counter(i);
        let actor = &mut self.actors[i];
        if q == actor.battle.current_anim {
            // A re-commit passes the rate decay as well (`0x8004B080`, ahead of
            // the install arms).
            actor.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::commit_rate_decay(
                actor.battle.anim_rate,
            );
            if let Some(p) = actor.battle_animation.as_mut() {
                p.rewind();
                // The seed (`0x8004BDEC..0x8004BE0C`) runs on a re-commit too.
                p.reseed_loop_window();
                if mirrors_loop {
                    actor.battle.hit_count_bound = p.entry_loop_count();
                }
            }
            actor.battle_effect_cursor = 0;
            actor.battle_anim_cue_cursor = 0;
            actor.battle.input_cursor = 0;
            actor.battle_clip_commits = actor.battle_clip_commits.wrapping_add(1);
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
        // An expansion pack art's clip latches the retail constant it
        // borrows (identity for every retail byte and with no pack).
        let latch = self.expansion_art_latch(i, q);
        self.actors[i].battle.latched_anim = latch;
        // The arts slow-motion arms (`FUN_8004AD80`; kernel
        // `legaia_engine_battle_vm::battle_anim_rate`). Order is retail's: the
        // unconditional decay first (`0x8004B080` - a non-normal actor's
        // committing clip rises to half speed), then the staged-id arms.
        // The SpecialStarter (`0x1A`) freezes every slot and puts the
        // acting actor at quarter speed; an art constant (`>= 0x1B`) drops
        // the whole battle to half speed (quarter under an armed
        // `ctx[+0x243]`). The restore back to normal is the SM's Done arm
        // (`FUN_801E93C8` via `battle_gauge_rearm::restore_anim_rates`).
        {
            use legaia_engine_battle_vm::battle_anim_rate as rl;
            let decayed = rl::commit_rate_decay(self.actors[i].battle.anim_rate);
            self.actors[i].battle.anim_rate = decayed;
            let marker = self.battle_ctx.gauge_rearm_latch != 0;
            // Party-ness is the actor's identity, not its slot: the engine
            // seats monsters wherever the formation put them (retail's
            // 0..2 / 3..7 split is `battle_monster_id` here).
            let is_party = self.actors[i].battle_monster_id.is_none();
            match rl::staged_commit_rate_effect(q, is_party, marker) {
                rl::CommitRateEffect::StarterFreeze => {
                    // Both arms walk retail's seven table seats
                    // (`sltiu v0,s2,0x7` at `0x8004B740`, the `+0x1C` bound
                    // at `0x8004BB74`): a fifth monster, at seat `7`, keeps
                    // its rate, as the restore `FUN_801E93C8` leaves it.
                    for e in self.battle_table_seats() {
                        self.actors[e].battle.anim_rate = rl::AnimRate(rl::RATE_FROZEN);
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
                        let found_stage = self.battle_ctx.arts_banner_stage;
                        let (stage, level) =
                            legaia_engine_battle_vm::battle_action::banner_on_starter_commit(
                                seat, pick,
                            );
                        self.battle_ctx.arts_banner_stage = stage;
                        self.battle_ctx.arts_banner_level = level;
                        // The same block's voice: the Miracle seat's / the
                        // starter mark's generic line, else the Hyper Art
                        // fanfare (`0x8004B7D0` / `0x8004BB34`).
                        self.emit_starter_commit_voice(i, found_stage);
                    }
                }
                rl::CommitRateEffect::StrikeSlow { rate } => {
                    for e in self.battle_table_seats() {
                        self.actors[e].battle.anim_rate = rl::AnimRate(rate);
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
            self.apply_battle_pose(i, legaia_engine_battle_vm::battle_action::Pose::Idle as u8);
            if mirrors_loop && let Some(a) = self.actors.get_mut(i) {
                a.battle.hit_count_bound = a
                    .battle_animation
                    .as_ref()
                    .map_or(0, |p| p.entry_loop_count());
            }
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
        // and the SM's Done arm clears. The routine's other half dims every
        // live bystander (`+0x21C = 1`, [`Self::sweep_solo_bystanders`]).
        // The value-2 arm (`0x8004E254..0x8004E2B4`): when the byte is `2`,
        // the previous `+0x243` was not, and the acting seat `ctx[+0x13]` is
        // a party one, it re-seeds the camera's yaw counter `ctx[+0x6DA] =
        // (rand() % 2) * 0x800 + 0x280` and zeroes the framing style
        // `ctx[+0xD]` - the swing camera's per-swing side. The counter is the
        // camera's (`BattleCamInputs::swing_reseed`); the style byte is the
        // live one both hosts feed it from.
        // PORT: FUN_8004E13C (the `+0x243` store, the value-2 re-seed and the `ctx[+0x26D]` coin)
        // The shout site (`0x8004BCF0..0x8004BD6C`): a party seat committing
        // a staged id `>= 0x10` asks the arts-voice selector when the byte
        // before the cursor is the known-art starter, or under the solo
        // latch - read here, ahead of this commit's own store below.
        if q >= 0x10 && self.actors[i].battle_monster_id.is_none() {
            let latch = self.battle_ctx.gauge_rearm_latch;
            self.emit_commit_arts_shout(i, latch);
        }
        if let Some(v) = clip
            .as_ref()
            .and_then(|c| c.entry_solo_flag())
            .filter(|&v| v != 0)
        {
            self.sweep_solo_bystanders();
            if v == 2
                && self.battle_ctx.gauge_rearm_latch != 2
                && self.battle_ctx.active_actor < self.party.party_count
            {
                let coin = (legaia_engine_system::rand::world_rand(&mut self.rng_state) & 1) as u8;
                self.battle_ctx.swing_yaw_seeds = self.battle_ctx.swing_yaw_seeds.wrapping_add(1);
                self.battle_ctx.swing_yaw_coin = coin;
                self.battle_ctx.camera_variant = 0;
            }
            self.battle_ctx.gauge_rearm_latch = v;
            // The routine's tail: `ctx[+0x26D] = rand() % 2`
            // (`0x8004E2BC..0x8004E2DC`), on every such commit, after the
            // re-seed's own draw.
            self.battle_ctx.track_coin =
                (legaia_engine_system::rand::world_rand(&mut self.rng_state) & 1) as u8;
            if let Some(coin) = self.dealt_track_coin() {
                self.battle_ctx.track_coin = coin;
            }
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
        //
        // The summon creature is the exception that shows the rule: retail's
        // natural-end path calls the commit unconditionally
        // (`0x80047B30..0x80047B58`), so with `+0x1DA == +0x1D9` the clip
        // replays, and nothing stages the creature back to idle - a cast
        // module bumps `+0x1DA` from the clip still playing (PROT 0907's arm
        // 8 moves `1` to `2`). Every creature clip therefore loops.
        let creature = self.casting.summon_actor_slot == Some(i as u8);
        let a = &mut self.actors[i];
        let player = clip.as_ref().and_then(|c| {
            if creature || c.action_id == legaia_engine_battle_vm::battle_action::WALK_TAG {
                legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(c)
            } else {
                legaia_engine_battle::battle_anim::MonsterAnimPlayer::new_one_shot(c)
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
                // `+0x21B = entry[+0x84]` (`0x8004BDEC..0x8004BDF4`); the
                // player seeds `+0x176` from the same byte.
                if mirrors_loop {
                    a.battle.hit_count_bound = p.entry_loop_count();
                }
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
                a.battle_clip_commits = a.battle_clip_commits.wrapping_add(1);
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
    /// (`legaia_engine_battle_vm::battle_action::monster_action_by_tag`).
    ///
    /// The clips are installed positionally
    /// (`legaia_actor_formats::monster_archive::animations_by_entry`), so a clip's
    /// index is its entry index and its `action_id` is the entry's first
    /// byte. A hole - an entry with no decodable stream - has no byte to
    /// report and reads as [`legaia_actor_formats::monster_archive::NO_ACTION_ENTRY`],
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
                    c.as_ref().map_or(
                        legaia_actor_formats::monster_archive::NO_ACTION_ENTRY,
                        |c| c.action_id,
                    )
                })
                .collect(),
        )
    }

    /// Install the per-slot battle action clips for actor `slot` (see
    /// [`Actor::battle_action_clips`]). The battle-action SM's `pose()` host
    /// hook then switches `battle_animation` between the idle loop and the
    /// matching action clip. No-ops for out-of-range slots.
    /// Install monster actor `slot`'s clip-morph table (see
    /// [`crate::world::Actor::battle_clip_morphs`]).
    pub fn set_actor_battle_clip_morphs(
        &mut self,
        slot: usize,
        table: std::sync::Arc<Vec<Option<legaia_actor_formats::monster_archive::ClipMorph>>>,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_clip_morphs = Some(table);
        }
    }

    /// Install battle actor `slot`'s four-slot beats
    /// ([`crate::world::Actor::battle_four_slot_beats`]), resolved by the host
    /// from the fight's heap placement.
    pub fn set_actor_battle_four_slot_beats(
        &mut self,
        slot: usize,
        beats: std::sync::Arc<Vec<Option<u8>>>,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_four_slot_beats = Some(beats);
        }
    }

    /// The load-address beat of actor `a`'s committed entry (`+0x1D9`), when
    /// its list is four-slot and the host resolved the placement.
    pub(in crate::world) fn committed_four_slot_beat(a: &crate::world::Actor) -> Option<u8> {
        a.battle_four_slot_beats
            .as_ref()?
            .get(usize::from(a.battle.current_anim))
            .copied()
            .flatten()
    }

    /// The clip-morph lanes battle actor `actor_idx`'s playing clip blends
    /// this frame, with their q12 weights at the clip cursor: the entry's
    /// `+0x04` lane, then `+0x08`, as `FUN_800495C8` walks them. Only lanes
    /// with a non-zero weight are returned, so an empty list means the rest
    /// mesh draws as is.
    ///
    /// PORT: FUN_800495C8 (lane walk: `s4` over `entry+4` / `entry+8`,
    /// `0x80049620..0x80049824`)
    pub fn battle_clip_morph_lanes(
        &self,
        actor_idx: usize,
    ) -> Vec<(&legaia_actor_formats::monster_archive::ClipMorph, i16)> {
        let Some(actor) = self.actors.get(actor_idx) else {
            return Vec::new();
        };
        let (Some(player), Some(table)) = (&actor.battle_animation, &actor.battle_clip_morphs)
        else {
            return Vec::new();
        };
        let phase = player.cursor_sixteenths() as i16;
        player
            .morph_lanes()
            .into_iter()
            .filter_map(|ix| {
                let m = table.get((ix as usize).checked_sub(1)?)?.as_ref()?;
                let w = legaia_actor_formats::monster_archive::clip_morph_weight(m.keys, phase);
                (w != 0).then_some((m, w))
            })
            .collect()
    }

    /// `tmd` with battle actor `actor_idx`'s active clip morphs blended into
    /// its vertices ([`Self::battle_clip_morph_lanes`],
    /// [`legaia_actor_formats::monster_archive::apply_clip_morphs`]) - the
    /// mesh both play hosts pose and draw this frame. `None` when no lane is
    /// active, so the caller keeps its rest mesh. The clone is retail's
    /// save / blend / restore around the draw (`FUN_800495C8` before it,
    /// `FUN_80049858` after it, `FUN_80048A08`).
    pub fn battle_morphed_tmd(
        &self,
        actor_idx: usize,
        tmd: &legaia_tmd::Tmd,
    ) -> Option<legaia_tmd::Tmd> {
        let lanes = self.battle_clip_morph_lanes(actor_idx);
        if lanes.is_empty() {
            return None;
        }
        let mut out = tmd.clone();
        legaia_actor_formats::monster_archive::apply_clip_morphs(&mut out, &lanes);
        Some(out)
    }

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
        use legaia_engine_battle_vm::battle_action::Pose;
        // Ready / recover play the idle loop (doc above): fold them into the
        // idle request so the loop is not restarted when the SM switches
        // between them.
        let pose_id = if pose_id == Pose::Ready as u8 || pose_id == Pose::Recover as u8 {
            Pose::Idle as u8
        } else {
            pose_id
        };
        let seats = self.battle_seat_map();
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
        // and accept the full pose set. The side is the engine's seating: a
        // small party's first monster sits below slot `3`.
        if !seats.is_party(u8::try_from(slot).unwrap_or(u8::MAX))
            && pose_id != legaia_engine_battle_vm::battle_action::Pose::Idle as u8
        {
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
                legaia_engine_battle::battle_anim::MonsterAnimPlayer::new_one_shot(clip)
                    .map(|p| (p, clip))
            }
            _ => clips.first().and_then(|c| c.as_ref()).and_then(|clip| {
                legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(clip).map(|p| (p, clip))
            }),
        };
        if let Some((mut player, clip)) = selected {
            // Anim record swapped: install its effect script and zero the
            // effect-script cursor (retail FUN_8004AD80, `sb zero,0x1f5`).
            let script = Some(clip.effect_script.clone()).filter(|s| !s.is_empty());
            // The seat's opening phase, drawn at battle setup
            // (`FUN_800513F0`, `0x80051544..0x80051574` and the monster twin
            // at `0x800518D0`): `(rand() % frames) << 4`.
            if let Some(draw) = actor.battle_idle_phase_draw.take()
                && clip_slot == 0
                && player.is_looping()
            {
                let frames = player.frame_count().max(1);
                actor.pose_frame = Some(player.seat_cursor(((draw % frames) << 4) as u16));
            }
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
        player: legaia_engine_battle::battle_anim::MonsterAnimPlayer,
    ) {
        if let Some(actor) = self.actors.get_mut(slot) {
            actor.battle_animation = Some(player);
            actor.pose_frame = None;
        }
    }

    /// A solo commit's bystander sweep: every live seat of the pool's first
    /// seven but the acting seat `ctx[+0x13]` and its target goes to the dim
    /// arm `+0x21C = 1`, which eases its lanes toward `0x20` at weight
    /// `0x1000` until the rate restore `FUN_801E93C8` drops the arm
    /// ([`solo_commit_state`](legaia_engine_battle_vm::battle_formulas::solo_commit_state)).
    /// The sweep reads the acting seat, not the committing body, as retail's
    /// loop does.
    ///
    /// An engine body at rest holds a zero colour word where retail's holds
    /// neutral (`0x20080200`), and the arm walks from the word it finds, so a
    /// dimmed body takes neutral first.
    ///
    /// PORT: FUN_8004E13C (`0x8004E15C..0x8004E24C`, the `+0x21C` sweep)
    pub(in crate::world) fn sweep_solo_bystanders(&mut self) {
        use legaia_engine_battle_vm::battle_formulas::{STATE_DIM, solo_commit_state};
        let seats = self.battle_seat_map();
        let acting_e = self.battle_ctx.active_actor;
        let target_e = self
            .actors
            .get(usize::from(acting_e))
            .map_or(0, |a| a.battle.active_target);
        let acting = self.retail_pool_slot(acting_e);
        let target = self.retail_pool_slot(target_e);
        for r in 0..7u8 {
            let Some(a) = seats
                .engine(r)
                .and_then(|e| self.actors.get_mut(usize::from(e)))
            else {
                continue;
            };
            if !a.active {
                continue;
            }
            let b = &mut a.battle;
            let state = solo_commit_state(r, b.hp, b.render_flag, acting, target);
            if state == STATE_DIM && b.render_flag != STATE_DIM && b.render_color == 0 {
                b.render_color = legaia_engine_battle_vm::battle_impact_fx::IMPACT_NEUTRAL_STATE;
            }
            b.render_flag = state;
        }
    }

    /// The anim tick's Rage arm: a party seat whose character record carries
    /// the auto-fight passive (record `+0xF8 & 0x2000`, the Evil Medallion's
    /// Rage) is stamped every advancing tick with the delegation group
    /// `+0x16E |= 0x380` - which hands its command to the delegated auto-fill
    /// and makes the tint pass draw it in the `0xF020F0` status colour - and
    /// with the tint words `+0x04 = 0x3FF00000`, `+0x0C = 0x1000`. A raised
    /// impact selector `+0x21F` is cleared. Skipped once the battle-end
    /// signal is up (`DAT_8007BD71 == 0xFE`) and while a presentation arm
    /// owns the body (`+0x21C != 0`).
    ///
    /// ```text
    /// 80047f40  andi v0,v0,0x2000      ; record +0xF8 (0x80084140 + id*0x414 + 0x6C0)
    /// 80047f58  beq v1,0xfe,...        ; DAT_8007BD71: battle over
    /// 80047f68  bne +0x21C,zero,...    ; presentation arm
    /// 80047f70  lui v0,0x3ff0 / sw v0,0x4(s2)
    /// 80047f78  li v0,0x1000 / sw v0,0xc(s2)
    /// 80047f88  ori v0,v0,0x380        ; the sh in the delay slot always stores
    /// 80047f94  sb zero,0x21f(s2)      ; only when +0x21F was set
    /// ```
    ///
    /// The frozen-idle exit (Stone / Numb) jumps past it, as the caller does.
    ///
    /// PORT: FUN_80047430 (`0x80047F10..0x80047F98`, the Rage arm)
    pub(in crate::world) fn stamp_rage_delegation(&mut self, slot: usize) {
        const RAGE_BIT: u32 = 0x2000;
        if slot >= usize::from(self.party.party_count.min(3)) || self.battle.end.is_some() {
            return;
        }
        let rslot = self.party_roster_slot(slot);
        let Some(member) = self.party.roster.members.get(rslot) else {
            return;
        };
        let bits = member.ability_bits();
        if u32::from_le_bytes([bits[4], bits[5], bits[6], bits[7]]) & RAGE_BIT == 0 {
            return;
        }
        let Some(actor) = self.actors.get_mut(slot) else {
            return;
        };
        let b = &mut actor.battle;
        if b.render_flag != 0 {
            return;
        }
        b.render_color = 0x3FF0_0000;
        b.render_blend = 0x1000;
        b.field_flags |= 0x0380;
        b.impact_state = 0;
    }
}

/// One `FUN_8004CCD4` pass over a party seat's model table, then the draw
/// set the draw pass `FUN_80048A08` reads off it for the playing stream's
/// channels. A seat with no table, or a clip that records no stream part
/// count (a synthetic clip), draws every object.
// REF: FUN_80048A08 (the draw count: `c < **(entry + 0x88)`)
fn step_model_table(
    table: &mut Option<legaia_engine_battle::battle_attach_swap::BattleModelTable>,
    drawn: &mut Option<Vec<bool>>,
    track: legaia_actor_formats::monster_archive::VariantTrack,
    frame: i16,
) {
    *drawn = match table.as_mut() {
        Some(t) if track.stream_parts != 0 => {
            t.step(&track.windows, frame, track.stream_parts);
            Some(t.drawn_objects(usize::from(track.stream_parts)))
        }
        _ => None,
    };
}
