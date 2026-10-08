//! The interaction motion-pause kick over the field NPC channels.
//!
//! Retail's `FUN_8003C9AC` (ported at [`legaia_engine_vm::motion_pause`])
//! walks the scene actor list and, for each moving-class actor with a
//! motion stream, reloads the requested-move pair `+0x5C` / `+0x88` from the
//! actor's `0x801C6470` record. The engine keeps those fields on each
//! placement's ambient channel, so this host projects the channels into the
//! port's actor view, runs it, and writes the reloads back.

use super::World;
use legaia_engine_vm::motion_pause::{
    PAUSE_SENTINEL, PAUSE_TABLE_STRIDE, PauseKickActor, motion_pause_kick,
};

/// A `move_id` the port can never write (it stores a zero-extended byte), so
/// an actor still holding it after the sweep was not kicked.
const NOT_KICKED: u16 = u16::MAX;

impl World {
    /// Run the motion-pause kick over every ambient channel. Returns how many
    /// channels it reloaded.
    ///
    /// Each channel stands in for one actor-list entry: `+0x10` is
    /// `actor_flags` (seeded moving-class by
    /// [`Self::seed_field_npc_ambient`], as `FUN_8003A1E4` seats it), `+0x80`
    /// is present because a channel exists only for a bound stream, and the
    /// `0x801C6470` record is `default_move`. The port indexes its table by
    /// actor id, so the projection numbers the channels in slot order and
    /// lays their records out in the same order.
    ///
    /// A kicked channel's `requested_move` / `move_pair` take the record's
    /// standing move and nothing else happens here: retail's kick only
    /// writes `+0x5C` / `+0x88`, and the move-table consumer plays it on the
    /// actor's next tick, after that tick's motion ops ran - so a walker the
    /// ops send on to its next step requests its walk anim again first and
    /// never shows the standing clip, while one parked in a wait or pick
    /// phase does. [`Self::tick_field_npc_ambient`] is that consumer.
    ///
    /// REF: FUN_8003C9AC (the kick, `legaia_engine_vm::motion_pause`),
    /// FUN_801D5B5C (caller `0x801D5BF0`), FUN_8003BDE0 (caller `0x8003C0D4`),
    /// FUN_800204F8 (the `+0x5C` consumer, in `tick_field_npc_ambient`)
    pub fn kick_field_npc_motion_pause(&mut self) -> usize {
        if self.npcs.ambient.is_empty() {
            return 0;
        }
        let slots: Vec<u8> = self.npcs.ambient.keys().copied().collect();
        let mut table = vec![PAUSE_SENTINEL; slots.len() * PAUSE_TABLE_STRIDE];
        let mut actors: Vec<PauseKickActor> = Vec::with_capacity(slots.len());
        for (i, slot) in slots.iter().enumerate() {
            let vm = &self.npcs.ambient[slot].vm;
            table[i * PAUSE_TABLE_STRIDE] = vm.default_move[0];
            actors.push(PauseKickActor {
                flags: vm.actor_flags,
                has_motion_stream: true,
                actor_id: i as u16,
                move_id: NOT_KICKED,
                move_id_mirror: NOT_KICKED,
            });
        }
        let kicked = motion_pause_kick(&mut actors, &table);
        for (slot, actor) in slots.into_iter().zip(actors) {
            if actor.move_id == NOT_KICKED {
                continue;
            }
            let id = actor.move_id as u8;
            if let Some(chan) = self.npcs.ambient.get_mut(&slot) {
                chan.vm.requested_move = Some(id);
                chan.vm.move_pair = Some(i16::from(id));
            }
        }
        kicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::FieldNpcAmbient;
    use legaia_engine_vm as vm;

    /// A channel seeded the way `seed_field_npc_ambient` seeds one, running
    /// `code` with op `0x17`'s record already installed.
    fn channel(slot: u8, default_move: [u8; 2], code: Vec<u8>) -> FieldNpcAmbient {
        let mut m = vm::ambient_motion::AmbientMotion::new(u32::from(slot), 0);
        m.actor_flags |= vm::motion_pause::MOVING_CLASS;
        m.default_move = default_move;
        FieldNpcAmbient {
            defers: false,
            walks: true,
            variants: vec![(legaia_asset::man_motion::SELECTOR_DEFAULT, code)],
            live: None,
            vm: m,
        }
    }

    /// `[05 60]` - a long wait, then restart: an NPC standing between legs.
    const WAIT: [u8; 3] = [0x05, 0x60, 0x01];
    /// `[03 00 04]` - a four-tile directional leg, then restart.
    const WALK: [u8; 4] = [0x03, 0x00, 0x04, 0x01];

    #[test]
    fn the_kick_reloads_the_pair_from_the_standing_byte() {
        let mut w = World::new();
        let mut c = channel(4, [0x0B, 0x0C], WAIT.to_vec());
        c.vm.requested_move = Some(0x0C);
        c.vm.move_pair = Some(0x0C);
        w.npcs.ambient.insert(4, c);
        assert_eq!(w.kick_field_npc_motion_pause(), 1);
        let vm = &w.npcs.ambient[&4].vm;
        assert_eq!(vm.requested_move, Some(0x0B));
        assert_eq!(vm.move_pair, Some(0x0B));
        assert!(
            w.npcs.anim_cues.is_empty(),
            "the kick writes the pair; the consumer plays it"
        );
    }

    #[test]
    fn an_unset_record_and_a_classless_actor_are_left_alone() {
        let mut w = World::new();
        let unset = vm::ambient_motion::DEFAULT_MOVE_UNSET;
        w.npcs
            .ambient
            .insert(2, channel(2, [unset, unset], WAIT.to_vec()));
        let mut bare = channel(3, [0x0B, 0x0C], WAIT.to_vec());
        bare.vm.actor_flags = 0;
        bare.vm.requested_move = Some(0x0C);
        w.npcs.ambient.insert(3, bare);
        assert_eq!(w.kick_field_npc_motion_pause(), 0);
        assert_eq!(w.npcs.ambient[&2].vm.requested_move, None);
        assert_eq!(w.npcs.ambient[&3].vm.requested_move, Some(0x0C));
    }

    /// A walker parked in a wait with its walk clip still up: the kick's
    /// request survives the tick's ops and the consumer plays the standing
    /// move.
    #[test]
    fn a_parked_walker_switches_to_its_standing_clip() {
        let mut w = World::new();
        w.npcs.animate = true;
        let mut c = channel(4, [0x0B, 0x0C], WAIT.to_vec());
        c.vm.requested_move = Some(0x0C);
        w.npcs.ambient.insert(4, c);
        w.npcs.clip_current.insert(4, 0x0C);
        w.kick_field_npc_motion_pause();
        w.tick_field_npc_ambient();
        assert_eq!(w.npcs.anim_cues.get(&4).map(|c| c.1), Some(0x0B));
    }

    /// A walker mid-leg: the next step's prologue requests the walk anim
    /// again before the consumer runs, so the kick leaves no trace - and the
    /// walk clip, already playing, is not restarted either.
    #[test]
    fn a_walker_mid_leg_keeps_walking_its_clip() {
        let mut w = World::new();
        w.npcs.animate = true;
        w.npcs
            .ambient
            .insert(5, channel(5, [0x0B, 0x0C], WALK.to_vec()));
        w.tick_field_npc_ambient();
        assert_eq!(
            w.npcs.anim_cues.get(&5).map(|c| c.1),
            Some(0x0C),
            "the first step requests the walk anim"
        );
        let _ = w.drain_field_anim_cues(None, None, |_| None);
        assert_eq!(w.npcs.clip_current.get(&5), Some(&0x0C));
        w.kick_field_npc_motion_pause();
        w.tick_field_npc_ambient();
        assert!(
            w.npcs.anim_cues.is_empty(),
            "the step re-requested the playing walk anim: no restart"
        );
    }

    /// The consumer's change test on its own: a walker requests its walk
    /// anim on every step, and only the first request restarts the clip.
    #[test]
    fn a_walk_cycle_is_not_restarted_on_every_step() {
        let mut w = World::new();
        w.npcs.animate = true;
        w.npcs
            .ambient
            .insert(5, channel(5, [0x0B, 0x0C], WALK.to_vec()));
        w.tick_field_npc_ambient();
        assert!(w.npcs.anim_cues.contains_key(&5));
        let _ = w.drain_field_anim_cues(None, None, |_| None);
        for _ in 0..3 {
            w.tick_field_npc_ambient();
            assert!(w.npcs.anim_cues.is_empty());
        }
    }

    /// With the liveliness off no walk is published, and no clip either.
    #[test]
    fn no_clip_requests_with_the_liveliness_off() {
        let mut w = World::new();
        w.npcs
            .ambient
            .insert(5, channel(5, [0x0B, 0x0C], WALK.to_vec()));
        w.tick_field_npc_ambient();
        assert!(w.npcs.anim_cues.is_empty());
    }
}
