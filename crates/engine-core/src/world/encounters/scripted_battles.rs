//! Scripted and boss encounters: the scripted-battle trigger and install, the
//! boss stagers read from the MAN, and placement engagements.

use super::*;
use legaia_engine_mode::mode::SceneMode;

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
    /// ([`legaia_engine_field::field_carrier_state::FieldCarrierState::pending_battle`], drained by
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
        // `FUN_801D9E1C(player, 0)` first (`0x801E0710`, the player's `+0x8E`
        // / `+0x8F` preset to `0xFF` so the same-tile early-out cannot fire):
        // the region battle setup for the tile the fight starts on, with
        // `a1 = 0` returning before the step roll.
        self.apply_region_battle_setup_at_player();
        // The arm rerolls the encounter step counter right after the `+0x94`
        // install (`jal 0x801DDF48` at `0x801E076C`, `sw v0, _DAT_8007B5FC`
        // at `0x801E0780`) and before the mode request - unconditionally, so
        // a fight that ends back in a rolling region starts from a fresh
        // `488..=1460` count rather than the pre-fight residue.
        self.reroll_encounter_step_counter();
        let formation_id = u16::from(row);
        let has_slots = self
            .tables
            .formation_table
            .formation(formation_id)
            .is_some_and(|def| !def.slots.is_empty());
        if !has_slots {
            log::warn!("field: op-0x3E scripted battle row {row} is not a registered formation");
            return false;
        }
        log::info!("field: op-0x3E scripted battle entry -> formation row {row}");
        self.carriers.pending_battle = Some(formation_id);
        // No no-escape latch here. The arm (`0x801E070C..0x801E0788`) writes
        // the entity's `+0x8A` / `+0x94`, the step counter and the mode
        // request, nothing else: whether the fight refuses Run is the row's
        // own `record[+0]` header byte, which the entity SM's confirm state
        // turns into `DAT_8007BD60 | 0x80` (`FUN_801DA51C`, `0x801DA5F8..
        // 0x801DA61C`) and battle init into `ctx+0x287`. Most `3E FF` rows
        // carry a non-zero byte; the Rim Elm ambush (town0b / town0c row 3),
        // town01 row 4 and deene row 11 carry zero, and retail lets the
        // party run from them. `World::enter_battle_from_formation` derives
        // the flag from the row.
        true
    }

    /// Arm (or disarm) the scripted-encounter consumer.
    ///
    /// While armed, the field VM's bare arm-encounter op (`0x37`/`0x41`) hands
    /// the record window overlaying the opcode to the host, which parses it as
    /// an [`legaia_scene_formats::encounter_record::EncounterRecord`] and routes it through
    /// [`Self::install_scripted_encounter`]. See
    /// [`legaia_engine_battle::encounter_state::EncounterState::scripted_armed`] for why this gate exists (there is no
    /// dedicated encounter opcode; the consuming entity SM is the retail
    /// discriminator).
    pub fn arm_scripted_encounter(&mut self, on: bool) {
        self.encounters.scripted_armed = on;
    }

    /// Install a scripted encounter from the inline bytecode window the field
    /// VM forwarded at the bare arm-encounter op (`0x37`/`0x41`); the record
    /// overlays the opcode (`[opcode][op1][op2][count][ids..]`).
    ///
    /// The window is parsed as an [`legaia_scene_formats::encounter_record::EncounterRecord`]
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
        let record = legaia_scene_formats::encounter_record::EncounterRecord::parse(record_bytes)?;
        if record.is_empty() {
            return None;
        }
        let scene = self.active_scene_label.clone();
        let id = self.install_encounter_from_record(&scene, &record);
        // Fire-once: retail clears `entity[+0x94]` after the formation copy so
        // the arm fires exactly once. Disarm the engine-side carrier flag too.
        if let Some(id) = id {
            self.encounters.scripted_armed = false;
            // The record's `record[+0]` here is the *install opcode itself*
            // (the record overlays the opcode), so it is non-zero by
            // construction and the confirm state raises the per-battle
            // `0x80`. Carry it onto the synthesized def so the intro style
            // and the transition's audio cue see the scripted battle.
            if let Some(def) = self.tables.formation_table.by_id.get_mut(&id) {
                def.header_flags = record_bytes.first().copied().unwrap_or(0);
            }
        }
        id
    }

    /// Install the active scene's **boss-stager placements** as approach /
    /// interact dispatch bindings, derived entirely from the MAN's own bytes
    /// ([`legaia_engine_field::man_field_scripts::boss_stager_placements`]).
    ///
    /// For each partition-1 placement whose record carries the scripted-battle
    /// op `3E FF <row>` (rikuroa's Caruban stager `P1[3]`: `52 89` marker SET
    /// then `3E FF 11`), and whose
    ///
    /// - `formation_row` resolves in the already-installed MAN formation table
    ///   (the [`Self::install_man_encounter`] scene-entry install - this drops
    ///   any text-desync phantom site), and
    /// - park-gate flag (the record's own head `SysFlag.Test`, e.g. `0x142`)
    ///   is still clear (the beaten-boss one-shot), and
    /// - placement slot is **not** already a scripted-encounter carrier slot
    ///   (the talk-first sparring partner; see the rule at the skip below),
    ///
    /// this registers the placement slot in [`crate::world::FieldPropState::boss_stagers`] and
    /// stations its approach point - the record's own `0x4C 0x51` NPC-run
    /// destination (Noa at the nest tile), or the placement spawn tile when
    /// the record has no station leg - as an interact-probe position
    /// ([`crate::world::FieldNpcState::positions`]) plus a walk-touch contact
    /// ([`crate::world::FieldPropState::walk_touch`],
    /// [`legaia_engine_field::man_field_scripts::WalkTouchEvent::StagerBeat`]). Walking
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
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
    ) {
        self.props.boss_stagers.clear();
        for site in legaia_engine_field::man_field_scripts::boss_stager_placements(man_file, man) {
            // The row must be a registered scene formation (scene entry merged
            // the MAN rows + their archive stats) - a desync phantom is not.
            let row_ok = self
                .tables
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
            // **One placement, one dispatch owner.** A placement the carrier
            // install already claimed as a
            // [`legaia_engine_field::man_field_scripts::FieldCarrierConfig::ScriptedEncounter`] is a
            // talk-first NPC whose `3E FF <row>` sits *behind* its dialogue
            // and its inline picker - retail's Rim Elm sparring partner
            // (`town01` P1[10], MAN formation row 4). Its interaction must
            // open the box and let the player choose the fight option; the
            // stager binding instead installs the whole record as a modal
            // timeline in [`Self::run_boss_stager_record`], which
            // [`Self::trigger_field_interact`] consults *first*, so the
            // dialogue and the picker never appear and the fight never
            // starts. It also overwrites the placement's probe position with
            // the record's station leg, moving the talk box off the
            // partner's tile.
            //
            // A genuine stager (`rikuroa` P1[3]'s Caruban, the `cave01` /
            // `rugi` ambushes) carries no scripted-encounter carrier entry,
            // so this skips exactly one placement in the retail corpus - the
            // one whose two models collide. Ordering matters and is
            // documented above: this install runs after
            // [`Self::install_field_carriers_from_man`], which is what fills
            // `carriers.slots`.
            if self.carriers.slots.contains_key(&slot) {
                log::debug!(
                    "field: P1[{slot}] is a scripted-encounter carrier; not arming it as a boss \
                     stager (the carrier owns the interaction)"
                );
                continue;
            }
            let station = match site.station_world {
                Some(w) => w,
                None if !site.spawn_parked => site.spawn_world,
                // Parked with no station leg: no reachable approach point.
                None => continue,
            };
            self.props.boss_stagers.insert(
                slot,
                FieldBossStager {
                    record: slot,
                    park_gate: site.park_gate_flag,
                },
            );
            self.npcs.positions.insert(slot, station);
            self.props.walk_touch.insert(
                slot,
                (
                    station,
                    legaia_engine_field::man_field_scripts::WalkTouchEvent::StagerBeat,
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
        let Some(&FieldBossStager { record, park_gate }) = self.props.boss_stagers.get(&slot)
        else {
            return false;
        };
        if park_gate.is_some_and(|flag| self.system_flag_test(flag)) {
            // Latched mid-visit (the fight resolved): drop the stale binding.
            self.props.boss_stagers.remove(&slot);
            self.props.walk_touch.remove(&slot);
            return false;
        }
        if self.cutscene_timeline_active() {
            return false;
        }
        let Some(man) = self.field_vm.channels_man.clone() else {
            return false;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            return false;
        };
        let installed = self.install_interaction_timeline(&man_file, &man, record);
        if installed {
            self.props.boss_stagers.remove(&slot);
            self.props.walk_touch.remove(&slot);
            log::info!("field: boss stager P1[{record}] launched as the beat timeline");
        }
        installed
    }

    /// Install placement `P1[record]`'s own context as the interaction
    /// timeline: start at its PC (past the spawn section the entry pre-run
    /// executed, or where its last interaction's `0x21` left it) and mark
    /// the timeline as that context, so it ends at the next executed `0x21`
    /// and the channel never runs a second copy of the same bytes
    /// (`CutsceneTimeline::interaction_slot`).
    // REF: FUN_80039B7C
    fn install_interaction_timeline(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
        record: u8,
    ) -> bool {
        if !self.install_cutscene_timeline_record(man_file, man, 1, record as usize, false) {
            return false;
        }
        let span = legaia_engine_field::man_field_scripts::partition_record_span(
            man_file,
            man,
            1,
            record as usize,
        );
        let channel_pc = self
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == record as usize && !c.done)
            .zip(span)
            .and_then(|(c, (start, _, len))| {
                (c.record_offset == start && c.pc < len).then_some(c.pc)
            });
        if let Some(tl) = self.cutscene.timeline.as_mut() {
            tl.interaction_slot = Some(record);
            if let Some(pc) = channel_pc {
                tl.pc = pc;
            }
        }
        true
    }

    /// Run each placement context a cross-context `B1 <id> 08` engaged
    /// ([`crate::world::FieldVmState::pending_engagements`]) as its
    /// interaction timeline, one at a time, once nothing else holds the
    /// frame: retail's per-actor tick runs an engaged context the frame after
    /// the write, but the engine has one modal timeline, so an engagement
    /// waits for it.
    // REF: FUN_8003BC08 (`0x8003BD10..0x8003BD38`), FUN_80039B7C
    pub fn drain_placement_engagements(&mut self) {
        if self.field_vm.pending_engagements.is_empty()
            || !matches!(self.mode, SceneMode::Field)
            || self.cutscene_timeline_active()
            || self.dialogue_owns_input()
            || self.field_scripts_held_for_battle()
        {
            return;
        }
        let Some(man) = self.field_vm.channels_man.clone() else {
            self.field_vm.pending_engagements.clear();
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            self.field_vm.pending_engagements.clear();
            return;
        };
        let placement = self.field_vm.pending_engagements.remove(0);
        let Ok(record) = u8::try_from(placement) else {
            return;
        };
        if self.install_interaction_timeline(&man_file, &man, record) {
            log::info!("field: P1[{record}] engaged by a cross-context CFLAG_SET bit 8");
        }
    }
}
