//! Battle sparring-tutorial prompt machine - the in-battle "how to fight"
//! boxes of the scripted Tetsu tutorial fight in the prologue.
//!
//! PORT: FUN_801F6B70, FUN_801F747C
//! REF: FUN_801F7628, FUN_8003CBA8, FUN_80035F04
//!
//! Ported from the **battle-stage slot-B overlay, extraction PROT 967**
//! (`base_va = 0x801F69D8`, label `battle_tutorial` in
//! `crates/asset/data/static-overlays.toml`). That overlay is the only
//! occupant the `+0x47` stage-overlay band can page in for battle-stage id
//! `1`, which is the sparring fight's id - see
//! [`docs/subsystems/battle.md`](../../../docs/subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band).
//!
//! ## Why the engine showed no intro dialogue
//!
//! The tutorial boxes are **not** battle-scene script, not MES text and not
//! part of the battle overlay `0898`. They are emitted by code that only
//! exists inside overlay 967, which the engine never paged in. Porting the
//! battle SM alone can therefore never produce them.
//!
//! ## Shape of the retail machine
//!
//! Overlay 967's tick (`0x801F6B70`) is a **jump table hook on the battle
//! flow-state byte**, not a linear script:
//!
//! ```text
//! ctx      = _DAT_8007BD24                  // battle context
//! if ctx[0x6B2] != 0  -> suppressed          // a box is already up
//! ctx[0x6B0] = 0
//! if ctx[0x6AE] != 0  -> already emitted     // one-shot latch
//! idx = ctx[0x06] - 0x1E                     // flow state, 91-entry table
//! if idx >= 0x5B      -> no-op
//! goto table[idx]                            // table @ 0x801F69D8
//! ```
//!
//! Only **nine** of the 91 slots are live; the other 82 point at the shared
//! no-op tail. Each live handler then switches on `ctx[0x28A]` - the same
//! byte the battle-action SM's `case 0xFF` increments
//! ([`crate::world::World::advance_battle_mode`]) - which the tutorial reads
//! as the **lesson index** (0 attacks, 1 items, 2 spirit, 3 hyper arts,
//! 4 -> done).
//!
//! So the tutorial is a *cross-product* table: `(flow state, lesson)` selects
//! a short burst of message boxes, plus a wrong-lesson rewind (`0x801F7628`)
//! when the player picks the action the current lesson is not teaching. That
//! cross-product is [`dispatch`].
//!
//! ## Text provenance - loaded from the disc, never committed
//!
//! The prompt strings are Sony bytes living inside overlay 967 itself. This
//! module therefore commits only their **addresses** ([`MessageId`] is the
//! string's VA) and reads the text out of the user's own disc at runtime via
//! [`BattleTutorialScript::from_overlay`]. No prompt text is checked in - the
//! same rule the item / spell / dialog-corpus parsers follow.

use std::collections::BTreeMap;

/// Load base of overlay 967 (`static-overlays.toml`, `battle_tutorial`).
pub const OVERLAY_967_BASE_VA: u32 = 0x801F_69D8;

/// Extraction PROT index of the battle-tutorial stage overlay.
pub const OVERLAY_967_PROT_INDEX: u32 = 967;

/// Battle-stage id that pages overlay 967 in (`ctx` byte `_DAT_8007B64A`).
/// Every other catalogued battle reads `0` (no stage overlay).
pub const TUTORIAL_STAGE_ID: u8 = 1;

/// A tutorial message, identified by the **VA of its string** inside overlay
/// 967. The engine resolves it to text through [`BattleTutorialScript`].
pub type MessageId = u32;

/// Which lesson the sparring fight is currently teaching - retail
/// `ctx[0x28A]`, shared with the battle-action SM's mode counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum TutorialLesson {
    /// Basic attacks.
    Attacks = 0,
    /// Using items.
    Items = 1,
    /// The Spirit (guard / AP charge) command.
    Spirit = 2,
    /// Hyper Arts (the `[High] [Low] [High]` drill).
    HyperArts = 3,
    /// All four lessons taught; the next tick closes the fight.
    Done = 4,
    /// Terminal - the fight has been closed out.
    Finished = 5,
}

impl TutorialLesson {
    /// Decode the raw `ctx[0x28A]` byte. Values above `5` clamp to
    /// [`TutorialLesson::Finished`], matching the retail `sltiu v0, v0, 5`
    /// clamp at `0x801F7390`.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Attacks,
            1 => Self::Items,
            2 => Self::Spirit,
            3 => Self::HyperArts,
            4 => Self::Done,
            _ => Self::Finished,
        }
    }

    /// Raw `ctx[0x28A]` value.
    pub fn raw(self) -> u8 {
        self as u8
    }

    /// The [`ActionCategory`](crate::battle_input) byte (`actor[+0x1DE]`) the
    /// player is expected to commit for this lesson - the flow-state `110`
    /// validator at `0x801F7088`.
    ///
    /// Hyper Arts is reached *through* Attack, so it expects the same `3`.
    pub fn expected_action_category(self) -> Option<u8> {
        match self {
            Self::Attacks => Some(3),   // Attack
            Self::Items => Some(1),     // Item
            Self::Spirit => Some(4),    // Spirit
            Self::HyperArts => Some(3), // Attack -> Command mode
            Self::Done | Self::Finished => None,
        }
    }

    /// The rewind message that names the lesson currently being taught, shown
    /// when the player picks an action some other lesson covers.
    pub fn wrong_lesson_message(self) -> Option<MessageId> {
        match self {
            Self::Attacks => Some(msg::WRONG_ATTACKS),
            Self::Items => Some(msg::WRONG_ITEMS),
            Self::Spirit => Some(msg::WRONG_SPIRIT),
            Self::HyperArts => Some(msg::WRONG_HYPER_ARTS),
            Self::Done | Self::Finished => None,
        }
    }
}

/// String VAs inside overlay 967. These are **addresses**, not text - the
/// text is read from the user's disc (see module docs).
pub mod msg {
    use super::MessageId;

    /// Lesson 0 turn-start: introduces basic attacks and asks for [Begin].
    pub const LESSON0_INTRO: MessageId = 0x801F_7684;
    /// Lesson 1 turn-start.
    pub const LESSON1_INTRO: MessageId = 0x801F_76B8;
    /// Lesson 2 turn-start.
    pub const LESSON2_INTRO: MessageId = 0x801F_76EC;
    /// Lesson 3 turn-start.
    pub const LESSON3_INTRO: MessageId = 0x801F_7718;

    /// First-time directional-button explainer.
    pub const HOWTO_DIRECTIONAL: MessageId = 0x801F_774C;
    /// Repeat-visit variant of the explainer.
    pub const HOWTO_HIGHLIGHT: MessageId = 0x801F_7780;

    /// Run rejected during the tutorial.
    pub const NO_RUNNING: MessageId = 0x801F_77DC;

    /// Names [Attack] as the category to pick next.
    pub const PICK_ATTACK: MessageId = 0x801F_7808;
    /// Names [Item] as the category to pick next.
    pub const PICK_ITEM: MessageId = 0x801F_7820;
    /// Names [Spirit] as the category to pick next.
    pub const PICK_SPIRIT: MessageId = 0x801F_7834;
    /// What Spirit does.
    pub const SPIRIT_EXPLAIN: MessageId = 0x801F_784C;

    /// Auto/Command mode prompt (free choice - lesson 0).
    pub const ATTACK_MODE_FREE: MessageId = 0x801F_78C0;
    /// Auto/Command mode prompt (must pick Command - lesson 3).
    pub const ATTACK_MODE_FORCED: MessageId = 0x801F_7918;
    /// What Auto vs Command mode mean.
    pub const ATTACK_MODE_EXPLAIN: MessageId = 0x801F_79C0;

    /// Wrong-lesson rewinds.
    pub const WRONG_ITEMS: MessageId = 0x801F_7964;
    /// Wrong-lesson rewind: spirit.
    pub const WRONG_SPIRIT: MessageId = 0x801F_7990;
    /// Wrong-lesson rewind: attacks.
    pub const WRONG_ATTACKS: MessageId = 0x801F_7A5C;
    /// Wrong-lesson rewind: hyper arts.
    pub const WRONG_HYPER_ARTS: MessageId = 0x801F_7A8C;

    /// Confirms the right category and asks for [Begin].
    pub const NOW_BEGIN: MessageId = 0x801F_7AC0;
    /// Asks the player to enter a command combination.
    pub const PICK_COMBINATION: MessageId = 0x801F_7AD4;
    /// Command sequences can become a hyper arts move.
    pub const COMBO_HINT: MessageId = 0x801F_7AF8;

    /// Asks the player to pick a target.
    pub const SELECT_TARGET: MessageId = 0x801F_7B30;
    /// How to move the target cursor.
    pub const TARGET_EXPLAIN: MessageId = 0x801F_7B44;

    /// Asks the player to pick an item.
    pub const SELECT_ITEM: MessageId = 0x801F_7BB4;
    /// Item window explainer.
    pub const ITEM_WINDOW_EXPLAIN: MessageId = 0x801F_7BC4;

    /// The `[High] [Low] [High]` drill instruction.
    pub const ENTER_HIGH_LOW_HIGH: MessageId = 0x801F_7C28;
    /// Drill failed.
    pub const WRONG_COMMANDS: MessageId = 0x801F_7C64;
    /// Tutorial complete.
    pub const PRACTICE_OVER: MessageId = 0x801F_7C80;
}

/// Box placement style - the `a1` argument of the emitter `FUN_801F747C`,
/// decoded from its 10-entry jump table at `0x801F6B48`.
///
/// `x` is either the fixed left margin `0x10` or centred at
/// `0xA0 - text_width / 2`; `y` is either the fixed top `0x0E` or bottom
/// anchored at `base - box_height` where `box_height = lines * 14 - 4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxStyle {
    /// Centre horizontally instead of using the `0x10` left margin.
    pub centred: bool,
    /// Bottom-anchor base (`None` = the fixed top anchor `y = 0x0E`).
    pub bottom_anchor: Option<i16>,
    /// Whether the box waits for the player to acknowledge it.
    pub waits_for_input: bool,
}

impl BoxStyle {
    /// Decode a raw style index `0..=9`. Out-of-range indices are the retail
    /// fall-through (no placement applied), reported as `None`.
    pub fn from_raw(style: u8) -> Option<Self> {
        let s = |centred, bottom_anchor, waits_for_input| {
            Some(BoxStyle {
                centred,
                bottom_anchor,
                waits_for_input,
            })
        };
        match style {
            0 => s(false, None, false),
            1 => s(true, None, false),
            2 => s(false, Some(0xCC), true),
            3 => s(true, Some(0xCC), true),
            4 => s(false, Some(0xB0), true),
            5 => s(true, Some(0xB0), true),
            6 => s(false, Some(0x9A), true),
            7 => s(true, Some(0x9A), true),
            8 => s(false, Some(0xCC), false),
            9 => s(true, Some(0xCC), false),
            _ => None,
        }
    }

    /// Resolve the top-left corner for a box of `text_width` pixels and
    /// `lines` rendered lines, exactly as `FUN_801F747C` computes it.
    pub fn position(&self, text_width: i16, lines: i16) -> (i16, i16) {
        let x = if self.centred {
            0xA0 - text_width / 2
        } else {
            0x10
        };
        let height = lines * 14 - 4;
        let y = match self.bottom_anchor {
            Some(base) => base - height,
            None => 0x0E,
        };
        (x, y)
    }
}

/// One emitted tutorial box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TutorialBox {
    /// Which string (by overlay VA) to show.
    pub message: MessageId,
    /// Raw style index, as passed to `FUN_801F747C`.
    pub style: u8,
}

impl TutorialBox {
    const fn new(message: MessageId, style: u8) -> Self {
        Self { message, style }
    }

    /// Decoded placement style.
    pub fn placement(&self) -> Option<BoxStyle> {
        BoxStyle::from_raw(self.style)
    }
}

/// What one hook dispatch produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TutorialEmission {
    /// Boxes to show, in emission order.
    pub boxes: Vec<TutorialBox>,
    /// The handler took the "wrong lesson / wrong input - try again" exit
    /// (`0x801F7628`), so the battle flow must rewind to the prompt.
    pub rewind: bool,
}

impl TutorialEmission {
    fn plain(boxes: Vec<TutorialBox>) -> Self {
        Self {
            boxes,
            rewind: false,
        }
    }

    fn rewind(message: MessageId) -> Self {
        Self {
            boxes: vec![TutorialBox::new(message, 0)],
            rewind: true,
        }
    }

    /// `true` when nothing was emitted (the 82 no-op table slots, and the
    /// lesson arms that fall through).
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && !self.rewind
    }
}

/// The nine live battle flow-state hook points (`ctx[0x06]`), in table order.
/// Every other value in `0x1E..=0x78` maps to the shared no-op tail.
pub const HOOK_STATES: [u8; 9] = [30, 40, 50, 60, 80, 90, 100, 110, 120];

/// Flow-state value the completion tail writes to `ctx[0x06]` to close the
/// sparring fight (`0x801F73A8`).
pub const FLOW_STATE_BATTLE_OVER: u8 = 0xC8;

/// Action-state value the completion tail writes to `ctx[0x07]`.
pub const ACTION_STATE_TERMINAL: u8 = 0xFF;

/// Live inputs the hook handlers read besides the flow state and lesson.
#[derive(Debug, Clone, Copy, Default)]
pub struct TutorialInputs {
    /// `ctx[0x266]` - set once the target-select explainer has been shown, so
    /// the follow-up box moves from the `0xB0` anchor to the `0xCC` one.
    pub target_explainer_seen: bool,
    /// `_DAT_801D46C8` - clears on the very first command prompt, so the
    /// lesson-0 turn-start shows the directional explainer once and the
    /// highlight explainer thereafter.
    pub command_prompt_seen: bool,
    /// `_DAT_801D46C4 == 1` - the hyper-arts drill auto-fills the command
    /// buffer for the player.
    pub autofill_drill: bool,
    /// `actor[+0x1DE]` - the action category the player committed. Read by
    /// the flow-state `110` validator.
    pub action_category: u8,
    /// `actor[+0x1DF..=+0x1E3]` - the entered command buffer, checked by the
    /// flow-state `90` hyper-arts drill.
    ///
    /// Five bytes, not four: the retail matcher's third alignment reads the
    /// word at `+0x1E0` masked with `0xFFFFFF00`, which reaches `+0x1E3`.
    pub command_buffer: [u8; 5],
}

/// Command byte for `[Low]` in the drill buffer.
pub const CMD_LOW: u8 = 0x0E;
/// Command byte for `[High]` in the drill buffer.
pub const CMD_HIGH: u8 = 0x0F;

/// The `[High] [Low] [High]` sequence the drill asks for.
pub const DRILL_SEQUENCE: [u8; 3] = [CMD_HIGH, CMD_LOW, CMD_HIGH];

/// The buffer the drill auto-fill writes (`0x801F6FB0`): `+0x1DF` and
/// `+0x1E1` take `0x0E`, `+0x1E0` and `+0x1E2` take `0x0F`.
pub const DRILL_AUTOFILL: [u8; 5] = [CMD_LOW, CMD_HIGH, CMD_LOW, CMD_HIGH, 0];

/// Does the entered command buffer satisfy the `[High] [Low] [High]` drill?
///
/// Port of the three-way match at `0x801F6FD8`. Retail tests the sequence at
/// three alignments, each a differently-masked load off `actor[+0x1DF]`:
///
/// * `lbu +0x1DF == 0x0F` and `lhu +0x1E0 == 0x0F0E`  -> offset 0
/// * `lw +0x1E0 & 0x00FFFFFF == 0x000F0E0F`           -> offset 1
/// * `lw +0x1E0 & 0xFFFFFF00 == 0x0F0E0F00`           -> offset 2
pub fn drill_satisfied(buf: &[u8; 5]) -> bool {
    (0..=2).any(|off| buf[off..off + 3] == DRILL_SEQUENCE)
}

/// Dispatch one tutorial hook.
///
/// `flow_state` is the retail `ctx[0x06]`; `lesson` is `ctx[0x28A]`. Returns
/// the boxes the retail handler would emit. A flow state outside
/// [`HOOK_STATES`] yields an empty emission - that is the 82-slot no-op tail,
/// not an error.
///
/// This is a pure function: the one-shot latch (`ctx[0x6AE]`) and the
/// suppressed-while-a-box-is-up guard (`ctx[0x6B2]`) live in
/// [`BattleTutorial`], which owns that state.
pub fn dispatch(
    flow_state: u8,
    lesson: TutorialLesson,
    inputs: &TutorialInputs,
) -> TutorialEmission {
    use TutorialLesson::*;
    let one = |m, s| TutorialEmission::plain(vec![TutorialBox::new(m, s)]);
    let two = |m0, s0, m1, s1| {
        TutorialEmission::plain(vec![TutorialBox::new(m0, s0), TutorialBox::new(m1, s1)])
    };
    // The rewind naming whichever lesson is currently being taught.
    let wrong = || match lesson.wrong_lesson_message() {
        Some(m) => TutorialEmission::rewind(m),
        None => TutorialEmission::default(),
    };

    match (flow_state, lesson) {
        // --- 30: turn start / top command menu opened (0x801F6C00) ---
        (30, Attacks) => {
            let explainer = if inputs.command_prompt_seen {
                msg::HOWTO_HIGHLIGHT
            } else {
                msg::HOWTO_DIRECTIONAL
            };
            two(msg::LESSON0_INTRO, 0, explainer, 3)
        }
        (30, Items) => one(msg::LESSON1_INTRO, 0),
        (30, Spirit) => one(msg::LESSON2_INTRO, 0),
        (30, HyperArts) => one(msg::LESSON3_INTRO, 0),

        // --- 40: [Begin] chosen, pick the action category (0x801F6CB8) ---
        (40, Attacks) => one(msg::PICK_ATTACK, 0),
        (40, Items) => one(msg::PICK_ITEM, 0),
        (40, Spirit) => two(msg::PICK_SPIRIT, 0, msg::SPIRIT_EXPLAIN, 3),
        // Lesson 3 has no prompt here - it falls through to the no-op tail.
        (40, HyperArts) => TutorialEmission::default(),

        // --- 50: Run selected - always rejected (0x801F6CAC) ---
        (50, _) => TutorialEmission::rewind(msg::NO_RUNNING),

        // --- 60: item window opened (0x801F6DCC) ---
        (60, Items) => two(msg::SELECT_ITEM, 0, msg::ITEM_WINDOW_EXPLAIN, 3),
        (60, _) => wrong(),

        // --- 80: arts command-entry screen opened (0x801F6E4C) ---
        (80, Attacks) => two(msg::PICK_COMBINATION, 0, msg::COMBO_HINT, 4),
        (80, HyperArts) => one(msg::ENTER_HIGH_LOW_HIGH, 0),
        (80, _) => wrong(),

        // --- 90: target select (0x801F6EE4) ---
        (90, Attacks) => {
            // ctx[0x266] picks the follow-up box's anchor.
            let style = if inputs.target_explainer_seen { 3 } else { 5 };
            two(msg::SELECT_TARGET, 0, msg::TARGET_EXPLAIN, style)
        }
        (90, HyperArts) => {
            // The drill either auto-fills the buffer or checks what the
            // player entered.
            let buf = if inputs.autofill_drill {
                DRILL_AUTOFILL
            } else if inputs.target_explainer_seen {
                // Not auto-filled and the explainer already ran: retail takes
                // the wrong-lesson rewind rather than re-checking.
                return TutorialEmission::rewind(msg::WRONG_HYPER_ARTS);
            } else {
                inputs.command_buffer
            };
            if drill_satisfied(&buf) {
                two(msg::SELECT_TARGET, 0, msg::TARGET_EXPLAIN, 5)
            } else {
                TutorialEmission::rewind(msg::WRONG_COMMANDS)
            }
        }
        (90, _) => wrong(),

        // --- 100: target confirm - unconditional (0x801F7060) ---
        (100, _) => two(msg::SELECT_TARGET, 0, msg::TARGET_EXPLAIN, 3),

        // --- 110: committed category validated against the lesson (0x801F7088) ---
        (110, _) => match lesson.expected_action_category() {
            Some(expected) if inputs.action_category == expected => one(msg::NOW_BEGIN, 0),
            Some(_) => wrong(),
            None => TutorialEmission::default(),
        },

        // --- 120: Auto / Command attack-mode prompt (0x801F6D30) ---
        (120, Attacks) => two(msg::ATTACK_MODE_FREE, 0, msg::ATTACK_MODE_EXPLAIN, 3),
        (120, HyperArts) => two(msg::ATTACK_MODE_FORCED, 0, msg::ATTACK_MODE_EXPLAIN, 3),
        (120, _) => wrong(),

        _ => TutorialEmission::default(),
    }
}

/// Live tutorial state - owns the retail latches so a hook fires once per
/// entry into a flow state.
#[derive(Debug, Clone, Default)]
pub struct BattleTutorial {
    /// `ctx[0x6AE]` - non-zero once this flow state has emitted.
    latch: u16,
    /// `ctx[0x6B2]` - a box is on screen; the dispatcher is suppressed.
    pub box_up: bool,
    /// `ctx[0x28A]` - the lesson counter.
    pub lesson: u8,
    /// Live inputs the handlers read.
    pub inputs: TutorialInputs,
    /// Set once the completion tail has run.
    pub finished: bool,
}

/// What a [`BattleTutorial::tick`] asks the battle host to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TutorialTick {
    /// Boxes to show this tick.
    pub emission: TutorialEmission,
    /// The completion tail ran: write these to `ctx[0x06]` / `ctx[0x07]` and
    /// close the fight.
    pub battle_over: bool,
}

impl BattleTutorial {
    /// Fresh tutorial at lesson 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current lesson.
    pub fn lesson(&self) -> TutorialLesson {
        TutorialLesson::from_raw(self.lesson)
    }

    /// Called when the battle flow state changes - clears the one-shot latch
    /// so the new state's hook can fire (retail `0x801F71E8`).
    pub fn enter_flow_state(&mut self) {
        self.latch = 0;
    }

    /// Advance the lesson counter - the battle-action SM's `case 0xFF`
    /// (`ctx[0x28A] += 1`), mirrored by
    /// [`crate::world::World::advance_battle_mode`].
    pub fn advance_lesson(&mut self) {
        self.lesson = self.lesson.saturating_add(1);
    }

    /// One dispatcher tick for the current battle flow state.
    ///
    /// Mirrors `FUN_801F6B70`: guard on the box-up latch, guard on the
    /// one-shot latch, dispatch the hook, then run the completion tail.
    pub fn tick(&mut self, flow_state: u8) -> TutorialTick {
        // ctx[0x6B2] != 0 -> suppressed entirely.
        if self.box_up {
            return TutorialTick::default();
        }

        let mut tick = TutorialTick::default();

        // ctx[0x6AE] != 0 -> already emitted for this flow state.
        if self.latch == 0 {
            tick.emission = dispatch(flow_state, self.lesson(), &self.inputs);
            // Every dispatch path bumps the latch (0x801F7190 / 0x801F71A4).
            self.latch = self.latch.wrapping_add(1);
        }

        // --- completion tail (0x801F7380) ---
        if self.lesson >= 5 {
            self.lesson = 5;
            tick.battle_over = true;
        } else if self.lesson == 4 && !self.finished {
            self.lesson = 5;
            self.finished = true;
            tick.battle_over = true;
            tick.emission
                .boxes
                .push(TutorialBox::new(msg::PRACTICE_OVER, 9));
        }

        tick
    }
}

/// The tutorial prompt strings, read out of the user's own disc copy of
/// overlay 967. Never committed - see the module docs.
#[derive(Debug, Clone, Default)]
pub struct BattleTutorialScript {
    strings: BTreeMap<MessageId, String>,
}

impl BattleTutorialScript {
    /// Every message VA this module can emit.
    pub const MESSAGE_IDS: [MessageId; 25] = [
        msg::LESSON0_INTRO,
        msg::LESSON1_INTRO,
        msg::LESSON2_INTRO,
        msg::LESSON3_INTRO,
        msg::HOWTO_DIRECTIONAL,
        msg::HOWTO_HIGHLIGHT,
        msg::NO_RUNNING,
        msg::PICK_ATTACK,
        msg::PICK_ITEM,
        msg::PICK_SPIRIT,
        msg::SPIRIT_EXPLAIN,
        msg::ATTACK_MODE_FREE,
        msg::ATTACK_MODE_FORCED,
        msg::ATTACK_MODE_EXPLAIN,
        msg::WRONG_ITEMS,
        msg::WRONG_SPIRIT,
        msg::WRONG_ATTACKS,
        msg::WRONG_HYPER_ARTS,
        msg::NOW_BEGIN,
        msg::PICK_COMBINATION,
        msg::COMBO_HINT,
        msg::SELECT_TARGET,
        msg::TARGET_EXPLAIN,
        msg::SELECT_ITEM,
        msg::ITEM_WINDOW_EXPLAIN,
    ];

    /// Parse the prompt strings out of overlay 967's as-loaded bytes.
    ///
    /// `bytes` is the blob `asset overlay extract --label battle_tutorial`
    /// produces (or the equivalent in-engine overlay read); `base_va` is its
    /// load base, normally [`OVERLAY_967_BASE_VA`].
    ///
    /// Unknown / out-of-range VAs are skipped rather than erroring, so a
    /// truncated or differently-based blob degrades to fewer messages instead
    /// of failing the battle.
    pub fn from_overlay(bytes: &[u8], base_va: u32) -> Self {
        let mut strings = BTreeMap::new();
        let mut ids: Vec<MessageId> = Self::MESSAGE_IDS.to_vec();
        ids.push(msg::ENTER_HIGH_LOW_HIGH);
        ids.push(msg::WRONG_COMMANDS);
        ids.push(msg::PRACTICE_OVER);
        for va in ids {
            let Some(off) = va.checked_sub(base_va).map(|o| o as usize) else {
                continue;
            };
            if off >= bytes.len() {
                continue;
            }
            let tail = &bytes[off..];
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            if end == 0 {
                continue;
            }
            // Retail uses '|' as the hard line break inside a prompt box.
            let text: String = tail[..end]
                .iter()
                .map(|&b| if b == b'|' { '\n' } else { b as char })
                .collect();
            strings.insert(va, text);
        }
        Self { strings }
    }

    /// Resolve a message to its text, if the disc supplied it.
    pub fn text(&self, id: MessageId) -> Option<&str> {
        self.strings.get(&id).map(String::as_str)
    }

    /// How many prompts were recovered.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// `true` when no prompt was recovered (no disc / wrong blob).
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_states_are_the_nine_live_table_slots() {
        // Every live slot is inside the 91-entry table's 0x1E..=0x78 window.
        for s in HOOK_STATES {
            assert!((0x1E..=0x78).contains(&s), "state {s} outside table window");
        }
        // And every other value in the window is a no-op.
        let inputs = TutorialInputs::default();
        for s in 0x1Eu8..=0x78 {
            if HOOK_STATES.contains(&s) {
                continue;
            }
            assert!(
                dispatch(s, TutorialLesson::Attacks, &inputs).is_empty(),
                "state {s} should be a no-op"
            );
        }
    }

    #[test]
    fn lesson_zero_turn_start_shows_intro_then_explainer() {
        let mut inputs = TutorialInputs::default();
        let e = dispatch(30, TutorialLesson::Attacks, &inputs);
        assert_eq!(e.boxes.len(), 2);
        assert_eq!(e.boxes[0].message, msg::LESSON0_INTRO);
        assert_eq!(e.boxes[1].message, msg::HOWTO_DIRECTIONAL);

        // Second visit swaps the explainer variant.
        inputs.command_prompt_seen = true;
        let e = dispatch(30, TutorialLesson::Attacks, &inputs);
        assert_eq!(e.boxes[1].message, msg::HOWTO_HIGHLIGHT);
    }

    #[test]
    fn run_is_always_rejected() {
        let inputs = TutorialInputs::default();
        for lesson in [
            TutorialLesson::Attacks,
            TutorialLesson::Items,
            TutorialLesson::Spirit,
            TutorialLesson::HyperArts,
        ] {
            let e = dispatch(50, lesson, &inputs);
            assert!(e.rewind);
            assert_eq!(e.boxes[0].message, msg::NO_RUNNING);
        }
    }

    #[test]
    fn wrong_lesson_rewinds_with_that_lessons_message() {
        let inputs = TutorialInputs::default();
        // Item window opened while teaching attacks.
        let e = dispatch(60, TutorialLesson::Attacks, &inputs);
        assert!(e.rewind);
        assert_eq!(e.boxes[0].message, msg::WRONG_ATTACKS);
        // ...and while teaching spirit.
        let e = dispatch(60, TutorialLesson::Spirit, &inputs);
        assert_eq!(e.boxes[0].message, msg::WRONG_SPIRIT);
        // The item lesson itself is the only one that proceeds.
        let e = dispatch(60, TutorialLesson::Items, &inputs);
        assert!(!e.rewind);
        assert_eq!(e.boxes[0].message, msg::SELECT_ITEM);
    }

    #[test]
    fn category_validator_matches_the_lesson() {
        // Lesson 0 expects Attack (3).
        let mut inputs = TutorialInputs {
            action_category: 3,
            ..Default::default()
        };
        assert_eq!(
            dispatch(110, TutorialLesson::Attacks, &inputs).boxes[0].message,
            msg::NOW_BEGIN
        );
        // Committing Item during the attack lesson rewinds.
        inputs.action_category = 1;
        let e = dispatch(110, TutorialLesson::Attacks, &inputs);
        assert!(e.rewind);
        assert_eq!(e.boxes[0].message, msg::WRONG_ATTACKS);
        // Spirit lesson expects 4.
        inputs.action_category = 4;
        assert_eq!(
            dispatch(110, TutorialLesson::Spirit, &inputs).boxes[0].message,
            msg::NOW_BEGIN
        );
        // Hyper arts is reached through Attack, so it expects 3 too.
        inputs.action_category = 3;
        assert_eq!(
            dispatch(110, TutorialLesson::HyperArts, &inputs).boxes[0].message,
            msg::NOW_BEGIN
        );
    }

    #[test]
    fn drill_accepts_high_low_high_at_all_three_alignments() {
        assert!(drill_satisfied(&[CMD_HIGH, CMD_LOW, CMD_HIGH, 0, 0]));
        assert!(drill_satisfied(&[0, CMD_HIGH, CMD_LOW, CMD_HIGH, 0]));
        assert!(drill_satisfied(&[0, 0, CMD_HIGH, CMD_LOW, CMD_HIGH]));
        assert!(!drill_satisfied(&[CMD_LOW; 5]));
        // The auto-fill buffer satisfies it at offset 1.
        assert!(drill_satisfied(&DRILL_AUTOFILL));
    }

    #[test]
    fn drill_failure_rewinds_with_wrong_commands() {
        let inputs = TutorialInputs {
            command_buffer: [CMD_LOW; 5],
            ..Default::default()
        };
        let e = dispatch(90, TutorialLesson::HyperArts, &inputs);
        assert!(e.rewind);
        assert_eq!(e.boxes[0].message, msg::WRONG_COMMANDS);
    }

    #[test]
    fn box_styles_decode_to_the_retail_placement_table() {
        // Style 0: left margin, top anchor, no wait.
        let s0 = BoxStyle::from_raw(0).unwrap();
        assert_eq!(s0.position(100, 2), (0x10, 0x0E));
        assert!(!s0.waits_for_input);
        // Style 3: centred, bottom anchored at 0xCC, waits.
        let s3 = BoxStyle::from_raw(3).unwrap();
        assert_eq!(s3.position(100, 2), (0xA0 - 50, 0xCC - (2 * 14 - 4)));
        assert!(s3.waits_for_input);
        // Style 9: centred, 0xCC anchor, does NOT wait.
        assert!(!BoxStyle::from_raw(9).unwrap().waits_for_input);
        // Out of range.
        assert!(BoxStyle::from_raw(10).is_none());
    }

    #[test]
    fn latch_makes_a_hook_fire_once_per_flow_state() {
        let mut t = BattleTutorial::new();
        t.enter_flow_state();
        let first = t.tick(30);
        assert_eq!(first.emission.boxes.len(), 2);
        // Ticking again in the same flow state emits nothing.
        assert!(t.tick(30).emission.is_empty());
        // Re-entering the state re-arms it.
        t.enter_flow_state();
        assert_eq!(t.tick(30).emission.boxes.len(), 2);
    }

    #[test]
    fn box_up_suppresses_the_whole_dispatcher() {
        let mut t = BattleTutorial::new();
        t.box_up = true;
        assert!(t.tick(30).emission.is_empty());
    }

    #[test]
    fn completion_tail_closes_the_fight_after_four_lessons() {
        let mut t = BattleTutorial::new();
        for _ in 0..4 {
            t.advance_lesson();
        }
        assert_eq!(t.lesson(), TutorialLesson::Done);
        t.enter_flow_state();
        let tick = t.tick(30);
        assert!(tick.battle_over);
        assert!(
            tick.emission
                .boxes
                .iter()
                .any(|b| b.message == msg::PRACTICE_OVER && b.style == 9)
        );
        assert_eq!(t.lesson(), TutorialLesson::Finished);
        // And it does not fire the completion box twice.
        t.enter_flow_state();
        let again = t.tick(30);
        assert!(again.battle_over);
        assert!(
            !again
                .emission
                .boxes
                .iter()
                .any(|b| b.message == msg::PRACTICE_OVER)
        );
    }

    #[test]
    fn lesson_raw_roundtrips_and_clamps() {
        for raw in 0u8..=5 {
            assert_eq!(TutorialLesson::from_raw(raw).raw(), raw);
        }
        assert_eq!(TutorialLesson::from_raw(200), TutorialLesson::Finished);
    }

    #[test]
    fn script_parses_nul_terminated_strings_and_maps_pipes() {
        // Synthetic blob: no Sony bytes. One message at LESSON0_INTRO.
        let base = OVERLAY_967_BASE_VA;
        let off = (msg::LESSON0_INTRO - base) as usize;
        let mut bytes = vec![0u8; off + 32];
        bytes[off..off + 7].copy_from_slice(b"ab|cd\0\0");
        let script = BattleTutorialScript::from_overlay(&bytes, base);
        assert_eq!(script.text(msg::LESSON0_INTRO), Some("ab\ncd"));
        assert_eq!(script.text(msg::NOW_BEGIN), None);
        assert_eq!(script.len(), 1);
    }

    #[test]
    fn script_from_empty_blob_is_empty_not_a_panic() {
        let script = BattleTutorialScript::from_overlay(&[], OVERLAY_967_BASE_VA);
        assert!(script.is_empty());
    }
}
