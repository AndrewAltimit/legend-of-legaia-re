//! Live model and animation overrides for NPCs and objects, and the player's
//! field position.

use super::*;

impl World {
    /// The live scene-bank model id each placed object's stream swapped in
    /// (op `0x0E`), keyed by bind record. Both hosts draw a placed object
    /// whose bind record is here with `env_pack[id]` instead of its `.MAP`
    /// pack slot; an id at or past `0xF0` (the player bank) is never a
    /// placed object's and is ignored.
    pub fn object_live_models(&self) -> &std::collections::BTreeMap<usize, i16> {
        &self.npcs.object_models
    }

    /// The look rotation (field-VM `4C 45`) an animated actor draws with
    /// this frame, or `None` while it turns nothing. Both play hosts fold it
    /// into the actor's pose with [`legaia_engine_field::actor_look::apply_look`] before
    /// they skin the mesh - the player's rig and every posed NPC.
    // REF: FUN_8001B964 (0x8001BB40..0x8001BB88)
    pub fn actor_look(
        &self,
        key: legaia_engine_field::actor_look::LookKey,
    ) -> Option<legaia_engine_field::actor_look::ActorLook> {
        self.npcs.looks.turning(key)
    }

    /// Capture alignment: set a placed object's live model where a motion
    /// stream drives it (a record outside
    /// [`crate::world::FieldNpcState::object_ambient`] is left alone). The
    /// retail comparison's image child only.
    pub fn seed_object_live_model(&mut self, record: usize, model: i16) {
        if self.npcs.object_ambient.contains_key(&record) {
            self.npcs.object_models.insert(record, model);
        }
    }

    /// The live model id the scripted-motion VM's op `0x0E` re-bound this
    /// placement slot to, or `None` while the actor still draws its spawn
    /// mesh.
    ///
    /// The **one** question a host asks per slot per scene load. Both hosts
    /// ask it: the native window's `upload_assets` and the browser's
    /// `play_npc_live_model` export, each feeding the id to
    /// [`legaia_engine_scene::model_bank::SceneModelBank::tmd_bytes`] to materialise the
    /// mesh. Retail needs no equivalent - it reloads the mesh inside
    /// `FUN_80024E08` - but the port's mesh lives on the host, so the id has
    /// to cross that boundary.
    pub fn field_npc_live_model(&self, slot: u8) -> Option<i16> {
        self.npcs.models.get(&slot).copied()
    }

    /// The clip id a placement slot's actor carries once its spawn prologue
    /// has run - retail's `actor[+0x5C]`, the word the per-actor anim tick
    /// `FUN_800204F8` binds as `record = id - 1`. The MAN placement header
    /// seeds it, but a prologue can rewrite it before the first drawn frame:
    /// every save crystal ships header anim `0` and its record sets `22`
    /// (the locomotion bundle's savepoint clip, record 21) - retail's
    /// `conc_field_card_boot` capture holds `0x16` at the crystal actor's
    /// `+0x5C` and draw kind `1` (posed). Read at the header anim byte alone,
    /// the crystal's three objects draw unposed on the origin.
    ///
    /// `None` when no channel owns the slot or the channel's id is zero (a
    /// model re-stage clears it); the host then keeps the header anim.
    // REF: FUN_800204F8 (clip bind off actor+0x5C), FUN_8003A1E4 (prologue)
    pub fn field_npc_live_anim(&self, slot: usize) -> Option<u8> {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == slot)
            .and_then(|c| u8::try_from(c.ctx.move_id).ok())
            .filter(|&id| id != 0)
    }

    /// Install a live model id on a slot, as op `0x0E` does. For a host or a
    /// test that drives the re-bind directly.
    pub fn set_field_npc_live_model(&mut self, slot: u8, id: i16) {
        self.npcs.models.insert(slot, id);
    }

    /// The player actor's live field position, or `None` when no player
    /// actor is seated (a headless scene inspection).
    pub fn player_field_position(&self) -> Option<(i16, i16)> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        if !a.active {
            return None;
        }
        Some((a.move_state.world_x, a.move_state.world_z))
    }
}
