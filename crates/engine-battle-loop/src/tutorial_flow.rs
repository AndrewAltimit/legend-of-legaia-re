//! The command-flow byte and the sparring tutorial's prompt boxes: the
//! flow state the HUD and the tutorial read, the lesson groups the flow
//! changes queue, and the sparring caption.

use crate::battle_state::BattleState;
use legaia_engine_battle::battle_sideband as sb;
use legaia_engine_battle::battle_tutorial::{BattleTutorial, BattleTutorialScript, TutorialLesson};
use legaia_engine_battle_menus::battle_flow::{
    ActiveTutorialBox, BattleFlowState, TUTORIAL_BOX_AUTO_FRAMES,
};

impl BattleState {
    /// The sparring fight past its completion tail: the prompt machine's
    /// countdown (phase `2`, `ctx[+0x6B4] != 0` with the flow closed at
    /// `ctx[+0x06] = 0xC8`) or the teardown staging (phase `3`) owns the
    /// frame. Retail runs neither battle state machine through it: the flow
    /// byte `0xC8` is no `FUN_801D0748` case and the hook holds it
    /// (`ctx[+0x6B0] = 1`), and the action SM only runs off the command
    /// flow's `0xFF` (`0x80047024..0x8004702C`).
    pub fn sparring_closing(&self) -> bool {
        self.stage_id == sb::STAGE_SPARRING
            && (self.sideband.phase >= 3 || self.tutorial.as_ref().is_some_and(|t| t.finished))
    }

    /// Whether the battle open still stands between the sparring fight and
    /// its first `0x14`. The side-band arms its caption on the flow byte
    /// reading `0x14` (`FUN_80056208` phase `0`), and retail's flow gets
    /// there only through the whole open: the frame driver runs the battle
    /// tick `FUN_801D0748` once its entry counter `gp+0x330` reads `0xFF`
    /// (`beq a0,v0,0x80047014` at `0x80046EF8`) - after the camera's entry
    /// sweep - and the tick then walks `0x00 -> 0x0A`, composes the enemy
    /// names and holds `0x0B` for the intro timer `ctx[+0x6D6]`
    /// (`0x801D0DE0..0x801D0E58`) before it stores `0x14`. So Tetsu's line
    /// comes up after the sweep and after his name label has gone, never
    /// under either: `s5_tetsu_battle` (flow `0x00`, counter `0xAF`) shows
    /// the bare arena, `v0_1_battle_start_tetsu` (flow `0x14`) the caption
    /// over the close-up.
    ///
    /// The port opens every other fight's round at the flip
    /// (`super::intro_names`); the spar is held to retail's order because
    /// its caption and the close-up it selects are what the open shows.
    ///
    /// REF: FUN_80046A20, FUN_801D0748, FUN_80056208
    pub fn sparring_open_held(&self) -> bool {
        self.stage_id == sb::STAGE_SPARRING
            && self.sideband.phase == 0
            && (self.battle_entry_sweeping() || self.intro_names_frames > 0)
    }

    /// Queue the sparring caption - `IntroCaption`'s host half: the caption
    /// string `0x80078CB4` behind HUD element `0x5A`, carried on the tutorial
    /// box queue (both hosts draw it framed and text-measured) at the retail
    /// frame's placement, the emitter's style `9` corner. The side-band's own
    /// timer dismisses it, so the box never ages itself out.
    ///
    /// A world with no caption text (no disc) skips the hold outright rather
    /// than showing an empty window.
    pub fn raise_sparring_caption(&mut self) {
        use legaia_engine_battle::battle_tutorial::{
            SPARRING_CAPTION_FRAMES, SPARRING_CAPTION_RECT, SPARRING_CAPTION_STYLE,
        };
        use legaia_engine_battle_menus::battle_flow::ActiveTutorialBox;
        let Some(text) = self
            .ui_strings
            .get(legaia_ui_formats::battle_ui_strings::BattleUiLabel::SparringIntro)
            // The SCUS string carries the text engine's `'|'` hard break; the
            // box queue holds newlines (`ActiveTutorialBox::text`), and its
            // line count sizes the rect - left as `'|'` the two-line caption
            // registered a one-line box, 14 px short and 14 px low.
            .map(|s| s.replace('|', "\n"))
        else {
            self.sideband.phase = 2;
            self.sideband.hold = 0;
            return;
        };
        let group = self.next_battle_tutorial_group();
        self.tutorial_boxes.push_back(ActiveTutorialBox {
            text,
            style: SPARRING_CAPTION_STYLE,
            waits_for_input: false,
            frames_remaining: SPARRING_CAPTION_FRAMES,
            group,
            any_press_dismisses: true,
            // HUD element `0x5A`'s own placement record, not a measured box.
            placed: Some(SPARRING_CAPTION_RECT),
            stands: false,
        });
    }

    /// Install the disc-read prompt corpus. Text only - this does **not** arm
    /// anything, because in retail nothing about having the strings decides
    /// whether the tutorial runs.
    ///
    /// Hosts call this once the PROT index is open (the engine does it at
    /// scene entry via `SceneHost::ensure_battle_tutorial_script`), and the
    /// arming then comes from the disc's own condition - see
    /// `World::take_battle_tutorial_arm`. A world with no script still arms
    /// normally; it just resolves no box text and so shows nothing.
    pub fn set_battle_tutorial_script(&mut self, script: BattleTutorialScript) {
        self.tutorial_script = script;
    }

    /// Force the sparring tutorial onto the next `World::enter_battle`,
    /// regardless of the disc condition, and install `script`.
    ///
    /// This is a **debug affordance**, not the port of anything: it exists so
    /// a host can reach the prompt machine in one run without playing to the
    /// Tetsu spar. The faithful path is `World::take_battle_tutorial_arm`,
    /// which needs nothing from the host.
    pub fn prime_battle_tutorial(&mut self, script: BattleTutorialScript) {
        self.tutorial_script = script;
        self.tutorial_pending = true;
    }

    /// Arm the sparring tutorial right now, using the already-primed script.
    /// Called by `World::enter_battle` when a tutorial battle was primed;
    /// hosts and tests can call it directly.
    pub fn arm_battle_tutorial(&mut self) {
        self.tutorial = Some(BattleTutorial::new());
        self.stage_id = legaia_engine_battle::battle_tutorial::TUTORIAL_STAGE_ID;
        self.tutorial_pending = false;
        self.tutorial_boxes.clear();
        self.tutorial_standing = None;
        self.flow = BattleFlowState::Idle;
    }

    /// `true` while a tutorial box is on screen. The battle loop parks on this
    /// (retail `ctx[+0x6B2]`).
    pub fn battle_tutorial_box_up(&self) -> bool {
        !self.tutorial_boxes.is_empty()
    }

    /// The box currently on screen, if any: the queue's head, or with the
    /// queue empty the standing prompt
    /// (`legaia_engine_battle_loop::battle_state::BattleState::tutorial_standing`). Hosts park the
    /// HUD surfaces a box covers on it.
    pub fn battle_tutorial_box(&self) -> Option<&ActiveTutorialBox> {
        self.tutorial_boxes
            .front()
            .or(self.tutorial_standing.as_ref())
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
        let group = self.tutorial_boxes.front().map(|b| b.group);
        let front = move |b: &&ActiveTutorialBox| Some(b.group) == group;
        let replaced = self
            .tutorial_boxes
            .iter()
            .take_while(front)
            .any(|b| b.stands);
        self.tutorial_standing
            .iter()
            .filter(move |_| !replaced)
            .chain(self.tutorial_boxes.iter().take_while(front))
    }

    /// The next free dispatch group id for the box queue.
    pub fn next_battle_tutorial_group(&self) -> u32 {
        self.tutorial_boxes
            .back()
            .map_or(0, |b| b.group.wrapping_add(1))
    }

    /// The lesson the sparring fight is currently teaching, when armed.
    pub fn battle_tutorial_lesson(&self) -> Option<TutorialLesson> {
        self.tutorial.as_ref().map(BattleTutorial::lesson)
    }

    /// Move the command flow to `next`, dispatching the tutorial hook on a
    /// change. Returns `true` when the hook took the rewind exit, i.e. the
    /// caller must bounce the player back to the command menu instead of
    /// letting the action through.
    ///
    /// The latch clear on entry is retail `0x801F71E8`; the dispatch is
    /// `FUN_801F6B70`.
    pub fn set_battle_flow(&mut self, next: BattleFlowState) -> bool {
        if self.flow == next {
            return false;
        }
        self.flow = next;
        let Some(tut) = self.tutorial.as_mut() else {
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
    pub fn run_sparring_completion_tail(&mut self) {
        let Some(mut tut) = self.tutorial.take() else {
            return;
        };
        let mut tick = legaia_engine_battle::battle_tutorial::TutorialTick::default();
        tut.completion_tail(&mut tick);
        self.tutorial = Some(tut);
        self.queue_tutorial_boxes(&tick.emission);
    }

    /// Queue one dispatch's boxes as one on-screen group.
    pub fn queue_tutorial_boxes(
        &mut self,
        emission: &legaia_engine_battle::battle_tutorial::TutorialEmission,
    ) {
        let group = self.next_battle_tutorial_group();
        for b in &emission.boxes {
            let Some(text) = self.tutorial_script.text(b.message) else {
                // No disc text for this VA - skip it rather than showing a
                // placeholder. A host booted without a disc shows no boxes.
                continue;
            };
            let waits_for_input = b.placement().is_some_and(|p| p.waits_for_input);
            self.tutorial_boxes.push_back(ActiveTutorialBox {
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
    pub fn dispatch_battle_tutorial(&mut self, state: BattleFlowState) -> bool {
        let Some(mut tut) = self.tutorial.take() else {
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
        self.tutorial = Some(tut);
        rewind
    }

    /// Recompute the flow state from `phase` plus the live submenus, and
    /// dispatch the hook on a change. `phase` is passed in rather than read off
    /// `legaia_engine_battle_loop::battle_state::BattleState::command` because the command flow drives its session
    /// detached from the World for the frame.
    pub fn sync_battle_flow(
        &mut self,
        phase: Option<&legaia_engine_battle_menus::battle_input::CommandPhase>,
    ) -> bool {
        use legaia_engine_battle_menus::battle_flow::{BattleMenuKind, flow_state_for};

        let menu = if self.item_menu.is_some() {
            BattleMenuKind::Item
        } else if self.spell_menu.is_some() {
            BattleMenuKind::Magic
        } else if self.arts_menu.is_some() || self.arts_input.is_some() {
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
    pub fn battle_tutorial_commit(&mut self, category: u8) -> bool {
        if self.tutorial.is_none() {
            return false;
        }
        if let Some(tut) = self.tutorial.as_mut() {
            tut.inputs.action_category = category;
        }
        let rewind = self.set_battle_flow(BattleFlowState::CommitBegin);
        if !rewind && let Some(tut) = self.tutorial.as_mut() {
            let expected = tut.lesson().expected_action_category();
            if expected == Some(category) {
                tut.pending_advance = true;
            }
        }
        rewind
    }
}
