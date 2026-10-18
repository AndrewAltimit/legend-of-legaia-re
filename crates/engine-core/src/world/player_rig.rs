//! Where the player's rig mesh comes from.
//!
//! A field scene seats the player on the lead's field form, the PROT 0874 §0
//! member the global pool holds for that roster slot. Op `4C 50` aimed at the
//! player (`CC F8 50 lo hi`) re-stages the player object onto another pool
//! model (`FUN_80024E08`), and the operand resolves like every other model id
//! ([`legaia_engine_scene::model_bank::resolve_model_id`]): at or above `0xF0` a player-bank
//! slot, below it a scene-bank model. The disc's four sites are `jagaroom`'s
//! two `CC F8 50 26 00` and `urudre1`'s `5D 00` then `F0 00`.
//!
//! [`World::player_rig_source`] is the id resolution; the mesh a host builds
//! from it is `legaia_engine_scene_host::scene_host::SceneHost::player_rig_mesh`.
//! The change signal is [`World::take_player_rig_change`].

use super::World;

/// Where the player's rig mesh comes from - the id resolution half of
/// `SceneHost::player_rig_mesh`, which needs no loaded scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRigSource {
    /// A PROT 0874 §0 slot of the global pool. `model_id` is `None` for the
    /// lead's own field form, `Some(0xF0 + slot)` for a scripted re-stage.
    PartySlot { slot: usize, model_id: Option<i16> },
    /// A scene-bank model (a re-stage operand below `0xF0`).
    SceneModel(i16),
}

impl World {
    /// Resolve [`crate::world::FieldLocomotion::player_live_model`] into the
    /// pool the player's rig mesh comes from.
    ///
    /// REF: FUN_80024E08 (the re-stage), FUN_8003A1E4 (the shared `0xF0`
    /// split, ported as `resolve_model_id`)
    pub fn player_rig_source(&self) -> PlayerRigSource {
        let lead = usize::from(self.field_leader());
        let Some(id) = self.locomotion.player_live_model else {
            return PlayerRigSource::PartySlot {
                slot: lead,
                model_id: None,
            };
        };
        let r = legaia_engine_scene::model_bank::resolve_model_id(id);
        match r.bank {
            legaia_engine_scene::model_bank::ModelBank::Player => PlayerRigSource::PartySlot {
                slot: usize::from(r.index),
                model_id: Some(id),
            },
            legaia_engine_scene::model_bank::ModelBank::Scene => PlayerRigSource::SceneModel(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_source_follows_the_scripted_model() {
        let mut w = World::default();
        w.party.active_party = vec![2];
        assert_eq!(
            w.player_rig_source(),
            PlayerRigSource::PartySlot {
                slot: 2,
                model_id: None
            }
        );
        // `urudre1`'s closing `CC F8 50 F0 00`: player bank slot 0.
        assert!(w.field_player_set_model(0xF0));
        assert!(w.take_player_rig_change(), "the change is signalled");
        assert!(!w.take_player_rig_change(), "once");
        assert_eq!(
            w.player_rig_source(),
            PlayerRigSource::PartySlot {
                slot: 0,
                model_id: Some(0xF0)
            }
        );
        // The same operand again raises nothing.
        w.field_player_set_model(0xF0);
        assert!(!w.take_player_rig_change());
        // `jagaroom`'s `CC F8 50 26 00`: a scene-bank model.
        w.field_player_set_model(0x26);
        assert!(w.take_player_rig_change());
        assert_eq!(w.player_rig_source(), PlayerRigSource::SceneModel(0x26));
        // Scene entry restores the lead's form and lowers the signal.
        w.field_player_set_model(0x5D);
        w.reset_field_warp_and_clip();
        assert!(!w.take_player_rig_change());
        assert_eq!(
            w.player_rig_source(),
            PlayerRigSource::PartySlot {
                slot: 2,
                model_id: None
            }
        );
    }

    /// A roster op that moves the lead mid-scene (`cave01`'s
    /// `3D 00 3D 01 3D 02 3C 01`) re-sources the rig, so the hosts have to
    /// hear about it the way they hear about a `CC F8 50`.
    #[test]
    fn a_lead_change_signals_a_rig_rebuild() {
        let mut w = World::default();
        w.party.active_party = vec![0, 1, 2];
        w.reset_field_warp_and_clip();
        // Off the map a lead move waits for the return.
        w.party.active_party = vec![1];
        assert!(!w.take_player_rig_change(), "a battle's composition");
        w.party.active_party = vec![0, 1, 2];
        w.mode = legaia_engine_mode::mode::SceneMode::Field;
        assert!(!w.take_player_rig_change(), "entry built Vahn's rig");
        w.party.active_party = vec![1];
        assert!(w.take_player_rig_change(), "Noa now leads");
        assert!(!w.take_player_rig_change(), "once");
        assert_eq!(
            w.player_rig_source(),
            PlayerRigSource::PartySlot {
                slot: 1,
                model_id: None
            }
        );
        // A re-staged model owns the mesh: the lead moving under it changes
        // nothing the rig is built from.
        w.field_player_set_model(0x26);
        assert!(w.take_player_rig_change());
        w.party.active_party = vec![2];
        assert!(!w.take_player_rig_change());
    }

    /// The rig follows the leader word, not the list head: op `4C 01`
    /// (`0x801E0C70..0x801E0C88`) writes only the leader cells, and a
    /// three-actor-talk swap leaves the battle composition alone.
    #[test]
    fn the_rig_follows_the_leader_word() {
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Field,
            ..Default::default()
        };
        w.party.active_party = vec![0, 1, 2];
        assert_eq!(w.field_leader(), 0, "no leader yet: the head");
        w.reset_field_warp_and_clip();
        w.party.party_leader_slot = Some(1);
        assert!(w.take_player_rig_change(), "Noa leads the same party");
        assert_eq!(
            w.player_rig_source(),
            PlayerRigSource::PartySlot {
                slot: 1,
                model_id: None
            }
        );
        // A composition installed around the party ops that drops the
        // leader: the head stands in.
        w.party.active_party = vec![2];
        w.party.party_actor_slots = vec![Some(2)];
        assert_eq!(w.field_leader(), 2);
    }
}
