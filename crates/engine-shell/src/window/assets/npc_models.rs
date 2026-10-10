//! Resolving the scene's NPC models outside the bulk upload: the scene ANM
//! bundle lookup, a live NPC's mesh, and the rebind after a scripted model
//! swap.

use super::*;

impl PlayWindowApp {
    /// The per-scene ANM bundle in the player-ANM frame-stream layout (the pose
    /// source `FUN_8001B964` walks): the type-`0x05` section of the scene's
    /// first PROT slot. It poses BOTH the MAN placement NPCs/props and the
    /// `.MAP` placed static objects whose bind names an anim id.
    ///
    /// `find_in_entry`'s descriptor-count seed varies per scene: town01's bundle
    /// resolves at 3, but the opening prologue scenes stash theirs deeper -
    /// opdeene (PROT entry 749), opstati (754), opurud (764) only surface it at
    /// descriptor count >= 5. Try the counts each scene needs and take the first
    /// bundle any entry yields.
    pub(in crate::window) fn find_scene_anm_bundle(
        &self,
    ) -> Option<legaia_asset::player_anm::PlayerAnmBundle> {
        self.session
            .host
            .scene
            .as_ref()
            .and_then(legaia_engine_core::npc_catalog::scene_anm_bundle)
    }

    /// Materialise one **scene-bank** model's mesh out of the loaded scene,
    /// for a scripted mesh re-bind (motion-VM op `0x0E`).
    ///
    /// `None` for an id the scene's bank does not carry and for the
    /// `>= 0xF0` player-bank arm, whose five meshes live in PROT 0874 rather
    /// than in any scene entry - the caller then keeps the placement's spawn
    /// mesh instead of drawing nothing.
    pub(super) fn live_npc_mesh(&self, id: i16) -> Option<(legaia_tmd::Tmd, Vec<u8>)> {
        let scene = self.session.host.scene.as_ref()?;
        let raw = self.session.host.model_bank.tmd_bytes(scene, id)?;
        let tmd = legaia_tmd::parse(&raw).ok()?;
        Some((tmd, raw))
    }

    /// Re-upload the scene's NPC meshes when a script re-bound one since the
    /// last upload (motion-VM op `0x0E`, recorded on
    /// `World::npcs.models`).
    ///
    /// `upload_assets` runs once per scene, so an id the world installs
    /// mid-scene would otherwise never reach the GPU. The comparison is
    /// against the id each draw was built from, so a stream that re-issues
    /// the same swap every frame uploads once.
    ///
    /// The player's rig rides the same re-upload: a `CC F8 50` that re-staged
    /// the player's model raises the world's rig-change signal
    /// (`World::take_player_rig_change`), and the upload binds the rig from
    /// `SceneHost::player_rig_mesh`.
    pub(in crate::window) fn rebind_live_npc_models(&mut self) {
        let player_changed = self.session.host.world.take_player_rig_change();
        let changed = player_changed || {
            let world = &self.session.host.world;
            self.field_npc_draws.iter().any(|d| {
                world
                    .field_npc_live_model(d.slot)
                    .is_some_and(|id| id != d.bound_model)
            })
        };
        if changed {
            self.upload_assets();
        }
    }
}
