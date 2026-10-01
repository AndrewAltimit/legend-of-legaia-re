//! Browser seat of a battle body's **whole-mesh semi-transparency** - the
//! colour word's ABE + ABR applied to every prim the body draws
//! ([`legaia_engine_core::battle_body_blend`]): the near-camera ghost pass
//! `FUN_8004DC68` (`B + F/4`) and the capture / defeat fade (`B + F`).
//!
//! The page uploads each actor's `[cba, tsb]` stream once per battle, and
//! its blend pass builds the per-ABR semi tail from that stream, so the
//! blend rides a re-upload: the page polls
//! [`LegaiaRuntime::play_battle_actor_blend_key`] each frame and, when it
//! changes, re-sends [`LegaiaRuntime::play_battle_actor_blend_cba_tsb`]
//! (`TmdRenderer::updateSceneMeshCbaTsb` rebuilds the tail). The native
//! window applies the same call (`BattleActorDrawPlan::apply_body_blend`) to
//! its per-frame posed mesh and to a blended body's rest mesh.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    /// Battle mesh `i`'s draw plan this frame (`None` off a battle body).
    fn body_blend_plan(&self, i: u32) -> Option<legaia_engine_core::world::BattleActorDrawPlan> {
        let (actor_idx, _, _) = self
            .battle_render
            .as_ref()?
            .actor_colour_stream(i as usize)?;
        self.battle_draw_plan(actor_idx)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// A key for battle mesh `i`'s blend: `0` for an opaque body, `1 + abr`
    /// for one whose colour word raises semi-transparency.
    pub fn play_battle_actor_blend_key(&self, i: u32) -> u32 {
        self.body_blend_plan(i)
            .and_then(|p| p.semi_mode())
            .map_or(0, |m| 1 + u32::from(m))
    }

    /// Battle mesh `i`'s `[cba, tsb]` stream with the body's blend applied
    /// (the rest stream for an opaque body) - same layout as
    /// `play_battle_actor_cba_tsb`. Empty when the mesh has none.
    pub fn play_battle_actor_blend_cba_tsb(&self, i: u32) -> Vec<u16> {
        let flat = self.play_battle_actor_cba_tsb(i);
        let mut pairs: Vec<[u16; 2]> = flat.as_chunks::<2>().0.to_vec();
        if let Some(plan) = self.body_blend_plan(i) {
            plan.apply_body_blend(&mut pairs);
        }
        pairs.into_iter().flatten().collect()
    }
}
