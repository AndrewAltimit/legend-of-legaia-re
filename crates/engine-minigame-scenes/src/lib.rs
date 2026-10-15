//! The minigames' **3D scene surfaces and venues**: the Baka Fighter duel
//! ([`baka_duel_scene`]), the Muscle Dome arena ([`muscle_dome_scene`]), the
//! dance hall ([`dance_venue`], [`dance_cast_scene`]) and the fishing venue
//! ([`fishing_venue`], [`fishing_scene`]), plus the `World`'s minigame seat
//! ([`minigame_state::MinigameState`]).
//!
//! The duel and arena decode their assets through a `read_prot` closure; the
//! venues load a `Scene` through `legaia-engine-scene`. Each builds the
//! combined vertex buffers and poses them each frame from the rules engines'
//! state - no `World`, no renderer. `legaia-engine-core` depends on this
//! crate.

pub mod baka_duel_scene;
pub mod dance_cast_scene;
pub mod dance_hud_art;
pub mod dance_venue;
pub mod fishing_rod;
pub mod fishing_scene;
pub mod fishing_venue;
pub mod minigame_state;
pub mod minigame_status;
pub mod muscle_dome_scene;
