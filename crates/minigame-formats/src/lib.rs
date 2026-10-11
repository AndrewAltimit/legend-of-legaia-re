//! The minigames' disc data: the fishing, slot-machine, dance, Baka Fighter
//! and Muscle Dome tables resident in their overlays, the art packs and the
//! slot machine's 3D scene, the runtime cue bank and the ringside stills.
//!
//! The rules engines that consume these live in `legaia-engine-minigames` /
//! `legaia-engine-fishing`. Sits on `legaia-container-formats` and names no
//! scene, UI or battle-model format.

#![forbid(unsafe_code)]

pub mod baka_opponents;
pub mod dance_art;
pub mod dance_cast;
pub mod dance_chart;
pub mod fishing_captions;
pub mod fishing_exchange;
pub mod fishing_species;
pub mod fishing_sprites;
pub mod minigame_art;
pub mod minigame_sfx;
pub mod minigame_slot_scene;
pub mod muscle_dome;
pub mod ringside_still;
pub mod slot_payout;
