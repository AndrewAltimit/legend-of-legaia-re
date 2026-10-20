//! The live effect layer's draw lists, read off a running `World` once for
//! both play hosts: the effect-pool billboards, the `etmd` effect models, the
//! move-VM scene-graph parts (summon, battle move-FX, field stagers), the
//! draw-kind-4 nodes and the battle ground shadows, each with its placement
//! composed.
//!
//! The native window and the browser page each carried this selection and
//! composition (about 250 lines apiece: `build_effect_billboards`,
//! `build_effect_model_draws`, `build_summon_and_move_fx_part_draws`,
//! `build_field_fx_part_draws` against `build_battle_fx` / `build_field_fx`),
//! one in glam, one in hand-rolled `[f32; 16]` math. What stays with each host
//! is the renderer: resolving a mesh source to an upload (or to the page's
//! model list) and the outer frame (the page's 4x world scale in battle and
//! its page-space Y flip on the field).
//!
//! # Frames
//!
//! Every matrix here is in the frame the **native** draw consumes:
//!
//! - **battle** - the Y-up actor frame the battle camera reads (its trailing
//!   flip cancels the per-model one), **before** the 4x world-scale base the
//!   camera composes ahead of the projection. Models carry the per-model Y
//!   flip (`WORLD_FLIP`) as their last factor; a part's placement takes the
//!   frame flip on its translation and its camera-relative prefix
//!   (`gte::part_model_place`), the rotation its conjugate
//!   (`gte::part_rotation(rot, true)`). A billboard centre is flipped the same
//!   way (`effect_billboard::battle_billboard_centre`), since a billboard has
//!   no model matrix to carry it.
//! - **field and overworld** - the raw retail Y-down frame the field cameras
//!   consume (they compose the world flip themselves): no per-model flip.
//!
//! The page multiplies its own outer frame on the left of each model: the 4x
//! world scale in battle (it projects through the unscaled battle VP), the
//! page-space `diag(1, -1, 1)` on the field.

use legaia_engine_camera::psx_camera::{WORLD_FLIP, mat4_mul, mat4_translation};
use legaia_engine_core::world::{BattleActorDrawPlan, EffectSprite, World};
use legaia_engine_effects::summon::SummonPartDraw;
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::gte::{
    PartCameraPose, camera_relative_model_prefix, part_model_place, part_rotation,
};

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// One effect-pool child as a camera-facing quad (retail `FUN_801E0080`
/// pass 2, one GP0 `0x2E` semi-transparent quad per child).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxBillboard {
    /// The child, its `world_pos` already in the draw frame (flipped in
    /// battle, raw on the field).
    pub sprite: EffectSprite,
    /// View-space half-extents (`effect_billboard::world_half_extents`).
    pub half: (f32, f32),
    /// Texel corners, TL / TR / BL / BR, with the child's random UV mirror
    /// applied.
    pub uv: [[u8; 2]; 4],
    /// `[clut, tsb]` with the prim-ABE enable in bit 15
    /// (`EffectSprite::packet_tsb`): every child is semi-transparent.
    pub cba_tsb: [u16; 2],
}

impl FxBillboard {
    /// The quad's corners around `centre` (TL / TR / BL / BR) on the drawing
    /// camera's world-space `right` / `up` basis.
    pub fn corners_at(&self, centre: [f32; 3], right: [f32; 3], up: [f32; 3]) -> [[f32; 3]; 4] {
        let (hw, hh) = self.half;
        let mut out = [[0.0f32; 3]; 4];
        for (i, (sr, su)) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
            .into_iter()
            .enumerate()
        {
            for k in 0..3 {
                out[i][k] = centre[k] + right[k] * hw * sr + up[k] * hh * su;
            }
        }
        out
    }

    /// [`Self::corners_at`] around the billboard's own centre.
    pub fn corners(&self, right: [f32; 3], up: [f32; 3]) -> [[f32; 3]; 4] {
        self.corners_at(self.sprite.world_pos, right, up)
    }
}

/// The texel corners of a pool child, TL / TR / BL / BR: the atlas rect with
/// the child's random UV-mirror (retail pass 2) swapping the base column /
/// row.
pub fn sprite_corner_uvs(s: &EffectSprite) -> [[u8; 2]; 4] {
    let [u0, v0] = s.uv;
    let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1)).min(255) as u8;
    let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1)).min(255) as u8;
    let (mut u0, mut u1) = ((u0 & 0xFF) as u8, u1);
    let (mut v0, mut v1) = ((v0 & 0xFF) as u8, v1);
    if s.flip_h {
        std::mem::swap(&mut u0, &mut u1);
    }
    if s.flip_v {
        std::mem::swap(&mut v0, &mut v1);
    }
    [[u0, v0], [u1, v0], [u0, v1], [u1, v1]]
}

/// The effect-pool billboards this frame owes. `view_scale` is the uniform
/// scale the drawing camera composes ahead of the projection (the 4x battle
/// base, `1.0` elsewhere), which the half-extents must not take twice.
///
/// Empty on the overworld: retail's pool walker has one caller on the disc,
/// the battle draw tick `FUN_800480D8`, and the field-run overworld never
/// reaches it.
pub fn effect_billboards(world: &World, view_scale: f32) -> Vec<FxBillboard> {
    if world.mode == SceneMode::WorldMap {
        return Vec::new();
    }
    let battle = world.mode == SceneMode::Battle;
    world
        .active_effect_sprites()
        .into_iter()
        .map(|mut sprite| {
            if battle {
                sprite.world_pos = legaia_render_kernels::effect_billboard::battle_billboard_centre(
                    sprite.world_pos,
                );
            }
            FxBillboard {
                half: legaia_render_kernels::effect_billboard::world_half_extents(
                    sprite.size,
                    view_scale,
                ),
                uv: sprite_corner_uvs(&sprite),
                cba_tsb: [sprite.clut, sprite.packet_tsb()],
                sprite,
            }
        })
        .collect()
}

/// A model drawn from a mesh library by index, with its placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxIndexedModel {
    /// The mesh index: the `etmd` effect-model library (`World::global_tmd`)
    /// for effect models, the scene TMD pack for field stager parts.
    pub index: usize,
    /// Column-major model matrix in the draw frame (module note).
    pub model: [f32; 16],
}

/// A move-VM scene-graph part with its placement. The mesh is the host's to
/// resolve: `World::part_draw_vram_mesh` (the morphed or rest mesh with the
/// part's render scale and colour word applied), or the library mesh when
/// `SummonPartDraw::draws_rest_mesh`.
#[derive(Debug, Clone)]
pub struct FxPartModel {
    /// The part.
    pub draw: SummonPartDraw,
    /// Column-major model matrix in the draw frame.
    pub model: [f32; 16],
}

/// A node that carries its own per-frame mesh (a draw-kind-4 ribbon or
/// sprite arm, a battle ground shadow), with its placement.
#[derive(Debug, Clone)]
pub struct FxMeshModel {
    /// The node's mesh, in model space.
    pub mesh: legaia_tmd::mesh::VramMesh,
    /// Column-major model matrix in the draw frame.
    pub model: [f32; 16],
}

/// The live effect layer's 3D draws for this frame, by source.
#[derive(Debug, Clone, Default)]
pub struct FxModelDraws {
    /// `etmd.dat` effect meshes (`World::active_effect_models`), indexed
    /// into the effect-model library. None on the overworld.
    pub effect_models: Vec<FxIndexedModel>,
    /// Summon parts, then battle move-FX parts (both resolve into the
    /// effect-model library). None on the overworld.
    pub parts: Vec<FxPartModel>,
    /// Draw-kind-4 nodes: effect ribbons (move-VM op `0x42`) and `0x4000`
    /// sprite-arm quads (op `0x23`). These draw on the overworld too:
    /// retail's per-actor dispatcher `FUN_8001ADA4` makes no mode test on its
    /// case-4 arm (`0x8001B060..0x8001B160`).
    pub kind4: Vec<FxMeshModel>,
    /// Battle ground shadows (`FUN_80048A08`'s disc, built by the default
    /// arm `FUN_80028158`), one per drawn body. Empty outside battle.
    pub shadows: Vec<FxMeshModel>,
    /// Field move-VM stager parts (op `0x34` sub 3), indexed into the scene
    /// TMD pack (`DAT_8007C018[5 + model_sel]`). None on the overworld.
    pub field_parts: Vec<FxIndexedModel>,
}

/// Compose this frame's effect model draws. `part_cam` is the retail camera
/// a part's `+0x52 & 0x780` camera-relative bits resolve against
/// (`FUN_8001CF50`), `None` under a host vantage; `plan_of` is the battle
/// actor pass's draw plan, which decides which bodies cast a shadow.
pub fn effect_model_draws(
    world: &World,
    part_cam: Option<&PartCameraPose>,
    plan_of: impl Fn(usize) -> Option<BattleActorDrawPlan>,
) -> FxModelDraws {
    let battle = world.mode == SceneMode::Battle;
    let overworld = world.mode == SceneMode::WorldMap;
    let model_flip = if battle { WORLD_FLIP } else { IDENTITY };
    // A node's placement: `T(F world_pos)`, or - for a `+0x52 & 0x780`
    // node - the camera-relative prefix `FUN_8001CF50` resolves to; then
    // the rotation and the per-model flip.
    let node = |flags: u16, pos: [f32; 3], rot: [f32; 3]| -> [f32; 16] {
        mat4_mul(
            &part_model_place(flags, pos, part_cam, battle),
            &mat4_mul(&part_rotation(rot, battle), &model_flip),
        )
    };
    let mut out = FxModelDraws::default();
    if !overworld {
        out.effect_models = world
            .active_effect_models()
            .into_iter()
            .filter(|em| world.global_tmd(em.tmd_index as i16).is_some())
            .map(|em| FxIndexedModel {
                index: em.tmd_index,
                model: mat4_mul(&mat4_translation(em.world_pos), &model_flip),
            })
            .collect();
        out.parts = world
            .active_summon_part_draws()
            .into_iter()
            .chain(world.active_move_fx_part_draws())
            .map(|draw| FxPartModel {
                model: node(draw.flags_52, draw.world_pos, draw.rot),
                draw,
            })
            .collect();
    }
    out.kind4 = world
        .active_effect_kind4_draws()
        .into_iter()
        .map(|rb| FxMeshModel {
            model: node(rb.flags_52, rb.world_pos, rb.rot),
            mesh: rb.mesh,
        })
        .collect();
    out.shadows = world
        .battle_ground_shadows(plan_of)
        .into_iter()
        .map(|rb| FxMeshModel {
            // Placed like a part (the frame flip on the translation too),
            // with no rotation: a bare `T(world_pos)` put the disc's lift on
            // the wrong side of the floor in the Y-up frame.
            model: mat4_mul(
                &part_model_place(rb.flags_52, rb.world_pos, part_cam, battle),
                &model_flip,
            ),
            mesh: rb.mesh,
        })
        .collect();
    if !overworld {
        // Field frame: the models compose on the raw retail frame, so the
        // camera-relative prefix takes no flip.
        out.field_parts = world
            .active_field_fx_part_draws()
            .into_iter()
            .map(|fp| {
                let place =
                    camera_relative_model_prefix(fp.flags_52, fp.world_pos, part_cam, false)
                        .unwrap_or_else(|| mat4_translation(fp.world_pos));
                FxIndexedModel {
                    index: fp.model_index,
                    model: mat4_mul(
                        &place,
                        &mat4_mul(&part_rotation(fp.rot, false), &model_flip),
                    ),
                }
            })
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite() -> EffectSprite {
        EffectSprite {
            world_pos: [10.0, -20.0, 30.0],
            size: [8.0, 4.0],
            uv: [16, 32],
            uv_size: [8, 4],
            page: 0,
            clut: 0,
            brightness: 0x80,
            flip_h: false,
            flip_v: false,
            age01: 0.0,
        }
    }

    #[test]
    fn the_uv_mirror_swaps_the_base_column_and_row() {
        let s = sprite();
        assert_eq!(
            sprite_corner_uvs(&s),
            [[16, 32], [23, 32], [16, 35], [23, 35]]
        );
        let m = EffectSprite {
            flip_h: true,
            flip_v: true,
            ..s
        };
        assert_eq!(
            sprite_corner_uvs(&m),
            [[23, 35], [16, 35], [23, 32], [16, 32]]
        );
    }

    #[test]
    fn corners_straddle_the_centre_by_the_half_extents() {
        let b = FxBillboard {
            sprite: sprite(),
            half: (2.0, 1.0),
            uv: [[0, 0]; 4],
            cba_tsb: [0, 0],
        };
        let [tl, tr, bl, br] = b.corners([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(tl, [8.0, -19.0, 30.0]);
        assert_eq!(tr, [12.0, -19.0, 30.0]);
        assert_eq!(bl, [8.0, -21.0, 30.0]);
        assert_eq!(br, [12.0, -21.0, 30.0]);
    }

    #[test]
    fn the_overworld_draws_no_pool_children() {
        let w = World {
            mode: SceneMode::WorldMap,
            ..World::default()
        };
        assert!(effect_billboards(&w, 1.0).is_empty());
        let lists = effect_model_draws(&w, None, |_| None);
        assert!(lists.effect_models.is_empty() && lists.parts.is_empty());
        assert!(lists.field_parts.is_empty());
    }
}
