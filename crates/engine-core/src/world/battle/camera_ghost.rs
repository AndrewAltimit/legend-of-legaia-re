//! The near-camera ghost pass (`FUN_8004DC68`, kernel
//! [`vm::battle_action::camera_ghost_pass`]) run over live world state, once
//! per frame after the battle camera ticks.
//!
//! The kernel reads retail's fixed pool slots (party `0..=2`, monsters
//! `3..=6`); the engine seats monsters compacted behind the party, so this
//! block converts both ways. It writes each body's
//! [`vm::battle_action::BattleActor::flag_word`], which
//! [`World::battle_actor_draw_plan`] hands the tint pass as the node colour's
//! top byte - the draw's semi-transparency mode.
//!
//! Two of the kernel's inputs have no engine mirror and are read as an
//! inference: the command-flow byte `ctx[+6]` is `0xFF` in every catalogued
//! state of a running action and is taken as `0xFF` whenever no command menu
//! is open, `0` (below the `0x1F` gate) while one is; `ctx[+0x26B]`, the
//! battle stream-slot request, is taken as idle.

use super::*;

use crate::action_effect_script::RotationLut;

impl World {
    /// The retail pool slot of engine actor `e`. Scope codes `8` / `9` pass
    /// through.
    fn retail_pool_slot(&self, e: u8) -> u8 {
        let pc = self.party.party_count.clamp(1, 3);
        match e {
            8.. => e,
            e if e < pc => e,
            e => e - pc + 3,
        }
    }

    /// One frame of the ghost pass. No-op outside battle.
    pub(in crate::world) fn tick_battle_camera_ghost(&mut self) {
        use vm::battle_action::{GhostInputs, GhostSlot, camera_ghost_pass};
        if self.mode != SceneMode::Battle {
            return;
        }
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let engine_of = |r: usize| -> Option<usize> {
            if r < 3 {
                (r < pc).then_some(r)
            } else {
                Some(pc + r - 3)
            }
        };
        let mut slots = [GhostSlot::default(); 7];
        for (r, s) in slots.iter_mut().enumerate() {
            let Some(a) = engine_of(r).and_then(|e| self.actors.get(e)) else {
                continue;
            };
            let seated = if r < 3 {
                a.battle_monster_id.is_none() && a.battle.max_hp > 0
            } else {
                a.battle_monster_id.is_some()
            };
            *s = GhostSlot {
                present: seated,
                x: a.move_state.world_x,
                z: a.move_state.world_z,
                flag_word: a.battle.flag_word,
            };
        }
        let acting_e = self.battle_ctx.active_actor;
        let (target_e, category) = self.actors.get(usize::from(acting_e)).map_or((0, 0), |a| {
            (a.battle.active_target, a.battle.action_category)
        });
        let pose = self.battle_cam_pose();
        let inputs = GhostInputs {
            yaw: (pose.yaw.rem_euclid(4096.0)) as u16,
            focus_x: pose.focus[0] as i32,
            focus_z: pose.focus[2] as i32,
            dist: pose.tr[2] as i32,
            acting: self.retail_pool_slot(acting_e),
            target: self.retail_pool_slot(target_e),
            category,
            flow: if self.battle.command.is_some() {
                0
            } else {
                0xFF
            },
            state: self.battle_ctx.action_state,
            formation: self.battle_ctx.formation_advantage,
            battle_end: if self.battle.victory.is_some() {
                0xFE
            } else {
                0xFF
            },
            ctx_26b: 0,
        };
        let lut = crate::action_effect_script::retail_rotation_lut();
        camera_ghost_pass(
            &inputs,
            &mut slots,
            |a| lut.b(a),
            |a| lut.a(a),
            |p1z, p1x, p2z, p2x| {
                let c = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                vm::battle_action::bearing_12bit_approx(c(p1z), c(p1x), c(p2z), c(p2x))
            },
        );
        for (r, s) in slots.iter().enumerate() {
            if !s.present {
                continue;
            }
            if let Some(a) = engine_of(r).and_then(|e| self.actors.get_mut(e)) {
                a.battle.flag_word = s.flag_word;
            }
        }
    }
}
