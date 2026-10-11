//! The disc patcher's play-as-Delilas mod: the playable party replaced by the
//! Delilas siblings, end to end - battle and field models, arts reskin and
//! movesets, signature attacks and their cast route, effects, voices and save
//! portraits.
//!
//! The model transforms themselves are `legaia-party-swap`'s pure kernels;
//! this crate is the layer that writes them onto a disc and carries
//! everything a swapped hero needs beyond a mesh. It ships only *code*: every
//! byte it writes is derived from the user's own disc, and every test that
//! needs real game data is disc-gated in `legaia-patcher`.
//!
//! It names `legaia-party-swap`, `legaia-arts-patch`, `legaia-code-hooks`,
//! `legaia-texture-replace`, `legaia-translate`, `legaia-disc-patch` and the
//! parser crates. The Delilas Challenge dome course is a different feature
//! and lives in `legaia-delilas-challenge`. See the crate README for the
//! module map.

#![forbid(unsafe_code)]

pub mod delilas_cast;
pub mod delilas_effects;
pub mod delilas_party;
pub mod delilas_signature_attack;
pub mod delilas_voice;
pub mod delilas_voice_fx;
pub mod delilas_xa_voice;
pub mod enemy_anim_mirror;
pub mod nivora_field;
