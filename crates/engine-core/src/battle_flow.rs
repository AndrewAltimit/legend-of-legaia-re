//! The battle **command-flow** state byte - retail `ctx[+0x06]`.
//!
//! PORT: FUN_801D0748
//! REF: FUN_801E295C, FUN_801D9BBC, FUN_801F747C
//!
//! Retail runs two independent battle state machines over the same context
//! struct (`ctx = _DAT_8007BD24`):
//!
//! * `ctx[+0x07]` - the per-actor **action** SM `FUN_801E295C`, ported as
//!   [`legaia_engine_vm::battle_action`]. It executes an already-chosen action
//!   (swing, cast, flee).
//! * `ctx[+0x06]` - the **command flow** SM `FUN_801D0748`, ported here. It is
//!   the menu/UI half: whose turn it is, which window is open, what the player
//!   has picked so far.
//!
//! The two are easy to confuse because both are byte cursors into a `jr` table
//! and both use values in `0x00..=0xFF`. They are not the same space:
//! `ctx[7] == 0x64` is `RunBegin`, while `ctx[6] == 0x64` is *target confirm*.
//!
//! ## The command-flow state space
//!
//! Below `0x1E` the flow is battle entry and turn setup (`0x00` init,
//! `0x0A`/`0x0B` the intro timer at `ctx[+0x6D6]`, `0x14` turn start, which
//! opens the top menu and falls into `0x1E`). From `0x1E` up it is the player's
//! command selection, and the states are **regular decimal multiples of ten**:
//!
//! | `ctx[+0x06]` | State | What is on screen |
//! |---|---|---|
//! | `0x1E` = 30 | [`BattleFlowState::TurnPrompt`] | Turn start: the `[Begin]` / `[Escape]` prompt. |
//! | `0x28` = 40 | [`BattleFlowState::CategoryMenu`] | The action-category menu (Attack / Arts / Magic / Item / Spirit). |
//! | `0x32` = 50 | [`BattleFlowState::EscapePrompt`] | `[Escape]` chosen - the flee confirm. |
//! | `0x3C` = 60 | [`BattleFlowState::ItemWindow`] | The item window. |
//! | `0x46` = 70 | [`BattleFlowState::MagicWindow`] | The magic window. |
//! | `0x50` = 80 | [`BattleFlowState::ArtsCommandEntry`] | The arts command-entry screen. |
//! | `0x5A` = 90 | [`BattleFlowState::TargetSelect`] | The target cursor. |
//! | `0x64` = 100 | [`BattleFlowState::TargetConfirm`] | Target confirmed. |
//! | `0x6E` = 110 | [`BattleFlowState::CommitBegin`] | Every member has committed - begin the round. |
//! | `0x78` = 120 | [`BattleFlowState::AttackModePrompt`] | The Auto / Command attack-mode prompt. |
//!
//! Above the selection band sit the resolution states (`0x5B..=0x67` per-window
//! target sub-cursors, `0xFE` "round armed, run the action SM", `0xFF` idle).
//!
//! ## Why this byte matters beyond the menus
//!
//! It is the key the **sparring-tutorial** hook table indexes
//! ([`crate::battle_tutorial`]). Overlay 967's tick subtracts `0x1E` from it
//! and jumps a 91-entry table; the nine live slots are exactly the nine
//! selection states above **minus** [`BattleFlowState::MagicWindow`] - the
//! tutorial teaches attacks, items, spirit and hyper arts, and never magic.
//! That the live hook set is `{30,40,50,60,80,90,100,110,120}` and the flow
//! band is `{30,40,50,60,70,80,90,100,110,120}` is the cross-check that pins
//! the mapping.

use crate::battle_input::CommandPhase;

/// The battle command-flow cursor, retail `ctx[+0x06]`.
///
/// Only the states the engine's command flow can be in are modelled; retail's
/// entry / resolution states outside the selection band collapse into
/// [`BattleFlowState::Idle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum BattleFlowState {
    /// No command selection in flight - the action SM owns the frame.
    /// Stands in for retail's entry (`0x00..=0x14`) and resolution
    /// (`0xFE`/`0xFF`) states, none of which carry a tutorial hook.
    #[default]
    Idle = 0,
    /// Turn start - the `[Begin]` / `[Escape]` prompt (retail `0x1E`).
    TurnPrompt = 30,
    /// The action-category menu (retail `0x28`).
    CategoryMenu = 40,
    /// `[Escape]` chosen - the flee confirm (retail `0x32`).
    EscapePrompt = 50,
    /// The item window (retail `0x3C`).
    ItemWindow = 60,
    /// The magic window (retail `0x46`). No tutorial hook.
    MagicWindow = 70,
    /// The arts command-entry screen (retail `0x50`).
    ArtsCommandEntry = 80,
    /// The target cursor (retail `0x5A`).
    TargetSelect = 90,
    /// Target confirmed (retail `0x64`).
    TargetConfirm = 100,
    /// Every party member has committed - begin the round (retail `0x6E`).
    CommitBegin = 110,
    /// The Auto / Command attack-mode prompt (retail `0x78`).
    AttackModePrompt = 120,
}

impl BattleFlowState {
    /// The raw `ctx[+0x06]` byte.
    pub fn raw(self) -> u8 {
        self as u8
    }

    /// Decode a raw `ctx[+0x06]` byte. Values outside the modelled selection
    /// band (retail's entry / resolution states) decode to
    /// [`BattleFlowState::Idle`].
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            30 => Self::TurnPrompt,
            40 => Self::CategoryMenu,
            50 => Self::EscapePrompt,
            60 => Self::ItemWindow,
            70 => Self::MagicWindow,
            80 => Self::ArtsCommandEntry,
            90 => Self::TargetSelect,
            100 => Self::TargetConfirm,
            110 => Self::CommitBegin,
            120 => Self::AttackModePrompt,
            _ => Self::Idle,
        }
    }
}

/// Which host-owned battle submenu is open, if any. The engine splits the
/// windows retail drives from inside `FUN_801D0748` into separate sessions
/// ([`crate::inventory_use`] / [`crate::battle_magic`] /
/// [`crate::battle_arts`]), so the flow state has to be recomposed from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BattleMenuKind {
    /// No submenu open.
    #[default]
    None,
    /// The inventory submenu - retail's item window.
    Item,
    /// The spell submenu - retail's magic window.
    Magic,
    /// The saved-chain submenu - retail's arts command-entry screen.
    Arts,
}

/// Map the engine's live command-selection state onto retail `ctx[+0x06]`.
///
/// `menu` wins over `phase`: an open submenu is exactly the window state retail
/// would be sitting in, and the command session that spawned it has already
/// resolved. When neither is live the flow is [`BattleFlowState::Idle`].
///
/// The engine has no separate `[Begin]` screen, so
/// [`BattleFlowState::TurnPrompt`] is not produced here - the World raises it
/// for the frame a command session is opened (see
/// `World::open_battle_command`), which is the same instant retail enters
/// `0x1E`.
pub fn flow_state_for(phase: Option<&CommandPhase>, menu: BattleMenuKind) -> BattleFlowState {
    match menu {
        BattleMenuKind::Item => return BattleFlowState::ItemWindow,
        BattleMenuKind::Magic => return BattleFlowState::MagicWindow,
        BattleMenuKind::Arts => return BattleFlowState::ArtsCommandEntry,
        BattleMenuKind::None => {}
    }
    match phase {
        Some(CommandPhase::Menu { .. }) => BattleFlowState::CategoryMenu,
        Some(CommandPhase::Targeting { .. }) => BattleFlowState::TargetSelect,
        // A confirmed target commits the action: retail's target cursor (0x5A)
        // goes straight to the commit state (0x6E) once the last member has
        // picked. Its 0x64 "target confirm" is the *item window's* own target
        // step, which the engine runs inside `crate::inventory_use` - so
        // nothing here produces `TargetConfirm` yet.
        Some(CommandPhase::Confirmed { .. }) => BattleFlowState::CommitBegin,
        Some(CommandPhase::OpenItemMenu) => BattleFlowState::ItemWindow,
        Some(CommandPhase::OpenSpellMenu) => BattleFlowState::MagicWindow,
        Some(CommandPhase::OpenArtsMenu) => BattleFlowState::ArtsCommandEntry,
        Some(CommandPhase::RunAway) => BattleFlowState::EscapePrompt,
        Some(CommandPhase::SpiritGuard) | Some(CommandPhase::Aborted) => {
            BattleFlowState::CommitBegin
        }
        None => BattleFlowState::Idle,
    }
}

/// A tutorial box the World has resolved to text and queued for display.
///
/// The queue is the engine's re-host of retail's single on-screen message box
/// plus its `ctx[+0x6B2]` busy flag: while any box is queued the battle loop is
/// parked, exactly as `FUN_801D0748` returns early when `FUN_801D9BBC` reports
/// a box up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTutorialBox {
    /// The prompt text, disc-sourced. `'|'` hard breaks are already newlines.
    pub text: String,
    /// Raw style index `0..=9` as passed to the retail emitter `FUN_801F747C`.
    pub style: u8,
    /// Styles `2..=7` wait for the player to acknowledge; `0`, `1`, `8`, `9`
    /// dismiss on their own.
    pub waits_for_input: bool,
    /// Frames left before a non-waiting box dismisses itself. Unused when
    /// [`Self::waits_for_input`].
    pub frames_remaining: u16,
}

impl ActiveTutorialBox {
    /// Number of rendered lines - the `'|'` hard breaks the overlay strings
    /// carry, already folded to newlines.
    pub fn lines(&self) -> i16 {
        (self.text.lines().count().max(1)) as i16
    }

    /// Top-left corner for this box, given the width its text measures in the
    /// host's font. Engine-core has no glyph metrics, so the host supplies the
    /// measured width and this applies the retail placement arithmetic
    /// ([`crate::battle_tutorial::BoxStyle::position`]).
    pub fn position(&self, text_width: i16) -> Option<(i16, i16)> {
        crate::battle_tutorial::BoxStyle::from_raw(self.style)
            .map(|s| s.position(text_width, self.lines()))
    }
}

/// How long a non-waiting tutorial box stays up, in frames.
///
/// Retail's non-waiting styles are dismissed by the emitting handler's own
/// sequencing rather than a timer; the engine's box queue needs a duration, so
/// it uses the dialog layer's standard auto-advance dwell.
pub const TUTORIAL_BOX_AUTO_FRAMES: u16 = 150;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_tutorial::HOOK_STATES;

    #[test]
    fn selection_band_is_regular_tens() {
        let band = [
            BattleFlowState::TurnPrompt,
            BattleFlowState::CategoryMenu,
            BattleFlowState::EscapePrompt,
            BattleFlowState::ItemWindow,
            BattleFlowState::MagicWindow,
            BattleFlowState::ArtsCommandEntry,
            BattleFlowState::TargetSelect,
            BattleFlowState::TargetConfirm,
            BattleFlowState::CommitBegin,
            BattleFlowState::AttackModePrompt,
        ];
        for (i, s) in band.iter().enumerate() {
            assert_eq!(s.raw(), 30 + 10 * i as u8);
            assert_eq!(BattleFlowState::from_raw(s.raw()), *s);
        }
    }

    /// The tutorial's nine live hook slots are the selection band minus the
    /// magic window - the cross-check that pins `ctx[+0x06]` as the table key.
    #[test]
    fn tutorial_hooks_are_the_selection_band_without_magic() {
        let mut expected: Vec<u8> = (0..10).map(|i| 30 + 10 * i).collect();
        expected.retain(|&s| s != BattleFlowState::MagicWindow.raw());
        assert_eq!(expected, HOOK_STATES.to_vec());
    }

    #[test]
    fn entry_and_resolution_states_are_idle() {
        for raw in [0u8, 0x0A, 0x0B, 0x14, 0x5B, 0x66, 0xFE, 0xFF] {
            assert_eq!(BattleFlowState::from_raw(raw), BattleFlowState::Idle);
        }
    }

    #[test]
    fn an_open_submenu_wins_over_the_command_phase() {
        let phase = CommandPhase::Menu { cursor: 0 };
        assert_eq!(
            flow_state_for(Some(&phase), BattleMenuKind::Item),
            BattleFlowState::ItemWindow
        );
        assert_eq!(
            flow_state_for(Some(&phase), BattleMenuKind::None),
            BattleFlowState::CategoryMenu
        );
        assert_eq!(
            flow_state_for(None, BattleMenuKind::None),
            BattleFlowState::Idle
        );
    }
}
