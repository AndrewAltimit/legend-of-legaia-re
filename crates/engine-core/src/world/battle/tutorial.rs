//! Wiring for the sparring-tutorial prompt machine: the
//! `CommandPhase -> ctx[+0x06]` bridge, the box queue, and the hook points the
//! command flow calls into.
//!
//! [`legaia_engine_battle::battle_tutorial`] is the ported machine; this is what makes it
//! *fire*. Retail's hook key is the command-flow byte `ctx[+0x06]`
//! ([`legaia_engine_battle_menus::battle_flow`]), which the engine has to recompose from its split
//! command session + submenus.
//!
//! ## Frame shape
//!
//! 1. [`World::tick_battle_tutorial_boxes`] runs first in the battle tick. If a
//!    box is up it ages / acknowledges it and the caller parks the whole battle
//!    loop - the port of retail's `ctx[+0x6B2]` guard, which makes
//!    `FUN_801D0748` return before it looks at the flow state at all.
//! 2. Otherwise the command flow runs, and each time it *changes* the flow
//!    state [`BattleState::set_battle_flow`] clears the one-shot latch and dispatches
//!    the hook, queueing whatever boxes the `(state, lesson)` cross-product
//!    yields.
//! 3. A hook that takes the rewind exit reopens the command menu instead of
//!    letting the action through - the wrong-lesson bounce.

use super::*;

use legaia_engine_battle_menus::battle_flow::ActiveTutorialBox;

impl World {
    /// Retail's own sparring-tutorial condition, consumed at battle entry.
    ///
    /// Tests [`legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG`] in the system-flag
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
    /// [`legaia_engine_battle::battle_tutorial::stage_id_at_battle_entry`], which carries the
    /// `PORT` anchor; this is its live consumption site.
    ///
    /// REF: FUN_801DA51C (`0x801DA698..0x801DA6B0`)
    pub fn take_battle_tutorial_arm(&mut self) -> bool {
        use legaia_engine_battle::battle_tutorial::{
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
    /// or a host forced it with [`BattleState::prime_battle_tutorial`].
    pub fn sparring_fight_pending(&self) -> bool {
        self.battle.tutorial_pending
            || self.system_flag_test(legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG)
    }

    /// Replay the one-shot system-flag arm the record that enters formation
    /// row `formation_id` raises before its `3E FF <row>` battle-entry op,
    /// for a direct entry into that row (`--battle <row>`), which runs the
    /// entry without the record.
    ///
    /// The disc-side condition is the pairing itself: the scene's own
    /// field-VM script carries a `SET` of
    /// [`legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG`] within a few coherently
    /// decoded ops of a `3E FF` that enters `formation_id`
    /// ([`Self::scene_battle_entry_arms`], read off the MAN at carrier
    /// install; the shape is [`legaia_engine_field::man_field_scripts::BattleEntryArm`]).
    /// A disc-wide census finds that SET in exactly one record - town01's
    /// sparring record, `50 19` three ops before its `3E FF 04` - so this
    /// raises the flag for the Tetsu fight and for nothing else. Returns
    /// `true` when it armed.
    ///
    /// The faithful path needs none of this: the field VM executing the
    /// record raises the flag itself ([`Self::take_battle_tutorial_arm`]).
    pub fn replay_scripted_battle_arm(&mut self, formation_id: u16) -> bool {
        use legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG;
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
    /// sound-set selection ([`legaia_engine_field::man_field_scripts::BattleEntryScore`]),
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
    /// ([`legaia_engine_field::man_field_scripts::SpawnScore`]).
    pub fn replay_field_bgm_words(&mut self, words: &[(u16, u8)]) {
        let mut host = crate::world::vm_hosts::FieldHostImpl { world: self };
        for &(text_id, sub_op) in words {
            legaia_engine_vm::field::FieldAudioHost::bgm(&mut host, text_id, sub_op);
        }
    }

    /// Age the box queue one frame. Returns `true` when a box is (still) up and
    /// the battle loop must park.
    ///
    /// The whole front group ages together (one dispatch's boxes are on
    /// screen at once - [`BattleState::battle_tutorial_boxes_on_screen`]). Inside
    /// it, a waiting box (styles `2..=7`) dismisses on Cross; a non-waiting
    /// box (`0`, `1`, `8`, `9`) counts itself down, and Cross skips it early
    /// so the player is never made to sit through a burst of them. The
    /// sparring caption is the one box any pad press dismisses - retail's
    /// `FUN_80056208` phase-1 test is on the whole packed pad word - and the
    /// side-band tick, which runs ahead of this every frame, is what retires
    /// it and opens the round it was holding back
    /// (`world/battle/sideband.rs`).
    pub(in crate::world) fn tick_battle_tutorial_boxes(&mut self) -> bool {
        use legaia_engine_system::input::PadButton;

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
}
