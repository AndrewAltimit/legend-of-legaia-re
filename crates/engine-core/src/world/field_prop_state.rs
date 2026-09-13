//! Field props + triggers: prop colliders and bank, walk-touch records, the scene's move-VM stager tables, boss stagers, live field effects and the resolved cold spawn.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Field props + triggers: prop colliders and bank, walk-touch records, the scene's move-VM stager tables, boss stagers, live field effects and the resolved cold spawn.
pub struct FieldPropState {
    /// Static prop colliders, one per placed object of the scene's field
    /// `.MAP` object grid - the engine's source for the **actor-collision
    /// arms** of the movement probe (retail `FUN_801CFC40`). Installed at
    /// field-scene entry from
    /// [`crate::scene::Scene::field_object_placements`] (each placement's
    /// [`collider_x`](legaia_asset::field_objects::Placement::collider_x) /
    /// `collider_z` = spawn position + the record's collision-footprint
    /// offset, live-verified against the spawned static actors of catalogued
    /// captures), with each bound placement's class bits decoded from its
    /// bind record's spawn prologue. **Solid by default** - retail's placed
    /// props always enter the collision candidate list (`FUN_801CF754`)
    /// unless their script sets `+0x10 & 3`; a closed door blocks the player
    /// until its touch pass runs `31 00`.
    pub colliders: Vec<FieldPropCollider>,
    /// The cold field-entry spawn `(x, z)` the scene host resolved at entry
    /// ([`Self::resolve_cold_field_spawn`]) - a standable, reachable spot in
    /// the scene's largest walkable component. Kept so the helper-context
    /// teardown can re-seat the player here if a partially-executed spawned
    /// record left them inside a wall (see [`Self::step_helper_contexts`]).
    /// `None` outside field scenes.
    pub resolved_cold_spawn: Option<(i16, i16)>,
    /// Per-scene bank of placed-prop animation + interaction runtimes (the
    /// door swings, the searchable cupboards), keyed by the placement's
    /// footprint-anchor tile. Built at field-scene entry
    /// ([`crate::field_env::PropAnimBank::build`]); clips advance every field
    /// tick, and a touched / interacted prop's bind record runs through the
    /// field VM ([`Self::start_prop_interaction`]).
    pub bank: crate::field_env::PropAnimBank,
    /// A prop the movement probe touched this tick (the `FUN_801CFC40`
    /// static-arm hit whose result bit `4` the locomotion auto-posts through
    /// `FUN_801D5B5C`): the anchor key of the touched [`crate::world::FieldPropState::bank`]
    /// entry. Drained by [`Self::tick_prop_interactions`], which starts the
    /// record's field-VM run.
    pub pending_touch: Option<(u8, u8)>,
    /// Per-placement walk-touch events, keyed by placement `slot`: the
    /// placements whose script fires on body contact (door warps, player
    /// throw-back teleports - [`crate::man_field_scripts::placement_walk_touch_event`]),
    /// with the placement's spawn position as the contact-box centre. The
    /// locomotion's per-step touch dispatch (`Self::check_field_walk_touch`)
    /// posts these without a button press - retail's `FUN_801d5b5c` auto
    /// event post on the static-entity collision arm.
    pub walk_touch: std::collections::BTreeMap<u8, ((i16, i16), WalkTouchEvent)>,
    /// For each `.MAP`-object door bind ([`Self::install_trigger_walk_touch`]),
    /// the **flat** MAN record index the object's script is. A door record is a
    /// field-VM script whose opening `SysFlag.Test` chain selects the arm that
    /// runs, so the effect is re-resolved against the live story flags at
    /// contact time ([`crate::man_field_scripts::resolve_walk_touch_event`])
    /// rather than frozen at scene load; the `field_walk_touch` entry keeps the
    /// structural decode as the fallback.
    pub walk_touch_records: std::collections::BTreeMap<u8, usize>,
    /// Walk-touch edge latch: the slot whose contact box the player currently
    /// stands in, so a sustained press posts its event once (retail gates the
    /// per-step post on the player's `+0x10 & 0x80000` engaged flag, cleared
    /// by the dialog SM teardown - the engine latches per contact instead).
    pub active_walk_touch: Option<u8>,
    /// The current scene's **field move-VM stager table** - the prescript
    /// records (`scene_event_scripts` / `scene_v12_table` offset `0x800`) parsed
    /// as summon-format move-VM stager records, the field-resident sibling of the
    /// per-summon stagers (see `docs/formats/scene-v12-table.md` +
    /// `legaia_asset::scene_event_scripts::move_stager_records`). The field VM's
    /// op `0x34` sub-3 ("Play 3D animation") installs one by id through
    /// `FUN_800252EC` → the part-stager `FUN_80021B04` → the move VM; the engine
    /// mirrors that in [`World::spawn_field_stager`]. Empty until
    /// [`World::install_field_stagers`] runs at scene entry. Distinct from the
    /// field-VM bytecode the scene also runs (`field_bytecode`); these records are
    /// the move-VM side of the same prescript bundle.
    pub stagers: Vec<legaia_asset::summon_overlay::SummonPart>,
    /// The prescript bundle bytes the [`field_stagers`](Self::field_stagers)
    /// records index into (needed to seed a part's move buffer when spawning).
    pub stager_bytes: Vec<u8>,
    /// Live field move-VM scene-graph effects spawned by op `0x34` sub-3, each a
    /// one-part [`crate::summon::SummonScene`]; ticked by
    /// [`World::tick_field_fx`], drawn via [`World::active_field_fx_part_draws`],
    /// with the non-visual nodes (the `0x4001` sound emitter) surfaced separately
    /// through [`World::active_field_fx_render_nodes`]. A `Vec` because several
    /// can be live at once (the prescript triggers them independently).
    pub active_fx: Vec<crate::summon::SummonScene>,
    /// Boss-stager bindings for the active scene, keyed by partition-1
    /// placement slot: the record an approach (walk-touch) or interact on
    /// that placed actor runs through the field VM. Derived from the scene
    /// MAN's own bytes at entry
    /// ([`World::install_boss_stagers_from_man`]); consumed by
    /// [`World::run_boss_stager_record`] (rikuroa's Caruban stager `P1[3]`:
    /// `52 89` staged-marker SET then `3E FF 11` battle entry - every flag
    /// in the chain lands from the record's own script bytes, nothing is
    /// engine-stamped).
    pub boss_stagers: std::collections::HashMap<u8, crate::world::FieldBossStager>,
}

impl FieldPropState {
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
            resolved_cold_spawn: None,
            bank: Default::default(),
            pending_touch: None,
            walk_touch: std::collections::BTreeMap::new(),
            walk_touch_records: std::collections::BTreeMap::new(),
            active_walk_touch: None,
            stagers: Vec::new(),
            stager_bytes: Vec::new(),
            active_fx: Vec::new(),
            boss_stagers: std::collections::HashMap::new(),
        }
    }
}

impl Default for FieldPropState {
    fn default() -> Self {
        Self::new()
    }
}
