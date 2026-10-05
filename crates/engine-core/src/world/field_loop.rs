//! Live field<->battle loop, encounter/formation battle entry, battle BGM swap, field player install/seating, and step_field.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    // --- live gameplay loop: Field <-> Battle round trip ------------------

    /// Per-frame field-side driver for the live gameplay loop. Gated by
    /// [`crate::world::WorldToggles::live_gameplay_loop`] in [`Self::tick`]; never called when the
    /// flag is off.
    ///
    /// Composes the already-existing encounter pieces into the per-frame
    /// flow the retail field loop runs:
    ///
    /// 1. **Step detection.** A "step" is the player actor crossing into a
    ///    new 128-unit collision tile (`pos >> 7`), sampled once per actor
    ///    game tick ([`crate::world::FrameClock::game_tick_fired`]). Each step drives one
    ///    [`Self::on_field_step`] roll - matching the retail per-step
    ///    counter rather than rolling every frame.
    /// 2. **Timers.** [`Self::tick_encounter`] advances the session's
    ///    `Transition` / `Grace` countdowns every frame regardless of
    ///    movement.
    /// 3. **Transition.** When the session reaches `Triggered`,
    ///    [`Self::drain_encounter_formation`] yields the rolled formation and
    ///    [`Self::begin_encounter_battle`] flips `Field -> Battle`.
    pub(crate) fn live_field_tick(&mut self) {
        // (1) step detection on tile crossing.
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get(slot as usize)
        {
            let tile = (actor.move_state.world_x >> 7, actor.move_state.world_z >> 7);
            // Per-tile region refresh (the `FUN_800180EC` / `FUN_801DBA20`
            // grain - retail re-runs the region scan when the player tile
            // changes).
            match self.terrain.last_tile {
                Some(prev) if prev != tile => {
                    self.terrain.last_tile = Some(tile);
                    self.refresh_field_regions();
                }
                None => self.terrain.last_tile = Some(tile),
                _ => {}
            }
            // The encounter step is sampled on the actor game tick only.
            // Retail reaches the region reader from an actor handler
            // (`FUN_801DA51C` state 0, `jal 0x801D9E1C` at `0x801DA5B0`), so
            // it compares the player's tile once every `frame_step` vsyncs
            // (2 in a field scene). Sampling per vsync counted a diagonal
            // that crosses its X and Z boundaries on different vsyncs of one
            // game tick as two steps where retail sees one.
            if self.clock.game_tick_fired {
                match self.terrain.step_tile {
                    Some(prev) if prev != tile => {
                        self.terrain.step_tile = Some(tile);
                        // A jump of two or more tiles (a script seating the
                        // player, a warp landing) is not a step to the
                        // region reader; a forced scripted formation still
                        // fires.
                        let step = crate::region_encounter::is_region_step(
                            (i32::from(prev.0), i32::from(prev.1)),
                            (i32::from(tile.0), i32::from(tile.1)),
                        );
                        if step || self.encounters.scripted_formation_pending {
                            self.on_field_step();
                        }
                    }
                    None => self.terrain.step_tile = Some(tile),
                    _ => {}
                }
            }
        }
        // (2) advance transition / grace timers.
        self.tick_encounter();
        // (3) Triggered -> begin battle.
        if let Some(roll) = self.drain_encounter_formation() {
            self.begin_encounter_battle(roll);
        }
    }

    /// Resolve `roll` to a concrete formation and flip into battle.
    ///
    /// Snapshots the field actor table (restored verbatim on victory),
    /// remembers the formation for [`Self::apply_battle_loot`], and seeds the
    /// battle actor table from the formation + monster catalog.
    ///
    /// An unregistered `formation_id` bails back to the field (the session has
    /// already advanced to `Battling`, so the next
    /// [`Self::end_encounter_battle`] cleans it up) - but **loudly**. This is
    /// the last gate a rolled encounter passes, so a quiet return here is a
    /// battle that the player saw the transition for and never got, with
    /// nothing in the log to say so. `install_man_encounter` cross-checks the
    /// same pairing at scene entry; if this ever fires, that check was bypassed
    /// or the table was replaced after it ran.
    pub(crate) fn begin_encounter_battle(&mut self, roll: crate::encounter::EncounterRoll) {
        let Some(formation) = self
            .tables
            .formation_table
            .formation(roll.formation_id)
            .cloned()
        else {
            log::error!(
                "encounter: rolled formation {} in scene '{}' is not registered - the battle is \
                 dropped and the field resumes (registered rows: {:?})",
                roll.formation_id,
                self.active_scene_label,
                self.registered_formation_ids()
            );
            self.end_encounter_battle();
            return;
        };
        if formation.slots.is_empty() {
            // Retail's reader clears the formation cell for a `count == 0` row
            // and spawns nothing, so this is not an error - but it is still a
            // step that produced no fight, and worth saying once.
            log::debug!(
                "encounter: rolled formation {} in '{}' carries no monsters (retail no-spawn row)",
                roll.formation_id,
                self.active_scene_label
            );
            self.end_encounter_battle();
            return;
        }
        self.field_return = Some(FieldReturnState {
            actors: self.actors.clone(),
            player_actor_slot: self.player_actor_slot,
            party_count: self.party.party_count,
        });
        self.battle.return_mode = SceneMode::Field;
        // No engine-side battle staging: a scripted boss fight's transient
        // staged marker (rikuroa's `0x289`) is SET by the stager record's own
        // script bytes (`P1[3]`'s `52 89`, executed through
        // [`Self::run_boss_stager_record`]) immediately before its `3E FF`
        // battle-entry op reaches this path.
        self.enter_battle_from_formation(&formation);
        self.battle.active_formation = Some(formation);
    }

    /// Seed the battle actor table from `formation` and enter
    /// [`SceneMode::Battle`].
    ///
    /// Party slots `0..party_count` keep their HP / MP (seeded from the
    /// roster by the boot path); monster slots take HP / attack / defense
    /// from [`crate::world::DiscTables::monster_catalog`]. Every combatant is marked alive,
    /// `action_category = Attack`, and party members target the first
    /// monster. The battle-action context is seeded at `Begin` with the
    /// Attack action queued. This is the live-loop counterpart to the
    /// generic [`Self::enter_battle`] placement helper.
    /// Configure the battle BGM track id. `Some(id)` enables the
    /// Battle↔Field music swap (the live loop switches to `id` on encounter
    /// and restores the field track on battle end); `None` disables it. See
    /// [`crate::world::AudioState::battle_bgm`].
    pub fn set_battle_bgm(&mut self, bgm_id: Option<u16>) {
        self.audio.battle_bgm = bgm_id;
    }

    /// Switch to the configured battle track at encounter start. No-op when
    /// [`crate::world::AudioState::battle_bgm`] is `None` or the swap is already active. Stashes
    /// the current field track for [`World::restore_field_bgm`] and queues a
    /// `FieldEvent::Bgm` start so the host's BGM director cross-fades to it.
    ///
    /// Which track follows the battle sound set `_DAT_8007B880` the field
    /// script left ([`crate::world::AudioState::battle_sound_set`]), as the
    /// intro `FUN_801CF5BC` does: `-1` loads nothing (phase 2's `bltz` at
    /// `0x801CF6EC`) and skips the completion arm's field stop and battle
    /// start (`0x801CF8FC..`), so the field score - a boss theme the event
    /// started - plays through the fight and nothing is stashed; `0` loads
    /// the default bundle (`0x370` instead of `0x36F` when `DAT_8007B64B` is
    /// set, `0x801CF6FC..0x801CF724`), which the port answers with the
    /// configured track; `N > 0` loads bundle `0x36F + N`
    /// ([`crate::music_labels::battle_bank_bgm_id`]).
    pub(crate) fn swap_to_battle_bgm(&mut self) {
        let Some(configured) = self.audio.battle_bgm else {
            return;
        };
        let set = self.audio.battle_sound_set;
        if set < 0 {
            return;
        }
        let battle = if set == 0 {
            let alt = self
                .encounters
                .region_setup
                .and_then(|s| s.keep_backdrop_object_1)
                .unwrap_or(false);
            if alt && configured == crate::music_labels::BATTLE_THEME_1_BGM_ID {
                crate::music_labels::BATTLE_THEME_2_BGM_ID
            } else {
                configured
            }
        } else {
            crate::music_labels::battle_bank_bgm_id(set).unwrap_or(configured)
        };
        if self.audio.battle_bgm_active || self.audio.current_bgm == Some(battle) {
            return;
        }
        self.audio.field_bgm_resume = self.audio.current_bgm;
        self.audio.current_bgm = Some(battle);
        self.audio.battle_bgm_active = true;
        self.pending_field_events.push(FieldEvent::Bgm {
            text_id: battle,
            sub_op: 1,
        });
    }

    /// Restore the field track stashed by [`World::swap_to_battle_bgm`] when
    /// a battle ends. No-op unless a battle swap is active. Queues a
    /// `FieldEvent::Bgm` start for the stashed track, or a stop
    /// ([`crate::scene::BGM_SUB_OP_ENGINE_STOP`]) when no field track was
    /// playing at encounter start.
    pub(crate) fn restore_field_bgm(&mut self) {
        if !self.audio.battle_bgm_active {
            return;
        }
        self.audio.battle_bgm_active = false;
        match self.audio.field_bgm_resume.take() {
            Some(track) => {
                self.audio.current_bgm = Some(track);
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: track,
                    sub_op: 1,
                });
            }
            None => {
                self.audio.current_bgm = None;
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: 0,
                    sub_op: crate::scene::BGM_SUB_OP_ENGINE_STOP,
                });
            }
        }
    }

    /// Hand the score to an in-world minigame's own global-pool track.
    ///
    /// The three minigames whose overlay init loads a track of its own - the
    /// dance chart loops, the Baka Fighter overture, the Muscle Dome battle
    /// theme - call this from the shared door-warp entry
    /// ([`crate::scene::SceneHost::drain_minigame_warp`]), so **both** hosts
    /// start the same music on the same frame. The slot machine and fishing
    /// pass `None` and inherit the host scene's BGM, which is retail's own
    /// behaviour for those two.
    ///
    /// Queued as an ordinary `FieldEvent::Bgm` start (sub-op 1) rather than
    /// played here: the host's BGM director is the only layer that can resolve
    /// a `music_01` entry, and routing it as an op-`0x35` start is what makes
    /// the native window and the browser play page reach the same code.
    /// `current_bgm` is deliberately **not** overwritten - it names the
    /// *scene's* track, which [`Self::restore_minigame_bgm`] resumes.
    pub(crate) fn swap_to_minigame_bgm(&mut self, bgm_id: u16) {
        if self.audio.minigame_bgm_active {
            return;
        }
        self.audio.minigame_bgm_resume = self.audio.current_bgm;
        self.audio.minigame_bgm_active = true;
        self.pending_field_events.push(FieldEvent::Bgm {
            text_id: bgm_id,
            sub_op: 1,
        });
    }

    /// Resume the field track a minigame's own music displaced, on the
    /// mode-24 return warp. No-op unless [`Self::swap_to_minigame_bgm`] armed
    /// the swap. A scene that had no track at entry gets a stop
    /// ([`crate::scene::BGM_SUB_OP_ENGINE_STOP`]) rather than being left with the minigame's music running under the
    /// field.
    pub(crate) fn restore_minigame_bgm(&mut self) {
        if !self.audio.minigame_bgm_active {
            return;
        }
        self.audio.minigame_bgm_active = false;
        match self.audio.minigame_bgm_resume.take() {
            Some(track) => {
                self.audio.current_bgm = Some(track);
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: track,
                    sub_op: 1,
                });
            }
            None => {
                self.audio.current_bgm = None;
                self.pending_field_events.push(FieldEvent::Bgm {
                    text_id: 0,
                    sub_op: crate::scene::BGM_SUB_OP_ENGINE_STOP,
                });
            }
        }
    }

    pub(crate) fn enter_battle_from_formation(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
    ) {
        // **The Tetsu spar is Vahn alone** - an engine rule, not a port.
        // Retail seats whoever is in the present-party list `DAT_8007BD10`
        // (`FUN_80052FA0` counts its non-zero ids into `ctx[+0]`,
        // `FUN_800513F0` loads one actor per id) and has no stage-keyed
        // override; the spar is solo only because the story's party is. The
        // prompt machine is written for one fighter (its lessons walk one
        // member's command flow), so a fuller party - `--party`, a save, a
        // debug force - would sit two idle members through a tutorial that
        // never addresses them. The field composition comes back at
        // [`World::finish_battle`].
        // Vahn is roster record 0 (char id 1, `0x80084708`).
        const VAHN_ROSTER_SLOT: u8 = 0;
        let mut party_count = self.party.party_count.clamp(1, 3);
        if self.sparring_fight_pending() && party_count > 1 {
            self.battle.solo_spar_restore = Some(std::mem::replace(
                &mut self.party.active_party,
                vec![VAHN_ROSTER_SLOT],
            ));
            party_count = 1;
        }
        let monster_count = formation.slots.len().min(5) as u8;
        // Drop any field dialogue left open across the transition. The
        // engage conversation already played in the field; a leftover
        // inline-script runner would otherwise re-walk the NPC's whole
        // segment bank over the battle (and nothing in battle mode owns
        // its input). Retail's in-battle tutorial boxes are a separate
        // stage-overlay (extraction 967) channel, not the field box.
        //
        // A prop run dropped here never reaches its teardown, which is what
        // clears the player's engaged bit; release it with the run, or the
        // field comes back with the pad controller locked for good.
        if self
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor.is_some())
        {
            self.set_player_engaged(false);
        }
        self.dialog.inline = None;
        self.dialog.current = None;
        self.carriers.menu = None;
        self.carriers.pending_engage = None;
        // Reuse the placement helper for actor spawn + seating, then overlay
        // per-slot stats.
        self.enter_battle(party_count, monster_count);
        // The scripted-fight flag `ctx[+0x287]`, derived the way retail's
        // battle init does: `FUN_800513F0` stores `(DAT_8007BD60 >> 5) & 4`,
        // i.e. "the formation's `record[+0]` header byte is non-zero"
        // (`FUN_801DA51C` ORs `0x80` in for exactly those rows). The field
        // VM's scripted-battle op writes no flag of its own, so a `3E FF`
        // row with a zero header byte (the Rim Elm ambush) is an ordinary,
        // escapable fight. A `no_escape` a host or test raised before entry
        // still counts - it is the same byte set from outside.
        //
        // It has to be settled here, before the monster seed below: the
        // battle loader's stat boost profile is picked by this flag.
        self.battle.scripted_fight = formation.per_battle_flags() != 0 || self.battle.no_escape;
        let scripted = self.battle.scripted_fight;
        // One retail byte, so one value: the escape roll (`FUN_801E791C`),
        // the monster flee roll (`FUN_801EC0DC`) and the formation roll
        // (`FUN_80051D84`) all read `ctx+0x287` itself.
        self.battle.no_escape = scripted;
        // The same byte inside the action context, `ctx[+0x287]`. All three
        // of `FUN_801E295C`'s reads of it (`0x801E4F94`, `0x801E5058`,
        // `0x801E5554`) are gates on this flag - the counter-attack byte is
        // `+0x288`. Retail's own value is `4` (`(DAT_8007BD60 >> 5) & 4`,
        // `FUN_800513F0` `0x80051430`), not `1`.
        self.battle_ctx.scripted_fight = if scripted { 4 } else { 0 };
        // The same flag picks the monster seat family (`FUN_800513F0`'s row
        // index adds it), so a scripted fight is seated on rows `5..8`; the
        // map-gated arm (`0x800517A0..0x800517E0`) adds a second `+4`.
        let first_id = formation.slots.first().map_or(0, |s| s.monster_id);
        let map_arm =
            matches!(self.battle.map_id, 0x0C | 0x15) && (0x3D..=0x3F).contains(&first_id);
        self.seat_monster_family(monster_count, u8::from(scripted) + u8::from(map_arm));
        let first_monster = party_count;
        for slot in 0..party_count as usize {
            // The Spirit (AP) gauge `+0x170` opens at the character record's
            // saved AP `+0x10E`: the party loader's last store
            // (`FUN_80053CB8`, `lhu v0,0x6d6(record)` / `sh v0,0x170(actor)`
            // at `0x800542BC..0x800542C4`). The results arms write it back
            // ([`World::persist_battle_party_hp`]), so the gauge carries from
            // fight to fight rather than restarting empty.
            let saved_ap = self
                .party
                .roster
                .members
                .get(self.party_roster_slot(slot))
                .map(|r| r.hp_mp_sp().sp_cur);
            let a = &mut self.actors[slot];
            if let Some(ap) = saved_ap {
                a.battle.spirit_gauge = ap;
            }
            a.battle.liveness = 1;
            a.battle.action_category = 3; // Attack
            a.battle.active_target = first_monster;
            // Party members are not monsters - clear any id left from a
            // previous battle that placed an enemy in this slot.
            a.battle_monster_id = None;
            a.battle_element = None;
        }
        // Fold the roster's live stats + equipped-gear bonuses onto the party
        // combatants' attack / defense (no-op for a zeroed roster).
        self.seed_party_battle_stats();
        // Clear any monster-slot SPD / accuracy / evasion / defence split left
        // over from a previous battle so this formation's values are the only
        // ones seen. The split matters here as much as the scalars: a slot that
        // held a party member in the previous battle carries that member's
        // (UDF, LDF) pair, and a formation whose monster id misses the catalog
        // would otherwise keep defending with it.
        for s in self.battle.speed.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle.accuracy.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self.battle.evasion.iter_mut().skip(party_count as usize) {
            *s = 0;
        }
        for s in self
            .battle
            .defense_split
            .iter_mut()
            .skip(party_count as usize)
        {
            *s = None;
        }
        for (i, fslot) in formation.slots.iter().take(5).enumerate() {
            let mslot = party_count as usize + i;
            if mslot >= self.actors.len() {
                break;
            }
            // Tag the slot with its monster id so a renderer can fetch the
            // battle mesh, even if the catalog has no stats for it.
            // A slot that seated another monster in an earlier fight still
            // carries that monster's action clips: the scene host installs
            // clips only for a seat that has none, so a stale set survived
            // into this fight and the strike loop staged the old record's
            // entries (Gobu Gobu's block clip as Gimard's swing). Retail's
            // loader stages each seated record's own entries per fight.
            if self.actors[mslot].battle_monster_id != Some(fslot.monster_id) {
                let a = &mut self.actors[mslot];
                a.battle_action_clips = None;
                a.battle_animation = None;
                a.battle_staged_anim = None;
                a.battle_pose = None;
            }
            self.actors[mslot].battle_monster_id = Some(fslot.monster_id);
            self.actors[mslot].battle_element = None;
            if let Some(def) = self.tables.monster_catalog.get(fslot.monster_id) {
                // The installed stat block for THIS fight's class. The catalog
                // is built at scene entry, before any formation is chosen, so
                // it carries the scripted profile; a random encounter installs
                // the other one (x7/4 defence, unboosted ATK) and that is
                // every rollable fight in the game.
                //
                // `[AGL, ATK, UDF, LDF, INT, SPD]`.
                let bs = def.installed_stats(scripted);
                let (agl, attack, udf, ldf, intel, speed) =
                    (bs[0], bs[1], bs[2], bs[3], bs[4], bs[5]);
                let int_byte = intel.min(u8::MAX as u16);
                let a = &mut self.actors[mslot];
                a.battle.hp = def.hp;
                a.battle.max_hp = def.hp;
                a.battle.mp = def.mp;
                a.battle.liveness = 1;
                a.battle.action_category = 3;
                // Live + base action gauge (actor `+0x154` / `+0x156`). The
                // round boundary restores the live one from the base each
                // round (`FUN_801D88CC` loop A); the swing-budget loop spends
                // it.
                a.battle.agl_base = agl;
                a.battle.agl = agl;
                if let Some(s) = self.battle.attack.get_mut(mslot) {
                    *s = attack;
                }
                // Both defence facets, not one collapsed scalar. Retail's melee
                // kernel picks UDF (`+0x15C`) or LDF (`+0x160`) by the swing's
                // command parity (`FUN_801EC3E4` at `0x801ECE14`), and the same
                // pair feeds the art-strike and summon-roll defenders. Seeding
                // only `max(udf, ldf)` made every enemy defend with its better
                // half against every swing and left the kernel's parity branch
                // dead for the whole monster band.
                if let Some(s) = self.battle.defense_split.get_mut(mslot) {
                    *s = Some((udf, ldf));
                }
                if let Some(s) = self.battle.defense.get_mut(mslot) {
                    // Kept as the scalar fallback (and the Defense-buff target);
                    // the split above is what the physical path reads.
                    *s = udf.max(ldf);
                }
                if let Some(s) = self.battle.speed.get_mut(mslot) {
                    *s = speed;
                }
                if let Some(s) = self.battle.accuracy.get_mut(mslot) {
                    *s = int_byte;
                }
                if let Some(s) = self.battle.evasion.get_mut(mslot) {
                    *s = int_byte;
                }
            }
        }
        // Retail's record -> actor copy writes each stat into BOTH halfwords
        // of its pair, so every base half opens the fight equal to its working
        // half. That is what makes the base half a usable "has a debuff moved
        // this stat?" probe for the Seru side-effect stager.
        self.sync_battle_stat_bases();

        // Roll for a rare shiny capturable enemy now that every monster slot
        // carries its stats + id (so capturability + the +35% boost see final
        // values). Clears last battle's flags first.
        self.seru.shiny_enemy_slots.clear();
        self.seru.shiny_captures.clear();
        self.roll_shiny_enemy(first_monster);

        // Roll this battle's formation advantage (`FUN_80051D84` -> `ctx+0x290`)
        // now that both sides' SPD is final. Retail skips the roll entirely
        // when `ctx+0x287` is set (`0x80051DA4`) or the battle-stage id
        // `DAT_8007B64A` is (`0x80051DB8`), so the flagged boss fights and
        // the sparring tutorial (stage 1, a zero-header `3E FF` row) never
        // open on a back attack or a pre-emptive strike. The Rim Elm ambush
        // is neither, so it rolls, and its map-gated force is what raises
        // the Ra-Seru bit.
        // `enter_battle` above installs a fresh `battle_ctx`, so `+0x290` /
        // `+0x291` (and the arm's one-shot flag) are already zero - there is
        // one copy of each and it lives there.
        // Battle init's pass over the special-battle word (`FUN_800513F0`,
        // `0x800519C0..0x80051A04`), ahead of the roll that may raise the
        // same bit again.
        let lead_monster = formation
            .slots
            .first()
            .map(|s| s.monster_id as u8)
            .unwrap_or(0);
        self.battle.special_word =
            vm::battle_formulas::battle_init_special_word(self.battle.special_word, lead_monster);
        // The stage id as battle init leaves it: the tutorial arm
        // `enter_battle` consumed, or the `0xB5` override (`FUN_80055B6C`,
        // `0x80055D2C..0x80055D44`), which pages the arrival module in.
        if let Some(stage) = crate::battle_stage_module::battle_init_stage_override(lead_monster) {
            self.battle.stage_id = stage;
        }
        let stage_set = self.battle.stage_id != 0;
        if !scripted && !stage_set {
            self.roll_battle_formation(formation);
        }

        self.battle_ctx.queued_action = 3;
        self.battle_ctx.active_actor = 0;
        // Fresh battle: clear the monster-AI cooldowns / phase counter / ring.
        self.battle.monster_ai_state.reset();
        // ...and the stolen band + steal latch, which retail zeroes with the
        // same battle-load sweep (`crate::battle_steal`).
        self.battle.steal = crate::battle_steal::StealBand::default();
        self.battle.steal_caption = None;
        // Seed the turn-order initiative keys for this battle. When real SPD is
        // present the next-actor selector runs the initiative scheme from the
        // very first turn (see the opener pick below). A no-SPD battle leaves
        // every key at 0 and stays on the round-robin fallback.
        self.seed_battle_initiative();
        // Flow state `0x0A`'s banners off the unlatched `+0x290`. The action
        // SM's latch is not here: retail's state `0x00` runs at each round's
        // `0xFE` (`World::run_round_state_zero`), after round one's commands.
        self.open_battle_formation();
        // Switch to the battle track (if configured) - the host's BGM
        // director cross-fades from the field music.
        self.swap_to_battle_bgm();
        // Round 1 - retail `0x0B -> 0x14`, or `0x0B -> 0xFE` on a back
        // attack: the actor sweep, the keys seeded above (ahead of the latch,
        // so the side lockout read the unlatched `+0x290`), and `Begin | Run`
        // for the round's first party command. Nobody - whoever won
        // initiative - acts before the last member commits
        // (`World::begin_battle_round`).
        self.battle.round_flow = crate::battle_round::RoundFlow::default();
        self.begin_battle_round();
    }

    /// Configure the actor at `slot` as the field player and reset the
    /// per-scene collision grid.
    ///
    /// REF: FUN_8003aeb0
    ///
    /// Mirrors the player-actor setup in the
    /// scene-entry map-init `FUN_8003aeb0` (`player[+0x72] = 0x1000`) plus
    /// the per-frame delta scalar `DAT_1f800393` (defaulted to `1` when the
    /// world hasn't installed one). Idempotent across scene transitions.
    pub fn install_field_player(&mut self, slot: u8) {
        self.player_actor_slot = Some(slot);
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.active = true;
            actor.move_state.field_72 = FIELD_PLAYER_SPEED_MULT;
        }
        // Scene entry resets the ramp pool (`FUN_8003CDA8`): a ramp the last
        // scene left running must not resize the player on this one.
        self.locomotion.player_scale_ramps.reset_pool();
        if self.move_vm.ramp_ratio == 0 {
            self.move_vm.ramp_ratio = 1;
        }
        self.reset_field_collision_grid();
    }

    /// Seat the player actor at a field tile centre (`world = tile*128 +
    /// 0x40` - the same tile->world mapping the MAN placement spawns and the
    /// op-`0x3E` region check use, whose inverse is `(world - 0x40) >> 7`).
    ///
    /// This is the warp-arrival placement: a door transition (field-VM op
    /// `0x3F`) carries the destination entry tile in its trailing bytes, and
    /// [`crate::scene::SceneHost::tick`] calls this after the destination
    /// scene loads so the player stands at the door it arrived through
    /// instead of the cold-boot spawn. The floor height is sampled so the
    /// player lands on the destination's terrain tier rather than `y = 0`.
    ///
    /// The tile operand is taken **exactly** - retail writes the decoded
    /// coordinate straight onto the player and never consults the collision
    /// grid on arrival, and an authored door tile routinely *is* a closed
    /// cell: a door is a gap in a wall, so its trigger pad sits on the wall
    /// row. Nudging such a seat onto open floor lands the player off the
    /// destination's walk-on band, which is a dead door rather than a
    /// rescued one. Callers naming a *derived* tile - the
    /// `LEGAIA_START_TILE` debug seat, an encounter region's AABB centre -
    /// want [`World::seat_player_at_tile_rescued`] instead.
    pub fn seat_player_at_tile(&mut self, tile_x: u8, tile_z: u8) {
        self.seat_player_at_tile_inner(tile_x, tile_z, false);
    }

    /// [`World::seat_player_at_tile`] with a bounded wall rescue: a tile the
    /// walkability grid marks closed is nudged to the nearest open sub-cell
    /// ([`World::nearest_standable_seat`]), and past that radius the
    /// coordinate is returned unchanged.
    ///
    /// For callers whose tile is **derived rather than authored**, where
    /// there is no walk-on band to miss and landing inside a wall blocks
    /// every direction of [`World::step_field_locomotion`] with nothing on
    /// screen to explain it. Never use it on an op-`0x3F` arrival: see
    /// [`World::seat_player_at_tile`] for why a door tile reads as a wall.
    pub fn seat_player_at_tile_rescued(&mut self, tile_x: u8, tile_z: u8) {
        self.seat_player_at_tile_inner(tile_x, tile_z, true);
    }

    fn seat_player_at_tile_inner(&mut self, tile_x: u8, tile_z: u8, rescue: bool) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        // Retail entry-byte decode (`FUN_801DE840` case 0x3F): the low 7
        // bits are the tile, the high bit selects the far half of the tile
        // (`(b & 0x7F) * 0x80 + 0x40`, `+0x80` when bit 7 is set).
        let half =
            |b: u8| -> i16 { i16::from(b & 0x7F) * 128 + if b & 0x80 != 0 { 0x80 } else { 0x40 } };
        let (ax, az) = (half(tile_x), half(tile_z));
        let (wx, wz) = if rescue {
            self.nearest_standable_seat(ax, az)
        } else {
            (ax, az)
        };
        let wy = self.sample_field_floor_height(wx as i32, wz as i32) as i16;
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.move_state.world_x = wx;
            actor.move_state.world_y = wy;
            actor.move_state.world_z = wz;
        }
    }

    /// Face the player along a warp-arrival compass sector - the op-`0x3F`
    /// trailing `dir` byte. Retail resolves `dir & 7` through the 8-entry
    /// i16 table at SCUS `0x80073F04` (`[0, 0x200, 0x400, .. 0xE00]` - the
    /// eight 45-degree compass points of the 12-bit angle space) into the
    /// arrival-facing global `_DAT_80073EFC`; the engine stores the same
    /// angle on the player's heading (`move_state.render_26`, `0` = +Z).
    ///
    /// REF: FUN_801DE840 (case 0x3F facing write, table 0x80073F04)
    pub fn face_player_sector(&mut self, dir: u8) {
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        if let Some(actor) = self.actors.get_mut(slot as usize) {
            actor.move_state.render_26 = i16::from(dir & 7) * 0x200;
        }
    }

    /// One field-VM step. Drives `field_ctx` + `field_pc` from the loaded
    /// `field_bytecode`. No-op when no bytecode is loaded.
    pub fn step_field(&mut self) -> Option<FieldStepResult> {
        if self.field_bytecode.is_empty() {
            return None;
        }
        if let Some(res) = self.step_field_cross_context_cflag() {
            return Some(res);
        }
        let ctx_ptr: *mut FieldCtx = &mut self.field_ctx;
        let bc_ptr: *const Vec<u8> = &self.field_bytecode;
        let pc = self.field_pc;
        let mut host = FieldHostImpl { world: self };
        // SAFETY: FieldHostImpl never borrows `world.field_ctx` or
        // `world.field_bytecode` through the borrow.
        let ctx = unsafe { &mut *ctx_ptr };
        let bc: &[u8] = unsafe { (*bc_ptr).as_slice() };
        // A player-aimed `+0x72` write (every kingdom map's entry script
        // opens with `CC F8 40 00 0C 00 00`) lands on the player, not on the
        // system context: `field_step_routed`.
        let res = field_step_routed(&mut host, ctx, bc, pc);
        match &res {
            FieldStepResult::Advance { next_pc } => self.field_pc = *next_pc,
            FieldStepResult::Yield { resume_pc } => self.field_pc = *resume_pc,
            FieldStepResult::Halt { final_pc } => self.field_pc = *final_pc,
            FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                self.field_pc = *pc;
            }
        }
        // The field-VM borrow has ended; install any scripted encounter the
        // op 0x34 sub-2 forwarded-PC capture queued this step.
        self.drain_pending_scripted_encounter();
        Some(res)
    }

    /// A system-script `CFLAG_SET` / `CFLAG_CLR` aimed at another actor
    /// (`B1 <id> <bit>` / `B2 <id> <bit>`): `FUN_8003C83C` resolves `<id>`
    /// through the actor list and the write lands on **that** actor's
    /// `+0x10`, not on the system context. Raising bit 8 (`0x100`) engages
    /// the target: its per-actor tick then runs its script from where its
    /// last interaction's `0x21` left the PC (`FUN_8003BC08`,
    /// `0x8003BD10..0x8003BD38` -> `FUN_80039B7C`). `tunnelc`'s `P1[0]` is
    /// the case: on the post-battle pass it tests `0x361` and runs
    /// `B1 0C 08`, which restarts Xain's record past the `3E FF 0A` / `21`
    /// that staged the fight - the post-fight scene that sets `0x1D5`.
    ///
    /// Only placement contexts resolve here; the player (`0xF8`) and the
    /// system context (`0xFB`) take the ordinary step. The system context
    /// bypasses the halted-target early-out, as retail's `+0x50 == 0xFB`
    /// test does (`0x801DE90C..0x801DE940`).
    ///
    /// REF: FUN_801DE840 (cases 0x31 / 0x32), FUN_8003C83C, FUN_8003BC08
    fn step_field_cross_context_cflag(&mut self) -> Option<FieldStepResult> {
        let pc = self.field_pc;
        let op = *self.field_bytecode.get(pc)?;
        if op != 0xB1 && op != 0xB2 {
            return None;
        }
        let target = *self.field_bytecode.get(pc + 1)?;
        let bit = *self.field_bytecode.get(pc + 2)? & 0x1F;
        if target == crate::field_env::PLAYER_ANCHOR_TARGET || target == 0xFB {
            return None;
        }
        let ci = crate::field_channels::resolve_target(&self.field_vm.channels, target)?;
        let ch = &mut self.field_vm.channels[ci];
        if ch.object_bind {
            return None;
        }
        let mask = 1u32 << bit;
        if op == 0xB1 {
            // The engine runs no placement channel on its own (the touch
            // and this engagement play its interactions), so a stale `0x100`
            // left by the spawn pre-run does not mean the context is already
            // running: every bit-8 write queues the interaction.
            let engaging = mask == 0x100;
            ch.ctx.flags |= mask;
            if mask == 0x100 {
                ch.ctx.saved_26 = ch.ctx.field_26;
            }
            if engaging
                && !self
                    .field_vm
                    .pending_engagements
                    .contains(&ch.placement_index)
            {
                self.field_vm.pending_engagements.push(ch.placement_index);
            }
        } else {
            ch.ctx.flags &= !mask;
        }
        let next_pc = pc + 3;
        self.field_pc = next_pc;
        Some(FieldStepResult::Advance { next_pc })
    }

    /// One retail **frame slice** of the loaded field-VM script: keep
    /// executing opcodes until the context parks, exactly as retail's
    /// per-context runner does.
    ///
    /// [`Self::step_field`] is a single *instruction*. Retail never runs a
    /// context one instruction per frame - `FUN_8003CF7C` (and the identical
    /// loop `FUN_8003AB2C` runs when it installs the ctx-`0xFB` system script
    /// at scene load) calls the VM in a loop and leaves it only when one of
    /// three things happens:
    ///
    /// 1. the instruction it just executed was the `0x21` NOP - the authored
    ///    end-of-frame marker (`beq s0,s4` against `li s4,0x21`);
    /// 2. the returned PC equals the PC it went in with - a wait / halt
    ///    parking the context on its own instruction (`beq s2,v0`);
    /// 3. the byte at the new PC has `op & 0x7F < 0x20` - the text-segment /
    ///    terminator band, i.e. the script has walked into a dialogue payload
    ///    (`andi v0,s0,0x7f; sltiu v0,v0,0x20`).
    ///
    /// The same guard is applied before the first instruction, so a slice
    /// never *starts* inside text either.
    ///
    /// The per-actor channel runner has always paced itself this way (retail `FUN_80039B7C`'s own `0x21` break); the
    /// system script did not, and one op per tick is a ~20x slowdown on a
    /// scene's per-frame system loop. Concretely, `town01` `P1[0]` starts BGM
    /// 2016 at `+0x000C` and stops it 32 instructions later at `+0x0061` on a
    /// first visit: retail does both inside the load frame and nothing is
    /// heard, while one-op-per-tick plays half a second of it. The same loop
    /// carries the player-position bbox tests that pick the scene's per-region
    /// clear colour (`4C 13`), so those tracked the player at 3 Hz instead of
    /// per frame.
    ///
    /// Returns the last [`FieldStepResult`] the slice produced, or `None` when
    /// there is no bytecode / the PC already sits in the text band.
    ///
    /// PORT: FUN_8003CF7C
    /// REF: FUN_8003AB2C (the same loop, run once at scene-script install)
    pub fn step_field_frame_slice(&mut self) -> Option<FieldStepResult> {
        if self.field_bytecode.is_empty() {
            return None;
        }
        // Retail's pre-loop guard: a context whose PC sits on a text /
        // terminator byte executes nothing at all this frame.
        if self.field_bytecode.get(self.field_pc)? & 0x7F < 0x20 {
            return None;
        }
        // The system script is driven by the SYSTEM entity's tick
        // `FUN_801DA51C`, which runs it through the interaction runner
        // `FUN_80039B7C` only while the entity state `+0x8A` is 0 (not mid
        // encounter, `0x801DA750`) - and which starts a NEW pass only while
        // the player's `+0x10 & 0x80000` is down (`0x801DA794..0x801DA7AC`);
        // a pass already open continues regardless (`0x801DA78C`). Every
        // engaged interaction context raises that player bit on each frame
        // it runs (`0x80039DB8..0x80039DD4`), so a scene's system loop sits
        // out an NPC beat, a spawned record or a cutscene and resumes when
        // they release the player. The Rim Elm bee beat depends on it: its
        // `SET 0x5C0` precedes `3E FF 03` by an eight-frame wait, and the
        // system loop's `0x5C0` test spawns the fight's outcome record - it
        // must see the flag only on the post-battle pass.
        if self.field_scripts_held_for_battle()
            || (!self.field_vm.system_pass_open
                && (self.script_context_engages_player() || self.dialogue_owns_input()))
        {
            return None;
        }
        self.field_vm.system_pass_open = true;
        let mode = self.mode;
        // Re-seat the system context's position anchor on the live player
        // before the slice runs. See [`Self::sync_field_ctx_player_anchor`].
        // The kingdom overworld is a mode-3 field-run scene in retail and its
        // `P1[0]` park loop opens on the same `CD F8` whole-map box test, so
        // it needs the anchor as much as a field does.
        if matches!(
            mode,
            crate::world::SceneMode::Field | crate::world::SceneMode::WorldMap
        ) {
            self.sync_field_ctx_player_anchor();
        }
        let mut last = None;
        for _ in 0..FIELD_FRAME_SLICE_BUDGET {
            let pc = self.field_pc;
            let Some(&opcode_byte) = self.field_bytecode.get(pc) else {
                break;
            };
            last = self.step_field();
            if last.is_none() {
                break;
            }
            // Retail has no fourth condition here because a scene change tears
            // the context down; the engine queues the request and drains it
            // after the tick, so the loop has to stop itself or the ops after a
            // `0x3F` run against the scene that is already going away. Same for
            // a mode flip (a scripted battle) - the frame belongs to the new
            // mode from that instruction on.
            //
            // No entry script exercises this today: a disassembly of `P1[0]` in
            // all 124 CDNAME scenes finds zero `SceneChange` ops, and scene
            // changes live in the partition-2 timeline records and the
            // partition-1 interaction records that other steppers drive. It is
            // here because the cost of being wrong is executing a dead scene's
            // bytecode, and this is the only driver that runs a whole slice.
            if self.mode != mode
                || self.pending_named_scene_transition.is_some()
                || self.pending_scene_transition.is_some()
            {
                break;
            }
            let next = self.field_pc;
            // (1) the `0x21` NOP is the authored frame boundary; (2) a PC that
            // did not move is a park (wait, halt, unimplemented op); (3) the
            // next byte being a text segment ends the slice.
            if opcode_byte == 0x21 {
                // The pass ends: the runner clears the context's engaged bit
                // on the executed `0x21` (`0x80039E68..0x80039E7C`).
                self.field_vm.system_pass_open = false;
                break;
            }
            if next == pc {
                break;
            }
            match self.field_bytecode.get(next) {
                Some(&b) if b & 0x7F >= 0x20 => {}
                _ => break,
            }
        }
        last
    }

    /// Seat the ctx-`0xFB` system context's position anchor on the **live**
    /// player actor.
    ///
    /// The scene system script addresses the player as cross-context target
    /// `0xF8`, which retail resolves to the live player object
    /// (`_DAT_8007C364`) and reads `+0x14`/`+0x18` off *each time*. The
    /// system context has no position of its own, so the engine keeps the
    /// player's in `field_ctx` - and it has to re-seat it every frame slice,
    /// not once at scene load, because the script's park loop re-evaluates
    /// its `CD F8` bounding-box tests on every pass.
    ///
    /// Seeding it once was a live defect rather than a rounding error: the
    /// script buffer's install resets `field_ctx` to the default (position
    /// `0, 0`), and only the three opening scenes run the load-frame pre-run
    /// that re-seats it, so every other scene evaluated its `CD F8` tests
    /// against the **origin** for the whole visit. `conc`'s entry script is
    /// the measurable case: its park loop sets system flag `0x6DE` whenever
    /// the player is outside tiles `10..=51` x `14..=72`, and tile `(-1,-1)`
    /// is outside, so the flag latched on regardless of where the player
    /// stood. The same loop carries each scene's camera-parameter bbox
    /// gates.
    ///
    /// REF: FUN_8003CF7C (the per-frame slice this precedes)
    pub fn sync_field_ctx_player_anchor(&mut self) {
        if let Some(slot) = self.player_actor_slot
            && let Some(a) = self.actors.get(slot as usize)
        {
            self.field_ctx.world_x = a.move_state.world_x as u16;
            self.field_ctx.world_z = a.move_state.world_z as u16;
        }
    }

    /// Run the just-loaded scene-entry system script (ctx `0xFB`) through the
    /// passes retail runs before the scene's cutscene record takes the player.
    ///
    /// Retail's install (`FUN_8003AB2C`, `0x8003AD3C..0x8003AD88`) calls the
    /// dispatcher `FUN_801DE840` op after op and leaves after the op it ran
    /// was `0x21` (`beq s0,s4`, `s4 = 0x21`, at `0x8003AD58`), on a PC that did
    /// not move (`0x8003AD68`), or on a text byte (`0x8003AD84`). Each later
    /// frame the system SM `FUN_801DA51C` runs one more `0x21`-bounded pass,
    /// until a context holds the player - here, the opening's cutscene record,
    /// which the entry script itself spawns with op `0x44`: `opstati` inside
    /// the install slice, `opurud` on the second pass, `opdeene` on the third
    /// (after its body has run the `0x52F` arrival-fade arm and the one-hot
    /// region selector `0x19B..0x1AA` at the arrival tile).
    ///
    /// The engine installs those records as the cutscene timeline at scene
    /// entry, so the timeline would hold the player from the first tick and
    /// the passes before the spawn would never run. This runs them in the
    /// load frame instead: whole `0x21`-bounded passes, ending with the pass
    /// that executes the spawn, which leaves the system context where
    /// retail's sits once the record has taken over (`opdeene`: PC `0x99`, the
    /// pass closed). Without the pass boundary the run went on through the
    /// per-frame loop until its step budget, re-evaluating the region
    /// selector and ending mid-pass with the pass open - so the next tick
    /// finished the pass at whatever seat it then saw.
    ///
    /// REF: FUN_8003AB2C (system-script install slice), FUN_801DA51C (system
    /// SM pass gate)
    pub fn pre_run_entry_script(&mut self) {
        const ENTRY_SCRIPT_STEP_BUDGET: usize = 2048;
        // Passes retail runs at most before the spawn (opdeene's three), with
        // headroom; a script that never spawns stops here.
        const ENTRY_SCRIPT_PASS_LIMIT: usize = 8;
        self.sync_field_ctx_player_anchor();
        let mut passes = 0usize;
        let mut spawned = false;
        for _ in 0..ENTRY_SCRIPT_STEP_BUDGET {
            let op = self.field_bytecode.get(self.field_pc).copied();
            let result = self.step_field();
            if op == Some(0x44) {
                spawned = true;
            }
            if op == Some(0x21) {
                // The executed `0x21` ends the pass.
                self.field_vm.system_pass_open = false;
                passes += 1;
                if spawned || passes >= ENTRY_SCRIPT_PASS_LIMIT {
                    break;
                }
                self.field_vm.system_pass_open = true;
            }
            match result {
                // Continue through `Yield` as well as `Advance`: several
                // dispatcher arms exit via retail's `addiu s8, s8, N` PC-delta
                // idiom (e.g. the nibble-7 tile-wall ops), which the VM models
                // as a yield even though retail continues the same frame.
                // Real parks stop the run: `WaitFrames` mid-wait reports
                // `Halt` at PC, and unimplemented / text ops report Pending /
                // Unknown.
                Some(FieldStepResult::Advance { .. } | FieldStepResult::Yield { .. }) => continue,
                _ => break,
            }
        }
    }

    /// Drain a queued scripted-encounter install (set by the `+0x94`
    /// forwarded-PC capture host hook) into the active encounter session.
    /// No-op when nothing is queued. Called by [`Self::step_field`] once the
    /// field-VM borrow has ended.
    pub fn drain_pending_scripted_encounter(&mut self) {
        if let Some(record) = self.encounters.pending_scripted.take() {
            self.install_scripted_encounter(&record);
        }
    }
}

#[cfg(test)]
mod cross_context_cflag_tests {
    use crate::world::World;

    fn channel(slot: usize, script_id: u16) -> crate::field_channels::FieldChannel {
        crate::field_channels::FieldChannel {
            placement_index: slot,
            ctx: legaia_engine_vm::field::FieldCtx {
                script_id,
                ..Default::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        }
    }

    /// `tunnelc` `P1[0]`'s post-battle `B1 0C 08` lands on the actor whose
    /// id is `0x0C` - Xain's placement - not on the system context, and
    /// raising bit 8 queues that placement's interaction.
    #[test]
    fn system_script_bit8_engages_the_target_placement() {
        let mut w = World::default();
        w.field_vm.channels = vec![channel(3, 0x0B), channel(4, 0x0C)];
        w.load_field_script_at(vec![0xB1, 0x0C, 0x08, 0xB2, 0x0C, 0x16, 0x21], 0);
        w.step_field();
        assert_eq!(w.field_vm.channels[1].ctx.flags & 0x100, 0x100);
        assert_eq!(
            w.field_ctx.flags & 0x100,
            0,
            "the system context stays disengaged"
        );
        assert_eq!(w.field_vm.pending_engagements, vec![4]);
        w.field_vm.channels[1].ctx.flags |= 1 << 0x16;
        w.step_field();
        assert_eq!(w.field_vm.channels[1].ctx.flags & (1 << 0x16), 0);
        assert_eq!(w.field_pc, 6);
    }

    /// The player anchor and an unresolved id take the ordinary step.
    #[test]
    fn unresolved_targets_fall_through() {
        let mut w = World::default();
        w.field_vm.channels = vec![channel(4, 0x0C)];
        w.load_field_script_at(vec![0xB1, 0x0D, 0x08, 0x21], 0);
        w.step_field();
        assert!(w.field_vm.pending_engagements.is_empty());
    }
}
