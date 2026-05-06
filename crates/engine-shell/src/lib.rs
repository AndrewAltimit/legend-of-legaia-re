//! `legaia-engine-shell` — the top-level engine driver crate.
//!
//! The `legaia-engine` binary lives under `src/bin/legaia-engine.rs` and
//! is the canonical "single command" that turns a CDNAME scene name into
//! runtime engine state ([`legaia_engine_core::scene_resources::SceneResources`]).
//!
//! This lib is empty by design — the crate's only deliverable is the
//! binary. Engines that want to embed the same plumbing should depend on
//! `legaia-engine-core` directly.
