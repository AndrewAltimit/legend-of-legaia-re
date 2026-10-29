//! The `World`-free runtime of an expansion pack: what a loaded
//! [`legaia_expansion_format::ExpansionPack`] does to battles, progression
//! and quests, as kernels over plain state. `engine-core` holds one
//! [`ExpansionState`] on the `World` (`World::expansion`) and calls these
//! kernels from its scene-entry, encounter and battle-spoils steps.
//!
//! With no pack loaded every kernel is the identity, so a world without a
//! pack plays exactly as before.

#![forbid(unsafe_code)]

pub mod battle;
pub mod progression;
pub mod dialog;
pub mod flags;
pub mod quest;
pub mod quest_challenge;
pub mod quest_save;

use std::sync::Arc;

use legaia_expansion_format::ExpansionPack;

/// The `World`'s expansion seat.
#[derive(Debug, Clone, Default)]
pub struct ExpansionState {
    /// The active pack, if any.
    pub pack: Option<Arc<ExpansionPack>>,
    /// The pack's compiled battle tables and per-fight state.
    pub battle: battle::BattleRuntime,
    /// The pack's progression state.
    pub progression: progression::ProgressionRuntime,
    /// Quest, challenge, pack-flag and pack-dialogue state
    /// ([`quest::QuestRuntime`]); what the save's expansion block carries.
    pub quests: quest::QuestRuntime,
}

impl ExpansionState {
    /// Whether a pack is active.
    pub fn is_active(&self) -> bool {
        self.pack.is_some()
    }
}
