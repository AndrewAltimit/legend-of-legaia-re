//! Player-driven battle command input for the live gameplay loop.
//!
//! The live battle loop (`crate::world::World::live_battle_tick`) can run a
//! battle two ways. By default it auto-resolves: every party turn commits a
//! physical Attack with no player choice. When
//! [`crate::world::World::battle_player_driven`] is set, each party turn pauses
//! the action state machine and runs a [`BattleCommandSession`] that reads the
//! pad.
//!
//! ## The open flow is three prompts, not one list
//!
//! Retail's battle dispatcher `FUN_801D0748` walks `ctx[+0x06]` through three
//! separate selection surfaces, and each is a **two- or four-chip cluster keyed
//! to the face buttons**, never a scrolling list. The port follows the same
//! shape ([`CommandPhase`]):
//!
//! | `ctx[+0x06]` | Phase | Chips |
//! |---|---|---|
//! | `0x1E` | [`CommandPhase::RoundPrompt`] | `Begin` / `Run` |
//! | `0x28` | [`CommandPhase::Menu`] | `Item` / `Attack` / magic / `Spirit` |
//! | `0x78` | [`CommandPhase::AttackMode`] | `Auto` / `Command` |
//!
//! The round prompt runs **once per round**, before the first party member
//! commands: `801d0e3c` (the intro-timer state `0x0B`) hands the flow to `0x14`
//! and `0x14` sets `0x1E` unconditionally, and the action SM's round-end
//! (`801e67e8`) parks the flow back at `0x14`. The one path that skips it is a
//! **back attack**: `ctx[+0x290] == 1` sends state `0x0B` straight to `0xFE`
//! (round armed, no input), which is why an ambushed party never gets to enter
//! a command that round.
//!
//! The command ring's four arms are seated on the pinned diamond, and each is a
//! face button rather than a cursor stop: Triangle picks the up arm (`Item`),
//! Square the left (`Attack`), Circle the right (magic), Cross the down
//! (`Spirit`). The port keeps a cursor on top of the same four seats so a
//! keyboard host has something to move, but the seating is retail's.
//!
//! **`Attack` is not the plain strike** - it is the door to the attack-mode
//! prompt (`0x78`), whose two chips are retail's `Auto` (auto-target the swing)
//! and `Command` (open the directional arts entry). That is the port's `Attack`
//! and its `Arts` under one ring arm, which is where retail puts them.
//!
//! `Item` / magic / `Command` resolve to [`Resolution::OpenItemMenu`] /
//! [`Resolution::OpenSpellMenu`] / [`Resolution::OpenArtsMenu`] hand-offs: the
//! command session can't run those pickers itself (they need the caster's saved
//! chains / learned spells / live MP / inventory + party stats), so the live
//! loop opens a host-owned [`crate::battle_arts::BattleArtsSession`] /
//! [`crate::battle_magic::BattleSpellSession`] /
//! [`crate::inventory_use::InventoryUseSession`] instead. `Spirit` and `Run`
//! resolve immediately (no target). Target selection reuses
//! [`crate::target_picker`].
//!
//! The session is a small state machine driven one frame at a time by
//! [`BattleCommandSession::input`] with an edge-triggered
//! [`BattleCommandInput`] (the host derives the edges from
//! [`crate::input::InputState`]). When [`BattleCommandSession::resolved`]
//! returns a value the live loop arms the action SM with the chosen target.
//!
//! Chip labels are the retail words. Their disc coordinates - the SCUS block at
//! `0x8007B658..0x8007B68D` and the battle overlay's own pool - are pinned in
//! [`legaia_asset::battle_ui_strings`], which is also where the per-character
//! Ra-Seru name the magic arm really carries (`Meta` / `Terra` / `Ozma`) comes
//! from.

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};

/// A top-level battle command, as listed in the battle command menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleCommand {
    /// Physical attack - opens a target cursor and commits a strike.
    Attack,
    /// Tactical Arts - hands off to the host saved-chain submenu (see
    /// [`crate::battle_arts`]).
    Arts,
    /// Magic spell - hands off to the host battle spell submenu (see
    /// [`crate::battle_magic`]).
    Magic,
    /// Use an item - hands off to the host inventory submenu (see
    /// [`crate::inventory_use`]).
    Item,
    /// Spirit: guard for the turn (+5 AP via
    /// [`crate::ap_gauge::ApGauge::charge_spirit`], guard-halved damage until
    /// the next turn). Resolves immediately - no target.
    Spirit,
    /// Run: attempt to flee through the action SM's run band
    /// (`RunBegin`/`RunWait`/`RunEscape`, retail states `0x64..0x66`).
    /// Resolves immediately - no target.
    Run,
}

impl BattleCommand {
    /// The command ring's four arms, in **seat order** - up, left, right,
    /// down - which is the order retail's placement records 8..=11 sit in.
    ///
    /// `Run` is not here: it belongs to the round prompt
    /// ([`CommandPhase::RoundPrompt`]), one level above. `Arts` is not here
    /// either: it is the attack-mode prompt's `Command` chip, one level below
    /// the `Attack` arm ([`CommandPhase::AttackMode`]).
    pub const MENU: [BattleCommand; 4] = [
        BattleCommand::Item,
        BattleCommand::Attack,
        BattleCommand::Magic,
        BattleCommand::Spirit,
    ];

    /// `true` when the command can actually be selected in the live loop.
    /// All six commands are wired: Attack (physical strike), Arts (saved-chain
    /// submenu), Magic (spell submenu), Item (inventory submenu), Spirit
    /// (guard + AP charge) and Run (the action SM's run band).
    pub fn enabled(self) -> bool {
        matches!(
            self,
            BattleCommand::Attack
                | BattleCommand::Arts
                | BattleCommand::Magic
                | BattleCommand::Item
                | BattleCommand::Spirit
                | BattleCommand::Run
        )
    }

    /// Can the command be chosen in *this* battle?
    ///
    /// [`Self::enabled`] answers "is the command wired at all"; this adds
    /// the per-battle refusal the retail flow has: a scripted no-escape
    /// battle ([`crate::world::World::battle_no_escape`], the same flag the
    /// field loop honours) forbids **Run**. A command that answers `false`
    /// still draws its chip - retail keeps the plate and puts a single `-`
    /// where the word would go
    /// (`legaia_engine_ui::battle_command_ui`).
    pub fn available(self, no_escape: bool) -> bool {
        if matches!(self, BattleCommand::Run) && no_escape {
            return false;
        }
        self.enabled()
    }

    /// Short label for the HUD / command menu.
    pub fn label(self) -> &'static str {
        match self {
            BattleCommand::Attack => "Attack",
            BattleCommand::Arts => "Arts",
            BattleCommand::Magic => "Magic",
            BattleCommand::Item => "Item",
            BattleCommand::Spirit => "Spirit",
            BattleCommand::Run => "Run",
        }
    }

    /// The target the command applies to. v0.1 only resolves Attack
    /// (single enemy); the rest carry their natural kind for when they land.
    /// Spirit / Run never open a picker (they resolve without a target) -
    /// their kinds here are placeholders.
    pub fn target_kind(self) -> TargetKind {
        match self {
            BattleCommand::Attack | BattleCommand::Arts => TargetKind::SingleEnemy,
            BattleCommand::Magic | BattleCommand::Run => TargetKind::SingleEnemy,
            BattleCommand::Item | BattleCommand::Spirit => TargetKind::SingleAllyOrSelf,
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

/// The round-open prompt's two chips - retail flow state `0x1E`, the pair
/// whose labels the placement table points straight at `SCUS_942.54`
/// (`0x8007B688` / `0x8007B684`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundChoice {
    /// Fight this round - falls through to the command ring.
    Begin,
    /// Try to flee. Retail takes this on Circle without a confirm press.
    Run,
}

impl RoundChoice {
    /// The prompt's chips in seat order (left, right).
    pub const PROMPT: [RoundChoice; 2] = [RoundChoice::Begin, RoundChoice::Run];

    /// Chip label.
    pub fn label(self) -> &'static str {
        match self {
            RoundChoice::Begin => "Begin",
            RoundChoice::Run => "Run",
        }
    }
}

/// The attack-mode prompt's two chips - retail flow state `0x78`, seated on the
/// command diamond's own left / right arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackMode {
    /// Auto-target the swing: the port's plain strike + target cursor.
    /// Retail state `0x5A`.
    Auto,
    /// Open the directional command entry: the port's arts submenu. Retail
    /// state `0x50`.
    Command,
}

impl AttackMode {
    /// The prompt's chips in seat order (left, right).
    pub const PROMPT: [AttackMode; 2] = [AttackMode::Auto, AttackMode::Command];

    /// Chip label.
    pub fn label(self) -> &'static str {
        match self {
            AttackMode::Auto => "Auto",
            AttackMode::Command => "Command",
        }
    }
}

/// Sub-phase of one party member's command selection.
#[derive(Debug, Clone)]
pub enum CommandPhase {
    /// The round-open `Begin` / `Run` prompt (retail `ctx[+0x06] == 0x1E`).
    /// `cursor` indexes [`RoundChoice::PROMPT`]. Raised once per round, ahead
    /// of the round's first party command.
    RoundPrompt { cursor: u8 },
    /// Choosing a ring command. `cursor` indexes [`BattleCommand::MENU`].
    Menu { cursor: u8 },
    /// The `Auto` / `Command` prompt the `Attack` arm opens (retail
    /// `ctx[+0x06] == 0x78`). `cursor` indexes [`AttackMode::PROMPT`].
    AttackMode { cursor: u8 },
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
    /// The player picked Arts. Hands off (like Magic / Item): the live loop
    /// opens a [`crate::battle_arts::BattleArtsSession`] over the caster's saved
    /// chains, executes the chosen art, then cycles the turn.
    OpenArtsMenu,
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
    /// The player picked Spirit: the live loop charges the AP gauge and sets
    /// the guard stance, then consumes the turn. No target.
    SpiritGuard,
    /// The player picked Run: the live loop rolls the escape and arms the
    /// action SM's run band. No target.
    RunAway,
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
    /// Scripted no-escape battle: the round prompt draws its `Run` chip with
    /// the `-` placeholder and refuses to take it. Set by the live loop from
    /// [`crate::world::World::battle_no_escape`]; defaults to `false` so a
    /// caller that does not know still gets a working prompt.
    pub no_escape: bool,
    pub phase: CommandPhase,
}

impl BattleCommandSession {
    /// Open the command ring for `actor` (party-row index `party_slot`) -
    /// the mid-round entry, used when a submenu is backed out of and when a
    /// later party member of the same round takes its turn.
    pub fn new(actor: u8, party_slot: u8) -> Self {
        let cursor = BattleCommand::MENU
            .iter()
            .position(|c| c.enabled())
            .unwrap_or(0) as u8;
        Self {
            actor,
            party_slot,
            no_escape: false,
            phase: CommandPhase::Menu { cursor },
        }
    }

    /// Open at the **round prompt** instead - retail's `0x1E`, raised once per
    /// round ahead of the round's first party command.
    pub fn new_round_open(actor: u8, party_slot: u8, no_escape: bool) -> Self {
        Self {
            no_escape,
            phase: CommandPhase::RoundPrompt { cursor: 0 },
            ..Self::new(actor, party_slot)
        }
    }

    /// The command currently under the ring cursor, or `None` once the
    /// session has left the ring.
    pub fn menu_command(&self) -> Option<BattleCommand> {
        match self.phase {
            CommandPhase::Menu { cursor } => BattleCommand::MENU.get(cursor as usize).copied(),
            _ => None,
        }
    }

    /// The round-prompt chip under the cursor, while the prompt is up.
    pub fn round_choice(&self) -> Option<RoundChoice> {
        match self.phase {
            CommandPhase::RoundPrompt { cursor } => {
                RoundChoice::PROMPT.get(cursor as usize).copied()
            }
            _ => None,
        }
    }

    /// The attack-mode chip under the cursor, while that prompt is up.
    pub fn attack_mode(&self) -> Option<AttackMode> {
        match self.phase {
            CommandPhase::AttackMode { cursor } => AttackMode::PROMPT.get(cursor as usize).copied(),
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
            CommandPhase::OpenArtsMenu => Some(Resolution::OpenArtsMenu),
            CommandPhase::OpenSpellMenu => Some(Resolution::OpenSpellMenu),
            CommandPhase::OpenItemMenu => Some(Resolution::OpenItemMenu),
            CommandPhase::SpiritGuard => Some(Resolution::SpiritGuard),
            CommandPhase::RunAway => Some(Resolution::RunAway),
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
            CommandPhase::RoundPrompt { cursor } => {
                self.phase = step_round_prompt(*cursor, ev, self.no_escape);
            }
            CommandPhase::AttackMode { cursor } => {
                self.phase = step_attack_mode(*cursor, ev, self.party_slot, party, monsters);
            }
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
            | CommandPhase::OpenArtsMenu
            | CommandPhase::OpenSpellMenu
            | CommandPhase::OpenItemMenu
            | CommandPhase::SpiritGuard
            | CommandPhase::RunAway
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
    /// The player picked Arts; the live loop should open the saved-chain
    /// submenu (it owns the caster's chain library).
    OpenArtsMenu,
    /// The player picked Magic; the live loop should open the spell submenu
    /// (it owns the caster's learned spells + live MP).
    OpenSpellMenu,
    /// The player picked Item; the live loop should open the inventory
    /// submenu (it owns the live inventory + party stats).
    OpenItemMenu,
    /// The player picked Spirit; the live loop should charge the caster's AP
    /// gauge, set its guard stance, and consume the turn.
    SpiritGuard,
    /// The player picked Run; the live loop should roll the escape and arm
    /// the action SM's run band (category 5).
    RunAway,
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

/// The ring entry seated beside `i` on its own row of the drawn diamond.
///
/// The ring is a four-arm cluster, not a list
/// (`legaia_engine_ui::battle_command_ui`): `Attack` and the magic arm are the
/// diamond's two flanking seats and share a row, while `Item` and `Spirit` sit
/// alone on the vertical arms and therefore stay put under Left / Right.
///
/// Up / Down remain the linear walk over [`BattleCommand::MENU`], which is
/// what still reaches every chip from every chip on a keyboard host. Retail
/// needs neither: each arm is one face button.
const fn row_neighbour(i: u8) -> u8 {
    match i {
        1 => 2, // Attack -> magic
        2 => 1, // magic -> Attack
        other => other,
    }
}

/// One frame of the **round prompt** (retail `0x1E`). Left / Right walk the
/// pair, Cross takes the highlighted chip, and Circle takes `Run` outright. A
/// scripted no-escape battle refuses both routes into `Run` and leaves the
/// prompt up.
///
/// **The Circle route is the port's, not retail's, and the citation it used to
/// carry was a raw-pad misread.** `FUN_801D0748`'s handlers test **packed**
/// masks (byte halves swapped against the raw BIOS word), so the
/// `andi v0,s2,0x2000` at `0x801D1058` that routes to flow state `0x32` is
/// **Right**, not Circle; packed Circle is `0x0020`. Retail's `0x1E` is a
/// two-chip prompt whose chips *are* directions - Left `0x8000` takes `Begin`,
/// Right `0x2000` takes `Run` - and confirm reaches one only indirectly: the
/// pre-dispatch block rewrites `s2` to the highlight it has been walking in
/// `ctx[+0x880]` (`0x801D0AC4..0x801D0B08`). The port's Cross-plus-cursor is an
/// ergonomic divergence; converting it wants the prompt's chrome moved with it,
/// which lives in `engine-ui`. See `docs/subsystems/battle.md`.
///
/// REF: FUN_801D0748 (state `0x1E`, `0x801D1038..0x801D10D4`)
fn step_round_prompt(cursor: u8, ev: BattleCommandInput, no_escape: bool) -> CommandPhase {
    let len = RoundChoice::PROMPT.len() as u8;
    let mut cursor = cursor.min(len - 1);
    if ev.left || ev.right {
        cursor = (cursor + 1) % len;
    }
    let run_now = ev.circle && !no_escape;
    if run_now {
        return CommandPhase::RunAway;
    }
    if ev.cross {
        match RoundChoice::PROMPT[cursor as usize] {
            RoundChoice::Begin => {
                let ring = BattleCommand::MENU
                    .iter()
                    .position(|c| c.enabled())
                    .unwrap_or(0) as u8;
                return CommandPhase::Menu { cursor: ring };
            }
            RoundChoice::Run if !no_escape => return CommandPhase::RunAway,
            RoundChoice::Run => {}
        }
    }
    CommandPhase::RoundPrompt { cursor }
}

/// One frame of the **attack-mode prompt** (retail `0x78`). Left / Right walk
/// the pair; Cross takes the highlighted chip; Circle backs out to the ring
/// with the cursor on the `Attack` arm it came from.
fn step_attack_mode(
    cursor: u8,
    ev: BattleCommandInput,
    party_slot: u8,
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> CommandPhase {
    let len = AttackMode::PROMPT.len() as u8;
    let mut cursor = cursor.min(len - 1);
    if ev.left || ev.right {
        cursor = (cursor + 1) % len;
    }
    if ev.circle {
        return CommandPhase::Menu {
            cursor: menu_index(BattleCommand::Attack),
        };
    }
    if ev.cross {
        return match AttackMode::PROMPT[cursor as usize] {
            AttackMode::Command => CommandPhase::OpenArtsMenu,
            AttackMode::Auto => {
                open_target_picker(BattleCommand::Attack, party_slot, party, monsters)
            }
        };
    }
    CommandPhase::AttackMode { cursor }
}

/// Open `command`'s target cursor, folding the outcomes a picker can resolve
/// in its own constructor so no frame is spent on a cursor nothing can move.
fn open_target_picker(
    command: BattleCommand,
    party_slot: u8,
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> CommandPhase {
    let picker = TargetPickerSession::new(command.target_kind(), party_slot, party, monsters);
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
            PickerOutcome::Cancelled => CommandPhase::Menu {
                cursor: menu_index(command),
            },
        };
    }
    CommandPhase::Targeting { command, picker }
}

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
    } else if ev.left || ev.right {
        cursor = row_neighbour(cursor);
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
        // Spirit acts on the caster - no target cursor.
        if command == BattleCommand::Spirit {
            return CommandPhase::SpiritGuard;
        }
        // The Attack arm is the door to the attack-mode prompt, not a strike.
        if command == BattleCommand::Attack {
            return CommandPhase::AttackMode { cursor: 0 };
        }
        if command.enabled() {
            return open_target_picker(command, party_slot, party, monsters);
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

    fn press(dir: fn(&mut BattleCommandInput)) -> BattleCommandInput {
        let mut ev = BattleCommandInput::default();
        dir(&mut ev);
        ev
    }

    /// Walk the ring cursor onto `command` with Down presses and hand back the
    /// session sitting on it.
    fn ring_on(command: BattleCommand) -> BattleCommandSession {
        let mut s = BattleCommandSession::new(0, 0);
        for _ in 0..menu_index(command) {
            s.input(press(|e| e.down = true), party3(), one_monster());
        }
        assert_eq!(s.menu_command(), Some(command));
        s
    }

    // ------------------------------------------------------- the round prompt

    /// A round-open session starts on the prompt, not on the ring - retail's
    /// `0x14` sets `ctx[+0x06] = 0x1E` before any member picks.
    #[test]
    fn a_round_open_session_starts_on_the_begin_run_prompt() {
        let s = BattleCommandSession::new_round_open(0, 0, false);
        assert_eq!(s.round_choice(), Some(RoundChoice::Begin));
        assert_eq!(s.menu_command(), None);
        assert!(s.resolved().is_none());
        // The mid-round entry skips it, which is where a submenu back-out and
        // every later member of the same round land.
        assert_eq!(BattleCommandSession::new(0, 0).round_choice(), None);
    }

    /// Left / Right walk the pair and Cross on `Begin` falls through to the
    /// ring - which is the only way into it.
    #[test]
    fn begin_falls_through_to_the_command_ring() {
        let mut s = BattleCommandSession::new_round_open(0, 0, false);
        s.input(press(|e| e.right = true), party3(), one_monster());
        assert_eq!(s.round_choice(), Some(RoundChoice::Run));
        s.input(press(|e| e.left = true), party3(), one_monster());
        assert_eq!(s.round_choice(), Some(RoundChoice::Begin));
        s.input(press_cross(), party3(), one_monster());
        assert_eq!(s.menu_command(), Some(BattleCommand::MENU[0]));
        assert!(s.resolved().is_none());
    }

    /// Both routes into `Run`: the highlighted chip under Cross, and Circle
    /// outright - retail's own `801d10bc` arm.
    #[test]
    fn run_resolves_from_the_prompt_by_cross_or_circle() {
        let mut by_cross = BattleCommandSession::new_round_open(0, 0, false);
        by_cross.input(press(|e| e.right = true), party3(), one_monster());
        by_cross.input(press_cross(), party3(), one_monster());
        assert_eq!(by_cross.resolved(), Some(Resolution::RunAway));

        let mut by_circle = BattleCommandSession::new_round_open(0, 0, false);
        by_circle.input(press(|e| e.circle = true), party3(), one_monster());
        assert_eq!(by_circle.resolved(), Some(Resolution::RunAway));
    }

    /// A scripted no-escape battle refuses both routes and leaves the prompt
    /// up, so Circle can't quietly flee a boss fight.
    #[test]
    fn a_no_escape_battle_refuses_run_from_either_route() {
        for circle in [false, true] {
            let mut s = BattleCommandSession::new_round_open(0, 0, true);
            if circle {
                s.input(press(|e| e.circle = true), party3(), one_monster());
            } else {
                s.input(press(|e| e.right = true), party3(), one_monster());
                s.input(press_cross(), party3(), one_monster());
            }
            assert!(s.resolved().is_none(), "circle={circle}");
            assert!(s.round_choice().is_some(), "circle={circle}");
        }
        // ... and the chip still draws, carrying the `-` the UI layer puts
        // there for an unavailable command.
        assert!(!BattleCommand::Run.available(true));
        assert!(BattleCommand::Run.available(false));
    }

    // --------------------------------------------------------- the ring shape

    /// The ring is retail's four arms in seat order, and `Run` / `Arts` are
    /// not among them - they live one level up and one level down.
    #[test]
    fn the_ring_is_the_four_retail_arms_in_seat_order() {
        assert_eq!(
            BattleCommand::MENU,
            [
                BattleCommand::Item,
                BattleCommand::Attack,
                BattleCommand::Magic,
                BattleCommand::Spirit,
            ]
        );
        assert!(!BattleCommand::MENU.contains(&BattleCommand::Run));
        assert!(!BattleCommand::MENU.contains(&BattleCommand::Arts));
        assert!(BattleCommand::MENU.iter().all(|c| c.enabled()));
    }

    #[test]
    fn opens_on_first_enabled_command() {
        let s = BattleCommandSession::new(0, 0);
        assert_eq!(s.menu_command(), Some(BattleCommand::Item));
        assert!(s.resolved().is_none());
    }

    /// Left / Right toggle within the drawn row: `Attack` and the magic arm
    /// flank the diamond, while `Item` and `Spirit` are alone on the vertical
    /// arms and stay put.
    #[test]
    fn left_right_step_along_the_drawn_row() {
        let seat = |from: u8, right: bool| {
            let mut s = BattleCommandSession::new(0, 0);
            s.phase = CommandPhase::Menu { cursor: from };
            s.input(
                BattleCommandInput {
                    left: !right,
                    right,
                    ..Default::default()
                },
                party3(),
                one_monster(),
            );
            s.menu_command()
        };
        assert_eq!(seat(1, true), Some(BattleCommand::Magic));
        assert_eq!(seat(2, false), Some(BattleCommand::Attack));
        assert_eq!(seat(0, true), Some(BattleCommand::Item));
        assert_eq!(seat(3, false), Some(BattleCommand::Spirit));
    }

    /// Up / Down keep the linear walk, so every arm stays reachable from
    /// every arm on a host with no face-button diamond.
    #[test]
    fn up_down_still_reach_every_command() {
        let mut s = BattleCommandSession::new(0, 0);
        let mut seen = vec![s.menu_command().unwrap()];
        for _ in 1..BattleCommand::MENU.len() {
            s.input(press(|e| e.down = true), party3(), one_monster());
            seen.push(s.menu_command().unwrap());
        }
        assert_eq!(seen, BattleCommand::MENU.to_vec());
    }

    #[test]
    fn item_and_magic_and_spirit_resolve_straight_off_the_ring() {
        for (command, want) in [
            (BattleCommand::Item, Resolution::OpenItemMenu),
            (BattleCommand::Magic, Resolution::OpenSpellMenu),
            (BattleCommand::Spirit, Resolution::SpiritGuard),
        ] {
            let mut s = ring_on(command);
            s.input(press_cross(), party3(), one_monster());
            assert_eq!(s.resolved(), Some(want), "{command:?}");
        }
    }

    // -------------------------------------------------- the attack-mode prompt

    /// `Attack` is a door, not a strike: it opens the `Auto | Command` prompt.
    #[test]
    fn the_attack_arm_opens_the_attack_mode_prompt() {
        let mut s = ring_on(BattleCommand::Attack);
        s.input(press_cross(), party3(), one_monster());
        assert_eq!(s.attack_mode(), Some(AttackMode::Auto));
        assert!(s.resolved().is_none());
        // Circle backs out onto the arm it came from.
        s.input(press(|e| e.circle = true), party3(), one_monster());
        assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
    }

    /// `Command` is where the port's arts entry lives - retail's `0x50`.
    #[test]
    fn command_opens_the_arts_entry() {
        let mut s = ring_on(BattleCommand::Attack);
        s.input(press_cross(), party3(), one_monster());
        s.input(press(|e| e.right = true), party3(), one_monster());
        assert_eq!(s.attack_mode(), Some(AttackMode::Command));
        s.input(press_cross(), party3(), one_monster());
        assert_eq!(s.resolved(), Some(Resolution::OpenArtsMenu));
    }

    /// `Auto` is the plain strike: it opens the target cursor, and a second
    /// Cross commits - retail's `0x5A`.
    #[test]
    fn auto_opens_the_target_cursor_then_confirms() {
        let mut s = ring_on(BattleCommand::Attack);
        s.input(press_cross(), party3(), one_monster());
        s.input(press_cross(), party3(), one_monster());
        assert!(matches!(s.phase, CommandPhase::Targeting { .. }));
        assert!(s.resolved().is_none());
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

    /// The whole chain end to end, from the round prompt to a committed
    /// strike - the flow the player actually walks.
    #[test]
    fn the_open_flow_runs_prompt_then_ring_then_mode_then_target() {
        let mut monsters = one_monster();
        monsters[1] = alive(true);
        monsters[2] = alive(true);
        let mut s = BattleCommandSession::new_round_open(0, 0, false);
        // Begin.
        s.input(press_cross(), party3(), monsters);
        // Ring: Item -> Attack.
        s.input(press(|e| e.down = true), party3(), monsters);
        assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
        // Attack -> the mode prompt -> Auto.
        s.input(press_cross(), party3(), monsters);
        assert_eq!(s.attack_mode(), Some(AttackMode::Auto));
        s.input(press_cross(), party3(), monsters);
        // Target cursor: walk two right, confirm.
        s.input(press(|e| e.right = true), party3(), monsters);
        s.input(press(|e| e.right = true), party3(), monsters);
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
    fn circle_in_targeting_returns_to_the_ring() {
        let mut monsters = one_monster();
        monsters[1] = alive(true);
        let mut s = ring_on(BattleCommand::Attack);
        s.input(press_cross(), party3(), monsters);
        s.input(press_cross(), party3(), monsters);
        assert!(matches!(s.phase, CommandPhase::Targeting { .. }));
        s.input(press(|e| e.circle = true), party3(), monsters);
        assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
        assert!(s.resolved().is_none());
    }

    #[test]
    fn no_living_target_aborts() {
        let dead_monsters = [
            SlotState::alive(true, false),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ];
        let mut s = ring_on(BattleCommand::Attack);
        s.input(press_cross(), party3(), dead_monsters);
        s.input(press_cross(), party3(), dead_monsters);
        assert_eq!(s.resolved(), Some(Resolution::Aborted));
    }
}
