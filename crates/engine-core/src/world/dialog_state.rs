//! Field dialogue state: the simplified dialog panel, the inline field-VM dialogue runner and the interact / talk latches.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Field dialogue state: the simplified dialog panel, the inline field-VM dialogue runner and the interact / talk latches.
pub struct DialogState {
    /// Active dialog request - populated by the field-VM op 0x3F handler,
    /// cleared by the engine after the user dismisses the box. The MES
    /// renderer reads `text_id` + `inline`; the world-coords + depth feed
    /// the box placement.
    pub current: Option<DialogRequest>,
    /// Active 3-actor talk session (field-VM op `0x43` sub-2; retail talk
    /// controller from `FUN_801D2D38`). Refreshed on every sub-2
    /// instruction; the paired system flag `0xD` is the retail talk-active
    /// lock. See [`ThreeActorTalk`].
    pub three_actor_talk: Option<ThreeActorTalk>,
    /// Host-latched "switch character" request for the active three-actor
    /// talk - the engine input standing in for retail's pad-derived word
    /// `_DAT_8007B874` bit `0x80` (the request route of `FUN_801D27E0`'s
    /// state-0 arm gate). Hosts latch it from their pad handler via
    /// [`crate::world::World::request_talk_leader_switch`]; the controller poll
    /// ([`crate::world::World::tick_three_actor_talk`]) consumes it on its next state-0
    /// frame and drops it when no talk is live.
    pub talk_switch_requested: bool,
    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,
    /// The interaction-prologue record for the dialogue [`crate::world::World::trigger_field_interact`]
    /// most recently opened (taken by [`crate::world::World::drive_inline_dialogue`] when it
    /// starts the runner). `None` when the opened NPC has no prologue record.
    pub active_inline_prologue: Option<crate::man_field_scripts::InlineDialogPrologue>,
    /// While [`crate::world::World::step_inline_dialogue`] is stepping the field VM over an
    /// NPC's interaction record, this carries that NPC's placement slot so the
    /// `0x4C 0x51` NPC-run host hook can route the walk to the right actor
    /// (the engine's stand-in for retail's per-actor script context pointer).
    pub stepping_inline_npc: Option<u8>,
    /// The placement slot [`crate::world::World::trigger_field_interact`] most recently
    /// opened a dialogue for; consumed by [`crate::world::World::drive_inline_dialogue`] so
    /// the inline runner knows which NPC its record belongs to.
    pub active_inline_slot: Option<u8>,
    /// Per-tick guard: set when a Cross/Circle press is consumed by a field
    /// dialogue open or dismiss this tick, so the script's `0x4C` dialog poll
    /// and the interaction probe can't both act on the same edge (double
    /// open/dismiss). Reset at the top of each [`crate::world::SceneMode::Field`] tick.
    pub input_consumed: bool,
    /// A running inline interaction script driven through the field VM (the
    /// faithful dialogue path). Opt-in alternative to the simplified
    /// [`crate::world::DialogState::current`] / `OwnedDialogPanel` path: it *executes* the
    /// prologue flag tests, branch flag-sets, and scene changes between text
    /// boxes. See [`crate::inline_dialogue`] and [`crate::world::World::step_inline_dialogue`].
    pub inline: Option<crate::inline_dialogue::InlineDialogue>,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            current: None,
            three_actor_talk: None,
            talk_switch_requested: false,
            last_field_interact: None,
            active_inline_prologue: None,
            stepping_inline_npc: None,
            active_inline_slot: None,
            input_consumed: false,
            inline: None,
        }
    }
}

impl Default for DialogState {
    fn default() -> Self {
        Self::new()
    }
}
