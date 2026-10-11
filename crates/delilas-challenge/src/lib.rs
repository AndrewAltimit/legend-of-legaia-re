//! The disc patcher's Delilas Challenge: a fourth Muscle Dome enrollment
//! option that runs a new two-round arena course against the Delilas
//! siblings, and the three custom items that can be its full-clear reward
//! (and that stand alone as `--custom-items`).
//!
//! This is a separate feature from the play-as-Delilas party swap
//! (`legaia-delilas-party`): the two are mutually exclusive on a disc and
//! share no code. It ships only *code* - the script bytes and routines are
//! the patcher's own, planned against image bytes handed in as slices; the
//! disc writes are sequenced by `legaia_patcher::apply`, and every test that
//! needs real game data is disc-gated in `legaia-patcher`.
//!
//! - [`enrollment`] - the `koin1` script half: the menu option, the gate on
//!   the Koru event and the warp into the arena.
//! - [`dome`] - the code half: the course seed hook, the slim sibling clones,
//!   the battle-rule edits and the coin payout.
//! - [`custom_items`] - Nature's Elixir, the Ra-Seru Tear and the Fury Bloom:
//!   item records, effect machinery, battle hooks and the reward grant.

#![forbid(unsafe_code)]

pub mod custom_items;
pub mod dome;
pub mod enrollment;
