//! Which mesh the player's rig is built from.
//!
//! A field scene seats the player on the lead's field form, the PROT 0874 §0
//! member the global pool holds for that roster slot. Op `4C 50` aimed at the
//! player (`CC F8 50 lo hi`) re-stages the player object onto another pool
//! model (`FUN_80024E08`), and the operand resolves like every other model id
//! ([`crate::model_bank::resolve_model_id`]): at or above `0xF0` a player-bank
//! slot, below it a scene-bank model. The disc's four sites are `jagaroom`'s
//! two `CC F8 50 26 00` and `urudre1`'s `5D 00` then `F0 00`.
//!
//! [`SceneHost::player_rig_mesh`] is that resolution for the hosts; the change
//! signal is [`crate::world::World::take_player_rig_change`].

use super::*;

/// The mesh a host builds the player's rig from.
#[derive(Debug, Clone)]
pub struct PlayerRigMesh {
    /// The parsed TMD.
    pub tmd: legaia_tmd::Tmd,
    /// Its bytes, for the VRAM mesh builders.
    pub raw: Vec<u8>,
    /// The PROT 0874 §0 slot the mesh is, when it is one: the party
    /// locomotion bank then applies to it (`slot <= 2`). `None` for a
    /// scene-bank model, which carries no locomotion rest pose.
    pub party_slot: Option<usize>,
    /// The model id the rig was resolved from, `None` for the lead's own
    /// field form.
    pub model_id: Option<i16>,
}

/// Where the player's rig mesh comes from - the id resolution half of
/// [`SceneHost::player_rig_mesh`], which needs no loaded scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRigSource {
    /// A PROT 0874 §0 slot of the global pool. `model_id` is `None` for the
    /// lead's own field form, `Some(0xF0 + slot)` for a scripted re-stage.
    PartySlot { slot: usize, model_id: Option<i16> },
    /// A scene-bank model (a re-stage operand below `0xF0`).
    SceneModel(i16),
}

impl crate::world::World {
    /// Resolve [`crate::world::FieldLocomotion::player_live_model`] into the
    /// pool the player's rig mesh comes from.
    ///
    /// REF: FUN_80024E08 (the re-stage), FUN_8003A1E4 (the shared `0xF0`
    /// split, ported as `resolve_model_id`)
    pub fn player_rig_source(&self) -> PlayerRigSource {
        let lead = self.party.active_party.first().copied().unwrap_or(0) as usize;
        let Some(id) = self.locomotion.player_live_model else {
            return PlayerRigSource::PartySlot {
                slot: lead,
                model_id: None,
            };
        };
        let r = crate::model_bank::resolve_model_id(id);
        match r.bank {
            crate::model_bank::ModelBank::Player => PlayerRigSource::PartySlot {
                slot: usize::from(r.index),
                model_id: Some(id),
            },
            crate::model_bank::ModelBank::Scene => PlayerRigSource::SceneModel(id),
        }
    }
}

impl SceneHost {
    /// Resolve the player's rig mesh from
    /// [`crate::world::FieldLocomotion::player_live_model`]
    /// ([`crate::world::World::player_rig_source`]): the lead's field form
    /// when no script has re-staged it, the player-bank slot `value - 0xF0`
    /// at or above `0xF0`, and the scene-bank model `value`
    /// ([`crate::model_bank::SceneModelBank::tmd_bytes`]) below it.
    ///
    /// `None` when the resolved slot holds nothing - a host then keeps the
    /// rig it has.
    pub fn player_rig_mesh(&self) -> Option<PlayerRigMesh> {
        match self.world.player_rig_source() {
            PlayerRigSource::PartySlot { slot, model_id } => self
                .world
                .global_tmd_pool
                .get(slot)
                .and_then(|s| s.as_ref())
                .map(|g| PlayerRigMesh {
                    tmd: g.tmd.clone(),
                    raw: g.raw.clone(),
                    party_slot: Some(slot),
                    model_id,
                }),
            PlayerRigSource::SceneModel(id) => {
                let scene = self.scene.as_ref()?;
                let raw = self.model_bank.tmd_bytes(scene, id)?;
                let tmd = legaia_tmd::parse(&raw).ok()?;
                Some(PlayerRigMesh {
                    tmd,
                    raw,
                    party_slot: None,
                    model_id: Some(id),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_source_follows_the_scripted_model() {
        let mut w = crate::world::World::default();
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
}
