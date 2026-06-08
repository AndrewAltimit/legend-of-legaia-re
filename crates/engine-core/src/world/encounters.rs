//! Encounter / formation installation + per-scene monster catalog and party
//! battle-stat seeding. Split out of `world.rs` as an additional `impl World`
//! block.

use super::*;

impl World {
    /// Install an [`crate::encounter::EncounterSession`] for the current
    /// scene. Engines call this on scene-enter once the per-scene encounter
    /// table is known. `None` disables encounters for the active scene.
    pub fn set_encounter_session(&mut self, session: Option<crate::encounter::EncounterSession>) {
        self.encounter = session;
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
    /// [`crate::encounter_registry::vanilla_encounter_registry`] or a
    /// custom composition.
    pub fn install_encounter_for_scene(
        &mut self,
        registry: &crate::encounter_registry::EncounterRegistry,
        scene_label: &str,
    ) -> bool {
        match registry.resolve(scene_label) {
            Some(table) => {
                let tracker = crate::encounter::EncounterTracker::new(table.clone());
                let session = crate::encounter::EncounterSession::new(tracker);
                let nonempty = !table.is_empty();
                self.encounter = Some(session);
                nonempty
            }
            None => {
                self.encounter = None;
                false
            }
        }
    }

    /// Install a fully-built [`crate::encounter::EncounterTable`] plus its
    /// per-row [`crate::monster_catalog::FormationDef`]s as the active
    /// scene's encounter source.
    ///
    /// This is the disc-resident path: the field scene-entry flow resolves
    /// the table + formations straight from the scene's MAN asset (retail
    /// `_DAT_8007B898`) via [`crate::encounter_man::scene_encounter_from_man`]
    /// and installs them here, in place of the synthetic-pattern
    /// [`Self::install_encounter_for_scene`] fallback. The formation defs are
    /// merged into `formation_table` so the table's row-index `formation_id`s
    /// resolve to concrete monster sets at battle-load.
    ///
    /// Returns whether the installed table is non-empty (an empty table is
    /// still installed-but-quiet so engines can call [`Self::on_field_step`]
    /// without nil checks).
    pub fn install_man_encounter(
        &mut self,
        table: crate::encounter::EncounterTable,
        formations: Vec<crate::monster_catalog::FormationDef>,
    ) -> bool {
        for def in formations {
            self.formation_table.insert(def);
        }
        let nonempty = !table.is_empty();
        let tracker = crate::encounter::EncounterTracker::new(table);
        self.encounter = Some(crate::encounter::EncounterSession::new(tracker));
        nonempty
    }

    /// Replace just the monster stat catalog, leaving `formation_table`
    /// untouched. The MAN encounter source carries formation monster-ids but
    /// not stat blocks, so the host installs the stat catalog separately when
    /// the formations come from [`Self::install_man_encounter`].
    pub fn set_monster_catalog(&mut self, catalog: crate::monster_catalog::MonsterCatalog) {
        self.monster_catalog = catalog;
    }

    /// Install the per-item battle-stat modifier table (weapon / armor /
    /// accessory bonuses). Boot wires this once; [`Self::seed_party_battle_stats`]
    /// folds the equipped items onto each party combatant at battle entry.
    pub fn set_equipment_table(&mut self, table: crate::battle_stats::EquipmentTable) {
        self.equipment_table = table;
    }

    /// Seed each party combatant's battle attack / defense from the roster's
    /// live stats plus equipped-gear bonuses.
    ///
    /// For every party slot whose roster record carries real stats (live
    /// attack `> 0`), this resolves a [`crate::battle_stats::BattleStats`] from
    /// the character's base attack / UDF / LDF and the modifiers of the items
    /// in its equipment slots ([`crate::battle_stats::compute_battle_stats_default`]
    /// against [`Self::equipment_table`]), then writes
    /// [`Self::battle_attack`] (= resolved attack) and
    /// [`Self::battle_defense_split`] (= resolved UDF / LDF). Slots with a
    /// zeroed roster record are left untouched, so synthetic battles that set
    /// `battle_attack` directly keep their values.
    ///
    /// Called automatically from the live-loop battle entry; also public so a
    /// host can refresh stats after an equipment change without re-entering the
    /// battle.
    pub fn seed_party_battle_stats(&mut self) {
        let pc = self.party_count.min(3) as usize;
        for slot in 0..pc {
            let Some(rec) = self.roster.members.get(slot) else {
                continue;
            };
            let live = rec.live_stats();
            // A zeroed roster carries no real stats; don't clobber any value a
            // synthetic battle set directly.
            if live.atk == 0 {
                continue;
            }
            // Per-turn AP budget scales with the character's level (retail base
            // = 4 + level/10, capped 10). Captured before the `ap_gauges`
            // mutable borrow below; `rec` still owns the immutable roster borrow
            // through the stat fold.
            let ap_base = crate::ap_gauge::ap_base_for_level(rec.level());
            let record = crate::battle_stats::StatRecord {
                base_attack: live.atk,
                base_udf: live.udf,
                base_ldf: live.ldf,
                base_accuracy: live.agl,
                base_evasion: live.agl,
                base_spd: live.spd,
                base_int: live.int,
                equip: rec.equipment().slots,
            };
            let stats = crate::battle_stats::compute_battle_stats_default(
                &record,
                &self.equipment_table,
                &[],
            );
            if let Some(s) = self.battle_attack.get_mut(slot) {
                *s = stats.atk;
            }
            if let Some(s) = self.battle_accuracy.get_mut(slot) {
                *s = stats.acc;
            }
            if let Some(s) = self.battle_evasion.get_mut(slot) {
                *s = stats.eva;
            }
            self.set_battle_defense_split(slot as u8, Some((stats.udf, stats.ldf)));
            // Seed the AP gauge base from the captured level. Base only — the
            // round-start `reset_party_ap` refills `current_ap` to it, and Fury
            // Boost still extends from / reverts to this base. Re-seeding
            // mid-battle (a permanent stat-up item also calls this) is a no-op:
            // the level is unchanged, so the base is rewritten to the same value
            // and the live balance is untouched.
            if let Some(g) = self.ap_gauges.get_mut(slot) {
                g.set_base_ap(ap_base);
            }
        }
    }

    /// Install a formation + monster catalog pair. Boot wires this once;
    /// engines read it at battle-load time.
    pub fn set_formation_table(
        &mut self,
        table: crate::monster_catalog::FormationTable,
        catalog: crate::monster_catalog::MonsterCatalog,
    ) {
        self.formation_table = table;
        self.monster_catalog = catalog;
    }

    /// Install a [`crate::encounter_record::EncounterRecord`] decoded from
    /// an on-disc byte slice as the next encounter for the active scene.
    ///
    /// Mirrors the retail flow at `0x801DA620..0x801DA678`: the parsed
    /// record's monster ids are turned into a [`crate::monster_catalog::FormationDef`]
    /// (registered into `formation_table`), wrapped in a single-row
    /// [`crate::encounter::EncounterTable`] (rate `0xFF/256` so the next
    /// step roll always fires), and installed as the active session.
    ///
    /// Returns the synthesized `formation_id` so engines can immediately
    /// transition to battle if they want to skip the per-step roll.
    /// `None` means the record was empty (no monsters).
    pub fn install_encounter_from_record(
        &mut self,
        scene_label: &str,
        record: &crate::encounter_record::EncounterRecord,
    ) -> Option<u16> {
        if record.is_empty() {
            return None;
        }
        let formation = record.to_formation_def(scene_label);
        let formation_id = formation.formation_id;
        self.formation_table.insert(formation);

        use crate::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(scene_label);
        // Force the next roll to succeed: the record IS the encounter.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounter = Some(EncounterSession::new(tracker));
        Some(formation_id)
    }

    /// Install an already-registered per-scene formation as the next encounter,
    /// by its `formation_id`.
    ///
    /// This is the faithful model of a scripted-battle carrier entity selecting
    /// a formation **by index** into the per-scene formation table - the
    /// mechanism the Rim Elm Tetsu tutorial fight uses. The per-scene formations
    /// load from the MAN asset into a contiguous 8-byte-stride table
    /// (`[3 reserved][count][<=4 ids]`, see [`crate::encounter_record`] /
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
            .formation_table
            .formation(formation_id)
            .is_some_and(|def| !def.slots.is_empty());
        if !has_slots {
            return None;
        }
        let scene_label = self.active_scene_label.clone();

        use crate::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new(&scene_label);
        // Force the next step roll: the scripted carrier installs this formation.
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(formation_id, 1));
        let tracker = EncounterTracker::new(table);
        self.encounter = Some(EncounterSession::new(tracker));
        Some(formation_id)
    }

    /// Arm (or disarm) the scripted-encounter consumer.
    ///
    /// While armed, the field VM's bare arm-encounter op (`0x37`/`0x41`) hands
    /// the record window overlaying the opcode to the host, which parses it as
    /// an [`crate::encounter_record::EncounterRecord`] and routes it through
    /// [`Self::install_scripted_encounter`]. See
    /// [`Self::scripted_encounter_armed`] for why this gate exists (there is no
    /// dedicated encounter opcode; the consuming entity SM is the retail
    /// discriminator).
    pub fn arm_scripted_encounter(&mut self, on: bool) {
        self.scripted_encounter_armed = on;
    }

    /// Install a scripted encounter from the inline bytecode window the field
    /// VM forwarded at the bare arm-encounter op (`0x37`/`0x41`); the record
    /// overlays the opcode (`[opcode][op1][op2][count][ids..]`).
    ///
    /// The window is parsed as an [`crate::encounter_record::EncounterRecord`]
    /// (`[flag][_][_][count][ids..]`) and, when it carries at least one
    /// monster, installed against the active scene via
    /// [`Self::install_encounter_from_record`] - so the next
    /// [`Self::on_field_step`] flips Field -> Battle. Emits a
    /// [`FieldEvent::ScriptedEncounter`] for engine visibility regardless of
    /// whether the parse yielded a non-empty formation.
    ///
    /// Returns the synthesized `formation_id`, or `None` if the window did not
    /// parse into a non-empty record.
    ///
    /// PORT: FUN_801DA51C (the `[+4 + slot]` record-overlay reader)
    pub fn install_scripted_encounter(&mut self, record_bytes: &[u8]) -> Option<u16> {
        self.pending_field_events
            .push(FieldEvent::ScriptedEncounter {
                record: record_bytes.to_vec(),
            });
        let record = crate::encounter_record::EncounterRecord::parse(record_bytes)?;
        if record.is_empty() {
            return None;
        }
        let scene = self.active_scene_label.clone();
        let id = self.install_encounter_from_record(&scene, &record);
        // Fire-once: retail clears `entity[+0x94]` after the formation copy so
        // the arm fires exactly once. Disarm the engine-side carrier flag too.
        if id.is_some() {
            self.scripted_encounter_armed = false;
        }
        id
    }

    /// Field-step trigger. Engines call this once per "the player walked
    /// one map cell" (typically when the player actor's grid coord moves)
    /// to advance the encounter tracker. Returns `true` if a battle
    /// transition was triggered this step.
    ///
    /// The method is a no-op when no [`crate::encounter::EncounterSession`]
    /// is installed, when the session is not in `Idle`, or when the world
    /// is not in [`SceneMode::Field`].
    pub fn on_field_step(&mut self) -> bool {
        if !matches!(self.mode, SceneMode::Field) {
            return false;
        }
        let rng = self.next_rng();
        match self.encounter.as_mut() {
            Some(session) => session.on_step(rng),
            None => false,
        }
    }

    /// Per-frame tick of the encounter session timers. Drives the
    /// `Transition` and `Grace` countdowns.
    pub fn tick_encounter(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.tick_frame();
        }
    }

    /// Return the resolved [`crate::monster_catalog::FormationDef`] for the
    /// currently-triggered encounter, if any. Engines call this after the
    /// session reports `Triggered` to drain the roll and resolve into a
    /// concrete monster set; the session advances to `Battling` as a
    /// side-effect.
    pub fn drain_encounter_formation(&mut self) -> Option<crate::encounter::EncounterRoll> {
        self.encounter.as_mut().and_then(|s| s.drain_triggered())
    }

    /// Mark that the active battle finished. Engines call this from the
    /// post-battle resolution path so the session enters its grace window
    /// (suppresses encounters for `grace_frames` frames).
    pub fn end_encounter_battle(&mut self) {
        if let Some(session) = self.encounter.as_mut() {
            session.end_battle();
        }
    }
}
