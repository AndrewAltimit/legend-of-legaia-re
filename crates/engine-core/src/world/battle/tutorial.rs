//! Wiring for the sparring-tutorial prompt machine: the
//! `CommandPhase -> ctx[+0x06]` bridge, the box queue, and the hook points the
//! command flow calls into.
//!
//! [`crate::battle_tutorial`] is the ported machine; this is what makes it
//! *fire*. Retail's hook key is the command-flow byte `ctx[+0x06]`
//! ([`crate::battle_flow`]), which the engine has to recompose from its split
//! command session + submenus.
//!
//! ## Frame shape
//!
//! 1. [`World::tick_battle_tutorial_boxes`] runs first in the battle tick. If a
//!    box is up it ages / acknowledges it and the caller parks the whole battle
//!    loop - the port of retail's `ctx[+0x6B2]` guard, which makes
//!    `FUN_801D0748` return before it looks at the flow state at all.
//! 2. Otherwise the command flow runs, and each time it *changes* the flow
//!    state [`World::set_battle_flow`] clears the one-shot latch and dispatches
//!    the hook, queueing whatever boxes the `(state, lesson)` cross-product
//!    yields.
//! 3. A hook that takes the rewind exit reopens the command menu instead of
//!    letting the action through - the wrong-lesson bounce.

use super::*;

use crate::battle_flow::{ActiveTutorialBox, BattleFlowState, TUTORIAL_BOX_AUTO_FRAMES};
use crate::battle_tutorial::{BattleTutorial, BattleTutorialScript, TutorialLesson};

impl World {
    /// Stage the sparring tutorial: keep `script` and arm the machine at the
    /// next [`World::enter_battle`].
    ///
    /// This is the engine's stand-in for retail's stage-overlay dispatch. There
    /// the battle scene loader reads the stage id at `_DAT_8007B64A` and pages
    /// overlay 967 into slot B when it is
    /// [`crate::battle_tutorial::TUTORIAL_STAGE_ID`] - the one battle in the
    /// catalogued library that does (see
    /// [`crate::overlay_loader::battle_stage_overlay_entry`]). The engine has
    /// no per-formation stage id yet, so the host decides and primes here.
    pub fn prime_battle_tutorial(&mut self, script: BattleTutorialScript) {
        self.battle_tutorial_script = script;
        self.battle_tutorial_pending = true;
    }

    /// Arm the sparring tutorial right now, using the already-primed script.
    /// Called by [`World::enter_battle`] when a tutorial battle was primed;
    /// hosts and tests can call it directly.
    pub fn arm_battle_tutorial(&mut self) {
        self.battle_tutorial = Some(BattleTutorial::new());
        self.battle_tutorial_pending = false;
        self.battle_tutorial_boxes.clear();
        self.battle_flow = BattleFlowState::Idle;
    }

    /// `true` while a tutorial box is on screen. The battle loop parks on this
    /// (retail `ctx[+0x6B2]`).
    pub fn battle_tutorial_box_up(&self) -> bool {
        !self.battle_tutorial_boxes.is_empty()
    }

    /// The box currently on screen, if any.
    pub fn battle_tutorial_box(&self) -> Option<&ActiveTutorialBox> {
        self.battle_tutorial_boxes.front()
    }

    /// The lesson the sparring fight is currently teaching, when armed.
    pub fn battle_tutorial_lesson(&self) -> Option<TutorialLesson> {
        self.battle_tutorial.as_ref().map(BattleTutorial::lesson)
    }

    /// Age the box queue one frame. Returns `true` when a box is (still) up and
    /// the battle loop must park.
    ///
    /// A waiting box (styles `2..=7`) dismisses on Cross; a non-waiting box
    /// (`0`, `1`, `8`, `9`) counts itself down, and Cross skips it early so the
    /// player is never made to sit through a burst of them.
    pub(in crate::world) fn tick_battle_tutorial_boxes(&mut self) -> bool {
        use crate::input::PadButton;

        let Some(front) = self.battle_tutorial_boxes.front_mut() else {
            return false;
        };
        let confirm = self.input.just_pressed(PadButton::Cross);
        let done = if front.waits_for_input {
            confirm
        } else {
            front.frames_remaining = front.frames_remaining.saturating_sub(1);
            confirm || front.frames_remaining == 0
        };
        if done {
            self.battle_tutorial_boxes.pop_front();
        }
        true
    }

    /// Move the command flow to `next`, dispatching the tutorial hook on a
    /// change. Returns `true` when the hook took the rewind exit, i.e. the
    /// caller must bounce the player back to the command menu instead of
    /// letting the action through.
    ///
    /// The latch clear on entry is retail `0x801F71E8`; the dispatch is
    /// `FUN_801F6B70`.
    pub(in crate::world) fn set_battle_flow(&mut self, next: BattleFlowState) -> bool {
        if self.battle_flow == next {
            return false;
        }
        self.battle_flow = next;
        let Some(tut) = self.battle_tutorial.as_mut() else {
            return false;
        };
        // Entering a state re-arms the one-shot latch (retail 0x801F71E8), so
        // this state's hook gets exactly one dispatch.
        tut.enter_flow_state();
        // Note there is deliberately no `box_up` suppression here. Retail needs
        // one because `FUN_801D0748` keeps being called while a box is on
        // screen; the engine parks the whole battle tick instead
        // (`live_battle_tick`), so the only calls that reach this with a box
        // queued are the synchronous walks through consecutive states a single
        // resolution passes through - which must queue their boxes in order,
        // not drop the later ones.
        self.dispatch_battle_tutorial(next)
    }

    /// Run one hook dispatch for `state` and queue the resulting boxes.
    fn dispatch_battle_tutorial(&mut self, state: BattleFlowState) -> bool {
        let Some(mut tut) = self.battle_tutorial.take() else {
            return false;
        };
        let tick = tut.tick(state.raw());
        let rewind = tick.emission.rewind;
        for b in &tick.emission.boxes {
            let Some(text) = self.battle_tutorial_script.text(b.message) else {
                // No disc text for this VA - skip it rather than showing a
                // placeholder. A host booted without a disc shows no boxes.
                continue;
            };
            let waits_for_input = b.placement().is_some_and(|p| p.waits_for_input);
            self.battle_tutorial_boxes.push_back(ActiveTutorialBox {
                text: text.to_string(),
                style: b.style,
                waits_for_input,
                frames_remaining: TUTORIAL_BOX_AUTO_FRAMES,
            });
        }
        let over = tick.battle_over;
        self.battle_tutorial = Some(tut);
        if over {
            // The completion tail wrote ctx[0x06] = 0xC8 / ctx[0x07] = 0xFF:
            // the sparring fight is done. Disarm so the closing box is the last
            // thing the machine ever emits.
            self.battle_tutorial = None;
        }
        rewind
    }

    /// Recompute the flow state from `phase` plus the live submenus, and
    /// dispatch the hook on a change. `phase` is passed in rather than read off
    /// [`World::battle_command`] because the command flow drives its session
    /// detached from the World for the frame.
    pub(in crate::world) fn sync_battle_flow(
        &mut self,
        phase: Option<&crate::battle_input::CommandPhase>,
    ) -> bool {
        use crate::battle_flow::{BattleMenuKind, flow_state_for};

        let menu = if self.battle_item_menu.is_some() {
            BattleMenuKind::Item
        } else if self.battle_spell_menu.is_some() {
            BattleMenuKind::Magic
        } else if self.battle_arts_menu.is_some() {
            BattleMenuKind::Arts
        } else {
            BattleMenuKind::None
        };
        let next = flow_state_for(phase, menu);
        self.set_battle_flow(next)
    }

    /// Run the commit hook (flow state `110`) for a resolution that commits
    /// `category` (the retail `actor[+0x1DE]` byte). Returns `true` when the
    /// tutorial rejected it, so the caller must reopen the command menu.
    ///
    /// On acceptance the lesson is marked due to advance; the bump lands at the
    /// next turn start so this validator kept the lesson it validated against.
    pub(in crate::world) fn battle_tutorial_commit(&mut self, category: u8) -> bool {
        if self.battle_tutorial.is_none() {
            return false;
        }
        if let Some(tut) = self.battle_tutorial.as_mut() {
            tut.inputs.action_category = category;
        }
        let rewind = self.set_battle_flow(BattleFlowState::CommitBegin);
        if !rewind && let Some(tut) = self.battle_tutorial.as_mut() {
            let expected = tut.lesson().expected_action_category();
            if expected == Some(category) {
                tut.pending_advance = true;
            }
        }
        rewind
    }
}
