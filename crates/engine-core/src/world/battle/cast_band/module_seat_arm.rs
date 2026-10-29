//! What each slot-B module's **seat arm** stores on its creature right after
//! the seat installer `FUN_801F19EC` returns, on the same pass.
//!
//! The installer stands the creature on the caster, wearing the caster's
//! facing, at animation rate `2` ([`World::seat_summon_actor`]). Most summon
//! modules then overwrite that seat with literals of their own: a ground
//! pair on the battle's own axes, a facing, a height and often a rate. Each
//! row below is read off the instructions between the module's
//! `jal 0x801F19EC` and the arm's exit; modules whose seat arm computes the
//! seat from a heading (PROT 0903, 0904, 0905, 0907, 0908, 0910, 0916) keep
//! it in their directors, which the seat adopts through `module_cam.creature`.

use super::*;
use legaia_engine_battle_loop::module_seat_arm::*;

use SeatCoord::{Keep, Lit, SeatPlus, VictimPlus};

impl World {
    /// Apply PROT `prot_entry`'s seat-arm stores to the creature at `slot`.
    pub(in crate::world) fn apply_seat_arm_write(&mut self, prot_entry: u32, slot: usize) {
        let Some(w) = seat_arm_write(prot_entry) else {
            return;
        };
        let victim = self.casting.module_cam.victim_slot.or_else(|| {
            let caster = self.casting.summon_stager.as_ref()?.caster;
            self.actors
                .get(usize::from(caster))
                .map(|a| a.battle.active_target)
        });
        let victim_pos = victim
            .and_then(|v| self.actors.get(usize::from(v)))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        let resolve = |c: SeatCoord, cur: i16, victim: Option<i16>| match c {
            Keep => cur,
            Lit(v) => v,
            SeatPlus(d) => cur.wrapping_add(d),
            VictimPlus(d) => victim.map_or(cur, |v| v.wrapping_add(d)),
        };
        let ms = &mut a.move_state;
        ms.world_x = resolve(w.x, ms.world_x, victim_pos.map(|p| p.0));
        ms.world_y = resolve(w.y, ms.world_y, None);
        ms.world_z = resolve(w.z, ms.world_z, victim_pos.map(|p| p.1));
        if let Some(f) = w.facing {
            a.battle.facing_angle = f;
        }
        if let Some(r) = w.rate {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(r);
        }
        if let Some(c) = w.colour {
            a.battle.render_color = c;
        }
        if let Some(sc) = w.render_scale {
            a.battle.render_scale = Some(sc);
        }
        if let Some((state, blend)) = w.tint {
            a.battle.render_flag = state;
            a.battle.render_blend = blend;
        }
        if let Some((count, timer)) = w.loop_hold {
            a.battle.hit_count_bound = count;
            if let Some(p) = a.battle_animation.as_mut() {
                p.set_loop_timer(timer);
            }
        }
    }

    /// The creature stores of a module's later arms that the engine runs
    /// beside its director: PROT 0922's slide (arm 4) and wake (arm 5).
    pub(in crate::world::battle::cast_band) fn apply_module_seat_motion(
        &mut self,
        prot_entry: u32,
        phase: u8,
    ) {
        if prot_entry != 922 {
            return;
        }
        let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
            .filter(|a| a.active)
        else {
            return;
        };
        match phase {
            PUERA_SLIDE_ARM => {
                let z = &mut a.move_state.world_z;
                *z = (*z + PUERA_SLIDE_PER_TICK).min(PUERA_SLIDE_END_Z);
            }
            PUERA_WAKE_ARM
                if a.battle.anim_rate.get() == 0
                    && a.battle.current_anim == 0
                    && a.battle.queued_anim == 0 =>
            {
                a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(8);
                a.battle.queued_anim = 1;
            }
            _ => {}
        }
    }
}
