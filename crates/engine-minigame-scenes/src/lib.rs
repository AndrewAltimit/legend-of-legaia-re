//! The minigames' **3D scene surfaces**: the Baka Fighter duel
//! ([`baka_duel_scene`]) and the Muscle Dome arena ([`muscle_dome_scene`]).
//!
//! Each surface decodes its assets through a `read_prot` closure, builds the
//! combined vertex buffers and poses them each frame from the rules engines'
//! state - no `World`, no `Scene`, no renderer. `legaia-engine-core` depends on
//! this crate.

pub mod baka_duel_scene;
pub mod muscle_dome_scene;
