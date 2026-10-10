//! The disc patcher's play-as-Delilas mod: the playable party replaced by the
//! Delilas siblings, end to end - battle and field models, arts reskin and
//! movesets, signature attacks, effects, voices and save portraits.
//!
//! The model transforms themselves are `legaia-party-swap`'s pure kernels;
//! this crate is the layer that writes them onto a disc and carries
//! everything a swapped hero needs beyond a mesh. It ships only *code*: every
//! byte it writes is derived from the user's own disc, and every test that
//! needs real game data is disc-gated in `legaia-patcher`.
//!
//! Every module names only its siblings and the crates below the patcher -
//! `legaia-party-swap`, `legaia-arts-patch`, `legaia-code-hooks`,
//! `legaia-texture-replace`, `legaia-translate`, `legaia-disc-patch` and the
//! parser crates - so the crate sits below `legaia-patcher`, which re-exports
//! each module at its old path. See the crate README for the module map and
//! the split line.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...`, which `legaia-patcher` re-exports
// at its root; binding them here keeps the moved files' paths unchanged.
use legaia_arts_patch::arts;
use legaia_code_hooks::{delilas_cast, mips};
use legaia_disc_patch::{disc, rng};
use legaia_party_swap::party_swap;
use legaia_texture_replace::save_icon;
use legaia_translate::translation;

pub mod delilas_effects;
pub mod delilas_party;
pub mod delilas_signature_attack;
pub mod delilas_voice;
pub mod delilas_voice_fx;
pub mod delilas_xa_voice;
pub mod enemy_anim_mirror;
pub mod nivora_field;
