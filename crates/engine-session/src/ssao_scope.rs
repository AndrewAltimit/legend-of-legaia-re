//! Which frames take the ambient-occlusion enhancement: one predicate both
//! play hosts ask before staging the SSAO camera (the native window's
//! `Renderer::set_ssao_camera`, the page's `play_ssao_frame`).
//!
//! The pass is built for the frames that draw through the scene camera with
//! an environment / actor split (`Renderer::set_occlusion_env_draws` natively,
//! `noOccl` placements on the page): the field, the world map and a battle.
//! A minigame venue owns the 3D frame instead - the Baka duel, the Muscle
//! Dome and the fishing pond under a camera of their own, the dance hall as
//! one baked mesh with its dancers in it, the slot machine as screen
//! primitives - and marks no actor draws, so the pass would shade the
//! fighters and dancers as walls. The page never ran the pass there; the
//! window did, which is the difference `host-pair.mjs` measured over the
//! dance, Baka and fishing scenes.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;

/// `true` when `mode`'s frame takes the SSAO pass (off for the minigame
/// venues; see the module docs).
pub fn ssao_applies_in(mode: SceneMode) -> bool {
    !matches!(
        mode,
        SceneMode::Dance
            | SceneMode::Fishing
            | SceneMode::SlotMachine
            | SceneMode::BakaFighter
            | SceneMode::MuscleDome
    )
}

/// [`ssao_applies_in`] for the world's current mode.
pub fn ssao_applies(world: &World) -> bool {
    ssao_applies_in(world.mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_camera_frames_take_the_pass_and_the_venues_do_not() {
        for m in [
            SceneMode::Field,
            SceneMode::Battle,
            SceneMode::WorldMap,
            SceneMode::Cutscene,
        ] {
            assert!(ssao_applies_in(m), "{}", m.name());
        }
        for m in [
            SceneMode::Dance,
            SceneMode::Fishing,
            SceneMode::SlotMachine,
            SceneMode::BakaFighter,
            SceneMode::MuscleDome,
        ] {
            assert!(!ssao_applies_in(m), "{}", m.name());
        }
    }
}
