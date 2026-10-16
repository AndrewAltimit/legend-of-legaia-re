//! The creature-seat writes a player Seru-magic director's arms make on
//! their own passes, held or not, that no phase-chain body carries: the
//! render node's floor clip `+0x42`, PROT 0903's hand-driven rise `+0x36`,
//! and a clip staged on a held pass.

use super::*;

/// PROT 0903's arms that seat the creature under the floor and raise it
/// (`0x801F6E48`, `0x801F6E94`, `0x801F6F54`, the landing `0x801F6F7C`).
const GIMARD_RISE_ARMS: std::ops::RangeInclusive<u8> = 3..=5;

impl World {
    /// Put the director's creature writes on the summon seat, on the pass
    /// the director made for `phase` of module `prot_entry`.
    ///
    /// - The floor clip ([`ModuleCamState::creature_floor_clip`]) on the
    ///   seat's draw; PROT 0903 and 0908 are the directed modules that write
    ///   `creature[+0x22C] + 0x42`.
    /// - PROT 0903's height through arms 3..5: seated at `y = 0x200` and
    ///   raised by the frame product a pass until arm 5 lands it on the
    ///   floor - with the clip, the creature climbs out of the ground.
    /// - A clip a held arm staged ([`ModuleCamState::creature_stage`]):
    ///   PROT 0905's arm 8 keeps the creature on its hover clip `2`.
    ///
    /// [`ModuleCamState::creature_floor_clip`]: legaia_engine_battle_vm::cast_module_camera::ModuleCamState::creature_floor_clip
    /// [`ModuleCamState::creature_stage`]: legaia_engine_battle_vm::cast_module_camera::ModuleCamState::creature_stage
    pub(in crate::world::battle::cast_band) fn apply_seru_seat_writes(
        &mut self,
        prot_entry: u32,
        phase: u8,
    ) {
        let stage = self.casting.module_cam.creature_stage.take();
        let Some(slot) = self.casting.summon_actor_slot else {
            return;
        };
        let st = self.casting.module_cam;
        let Some(a) = self.actors.get_mut(usize::from(slot)) else {
            return;
        };
        // Every directed module rewrites the clip, so a seat slot a
        // previous cast left clipped is cleared.
        a.battle.floor_clip = st.creature_floor_clip;
        if prot_entry == 903
            && GIMARD_RISE_ARMS.contains(&phase)
            && let Some(c) = st.creature
        {
            a.move_state.world_y = c.y;
        }
        if let Some(clip) = stage {
            a.battle.queued_anim = clip;
        }
    }
}
