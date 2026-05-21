//! Player-driven battle command input for the live gameplay loop.
//!
//! The live battle loop ([`crate::world::World::live_battle_tick`]) can run a
//! battle two ways. By default it auto-resolves: every party turn commits a
//! physical Attack with no player choice. When
//! [`crate::world::World::battle_player_driven`] is set, each party turn pauses
//! the action state machine and runs a [`BattleCommandSession`] that reads the
//! pad: the player picks a command from the battle command menu, then a target,
//! before the strike commits.
//!
//! **Attack**, **Magic** and **Item** are wired into the live loop. Attack
//! opens a target cursor and commits a physical strike. Magic and Item resolve
//! to [`Resolution::OpenSpellMenu`] / [`Resolution::OpenItemMenu`] hand-offs:
//! the command session can't run those pickers itself (they need the caster's
//! learned spells / live MP / inventory + party stats), so the live loop opens
//! a host-owned [`crate::battle_magic::BattleSpellSession`] /
//! [`crate::inventory_use::InventoryUseSession`] instead. Arts still appears in
//! the menu but is not selectable yet (it hangs off [`crate::battle_session`]).
//! Target selection reuses [`crate::target_picker`] so the cursor behaviour
//! matches the rest of the battle UI.
//!
//! The session is a small state machine - [`CommandPhase`] - driven one frame
//! at a time by [`BattleCommandSession::input`] with an edge-triggered
//! [`BattleCommandInput`] (the host derives the edges from
//! [`crate::input::InputState`]). When [`BattleCommandSession::resolved`]
//! returns a value the live loop arms the action SM with the chosen target.

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};

/// A top-level battle command, as listed in the battle command menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleCommand {
    /// Physical attack - opens a target cursor and commits a strike.
    Attack,
    /// Tactical Arts. Listed but not selectable yet (see [`crate::tactical_arts`]).
    Arts,
    /// Magic spell - hands off to the host battle spell submenu (see
    /// [`crate::battle_magic`]).
    Magic,
    /// Use an item - hands off to the host inventory submenu (see
    /// [`crate::inventory_use`]).
    Item,
}

impl BattleCommand {
    /// Menu entries in display order.
    pub const MENU: [BattleCommand; 4] = [
        BattleCommand::Attack,
        BattleCommand::Arts,
        BattleCommand::Magic,
        BattleCommand::Item,
    ];

    /// `true` when the command can actually be selected in the live loop.
    /// Attack (physical strike), Magic (spell submenu) and Item (inventory
    /// submenu) are wired; Arts is still pending its submenu.
    pub fn enabled(self) -> bool {
        matches!(
            self,
            BattleCommand::Attack | BattleCommand::Magic | BattleCommand::Item
        )
    }

    /// Short label for the HUD / command menu.
    pub fn label(self) -> &'static str {
        match self {
            BattleCommand::Attack => "Attack",
            BattleCommand::Arts => "Arts",
            BattleCommand::Magic => "Magic",
            BattleCommand::Item => "Item",
        }
    }

    /// The target the command applies to. v0.1 only resolves Attack
    /// (single enemy); the rest carry their natural kind for when they land.
    pub fn target_kind(self) -> TargetKind {
        match self {
            BattleCommand::Attack | BattleCommand::Arts => TargetKind::SingleEnemy,
            BattleCommand::Magic => TargetKind::SingleEnemy,
            BattleCommand::Item => TargetKind::SingleAllyOrSelf,
        }
    }
}

/// Per-frame, edge-triggered pad bundle for the command session. The host
/// fills this from [`crate::input::InputState::just_pressed`] so navigation is
/// one step per press (battle menus don't auto-repeat in v0.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleCommandInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Confirm (Cross).
    pub cross: bool,
    /// Cancel / back (Circle).
    pub circle: bool,
}

/// Sub-phase of one party member's command selection.
#[derive(Debug, Clone)]
pub enum CommandPhase {
    /// Choosing a top-level command. `cursor` indexes [`BattleCommand::MENU`].
    Menu { cursor: u8 },
    /// A command is chosen; picking its target.
    Targeting {
        command: BattleCommand,
        picker: TargetPickerSession,
    },
    /// Resolved: the live loop should arm `command` against `target_slot`
    /// (a monster-row index for enemy targets, party-row otherwise).
    Confirmed {
        command: BattleCommand,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// The player picked Magic. Like Item, the command session can't run the
    /// spell picker itself (it needs the caster's learned spells + live MP), so
    /// it hands off: the live loop opens a
    /// [`crate::battle_magic::BattleSpellSession`], casts the chosen spell, then
    /// cycles the turn.
    OpenSpellMenu,
    /// The player picked Item. The command session can't run the inventory
    /// picker itself (it needs the live inventory + party stats), so it hands
    /// off: the live loop opens an [`crate::inventory_use::InventoryUseSession`]
    /// and applies the chosen item, then cycles the turn.
    OpenItemMenu,
    /// No valid action was possible (e.g. nothing left to target). The live
    /// loop should fall back to a default strike so it never deadlocks.
    Aborted,
}

/// One party member's command-selection session, driven a frame at a time.
#[derive(Debug, Clone)]
pub struct BattleCommandSession {
    /// Actor-table index of the acting party member.
    pub actor: u8,
    /// Party-row index (0..=2) of the acting member - the target picker uses
    /// it to skip-self on ally-targeting commands.
    pub party_slot: u8,
    pub phase: CommandPhase,
}

impl BattleCommandSession {
    /// Open the menu for `actor` (party-row index `party_slot`). The cursor
    /// starts on the first enabled command.
    pub fn new(actor: u8, party_slot: u8) -> Self {
        let cursor = BattleCommand::MENU
            .iter()
            .position(|c| c.enabled())
            .unwrap_or(0) as u8;
        Self {
            actor,
            party_slot,
            phase: CommandPhase::Menu { cursor },
        }
    }

    /// The command currently under the menu cursor, or `None` once the
    /// session has left the menu.
    pub fn menu_command(&self) -> Option<BattleCommand> {
        match self.phase {
            CommandPhase::Menu { cursor } => BattleCommand::MENU.get(cursor as usize).copied(),
            _ => None,
        }
    }

    /// The active target picker, while one is open.
    pub fn picker(&self) -> Option<&TargetPickerSession> {
        match &self.phase {
            CommandPhase::Targeting { picker, .. } => Some(picker),
            _ => None,
        }
    }

    /// `(command, target_row, slot)` once the player has confirmed, or the
    /// chosen command on an abort (no valid target). `None` while still
    /// selecting.
    pub fn resolved(&self) -> Option<Resolution> {
        match &self.phase {
            CommandPhase::Confirmed {
                command,
                target_row,
                target_slot,
            } => Some(Resolution::Confirmed {
                command: *command,
                target_row: *target_row,
                target_slot: *target_slot,
            }),
            CommandPhase::OpenSpellMenu => Some(Resolution::OpenSpellMenu),
            CommandPhase::OpenItemMenu => Some(Resolution::OpenItemMenu),
            CommandPhase::Aborted => Some(Resolution::Aborted),
            _ => None,
        }
    }

    /// Advance one frame. `party` / `monsters` describe slot occupancy +
    /// alive state for the target picker (rebuilt by the host from the live
    /// actor table each frame). A no-op once the session has resolved.
    pub fn input(
        &mut self,
        ev: BattleCommandInput,
        party: [SlotState; 3],
        monsters: [SlotState; 5],
    ) {
        match &mut self.phase {
            CommandPhase::Menu { cursor } => {
                self.phase = step_menu(*cursor, ev, self.party_slot, party, monsters);
            }
            CommandPhase::Targeting { command, picker } => {
                let command = *command;
                picker.input(PickerInput {
                    up: ev.up,
                    down: ev.down,
                    left: ev.left,
                    right: ev.right,
                    cross: ev.cross,
                    circle: ev.circle,
                });
                if let Some(outcome) = picker.outcome() {
                    self.phase = match outcome {
                        PickerOutcome::Single { slot, row } => CommandPhase::Confirmed {
                            command,
                            target_row: row,
                            target_slot: slot,
                        },
                        PickerOutcome::Sweep { row } => CommandPhase::Confirmed {
                            command,
                            target_row: row,
                            target_slot: 0,
                        },
                        // Backing out of targeting returns to the menu.
                        PickerOutcome::Cancelled => CommandPhase::Menu {
                            cursor: menu_index(command),
                        },
                        PickerOutcome::NoCandidates => CommandPhase::Aborted,
                    };
                }
            }
            CommandPhase::Confirmed { .. }
            | CommandPhase::OpenSpellMenu
            | CommandPhase::OpenItemMenu
            | CommandPhase::Aborted => {}
        }
    }
}

/// Outcome of a resolved [`BattleCommandSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The player confirmed `command` against the given target.
    Confirmed {
        command: BattleCommand,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// The player picked Magic; the live loop should open the spell submenu
    /// (it owns the caster's learned spells + live MP).
    OpenSpellMenu,
    /// The player picked Item; the live loop should open the inventory
    /// submenu (it owns the live inventory + party stats).
    OpenItemMenu,
    /// No valid action existed; the live loop should fall back to a default
    /// strike on the first living enemy.
    Aborted,
}

/// Index of `command` within [`BattleCommand::MENU`].
fn menu_index(command: BattleCommand) -> u8 {
    BattleCommand::MENU
        .iter()
        .position(|c| *c == command)
        .unwrap_or(0) as u8
}

/// One frame of the command menu. Up/Down move the cursor (wrapping); Cross
/// on an enabled command opens its target picker (or aborts if there is no
/// valid target). Disabled commands and Circle are no-ops in v0.1.
fn step_menu(
    cursor: u8,
    ev: BattleCommandInput,
    party_slot: u8,
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> CommandPhase {
    let len = BattleCommand::MENU.len() as u8;
    let mut cursor = cursor.min(len - 1);

    if ev.up {
        cursor = (cursor + len - 1) % len;
    } else if ev.down {
        cursor = (cursor + 1) % len;
    }

    if ev.cross {
        let command = BattleCommand::MENU[cursor as usize];
        // Magic / Item hand off to the host's own submenus instead of opening
        // a target cursor here - the picker can't show spell / item rows.
        if command == BattleCommand::Magic {
            return CommandPhase::OpenSpellMenu;
        }
        if command == BattleCommand::Item {
            return CommandPhase::OpenItemMenu;
        }
        if command.enabled() {
            let picker =
                TargetPickerSession::new(command.target_kind(), party_slot, party, monsters);
            // Immediate kinds (and empty-target kinds) resolve in the
            // constructor; fold that here so we don't stall a frame.
            if let Some(outcome) = picker.outcome() {
                return match outcome {
                    PickerOutcome::Single { slot, row } => CommandPhase::Confirmed {
                        command,
                        target_row: row,
                        target_slot: slot,
                    },
                    PickerOutcome::Sweep { row } => CommandPhase::Confirmed {
                        command,
                        target_row: row,
                        target_slot: 0,
                    },
                    PickerOutcome::NoCandidates => CommandPhase::Aborted,
                    PickerOutcome::Cancelled => CommandPhase::Menu { cursor },
                };
            }
            return CommandPhase::Targeting { command, picker };
        }
    }

    CommandPhase::Menu { cursor }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(present: bool) -> SlotState {
        SlotState::alive(present, true)
    }

    fn party3() -> [SlotState; 3] {
        [alive(true), alive(true), alive(true)]
    }

    fn one_monster() -> [SlotState; 5] {
        [
            alive(true),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ]
    }

    fn press_cross() -> BattleCommandInput {
        BattleCommandInput {
            cross: true,
            ..Default::default()
        }
    }

    #[test]
    fn opens_on_first_enabled_command() {
        let s = BattleCommandSession::new(0, 0);
        assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
        assert!(s.resolved().is_none());
    }

    #[test]
    fn cross_selects_attack_then_cross_confirms_target() {
        let mut s = BattleCommandSession::new(0, 0);
        // First Cross selects Attack -> opens the target cursor on the lone
        // monster (a single-enemy picker still shows a cursor; only sweep
        // kinds auto-confirm).
        s.input(press_cross(), party3(), one_monster());
        assert!(s.resolved().is_none());
        assert!(matches!(s.phase, CommandPhase::Targeting { .. }));
        // Second Cross confirms the target.
        s.input(press_cross(), party3(), one_monster());
        assert_eq!(
            s.resolved(),
            Some(Resolution::Confirmed {
                command: BattleCommand::Attack,
                target_row: CursorRow::Enemy,
                target_slot: 0,
            })
        );
    }

    #[test]
    fn target_cursor_walks_multiple_monsters_before_confirm() {
        let mut monsters = one_monster();
        monsters[1] = alive(true);
        monsters[2] = alive(true);
        let mut s = BattleCommandSession::new(0, 0);
        // Select Attack -> opens the cursor on monster 0 (multiple targets,
        // so it doesn't auto-resolve).
        s.input(press_cross(), party3(), monsters);
        assert!(matches!(s.phase, CommandPhase::Targeting { .. }));
        // Move right twice, then confirm monster 2.
        s.input(
            BattleCommandInput {
                right: true,
                ..Default::default()
            },
            party3(),
            monsters,
        );
        s.input(
            BattleCommandInput {
                right: true,
                ..Default::default()
            },
            party3(),
            monsters,
        );
        s.input(press_cross(), party3(), monsters);
        assert_eq!(
            s.resolved(),
            Some(Resolution::Confirmed {
                command: BattleCommand::Attack,
                target_row: CursorRow::Enemy,
                target_slot: 2,
            })
        );
    }

    #[test]
    fn circle_in_targeting_returns_to_menu() {
        let mut monsters = one_monster();
        monsters[1] = alive(true);
        let mut s = BattleCommandSession::new(0, 0);
        s.input(press_cross(), party3(), monsters);
        assert!(matches!(s.phase, CommandPhase::Targeting { .. }));
        s.input(
            BattleCommandInput {
                circle: true,
                ..Default::default()
            },
            party3(),
            monsters,
        );
        assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
        assert!(s.resolved().is_none());
    }

    #[test]
    fn disabled_commands_are_not_selectable() {
        let mut s = BattleCommandSession::new(0, 0);
        // Move down to Arts (index 1) and try to confirm: stays in the menu.
        s.input(
            BattleCommandInput {
                down: true,
                ..Default::default()
            },
            party3(),
            one_monster(),
        );
        assert_eq!(s.menu_command(), Some(BattleCommand::Arts));
        s.input(press_cross(), party3(), one_monster());
        assert!(s.resolved().is_none());
        assert_eq!(s.menu_command(), Some(BattleCommand::Arts));
    }

    #[test]
    fn item_command_resolves_to_open_item_menu() {
        let mut s = BattleCommandSession::new(0, 0);
        // Walk the cursor down to Item (index 3) - Arts/Magic are skipped over
        // even though they're disabled, the cursor still moves through them.
        for _ in 0..3 {
            s.input(
                BattleCommandInput {
                    down: true,
                    ..Default::default()
                },
                party3(),
                one_monster(),
            );
        }
        assert_eq!(s.menu_command(), Some(BattleCommand::Item));
        s.input(press_cross(), party3(), one_monster());
        // Item hands off to the host's inventory submenu rather than opening a
        // target cursor here.
        assert_eq!(s.resolved(), Some(Resolution::OpenItemMenu));
    }

    #[test]
    fn magic_command_resolves_to_open_spell_menu() {
        let mut s = BattleCommandSession::new(0, 0);
        // Down twice: Attack (0) -> Arts (1) -> Magic (2).
        for _ in 0..2 {
            s.input(
                BattleCommandInput {
                    down: true,
                    ..Default::default()
                },
                party3(),
                one_monster(),
            );
        }
        assert_eq!(s.menu_command(), Some(BattleCommand::Magic));
        s.input(press_cross(), party3(), one_monster());
        assert_eq!(s.resolved(), Some(Resolution::OpenSpellMenu));
    }

    #[test]
    fn no_living_target_aborts() {
        let mut s = BattleCommandSession::new(0, 0);
        let dead_monsters = [
            SlotState::alive(true, false),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ];
        s.input(press_cross(), party3(), dead_monsters);
        assert_eq!(s.resolved(), Some(Resolution::Aborted));
    }
}
