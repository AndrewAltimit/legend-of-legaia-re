//! Random / scripted encounter state: the per-scene encounter session, the scripted-encounter arm and the roll gates.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Random / scripted encounter state: the per-scene encounter session, the scripted-encounter arm and the roll gates.
pub struct EncounterState {
    /// Pending scripted-encounter install (field-VM bare arm-encounter op
    /// `0x37`/`0x41`). When that op runs and [`crate::world::EncounterState::scripted_armed`]
    /// is set, the host records the bounded record window overlaying the opcode
    /// here; the field-step driver drains it after the VM borrow ends and feeds
    /// it to [`crate::world::World::install_scripted_encounter`]. `None` between installs.
    ///
    /// Retail writes the install opcode pointer into `actor[+0x94]`
    /// (`0x801DEEDC`) and the 5-state `FUN_801DA51C` SM reads it as a formation
    /// record once it reaches the encounter-confirm state. There is no
    /// dedicated encounter opcode - the consuming entity SM is the
    /// discriminator. `scripted_encounter_armed` is the engine-side stand-in
    /// for "the active entity is an encounter carrier" until the per-scene
    /// carrier identity / SM-confirm trigger is pinned from disc bytecode.
    pub pending_scripted: Option<Vec<u8>>,
    /// When `true`, the field VM's bare arm-encounter op (`0x37`/`0x41`) is
    /// treated as a scripted-encounter install: the record window overlaying
    /// the opcode is parsed as an [`crate::encounter_record::EncounterRecord`]
    /// and installed via [`crate::world::World::install_scripted_encounter`], which then
    /// disarms (fire-once). Default `false` so generic script yields are never
    /// mistaken for encounter arms. See [`crate::world::World::arm_scripted_encounter`].
    pub scripted_armed: bool,
    /// One-shot override: a scripted/forced formation has been installed
    /// ([`crate::world::World::install_man_formation`] / [`crate::world::World::install_encounter_from_record`])
    /// and the next [`crate::world::World::on_field_step`] must fire it regardless of any
    /// per-region random rate. Retail copies the carrier's `entity[+0x94]`
    /// formation into the battle cell independent of the random-roll path
    /// (`FUN_801D9E1C`), so a 0%-random scene (e.g. town01's Rim Elm tutorial)
    /// still starts the scripted fight. Cleared when the step consumes it.
    pub scripted_formation_pending: bool,
    /// Active encounter session - bracketed transition + grace machine for
    /// step-driven random battles. `Some` when an encounter table is
    /// installed; `None` in scenes where encounters are disabled
    /// (towns / cutscenes / world-map). Engines call
    /// [`crate::world::World::on_field_step`] from the field-step path (player walks one
    /// tile) to advance the tracker; the resulting [`crate::encounter::EncounterPhase`]
    /// drives the camera-shake / fade / battle-load chain.
    pub session: Option<crate::encounter::EncounterSession>,
    /// Cached answer to [`crate::world::World::scene_can_roll_encounters`] for the scene
    /// currently installed, refreshed by
    /// [`crate::world::World::refresh_encounter_rollable`] whenever the encounter tables
    /// change. Hosts read it per frame (the underlying scan walks region
    /// AABBs, so it is not a per-frame query) to tell the player that a
    /// scene has no random encounters *by design* - several retail scenes,
    /// `town01` among them, have every rollable region shadowed by an
    /// earlier rate-0 row.
    pub scene_rollable: bool,
    /// Frames left on the "no random encounters in this scene" hint, armed by
    /// [`crate::world::World::arm_live_loop`] when the loop lands on such a scene and aged
    /// by [`crate::world::World::tick`]. Read through [`crate::world::World::show_encounter_hint`].
    pub scene_hint_frames: u16,
    /// `_DAT_8007B5FC` - the encounter step counter. One retail global, not
    /// per scene: the region trackers (field + overworld) are seeded from it
    /// when a scene installs them and write it back after every step, and the
    /// non-roll writers (field-VM op `4C EC`, the op-`0x3E` formation arm,
    /// the scene-entry top-up) set it through
    /// [`crate::world::World::set_encounter_step_counter`].
    pub step_counter: i32,
}

impl EncounterState {
    pub fn new() -> Self {
        Self {
            pending_scripted: None,
            scripted_armed: false,
            scripted_formation_pending: false,
            session: None,
            scene_rollable: false,
            scene_hint_frames: 0,
            step_counter: crate::region_encounter::ENCOUNTER_COUNTER_BASE,
        }
    }
}

impl Default for EncounterState {
    fn default() -> Self {
        Self::new()
    }
}
