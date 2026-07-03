//! High-level battle-session orchestrator.
//!
//! [`BattleSession`] composes [`BattleRunner`] (input → action queue),
//! [`BattleRound`] (per-round AP / stat refresh, status tick, death count),
//! [`BattleHud`] (renderer-agnostic UI model), and the action SM driver into
//! a single state machine.
//!
//! Engines drive a battle frame as:
//!
//! ```ignore
//! let mut session = BattleSession::new();
//! session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
//! session.set_monsters(monsters);
//! session.begin_round(&mut world, &per_slot, &equipment, &modifiers);
//!
//! // Per-frame:
//! let events = session.tick(&mut world, input);
//! for event in events {
//!     match event {
//!         SessionEvent::DamageApplied { slot, amount } => { ... }
//!         SessionEvent::TurnCommitted => { ... }
//!         SessionEvent::BattleEnded { cause } => break,
//!         _ => {}
//!     }
//! }
//! ```
//!
//! The session is renderer-agnostic - engines render `session.hud` via
//! [`legaia_engine_render::battle_hud_draws_for`].
//!
//! This module is split into cohesive submodules (Rust 2018 style - the file
//! stays at `battle_session.rs` with children under `battle_session/`); every
//! public item is re-exported here so external paths
//! (`legaia_engine_core::battle_session::<Item>`) keep resolving unchanged.

use crate::battle_hud::{BattleHud, LogAccent, SlotSyncInfo};
use crate::battle_round::BattleRound;
use crate::battle_runner::{BattleRunner, BattleRunnerError, BattleRunnerState};
use crate::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
use crate::world::World;
use legaia_art::{ActionConstant, Character, Command};
use legaia_engine_vm::battle_action::{
    ActionCategory, ActionState, ActorFlags, BattleEndCause, StepOutcome,
};
use legaia_engine_vm::battle_formulas::{accuracy_roll, psyq_rand_step};
use legaia_engine_vm::status_effects::StatusKind;

mod command_input;
mod events;
mod lifecycle;
mod resolve;
mod target;
mod types;

pub use types::*;

#[cfg(test)]
mod tests;

/// Battle session orchestrator.
///
/// One instance per active battle; engines reset by constructing a fresh
/// session at battle entry.
#[derive(Debug, Clone)]
pub struct BattleSession {
    /// Current phase.
    phase: BattlePhase,
    /// Frames spent in the current phase. Reset on every `phase` transition.
    /// Used to drive auto-advance (e.g. `RoundIntro` lasts `intro_frames`).
    phase_frames: u64,
    /// Frames the `RoundIntro` splash holds. Default 60 (1 sec at 60 FPS).
    intro_frames: u64,
    /// Frames the `RoundOutro` status-tick splash holds. Default 30.
    outro_frames: u64,
    /// Number of completed turns. Incremented on the `Resolve → RoundOutro`
    /// transition so engines can stripe the log column.
    pub turn: u32,
    /// Wrapped command runner. Public so engines can read the resolved
    /// queues post-commit.
    pub runner: BattleRunner,
    /// HUD model. Engines render off this each frame.
    pub hud: BattleHud,
    /// Last `BattleRound` returned by `begin_round`. `None` between rounds.
    pub round: Option<BattleRound>,
    /// Per-slot session-level metadata (name, party flag, MP cap, stat
    /// record). Synced to the HUD on `begin_round`.
    slots: [SessionSlotInfo; 8],
    /// Per-slot stat-record snapshot - matches `slots[i].record` but keyed
    /// for `BattleRound::begin`'s array shape.
    per_slot_records: [Option<StatRecord>; 8],
    /// Equipment table - engines populate before `begin_round` so the
    /// stat-aggregator can sum per-item modifiers.
    pub equipment: EquipmentTable,
    /// Status modifiers (Toxic -ATK, Confuse -accuracy, etc.).
    pub modifiers: StatusModifiers,
    /// Number of monster slots in the current battle (3..=5).
    monster_count: u8,
    /// Currently-active target picker (if any). When `Some`, the session
    /// is in a `CommandInput` sub-phase ([`SubPhase::TargetPick`]):
    /// directional / cross / circle input goes to the picker instead of
    /// the runner. The picker is opened by [`Self::open_target_picker`]
    /// (or by [`Self::push_command_with_target`]) and closed when its
    /// outcome resolves (Confirmed → command admitted; Cancelled →
    /// pending command popped).
    target_picker: Option<crate::target_picker::TargetPickerSession>,
    /// Pending command waiting for a target. When the picker confirms,
    /// the command is pushed via [`Self::push_command`].
    pending_target_command: Option<Command>,
    /// Driver state for the [`BattlePhase::Resolve`] phase. `Some` once
    /// commit has installed the resolved per-slot queues and the session
    /// owns the action SM until every party slot's swing has finished.
    resolve_driver: Option<ResolveDriver>,
    /// Deterministic RNG seed for the in-session formula rolls
    /// (accuracy + variance). Engines that want reproducible test
    /// playthroughs pin this before [`Self::begin_round`].
    pub rng_seed: u32,
}

/// Internal driver state used while the session is in [`BattlePhase::Resolve`].
///
/// One entry per party slot that has buffered a non-empty resolved queue,
/// in slot order (0 → 1 → 2). The session arms the action SM for each
/// attacker in turn, advances `world.tick()` until it lands in
/// `EndOfAction`, then pops the head and re-arms for the next slot. When
/// `pos >= queue.len()` the session transitions to `RoundOutro`.
#[derive(Debug, Clone)]
struct ResolveDriver {
    queue: Vec<ResolveSlot>,
    pos: usize,
    armed: bool,
}

/// One entry in [`ResolveDriver::queue`] - a party slot the action SM still
/// needs to drive this round.
#[derive(Debug, Clone, Copy)]
struct ResolveSlot {
    /// Party slot index (0..=2). Maps directly to `world.actors` /
    /// `BattleSession::slots`.
    slot: u8,
    /// Action category byte to push into `world.battle_ctx.queued_action`
    /// when the SM is armed for this slot. Mirrors the retail
    /// `actor.action_category` byte at `+0x1DE`.
    category: u8,
}

/// One-frame input → first directional command, if any. Order matches the
/// retail input prio: Right > Left > Down > Up. Engines that want explicit
/// per-direction event semantics should bypass this and call
/// [`BattleSession::push_command`] directly.
fn input_to_command(input: SessionInput) -> Option<Command> {
    if input.right {
        Some(Command::Right)
    } else if input.left {
        Some(Command::Left)
    } else if input.down {
        Some(Command::Down)
    } else if input.up {
        Some(Command::Up)
    } else {
        None
    }
}
