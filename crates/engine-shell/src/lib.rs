//! `legaia-engine-shell` - the top-level engine driver crate.
//!
//! Houses the `legaia-engine` binary and the native play window over the
//! shared game session (`legaia_engine_session::native::NativeSession`).
//!
//! The parity oracles and the retail comparison corpus live in
//! `legaia-parity`, which the binary's trace subcommands drive.

pub mod cutscene_av;
pub mod host_setup;
pub mod launcher;
pub mod replay;
pub mod scenarios;
pub mod window;
pub mod xa_clip;
