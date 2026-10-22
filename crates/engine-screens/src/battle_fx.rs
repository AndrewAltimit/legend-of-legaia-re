//! The in-battle screen-FX layers both play hosts draw, placed and projected
//! once.
//!
//! Three retail draws ride the battle camera into screen space as PSX
//! primitives: the weapon-trail bands (`FUN_8005112C` / `FUN_80048310` ->
//! `FUN_800485BC`), the move-FX afterimage streak and its ribbon
//! (`FUN_801E09F8` -> `FUN_801E1AB0` / `FUN_801E1D98`, plus the clip-tag
//! ribbon from `FUN_8004CE2C`), and PROT 0904's Theeder beam. Their packets
//! were already shared kernels; what each host kept was the camera they
//! project through, and the two had drifted: the native window projected the
//! streak without the battle world scale and drew it as an opaque scene mesh
//! with the packet's semi-transparency and OT bucket dropped, while the page
//! drew the same packets as screen primitives. Every surface now takes the
//! layer from here and contributes nothing but the list.
//!
//! The camera is the battle mode's (`World::battle_cam_pose` through
//! [`battle_vp`]) at the stage's 4:3 - the projection lands on the retail
//! 320x240 stage, whatever the surface's own aspect - with the battle world
//! scale ([`BATTLE_WORLD_SCALE`], retail's base matrix `0x8007BF10`) folded in
//! as the bodies the effects ride are drawn.

use legaia_engine_camera::projection::{BATTLE_WORLD_SCALE, battle_vp};
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The aspect every battle screen-FX layer projects at: the retail stage's
/// 4:3, which is what the screen-prim pass rasterises into.
pub const BATTLE_FX_ASPECT: f32 = 4.0 / 3.0;

/// The battle view-projection the screen-FX layers project through: the
/// phase-scripted battle camera at the stage's 4:3, the focus lifted by the
/// battle world scale (`legaia_engine_camera::projection::battle_vp`). The
/// native window's glam composition (`battle_scene_mvp`) is pinned equal to
/// it by `battle_vp_matches_the_native_glam_composition`.
pub fn battle_fx_vp(world: &World) -> [f32; 16] {
    battle_vp(
        &world.battle_cam_pose(),
        BATTLE_WORLD_SCALE,
        BATTLE_FX_ASPECT,
    )
}

/// This frame's weapon-trail bands, then the move-FX afterimage streak and
/// the clip-tag ribbon, as stage primitives. Empty outside battle.
///
/// The trail's placement is `screen_passes::weapon_trail_prims`; the streak
/// and ribbon packets are `screen_passes::move_fx_streak_quads`, linked at
/// `streak_pass::MOVE_FX_STREAK_OT` with the packet's own semi-transparency
/// (retail sends prim code `0x2E`).
// REF: FUN_800485BC, FUN_801E1AB0, FUN_801E1D98 - the packet ports live in
// render-kernels (`battle_trail`, `afterimage`, `streak_pass`).
pub fn battle_fx_prims(world: &World) -> Vec<ScreenPrim> {
    use legaia_engine_world_views::screen_passes::{move_fx_streak_quads, weapon_trail_prims};
    use legaia_render_kernels::streak_pass::MOVE_FX_STREAK_OT;
    if world.mode != SceneMode::Battle {
        return Vec::new();
    }
    let vp = battle_fx_vp(world);
    let mut out = weapon_trail_prims(world, &vp, BATTLE_WORLD_SCALE);
    let mvp = glam::Mat4::from_cols_array(&vp)
        * glam::Mat4::from_scale(glam::Vec3::splat(BATTLE_WORLD_SCALE));
    out.extend(
        move_fx_streak_quads(world, &mvp)
            .iter()
            .map(|q| q.to_screen_prim(MOVE_FX_STREAK_OT)),
    );
    out
}

/// PROT 0904's (Theeder, spell `0x82`) beam packets this frame
/// (`World::theeder_draw` through `engine-ui::cast_theeder::theeder_prims`),
/// each point placed and projected as a battle body is
/// (`battle_trail::project_battle_point`: frame flip, world scale, the
/// battle camera) - the weapon trail's projection. Empty when no beam is
/// live, which `World::theeder_draw` decides (battle only).
pub fn theeder_prims(world: &World) -> Vec<ScreenPrim> {
    let Some(packet) = world.theeder_draw() else {
        return Vec::new();
    };
    let vp = battle_fx_vp(world);
    legaia_engine_ui::cast_theeder::theeder_prims(&packet, world.theeder_trail(), |p| {
        legaia_render_kernels::battle_trail::project_battle_point(&vp, BATTLE_WORLD_SCALE, p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_is_empty_off_battle() {
        let world = World::default();
        assert!(battle_fx_prims(&world).is_empty());
        assert!(theeder_prims(&world).is_empty());
    }

    #[test]
    fn the_fx_vp_is_the_battle_cameras_at_the_stage_aspect() {
        let world = World::default();
        assert_eq!(
            battle_fx_vp(&world),
            battle_vp(&world.battle_cam_pose(), 4.0, 4.0 / 3.0)
        );
    }
}
