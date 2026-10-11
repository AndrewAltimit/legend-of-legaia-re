//! The field VM's `FieldHost` bridge (`FieldHostImpl`) and the field-step
//! helpers it routes through.
//!
//! `FieldHost` is the sum of per-owner supertraits; each `<group>_host.rs`
//! child holds `FieldHostImpl`'s impl of one of them, and the other children
//! hold the `World` / `FieldHostImpl` bodies those impls route into.

use super::*;
use vm::field::{
    FieldActorFxHost, FieldActorHost, FieldActorSpawnHost, FieldAudioHost, FieldCameraHost,
    FieldCameraMotionHost, FieldDialogMenuHost, FieldFlagsHost, FieldHaltHost,
    FieldPartyInventoryHost, FieldPlayerHost, FieldSceneHost, FieldScreenHost, FieldTerrainHost,
};

mod actor_fx_host;
mod actor_host;
mod actor_motion;
mod actor_spawn;
mod actor_spawn_host;
mod audio_host;
mod camera_host;
mod camera_motion_host;
mod dialog_menu_host;
mod flags_host;
mod halt_host;
mod op49_submode;
mod party_inventory;
mod party_inventory_host;
mod player_host;
mod scene_host;
mod screen_and_camera;
mod screen_host;
mod script_teleport;
mod sound_cues;
mod step_routing;
mod talk;
mod terrain_host;

#[cfg(test)]
pub(in crate::world) use script_teleport::apply_script_table_teleport;
pub(in crate::world) use step_routing::*;

/// Whose drawn heading a field context's `+0x26` is.
pub(in crate::world) enum HeadingOwner {
    Player,
    Placement(u8),
}

impl World {
    /// Resolve the actor whose heading `ctx` carries: the player for the
    /// `0xF8` context outside a placement step; else the placement being
    /// stepped (its own script or a poke on it), else the NPC whose talk the
    /// inline runner runs (the runner's context carries no id of its own),
    /// else the placement channel whose context id it is.
    pub(in crate::world) fn heading_owner(&self, ctx: &FieldCtx) -> Option<HeadingOwner> {
        let channel = self.field_vm.executing_channel;
        if channel.is_none()
            && ctx.script_id == u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        {
            return Some(HeadingOwner::Player);
        }
        channel
            .or(self.dialog.stepping_inline_npc)
            .or_else(|| {
                self.channel_view()
                    .iter()
                    .find(|c| !c.object_bind && c.ctx.script_id == ctx.script_id)
                    .and_then(|c| u8::try_from(c.placement_index).ok())
            })
            .map(HeadingOwner::Placement)
    }
}

pub(in crate::world) struct FieldHostImpl<'a> {
    pub(in crate::world) world: &'a mut World,
}

impl FieldHostImpl<'_> {
    /// Whether `ctx` is the **player** for an arm whose retail form compares
    /// the executing context against the player object `_DAT_8007C364`
    /// (`0x23` at `0x801DEC7C`, `4C 51` at `0x801E1954`) rather than testing
    /// a flag. The port has no context pointers, so it reads the party-bank
    /// bit `0x01000000` the player context carries - but a placement seated
    /// with a `>= 0xF0` party model carries that bit too (`FUN_8003A1E4` ORs
    /// it in at `0x8003A2DC..0x8003A3B4`, and scripts toggle it on every
    /// gesture). A context stepped as a placement channel - its own script,
    /// a cross-context poke on it, or the scene-entry pre-run - is never the
    /// player, whatever its bit says.
    ///
    /// REF: FUN_801DE840 (the player-identity compares), FUN_8003A1E4
    fn ctx_is_player(&self, ctx: &FieldCtx) -> bool {
        ctx.flags & legaia_engine_field::field_player_clip::PARTY_BANK_FLAG != 0
            && self.world.field_vm.executing_channel.is_none()
            && !self.world.field_vm.entry_prerun
    }

    /// Whose side buffer a `4C 45` writes: the player (an op on the player
    /// stand-in context, `CC F8 45 ..`), the placement channel stepping, or
    /// nobody - a placed object's context draws through the static bracket,
    /// which never reads the look.
    fn look_key(&self, ctx: &FieldCtx) -> Option<legaia_engine_field::actor_look::LookKey> {
        use legaia_engine_field::actor_look::LookKey;
        if ctx.script_id == u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
            || self.ctx_is_player(ctx)
        {
            return Some(LookKey::Player);
        }
        if self.world.field_vm.executing_object.is_some() {
            return None;
        }
        self.world.field_vm.executing_channel.map(LookKey::Npc)
    }
}
