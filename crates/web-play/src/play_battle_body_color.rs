//! Browser seat of a battle body's **untextured half**: the `F*` / `G*`
//! prims its TMD carries, which the textured actor upload skips and retail's
//! body draw `FUN_80048A08` hands to the prim dispatcher's untextured leaves
//! (kernel `legaia_engine_world_views::battle_draws::battle_body_color_mesh`).
//!
//! The page uploads the half as a second scene mesh beside the body - every
//! vertex untextured (`a_flat_rgba` alpha `0`) - poses it each frame with the
//! body's pose, morph and hidden objects through its own object ids, and
//! re-sends its colour and blend streams on the body's limb and blend keys.
//! Each blend word carries the ABR the body's textured packets leave in the
//! GPU's texpage register, with the colour word's ABE and ABR ORed over it
//! (`BattleActorDrawPlan::apply_body_blend_color`), as the native window's
//! `build_battle_body_color_meshes` does per frame. Empty streams for a body
//! with no untextured prim.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    /// Battle mesh `i`'s untextured half at rest, its object ids, and the
    /// world slot it draws for.
    fn body_color_rest(&self, i: u32) -> Option<(usize, legaia_tmd::mesh::ColorMesh, Vec<u32>)> {
        let (actor_idx, src) = self.battle_render.as_ref()?.actor_color_src(i as usize)?;
        let (mesh, ids) =
            legaia_engine_world_views::battle_draws::battle_body_color_mesh(&src.0, &src.1)?;
        Some((actor_idx, mesh, ids))
    }

    /// [`Self::body_color_rest`] with the body's draw plan applied: the
    /// colour word's blend onto the words, bank 2's flat colours, and the Rot
    /// limb dimming.
    fn body_color_drawn(&self, i: u32) -> Option<legaia_tmd::mesh::ColorMesh> {
        let (actor_idx, mut mesh, ids) = self.body_color_rest(i)?;
        let host = self.scene_host.host()?;
        let objects = ids.iter().copied().max().map_or(0, |m| m as usize + 1);
        if let Some(plan) = host.world.battle_limb_dim_plan(actor_idx, objects) {
            let ch = legaia_engine_world_views::battle_draws::battle_live_object_channels(
                &host.world,
                actor_idx,
            );
            let ids: Vec<u32> = match ch {
                Some(ch) => ids
                    .iter()
                    .map(|&o| ch.get(o as usize).copied().unwrap_or(o))
                    .collect(),
                None => ids,
            };
            plan.apply_rgb(&mut mesh.colors, &ids);
        }
        if let Some(plan) = self.battle_draw_plan(actor_idx) {
            plan.apply_body_blend_color(&mut mesh);
        }
        Some(mesh)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Battle mesh `i`'s untextured half: object-local positions, `xyz` per
    /// vertex. Empty for a body with none.
    pub fn play_battle_actor_color_positions(&self, i: u32) -> Vec<f32> {
        self.body_color_rest(i)
            .map(|(_, m, _)| m.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// The untextured half's triangle list.
    pub fn play_battle_actor_color_indices(&self, i: u32) -> Vec<u32> {
        self.body_color_rest(i)
            .map(|(_, m, _)| m.indices)
            .unwrap_or_default()
    }

    /// The untextured half's per-vertex TMD object ids (its pose rig).
    pub fn play_battle_actor_color_object_ids(&self, i: u32) -> Vec<u32> {
        self.body_color_rest(i)
            .map(|(_, _, ids)| ids)
            .unwrap_or_default()
    }

    /// The untextured half's `[cba, tsb]` stream: CBA `0`, the TSB slot its
    /// blend word, under the body's colour word this frame.
    pub fn play_battle_actor_color_cba_tsb(&self, i: u32) -> Vec<u16> {
        self.body_color_drawn(i)
            .map(|m| m.blend.iter().flat_map(|&b| [0, b]).collect())
            .unwrap_or_default()
    }

    /// The untextured half's `[r, g, b, 0]` fill stream (alpha `0`: every
    /// vertex is an untextured prim's), dimmed and flattened as the body's.
    pub fn play_battle_actor_color_rgba(&self, i: u32) -> Vec<u8> {
        self.body_color_drawn(i)
            .map(|m| {
                m.colors
                    .iter()
                    .flat_map(|c| [c[0], c[1], c[2], 0])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The untextured half's object-local positions with the playing clip's
    /// morph lanes blended in (`World::battle_morphed_tmd`), index-aligned
    /// with [`Self::play_battle_actor_color_positions`]. Empty while no lane
    /// is active.
    pub fn play_battle_actor_color_morph(&self, i: u32) -> Vec<f32> {
        let Some((actor_idx, src)) = self
            .battle_render
            .as_ref()
            .and_then(|b| b.actor_color_src(i as usize))
        else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let Some(tmd) = host.world.battle_morphed_tmd(actor_idx, &src.0) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_color_mesh(&tmd, &src.1)
            .positions
            .iter()
            .flatten()
            .copied()
            .collect()
    }
}
