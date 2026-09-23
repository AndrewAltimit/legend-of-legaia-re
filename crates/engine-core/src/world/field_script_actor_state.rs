//! Field script actors: the pool actors the field VM spawns onto a target
//! actor - the op `0x43` sub-0/1/A/B **arc jump** (`FUN_801D25EC` and its two
//! records) and the op `0x34` sub-1 **attached light** (`FUN_801E5668`,
//! ticked by `FUN_801E4470`).
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit; the behaviour lives in `world/field_script_actors.rs`.

use legaia_engine_vm::field_actor_billboard::AttachedSprite;
use legaia_engine_vm::field_ledge_hop_arc::{HopArc, HopEmitter};

/// The actor a field-script pool actor rides - retail's `+0x90` back-link.
///
/// The engine has no field actor pool: the player is its actor slot and a
/// field NPC is its placement slot, so the link names one of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptActorRef {
    /// `_DAT_8007C364` - the player (extended channel `0xF8`).
    Player,
    /// A field NPC by placement slot (`World::npcs.positions` key).
    Npc(u8),
}

/// One in-flight scripted arc: the `0x801F227C` arc helper and the
/// `0x801F22AC` release watcher `FUN_801D25EC` allocates, as one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldScriptArc {
    /// The arced actor (the arc helper's `+0x90`).
    pub actor: ScriptActorRef,
    /// The Bezier clip.
    pub arc: HopArc,
    /// The watcher's fields.
    pub watcher: HopEmitter,
    /// The watcher's `+0x94` as the engine can name it: the placement slot
    /// whose script channel(s) the landing un-halts - the arced NPC's own, or
    /// for a player arc the channel that ran the op (retail halts the caller
    /// with the player). `None` for a caller outside the channel set (the
    /// cutscene timeline, which parks on the arc instead).
    pub release_channel: Option<u8>,
}

/// One live attached light.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAttachedLight {
    /// The actor the light rides (`+0x90`).
    pub parent: ScriptActorRef,
    /// The light's own record.
    pub sprite: AttachedSprite,
}

/// Field script actors: scripted arcs, the height channel they leave NPCs
/// on, and attached lights.
#[derive(Debug, Clone, Default)]
pub struct FieldScriptActorState {
    /// In-flight arcs, in spawn order.
    pub arcs: Vec<FieldScriptArc>,
    /// A field NPC's height, keyed by placement slot, as `(x, z, y)`: the
    /// arc writes all three of the actor's `+0x14..+0x18` every frame, and
    /// the NPC position map carries X / Z only. The Y stays authoritative for
    /// as long as the NPC stands on the `(x, z)` it was written at - a walk
    /// leg that moves it hands the height back to the floor sampler.
    pub npc_heights: std::collections::BTreeMap<u8, (i16, i16, i16)>,
    /// Live attached lights, in spawn order.
    pub lights: Vec<FieldAttachedLight>,
}
