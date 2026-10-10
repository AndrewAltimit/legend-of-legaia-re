//! Fishing-overlay (PROT 0972) **actor-side** kernels: the free-swim wander,
//! the pre-hook tick's camera seeding and debug readout, the bite roll and
//! its interval ladder, the catch-celebration tiers, and the overlay's own
//! 3-D segment clip + projection.
//!
//! Every routine here is an entry of the *fishing* overlay, confirmed by
//! disassembling PROT 0972 at slot-A base `0x801CE818`
//! (`scripts/ghidra-analysis/locate-entry-image.py` frames each one in 0972
//! and in no other based image). The dumps also exist under an
//! `overlay_debug_menu_` prefix; that prefix names the **capture**, whose
//! slot A held these bytes above PROT 0971's much shorter footprint - it is
//! not a claim that the code is dev-menu code. See
//! `docs/tooling/dump-corpus-integrity.md`.
//!
//! Companion prose: `docs/subsystems/minigame-fishing.md`.
//!
//! ## Wiring status is per item, not per module
//!
//! This file carried a blanket `# NOT WIRED` heading, and it stopped being
//! true: the bite-roll trio ([`bite_interval`], [`bite_credit_override`],
//! [`roll_hit_type`]) is now on the live fight path through
//! [`crate::fishing::BandCheck::tick`]. A module blanket is read
//! unconditionally by every anchor in the file, so one wired item makes it
//! assert something false about that item and it cannot be narrowed in place.
//! Each item carries its own status instead: the one routine no host owes a
//! port is tagged `REPLACED-BY:` where it is defined.
//!
//! `crate::fishing` models the minigame as *rules* (cast power, reel
//! tug-of-war, catch scoring); the actor-side kernels drive the retail
//! overlay's per-frame actor structs (`+0x14/+0x16/+0x18` position, `+0x22`
//! phase, `+0x26` facing) and the scene camera globals. [`FishWander`] and
//! [`LineActorSim`] carry those actors as advancing objects, hosted by the
//! play window's fishing frame (`window/minigames.rs`); the items still
//! inert (the line-draw pair, the walk-grid probes) name their own remaining
//! blocker in place.

mod bite;
mod line_actor;
mod lure;
mod pre_hook;
mod rod;
mod segment_clip;
mod tracked_points;
mod walk_grid_probe;
mod wander;

pub use bite::*;
pub use line_actor::*;
pub use lure::*;
pub use pre_hook::*;
pub use rod::*;
pub use segment_clip::*;
pub use tracked_points::*;
pub use walk_grid_probe::*;
pub use wander::*;

#[cfg(test)]
mod tests;
