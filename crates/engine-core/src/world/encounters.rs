//! Encounter / formation installation + per-scene monster catalog and party
//! battle-stat seeding. Split out of `world.rs` as an additional `impl World`
//! block.

use super::*;

/// A boss-stager binding installed for the active scene: the partition-1
/// record an approach/interact on the placement slot runs, plus its park
/// gate. See [`World::install_boss_stagers_from_man`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldBossStager {
    /// Partition-1 record index (= the placement index) to execute.
    pub record: u8,
    /// The record's own head gate: refuse the launch while this system flag
    /// is SET (the beaten-boss one-shot - e.g. rikuroa's `0x142`).
    pub park_gate: Option<u16>,
}

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

    /// Install the accessory ("Goods") passive-effect catalog (item id →
    /// passive index + party-wide scope flags, decoded from the executable -
    /// see [`crate::accessory_passives::AccessoryPassives::from_disc`]).
    /// Boot wires this once; [`Self::refresh_party_ability_bits`] derives the
    /// per-character ability bitfields from it and
    /// [`Self::seed_party_battle_stats`] applies the percent stat boosts.
    pub fn set_accessory_passives(&mut self, p: crate::accessory_passives::AccessoryPassives) {
        self.accessory_passives = p;
    }

    /// Install the pause-menu text tables (item names/descriptions, spell
    /// names/descriptions, accessory passive lines) from a `SCUS_942.54`
    /// image. Boot wires this once when the executable is reachable; the
    /// Items / Magic pause screens read it through
    /// [`crate::pause_screens::MenuTextTables`].
    pub fn install_menu_text(&mut self, scus: &[u8]) {
        self.menu_text = Some(crate::pause_screens::MenuTextTables::from_scus(scus));
    }

    /// Rebuild every party member's ability bitfield from their equipped
    /// items, plus the party-global mask.
    ///
    /// PORT: FUN_80042558 (ability-bit rebuild + global-mask OR arms)
    ///
    /// Mirrors the retail aggregator's bitfield pass: each member's record
    /// `+0xF4` 4×u32 field is zeroed and re-derived from the eight equipment
    /// slots ([`crate::accessory_passives::AccessoryPassives::bits_for_equipment`]),
    /// then all members' words OR into the global mask (the engine's
    /// [`Self::party_ability_mask`], mirroring `DAT_80074358`). The rebuilt
    /// word 0 also lands in [`Self::character_ability_bits`] - the mask the
    /// MP-cost consumers read (`Self::build_battle_spell_session` /
    /// `cast_spell_on_slots` / the battle-action VM host) - together with the
    /// party-wide-scoped bits any member contributes, so a party-wide passive
    /// is visible through every member's effective mask.
    ///
    /// No-op while [`Self::accessory_passives`] is empty (the disc-free
    /// default), so synthetic setups that write `character_ability_bits`
    /// directly keep their values.
    ///
    /// Called from [`Self::seed_party_battle_stats`] (battle entry / stat
    /// refresh) and from the equip-commit paths, mirroring retail's
    /// rebuild-on-every-aggregator-pass behaviour.
    pub fn refresh_party_ability_bits(&mut self) {
        use crate::accessory_passives::ABILITY_WORDS;
        if self.accessory_passives.is_empty() {
            return;
        }
        let pc = (self.party_count.min(3) as usize).min(self.roster.members.len());
        let mut own = [[0u32; ABILITY_WORDS]; 3];
        let mut global = [0u32; ABILITY_WORDS];
        for (slot, own_words) in own.iter_mut().enumerate().take(pc) {
            // `slot` is the battle ordinal; the equipment that sources the
            // bits belongs to the character occupying it.
            let rslot = self.party_roster_slot(slot);
            let Some(member) = self.roster.members.get(rslot) else {
                continue;
            };
            let equip = member.equipment().slots;
            let words = self.accessory_passives.bits_for_equipment(&equip);
            // Rebuild the record-side bitfield (retail zeroes `+0xF4..+0x103`
            // and re-derives it from equipment on every pass). Bytes past the
            // four words stay zero - passive indices live below 0x40.
            let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
            for (w, word) in words.iter().enumerate() {
                bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            self.roster.members[rslot].set_ability_bits(bytes);
            for (g, w) in global.iter_mut().zip(words.iter()) {
                *g |= *w;
            }
            *own_words = words;
        }
        self.party_ability_mask = global;
        // Effective per-member mask for the u32 consumers: own bits plus the
        // party-wide-scoped bits any member contributes (the engine shape of
        // "consumers test the global mask for party-wide passives").
        let pw = self.accessory_passives.party_wide_mask();
        for (bits, own_words) in self
            .character_ability_bits
            .iter_mut()
            .zip(own.iter())
            .take(pc)
        {
            *bits = own_words[0] | (global[0] & pw[0]);
        }
    }

    /// `true` when any party member's rebuilt ability bitfield carries
    /// passive bit `index` (0..=127; retail uses 0..=0x3F).
    ///
    /// PORT: FUN_800431D0
    ///
    /// The port of retail's global-mask bit test
    /// (`DAT_80074358[index >> 5] & 1 << (index & 0x1F)`), against the
    /// engine's [`Self::party_ability_mask`]. Point-of-use consumers of the
    /// party-wide passives (encounter rate, escape, battle-end rewards) call
    /// this.
    pub fn party_has_ability(&self, index: u8) -> bool {
        self.party_ability_mask
            .get((index >> 5) as usize)
            .is_some_and(|w| w & (1u32 << (index & 0x1F)) != 0)
    }

    /// Resolve the current accessory / status encounter-rate modifiers
    /// (`FUN_801D9E1C`'s four pre-roll tests): the High/Low Encounter
    /// passives (ability bits `0x3B` / `0x3C`, via
    /// [`Self::party_has_ability`] = `FUN_800431D0`) and system flags
    /// `0x1D` / `0x1E` (via [`Self::system_flag_test`] = `FUN_8003CE64`).
    /// Refreshed onto the active region tracker before each step roll.
    pub fn encounter_rate_modifiers(&self) -> crate::region_encounter::EncounterRateModifiers {
        crate::region_encounter::EncounterRateModifiers {
            high_encounter: self.party_has_ability(0x3B),
            low_encounter: self.party_has_ability(0x3C),
            flag_high: self.system_flag_test(0x1D),
            flag_low: self.system_flag_test(0x1E),
        }
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
        // Rebuild the per-member ability bitfields (accessory passives) first
        // so both the stat fold below and the MP-cost consumers read fresh
        // equipment-derived bits, mirroring retail's single-pass aggregator.
        self.refresh_party_ability_bits();
        let pc = self.party_count.min(3) as usize;
        for slot in 0..pc {
            // `slot` stays the battle ordinal for every live mirror written
            // below; the stats are read off the occupying character's record.
            let Some(rec) = self.roster.members.get(self.party_roster_slot(slot)) else {
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
            // Aggregator base stats: prefer the record-side base window
            // (`+0x11C..`, [`legaia_save::RecordStats`]) - the window retail's
            // `FUN_80042558` rebuilds the live stats FROM - so the accessory
            // percent boosts compute from the true base (a record imported
            // from a retail save carries the last rebuilt, already-boosted
            // values in its live window). Fall back to the live window for
            // synthetic records that only populate live stats. Only taken
            // when a passives catalog is installed: without one no percent
            // boost is re-applied, so the live window (which may carry them)
            // remains the better effective source.
            let recs = rec.record_stats();
            let (agl, atk, udf, ldf, spd, int) =
                if !self.accessory_passives.is_empty() && recs.atk != 0 {
                    (recs.agl, recs.atk, recs.udf, recs.ldf, recs.spd, recs.int)
                } else {
                    (live.agl, live.atk, live.udf, live.ldf, live.spd, live.int)
                };
            let base_max_hp = if recs.hp_max != 0 {
                recs.hp_max
            } else {
                rec.hp_mp_sp().hp_max
            };
            let record = crate::battle_stats::StatRecord {
                base_attack: atk,
                base_udf: udf,
                base_ldf: ldf,
                base_accuracy: agl,
                base_evasion: agl,
                base_spd: spd,
                base_int: int,
                equip: rec.equipment().slots,
            };
            let stats = crate::battle_stats::compute_battle_stats_with_passives(
                &record,
                &self.equipment_table,
                &self.accessory_passives,
                &[],
                &crate::battle_stats::StatusModifiers::default(),
            );
            // Max-HP percent passives (indices 0x00 / 0x01) rebuild the
            // effective max HP from the base, capped at 9999 - applied to the
            // live battle actor, the engine's effective-max-HP holder. Only
            // touched when a boost bit is present so synthetic max-HP setups
            // keep their values. (Retail's max-MP boosts, indices 0x02/0x03,
            // have no engine consumer yet: the battle actor carries current
            // MP only, no max-MP mirror.)
            let pwords = self.accessory_passives.bits_for_equipment(&record.equip);
            if pwords[0] & 0x3 != 0 {
                let mut max_hp = base_max_hp;
                if pwords[0] & 0x1 != 0 {
                    max_hp = max_hp.saturating_add(base_max_hp / 10);
                }
                if pwords[0] & 0x2 != 0 {
                    max_hp = max_hp.saturating_add(base_max_hp / 4);
                }
                let max_hp = max_hp.min(9999);
                if let Some(a) = self.actors.get_mut(slot) {
                    a.battle.max_hp = max_hp;
                    // Retail clamps current HP to the rebuilt max.
                    a.battle.hp = a.battle.hp.min(max_hp);
                }
            }
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
            // Seed the AP gauge base from the captured level. Base only - the
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
        // One-shot override: fire on the next field step regardless of any
        // installed per-region random tracker.
        self.scripted_formation_pending = true;
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
        // One-shot override: fire on the next field step even when a per-region
        // random tracker is installed (town01 is 0% random). See the field.
        self.scripted_formation_pending = true;
        Some(formation_id)
    }

    /// Enter the scripted battle the field-VM op `3E FF <row>` selects: the
    /// per-scene MAN formation-table row `row`, latched for immediate battle
    /// entry (no field step required).
    ///
    /// PORT: FUN_801DE840 (case 0x3E interact arm: `sys_ctx[+0x8A] = 1`,
    /// `sys_ctx[+0x94] = *(ctrl+0x20) + row * *(ctrl+0x5D) + 1`, then the
    /// mode-0xE request `FUN_8003CE08(0xE)`)
    /// REF: FUN_801DA51C (the entity SM's confirm state copies the installed
    /// row into the battle formation cell `0x8007BD0C`)
    ///
    /// Retail points the SYSTEM entity's `+0x94` at the formation row and
    /// advances its 5-state SM to Activating; the entity tick then performs
    /// the record copy and the battle transition without any player step.
    /// The engine models that confirm-and-transition with the same immediate
    /// latch the field-carrier SM resolution uses
    /// ([`Self::pending_field_carrier_battle`], drained by
    /// `Self::tick_field_carriers` in the same frame): the formation must
    /// already be registered by [`Self::install_man_encounter`] (the
    /// scene-entry MAN install, which also merges the row's monster ids'
    /// real archive stats), so the fight resolves against pure disc data.
    ///
    /// This is the boss-fight entry mechanism: the scripted rows sit outside
    /// every region's rollable `[base, base+count)` slice (garmel rows 8/9 =
    /// lone Songi `0x4C` / lone Zeto `0x4B`; rikuroa row 17 = lone Caruban
    /// `0x49`), reachable only through this op at the end of the scene's
    /// beat records.
    ///
    /// Returns `false` (and enters nothing) when the row isn't registered or
    /// has no monsters, mirroring the reader's `count == 0` no-spawn arm.
    pub fn trigger_scripted_battle(&mut self, row: u8) -> bool {
        let formation_id = u16::from(row);
        let has_slots = self
            .formation_table
            .formation(formation_id)
            .is_some_and(|def| !def.slots.is_empty());
        if !has_slots {
            log::warn!("field: op-0x3E scripted battle row {row} is not a registered formation");
            return false;
        }
        log::info!("field: op-0x3E scripted battle entry -> formation row {row}");
        self.pending_field_carrier_battle = Some(formation_id);
        // Scripted rows carry a non-zero first header byte the retail reader
        // ORs `0x80` into a battle-setup flag for; the staged fight refuses
        // the Run command (the `ctx+0x287` no-escape input of the escape
        // roll `FUN_801E791C`). Cleared by `finish_battle`.
        self.battle_no_escape = true;
        true
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

    /// Install the active scene's **boss-stager placements** as approach /
    /// interact dispatch bindings, derived entirely from the MAN's own bytes
    /// ([`crate::man_field_scripts::boss_stager_placements`]).
    ///
    /// For each partition-1 placement whose record carries the scripted-battle
    /// op `3E FF <row>` (rikuroa's Caruban stager `P1[3]`: `52 89` marker SET
    /// then `3E FF 11`), and whose
    ///
    /// - `formation_row` resolves in the already-installed MAN formation table
    ///   (the [`Self::install_man_encounter`] scene-entry install - this drops
    ///   any text-desync phantom site), and
    /// - park-gate flag (the record's own head `SysFlag.Test`, e.g. `0x142`)
    ///   is still clear (the beaten-boss one-shot),
    ///
    /// this registers the placement slot in [`Self::field_boss_stagers`] and
    /// stations its approach point - the record's own `0x4C 0x51` NPC-run
    /// destination (Noa at the nest tile), or the placement spawn tile when
    /// the record has no station leg - as an interact-probe position
    /// ([`Self::field_npc_positions`]) plus a walk-touch contact
    /// ([`Self::field_walk_touch`],
    /// [`crate::man_field_scripts::WalkTouchEvent::StagerBeat`]). Walking
    /// into / interacting with the placed actor then runs the record itself
    /// ([`Self::run_boss_stager_record`]) - the engine mirror of retail's
    /// touch dispatch resuming the parked stager script.
    ///
    /// Call after [`Self::install_field_carriers_from_man`] (whose inner
    /// install clears the walk-touch map) and after
    /// [`Self::install_man_encounter`] (the formation-row validator).
    // REF: FUN_801d5b5c (touch dispatch), FUN_801cf9f4 (interaction probe)
    pub fn install_boss_stagers_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        self.field_boss_stagers.clear();
        for site in crate::man_field_scripts::boss_stager_placements(man_file, man) {
            // The row must be a registered scene formation (scene entry merged
            // the MAN rows + their archive stats) - a desync phantom is not.
            let row_ok = self
                .formation_table
                .formation(u16::from(site.formation_row))
                .is_some_and(|def| !def.slots.is_empty());
            if !row_ok {
                continue;
            }
            // One-shot park gate (the record's own head test): a beaten boss
            // never re-arms.
            if site
                .park_gate_flag
                .is_some_and(|flag| self.system_flag_test(flag))
            {
                continue;
            }
            let Ok(slot) = u8::try_from(site.placement_index) else {
                continue;
            };
            let station = match site.station_world {
                Some(w) => w,
                None if !site.spawn_parked => site.spawn_world,
                // Parked with no station leg: no reachable approach point.
                None => continue,
            };
            self.field_boss_stagers.insert(
                slot,
                FieldBossStager {
                    record: slot,
                    park_gate: site.park_gate_flag,
                },
            );
            self.field_npc_positions.insert(slot, station);
            self.field_walk_touch.insert(
                slot,
                (
                    station,
                    crate::man_field_scripts::WalkTouchEvent::StagerBeat,
                ),
            );
            log::info!(
                "field: boss stager P1[{slot}] armed at {station:?} (formation row {}, park gate {:?})",
                site.formation_row,
                site.park_gate_flag,
            );
        }
    }

    /// Run the boss-stager record bound to placement `slot`: install its
    /// partition-1 record as the modal cutscene timeline, so the record's own
    /// script bytes stage the fight (rikuroa `P1[3]`: choreography + dialog,
    /// `52 89` staged-marker SET, then `3E FF 11` ->
    /// [`Self::trigger_scripted_battle`] on MAN formation row 17).
    ///
    /// The engine mirror of retail's approach dispatch: the locomotion touch
    /// dispatch (`FUN_801d5b5c`) / interaction probe (`FUN_801cf9f4`) resumes
    /// the placed actor's parked script context - no script-side un-halt poke
    /// to the stager channel exists in the MAN. Reached from
    /// [`Self::trigger_field_interact`] (which both paths route through).
    ///
    /// Returns `true` when the record installed as the timeline. The binding
    /// is consumed on install (one approach = one beat; the record's own gate
    /// flag latches the one-shot across visits). Refuses while another
    /// timeline plays, when the park gate has latched mid-visit, or when no
    /// scene MAN is resident.
    // REF: FUN_801d5b5c, FUN_801cf9f4, FUN_8003BDE0 (context install)
    pub fn run_boss_stager_record(&mut self, slot: u8) -> bool {
        let Some(&FieldBossStager { record, park_gate }) = self.field_boss_stagers.get(&slot)
        else {
            return false;
        };
        if park_gate.is_some_and(|flag| self.system_flag_test(flag)) {
            // Latched mid-visit (the fight resolved): drop the stale binding.
            self.field_boss_stagers.remove(&slot);
            self.field_walk_touch.remove(&slot);
            return false;
        }
        if self.cutscene_timeline_active() {
            return false;
        }
        let Some(man) = self.field_channels_man.clone() else {
            return false;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            return false;
        };
        let installed =
            self.install_cutscene_timeline_record(&man_file, &man, 1, record as usize, false);
        if installed {
            self.field_boss_stagers.remove(&slot);
            self.field_walk_touch.remove(&slot);
            log::info!("field: boss stager P1[{record}] launched as the beat timeline");
        }
        installed
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
        // Scripted/forced formation (install_man_formation /
        // install_encounter_from_record): a one-shot override that fires on this
        // step regardless of any per-region random tracker. Retail copies the
        // carrier's `entity[+0x94]` formation into the battle cell independent of
        // the random-roll path (`FUN_801D9E1C`), so a 0%-random scene still
        // starts the scripted fight. Drive the forced 0xFF session directly and
        // consume the flag.
        if self.scripted_formation_pending {
            self.scripted_formation_pending = false;
            let rng = self.next_rng();
            return match self.encounter.as_mut() {
                Some(session) => {
                    // Retail's scripted path bypasses the rate math entirely;
                    // keep the forced 0xFF-rate roll free of the accessory /
                    // status modifiers (a >>1 would halve a forced trigger).
                    session.tracker_mut().set_rate_modifiers(Default::default());
                    session.on_step(rng)
                }
                None => false,
            };
        }
        // Per-region path: when a field region tracker is installed, the
        // player's active region (rate increment + formation-range pick)
        // drives the roll (`FUN_801D9E1C`) instead of the session's
        // aggregated mean rate. The session still owns the transition /
        // grace SM, so only roll while it is `Idle` (mirroring
        // [`crate::encounter::EncounterSession::on_step`]'s own gate) and feed
        // a trigger through [`crate::encounter::EncounterSession::trigger_with`].
        // REF: FUN_801D9E1C (ported in crate::region_encounter)
        if self.field_region_tracker.is_some() {
            let idle = self
                .encounter
                .as_ref()
                .is_none_or(|s| matches!(s.phase(), crate::encounter::EncounterPhase::Idle));
            if !idle {
                return false;
            }
            let Some(slot) = self.player_actor_slot else {
                return false;
            };
            let (wx, wz) = match self.actors.get(slot as usize) {
                Some(a) => (a.move_state.world_x, a.move_state.world_z),
                None => return false,
            };
            // Take the tracker out so the RNG closure can borrow `self`
            // (same borrow-window pattern as `live_world_map_tick`).
            let mut tracker = self.field_region_tracker.take().expect("is_some checked");
            tracker.set_modifiers(self.encounter_rate_modifiers());
            let roll = tracker.on_step(wx, wz, || self.next_rng());
            self.field_region_tracker = Some(tracker);
            return match roll {
                Some(r) => {
                    let er = crate::encounter::EncounterRoll {
                        formation_id: r.formation_id as u16,
                        row_index: r.formation_id as usize,
                        roll_q8: 0,
                    };
                    self.encounter
                        .as_mut()
                        .map(|s| s.trigger_with(er))
                        .unwrap_or(false)
                }
                None => false,
            };
        }
        let rng = self.next_rng();
        let modifiers = self.encounter_rate_modifiers();
        match self.encounter.as_mut() {
            Some(session) => {
                session.tracker_mut().set_rate_modifiers(modifiers);
                session.on_step(rng)
            }
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
