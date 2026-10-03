//! Battle-effect lifetime: what retail's actor-pool reset takes down at a
//! mode switch, and the residue probe that proves nothing survives it.
//!
//! Every battle visual the port keeps outside the battle actor table - the
//! `efect.dat` billboard pool, the move-FX / effect-script / summon move-VM
//! scene-graphs, the move-FX streak block, the cast-module band state - is,
//! in retail, either an actor on the SCUS actor lists or a word in the
//! battle overlay's own image. Both die at the next mode switch:
//!
//! - The mode initialiser `FUN_8001DCF8` calls the per-stage init
//!   `FUN_8001E1B4` (`jal` at `0x8001E020`), which re-seeds the 143-slot actor
//!   free stack (`FUN_800203EC` at `0x8001E324`) and re-pops the seven
//!   actor-list sentinels (`FUN_80020424` x7, `0x8001E32C..0x8001E364`), each
//!   left pointing at itself. Whatever effect actor was still on a list when
//!   the battle ended is unlinked wholesale - there is no per-actor walk.
//! - The `efect.dat` walker (`FUN_801E0080`) is battle-overlay code reached
//!   only from the battle frame driver, behind the pool-ready byte
//!   `0x8007BD58`; the battle loader re-initialises the pool at stage `0xE`
//!   (`FUN_801DE914`). No field frame runs it.
//!
//! The port's world outlives both events, so it has to drop the same state by
//! name: [`World::teardown_battle_effects`] runs at battle exit
//! ([`World::finish_battle`] / [`World::resolve_game_over_hold`]) and at scene
//! load (the scene host), and [`World::battle_effect_residue`] is the probe
//! the soak harness and the regression tests read.

use super::*;

impl World {
    /// The names of every battle-scoped effect family still live - empty once
    /// a battle has been torn down. A detector's view: "effects alive after
    /// battle exit / after a scene change" is this list being non-empty on a
    /// field or world-map frame.
    ///
    /// Covers the families both play hosts draw without a mode test: the
    /// `efect.dat` billboard pool ([`Self::active_effect_sprites`]), the
    /// summon / move-FX / effect-script scene-graphs
    /// ([`Self::active_move_fx_part_draws`], [`Self::active_effect_kind4_draws`]),
    /// the streak block and its trail texpage, and the cast band's pending
    /// requests. Field-owned effects (the ambient tree, the op-`0x34` field
    /// stagers, CLUT-cell cyclers) are deliberately not in it.
    pub fn battle_effect_residue(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.effect_pool.active_count() > 0 || self.effect_pool.active_child_count() > 0 {
            out.push("efect_pool");
        }
        let c = &self.casting;
        if c.active_summon.is_some() {
            out.push("active_summon");
        }
        if c.active_move_fx.is_some() {
            out.push("active_move_fx");
        }
        if !c.active_action_fx.is_empty() {
            out.push("active_action_fx");
        }
        if c.move_fx_trail_texpage.is_some() {
            out.push("move_fx_trail_texpage");
        }
        if c.move_fx_streak.is_armed() {
            out.push("move_fx_streak");
        }
        if c.summon_stager.is_some() || c.summon_actor_slot.is_some() {
            out.push("summon_stager");
        }
        if c.capture_spell.is_some() || c.pending_cast.is_some() {
            out.push("cast_band");
        }
        if c.pending_summon_spawn.is_some()
            || c.pending_move_fx_spawn.is_some()
            || c.pending_move_fx_cue.is_some()
        {
            out.push("pending_fx_request");
        }
        if self.battle.clip_ribbon.is_some() {
            out.push("clip_ribbon");
        }
        if !self.battle.effect_spawns.is_empty() {
            out.push("effect_spawns");
        }
        out
    }

    /// Retire the cast-module / summon scene-graph when the action SM opens
    /// the next action (state `0x00`).
    ///
    /// The engine stages a module's spawn records as **data**
    /// ([`Self::spawn_cast_module_fx`]) and runs its tick body separately
    /// ([`Self::run_cast_module_code`]), so nothing halts a record the module
    /// code would have halted - and many records are infinite loops (an
    /// emitter's `0x18 0x4000` loop, a held glow) that retail's tick body
    /// ends itself. Retail cannot let them outlive the cast: their move-VM
    /// bytecode lives in the slot-B image, which the next cast re-pages, and
    /// the band only leaves `0x70` once the module tick reports its
    /// choreography done. Retiring at the next action's `0x00` is the latest
    /// point both of those allow while keeping every record's own tail - a
    /// finite effect has run out by then, and only the loops are cut.
    ///
    /// The bound is an `inference` from those two facts, not a capture of the
    /// module code's own halt sites.
    pub(in crate::world) fn retire_cast_scene_at_action_begin(&mut self) {
        self.casting.active_summon = None;
    }

    /// Drop every battle-scoped effect: the port's stand-in for the actor
    /// pool reset retail's per-stage init `FUN_8001E1B4` performs on every
    /// mode switch (`0x8001E324..0x8001E364`), plus the battle overlay's own
    /// image words the cast band keeps in [`crate::world::CastFxState`].
    ///
    /// Keeps the one non-battle member of `CastFxState`: the installed
    /// cast-effect **data** pool (PROT 0903..0966, loaded once per host).
    /// Idempotent.
    // REF: FUN_8001E1B4 (the actor free-stack + list-sentinel reset)
    pub fn teardown_battle_effects(&mut self) {
        self.effect_pool = vm::effect_vm::Pool::new();
        let data_pool = self.casting.effect_pool.take();
        self.casting = crate::world::CastFxState::new();
        self.casting.effect_pool = data_pool;
        self.battle.clip_ribbon = None;
        self.battle.effect_spawns.clear();
    }
}
