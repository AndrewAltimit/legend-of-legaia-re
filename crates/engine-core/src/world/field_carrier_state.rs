//! Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
pub struct FieldCarrierState {
    /// Per-entity **field** state machines - the same `FUN_801DA51C` SM the
    /// overworld uses ([`vm::world_map`]), but ticked in [`crate::world::SceneMode::Field`]
    /// for the scene's MAN-placed actors. A scripted-encounter carrier (the
    /// Rim Elm Tetsu fight) sits Idle until [`crate::world::World::engage_field_carrier`]
    /// (the dialogue-accept) advances it to `Activating`; the next
    /// `Self::tick_field_carriers` then copies its formation and launches the
    /// battle, mirroring retail's state-1 `entity[+0x94]` copy + `case 2/3`
    /// fall-through battle handoff. Empty unless
    /// [`crate::world::World::install_field_carriers`] seeded them.
    pub entities: Vec<vm::world_map::WorldMapEntityCtx>,
    /// Per-carrier role config, paired by index with [`crate::world::FieldCarrierState::entities`].
    pub configs: Vec<FieldCarrierConfig>,
    /// Field carrier battle pending resolution: the MAN `formation_id` a
    /// carrier SM latched on its scene-transition this frame. Drained at the
    /// end of `Self::tick_field_carriers` to flip Field -> Battle. `None`
    /// between transitions.
    pub pending_battle: Option<u16>,
    /// Field-interact `slot` -> [`crate::world::FieldCarrierState::entities`] index, for the
    /// **scripted-encounter** carriers only. Built by
    /// [`crate::world::World::install_field_carriers_from_man`] so a field-interact on the
    /// sparring partner's placement can find its carrier and auto-arm the fight
    /// (the dialogue-accept drives the engage instead of the manual API). Plain
    /// talk NPCs are deliberately absent - interacting with them never launches
    /// a battle.
    pub slots: std::collections::HashMap<u8, usize>,
    /// A scripted-encounter carrier whose dialogue the player opened via a
    /// field-interact and which engages when that dialogue is dismissed (the
    /// accept). Set in `World::trigger_field_interact`, consumed
    /// by the dialog-advance dismiss (`op 0x4C n5 sub-4`). `None` when no
    /// scripted carrier's prompt is up.
    ///
    /// This any-accept path is used for a carrier whose dialogue has **no
    /// picker**. The Rim Elm spar's dialogue *does* (a 4-option menu whose
    /// index-2 entry "I want to practice with you." arms the fight), so it takes
    /// the faithful [`crate::world::FieldCarrierState::menu`] path instead - the engage there fires
    /// only on the fight option, matching retail (live-pinned by
    /// `autorun_tetsu_confirm.lua`: a dialog-SM inline picker, cursor at
    /// `*(0x801C6EA4)+0x0C`, confirming index 2 drives `0x03 -> 0x09 -> 0x15`).
    pub pending_engage: Option<usize>,
    /// The faithful counterpart to [`crate::world::FieldCarrierState::pending_engage`]: when the
    /// opened carrier dialogue carries a 4-option picker (the Rim Elm spar menu),
    /// this holds the live menu so the engage fires **only** on the fight option
    /// ("I want to practice with you.", picker index 2 - RE-pinned live by
    /// `autorun_tetsu_confirm.lua`), not on any accept. `None` when the carrier
    /// has no picker (then `pending_carrier_engage` keeps the any-accept path).
    pub menu: Option<CarrierMenu>,
}

impl FieldCarrierState {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            configs: Vec::new(),
            pending_battle: None,
            slots: std::collections::HashMap::new(),
            pending_engage: None,
            menu: None,
        }
    }
}

impl Default for FieldCarrierState {
    fn default() -> Self {
        Self::new()
    }
}
