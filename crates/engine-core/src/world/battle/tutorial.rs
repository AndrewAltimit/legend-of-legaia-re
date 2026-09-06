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
        self.battle_tutorial_script = script;
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

    /// Force the sparring tutorial onto the next [`World::enter_battle`],
    /// regardless of the disc condition, and install `script`.
    ///
    /// This is a **debug affordance**, not the port of anything: it exists so
    /// a host can reach the prompt machine in one run without playing to the
    /// Tetsu spar. The faithful path is [`Self::take_battle_tutorial_arm`],
    /// which needs nothing from the host.
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

    /// Every box on screen this frame: the **front group** of the queue.
    ///
    /// One retail hook dispatch registers all of its boxes at once - the
    /// lesson intro at the top and its explainer at the bottom share the
    /// frame at `Begin | Run` - and each is its own text actor, so a host
    /// draws the whole group, not the queue's head. A later dispatch's boxes
    /// wait behind the group until it has been dismissed.
    pub fn battle_tutorial_boxes_on_screen(&self) -> impl Iterator<Item = &ActiveTutorialBox> + '_ {
        let group = self.battle_tutorial_boxes.front().map(|b| b.group);
        self.battle_tutorial_boxes
            .iter()
            .take_while(move |b| Some(b.group) == group)
    }

    /// The next free dispatch group id for the box queue.
    pub(in crate::world) fn next_battle_tutorial_group(&self) -> u32 {
        self.battle_tutorial_boxes
            .back()
            .map_or(0, |b| b.group.wrapping_add(1))
    }

    /// The sparring fight's opening caption - the SCUS battle side-band
    /// tick's stage-1 arm (`FUN_80056208`), which the flow SM's round start
    /// waits behind.
    ///
    /// Retail keys the arm on the stage-1 phase byte `ctx[+0x289]`:
    ///
    /// ```text
    /// 800562c8  lbu  v1,0x6(a2)          ; phase 0: wait for ctx[+0x06] == 0x14
    /// 800562d0  bne  v1,v0,...           ;   (the round-start state the 0x0B timer stores)
    /// 800562e8  sb   v0,0x289(a2)        ; phase = 1
    /// 800562f8  sh   v0,0x6ae(a2)        ; hold timer = 0xB40 (drains 8 per frame)
    /// 80056318  addiu v0,v0,-0x734c      ; the caption string (SCUS 0x80078CB4)
    /// 80056320  _sw  v0,0x7494(v1)       ; -> the HUD caption pointer _DAT_80077494
    /// 8005631c  jal  0x801d8de8          ; raise HUD element 0x5A
    /// 80056360  jal  0x801d829c          ; aim the camera at the first monster seat
    /// 80056370  ...                      ; phase 1: timer -= 8/frame, ANY pad press zeroes it
    /// 80056400  sh   s0,0x6ae(v1)        ;   expired -> phase = 2 (the overlay-967 hook runs)
    /// 800565c4  sh   s0,0x6b0(v0)        ; ctx[+0x6B0] = 1 through phases 0..1
    /// ```
    ///
    /// `ctx[+0x6B0] != 0` is what parks the flow SM: `FUN_801D0748` tests it
    /// at `0x801D0BDC` and returns before its state switch, so the `0x14`
    /// arm - the actor sweep, the initiative seed, `Begin | Run` - does not
    /// run until the caption has gone. Phase `2` is the only phase that
    /// ticks the prompt machine (`jal 0x801f6b70` at `0x80056418`).
    ///
    /// The engine's box queue is the caption's carrier (both hosts already
    /// draw it, framed and text-measured), placed by the retail frame: the
    /// caption sits centred at the bottom anchor `0xCC`, which is the
    /// emitter's style `9` corner. Returns `true` when the round start has
    /// to wait; [`Self::tick_battle_tutorial_boxes`] advances the phase to
    /// `2` when the caption is dismissed and opens the round then.
    ///
    /// A world with no caption text (no disc) skips the hold outright rather
    /// than showing an empty window.
    ///
    /// PORT: FUN_80056208 (stage-1 arm; phases 0 and 1)
    pub(in crate::world) fn raise_sparring_caption_if_due(&mut self) -> bool {
        use crate::battle_tutorial::{SPARRING_CAPTION_FRAMES, SPARRING_CAPTION_STYLE};
        if self.battle_tutorial.is_none() || self.battle_sparring_phase != 0 {
            return false;
        }
        let Some(text) = self
            .battle_ui_strings
            .get(legaia_asset::battle_ui_strings::BattleUiLabel::SparringIntro)
            .map(str::to_string)
        else {
            self.battle_sparring_phase = 2;
            return false;
        };
        self.battle_sparring_phase = 1;
        let group = self.next_battle_tutorial_group();
        self.battle_tutorial_boxes.push_back(ActiveTutorialBox {
            text,
            style: SPARRING_CAPTION_STYLE,
            waits_for_input: false,
            frames_remaining: SPARRING_CAPTION_FRAMES,
            group,
            any_press_dismisses: true,
        });
        true
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

    /// The lesson the sparring fight is currently teaching, when armed.
    pub fn battle_tutorial_lesson(&self) -> Option<TutorialLesson> {
        self.battle_tutorial.as_ref().map(BattleTutorial::lesson)
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
    /// `FUN_80056208` phase-1 test is on the whole packed pad word.
    ///
    /// When the caption goes, the side-band phase advances to `2` and the
    /// round it was holding back opens (retail: `ctx[+0x6B0]` drops and the
    /// flow SM's `0x14` arm finally runs).
    pub(in crate::world) fn tick_battle_tutorial_boxes(&mut self) -> bool {
        use crate::input::PadButton;

        let Some(group) = self.battle_tutorial_boxes.front().map(|b| b.group) else {
            return false;
        };
        let confirm = self.input.just_pressed(PadButton::Cross);
        let any_press = self.input.pad() & !self.input.pad_prev() != 0;
        for b in self
            .battle_tutorial_boxes
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
            .battle_tutorial_boxes
            .iter()
            .position(|b| b.group != group)
            .unwrap_or(self.battle_tutorial_boxes.len());
        let mut i = 0;
        let mut end = keep_from;
        while i < end {
            if done(&self.battle_tutorial_boxes[i]) {
                self.battle_tutorial_boxes.remove(i);
                end -= 1;
            } else {
                i += 1;
            }
        }
        if self.battle_sparring_phase == 1 && self.battle_tutorial_boxes.is_empty() {
            self.battle_sparring_phase = 2;
            self.begin_battle_round();
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
        // One dispatch = one group: its boxes share the frame.
        let group = self.next_battle_tutorial_group();
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
                group,
                any_press_dismisses: false,
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
        } else if self.battle_arts_menu.is_some() || self.battle_arts_input.is_some() {
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
