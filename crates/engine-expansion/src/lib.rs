//! The `World`-free runtime of an expansion pack: what a loaded
//! [`legaia_expansion_format::ExpansionPack`] does to battles, progression
//! and quests, as kernels over plain state. `engine-core` holds one
//! [`ExpansionState`] on the `World` (`World::expansion`) and calls these
//! kernels from its scene-entry, encounter and battle-spoils steps.
//!
//! With no pack loaded every kernel is the identity, so a world without a
//! pack plays exactly as before.

#![forbid(unsafe_code)]

use std::sync::Arc;

use legaia_expansion_format::ExpansionPack;

/// The `World`'s expansion seat.
#[derive(Debug, Clone, Default)]
pub struct ExpansionState {
    /// The active pack, if any.
    pub pack: Option<Arc<ExpansionPack>>,
}

impl ExpansionState {
    /// Whether a pack is active.
    pub fn is_active(&self) -> bool {
        self.pack.is_some()
    }
}
