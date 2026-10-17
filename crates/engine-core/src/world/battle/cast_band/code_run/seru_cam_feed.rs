//! The live inputs the player-Seru camera directors read beside the seats:
//! the yaw global, the cast's target group as `FUN_801DCEAC` measures it, and
//! the module words a ported tick body owns.

use super::*;

impl World {
    /// Fill the director inputs of `st` that the seats do not carry
    /// ([`legaia_engine_cast::cast_module_camera::ModuleCamState::live_yaw`],
    /// `target_aim`, `module_words`) for module `entry` cast by `caster`.
    ///
    /// REF: FUN_801DCEAC
    pub(in crate::world::battle::cast_band) fn feed_seru_cam(
        &self,
        entry: u32,
        caster: u8,
        st: &mut legaia_engine_cast::cast_module_camera::ModuleCamState,
    ) {
        use legaia_engine_battle_vm::battle_target_group::{
            GroupSlot, MIN_GROUP_EXTENT, RENDER_FLAG_HIDDEN, target_group_aim,
        };
        st.live_yaw = self
            .battle
            .camera
            .as_ref()
            .map(|c| (c.pose().yaw.rem_euclid(4096.0) as i32 & 0xFFF) as u16);
        let code = self
            .actors
            .get(usize::from(caster))
            .map_or(0, |a| a.battle.active_target);
        st.target_code = Some(code);
        st.target_aim = if code < 8 {
            self.actors.get(usize::from(code)).map(|t| {
                legaia_engine_battle_vm::battle_target_group::GroupAim {
                    centroid_x: t.move_state.world_x.wrapping_neg(),
                    centroid_z: t.move_state.world_z.wrapping_neg(),
                    extent: MIN_GROUP_EXTENT,
                }
            })
        } else {
            // `FUN_801DCEAC` walks retail numbering (party `0..3`, monsters
            // `3..7`); the engine seats monsters straight after the party.
            let seats = self.battle_seat_map();
            let mut slots = [GroupSlot {
                live: false,
                x: 0,
                z: 0,
            }; 8];
            for (retail, out) in slots.iter_mut().enumerate() {
                let Some(engine) = seats.engine(retail as u8) else {
                    continue;
                };
                let Some(t) = self.actors.get(usize::from(engine)) else {
                    continue;
                };
                let live = seats.is_party(engine)
                    || (t.battle_monster_id.is_some()
                        && t.battle.render_flag != RENDER_FLAG_HIDDEN);
                *out = GroupSlot {
                    live,
                    x: t.move_state.world_x,
                    z: t.move_state.world_z,
                };
            }
            target_group_aim(code, &slots)
        };
        if entry == 910 {
            let s = &self.casting.module_swordie;
            st.module_words = [s.hits as i32, s.progress[3] as i32];
        }
    }
}
