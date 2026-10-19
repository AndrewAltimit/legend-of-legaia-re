//! Which mesh the player's rig is built from.
//!
//! A field scene seats the player on the lead's field form, the PROT 0874 §0
//! member the global pool holds for that roster slot. Op `4C 50` aimed at the
//! player (`CC F8 50 lo hi`) re-stages the player object onto another pool
//! model (`FUN_80024E08`), and the operand resolves like every other model id
//! ([`legaia_engine_scene::model_bank::resolve_model_id`]): at or above `0xF0` a player-bank
//! slot, below it a scene-bank model. The disc's four sites are `jagaroom`'s
//! two `CC F8 50 26 00` and `urudre1`'s `5D 00` then `F0 00`.
//!
//! [`SceneHost::player_rig_mesh`] is that resolution for the hosts; the change
//! signal is [`legaia_engine_core::world::World::take_player_rig_change`].

use super::*;
use legaia_engine_core::world::PlayerRigSource;

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

impl SceneHost {
    /// Resolve the player's rig mesh from
    /// [`legaia_engine_core::world::FieldLocomotion::player_live_model`]
    /// ([`legaia_engine_core::world::World::player_rig_source`]): the lead's field form
    /// when no script has re-staged it, the player-bank slot `value - 0xF0`
    /// at or above `0xF0`, and the scene-bank model `value`
    /// ([`legaia_engine_scene::model_bank::SceneModelBank::tmd_bytes`]) below it.
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
