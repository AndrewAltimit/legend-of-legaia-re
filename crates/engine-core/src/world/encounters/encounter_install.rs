//! Installing a scene's random-encounter session: from the scene, the MAN
//! record or a bracket, plus the formation table and forced encounters.

use super::*;

impl World {
    /// Install an [`legaia_engine_battle::encounter::EncounterSession`] for the current
    /// scene. Engines call this on scene-enter once the per-scene encounter
    /// table is known. `None` disables encounters for the active scene.
    pub fn set_encounter_session(
        &mut self,
        session: Option<legaia_engine_battle::encounter::EncounterSession>,
    ) {
        self.encounters.session = session;
    }

    /// Install an encounter session resolved from a registry against the
    /// given CDNAME label. Engines call this from the scene-load path so
    /// every scene gets its retail-mapped encounter table without having
    /// to plumb tables through the boot config.
    ///
    /// Returns `true` when the registry resolved a non-empty table and
    /// installed it, `false` when no rule matched (or the resolved table
    /// has `trigger_rate_q8 == 0` - in which case the session is
    /// installed-but-quiet so the engine can still call `on_field_step`
    /// without nil checks).
    ///
    /// The on-disc resolver (reading the per-scene encounter table out of
    /// `0865_battle_data`) lands once a runtime watchpoint trace pins the
    /// table offset. Engines currently feed the registry from
    /// [`legaia_engine_battle::encounter_registry::vanilla_encounter_registry`] or a
    /// custom composition.
    pub fn install_encounter_for_scene(
        &mut self,
        registry: &legaia_engine_battle::encounter_registry::EncounterRegistry,
        scene_label: &str,
    ) -> bool {
        match registry.resolve(scene_label) {
            Some(table) => {
                let tracker = legaia_engine_battle::encounter::EncounterTracker::new(table.clone());
                let session = legaia_engine_battle::encounter::EncounterSession::new(tracker);
                let nonempty = !table.is_empty();
                self.encounters.session = Some(session);
                nonempty
            }
            None => {
                self.encounters.session = None;
                false
            }
        }
    }

    /// Install a fully-built [`legaia_engine_battle::encounter::EncounterTable`] plus its
    /// per-row [`legaia_engine_battle::monster_catalog::FormationDef`]s as the active
    /// scene's encounter source.
    ///
    /// This is the disc-resident path: the field scene-entry flow resolves
    /// the table + formations straight from the scene's MAN asset (retail
    /// `_DAT_8007B898`) via [`legaia_engine_battle::encounter_man::scene_encounter_from_man`]
    /// and installs them here, in place of the synthetic-pattern
    /// [`Self::install_encounter_for_scene`] fallback. The formation defs are
    /// merged into `formation_table` so the table's row-index `formation_id`s
    /// resolve to concrete monster sets at battle-load.
    ///
    /// Returns whether the installed table is non-empty (an empty table is
    /// still installed-but-quiet so engines can call [`Self::on_field_step`]
    /// without nil checks).
    ///
    /// The two id spaces are cross-checked here rather than at battle-load:
    /// a table row whose `formation_id` has no registered def would otherwise
    /// roll fine and then evaporate in [`Self::begin_encounter_battle`], which
    /// is a fight that never happens with nothing on screen to say why.
    pub fn install_man_encounter(
        &mut self,
        table: legaia_engine_battle::encounter::EncounterTable,
        formations: Vec<legaia_engine_battle::monster_catalog::FormationDef>,
    ) -> bool {
        for def in formations {
            self.tables.formation_table.insert(def);
        }
        for entry in &table.entries {
            if self
                .tables
                .formation_table
                .formation(entry.formation_id)
                .is_none_or(|d| d.slots.is_empty())
            {
                log::warn!(
                    "encounter: scene '{}' table row {} names formation {} which resolves to no \
                     monsters - a roll landing there would enter no battle",
                    table.scene_label,
                    entry.formation_id,
                    entry.formation_id
                );
            }
        }
        let nonempty = !table.is_empty();
        let tracker = legaia_engine_battle::encounter::EncounterTracker::new(table);
        self.encounters.session = Some(legaia_engine_battle::encounter::EncounterSession::new(
            tracker,
        ));
        nonempty
    }

    /// Install a **bare** [`legaia_engine_battle::encounter::EncounterSession`] for the
    /// active scene: an empty mean-rate table (which therefore never rolls of
    /// its own accord) wrapped in the transition / grace state machine.
    ///
    /// This is the sink a roll that came from somewhere *else* needs - the
    /// per-region tracker ([`legaia_engine_battle::region_encounter::RegionEncounterTracker`],
    /// the faithful `FUN_801D9E1C` model) and the
    /// [`Self::force_encounter`] harness both own their own pick and only want
    /// the bracketing. Installing it costs nothing and turns "the session was
    /// cleared under us" from a lost battle into a recoverable state.
    pub fn install_encounter_bracket(&mut self) {
        let label = self.active_scene_label.clone();
        let table = legaia_engine_battle::encounter::EncounterTable::new(label);
        debug_assert!(table.is_empty(), "the bracket table must never self-roll");
        let tracker = legaia_engine_battle::encounter::EncounterTracker::new(table);
        self.encounters.session = Some(legaia_engine_battle::encounter::EncounterSession::new(
            tracker,
        ));
        // `scene_can_roll_encounters` answers `false` while no session is
        // installed, so the cached answer (and the "no random encounters in
        // this scene" hint a host draws from it) has to be re-taken now that
        // one is.
        self.refresh_encounter_rollable();
        log::debug!(
            "encounter: installed a bare transition bracket for '{}'",
            self.active_scene_label
        );
    }

    /// Force the registered formation `formation_id` into battle **through the
    /// ordinary encounter path**: the roll is handed to the session's
    /// transition state machine exactly as a region roll is, so the intro
    /// transition, the battle BGM swap and
    /// [`Self::begin_encounter_battle`] all run unchanged.
    ///
    /// This is the engine side of the `play-window --battle <row>` harness:
    /// reaching a battle by walking is a probabilistic several-thousand-step
    /// affair, which makes verifying anything on the battle screen expensive
    /// and flaky. It deliberately does **not** shortcut into
    /// [`Self::enter_battle_from_formation`] - a harness that skips the path
    /// it is meant to exercise proves nothing about it.
    ///
    /// `formation_id` is the scene's MAN formation-row index - the same id
    /// space [`Self::install_man_encounter`] registers and the region roll
    /// produces. Returns `false` (changing nothing) when the row is not
    /// registered or carries no monsters.
    ///
    /// Clears any post-battle grace / suppression first so a second call
    /// straight after a fight still lands.
    pub fn force_encounter(&mut self, formation_id: u16) -> bool {
        let Some(def) = self.tables.formation_table.formation(formation_id) else {
            log::error!(
                "encounter: forced formation {formation_id} is not registered for scene '{}' \
                 (registered rows: {:?})",
                self.active_scene_label,
                self.registered_formation_ids()
            );
            return false;
        };
        if def.slots.is_empty() {
            log::error!(
                "encounter: forced formation {formation_id} carries no monsters - retail's reader \
                 clears the formation cell for such a row and spawns nothing"
            );
            return false;
        }
        if self.encounters.session.is_none() {
            self.install_encounter_bracket();
        }
        if let Some(session) = self.encounters.session.as_mut() {
            session.reset();
            session.tracker_mut().clear_suppression();
        }
        if let Some(t) = self.terrain.region_tracker.as_mut() {
            t.clear_suppression();
        }
        let roll = legaia_engine_battle::encounter::EncounterRoll {
            formation_id,
            row_index: formation_id as usize,
            roll_q8: 0,
        };
        let armed = self
            .encounters
            .session
            .as_mut()
            .map(|s| s.trigger_with(roll))
            .unwrap_or(false);
        if armed {
            log::info!(
                "encounter: forced formation row {formation_id} in '{}' -> transition armed",
                self.active_scene_label
            );
        }
        armed
    }

    /// Every `formation_id` currently registered in [`crate::world::DiscTables::formation_table`],
    /// sorted. The answer to "which rows can `--battle` name" and the list the
    /// unresolved-formation diagnostics print.
    pub fn registered_formation_ids(&self) -> Vec<u16> {
        let mut ids: Vec<u16> = self.tables.formation_table.by_id.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// The lowest registered formation row that actually carries monsters, or
    /// `None` when the scene registered none. What `--battle first` resolves.
    pub fn first_rollable_formation_id(&self) -> Option<u16> {
        self.registered_formation_ids().into_iter().find(|id| {
            self.tables
                .formation_table
                .formation(*id)
                .is_some_and(|d| !d.slots.is_empty())
        })
    }

    /// Install a formation + monster catalog pair. Boot wires this once;
    /// engines read it at battle-load time.
    pub fn set_formation_table(
        &mut self,
        table: legaia_engine_battle::monster_catalog::FormationTable,
        catalog: legaia_engine_battle::monster_catalog::MonsterCatalog,
    ) {
        self.tables.formation_table = table;
        self.tables.monster_catalog = catalog;
    }

    /// Install a [`legaia_scene_formats::encounter_record::EncounterRecord`] decoded from
    /// an on-disc byte slice as the next encounter for the active scene.
    ///
    /// Mirrors the retail flow at `0x801DA620..0x801DA678`: the parsed
    /// record's monster ids are turned into a [`legaia_engine_battle::monster_catalog::FormationDef`]
    /// (registered into `formation_table`), wrapped in a single-row
    /// [`legaia_engine_battle::encounter::EncounterTable`] (rate `0xFF/256` so the next
    /// step roll always fires), and installed as the active session.
    ///
    /// Returns the synthesized `formation_id` so engines can immediately
    /// transition to battle if they want to skip the per-step roll.
    /// `None` means the record was empty (no monsters).
    pub fn install_encounter_from_record(
        &mut self,
        scene_label: &str,
        record: &legaia_scene_formats::encounter_record::EncounterRecord,
    ) -> Option<u16> {
        if record.is_empty() {
            return None;
        }
        let formation = legaia_engine_battle::encounter_formation::formation_def_from_record(
            record,
            scene_label,
        );
        let formation_id = formation.formation_id;
        self.tables.formation_table.insert(formation);

        use legaia_engine_battle::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(scene_label);
        // Force the next roll to succeed: the record IS the encounter.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounters.session = Some(EncounterSession::new(tracker));
        // One-shot override: fire on the next field step regardless of any
        // installed per-region random tracker.
        self.encounters.scripted_formation_pending = true;
        Some(formation_id)
    }

    /// Install an already-registered per-scene formation as the next encounter,
    /// by its `formation_id`.
    ///
    /// This is the faithful model of a scripted-battle carrier entity selecting
    /// a formation **by index** into the per-scene formation table - the
    /// mechanism the Rim Elm Tetsu tutorial fight uses. The per-scene formations
    /// load from the MAN asset into a contiguous 8-byte-stride table
    /// (`[3 reserved][count][<=4 ids]`, see [`legaia_scene_formats::encounter_record`] /
    /// `docs/formats/encounter.md`); a carrier entity arms its encounter by
    /// pointing `actor[+0x94]` at one entry (`table_base + index*8`), and
    /// `FUN_801DA51C` copies that record into the formation cell on confirm. The
    /// id 0x4F that lands in the cell is **not** an inline script literal - it is
    /// the `monster_id` of town01 MAN `formation_id` 4, already registered by
    /// [`Self::install_man_encounter`] at scene entry (with its real archive
    /// stats merged).
    ///
    /// Unlike [`Self::install_encounter_from_record`], this re-encodes nothing:
    /// it forces the existing table row, so the scene's merged monster stats
    /// stand. Returns the `formation_id` installed, or `None` when it isn't
    /// registered or has no slots.
    ///
    /// REF: FUN_801DA51C
    pub fn install_man_formation(&mut self, formation_id: u16) -> Option<u16> {
        let has_slots = self
            .tables
            .formation_table
            .formation(formation_id)
            .is_some_and(|def| !def.slots.is_empty());
        if !has_slots {
            return None;
        }
        let scene_label = self.active_scene_label.clone();

        use legaia_engine_battle::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(&scene_label);
        // Force the next step roll: the scripted carrier installs this formation.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounters.session = Some(EncounterSession::new(tracker));
        // One-shot override: fire on the next field step even when a per-region
        // random tracker is installed (town01 is 0% random). See the field.
        self.encounters.scripted_formation_pending = true;
        Some(formation_id)
    }
}
