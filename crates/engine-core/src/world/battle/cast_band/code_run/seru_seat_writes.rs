//! The creature-seat writes a player Seru-magic director's arms make on
//! their own passes, held or not, that no phase-chain body carries: the
//! render node's floor clip `+0x42`, PROT 0903's hand-driven rise `+0x36`,
//! a clip staged on a held pass, a loop-counter clear or re-arm, a cursor
//! store and an anim-rate store.

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
    /// [`ModuleCamState::creature_floor_clip`]: legaia_engine_cast::cast_module_camera::ModuleCamState::creature_floor_clip
    /// [`ModuleCamState::creature_stage`]: legaia_engine_cast::cast_module_camera::ModuleCamState::creature_stage
    pub(in crate::world::battle::cast_band) fn apply_seru_seat_writes(
        &mut self,
        prot_entry: u32,
        phase: u8,
    ) {
        let stage = self.casting.module_cam.creature_stage.take();
        let release = std::mem::take(&mut self.casting.module_cam.creature_loop_release);
        let rearm = std::mem::take(&mut self.casting.module_cam.creature_loop_arm);
        let seek = self.casting.module_cam.creature_seek.take();
        let rate = self.casting.module_cam.creature_rate.take();
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
        // A director arm's `+0x21B = 0xFF` / `+0x176 = 0xFF0`: the playing
        // clip's park is held again (PROT 0914, 0923, 0928).
        if rearm {
            a.battle.hit_count_bound = 0xFF;
            if let Some(p) = a.battle_animation.as_mut() {
                p.set_loop_timer(0xFF0);
            }
        }
        // A cursor store on the creature's node (PROT 0928's `0xF0`).
        if let Some(c) = seek
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.seek_sixteenths(c);
        }
        // An anim-rate store `+0x21D` (PROT 0914's arm `0xC` ramp).
        if let Some(r) = rate {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(r);
        }
        // A director arm's `+0x21B` / `+0x176` clear (PROT 0905's arm 8).
        if release {
            a.battle.hit_count_bound = 0;
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
        }
    }
}
