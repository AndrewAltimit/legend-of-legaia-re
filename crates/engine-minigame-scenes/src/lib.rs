//! The minigames' **3D scene surfaces**: the Baka Fighter duel
//! ([`baka_duel_scene`]) and the Muscle Dome arena ([`muscle_dome_scene`]).
//!
//! Each surface decodes its assets through a `read_prot` closure, builds the
//! combined vertex buffers and poses them each frame from the rules engines'
//! state - no `World`, no `Scene`, no renderer. `legaia-engine-core` depends on
//! this crate and re-exports both modules at their old paths
//! (`legaia_engine_core::baka_duel_scene`, `legaia_engine_core::muscle_dome_scene`).

// The sibling kernels the surfaces name as `crate::<module>`, aliased from the
// World-free crates that own them.
pub(crate) use legaia_engine_battle::battle_seats;
pub(crate) use legaia_engine_field::packet_color;
pub(crate) use legaia_engine_menus::{battle_input, muscle_dome};
pub(crate) use legaia_engine_minigames::{
    baka_cabinet, baka_fighter, baka_fighter_chrome, baka_impact_fx,
};
pub(crate) use legaia_engine_system::mode_entry_init;

pub mod baka_duel_scene;
pub mod muscle_dome_scene;
