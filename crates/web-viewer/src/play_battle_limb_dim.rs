//! Browser seat of the battle draw's **Rot limb dimming** - the one
//! per-object colour rule of the battle per-actor draw `FUN_80048A08`
//! (kernel [`legaia_engine_vm::battle_actor_draw`], resolved against the
//! world by `World::battle_limb_dim_plan`).
//!
//! A party member carrying a Rot limb bit draws that limb's object range
//! darkened toward a blue-black far colour. The page uploads each actor's
//! packet-colour stream once per battle, so the dimming rides a re-upload:
//! the page polls [`LegaiaRuntime::play_battle_actor_limb_key`] each frame
//! and, when it changes, re-sends [`LegaiaRuntime::play_battle_actor_limb_rgba`]
//! into the mesh's colour buffer. The native window applies the same plan
//! to its per-frame posed mesh.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

impl LegaiaRuntime {
    fn limb_dim_plan(&self, i: u32) -> Option<legaia_engine_vm::battle_actor_draw::LimbDimPlan> {
        let br = self.battle_render.as_ref()?;
        let host = self.scene_host.as_ref()?;
        let (actor_idx, _, ids) = br.actor_colour_stream(i as usize)?;
        let objects = ids.iter().copied().max().map_or(0, |m| m as usize + 1);
        host.world.battle_limb_dim_plan(actor_idx, objects)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// A key for battle mesh `i`'s current limb dimming: `0` when nothing
    /// dims, otherwise a hash that changes whenever the dimmed stream does.
    /// Returned as `f64` so the full 53-bit value survives the JS boundary.
    pub fn play_battle_actor_limb_key(&self, i: u32) -> f64 {
        self.limb_dim_plan(i)
            .map_or(0.0, |p| (p.key() & ((1u64 << 53) - 1)).max(1) as f64)
    }

    /// Battle mesh `i`'s packet-colour stream with the limb dimming applied
    /// (the rest stream when nothing dims) - same layout as
    /// `play_battle_actor_flat_rgba`. Empty when the mesh has no stream.
    pub fn play_battle_actor_limb_rgba(&self, i: u32) -> Vec<u8> {
        let Some((_, flat, ids)) = self
            .battle_render
            .as_ref()
            .and_then(|b| b.actor_colour_stream(i as usize))
        else {
            return Vec::new();
        };
        let mut out = flat.to_vec();
        if let Some(plan) = self.limb_dim_plan(i) {
            plan.apply_rgba(&mut out, ids);
        }
        out
    }
}
