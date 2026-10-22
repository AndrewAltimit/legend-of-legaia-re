//! The command surfaces' UI blips: which cue a frame of the battle command
//! flow fires, decided once from the surface's state before and after the
//! frame's input, for both hosts.
//!
//! Retail has two producers here, and the ids they write differ by one:
//!
//! - The command-flow SM `FUN_801D0748` calls the cue dispatcher
//!   `FUN_8004FCC8` with `0x21` (a commit), `0x22` (a cursor step) and `0x23`
//!   (a cancel or a refused arm). Below `0x40` the dispatcher stores `id - 1`
//!   in the ring (`addiu a1,s0,-0x1` / `sh a1,0x0(v0)` at
//!   `0x8004FD9C..0x8004FE28`), so the drainer keys descriptors `0x20`,
//!   `0x21` and `0x22`. Sites: the round prompt's `Begin` / `Run` and every
//!   ring arm through the shared tail `0x801D20D4`; the `Attack` arm at
//!   `0x801D15D0`; the attack-mode prompt at `0x801D175C` / `0x801D17CC`; the
//!   target cursor's step at `0x801D263C`, its confirm at `0x801D22B0` and its
//!   cancel at `0x801D2320`; the commit-confirm screen's `Begin` at
//!   `0x801D31A4` and `Reselect` at `0x801D3054`; the ring's cancel and its
//!   refused arms through the shared tail `0x801D2968`; the arts entry's
//!   refused directions at `0x801D1EA0..0x801D1F78` and its cancel at
//!   `0x801D2104`.
//! - The item and magic windows are pause-menu list windows paged by the
//!   SCUS list kernel `FUN_80032A44`, which writes the ring itself: `0x21`
//!   a cursor step, `0x20` a confirm, `0x23` a disabled row, `0x37` a cancel
//!   (`docs/subsystems/field-menu.md`). Their target sub-cursors (`0x5B..0x5E`,
//!   `0x64..0x67`) are back in `FUN_801D0748` and use the dispatcher.
//!
//! So [`BattleUiCue`] names the descriptor the drainer keys, which is what
//! the ring carries.
//!
//! REF: FUN_801D0748, FUN_8004FCC8, FUN_80032A44

use crate::world::World;
use legaia_engine_battle::arts_command_input::ArtsInputPhase;
use legaia_engine_battle::battle_magic::SpellPhase;
use legaia_engine_battle_menus::battle_input::CommandPhase;
use legaia_engine_menus::inventory_use::{InventoryUseEvent, InventoryUseState};
use legaia_engine_system::sound_state::SfxRingOp;

/// One command-surface blip, by the descriptor the drainer keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleUiCue {
    /// A commit: `FUN_8004FCC8(0x21)`, or the list kernel's `0x20`.
    Confirm,
    /// A cursor step: `FUN_8004FCC8(0x22)`, or the list kernel's `0x21`.
    Cursor,
    /// A cancel or a refused arm in the command flow: `FUN_8004FCC8(0x23)`.
    Back,
    /// A confirm on a disabled list row: the list kernel's `0x23`.
    Buzz,
    /// A cancel out of a list window: the list kernel's `0x37`.
    ListCancel,
}

impl BattleUiCue {
    /// The ring id - the static descriptor the drainer keys.
    pub const fn ring_id(self) -> i16 {
        match self {
            Self::Confirm => 0x20,
            Self::Cursor => 0x21,
            Self::Back => 0x22,
            Self::Buzz => 0x23,
            Self::ListCancel => 0x37,
        }
    }
}

/// The blip a command-session frame fires, from its phase before and after
/// the input. A press that resolves the surface into the next one commits;
/// a cancel steps back; a target-cursor move steps; a frame that changes
/// nothing is silent.
pub fn command_phase_cue(before: &CommandPhase, after: &CommandPhase) -> Option<BattleUiCue> {
    use CommandPhase as P;
    match (before, after) {
        (_, P::StepBack | P::Reselect) => Some(BattleUiCue::Back),
        (P::AttackMode { .. }, P::Menu { .. }) => Some(BattleUiCue::Back),
        (P::Targeting { .. }, P::Menu { .. }) => Some(BattleUiCue::Back),
        (P::Targeting { picker: a, .. }, P::Targeting { picker: b, .. }) => {
            (a.state() != b.state()).then_some(BattleUiCue::Cursor)
        }
        (P::Targeting { .. }, P::Confirmed { .. }) => Some(BattleUiCue::Confirm),
        (_, P::Aborted) => None,
        (P::RoundPrompt { .. }, P::RoundPrompt { .. })
        | (P::Menu { .. }, P::Menu { .. })
        | (P::AttackMode { .. }, P::AttackMode { .. })
        | (P::CommitConfirm { .. }, P::CommitConfirm { .. }) => None,
        (
            P::RoundPrompt { .. } | P::Menu { .. } | P::AttackMode { .. } | P::CommitConfirm { .. },
            _,
        ) => Some(BattleUiCue::Confirm),
        _ => None,
    }
}

/// The blip a magic-window frame fires. The spell list is a list-kernel
/// window; its target cursor is the command flow's.
pub fn spell_phase_cue(
    before: &SpellPhase,
    after: &SpellPhase,
    cross: bool,
) -> Option<BattleUiCue> {
    use SpellPhase as P;
    match (before, after) {
        (P::Select { cursor: a }, P::Select { cursor: b }) => {
            if a != b {
                Some(BattleUiCue::Cursor)
            } else if cross {
                // A confirm the list refused: an unaffordable spell.
                Some(BattleUiCue::Buzz)
            } else {
                None
            }
        }
        (P::Select { .. }, P::Targeting { .. } | P::Confirmed { .. }) => Some(BattleUiCue::Confirm),
        (P::Select { .. }, P::Aborted) => Some(BattleUiCue::ListCancel),
        (P::Targeting { picker: a, .. }, P::Targeting { picker: b, .. }) => {
            (a.state() != b.state()).then_some(BattleUiCue::Cursor)
        }
        (P::Targeting { .. }, P::Confirmed { .. }) => Some(BattleUiCue::Confirm),
        (P::Targeting { .. }, P::Select { .. }) => Some(BattleUiCue::Back),
        _ => None,
    }
}

/// The blip an item-window frame fires, from the session's own events and
/// the state it was in before the input: the first event decides.
pub fn item_events_cue(
    before: &InventoryUseState,
    events: &[InventoryUseEvent],
) -> Option<BattleUiCue> {
    let browsing = matches!(before, InventoryUseState::Browsing { .. });
    events.first().map(|ev| match ev {
        InventoryUseEvent::CursorMoved => BattleUiCue::Cursor,
        InventoryUseEvent::InvalidConfirm => BattleUiCue::Buzz,
        InventoryUseEvent::EnteredTargetSelect | InventoryUseEvent::Used { .. } => {
            BattleUiCue::Confirm
        }
        InventoryUseEvent::Cancelled if browsing => BattleUiCue::ListCancel,
        InventoryUseEvent::Cancelled => BattleUiCue::Back,
    })
}

/// The blip an arts-entry frame fires. `confirm` is whether the frame's
/// confirm button was down: leaving the entry on the confirm keys the commit
/// cue, while the press that exhausts the gauge advances silently (the
/// `bne s0,zero,0x801D3290` at `0x801D20CC` skips the tail's cue).
pub fn arts_phase_cue(
    before: &ArtsInputPhase,
    after: &ArtsInputPhase,
    confirm: bool,
) -> Option<BattleUiCue> {
    use ArtsInputPhase as P;
    match (before, after) {
        (P::Entering, P::Aborted) => Some(BattleUiCue::Back),
        (P::Entering, P::Review | P::Targeting { .. } | P::Confirmed { .. }) => {
            confirm.then_some(BattleUiCue::Confirm)
        }
        (P::Review | P::Targeting { .. }, P::Entering) => Some(BattleUiCue::Back),
        (P::Targeting { picker: a }, P::Targeting { picker: b }) => {
            (a.state() != b.state()).then_some(BattleUiCue::Cursor)
        }
        (P::Review | P::Targeting { .. }, P::Confirmed { .. }) => Some(BattleUiCue::Confirm),
        _ => None,
    }
}

impl World {
    /// Put one command-surface blip on the SFX ring both hosts drain.
    pub(in crate::world) fn push_battle_ui_cue(&mut self, cue: BattleUiCue) {
        self.audio.sfx_ring_ops.push(SfxRingOp::Push(cue.ring_id()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_battle_menus::battle_input::BattleCommand;

    #[test]
    fn the_dispatcher_ids_land_one_below_on_the_ring() {
        // `FUN_8004FCC8(0x21 / 0x22 / 0x23)` stores `id - 1`.
        assert_eq!(BattleUiCue::Confirm.ring_id(), 0x21 - 1);
        assert_eq!(BattleUiCue::Cursor.ring_id(), 0x22 - 1);
        assert_eq!(BattleUiCue::Back.ring_id(), 0x23 - 1);
    }

    #[test]
    fn ring_commits_confirm_and_its_cancel_steps_back() {
        let menu = CommandPhase::Menu { cursor: 1 };
        for after in [
            CommandPhase::OpenItemMenu,
            CommandPhase::OpenSpellMenu,
            CommandPhase::SpiritGuard,
            CommandPhase::AttackMode { cursor: 0 },
        ] {
            assert_eq!(
                command_phase_cue(&menu, &after),
                Some(BattleUiCue::Confirm),
                "{after:?}"
            );
        }
        assert_eq!(
            command_phase_cue(&menu, &CommandPhase::StepBack),
            Some(BattleUiCue::Back)
        );
        assert_eq!(command_phase_cue(&menu, &menu.clone()), None);
        assert_eq!(
            command_phase_cue(
                &CommandPhase::RoundPrompt { cursor: 0 },
                &CommandPhase::Menu { cursor: 1 }
            ),
            Some(BattleUiCue::Confirm)
        );
        assert_eq!(
            command_phase_cue(
                &CommandPhase::AttackMode { cursor: 0 },
                &CommandPhase::Menu {
                    cursor: BattleCommand::MENU
                        .iter()
                        .position(|c| *c == BattleCommand::Attack)
                        .unwrap() as u8
                }
            ),
            Some(BattleUiCue::Back)
        );
        assert_eq!(
            command_phase_cue(
                &CommandPhase::CommitConfirm { cursor: 0 },
                &CommandPhase::BeginRound
            ),
            Some(BattleUiCue::Confirm)
        );
    }

    #[test]
    fn list_windows_use_the_list_kernel_cues() {
        let b = InventoryUseState::Browsing { cursor: 0 };
        let t = InventoryUseState::TargetSelect {
            item_cursor: 0,
            cursor: 0,
        };
        assert_eq!(
            item_events_cue(&b, &[InventoryUseEvent::CursorMoved]),
            Some(BattleUiCue::Cursor)
        );
        assert_eq!(
            item_events_cue(&b, &[InventoryUseEvent::Cancelled]),
            Some(BattleUiCue::ListCancel)
        );
        assert_eq!(
            item_events_cue(&t, &[InventoryUseEvent::Cancelled]),
            Some(BattleUiCue::Back)
        );
        assert_eq!(
            item_events_cue(&b, &[InventoryUseEvent::InvalidConfirm]),
            Some(BattleUiCue::Buzz)
        );
        assert_eq!(item_events_cue(&b, &[]), None);
        let s = SpellPhase::Select { cursor: 0 };
        assert_eq!(
            spell_phase_cue(&s, &SpellPhase::Select { cursor: 1 }, false),
            Some(BattleUiCue::Cursor)
        );
        assert_eq!(
            spell_phase_cue(&s, &SpellPhase::Aborted, false),
            Some(BattleUiCue::ListCancel)
        );
        assert_eq!(spell_phase_cue(&s, &s.clone(), false), None);
    }
}
