//! The dome's **command flow** - the battle's own selection screens, run
//! over a [`MuscleDomeSession`].
//!
//! A dome round is an ordinary battle round (`FUN_801D0748`, the battle
//! overlay's round driver), so its selection is the battle's, screen for
//! screen: the command ring (`ctx+6 = 0x28`), the `Auto | Command` prompt
//! (`0x78`), the direction entry (`0x50`) and its review (`0x5A`), the
//! Ra-Seru list (`0x46`) and the `Begin | Reselect` confirm (`0x6E`). The
//! port already models every one of those for the regular battle:
//! [`BattleCommandSession`] runs the ring, the attack-mode prompt and the
//! commit confirm, and [`ArtsCommandInputSession`] the entry and its review.
//! [`DomeMenu`] holds whichever of them owns the pad, so a dome selection is
//! driven by the same step functions a battle selection is - and the HUD
//! projects it through the same chip clusters and arts-input chrome
//! (`crate::battle_hud::battle_command_chips`,
//! `crate::world::World::arts_input_view`).
//!
//! What the session itself still owns is the dome bookkeeping those screens
//! write into: the fighter's queue / budget / spent triple
//! (`actor+0x1DF`, `ctx+0x6DC`, `ctx+0x6D8`), which the entry session is
//! mirrored into after every press, and the cast the Ra-Seru list commits.
//!
//! Disclosed host models, each where the port has nothing to run:
//!
//! * **Item** refuses. The ring's Item arm opens the battle item window; the
//!   dome session carries no bag, so the press is refused like a forbidden
//!   chip (every dome course's special word forbids items anyway).
//! * **Auto** fills the string greedily in deal order (the opponent's own
//!   selection rule). Retail's Auto reloads the character's saved string and
//!   the round's pool arm rebuilds it (`FUN_801F0450`; see
//!   `docs/subsystems/minigame-muscle-dome.md`, "The Auto arm reloads a saved
//!   string").
//! * **Spirit** commits an empty string: the session has no guard stance or
//!   Spirit charge to apply, so the fighter simply does not swing.

use super::*;
use crate::arts_command_input::{
    ARTS_LIST_ROWS_PER_PAGE, ArtsCommandInputSession, ArtsCommandPad, ArtsInputResolution,
};
use crate::battle_input::{
    BattleCommand, BattleCommandInput, BattleCommandSession, CommandPhase, Resolution,
};
use crate::target_picker::SlotState;

/// Which selection screen owns the dome fighter's pad.
#[derive(Debug, Clone)]
pub enum DomeMenu {
    /// The command ring (`0x28`), the `Auto | Command` prompt (`0x78`) or
    /// the `Begin | Reselect` confirm (`0x6E`) - the battle's own session.
    Command(BattleCommandSession),
    /// The direction entry (`0x50`) and its review (`0x5A`) - the battle's
    /// own arts-entry session.
    Input(ArtsCommandInputSession),
    /// The Ra-Seru list (`0x46`), over [`MuscleDomeSession::spell_rows`].
    Magic,
}

impl Default for DomeMenu {
    fn default() -> Self {
        DomeMenu::Command(BattleCommandSession::new(0, 0))
    }
}

/// What one frame of the command flow did - the cue a host plays for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomeMenuEvent {
    /// Nothing changed.
    Idle,
    /// A cursor moved or a screen changed without committing anything.
    Cursor,
    /// Something was taken: a direction, a pick, a commit.
    Confirm,
    /// The press was refused (unaffordable, forbidden, nothing to open).
    Refused,
    /// `Begin` was taken: both fighters' selections are closed and the turn
    /// is ready to resolve.
    Fight,
}

/// The ring seat of a [`DomeRingChip`] in [`BattleCommand::MENU`] order.
fn ring_cursor(chip: DomeRingChip) -> u8 {
    match chip {
        DomeRingChip::Item => 0,
        DomeRingChip::Attack => 1,
        DomeRingChip::RaSeru => 2,
        DomeRingChip::Spirit => 3,
    }
}

fn ring_at(cursor: u8) -> BattleCommandSession {
    let mut s = BattleCommandSession::new(0, 0);
    s.phase = CommandPhase::Menu { cursor };
    s
}

fn command_cursor(phase: &CommandPhase) -> Option<u8> {
    match phase {
        CommandPhase::RoundPrompt { cursor }
        | CommandPhase::Menu { cursor }
        | CommandPhase::AttackMode { cursor }
        | CommandPhase::CommitConfirm { cursor } => Some(*cursor),
        _ => None,
    }
}

impl MuscleDomeSession {
    /// The screen that owns the player's pad.
    pub fn menu(&self) -> &DomeMenu {
        &self.menu
    }

    /// The slot occupancy the battle's target picker reads: the fighter
    /// alone on the party row, the opponent alone on the monster row.
    fn picker_rows(&self) -> ([SlotState; 3], [SlotState; 5]) {
        let empty = SlotState::alive(false, false);
        (
            [SlotState::alive(true, self.f[0].hp > 0), empty, empty],
            [
                SlotState::alive(true, self.f[1].hp > 0),
                empty,
                empty,
                empty,
                empty,
            ],
        )
    }

    /// The four per-direction entry costs in the entry session's order
    /// (Left, Right, Down, Up = command ids `0xC..=0xF`), off the fighter's
    /// dealt hand.
    fn entry_costs(&self) -> [u16; 4] {
        let mut costs = [crate::arts_command_input::FAVORED_COST; 4];
        for card in &self.f[0].hand {
            if let Some(cmd) = dome_command_of_action_byte(card.command_id) {
                costs[usize::from(cmd.as_byte() - 1)] = card.cost;
            }
        }
        costs
    }

    /// Pages of the entry screen's Triangle arts list - the fighter's
    /// learned normal arts, five rows a page.
    pub fn arts_list_pages(&self) -> u8 {
        self.art_catalog[0]
            .len()
            .div_ceil(ARTS_LIST_ROWS_PER_PAGE)
            .min(u8::MAX as usize) as u8
    }

    /// Open the direction entry over the fighter's live budget.
    fn open_entry(&self) -> ArtsCommandInputSession {
        ArtsCommandInputSession::new(
            0,
            0,
            self.f[0].budget,
            self.entry_costs(),
            self.arts_list_pages(),
        )
    }

    /// Mirror the entry session's buffer into the fighter's queue / budget /
    /// spent triple - the retail state both screens write
    /// (`actor+0x1DF`, `ctx+0x6DC`, `ctx+0x6D8`).
    fn sync_entry(&mut self) {
        let DomeMenu::Input(entry) = &self.menu else {
            return;
        };
        let string: &[u8] = entry.committed_string();
        let queue: Vec<u8> = string
            .iter()
            .take(QUEUE_CAP)
            .map(|b| b + HAND_COMMAND_BASE)
            .collect();
        let spent: u16 = entry.spent.iter().sum();
        let f = &mut self.f[0];
        f.queue = queue;
        f.spent = spent;
        f.budget = f.budget_pool.saturating_sub(spent);
    }

    /// The `Begin | Reselect` confirm, raised after the fighter's commit.
    fn raise_commit_confirm(&mut self) {
        self.menu = DomeMenu::Command(BattleCommandSession::new_commit_confirm(0, 0));
    }

    /// One frame of edge-triggered pad through the command flow - the one
    /// selection surface every dome host drives.
    ///
    /// Retail runs these screens in `FUN_801D0748`'s phases `0x28` / `0x78`
    /// / `0x50` / `0x5A` / `0x46` / `0x6E`; the step rules are the battle
    /// sessions' ([`BattleCommandSession::input`],
    /// [`ArtsCommandInputSession::input`]), so the dome cannot grow an input
    /// rule the battle does not have.
    ///
    /// REF: FUN_801d0748 (the command flow; the per-screen step functions
    /// carry the port tags)
    pub fn select_input(&mut self, pad: DomeSelectPad) -> DomeMenuEvent {
        if self.phase != MusclePhase::Select {
            return DomeMenuEvent::Idle;
        }
        match std::mem::take(&mut self.menu) {
            DomeMenu::Command(cmd) => self.command_step(cmd, pad),
            DomeMenu::Input(entry) => self.entry_step(entry, pad),
            DomeMenu::Magic => self.magic_step(pad),
        }
    }

    fn command_step(&mut self, mut cmd: BattleCommandSession, pad: DomeSelectPad) -> DomeMenuEvent {
        let before = command_cursor(&cmd.phase);
        let before_screen = std::mem::discriminant(&cmd.phase);
        let was_ring = matches!(cmd.phase, CommandPhase::Menu { .. });
        let (party, monsters) = self.picker_rows();
        cmd.input(
            BattleCommandInput {
                up: pad.up,
                down: pad.down,
                left: pad.left,
                right: pad.right,
                cross: pad.confirm,
                circle: pad.cancel,
                select_attack: pad.select_attack,
            },
            party,
            monsters,
        );
        // Every arm the Attack chip opens (the prompt, or the option word's
        // straight-to-entry / straight-to-strike shortcuts) is refused while
        // the chip is - retail's arm tests the Rot limbs before it moves.
        let attack_arm = was_ring
            && (matches!(cmd.phase, CommandPhase::AttackMode { .. })
                || matches!(cmd.resolved(), Some(Resolution::OpenArtsMenu))
                || matches!(
                    cmd.resolved(),
                    Some(Resolution::Confirmed {
                        command: BattleCommand::Attack,
                        ..
                    })
                ));
        if attack_arm && !self.chip_enabled(0, DomeRingChip::Attack) {
            self.menu = DomeMenu::Command(ring_at(ring_cursor(DomeRingChip::Attack)));
            return DomeMenuEvent::Refused;
        }
        match cmd.resolved() {
            None => {
                let moved = command_cursor(&cmd.phase) != before
                    || std::mem::discriminant(&cmd.phase) != before_screen;
                self.menu = DomeMenu::Command(cmd);
                if moved {
                    DomeMenuEvent::Cursor
                } else {
                    DomeMenuEvent::Idle
                }
            }
            Some(Resolution::OpenItemMenu) => {
                self.menu = DomeMenu::Command(ring_at(ring_cursor(DomeRingChip::Item)));
                DomeMenuEvent::Refused
            }
            Some(Resolution::OpenSpellMenu) => match self.open_magic(0) {
                Ok(()) => {
                    self.menu = DomeMenu::Magic;
                    DomeMenuEvent::Confirm
                }
                Err(_) => {
                    self.menu = DomeMenu::Command(ring_at(ring_cursor(DomeRingChip::RaSeru)));
                    DomeMenuEvent::Refused
                }
            },
            Some(Resolution::OpenArtsMenu) => {
                self.menu = DomeMenu::Input(self.open_entry());
                DomeMenuEvent::Confirm
            }
            Some(Resolution::Confirmed { .. }) => {
                // Auto: the string is filled for the player (host model,
                // see the module docs) and the confirm comes up.
                self.ai_commit_all(0);
                self.raise_commit_confirm();
                DomeMenuEvent::Confirm
            }
            Some(Resolution::SpiritGuard) => {
                self.raise_commit_confirm();
                DomeMenuEvent::Confirm
            }
            Some(Resolution::BeginRound) => {
                self.ai_commit_all(1);
                self.end_selection();
                self.menu = DomeMenu::default();
                DomeMenuEvent::Fight
            }
            Some(Resolution::Reselect) => {
                // Retail's `0x6E` Reselect steps back to the last member's
                // ring with its string thrown away; the dome fields one.
                self.reset_selection(0);
                self.menu = DomeMenu::Command(ring_at(ring_cursor(DomeRingChip::Attack)));
                DomeMenuEvent::Cursor
            }
            // Cancel on the ring has no earlier member to step back to, a
            // dome round has no Run, and an abort has no target to miss:
            // all three leave the ring up.
            Some(Resolution::StepBack | Resolution::RunAway | Resolution::Aborted) => {
                self.menu = DomeMenu::Command(ring_at(before.unwrap_or(1)));
                DomeMenuEvent::Idle
            }
        }
    }

    fn entry_step(
        &mut self,
        mut entry: ArtsCommandInputSession,
        pad: DomeSelectPad,
    ) -> DomeMenuEvent {
        let (party, monsters) = self.picker_rows();
        let before = (entry.buffer.len(), entry.list_page, entry.pool);
        let reviewing = matches!(
            entry.phase,
            crate::arts_command_input::ArtsInputPhase::Review
        );
        entry.input(
            ArtsCommandPad {
                up: pad.up,
                down: pad.down,
                left: pad.left,
                right: pad.right,
                cross: pad.confirm,
                circle: pad.cancel,
                triangle: pad.triangle,
            },
            party,
            monsters,
        );
        // The review's commit opens the battle's target cursor; the dome
        // fields one opponent, and the captured dome chain runs the review
        // (`0x5A`) straight into the Begin | Reselect confirm (`0x6E`), so a
        // cursor that opens is taken as the commit.
        let targeting = matches!(
            entry.phase,
            crate::arts_command_input::ArtsInputPhase::Targeting { .. }
        );
        match entry.resolved() {
            _ if targeting => {
                self.menu = DomeMenu::Input(entry);
                self.sync_entry();
                self.raise_commit_confirm();
                DomeMenuEvent::Confirm
            }
            Some(ArtsInputResolution::Confirmed { .. }) => {
                self.menu = DomeMenu::Input(entry);
                self.sync_entry();
                self.raise_commit_confirm();
                DomeMenuEvent::Confirm
            }
            Some(ArtsInputResolution::Aborted) => {
                // An empty entry backs out to the attack-mode prompt
                // (`0x801D219C`), its `Command` chip under the cursor.
                self.f[0].queue.clear();
                self.f[0].budget = self.f[0].budget_pool;
                self.f[0].spent = 0;
                let mut cmd = BattleCommandSession::new(0, 0);
                cmd.phase = CommandPhase::AttackMode { cursor: 1 };
                self.menu = DomeMenu::Command(cmd);
                DomeMenuEvent::Cursor
            }
            None => {
                let pressed_dir = pad.up || pad.down || pad.left || pad.right;
                let after = (entry.buffer.len(), entry.list_page, entry.pool);
                let to_review = !reviewing
                    && matches!(
                        entry.phase,
                        crate::arts_command_input::ArtsInputPhase::Review
                    );
                self.menu = DomeMenu::Input(entry);
                self.sync_entry();
                if after.0 > before.0 || to_review {
                    DomeMenuEvent::Confirm
                } else if pressed_dir && !reviewing {
                    DomeMenuEvent::Refused
                } else if after != before {
                    DomeMenuEvent::Cursor
                } else {
                    DomeMenuEvent::Idle
                }
            }
        }
    }

    fn magic_step(&mut self, pad: DomeSelectPad) -> DomeMenuEvent {
        if pad.cancel {
            self.close_magic();
            self.menu = DomeMenu::Command(ring_at(ring_cursor(DomeRingChip::RaSeru)));
            return DomeMenuEvent::Cursor;
        }
        self.menu = DomeMenu::Magic;
        let mut moved = false;
        if pad.up {
            self.move_magic_cursor(0, -1);
            moved = true;
        }
        if pad.down {
            self.move_magic_cursor(0, 1);
            moved = true;
        }
        if pad.confirm {
            // A refusal leaves the list open, which is retail's answer to an
            // unaffordable pick (`0x8007BB94` is cleared and the arm returns
            // without committing).
            return match self.confirm_magic(0) {
                Ok(_) => {
                    self.raise_commit_confirm();
                    DomeMenuEvent::Confirm
                }
                Err(_) => DomeMenuEvent::Refused,
            };
        }
        if moved {
            DomeMenuEvent::Cursor
        } else {
            DomeMenuEvent::Idle
        }
    }

    /// The press a scripted driver makes to play the player's selection
    /// through the command flow: the ring's Attack arm, the prompt's
    /// `Command` chip, the cheapest still-affordable direction until the
    /// entry auto-ends, then confirm through the review and `Begin`. `None`
    /// outside the selection. Harnesses (the world-tick oracles, the soak
    /// and replay drivers) share it so none of them encodes the screen order
    /// on its own.
    pub fn scripted_press(&self) -> Option<crate::input::PadButton> {
        use crate::input::PadButton;
        if self.phase != MusclePhase::Select {
            return None;
        }
        Some(match &self.menu {
            DomeMenu::Command(c) => match c.phase {
                CommandPhase::Menu { .. } => PadButton::Left,
                CommandPhase::AttackMode { .. } => PadButton::Right,
                _ => PadButton::Cross,
            },
            DomeMenu::Input(_) => {
                let pick = (0..HAND_SLOTS)
                    .filter(|&c| self.can_commit(0, c))
                    .min_by_key(|&c| self.f[0].hand[c].cost);
                match pick.map(|c| self.f[0].hand[c].command_id) {
                    Some(0x0C) => PadButton::Left,
                    Some(0x0D) => PadButton::Right,
                    Some(0x0E) => PadButton::Down,
                    Some(_) => PadButton::Up,
                    None => PadButton::Cross,
                }
            }
            DomeMenu::Magic => PadButton::Circle,
        })
    }

    /// The entry screen's view, while the direction entry or its review owns
    /// the pad - the snapshot the shared arts-input chrome draws from
    /// (`crate::world::World::arts_input_view` hands it to the hosts during a
    /// dome leg).
    pub fn arts_input_view(&self) -> Option<crate::arts_command_input::ArtsInputView<'_>> {
        let DomeMenu::Input(s) = &self.menu else {
            return None;
        };
        if self.phase != MusclePhase::Select {
            return None;
        }
        Some(crate::arts_command_input::ArtsInputView {
            buffer: &s.buffer,
            spent: &s.spent,
            pool: s.pool,
            pool_max: s.pool_max,
            costs: s.costs,
            // The right-hand plate reads the fighter's Spirit gauge, which
            // never moves during entry.
            plate_value: self.spirit(0).min(100) as u8,
            list_page: s.list_page,
            list_pages: s.list_pages,
            phase: (&s.phase).into(),
            status: self.ring(0).status,
        })
    }

    /// The command cluster this frame draws for the player, as the battle's
    /// chip projection: `(label, enabled)` per chip in seat order, the
    /// cursor, and the cluster. `None` while the entry or the Ra-Seru list
    /// owns the pad, or outside the selection.
    ///
    /// `raseru_label` is the ring's right-arm word - the fighter's Ra-Seru
    /// name, or `-` for a fighter carrying none (`FUN_801D8DE8` record
    /// `0xA`); `confirm_labels` the disc's `Begin` / `Reselect` words.
    pub fn command_chips(
        &self,
        raseru_label: &str,
        confirm_labels: [&str; 2],
    ) -> Option<crate::battle_hud::BattleCommandChips> {
        use crate::battle_hud::{BattleCommandChips, CommandChipPhase};
        use crate::battle_input::AttackMode;
        if self.phase != MusclePhase::Select {
            return None;
        }
        let DomeMenu::Command(cmd) = &self.menu else {
            return None;
        };
        let chip = |label: &str, enabled: bool| (label.to_string(), enabled);
        match cmd.phase {
            CommandPhase::Menu { cursor } => Some(BattleCommandChips {
                chips: DomeRingChip::RING
                    .iter()
                    .map(|c| {
                        let label = match c {
                            DomeRingChip::Item => BattleCommand::Item.label(),
                            DomeRingChip::Attack => BattleCommand::Attack.label(),
                            DomeRingChip::RaSeru => raseru_label,
                            DomeRingChip::Spirit => BattleCommand::Spirit.label(),
                        };
                        chip(label, self.chip_enabled(0, *c))
                    })
                    .collect(),
                cursor: usize::from(cursor),
                phase: CommandChipPhase::CommandRing,
            }),
            CommandPhase::AttackMode { cursor } => Some(BattleCommandChips {
                chips: AttackMode::PROMPT
                    .iter()
                    .map(|m| chip(m.label(), true))
                    .collect(),
                cursor: usize::from(cursor),
                phase: CommandChipPhase::AttackMode,
            }),
            CommandPhase::CommitConfirm { cursor } => Some(BattleCommandChips {
                chips: confirm_labels.iter().map(|l| chip(l, true)).collect(),
                cursor: usize::from(cursor),
                phase: CommandChipPhase::CommitConfirm,
            }),
            _ => None,
        }
    }

    /// The marks the ring wears this frame (only while the ring is up), in
    /// the battle's [`RingMarks`](legaia_engine_vm::battle_party_panel::RingMarks)
    /// shape so the shared chip builder draws them.
    pub fn ring_marks(&self) -> legaia_engine_vm::battle_party_panel::RingMarks {
        let ring_up = self.phase == MusclePhase::Select
            && matches!(&self.menu, DomeMenu::Command(c) if matches!(c.phase, CommandPhase::Menu { .. }));
        if !ring_up {
            return Default::default();
        }
        let mark = |c| self.chip_mark(0, c);
        legaia_engine_vm::battle_party_panel::RingMarks {
            item_forbidden: mark(DomeRingChip::Item) == Some(ChipMark::Forbidden),
            raseru_forbidden: mark(DomeRingChip::RaSeru) == Some(ChipMark::Forbidden),
            attack_rotted: mark(DomeRingChip::Attack) == Some(ChipMark::Blocked),
            magic_cursed: mark(DomeRingChip::RaSeru) == Some(ChipMark::Sealed),
        }
    }
}

/// The action-byte base of the four direction commands: entry byte `n`
/// (`Command::as_byte`, Left = 1) is command id `0xB + n`.
const HAND_COMMAND_BASE: u8 = legaia_asset::muscle_dome::HAND_COMMAND_MIN - 1;
