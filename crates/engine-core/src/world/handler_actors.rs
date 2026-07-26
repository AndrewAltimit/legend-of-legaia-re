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

use crate::actor_handler::{ActorHandler, MAN_LOAD_RETIRED_HANDLERS};
use crate::field_actor_kernels::{
    ACTOR_FLAG_YIELD, ColourTween, ScreenTintPush, SweepActor, SweepDecision, sweep_actor,
};
use crate::field_submode::{
    SCENE_ACTOR_REQUESTED_STATE, SubmodeOpen, open_submode, scene_actor_initial_state,
};

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
    /// Live: [`Self::man_load_actor_reset`], and the field VM's `4C 9F` /
    /// `4C 87` fade-cancel ops through [`Self::cancel_scripted_fades`].
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

    /// Cancel every running scripted fade - the field VM's `4C 9F` / `4C 87`
    /// ops, and the first of the MAN loader's two inlined sweeps.
    ///
    /// REF: FUN_8003CF40 against `LAB_801DA930`
    ///
    /// Named for what it does rather than for what those ops were called:
    /// `LAB_801DA930` is the handler on spawn descriptor `0x801F27EC`, the one
    /// the fade spawner `FUN_801DDE34` allocates from, so retiring it kills the
    /// fade. Nothing is *registered* anywhere - `FUN_8003CF40` has no return
    /// value and writes only the flag word.
    pub fn cancel_scripted_fades(&mut self) -> usize {
        self.retire_actors_by_handler(ActorHandler::FadeFamily)
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
    pub fn screen_tint_pushes(&self) -> Vec<ScreenTintPush> {
        self.actors
            .iter()
            .filter(|a| a.active)
            .filter_map(|a| a.tint_push)
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
    /// pool - today exactly [`crate::actor_handler::HandlerKernel::ColourTween`], per
    /// [`crate::actor_handler::HandlerKernel::runs_in_actor_loop`].
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
            if !a.handler.kernel().runs_in_actor_loop() {
                continue;
            }
            let Some(tween) = a.colour_tween else {
                continue;
            };
            let mut t = tween;
            t.flags = a.physics.status_flags;
            let step = crate::field_actor_kernels::step_colour_tween(t, frame_delta);
            let mut next = tween;
            next.clock = step.clock;
            a.colour_tween = Some(next);
            a.physics.status_flags = step.flags;
            a.tint_push = step.push;
        }
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
                a.tint_push = None;
                n += 1;
            }
        }
        n
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
        let present = self
            .find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some();
        let (seeds, outcome) = open_submode(present);
        self.submode_context = seeds.map(|(_, v)| v);
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
                scene_actor_initial_state(SCENE_ACTOR_REQUESTED_STATE, self.field_mode_flags);
        }
        self.man_load_resume_programs();
        outcome
    }

    /// Re-spawn any scripted-scene program a scene change interrupted.
    ///
    /// PORT: FUN_8003AEB0 (`0x8003BAF0..0x8003BB3C`), driving
    /// [`crate::field_actor_program::spawn_program`] (`FUN_801D5A24`)
    ///
    /// The loader tests two bits of the shared flag bank and spawns the
    /// matching program for each. The pairs are in
    /// [`crate::field_actor_program::MAN_LOAD_RESUME`], and what they mean is
    /// on that const: each flag is set by an *opener* program and cleared by
    /// its *closer*, so a flag still set at scene load says the opener ran and
    /// its closer did not - the loader starts the closer. Returns the programs
    /// it spawned.
    ///
    /// Nothing ticks the spawned actor yet; the gap is disclosed on
    /// [`crate::field_actor_program::step`].
    ///
    /// Carries the same deliberate divergence as the scene-actor spawn above:
    /// a previous load's program actor is retired first, standing in for the
    /// per-scene list teardown the engine does not have. Without it a flag
    /// that stays set across several scene changes seats one actor per change.
    pub fn man_load_resume_programs(&mut self) -> Vec<u16> {
        self.retire_actors_by_handler(ActorHandler::ScriptedScene);
        self.retire_yielded_actors();
        let mut spawned = Vec::new();
        for (flag, program) in crate::field_actor_program::MAN_LOAD_RESUME {
            if !self.system_flag_test(u16::from(flag)) {
                continue;
            }
            let Some(slot) = self.spawn_handler_actor(ActorHandler::ScriptedScene) else {
                break;
            };
            let a = crate::field_actor_program::spawn_program(program);
            self.actors[slot].state_50 = a.program;
            self.actors[slot].state_54 = a.state;
            spawned.push(program);
        }
        spawned
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
    use crate::fade::FadeTemplate;
    use crate::field_actor_kernels::{ACTOR_FLAG_TRANSITION, TWEEN_HOLD_FOREVER};

    fn world_with(handlers: &[ActorHandler]) -> World {
        let mut w = World::default();
        for (i, h) in handlers.iter().enumerate() {
            w.actors[i].active = true;
            w.actors[i].handler = *h;
        }
        w
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
            ActorHandler::FadeFamily,
            ActorHandler::ColourTween,
            ActorHandler::FadeFamily,
        ]);
        assert_eq!(w.cancel_scripted_fades(), 2);
        assert_ne!(w.actors[0].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        assert_eq!(w.actors[1].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        assert_ne!(w.actors[2].physics.status_flags & ACTOR_FLAG_YIELD, 0);
        // Inert when nothing is live on the handler - the property the
        // `4C 9F` "register callback" reading got wrong.
        let mut empty = World::default();
        assert_eq!(empty.cancel_scripted_fades(), 0);
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
        w.submode_context = [0; 10];
        assert_eq!(w.man_load_actor_reset(), SubmodeOpen::AlreadyOpen);
        assert_eq!(
            w.find_actor_by_handler(ActorHandler::SubmodeDriver),
            Some(driver)
        );
        assert_eq!(
            w.submode_context[0],
            crate::field_submode::SUBMODE_STATE_OPEN
        );
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
        let mut w = World {
            field_mode_flags: 1,
            ..Default::default()
        };
        w.man_load_actor_reset();
        assert!(w.actors.iter().any(|a| a.active && a.state_54 == 1));
        assert!(
            !w.actors
                .iter()
                .any(|a| a.active && a.state_54 == SCENE_ACTOR_REQUESTED_STATE)
        );
    }

    #[test]
    fn a_spawned_tween_ticks_pushes_and_then_retires_itself() {
        // The producer -> dispatch -> output -> retire loop end to end, which
        // is the only way to tell a dispatched kernel from one dispatched over
        // nothing. Delay 0, duration 4, hold 2: pushes while it runs, gone
        // after.
        let mut w = World::default();
        let t = crate::field_actor_kernels::tween_from_fade_template(
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
        let t = crate::field_actor_kernels::tween_from_fade_template(
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
        use crate::field_actor_program::{FLAG_PROGRAM_1, FLAG_SCENE_ACTIVE};
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
        w.system_flag_set(u16::from(crate::field_actor_program::FLAG_PROGRAM_1));
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
        let t = crate::field_actor_kernels::tween_from_fade_template(&tpl, 5);
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
}
