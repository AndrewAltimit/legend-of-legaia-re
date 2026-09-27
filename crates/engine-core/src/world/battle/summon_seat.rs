//! Releasing a player-summon creature's seat when the battle it was spawned
//! into ends.
//!
//! The creature is seated by the host (`spawn_summon_creature` on the native
//! window, `spawn_summon_creature_web` on the browser page) at a slot past
//! the combatant table, with a mesh binding, a texture slot, an animation
//! player and a pose. The teardown used to be each host's own: the window
//! cleared all five fields, the page only the first two, so the page's next
//! fight found a creature's clip and pose still riding the reused seat. One
//! release now serves both.

use super::*;

impl World {
    /// Clear the per-actor state a summon creature's spawn staged on `slot`.
    pub fn release_summon_seat(&mut self, slot: usize) {
        if let Some(a) = self.actors.get_mut(slot) {
            a.active = false;
            a.tmd_binding = None;
            a.battle_tex_slot = None;
            a.battle_animation = None;
            a.pose_frame = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_clears_every_field_the_spawn_staged() {
        let mut w = World::new();
        while w.actors.len() < 10 {
            w.actors.push(Actor::default());
        }
        let a = &mut w.actors[9];
        a.active = true;
        a.tmd_binding = Some(3);
        a.battle_tex_slot = Some(4);
        w.release_summon_seat(9);
        let a = &w.actors[9];
        assert!(!a.active);
        assert_eq!(a.tmd_binding, None);
        assert_eq!(a.battle_tex_slot, None);
        assert!(a.battle_animation.is_none());
        assert!(a.pose_frame.is_none());
        // An out-of-range seat is a no-op, not a panic.
        w.release_summon_seat(99);
    }
}
