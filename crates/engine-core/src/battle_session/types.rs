//! Session data model: the public phase / input / event enums plus the
//! per-slot info bundle. The [`BattleSession`] struct itself lives in the
//! parent coordinator so its inherent-impl submodules (descendants of the
//! parent) can reach its private fields.

use super::*;

/// Per-slot configuration the session needs to render the HUD and sync
/// `BattleRound::begin`.
#[derive(Debug, Clone, Default)]
pub struct SessionSlotInfo {
    /// Display name for the HUD (`"Vahn"`, `"Noa"`, `"Snake-Lizard"`).
    pub name: String,
    /// `true` for slots 0..=2 (the player's party), `false` for monsters.
    pub is_party: bool,
    /// Stat record consumed by [`BattleRound::begin`] when computing
    /// per-slot ATK / UDF / LDF / accuracy / evasion. `None` for empty
    /// monster slots.
    pub record: Option<StatRecord>,
    /// MP cap. Engines populate from the character record at battle init;
    /// the live MP is folded into the World by `set_character_max_mp`.
    pub mp_max: u16,
}

/// Phase of the battle session SM.
///
/// Engines render different UI per phase: `RoundIntro` shows the encounter
/// banner, `CommandInput` shows the command menu + AP gauges, `Resolve`
/// shows damage popups + the ringed log, `RoundOutro` runs status ticks
/// and shows tick-damage popups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BattlePhase {
    /// No active battle. The session was constructed or `end()` was called.
    #[default]
    Idle,
    /// Banner / encounter-name splash before the first command-input phase.
    /// Auto-advances after `phase_frames` reaches the splash duration.
    RoundIntro,
    /// Player picks commands for each party slot. Engines feed [`SessionInput`]
    /// per-frame; the session forwards admissible inputs to [`BattleRunner`].
    CommandInput,
    /// Player has committed; the SM consumes the resolved queue. Engines
    /// drain `World::pending_battle_events` as usual; the session folds the
    /// outcomes into the HUD popups + log automatically.
    Resolve,
    /// End-of-round bookkeeping: status tick, death count, tick-damage
    /// popups. Auto-advances back to `RoundIntro` if the battle isn't over.
    RoundOutro,
    /// Player wiped the monster pool. Terminal state.
    Victory,
    /// Monsters wiped the party. Terminal state.
    Defeat,
    /// Player chose Escape and the dice came up favourable. Terminal.
    Escaped,
}

/// Sub-phase of [`BattlePhase::CommandInput`].
///
/// During CommandInput the player either selects commands directly
/// ([`SubPhase::CommandSelect`]) or picks a target for a buffered
/// command ([`SubPhase::TargetPick`]). Engines query
/// [`BattleSession::sub_phase`] to render the right overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubPhase {
    /// Default: input drives the command queue + AP gauge.
    #[default]
    CommandSelect,
    /// A command was buffered; input drives the cursor in
    /// [`crate::target_picker::TargetPickerSession`]. The runner is paused
    /// until the picker resolves.
    TargetPick,
}

/// One-frame input bundle the session consumes.
///
/// Engines map raw keyboard / pad input to these. The session handles the
/// per-phase dispatch (CommandInput consumes directional commands; Resolve
/// only honours `pause` / `cancel`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionInput {
    /// D-pad up; in CommandInput, queues `Command::Up`.
    pub up: bool,
    /// D-pad down; in CommandInput, queues `Command::Down`.
    pub down: bool,
    /// D-pad left; in CommandInput, queues `Command::Left`.
    pub left: bool,
    /// D-pad right; in CommandInput, queues `Command::Right`.
    pub right: bool,
    /// Cross - confirm / advance phase.
    pub cross: bool,
    /// Circle - cancel / pop the last command from the buffer.
    pub circle: bool,
    /// Triangle - advance to the next party slot's command-input phase
    /// without committing yet.
    pub triangle: bool,
    /// Square - Spirit press (`ApGauge::charge_spirit`).
    pub square: bool,
    /// Start - commit the current turn (transitions to `Resolve`).
    pub start: bool,
}

/// Events the session emits per `tick()`.
///
/// Engines surface these for UI / sound. The session has *already* mutated
/// the HUD and the world by the time the event is handed back, so the
/// engine doesn't need to re-apply the gameplay-state side.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Phase moved from `from` to `to` this frame.
    PhaseChanged { from: BattlePhase, to: BattlePhase },
    /// A command was admitted for the active party slot.
    CommandPushed { slot: u8, command: Command },
    /// The most recent command was popped (Circle pressed in CommandInput).
    CommandPopped { slot: u8, command: Command },
    /// Turn committed - every party slot's queue has been resolved through
    /// `resolve_action_queue` and stashed on the runner.
    TurnCommitted,
    /// HP delta was applied to a slot's `BattleActor::hp`. The amount is
    /// always positive; `is_heal` distinguishes heals from damage.
    HpChanged {
        slot: u8,
        amount: u16,
        is_heal: bool,
    },
    /// A status effect was applied to a slot (folded from
    /// `BattleEvent::ApplyArtStrike`'s `enemy_effect` when present).
    StatusApplied { slot: u8, kind: StatusKind },
    /// The Spirit gauge for the active party slot received the +5 bonus.
    SpiritCharged { slot: u8 },
    /// Battle ended - the session transitioned to a terminal phase.
    BattleEnded { cause: BattleEndCause },
    /// Engine opened a target picker (engines render the cursor overlay
    /// against the picker state until the picker's outcome resolves).
    TargetPickerOpened {
        kind: crate::target_picker::TargetKind,
    },
    /// Target picker resolved with a confirmed slot.
    TargetConfirmed { target_slot: u8 },
    /// Target picker resolved with a sweep (all enemies / allies / self).
    TargetSweepConfirmed,
    /// Target picker was cancelled by the player; pending command was
    /// dropped without affecting the queue.
    TargetCancelled,
}
