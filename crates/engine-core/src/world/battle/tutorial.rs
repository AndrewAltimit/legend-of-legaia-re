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
    /// Install the disc-read prompt corpus. Text only - this does **not** arm
    /// anything, because in retail nothing about having the strings decides
    /// whether the tutorial runs.
    ///
    /// Hosts call this once the PROT index is open (the engine does it at
    /// scene entry via `SceneHost::ensure_battle_tutorial_script`), and the
    /// arming then comes from the disc's own condition - see
    /// [`Self::take_battle_tutorial_arm`]. A world with no script still arms
    /// normally; it just resolves no box text and so shows nothing.
    pub fn set_battle_tutorial_script(&mut self, script: BattleTutorialScript) {
        self.battle.tutorial_script = script;
    }

    /// Retail's own sparring-tutorial condition, consumed at battle entry.
    ///
    /// Tests [`crate::battle_tutorial::TUTORIAL_ARM_FLAG`] in the system-flag
    /// bank and, when it is set, clears it and reports the armed stage. The
    /// clear is what makes the tutorial fire for exactly one battle - the same
    /// `TEST` / `CLEAR` pair the entity SM's battle-entry tail runs before it
    /// writes the stage-id byte `_DAT_8007B64A`.
    ///
    /// The flag itself is set by the field VM executing town01's Tetsu record
    /// (`50 19`, two ops before its `3E FF` battle-entry op), so this needs no
    /// host cooperation: any host that runs the field VM and enters battles
    /// gets the tutorial in the same fight retail does, and no other.
    ///
    /// The tail's arithmetic itself lives in
    /// [`crate::battle_tutorial::stage_id_at_battle_entry`], which carries the
    /// `PORT` anchor; this is its live consumption site.
    ///
    /// REF: FUN_801DA51C (`0x801DA698..0x801DA6B0`)
    pub fn take_battle_tutorial_arm(&mut self) -> bool {
        use crate::battle_tutorial::{
            TUTORIAL_ARM_FLAG, TUTORIAL_STAGE_ID, stage_id_at_battle_entry,
        };
        let armed = self.system_flag_test(TUTORIAL_ARM_FLAG);
        // Retail writes the stage id unconditionally (0 in the delay slot) and
        // only overwrites it on the set arm; going through the same resolver
        // keeps the "which stage overlay" question in one place.
        if stage_id_at_battle_entry(armed) != TUTORIAL_STAGE_ID {
            return false;
        }
        self.system_flag_clear(TUTORIAL_ARM_FLAG);
        true
    }

    /// Whether the next [`World::enter_battle`] is the sparring tutorial: the
    /// disc's one-shot arm flag is up (not consumed - the entry consumes it)
    /// or a host forced it with [`Self::prime_battle_tutorial`].
    pub fn sparring_fight_pending(&self) -> bool {
        self.battle.tutorial_pending
            || self.system_flag_test(crate::battle_tutorial::TUTORIAL_ARM_FLAG)
    }

    /// Force the sparring tutorial onto the next [`World::enter_battle`],
    /// regardless of the disc condition, and install `script`.
    ///
    /// This is a **debug affordance**, not the port of anything: it exists so
    /// a host can reach the prompt machine in one run without playing to the
    /// Tetsu spar. The faithful path is [`Self::take_battle_tutorial_arm`],
    /// which needs nothing from the host.
    pub fn prime_battle_tutorial(&mut self, script: BattleTutorialScript) {
        self.battle.tutorial_script = script;
        self.battle.tutorial_pending = true;
    }

    /// Arm the sparring tutorial right now, using the already-primed script.
    /// Called by [`World::enter_battle`] when a tutorial battle was primed;
    /// hosts and tests can call it directly.
    pub fn arm_battle_tutorial(&mut self) {
        self.battle.tutorial = Some(BattleTutorial::new());
        self.battle.stage_id = crate::battle_tutorial::TUTORIAL_STAGE_ID;
        self.battle.tutorial_pending = false;
        self.battle.tutorial_boxes.clear();
        self.battle.tutorial_standing = None;
        self.battle.flow = BattleFlowState::Idle;
    }

    /// `true` while a tutorial box is on screen. The battle loop parks on this
    /// (retail `ctx[+0x6B2]`).
    pub fn battle_tutorial_box_up(&self) -> bool {
        !self.battle.tutorial_boxes.is_empty()
    }

    /// The box currently on screen, if any: the queue's head, or with the
    /// queue empty the standing prompt
    /// ([`crate::world::BattleState::tutorial_standing`]). Hosts park the
    /// HUD surfaces a box covers on it.
    pub fn battle_tutorial_box(&self) -> Option<&ActiveTutorialBox> {
        self.battle
            .tutorial_boxes
            .front()
            .or(self.battle.tutorial_standing.as_ref())
    }

    /// Every box on screen this frame: the **front group** of the queue.
    ///
    /// One retail hook dispatch registers all of its boxes at once - the
    /// lesson intro at the top and its explainer at the bottom share the
    /// frame at `Begin | Run` - and each is its own text actor, so a host
    /// draws the whole group, not the queue's head. A later dispatch's boxes
    /// wait behind the group until it has been dismissed.
    ///
    /// The standing prompt draws under them: it is the key-`1` actor, and a
    /// queued self-dismissing box is the registration that replaces it, so
    /// it is yielded only while the front group brings none of its own.
    pub fn battle_tutorial_boxes_on_screen(&self) -> impl Iterator<Item = &ActiveTutorialBox> + '_ {
        let group = self.battle.tutorial_boxes.front().map(|b| b.group);
        let front = move |b: &&ActiveTutorialBox| Some(b.group) == group;
        let replaced = self
            .battle
            .tutorial_boxes
            .iter()
            .take_while(front)
            .any(|b| b.stands);
        self.battle
            .tutorial_standing
            .iter()
            .filter(move |_| !replaced)
            .chain(self.battle.tutorial_boxes.iter().take_while(front))
    }

    /// The next free dispatch group id for the box queue.
    pub(in crate::world) fn next_battle_tutorial_group(&self) -> u32 {
        self.battle
            .tutorial_boxes
            .back()
            .map_or(0, |b| b.group.wrapping_add(1))
    }

    /// Replay the one-shot system-flag arm the record that enters formation
    /// row `formation_id` raises before its `3E FF <row>` battle-entry op,
    /// for a direct entry into that row (`--battle <row>`), which runs the
    /// entry without the record.
    ///
    /// The disc-side condition is the pairing itself: the scene's own
    /// field-VM script carries a `SET` of
    /// [`crate::battle_tutorial::TUTORIAL_ARM_FLAG`] within a few coherently
    /// decoded ops of a `3E FF` that enters `formation_id`
    /// ([`Self::scene_battle_entry_arms`], read off the MAN at carrier
    /// install; the shape is [`crate::man_field_scripts::BattleEntryArm`]).
    /// A disc-wide census finds that SET in exactly one record - town01's
    /// sparring record, `50 19` three ops before its `3E FF 04` - so this
    /// raises the flag for the Tetsu fight and for nothing else. Returns
    /// `true` when it armed.
    ///
    /// The faithful path needs none of this: the field VM executing the
    /// record raises the flag itself ([`Self::take_battle_tutorial_arm`]).
    pub fn replay_scripted_battle_arm(&mut self, formation_id: u16) -> bool {
        use crate::battle_tutorial::TUTORIAL_ARM_FLAG;
        let armed = self
            .scene_battle_entry_arms
            .iter()
            .any(|a| a.flag == TUTORIAL_ARM_FLAG && u16::from(a.row) == formation_id);
        if !armed {
            return false;
        }
        self.system_flag_set(TUTORIAL_ARM_FLAG);
        true
    }

    /// Replay the op-`0x35` BGM words the record that enters formation row
    /// `formation_id` runs before its `3E FF <row>` battle-entry op - its
    /// last track start with the control words after it, then its battle
    /// sound-set selection ([`crate::man_field_scripts::BattleEntryScore`]),
    /// for a direct entry into that row, which runs the entry without the
    /// record.
    ///
    /// Each word goes through the field VM's own op-`0x35` handler, so the
    /// track word, the battle sound set and the host's BGM events are what
    /// the record would have left: `korb3`'s Gaza row starts `2028` and
    /// selects set `-1`, so the fight plays on that theme rather than on the
    /// scene's parked entry track and the default battle theme. A row two
    /// records enter replays the first. Returns `true` when it replayed.
    ///
    /// The faithful path needs none of this: the field VM executing the
    /// record runs the words itself.
    pub fn replay_scripted_battle_score(&mut self, formation_id: u16) -> bool {
        let Some(words) = self
            .scene_battle_entry_scores
            .iter()
            .find(|s| u16::from(s.row) == formation_id)
            .map(|s| s.words.clone())
        else {
            return false;
        };
        self.replay_field_bgm_words(&words);
        true
    }

    /// Run op-`0x35` BGM words through the field VM's own handler, in order -
    /// the track word, the battle sound set and the host's BGM events end up
    /// where a record executing those ops would have left them. The replay
    /// primitive behind [`Self::replay_scripted_battle_score`] and the retail
    /// comparison's resume of a spawned record
    /// ([`crate::man_field_scripts::SpawnScore`]).
    pub fn replay_field_bgm_words(&mut self, words: &[(u16, u8)]) {
        let mut host = crate::world::vm_hosts::FieldHostImpl { world: self };
        for &(text_id, sub_op) in words {
            legaia_engine_vm::field::FieldHost::bgm(&mut host, text_id, sub_op);
        }
    }

    /// The lesson the sparring fight is currently teaching, when armed.
    pub fn battle_tutorial_lesson(&self) -> Option<TutorialLesson> {
        self.battle.tutorial.as_ref().map(BattleTutorial::lesson)
    }

    /// Age the box queue one frame. Returns `true` when a box is (still) up and
    /// the battle loop must park.
    ///
    /// The whole front group ages together (one dispatch's boxes are on
    /// screen at once - [`Self::battle_tutorial_boxes_on_screen`]). Inside
    /// it, a waiting box (styles `2..=7`) dismisses on Cross; a non-waiting
    /// box (`0`, `1`, `8`, `9`) counts itself down, and Cross skips it early
    /// so the player is never made to sit through a burst of them. The
    /// sparring caption is the one box any pad press dismisses - retail's
    /// `FUN_80056208` phase-1 test is on the whole packed pad word - and the
    /// side-band tick, which runs ahead of this every frame, is what retires
    /// it and opens the round it was holding back
    /// (`world/battle/sideband.rs`).
    pub(in crate::world) fn tick_battle_tutorial_boxes(&mut self) -> bool {
        use crate::input::PadButton;

        let Some(group) = self.battle.tutorial_boxes.front().map(|b| b.group) else {
            return false;
        };
        let confirm = self.input.just_pressed(PadButton::Cross);
        let any_press = self.input.pad() & !self.input.pad_prev() != 0;
        for b in self
            .battle
            .tutorial_boxes
            .iter_mut()
            .take_while(|b| b.group == group)
        {
            if !b.waits_for_input {
                b.frames_remaining = b.frames_remaining.saturating_sub(1);
            }
        }
        let done = |b: &ActiveTutorialBox| {
            if b.waits_for_input {
                confirm
            } else {
                confirm || (b.any_press_dismisses && any_press) || b.frames_remaining == 0
            }
        };
        // Drop the finished members of the front group only; the ones behind
        // it are a later dispatch and keep their frames.
        let keep_from = self
            .battle
            .tutorial_boxes
            .iter()
            .position(|b| b.group != group)
            .unwrap_or(self.battle.tutorial_boxes.len());
        // The hook's unregister pair: under a waiting box at flow `0x5A` it
        // frees text actors `0` and `1` (`0x801F71BC..0x801F71D8`), so the
        // standing prompt goes when the target cursor opens over one.
        if self.battle.flow.raw() == 0x5A
            && self
                .battle
                .tutorial_boxes
                .iter()
                .take(keep_from)
                .any(|b| b.waits_for_input)
        {
            self.battle.tutorial_standing = None;
        }
        let mut i = 0;
        let mut end = keep_from;
        while i < end {
            if done(&self.battle.tutorial_boxes[i]) {
                // A self-dismissing prompt leaves the queue, not the
                // screen: it is the key-`1` actor until the next one.
                if let Some(b) = self.battle.tutorial_boxes.remove(i)
                    && b.stands
                {
                    self.battle.tutorial_standing = Some(b);
                }
                end -= 1;
            } else {
                i += 1;
            }
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
        if self.battle.flow == next {
            return false;
        }
        self.battle.flow = next;
        let Some(tut) = self.battle.tutorial.as_mut() else {
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

    /// The completion tail on a frame with no flow edge (the side-band's
    /// per-frame hook call): queue the sign-off box it emits.
    pub(in crate::world) fn run_sparring_completion_tail(&mut self) {
        let Some(mut tut) = self.battle.tutorial.take() else {
            return;
        };
        let mut tick = crate::battle_tutorial::TutorialTick::default();
        tut.completion_tail(&mut tick);
        self.battle.tutorial = Some(tut);
        self.queue_tutorial_boxes(&tick.emission);
    }

    /// Queue one dispatch's boxes as one on-screen group.
    fn queue_tutorial_boxes(&mut self, emission: &crate::battle_tutorial::TutorialEmission) {
        let group = self.next_battle_tutorial_group();
        for b in &emission.boxes {
            let Some(text) = self.battle.tutorial_script.text(b.message) else {
                // No disc text for this VA - skip it rather than showing a
                // placeholder. A host booted without a disc shows no boxes.
                continue;
            };
            let waits_for_input = b.placement().is_some_and(|p| p.waits_for_input);
            self.battle.tutorial_boxes.push_back(ActiveTutorialBox {
                stands: !waits_for_input,
                text: text.to_string(),
                style: b.style,
                waits_for_input,
                frames_remaining: TUTORIAL_BOX_AUTO_FRAMES,
                group,
                any_press_dismisses: false,
                placed: None,
            });
        }
    }

    /// Run one hook dispatch for `state` and queue the resulting boxes.
    fn dispatch_battle_tutorial(&mut self, state: BattleFlowState) -> bool {
        let Some(mut tut) = self.battle.tutorial.take() else {
            return false;
        };
        let tick = tut.tick(state.raw());
        let rewind = tick.emission.rewind;
        // One dispatch = one group: its boxes share the frame.
        self.queue_tutorial_boxes(&tick.emission);
        // The completion tail (`tick.battle_over`) wrote ctx[0x06] = 0xC8 /
        // ctx[0x07] = 0xFF and armed the `ctx[+0x6B4]` countdown. The machine
        // stays armed: its per-frame countdown (the side-band's phase-2 hook,
        // `World::tick_battle_sideband`) is what ends the fight, and the
        // closed command flow never raises another hook state.
        self.battle.tutorial = Some(tut);
        rewind
    }

    /// Recompute the flow state from `phase` plus the live submenus, and
    /// dispatch the hook on a change. `phase` is passed in rather than read off
    /// [`crate::world::BattleState::command`] because the command flow drives its session
    /// detached from the World for the frame.
    pub(in crate::world) fn sync_battle_flow(
        &mut self,
        phase: Option<&crate::battle_input::CommandPhase>,
    ) -> bool {
        use crate::battle_flow::{BattleMenuKind, flow_state_for};

        let menu = if self.battle.item_menu.is_some() {
            BattleMenuKind::Item
        } else if self.battle.spell_menu.is_some() {
            BattleMenuKind::Magic
        } else if self.battle.arts_menu.is_some() || self.battle.arts_input.is_some() {
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
        if self.battle.tutorial.is_none() {
            return false;
        }
        if let Some(tut) = self.battle.tutorial.as_mut() {
            tut.inputs.action_category = category;
        }
        let rewind = self.set_battle_flow(BattleFlowState::CommitBegin);
        if !rewind && let Some(tut) = self.battle.tutorial.as_mut() {
            let expected = tut.lesson().expected_action_category();
            if expected == Some(category) {
                tut.pending_advance = true;
            }
        }
        rewind
    }
}
