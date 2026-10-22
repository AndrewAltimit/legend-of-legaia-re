//! Route this tick's battle **effect-script spawn requests** into the world's
//! two spawn paths - the one routing both play hosts run.
//!
//! The per-actor effect-script walk (`FUN_801DEA50`, driven from the
//! animation tick) queues one [`legaia_engine_battle::battle_events::BattleEffectSpawn`] per
//! effect record it consumed. Retail hands each straight to its spawner: the
//! `0x80`-flagged **direct** form to the 2D effect pool `FUN_801DFDF0`
//! (`jal` at `0x801DEE9C`, spawn angle = the actor's facing), the **table**
//! form to the `0x801F6324` prototype scene. Both play hosts carried their
//! own copy of this loop; this is the one they now share.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// What one routed spawn did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedEffectSpawn {
    /// The request as the walk queued it.
    pub spawn: legaia_engine_battle::battle_events::BattleEffectSpawn,
    /// For the table form, whether a prototype scene was staged; `None` for
    /// the direct form (the pool spawn reports nothing).
    pub table_staged: Option<bool>,
}

impl World {
    /// Drain the queued effect-script spawns and seat each on its path.
    /// Returns what was routed, for a host's log.
    pub fn route_battle_effect_spawns(&mut self) -> Vec<RoutedEffectSpawn> {
        let spawns = self.drain_battle_effect_spawns();
        let mut out = Vec::with_capacity(spawns.len());
        // Retail spawns these ahead of the frame's effect-pool walker (the
        // anim tick and the action SM precede the draw tick that walks the
        // pool), so each direct spawn takes the sweeps this tick's walker
        // already gave the rest (`Pool::catch_up_master`).
        let owed = if self.mode == SceneMode::Battle {
            self.battle.anim_pass_vsyncs
        } else {
            0
        };
        for s in spawns {
            let at = [
                s.at.0.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                s.at.1.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                s.at.2.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            ];
            let table_staged = if s.direct {
                self.spawn_effect_caught_up(s.effect, at, s.facing, owed);
                None
            } else if let Some(slot) = s.homing_slot {
                // The flight's table arm (`0x801E1204..0x801E1238`): rotation
                // trio `ctx[+0x1164 + slot * 8]` - the slot heading, the two
                // other angles never written - at scale `0x1000`, and the
                // spawned actor replaces the slot's handle.
                let staged = self.spawn_action_table_effect_posed(
                    s.effect,
                    at,
                    [0, s.facing as i16, 0],
                    0x1000,
                );
                if staged {
                    let fx = &mut self.casting.active_action_fx;
                    for scene in fx.iter_mut() {
                        if scene.homing_slot == Some(slot) {
                            scene.homing_slot = None;
                        }
                    }
                    if let Some(scene) = fx.last_mut() {
                        scene.homing_slot = Some(slot);
                    }
                }
                Some(staged)
            } else {
                // `FUN_801DEA50`'s table arm: the actor's facing turned half
                // a circle, the code's base scale times the actor's
                // mesh-header `+0x72` (the engine actor carries no render
                // node; the unit stands in, as for the walk's offsets).
                let rot = [0, (s.facing as i16).wrapping_add(0x800), 0];
                let scale = crate::world::effects::action_fx_base_scale(s.effect);
                Some(self.spawn_action_table_effect_posed(s.effect, at, rot, scale))
            };
            out.push(RoutedEffectSpawn {
                spawn: s,
                table_staged,
            });
        }
        // Both hosts route once a frame, after the world tick and before
        // the draw: every part this frame seated - here, or in the tick's
        // stager and module arms - gets the seater's VM run before it draws.
        self.seat_run_battle_fx();
        out
    }
}
