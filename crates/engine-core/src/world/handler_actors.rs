//! The `+0x0C` per-frame-handler half of the actor pool: the two SCUS list
//! leaves that key on it, the scene-transition sweep, the MAN-load reset, and
//! the per-frame dispatch of the handler kernels that have ported bodies.
//!
//! PORT: FUN_8003CF04, FUN_8003CF40
//!
//! Retail keeps actors on seven linked lists and walks each one through its
//! `+0x00` next pointer. The engine keeps a single fixed pool with an `active`
//! flag (see [`World::tick_actor_physics_with`] for why), so every walk here is
//! a scan over `self.actors` in slot order. That changes the *iteration order*
//! against retail's list order and nothing else: all four routines are
//! order-independent except the finder, which returns the first match either
//! way and has exactly one live spawner competing for it.
//!
//! REF: FUN_80020DE0 (installs the handler), FUN_8002519C (dispatches it),
//! FUN_801D6704 (calls the transition sweep), FUN_8003AEB0 (the MAN loader)

use super::*;
use legaia_engine_mode::mode::SceneMode;

use legaia_engine_field::actor_colour_tween::{ColourTween, ScreenTintPush};
use legaia_engine_field::actor_handler::{ActorHandler, MAN_LOAD_RETIRED_HANDLERS};
use legaia_engine_field::field_submode::{
    SCENE_ACTOR_REQUESTED_STATE, SubmodeOpen, open_submode, scene_actor_initial_state,
};
use legaia_engine_field::scene_transition_sweep::{
    ACTOR_FLAG_YIELD, SweepActor, SweepDecision, sweep_actor,
};

/// The player's model word `+0x64` in the raw `4C 50` operand space while no
/// script has re-staged it: player-bank slot `0`, the id the Room of Mirrors
/// image re-stages itself to (`4C 50 F0`). A capture of the `conc2` mirror
/// tick reads the player's `+0x64` equal to the image's (both resolve to
/// player-bank index `0`), which is the reflection's clip-copy gate.
const PLAYER_LEAD_MODEL: i16 = 0xF0;

/// What one [`World::scene_transition_actor_sweep`] pass did, for callers and
/// tests that want to see the sweep worked rather than trusting that it ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransitionSweepReport {
    /// Pool slots visited (every active slot).
    pub visited: usize,
    /// Slots that took [`ACTOR_FLAG_YIELD`] because their handler is one of
    /// the sweep's three retire classes.
    pub retired: Vec<u8>,
    /// Slots whose `0x800` bit asked for a fresh `0x9C`-byte side buffer.
    pub side_buffer_reallocs: Vec<u8>,
    /// Slots whose ocean CLUT-walk accumulator was reseeded.
    pub clut_walk_reseeds: Vec<u8>,
    /// Slots that took the move-VM long arm instead of being retired.
    pub move_vm_arms: Vec<u8>,
}

impl World {
    /// Find the first **live** actor running `handler`.
    ///
    /// PORT: FUN_8003CF04 (`0x8003cf04..0x8003cf3c`)
    ///
    /// Fifteen instructions: walk the list, skip a node whose `+0x0C` is not
    /// the handler, skip one whose `+0x10` already carries the kill bit `8`,
    /// return the first survivor (`0` on exhaustion). The kill-bit skip is the
    /// half that is easy to drop and load-bearing: an actor retired earlier
    /// this frame is *not* a hit, which is what stops a find-or-spawn API from
    /// re-using a corpse.
    ///
    /// Live: [`Self::man_load_actor_reset`] (every scene load) and
    /// [`Self::retire_actors_by_handler`]'s sibling tests.
    pub fn find_actor_by_handler(&self, handler: ActorHandler) -> Option<usize> {
        self.actors.iter().position(|a| {
            a.active && a.handler == handler && a.physics.status_flags & ACTOR_FLAG_YIELD == 0
        })
    }

    /// Retire **every** actor running `handler` by setting the kill bit.
    ///
    /// PORT: FUN_8003CF40 (`0x8003cf40..0x8003cf78`)
    ///
    /// The same fifteen-instruction walk as [`Self::find_actor_by_handler`]
    /// with the tail swapped: `ori 8` into `+0x10` and keep going. It is a
    /// **retire sweep, not a registration** - it has no return value and
    /// touches nothing but the flag word, so with no matching actor live it is
    /// entirely inert. Returns how many slots it marked.
    ///
    /// Live: [`Self::man_load_actor_reset`], the field VM's `4C 9F`
    /// ladder-oscillator retire through
    /// [`Self::retire_floor_ladder_oscillators`], and its `4C 87` reflection
    /// retire through [`Self::retire_reflection_controllers`]. The two ops
    /// share the exit, not the handler: `4C 9F` forms `LAB_801DA930`,
    /// `4C 87` forms `0x801E5154`.
    pub fn retire_actors_by_handler(&mut self, handler: ActorHandler) -> usize {
        let mut n = 0;
        for a in self.actors.iter_mut() {
            if a.active && a.handler == handler {
                a.physics.status_flags |= ACTOR_FLAG_YIELD;
                n += 1;
            }
        }
        n
    }

    /// Retire every pool actor running `LAB_801DA930` - the field VM's
    /// `4C 9F` op, and the first of the MAN loader's two inlined sweeps.
    /// (`4C 87` is the neighbouring op that shares the same exit but forms
    /// the reflection handler `0x801E5154` instead - see
    /// [`Self::retire_reflection_controllers`].)
    ///
    /// REF: FUN_8003CF40 against `LAB_801DA930`
    ///
    /// `LAB_801DA930` is the handler on spawn descriptor `0x801F27EC`, and
    /// what that tick drives is **one rung of the scene floor-height ladder**
    /// at `0x1F80035C`, not a fade - see
    /// [`legaia_engine_field::field_actor_timers::FloorTierBob`]. The engine keeps
    /// those records off the pool, so the callers that mean the retire
    /// (`4C 9F`, [`Self::man_load_actor_reset`]) clear
    /// [`crate::world::FieldTerrain::floor_tier_bobs`] alongside this sweep. Nothing
    /// is *registered* anywhere and no fade is cancelled - `FUN_8003CF40` has
    /// no return value and writes only the flag word.
    pub fn retire_floor_ladder_oscillators(&mut self) -> usize {
        self.retire_actors_by_handler(ActorHandler::FloorLadder)
    }

    /// Seat a pool actor carrying `handler`, returning its slot.
    ///
    /// REF: FUN_80020DE0 (retail allocates from a free list and copies the
    /// spawn descriptor's `+0x8` word into `actor[+0x0C]`)
    ///
    /// The engine's pool is fixed-size, so "allocate" is "take a free slot";
    /// `None` on a full pool is retail's `0` return, which every caller of
    /// `FUN_80020DE0` in the field overlay ignores - so the callers here ignore
    /// it too, rather than inventing a failure path retail does not have.
    ///
    /// **Allocation runs from the top of the pool down**, and that is not a
    /// style choice. Retail keeps these on their own list (`_DAT_8007C34C`),
    /// disjoint from the script-addressed scene actors; the engine has one
    /// pool, and its low slots are *named* - the field VM's `ensure_actor(id)`
    /// addresses them by script id and `init_scene_animations` binds actor `k`
    /// to scene TMD `k`. Handing a handler actor slot 0 would hand it the
    /// player. Top-down keeps the two populations apart in the one pool.
    pub fn spawn_handler_actor(&mut self, handler: ActorHandler) -> Option<usize> {
        let slot = self.actors.iter().rposition(|a| !a.active)?;
        let a = &mut self.actors[slot];
        *a = Actor::new();
        a.active = true;
        a.handler = handler;
        Some(slot)
    }

    /// Every full-screen colour push the actor pool emitted this frame.
    ///
    /// REF: FUN_80024EE4 - retail's screen-effect push. The engine collects the
    /// argument triples rather than resolving them to a framebuffer operation,
    /// because which quad each `(kind, blend)` pair draws is the renderer's
    /// business.
    ///
    /// None in a battle: the colour-tween actor's handler is field-overlay
    /// code and battle loads PROT 0898 over that window, so nothing pushes
    /// while a fight runs. A field wash still live at the encounter - the
    /// night tint of `town0b` - is the field's, and it comes back with it.
    ///
    /// None in a minigame either, for the same reason: the mode-24 entry
    /// streams the minigame's overlay into slot A, the field overlay's
    /// window (`FUN_80025980`'s `FUN_8003EBE4(sub_id + 0x4D)`; the
    /// `baka_fighter_entry_pretransition` capture shows the overlay landing
    /// there). Every casino seat fades out on op `34 01 FF FF FF 1E` before
    /// its `3E` - a subtractive (blend `2`) white wash held at full - and
    /// left pushing it hid the whole 3D duel under black, leaving only the
    /// screen-space HUD (the round digit) on a seat-entered Baka Fighter.
    /// The cabinet's own return path clears the wash after the warp.
    pub fn screen_tint_pushes(&self) -> Vec<ScreenTintPush> {
        if matches!(
            self.mode,
            SceneMode::Battle
                | SceneMode::BakaFighter
                | SceneMode::SlotMachine
                | SceneMode::MuscleDome
                | SceneMode::Fishing
                | SceneMode::Dance
        ) {
            return Vec::new();
        }
        self.actors
            .iter()
            .filter(|a| a.active)
            .filter_map(|a| a.tint_push)
            .collect()
    }

    /// The scene's held **scripted grade**: `(blend, target_rgb)` of the live
    /// op-`0x34` sub-0 screen-effect tween (retail `_DAT_8007BCE0` and
    /// `_DAT_8007BCCD..CF` while `_DAT_8007B62C` names a live slot), `None`
    /// when no tween is installed. `town0b`'s entry script holds
    /// `(2, [0x30, 0x30, 0x00])` - its night. The enhanced-lighting mood
    /// reads it on both hosts
    /// (`legaia_render_kernels::scene_lighting::TimeOfDay::mood_graded`).
    pub fn held_scene_grade(&self) -> Option<(i16, [i16; 3])> {
        self.presentation.effect_tween_slot?;
        Some((
            self.presentation.effect_blend,
            self.presentation.effect_target_rgb,
        ))
    }

    /// The same frame's pushes as the `(layer, blend, packed)` argument
    /// triples retail's emitter takes, ready for
    /// `legaia_render_kernels::screen_prim::screen_effect_push_prims`.
    ///
    /// REF: FUN_80024EE4 - its `a0`/`a1`/`a2`, in that order.
    ///
    /// Both hosts composite through this rather than reading
    /// [`Self::screen_tint_pushes`] field by field. `ScreenTintPush::kind`
    /// and `ScreenTintPush::blend` are two `i16`s a call site can swap
    /// silently, and they mean entirely different things - the first is the
    /// ordering-table bucket, the second the ABR equation - so the order is
    /// fixed once, here, where the retail argument list is the authority.
    pub fn screen_tint_push_args(&self) -> Vec<(i16, i16, u32)> {
        self.screen_tint_pushes()
            .into_iter()
            .map(|p| (p.kind, p.blend, p.packed))
            .chain(self.map_display_fade_push_args())
            .collect()
    }

    /// Run the per-frame handler kernels over the actor pool, then retire the
    /// actors that asked to go.
    ///
    /// PORT driver for FUN_801DDC20; REF: FUN_8002519C
    ///
    /// `FUN_8002519C` walks a list and either `jalr`s `node[+0x0C]` or runs the
    /// physics tick inline when the handler is `FUN_80021DF4`. The engine
    /// splits that: [`Self::tick_actor_physics`] is the inline arm (it runs for
    /// every active slot, matching the retail special case), and this is the
    /// `jalr` arm for the handler classes with a ported body that lives on the
    /// pool - the per-actor colour tween and the clip-fraction fade the field
    /// VM's actor clone spawns, per
    /// [`legaia_engine_field::actor_handler::HandlerKernel::runs_in_actor_loop`].
    ///
    /// `frame_delta` is retail's `DAT_1F800393`, the same scalar the physics
    /// pass carries - the tween advances its clock by it, so a tween's
    /// wall-clock duration is cadence-invariant exactly like every other
    /// retail duration.
    ///
    /// The end-of-pass retire is retail's kill-bit contract: bit `8` means
    /// "retire me at the end of this frame", and the two producers of it (this
    /// tween's hold expiry, and [`Self::scene_transition_actor_sweep`]) both
    /// depend on something acting on it. Returns the number of slots retired.
    pub fn tick_handler_actors(&mut self, frame_delta: u8) -> usize {
        for a in self.actors.iter_mut() {
            a.tint_push = None;
            if !a.active {
                continue;
            }
            // The clip-fraction fade is the second pool-resident kernel: one
            // multiply-accumulate against the cadence byte, and the retire bit
            // when the 12-bit accumulator fills. Its actors are the clones the
            // field VM's op `0x4C` sub-1 sub-op `0x14` spawns
            // (`legaia_engine_field::field_actor_clone`), and the end-of-pass retire below is
            // what collects them.
            // REF: FUN_801D820C
            if a.handler.kernel() == legaia_engine_field::actor_handler::HandlerKernel::ClipFade {
                vm::actor_tick::clip_fraction_step(&mut a.physics, frame_delta);
                continue;
            }
            // The morph-weight envelope: the ping-pong ramp in the tail of
            // `FUN_8002174C`, over the actors the field VM's `4C D8`
            // allocator seated (`World::spawn_morph_weight_actor`). Retail
            // runs the ramp in the same per-frame handler call as the apply
            // pass; the engine splits them because the blend is a render-time
            // read the hosts make (`World::morph_weight_posed_tmd`) and only
            // the envelope is simulation state.
            // REF: FUN_8002174C
            if a.handler.kernel() == legaia_engine_field::actor_handler::HandlerKernel::MorphWeights
            {
                if let Some(m) = a.morph_weights.as_mut() {
                    m.envelope.tick(frame_delta);
                }
                continue;
            }
            if !a.handler.kernel().runs_in_actor_loop() {
                continue;
            }
            let Some(tween) = a.colour_tween else {
                continue;
            };
            let mut t = tween;
            t.flags = a.physics.status_flags;
            let step = legaia_engine_field::actor_colour_tween::step_colour_tween(t, frame_delta);
            let mut next = tween;
            next.clock = step.clock;
            a.colour_tween = Some(next);
            a.physics.status_flags = step.flags;
            a.tint_push = step.push;
        }
        // The third pool-resident kernel: the reflection pairs the field
        // VM's `4C 86` arm installs. It runs after the physics writes of
        // this frame rather than inside the loop above, because it reads
        // one seat and writes another and so cannot borrow a single slot.
        // REF: FUN_801E5154
        self.tick_reflection_controllers();
        // The second `jalr node[+0x0C]` class with a ported body: the op-0x49
        // submode driver (`ActorHandler::SubmodeDriver`, spawned by
        // `man_load_actor_reset`). Its body is the dispatcher `FUN_801F159C`
        // in `crate::field_submode_screen`, which retires the actor itself
        // when a screen hands back - hence before the retire sweep below.
        self.tick_submode_screen(frame_delta);
        // The scripted-scene programs the MAN loader resumed
        // (`FUN_801D4A60`); a closer retires itself, so before the sweep.
        self.tick_scene_programs(frame_delta);
        self.retire_yielded_actors()
    }

    /// Deactivate every active actor carrying the kill bit `+0x10 & 8`.
    ///
    /// REF: FUN_8002519C (the frame walker drops killed nodes off the list)
    ///
    /// Retail unlinks the node; the engine has no list, so "unlinked" is
    /// `active = false`. Returns how many went.
    pub fn retire_yielded_actors(&mut self) -> usize {
        let mut n = 0;
        for a in self.actors.iter_mut() {
            if a.active && a.physics.status_flags & ACTOR_FLAG_YIELD != 0 {
                a.active = false;
                a.colour_tween = None;
                a.morph_weights = None;
                a.scene_program = None;
                a.tint_push = None;
                n += 1;
            }
        }
        n
    }

    /// Step every live **reflection pair**.
    ///
    /// PORT driver for FUN_801E5154 (the kernel is
    /// [`legaia_engine_field::field_actor_reflect::tick_reflection`])
    ///
    /// One pass per game tick, over the pool slots the `4C 86` arm seated
    /// through [`Self::spawn_reflection_controller`]. Each pair reads its
    /// `+0x94` end, runs the retail tick, and writes the mirrored pose back
    /// to its `+0x90` end.
    ///
    /// A seat that no longer resolves is retail's dead actor: the tick's own
    /// first test is `either end's +0x10 & 8`, and it answers by setting the
    /// controller's kill bit, so the end-of-pass sweep collects the slot.
    /// The engine reaches the same state by treating an unresolvable seat as
    /// that dead end.
    ///
    /// The engine mirrors the **pose** - the position triple and the facing
    /// angle - and, when both ends' model words `+0x64` agree, the **clip**
    /// (`0x801E5204..0x801E5250`): the party-bank bit, the clip id `+0x5C`,
    /// the rate `+0x6A` and the cursor `+0x68`. That copy is what makes the
    /// Room of Mirrors image (`concnow` / `conc2` `P1[1]`, which re-stages
    /// itself to the party model `0xF0` before it installs the pair) walk,
    /// run and stand with the player; without it the image slid along the
    /// glass on its own idle clip.
    fn tick_reflection_controllers(&mut self) {
        use legaia_engine_field::field_actor_reflect::{ReflectOutcome, flags, tick_reflection};

        let live: Vec<(usize, crate::world::ReflectionLink)> = self
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.active && a.handler == ActorHandler::Reflection)
            .filter_map(|(slot, a)| a.reflection.map(|link| (slot, link)))
            .collect();

        for (slot, mut link) in live {
            let ends = self
                .reflect_seat_read(link.source)
                .zip(self.reflect_seat_read(link.destination));
            let Some(((src, src_step), (mut dst, _))) = ends else {
                link.controller.flags |= flags::TEARDOWN;
                self.actors[slot].physics.status_flags |= ACTOR_FLAG_YIELD;
                self.actors[slot].reflection = Some(link);
                continue;
            };
            // The kernel's own gate (`lh v1,0x64(a3)` / `lh v0,0x64(a2)` /
            // `bne` at `0x801E5204..0x801E5210`), asked again here so the
            // write-back knows whether the clip half ran.
            let clip_copied = dst.anim_set == src.anim_set;
            if tick_reflection(&mut link.controller, &mut dst, &src) == ReflectOutcome::Reflected {
                let clip = clip_copied.then_some(src_step);
                self.reflect_seat_write(link.destination, &dst, clip);
            }
            if link.controller.flags & flags::TEARDOWN != 0 {
                self.actors[slot].physics.status_flags |= ACTOR_FLAG_YIELD;
            }
            self.actors[slot].reflection = Some(link);
        }
    }

    /// The fields the reflection tick reads off one end of a pair, plus the
    /// per-tick step of the clip that end plays in the port's cursor units
    /// (`None` when unknown). Retail needs no step: the image's own clip
    /// tick steps the copied clip at the copied rate through the same record
    /// header. The port's image cursor is bound from a clip player whose
    /// step already folds that header in, so the step crosses over instead.
    ///
    /// `None` is "this end is gone", which the tick treats as the retail
    /// kill bit.
    fn reflect_seat_read(
        &self,
        seat: crate::world::EasedMoveTarget,
    ) -> Option<(
        legaia_engine_field::field_actor_reflect::ReflectActor,
        Option<u32>,
    )> {
        use legaia_engine_field::field_actor_reflect::{ReflectActor, flags};
        match seat {
            crate::world::EasedMoveTarget::Player => {
                let slot = self.player_actor_slot? as usize;
                let a = self.actors.get(slot).filter(|a| a.active)?;
                let (clip, party, cursor, step) = self.player_reflect_clip();
                let bank = if party { flags::ANIM_MIRROR } else { 0 };
                Some((
                    ReflectActor {
                        flags: (a.physics.status_flags & !flags::ANIM_MIRROR) | bank,
                        x: a.move_state.world_x,
                        y: a.physics.world_y,
                        z: a.move_state.world_z,
                        facing: a.move_state.render_26,
                        anim_cursor: clip,
                        anim_set: self
                            .locomotion
                            .player_live_model
                            .unwrap_or(PLAYER_LEAD_MODEL),
                        anim_frame: cursor,
                        anim_timer: self.locomotion.player_clip_rate as u16,
                    },
                    step,
                ))
            }
            crate::world::EasedMoveTarget::Placement(p) => {
                let (x, z) = self.npcs.positions.get(&p).copied()?;
                let ch = self
                    .field_vm
                    .channels
                    .iter()
                    .find(|c| !c.object_bind && c.placement_index == usize::from(p));
                let cursor = self.npcs.clip_cursors.get(&p);
                let model = self
                    .npcs
                    .models
                    .get(&p)
                    .copied()
                    .or_else(|| ch.map(|c| c.ctx.model_id as i16))
                    .unwrap_or(0);
                Some((
                    ReflectActor {
                        flags: ch.map_or(0, |c| c.ctx.flags & flags::ANIM_MIRROR),
                        x,
                        z,
                        facing: self.npcs.heading(p),
                        anim_cursor: cursor
                            .map(|c| u16::from(c.anim_id))
                            .or_else(|| ch.map(|c| c.ctx.move_id))
                            .unwrap_or(0),
                        anim_set: model,
                        anim_frame: cursor.map_or(0, |c| c.cursor as u16),
                        anim_timer: ch.map_or(0, |c| c.ctx.field_6a as u16),
                        ..ReflectActor::default()
                    },
                    cursor.map(|c| c.rate.max(1) as u32),
                ))
            }
        }
    }

    /// The player's clip as the reflection copy reads it: `(+0x5C, the
    /// party-bank bit, +0x68, the per-tick step)`, off the clip the player's
    /// body plays this tick. A bank clip is record `leader * 7 + slot + 1`
    /// of the party bank (the settle tail's stride, `0x801D1E0C`); the
    /// motion-derived walk and idle are the bank's walk and idle slots; a
    /// scene-bank pick is record `+ 1` with the bank bit down.
    fn player_reflect_clip(&self) -> (u16, bool, u16, Option<u32>) {
        use legaia_actor_formats::character_pack::{
            LOCOMOTION_BANK_STRIDE, LOCOMOTION_IDLE_SLOT, LOCOMOTION_WALK_SLOT,
        };
        let fallback = (
            self.locomotion.player_clip.max(0) as u16,
            self.locomotion.player_party_bank,
            0,
            None,
        );
        let Some(anim) = self.locomotion.player_anim.as_ref() else {
            return fallback;
        };
        let (source, id, cursor, _) = anim.playing_clip();
        let bank = |slot: usize| anim.leader * LOCOMOTION_BANK_STRIDE as u16 + slot as u16 + 1;
        let (clip, party) = match source {
            "scene" => (id as u16 + 1, false),
            "bank" => (bank(id), true),
            "walk" => (bank(LOCOMOTION_WALK_SLOT), true),
            "idle" => (bank(LOCOMOTION_IDLE_SLOT), true),
            _ => return fallback,
        };
        (clip, party, cursor as u16, anim.playing_step())
    }

    /// Write the mirrored pose back to the `+0x90` end, and the copied clip
    /// when the tick's model gate passed (`clip` carries the source clip's
    /// port step).
    ///
    /// The clip lands on the image's world-owned cursor: the same clip id
    /// takes the source's cursor and step, a new one is cued like any other
    /// `+0x5C` write, so both hosts re-target the image's clip player in
    /// their cue drain and the next tick's copy seats its cursor. The
    /// party-bank bit goes onto the image's channel, where the cue drain
    /// reads which bank the id names.
    fn reflect_seat_write(
        &mut self,
        seat: crate::world::EasedMoveTarget,
        mirrored: &legaia_engine_field::field_actor_reflect::ReflectActor,
        clip: Option<Option<u32>>,
    ) {
        use legaia_engine_field::field_actor_reflect::flags;
        match seat {
            crate::world::EasedMoveTarget::Player => {
                // No shipped `4C 86` writes the player; the pose is all the
                // engine carries for that end.
                let Some(slot) = self.player_actor_slot.map(usize::from) else {
                    return;
                };
                let Some(a) = self.actors.get_mut(slot) else {
                    return;
                };
                a.move_state.world_x = mirrored.x;
                a.move_state.world_z = mirrored.z;
                a.move_state.render_26 = mirrored.facing;
                a.physics.world_x = mirrored.x;
                a.physics.world_y = mirrored.y;
                a.physics.world_z = mirrored.z;
            }
            crate::world::EasedMoveTarget::Placement(p) => {
                self.npcs.positions.insert(p, (mirrored.x, mirrored.z));
                self.npcs.headings.insert(p, mirrored.facing);
                let Some(step) = clip else {
                    return;
                };
                if let Some(ch) = self
                    .field_vm
                    .channels
                    .iter_mut()
                    .find(|c| !c.object_bind && c.placement_index == usize::from(p))
                {
                    ch.ctx.flags = (ch.ctx.flags & !flags::ANIM_MIRROR)
                        | (mirrored.flags & flags::ANIM_MIRROR);
                    ch.ctx.move_id = mirrored.anim_cursor;
                    ch.ctx.field_6a = mirrored.anim_timer as i16;
                }
                let Ok(id) = u8::try_from(mirrored.anim_cursor) else {
                    return;
                };
                if id == 0 {
                    return;
                }
                match self.npcs.clip_cursors.get_mut(&p) {
                    Some(c) if c.anim_id == id => {
                        let span = i32::from(c.frames.max(1)) * 16;
                        c.cursor =
                            (i32::from(mirrored.anim_frame as i16)).clamp(0, span - 1) as i16;
                        if let Some(step) = step {
                            c.rate = i16::try_from(step).unwrap_or(i16::MAX).max(1);
                        }
                    }
                    _ => {
                        if self.npcs.anim_cues.get(&p).is_none_or(|c| c.1 != id) {
                            self.npcs.anim_cues.insert(p, (1, id, Vec::new()));
                        }
                    }
                }
            }
        }
    }

    /// Retire every live reflection controller - the field VM's `4C 87`.
    ///
    /// REF: FUN_8003CF40 against `0x801E5154`
    ///
    /// The install's teardown sibling, and a retire sweep like every other
    /// `FUN_8003CF40` caller: it registers nothing and returns nothing but
    /// the count of slots it marked.
    pub fn retire_reflection_controllers(&mut self) -> usize {
        self.retire_actors_by_handler(ActorHandler::Reflection)
    }

    /// Scene-transition teardown sweep over the whole pool.
    ///
    /// PORT driver for FUN_801D7518 (the decision kernel is
    /// [`sweep_actor`]).
    ///
    /// Retail's field initialiser `FUN_801D6704` calls the sweep once per actor
    /// list - seven times - on a warp entry (`_DAT_8007B8B8 == 2`); the engine's
    /// single pool makes that one pass. Every visited actor takes the
    /// transition stamp, the three retire classes take the kill bit, and the
    /// buffer work the retail sweep does is reported rather than performed
    /// (the `0x9C` side buffer and the move-VM arm's keyframe/vertex blocks are
    /// actor-local scratch the engine does not allocate).
    ///
    /// Live: `SceneHost::load_scene`, gated on a scene already being loaded -
    /// which is exactly the scene-to-scene condition retail's `== 2` encodes.
    pub fn scene_transition_actor_sweep(&mut self) -> TransitionSweepReport {
        let mut report = TransitionSweepReport::default();
        for slot in 0..self.actors.len() {
            if !self.actors[slot].active {
                continue;
            }
            let a = &self.actors[slot];
            let decision: SweepDecision = sweep_actor(SweepActor {
                handler: a.handler.va(),
                flags: a.physics.status_flags,
                // `+0x56` in the sweep's view is the render-mode word; the
                // engine's physics view spells the same halfword
                // `move_vm_kick`. Only the low nibble is read.
                render_mode: a.physics.move_vm_kick as u16,
            });
            self.actors[slot].physics.status_flags = decision.flags;
            report.visited += 1;
            let s = slot as u8;
            if decision.retired {
                report.retired.push(s);
            }
            if decision.realloc_side_buffer {
                report.side_buffer_reallocs.push(s);
            }
            if decision.clut_walk_seed.is_some() {
                report.clut_walk_reseeds.push(s);
            }
            if decision.move_vm_arm {
                report.move_vm_arms.push(s);
            }
        }
        report
    }

    /// The actor-list work the scene MAN loader does, in retail order.
    ///
    /// PORT driver for FUN_801D9C3C and FUN_801DE478; REF: FUN_8003AEB0
    ///
    /// `FUN_8003AEB0` runs four things back to back on `_DAT_8007C34C`:
    ///
    /// 1. an inlined `FUN_8003CF40` against `LAB_801DA930` (`0x8003B3C8`);
    /// 2. a second one against `FUN_80037018` (`0x8003B414`);
    /// 3. `FUN_801D9C3C()` - the submode open (`0x8003B444`);
    /// 4. `FUN_801DE478(0xF)` - the fixed-template scene-actor spawn
    ///    (`0x8003B9B0`).
    ///
    /// Order is not cosmetic: both retire sweeps precede the open, so a driver
    /// actor marked by step 1 or 2 is invisible to step 3's find (the finder
    /// skips kill-bit nodes) and the submode re-spawns rather than adopting a
    /// dying one.
    ///
    /// Returns the submode-open outcome so a caller can tell an adoption from
    /// a spawn.
    pub fn man_load_actor_reset(&mut self) -> SubmodeOpen {
        for handler in MAN_LOAD_RETIRED_HANDLERS {
            self.retire_actors_by_handler(handler);
        }
        // The first of those two sweeps is `FUN_8003CF40(_DAT_8007C34C,
        // LAB_801DA930)` - the **floor-height-ladder** oscillator's handler.
        // The engine keeps those records off the pool (see
        // `World::terrain.floor_tier_bobs`), so the sweep has to reach them here or a
        // rung left oscillating would keep writing into the next scene's
        // ladder. Its two siblings on the same list go with it: a bar
        // envelope and an eased move are both scene content.
        self.terrain.floor_tier_bobs.clear();
        self.presentation.cinematic_bars = None;
        self.presentation.cinematic_bar = 0;
        self.presentation.screen_frames.clear();
        // The ending's screen-effect widgets (mask, image panel, credit
        // sprites, letterbox) are nodes on the same list 0
        // (`FUN_80020DE0(.., _DAT_8007C34C)` at `0x801F891C` and its three
        // siblings), and a scene load frees every scene node on it: the
        // `ending_panel_corner` state holds mask + panel + two sprites in
        // `edteien`, the `ending_scene_load_gap` state after it holds none,
        // and `ending_vignette_biron` holds only the mask `edbylon`'s own
        // script spawned. Kept, a vignette's corner panel would draw the
        // next scene's VRAM at `(512, 0)` over it.
        self.presentation.fx.clear();
        self.presentation.fx_frame = Default::default();
        self.field_vm.eased_moves.clear();
        // The second sweep's own class: `FUN_80037018` is the camera-register
        // zone ramp's handler (op `43 03..06`), whose records the engine holds
        // off the pool on `camera.register_ramps`. The sweep at
        // `0x8003B414..0x8003B43C` ORs `+0x10 |= 8` into every node whose
        // `+0x0C` is that handler, so no ramp of the last scene outlives the
        // load. The four registers it wrote go back to the zone-miss set the
        // file starts from: retail's registers ARE the zone block's words
        // (`FUN_801DBC20` stores all four at `0x801DBD90..0x801DBE84`), which
        // the new scene's first zone query rewrites. Kept, Ratayu's
        // projection ramp (`retockin` `P1[0]`, GTE `H` at `0x8007B618`) went
        // on writing `H` in every later scene, and its last value overrode
        // each next scene's zone `H` on the landing frame.
        // REF: FUN_80037018, FUN_801DBC20
        self.camera.register_ramps.clear();
        self.camera.registers = Default::default();
        // The property-ramp pool goes too. On an ordinary scene change
        // (`_DAT_8007B8B8 != 2`, the `bne` at `0x8003B510` to `0x8003B658`)
        // the loader runs an inlined copy of the pool reset `FUN_8003CDA8`
        // (`0x8003B854..0x8003B898`): every slot's busy word `+0x1E` zeroed,
        // slot 0's links cleared, `_DAT_801C66BE = 1`, and a fresh driver
        // actor spawned from `0x800742EC`. A floor-rung ramp (move-VM ext
        // `0x29`) or a timed `4C 49` camera-offset ramp still running at the
        // door therefore ends there instead of writing into the next scene's
        // ladder or offset. The return from a battle (`== 2`) jumps past it
        // (`j 0x8003B9A8` at `0x8003B650`), and so does the engine's, which
        // runs no MAN load.
        // REF: FUN_8003CDA8, FUN_8003C5F0
        self.terrain.floor_ladder_ramps.reset_pool();
        self.camera.offset_ramps.reset_pool();
        // The MAN loader resets the live audio level `_DAT_8007B910` to the
        // configured one, so a field script's duck (`4C 4D`) ends at the door.
        self.audio.field_duck_pct = Some(100);
        let present = self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some();
        let (seeds, outcome) = open_submode(present);
        self.field_vm.submode_context = seeds.map(|(_, v)| v);
        if outcome == SubmodeOpen::Spawned
            && let Some(slot) = self.spawn_handler_actor(ActorHandler::SubmodeDriver)
        {
            // Retail clears the fresh driver's `+0x50` and `+0x54`.
            self.actors[slot].state_54 = 0;
        }
        // `FUN_801DE478(0xF)`: the fixed-template spawn, from descriptor
        // `0x801F2810` whose `+8` word is `0x801DBE9C` - so the new actor
        // carries `ActorHandler::SceneActor` and the state word this returns.
        //
        // DIVERGENCE, deliberate: retail allocates a *fresh* node here every
        // MAN load and relies on the per-scene actor-list teardown to reclaim
        // the previous one. The engine has no list teardown, so an unqualified
        // spawn would leak a pool slot per scene change. Retiring the previous
        // scene actor first is the engine's stand-in for that teardown, and it
        // is scoped to this one handler class so it cannot touch anything a
        // script owns.
        self.retire_actors_by_handler(ActorHandler::SceneActor);
        self.retire_yielded_actors();
        if let Some(slot) = self.spawn_handler_actor(ActorHandler::SceneActor) {
            self.actors[slot].state_54 =
                scene_actor_initial_state(SCENE_ACTOR_REQUESTED_STATE, self.field_vm.mode_flags);
        }
        self.man_load_resume_programs();
        self.man_load_clear_pad_octant();
        outcome
    }

    /// The MAN loader's pad-octant reset.
    ///
    /// PORT: FUN_801E565C, overlay_field_0897_801e565c
    ///
    /// A three-word field-overlay leaf, `lui v0,0x8008; jr ra;
    /// sw zero,-0x4A10(v0)`: it zeroes the pad-rotation octant `gp+0x2D8`
    /// (`0x8007B5F0`). `FUN_8003AEB0` calls it at `0x8003B710` on the arm its
    /// `_DAT_8007B8B8 != 2` test takes (`0x8003B510`), i.e. on every ordinary
    /// scene change, so a scene that never authors an octant (field-VM op
    /// `4C 2x`) walks with the pad unrotated. The return-from-battle /
    /// minigame / FMV arm (`== 2`) skips it. The engine reaches this only
    /// through `SceneHost::load_scene`, which a battle return does not run.
    pub(crate) fn man_load_clear_pad_octant(&mut self) {
        self.locomotion.pad_octant = 0;
    }

    /// The pad-rotation octant `gp+0x2D8` (`0x8007B5F0`) a scene's script
    /// authors with field-VM op `4C 2x`.
    pub fn pad_octant(&self) -> u32 {
        self.locomotion.pad_octant
    }

    /// Re-spawn any scripted-scene program a scene change interrupted.
    ///
    /// PORT: FUN_8003AEB0 (`0x8003BAF0..0x8003BB3C`), driving
    /// [`legaia_engine_field::field_actor_program::spawn_program`] (`FUN_801D5A24`)
    ///
    /// The loader tests two bits of the shared flag bank and spawns the
    /// matching program for each. The pairs are in
    /// [`legaia_engine_field::field_actor_program::MAN_LOAD_RESUME`], and what they mean is
    /// on that const: each flag is set by an *opener* program and cleared by
    /// its *closer*, so a flag still set at scene load says the opener ran and
    /// its closer did not - the loader starts the closer. Returns the programs
    /// it spawned.
    ///
    /// The spawned actor is stepped by [`Self::tick_scene_programs`] from
    /// the handler pass.
    ///
    /// Carries the same deliberate divergence as the scene-actor spawn above:
    /// a previous load's program actor is retired first, standing in for the
    /// per-scene list teardown the engine does not have. Without it a flag
    /// that stays set across several scene changes seats one actor per change.
    pub fn man_load_resume_programs(&mut self) -> Vec<u16> {
        self.retire_actors_by_handler(ActorHandler::ScriptedScene);
        self.retire_yielded_actors();
        let mut spawned = Vec::new();
        for (flag, program) in legaia_engine_field::field_actor_program::MAN_LOAD_RESUME {
            if !self.system_flag_test(u16::from(flag)) {
                continue;
            }
            if self.spawn_scene_program(program).is_none() {
                break;
            }
            spawned.push(program);
        }
        spawned
    }

    /// Seat one scripted-scene program actor (`FUN_801D5A24(program)`): the
    /// MAN loader's closers above, and the travel arts' openers
    /// (`FUN_801EE094` / `FUN_801EE328` phase 0). Stepped by
    /// [`Self::tick_scene_programs`]. `None` when the pool is full.
    ///
    /// REF: FUN_801D5A24 (ported as [`legaia_engine_field::field_actor_program::spawn_program`])
    pub fn spawn_scene_program(&mut self, program: u16) -> Option<usize> {
        let slot = self.spawn_handler_actor(ActorHandler::ScriptedScene)?;
        let a = legaia_engine_field::field_actor_program::spawn_program(program);
        self.actors[slot].state_50 = a.program;
        self.actors[slot].state_54 = a.state;
        self.actors[slot].scene_program = Some(a);
        Some(slot)
    }

    /// Install a colour tween on `slot`, seating the handler the sweep and the
    /// frame dispatcher both key on.
    ///
    /// REF: FUN_80020DE0 (the template `+0x8` word this stands in for)
    ///
    /// The engine has no `0x801F28xx` spawn-descriptor table, so a caller
    /// supplies the block directly; what matters for fidelity is that the
    /// actor ends up carrying [`ActorHandler::ColourTween`] at `+0x0C`, since
    /// that is the identity `FUN_801D7518` retires on and
    /// [`Self::tick_handler_actors`] dispatches on.
    pub fn install_colour_tween(&mut self, slot: usize, tween: ColourTween) {
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        a.active = true;
        a.handler = ActorHandler::ColourTween;
        a.physics.status_flags = tween.flags;
        a.colour_tween = Some(tween);
    }

    /// Give a tween a script just spawned its first handler call, in the game
    /// frame that spawned it.
    ///
    /// Retail's frame walks the script's actor before the node its spawn
    /// linked, so the tween is ticked in the frame the op ran: a capture
    /// parked on the `4A` behind `34 01 FF FF FF 1E` (`koin3` `P2[6]`) holds
    /// the tween's clock `+0xC8` and the wait's sum `+0x54` on the same
    /// value, `21`, and both gain the frame step a frame. The engine's actor
    /// pass runs ahead of its script pass, so without this the tween's clock
    /// trails the wait by one game frame and every scripted fade draws one
    /// frame light. Only on a vsync that is a game frame: a script that
    /// spawns between two is not on retail's grid, and the pass that follows
    /// is less than a frame away.
    ///
    /// REF: FUN_8002519C (the walk), FUN_801DDC20 (the handler)
    pub(crate) fn tick_spawned_colour_tween(&mut self, slot: usize) {
        if !(self.clock.game_tick_live && self.clock.game_tick_fired) {
            return;
        }
        let delta = self.clock.frame_step.max(1);
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        let Some(tween) = a.colour_tween else {
            return;
        };
        let mut t = tween;
        t.flags = a.physics.status_flags;
        let step = legaia_engine_field::actor_colour_tween::step_colour_tween(t, delta);
        let mut next = tween;
        next.clock = step.clock;
        a.colour_tween = Some(next);
        a.physics.status_flags = step.flags;
        a.tint_push = step.push;
    }

    /// Spawn a colour tween on a free slot. `None` on a full pool.
    pub fn spawn_colour_tween(&mut self, tween: ColourTween) -> Option<usize> {
        let slot = self.spawn_handler_actor(ActorHandler::ColourTween)?;
        self.install_colour_tween(slot, tween);
        Some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::actor_colour_tween::TWEEN_HOLD_FOREVER;
    use legaia_engine_field::scene_transition_sweep::ACTOR_FLAG_TRANSITION;
    use legaia_engine_mode::mode::SceneMode;
    use legaia_engine_system::fade::FadeTemplate;

    fn world_with(handlers: &[ActorHandler]) -> World {
        let mut w = World::default();
        for (i, h) in handlers.iter().enumerate() {
            w.actors[i].active = true;
            w.actors[i].handler = *h;
        }
        w
    }

    /// The ending's screen-effect widgets are list-0 nodes, and a scene load
    /// frees them: `ending_panel_corner` holds mask + panel + two credit
    /// sprites in `edteien`, `ending_scene_load_gap` (the load into
    /// `edbylon`) holds none. A corner panel kept across the load drew the
    /// next scene's VRAM at `(512, 0)` over its vignette.
    #[test]
    fn a_scene_load_drops_the_screen_effect_widgets() {
        let mut w = World::default();
        // `43 11` mask to black, `43 13` the fullscreen photo panel.
        w.presentation.fx.mask_rect([0x20, 0x20, 0x20, 0x20, 0]);
        let mut panel = vec![0x13u8];
        for v in [0i16, 0, 0x140, 0xE0, 0x200, 0] {
            panel.extend_from_slice(&v.to_le_bytes());
        }
        w.presentation.fx.panel_spawn(&panel);
        w.presentation.fx.panel_move([16, 112, 0x700, 0]);
        w.presentation.fx_frame = w.presentation.fx.tick(1, |_| false);
        assert!(w.presentation.fx.is_active());
        assert!(!w.presentation.fx_frame.is_empty());
        w.man_load_actor_reset();
        assert!(!w.presentation.fx.is_active(), "every widget freed");
        assert!(w.presentation.fx_frame.is_empty(), "nothing left to draw");
    }

    /// The MAN loader's second sweep retires every `FUN_80037018` node, the
    /// camera-register zone ramp (`0x8003B414..0x8003B43C`), and the next
    /// scene's zone query rewrites the four registers. A ramp spawned by
    /// `retockin`'s entry script must not reach the next scene.
    #[test]
    fn a_scene_load_retires_the_camera_register_ramps() {
        use legaia_engine_field::register_ramp::{
            CameraRegisterFile, RampSlot, spawn_register_ramp,
        };
        let mut w = World::default();
        // `retockin` P1[0]: `43 03 0E 5A 13 5F 58 01 A0 00`.
        let ramp = spawn_register_ramp(3, [0x0E, 0x5A, 0x13, 0x5F], 0x158, 0xA0).unwrap();
        w.camera.register_ramps.push(ramp);
        w.camera.registers.set(RampSlot::Dat8007B618, 0xA0);
        assert!(w.camera.registers.written());
        w.man_load_actor_reset();
        assert!(w.camera.register_ramps.is_empty(), "the ramp is retired");
        assert_eq!(w.camera.registers, CameraRegisterFile::default());
        assert!(
            !w.camera.registers.written(),
            "the next scene's zone owns H"
        );
    }

    /// The MAN loader's inlined pool reset (`0x8003B854..0x8003B898`) ends a
    /// floor-rung ramp and a timed camera-offset ramp at the door.
    #[test]
    fn a_scene_load_resets_the_property_ramp_pool() {
        let mut w = World::default();
        w.ramp_camera_offset_ease(0x100, 60);
        w.ramp_camera_scene_offset(0x80, 60);
        w.terrain
            .floor_ladder_ramps
            .install(legaia_engine_motion_vm::ambient_motion::Ramp {
                dest: 3,
                owner: 0,
                start: 0,
                end: -0x80,
                total: 60,
                remaining: 60,
                kind: legaia_engine_motion_vm::ambient_motion::RampKind::U16,
            });
        assert_eq!(w.camera.offset_ramps.active(), 2);
        assert_eq!(w.terrain.floor_ladder_ramps.active(), 1);
        w.man_load_actor_reset();
        assert_eq!(w.camera.offset_ramps.active(), 0);
        assert_eq!(w.terrain.floor_ladder_ramps.active(), 0);
    }

    #[test]
    fn the_finder_skips_a_node_that_already_carries_the_kill_bit() {
        // The half of FUN_8003CF04 that is easy to lose: two actors on the
        // same handler, the earlier one killed, and the finder must return
        // the LATER one rather than the first address match.
        let mut w = world_with(&[ActorHandler::SubmodeDriver, ActorHandler::SubmodeDriver]);
        w.actors[0].physics.status_flags |= ACTOR_FLAG_YIELD;
        assert_eq!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver),
            Some(1)
        );
        // Kill the survivor too and the search reports a miss, which is what
        // makes a find-or-spawn API spawn instead of adopting a corpse.
        w.actors[1].physics.status_flags |= ACTOR_FLAG_YIELD;
        assert_eq!(w.find_actor_by_handler(ActorHandler::SubmodeDriver), None);
    }

    #[test]
    fn the_retire_sweep_marks_every_match_and_only_matches() {
        let mut w = world_with(&[
            ActorHandler::FloorLadder,
            ActorHandler::ColourTween,
            ActorHandler::FloorLadder,
        ]);
        assert_eq!(w.retire_floor_ladder_oscillators(), 2);
        assert_ne!(w.actors[0].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        assert_eq!(w.actors[1].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        assert_ne!(w.actors[2].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        // Inert when nothing is live on the handler - the property the
        // `4C 9F` "register callback" reading got wrong.
        let mut empty = World::default();
        assert_eq!(empty.retire_floor_ladder_oscillators(), 0);
    }

    #[test]
    fn the_transition_sweep_stamps_everyone_and_retires_the_three_classes() {
        let mut w = world_with(&[
            ActorHandler::ColourTween,
            ActorHandler::ActorTick,
            ActorHandler::Retail(0x8002_5000),
            ActorHandler::MorphWeights,
            ActorHandler::None,
        ]);
        let r = w.scene_transition_actor_sweep();
        assert_eq!(r.visited, 5);
        assert_eq!(r.retired, vec![0, 2, 3]);
        // The move-VM handler takes the long arm, not the retire.
        assert_eq!(r.move_vm_arms, vec![1]);
        for slot in 0..5 {
            assert_ne!(
                w.actors[slot].physics.status_flags & ACTOR_FLAG_TRANSITION,
                0,
                "slot {slot} missed the transition stamp"
            );
        }
    }

    #[test]
    fn a_swept_actor_actually_leaves_the_pool_on_the_next_tick() {
        // The property that separates "the sweep ran" from "the sweep did
        // something": marking bit 8 has to end in a deactivated slot, or the
        // whole teardown is a no-op with a flag word to show for it.
        let mut w = world_with(&[ActorHandler::ColourTween, ActorHandler::ActorTick]);
        w.scene_transition_actor_sweep();
        assert!(w.actors[0].active, "the sweep marks, it does not unlink");
        assert_eq!(w.tick_handler_actors(1), 1);
        assert!(!w.actors[0].active);
        assert!(w.actors[1].active, "an unmarked actor survives");
    }

    #[test]
    fn the_man_load_reset_spawns_a_driver_then_adopts_it() {
        let mut w = World::default();
        assert_eq!(w.man_load_actor_reset(), SubmodeOpen::Spawned);
        let driver = w
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .expect("a driver actor was spawned");
        // Second load with the driver still live: retail returns 0 and spawns
        // nothing, but the context reset happens either way.
        w.field_vm.submode_context = Default::default();
        assert_eq!(w.man_load_actor_reset(), SubmodeOpen::AlreadyOpen);
        assert_eq!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver),
            Some(driver)
        );
        assert_eq!(
            w.field_vm.submode_context[0],
            legaia_engine_field::field_submode::SUBMODE_STATE_OPEN
        );
    }

    #[test]
    fn the_man_load_reset_clears_the_pad_octant() {
        // FUN_801E565C, called from FUN_8003AEB0 at 0x8003B710: a scene
        // change leaves the pad unrotated until the new scene authors one.
        let mut w = World::default();
        w.locomotion.pad_octant = 5;
        w.man_load_actor_reset();
        assert_eq!(w.locomotion.pad_octant, 0);
    }

    #[test]
    fn a_driver_retired_before_the_open_is_not_adopted() {
        // Order inside FUN_8003AEB0: both `FUN_8003CF40` sweeps precede
        // `FUN_801D9C3C`, so a driver killed by one of them is invisible to
        // the open's find. Retiring it by hand reproduces the same shape.
        let mut w = World::default();
        w.man_load_actor_reset();
        let first = w
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .unwrap();
        w.retire_actors_by_handler(ActorHandler::SubmodeDriver);
        assert_eq!(w.man_load_actor_reset(), SubmodeOpen::Spawned);
        let second = w
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn the_scene_actor_state_word_follows_the_dev_flag() {
        let mut w = World::default();
        w.man_load_actor_reset();
        // Retail (`_DAT_8007B868 == 0`) seats the requested 0xF.
        assert!(
            w.actors
                .iter()
                .any(|a| a.active && a.state_54 == SCENE_ACTOR_REQUESTED_STATE)
        );
        let mut w = World::default();
        w.field_vm.mode_flags = 1;
        w.man_load_actor_reset();
        assert!(w.actors.iter().any(|a| a.active && a.state_54 == 1));
        assert!(
            !w.actors
                .iter()
                .any(|a| a.active && a.state_54 == SCENE_ACTOR_REQUESTED_STATE)
        );
    }

    #[test]
    fn a_battle_draws_no_field_screen_wash() {
        let mut w = World::default();
        let t = legaia_engine_field::actor_colour_tween::tween_from_fade_template(
            &FadeTemplate {
                kind: 2,
                duration: 64,
                start_rgb: [0, 0, 0x40],
                end_rgb: [0, 0, 0x40],
                mode: [0, 2, 0],
            },
            1,
        );
        w.spawn_colour_tween(t).expect("pool had room");
        w.tick_handler_actors(1);
        assert!(!w.screen_tint_pushes().is_empty(), "the field wash pushes");
        w.mode = SceneMode::Battle;
        assert!(
            w.screen_tint_pushes().is_empty(),
            "no field push in a fight"
        );
        assert!(w.screen_tint_push_args().is_empty());
    }

    #[test]
    fn a_spawned_tween_ticks_pushes_and_then_retires_itself() {
        // The producer -> dispatch -> output -> retire loop end to end, which
        // is the only way to tell a dispatched kernel from one dispatched over
        // nothing. Delay 0, duration 4, hold 2: pushes while it runs, gone
        // after.
        let mut w = World::default();
        let t = legaia_engine_field::actor_colour_tween::tween_from_fade_template(
            &FadeTemplate {
                kind: 2,
                duration: 4,
                start_rgb: [0, 0, 0],
                end_rgb: [0x40, 0x20, 0x10],
                mode: [0, 2, 0],
            },
            1,
        );
        let slot = w.spawn_colour_tween(t).expect("pool had room");
        let mut pushes = 0;
        for _ in 0..16 {
            w.tick_handler_actors(1);
            pushes += w.screen_tint_pushes().len();
            if !w.actors[slot].active {
                break;
            }
        }
        assert!(pushes > 0, "a live tween must push a colour every frame");
        assert!(!w.actors[slot].active, "the hold expiry must retire it");
        assert!(w.screen_tint_pushes().is_empty());
    }

    #[test]
    fn a_hold_forever_tween_is_only_ever_removed_by_the_transition_sweep() {
        // `hold == -1` never self-retires, so the scene-transition sweep is
        // the thing that stops a scripted fade leaking across a scene change.
        let mut w = World::default();
        let t = legaia_engine_field::actor_colour_tween::tween_from_fade_template(
            &FadeTemplate {
                kind: 0,
                duration: 2,
                start_rgb: [0; 3],
                end_rgb: [0xFF; 3],
                mode: [0, TWEEN_HOLD_FOREVER, 0],
            },
            1,
        );
        let slot = w.spawn_colour_tween(t).unwrap();
        for _ in 0..64 {
            w.tick_handler_actors(1);
        }
        assert!(w.actors[slot].active, "hold-forever holds");
        w.scene_transition_actor_sweep();
        w.tick_handler_actors(1);
        assert!(!w.actors[slot].active);
    }

    #[test]
    fn the_man_load_resumes_only_the_programs_whose_flag_survived() {
        use legaia_engine_field::field_actor_program::{FLAG_PROGRAM_1, FLAG_SCENE_ACTIVE};
        // No flag set: no program actor at all. This is the ordinary case, so
        // if the gate were inverted every scene load would spawn two.
        let mut w = World::default();
        assert!(w.man_load_resume_programs().is_empty());
        assert_eq!(w.find_actor_by_handler(ActorHandler::ScriptedScene), None);

        // Opener 0's flag survived a scene change -> its closer (program 2).
        let mut w = World::default();
        w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
        assert_eq!(w.man_load_resume_programs(), vec![2]);
        let slot = w
            .find_actor_by_handler(ActorHandler::ScriptedScene)
            .expect("the closer was spawned");
        assert_eq!(w.actors[slot].state_50, 2);
        assert_eq!(w.actors[slot].state_54, 0, "it enters at the entry state");

        // Both flags -> both closers, in the loader's order.
        let mut w = World::default();
        w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
        w.system_flag_set(u16::from(FLAG_PROGRAM_1));
        assert_eq!(w.man_load_resume_programs(), vec![2, 3]);
    }

    #[test]
    fn a_scene_load_resumes_an_interrupted_program() {
        // The whole chain in one: the flag an opener left behind survives into
        // `man_load_actor_reset`, which is what `SceneHost::load_scene` calls.
        let mut w = World::default();
        w.system_flag_set(u16::from(
            legaia_engine_field::field_actor_program::FLAG_PROGRAM_1,
        ));
        w.man_load_actor_reset();
        let slot = w
            .find_actor_by_handler(ActorHandler::ScriptedScene)
            .expect("the MAN-load reset resumed the program");
        assert_eq!(w.actors[slot].state_50, 3);
    }

    #[test]
    fn the_fade_template_decodes_the_same_way_for_both_of_its_consumers() {
        // FUN_801DE2B0 and FUN_80024E80 read one block. Duration and both RGB
        // triples must land identically, or one of the two decodes is wrong.
        let tpl = FadeTemplate {
            kind: 3,
            duration: 0x40,
            start_rgb: [0x11, 0x22, 0x33],
            end_rgb: [0x44, 0x55, 0x66],
            mode: [7, 9, -1],
        };
        let t = legaia_engine_field::actor_colour_tween::tween_from_fade_template(&tpl, 5);
        assert_eq!(t.duration, tpl.duration);
        assert_eq!(t.from, (0x11, 0x22, 0x33));
        assert_eq!(t.to, (0x44, 0x55, 0x66));
        // The two words `fade.rs` records as unpinned are the tween's delay
        // and hold; `[12]` is the one this arm never reads.
        assert_eq!(t.delay, tpl.mode[0]);
        assert_eq!(t.hold, tpl.mode[1]);
        // `kind` is the argument, NOT template [0] - that is the blend.
        assert_eq!(t.push_kind, 5);
        assert_eq!(t.push_blend, tpl.kind);
        assert_eq!(t.clock, 0);
    }

    /// The Room of Mirrors pair: an image placement re-staged to the lead's
    /// party model mirrors the player's clip, not only its pose
    /// (`FUN_801E5154` `0x801E5204..0x801E5250`). A new clip id is cued for
    /// the hosts' re-target with the party-bank bit on the image's channel;
    /// once bound, the next tick seats the player's cursor and step. A model
    /// mismatch keeps the image's own clip.
    #[test]
    fn a_mirror_image_copies_the_players_clip_when_the_models_agree() {
        use legaia_engine_field::field_anim::{FieldClipPlayer, FieldPlayerAnim, synth_anm_bundle};
        use legaia_engine_field::field_player_clip::PARTY_BANK_FLAG;
        let bundle = synth_anm_bundle(&[(2, 6), (2, 4), (2, 5), (2, 2), (2, 2), (2, 2), (2, 2)]);
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.install_field_player(0);
        let s = w.player_actor_slot.unwrap() as usize;
        w.actors[s].move_state.world_x = 35 * 128 + 64;
        w.actors[s].move_state.world_z = 96 * 128 + 64;
        let mut anim = FieldPlayerAnim::from_locomotion_bank(&bundle, 0).expect("bank");
        anim.select_retail_slot(Some(0));
        w.locomotion.player_anim = Some(anim);
        for _ in 0..3 {
            w.tick_field_player_anim();
        }
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 1,
                ctx: legaia_engine_vm::field::FieldCtx {
                    flags: 0x2_0000,
                    model_id: 0xF0,
                    move_id: 2,
                    ..Default::default()
                },
                record_offset: 0,
                pc: 0,
                done: false,
                object_bind: false,
            });
        w.npcs.positions.insert(1, (0, 0));
        w.set_field_npc_live_model(1, 0xF0);
        let idle = FieldClipPlayer::from_record(&bundle, 1).unwrap();
        w.bind_npc_clip_cursor(1, 2, &idle);
        w.field_vm.executing_channel = Some(1);
        let words = [0, 0x37A0, 30, 90, 38, 110];
        assert!(w.spawn_reflection_controller(false, 0xF8, words).is_some());
        w.field_vm.executing_channel = None;

        w.tick_handler_actors(1);
        assert_eq!(
            w.npcs.positions[&1],
            (35 * 128 + 64, 2 * 0x37A0 - (96 * 128 + 64)),
            "the pose is mirrored"
        );
        assert_eq!(w.npcs.anim_cues.get(&1).map(|c| c.1), Some(1), "walk cued");
        let ch = &w.field_vm.channels[0];
        assert_eq!(
            (ch.ctx.move_id, ch.ctx.flags & PARTY_BANK_FLAG),
            (1, PARTY_BANK_FLAG)
        );

        // The host's re-target binds the walk; the next copy seats the cursor.
        w.npcs.anim_cues.clear();
        let walk = FieldClipPlayer::from_record(&bundle, 0).unwrap();
        w.bind_npc_clip_cursor(1, 1, &walk);
        w.tick_field_player_anim();
        w.tick_handler_actors(1);
        let (_, _, cursor, _) = w.locomotion.player_anim.as_ref().unwrap().playing_clip();
        assert_eq!(w.npcs.clip_cursors[&1].cursor as u32, cursor);

        // Another model: the pose still mirrors, the clip stays the image's.
        w.set_field_npc_live_model(1, 0xF1);
        w.locomotion
            .player_anim
            .as_mut()
            .unwrap()
            .select_retail_slot(Some(2));
        w.tick_field_player_anim();
        w.tick_handler_actors(1);
        assert!(w.npcs.anim_cues.is_empty(), "no copy across models");
        assert_eq!(w.npcs.clip_cursors[&1].anim_id, 1);
    }
}
