//! Disc readers for kernels that take parsed tables rather than a disc.
//!
//! Each child is named for the kernel module it feeds: it pulls one PROT
//! entry (or one loaded [`legaia_engine_scene::scene::Scene`]) through the
//! format parser and hands the kernel the result. They sit here because the
//! kernel crates do not depend on `engine-scene`.

pub mod battle_open;
pub mod battle_tutorial;
pub mod dance;
pub mod encounter_man;
pub mod fishing_actors;
