//! The `World` side of an expansion pack: activation, and the battle,
//! encounter and spoils hooks the pack's `battle.toml` and
//! `progression.toml` reach the game through.
//!
//! Every hook here is the identity while [`World::expansion`] holds no pack:
//! each one reads [`legaia_engine_expansion::battle::BattleRuntime::compiled`]
//! (or the pack itself) first and returns untouched without one, so a world
//! with no pack plays exactly the retail game.
//!
//! The kernels these call are `World`-free and live in
//! `legaia_engine_expansion::{battle, progression}`.

use std::sync::Arc;

use legaia_engine_battle::encounter::{EncounterPhase, EncounterRoll};
use legaia_engine_battle::monster_catalog::FormationDef;
use legaia_engine_battle_loop::battle_state::BattleRewards;
use legaia_engine_expansion::battle::{self as xb, BattleRuntime, CompiledBattle};
use legaia_engine_expansion::battle_waves::{Moment, WaveRun};
use legaia_engine_expansion::progression::{self as xp, ProgressionRuntime};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::ExpansionPack;

use super::World;

impl World {
    /// Activate `pack`: compile its battle tables, install its formations and
    /// XP curve. Any pack already active is deactivated first. On an error
    /// (the pack's own problem list) the world is left without a pack.
    ///
    /// Monsters the pack builds on are merged from the disc's monster archive
    /// by the scene host at each scene entry
    /// ([`Self::expansion_base_monster_ids`]); a pack activated mid-scene has
    /// them from the next scene on, or once the host merges them itself.
    pub fn activate_expansion(&mut self, pack: Arc<ExpansionPack>) -> Result<(), Vec<String>> {
        self.deactivate_expansion();
        // A loaded save's dormant block this activation will adopt.
        let adopting =
            self.expansion_dormant_pack_id().as_deref() == Some(pack.manifest.id.as_str());
        let mut problems = pack.progression.problems();
        let compiled = match CompiledBattle::compile(&pack.manifest.id, &pack.battle) {
            Ok(c) => Some(c),
            Err(p) => {
                problems.extend(p);
                None
            }
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        let mut progression = ProgressionRuntime::new(&pack.progression);
        if pack.progression.xp_curve != Default::default() {
            let tracker = &mut self.party.level_up_tracker;
            let table = xp::xp_table(&pack.progression.xp_curve, &tracker.xp_table);
            progression.saved_xp_table = Some(std::mem::replace(&mut tracker.xp_table, table));
        }
        self.expansion.battle = BattleRuntime {
            compiled: compiled.map(Arc::new),
            ..Default::default()
        };
        self.expansion.progression = progression;
        self.expansion.pack = Some(pack);
        self.install_expansion_formations();
        // One activation path for battles and quests: the quest runtime
        // follows the pack (adopting progress a save left dormant for it).
        self.adopt_expansion_progress();
        // A save loaded before this pack was active: its "not loaded" notice
        // is now wrong, and a save made with another version of the pack
        // says what it lost instead.
        if adopting {
            self.expansion.toasts.clear();
        }
        self.toast_expansion_compat();
        self.compile_expansion_items();
        self.compile_expansion_arts();
        // The run rules' flags a pack can gate on (`run.ng_plus`, ...).
        self.sync_run_pack_flags();
        Ok(())
    }

    /// Drop the active pack and undo what activation installed: the XP
    /// curve, the pack formations and any catalog swap a fight still holds.
    pub fn deactivate_expansion(&mut self) {
        if self.expansion.pack.is_none() {
            return;
        }
        self.expansion_end_battle();
        self.expansion_release_item_aliases();
        self.expansion_release_arts();
        if let Some(c) = self.expansion.battle.compiled.as_ref() {
            for f in &c.formations {
                self.tables
                    .formation_table
                    .by_id
                    .remove(&f.def.formation_id);
            }
        }
        if let Some(table) = self.expansion.progression.saved_xp_table.take() {
            self.party.level_up_tracker.xp_table = table;
        }
        // The quest progress outlives the pack as the dormant block, with the
        // once-only boss record folded in, so a re-activation (or a save made
        // meanwhile) keeps it.
        self.expansion.quests.progress.cleared_bosses =
            std::mem::take(&mut self.expansion.battle.cleared_bosses);
        let mut quests = std::mem::take(&mut self.expansion.quests);
        quests.park();
        self.expansion = Default::default();
        self.expansion.quests = quests;
    }

    /// The retail monster ids the active pack's fights can seat (empty with
    /// no pack). The scene host merges these from the monster archive after
    /// its own PROT 867 merge.
    pub fn expansion_base_monster_ids(&self) -> Vec<u16> {
        self.expansion
            .battle
            .compiled
            .as_ref()
            .map(|c| c.base_monster_ids())
            .unwrap_or_default()
    }

    /// Register the pack's formations in the formation table. Idempotent;
    /// called at activation and before every pack fight, since a scene's
    /// MAN install replaces the table.
    pub fn install_expansion_formations(&mut self) {
        let Some(c) = self.expansion.battle.compiled.clone() else {
            return;
        };
        for f in &c.formations {
            self.tables.formation_table.insert(f.def.clone());
        }
    }

    /// The engine formation id of pack formation `formation` (local or
    /// qualified key).
    pub fn expansion_formation_id(&self, formation: &str) -> Option<u16> {
        self.expansion
            .battle
            .compiled
            .as_ref()?
            .formation_id(formation)
    }

    /// Start a fight on pack formation `formation` through the ordinary
    /// encounter path (transition, battle music, [`Self::begin_encounter_battle`]),
    /// as [`Self::force_encounter`] does. `false`, changing nothing, when no
    /// pack is active or it has no such formation.
    pub fn start_expansion_battle(&mut self, formation: &str) -> bool {
        let Some(id) = self.expansion_formation_id(formation) else {
            log::error!("expansion: no pack formation `{formation}`");
            return false;
        };
        self.install_expansion_formations();
        self.force_encounter(id)
    }

    /// Start the pack boss `boss`: its formation through
    /// [`Self::start_expansion_battle`]'s path, with its battle track and its
    /// victory reward armed. `false` when there is no such boss, or it is a
    /// once-only boss the party has already beaten.
    pub fn start_expansion_boss(&mut self, boss: &str) -> bool {
        let Some(c) = self.expansion.battle.compiled.clone() else {
            return false;
        };
        let Some(key) = legaia_expansion_format::battle::local_key(&c.pack_id, boss) else {
            return false;
        };
        let Some(b) = c.bosses.get(key) else {
            log::error!("expansion: no pack boss `{boss}`");
            return false;
        };
        if b.once && self.expansion.battle.cleared_bosses.contains(key) {
            log::info!("expansion: boss `{boss}` is already beaten");
            return false;
        }
        // The boss's intro plays first; the fight starts as it ends. Off
        // its map (or with another cutscene running) the fight starts now.
        let intro = self
            .expansion
            .pack
            .as_ref()
            .and_then(|p| p.battle.bosses.get(key))
            .and_then(|d| d.intro.clone());
        let played = self.expansion.battle.intro_played.take();
        if let Some(intro) = intro
            && played.as_deref() != Some(key)
            && self.start_expansion_cutscene(&intro)
        {
            self.expansion.battle.boss_intro = Some((intro, key.to_string()));
            return true;
        }
        self.install_expansion_formations();
        if !self.force_encounter(b.formation_id) {
            return false;
        }
        self.expansion.battle.active_boss = Some(key.to_string());
        self.expansion.battle.battle_bgm = self.expansion_boss_track(b.music.as_ref()).or(b.bgm);
        true
    }

    /// Whether once-only boss `boss` has been beaten.
    pub fn expansion_boss_cleared(&self, boss: &str) -> bool {
        let Some(c) = self.expansion.battle.compiled.as_ref() else {
            return false;
        };
        legaia_expansion_format::battle::local_key(&c.pack_id, boss)
            .is_some_and(|k| self.expansion.battle.cleared_bosses.contains(k))
    }

    /// The battle track the next fight's music swap plays in place of the
    /// configured one: the active boss's, else `audio.toml`'s `music.battle`
    /// for a fight on one of the pack's maps.
    pub(in crate::world) fn expansion_battle_bgm(&self) -> Option<u16> {
        self.expansion
            .battle
            .battle_bgm
            .or_else(|| self.expansion_map_battle_track())
    }

    /// The strongest present member's level: what enemy scaling reads.
    fn expansion_party_level(&self) -> u8 {
        (0..usize::from(self.party.party_count.clamp(1, 3)))
            .filter_map(|m| {
                self.party
                    .level_up_tracker
                    .level
                    .get(self.party_roster_slot(m))
                    .copied()
            })
            .max()
            .unwrap_or(1)
    }

    /// Seat the pack's numbers for a fight on `formation`: swap each pack
    /// monster (and, under enemy scaling, each scaled monster) into the
    /// catalog under its base id, keeping the scene's entries to restore.
    /// Runs at the top of [`Self::enter_battle_from_formation`], before any
    /// stat is read.
    pub(in crate::world) fn expansion_prepare_battle(&mut self, formation: &FormationDef) {
        self.expansion_restore_catalog();
        self.expansion.arts.playing.clear();
        self.expansion_end_pack_cast();
        self.expansion_install_spell_entries();
        let Some(c) = self.expansion.battle.compiled.clone() else {
            return;
        };
        let pack_formation = c.formation(formation.formation_id).is_some();
        let (scale, rewards) = match self
            .expansion
            .pack
            .as_ref()
            .and_then(|p| p.progression.enemy_scaling.as_ref())
        {
            Some(s) if xp::scaling_applies(s, pack_formation) => (
                xp::enemy_scaling_permille(s, self.expansion_party_level()),
                s.rewards,
            ),
            _ => (1000, false),
        };
        // A formation with later waves swaps every wave's monsters at once:
        // one swap covers the whole battle.
        let waved = c
            .formation(formation.formation_id)
            .filter(|f| !f.waves.is_empty());
        // Every wave's monsters and every boss summon swap in at once.
        let planned = c
            .formation(formation.formation_id)
            .filter(|f| !f.waves.is_empty() || !f.summons.is_empty())
            .map_or_else(|| formation.clone(), |f| f.all_waves_def());
        self.expansion.battle.waves =
            waved.map(|f| WaveRun::new(f.waves.clone(), formation.slots.len()));
        self.expansion.battle.reseated.clear();
        self.expansion_boss_prepare(formation);
        let catalog = &self.tables.monster_catalog;
        let plan = xb::plan_battle(&c, &planned, |id| catalog.get(id).cloned(), scale, rewards);
        if pack_formation {
            for slot in &planned.slots {
                if catalog.get(slot.monster_id).is_none() {
                    log::error!(
                        "expansion: formation {} seats monster {} with no catalog entry - the \
                         host has not merged the pack's base monsters",
                        formation.formation_id,
                        slot.monster_id
                    );
                }
            }
        }
        for def in plan.defs {
            let id = def.id;
            let old = self.tables.monster_catalog.by_id.insert(id, def);
            self.expansion.battle.shadowed.push((id, old));
        }
        self.expansion.battle.steals = plan
            .steals
            .into_iter()
            .map(|(id, chance, item)| (id, (chance, item)))
            .collect();
        self.expansion_lend_item_aliases(&plan.pack_drops, &plan.pack_steals);
    }

    /// Put back the catalog entries a fight's swap replaced.
    fn expansion_restore_catalog(&mut self) {
        for (id, old) in std::mem::take(&mut self.expansion.battle.shadowed)
            .into_iter()
            .rev()
        {
            match old {
                Some(def) => {
                    self.tables.monster_catalog.by_id.insert(id, def);
                }
                None => {
                    self.tables.monster_catalog.by_id.remove(&id);
                }
            }
        }
        self.expansion.battle.steals.clear();
    }

    /// End-of-battle cleanup: restore the catalog and drop the boss staging.
    /// Runs after the spoils, from [`Self::finish_battle`] and
    /// [`Self::abandon_live_battle`].
    pub(in crate::world) fn expansion_end_battle(&mut self) {
        self.expansion_restore_catalog();
        self.expansion.battle.waves = None;
        self.expansion.battle.reseated.clear();
        self.expansion_boss_end();
        self.expansion.arts.playing.clear();
        self.expansion_end_pack_cast();
        self.expansion.battle.active_boss = None;
        self.expansion.battle.battle_bgm = None;
    }

    /// The steal entry a pack monster carries in place of the static table's.
    pub(in crate::world) fn expansion_steal_override(
        &self,
        monster_id: u16,
    ) -> Option<legaia_game_tables::steal_table::StealEntry> {
        self.expansion
            .battle
            .steals
            .get(&monster_id)
            .map(
                |&(chance_pct, item_id)| legaia_game_tables::steal_table::StealEntry {
                    chance_pct,
                    item_id,
                },
            )
    }

    /// Scale a victory's summed EXP and credited gold by the pack's
    /// multipliers.
    pub(in crate::world) fn expansion_scale_spoils(&self, xp_total: u32, gold: u32) -> (u32, u32) {
        let p = &self.expansion.progression;
        (
            xb::scale_permille(xp_total, p.xp_permille, u32::MAX),
            xb::scale_permille(gold, p.gold_permille, u32::MAX),
        )
    }

    /// A won boss fight's own reward, on top of the formation's spoils.
    pub(in crate::world) fn expansion_boss_victory(&mut self, rewards: &mut BattleRewards) {
        let Some(key) = self.expansion.battle.active_boss.take() else {
            return;
        };
        let Some(c) = self.expansion.battle.compiled.clone() else {
            return;
        };
        let Some(b) = c.bosses.get(&key) else {
            return;
        };
        // The boss staging survives a fight that never came; only pay the
        // fight that seated the boss's own formation.
        let fought = self
            .battle
            .active_formation
            .as_ref()
            .is_some_and(|f| f.formation_id == b.formation_id);
        if !fought {
            return;
        }
        for (item, count) in &b.reward_items {
            // A retail reward is named on the results window by its id; a
            // pack reward by its toast (it has no id outside a fight).
            if self.grant_item_ref(item, *count)
                && let Some(id) = item.retail_id()
            {
                rewards.drops.push(id);
            }
        }
        if b.reward_gold > 0 {
            let money = i64::from(self.party.money).saturating_add(i64::from(b.reward_gold));
            self.party.money = money.min(i64::from(super::BATTLE_GOLD_CAP)) as i32;
            rewards.gold = rewards.gold.saturating_add(b.reward_gold);
        }
        let outro = self
            .expansion
            .pack
            .as_ref()
            .and_then(|p| p.battle.bosses.get(&key))
            .and_then(|d| d.outro.clone());
        if let Some(c) = outro {
            // Played back on the field, after the results and the quest's
            // own lines.
            self.expansion_play_cutscene(&c);
        }
        if b.once {
            self.expansion.battle.cleared_bosses.insert(key);
        }
    }

    /// Grant the pack's level-up rewards for roster slot `char_slot`
    /// levelling `old -> new`.
    pub(in crate::world) fn expansion_level_rewards(&mut self, char_slot: u8, old: u8, new: u8) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        for (item, count) in xp::level_rewards(&pack.progression.level_rewards, char_slot, old, new)
        {
            self.grant_item_ref(&item, count);
        }
    }

    /// Whether the active pack overrides the active scene's encounters - on
    /// a pack map, the encounter zone's table ([`Self::expansion_encounter_key`]).
    fn expansion_encounter(&self) -> Option<legaia_engine_expansion::battle::CompiledEncounter> {
        let key = self.expansion_encounter_key()?;
        self.expansion
            .battle
            .compiled
            .as_ref()?
            .encounters
            .get(&key)
            .cloned()
    }

    /// Whether the active pack swaps the formations retail's own rate rolls
    /// in the active scene.
    pub(in crate::world) fn expansion_substitutes_rolls(&self) -> bool {
        self.expansion_encounter()
            .is_some_and(|e| e.steps.is_none())
    }

    /// The formation a retail roll fights under the pack's override for the
    /// active scene; `roll` itself with no override.
    pub(in crate::world) fn expansion_substitute_roll(
        &mut self,
        roll: EncounterRoll,
    ) -> EncounterRoll {
        let Some(enc) = self.expansion_encounter().filter(|e| e.steps.is_none()) else {
            return roll;
        };
        let rand = self.next_rand();
        let Some(id) = xb::pick_formation(&enc, Some(roll.formation_id), rand) else {
            return roll;
        };
        if id == roll.formation_id {
            return roll;
        }
        self.install_expansion_formations();
        log::info!(
            "expansion: encounter in '{}' -> pack formation {id:#06X}",
            self.active_scene_label
        );
        EncounterRoll {
            formation_id: id,
            row_index: usize::from(id),
            roll_q8: roll.roll_q8,
        }
    }

    /// One field step under a pack that owns the active scene's encounter
    /// rate. `None` when it does not (retail's roll runs); otherwise whether
    /// the step triggered a fight.
    ///
    /// The pack's counter replaces the scene's region and mean-rate rolls
    /// outright, so a scene retail never fights in can hold encounters. The
    /// session's transition / grace bracket, the Incense window and the
    /// engaged hold still gate it.
    pub(in crate::world) fn expansion_encounter_step(&mut self) -> Option<bool> {
        let enc = self.expansion_encounter()?;
        let steps = enc.steps?;
        if self.encounters.session.is_none() {
            self.install_encounter_bracket();
        }
        let idle = self
            .encounters
            .session
            .as_ref()
            .is_none_or(|s| matches!(s.phase(), EncounterPhase::Idle));
        if !idle || self.locomotion.walk_regen_window != 0 || self.encounter_roll_held() {
            return Some(false);
        }
        let scene = self.expansion_encounter_key().unwrap_or_default();
        let left = match self.expansion.battle.step_counter.take() {
            Some((s, n)) if s == scene => n,
            _ => {
                let r = self.next_rand();
                xb::draw_steps(steps, r)
            }
        };
        let left = left.saturating_sub(1);
        if left > 0 {
            self.expansion.battle.step_counter = Some((scene, left));
            return Some(false);
        }
        let r = self.next_rand();
        let next = xb::draw_steps(steps, r);
        self.expansion.battle.step_counter = Some((scene, next));
        let r = self.next_rand();
        let id = xb::pick_formation(&enc, None, r)?;
        self.install_expansion_formations();
        let roll = EncounterRoll {
            formation_id: id,
            row_index: usize::from(id),
            roll_q8: 0,
        };
        log::info!(
            "expansion: pack encounter in '{}' -> formation {id:#06X}",
            self.active_scene_label
        );
        Some(
            self.encounters
                .session
                .as_mut()
                .is_some_and(|s| s.trigger_with(roll)),
        )
    }

    /// Seat what an expansion fight's wave run brings at `moment`
    /// ([`legaia_engine_expansion::battle_waves`]): the next wave once the
    /// monster side is down, or a refill wave's arrivals into freed seats as
    /// a round opens. `false`, changing nothing, when the fight has no waves
    /// still to come or nothing arrives now - always so without a pack.
    ///
    /// Each arrival takes its monster seat through the battle entry's own
    /// seat step ([`Self::seed_monster_seat_stats`]) and joins the fight's
    /// formation, so the one victory pays every wave's spoils together.
    pub(in crate::world) fn expansion_seat_wave(&mut self, moment: Moment) -> bool {
        if self.mode != SceneMode::Battle || self.battle.victory.is_some() {
            return false;
        }
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let standing = self.expansion_standing_seats();
        let Some(run) = self.expansion.battle.waves.as_mut() else {
            return false;
        };
        let arrivals = run.arrivals(moment, &standing);
        if arrivals.is_empty() {
            return false;
        }
        let total_waves = run.total_waves();
        let scripted = self.battle.scripted_fight;
        if let Some(layout) = arrivals.layout {
            self.expansion_relayout_monsters(layout);
        }
        let layout = self
            .expansion
            .battle
            .waves
            .as_ref()
            .map_or(1, |r| r.layout());
        for &(k, id) in &arrivals.seats {
            let slot = pc + k;
            if slot >= self.actors.len() {
                continue;
            }
            let seat = legaia_engine_battle::battle_seats::monster_seat(layout, k, scripted);
            self.seat_wave_monster(slot, k, id, seat, scripted);
            if let Some(f) = self.battle.active_formation.as_mut() {
                f.slots
                    .push(legaia_engine_battle::monster_catalog::FormationSlot::new(
                        id,
                    ));
            }
        }
        if let Some(&n) = arrivals.waves_begun.last() {
            let text = format!("Wave {n} / {total_waves}");
            log::info!(
                "expansion: {text} - {} monster(s) seated",
                arrivals.seats.len()
            );
            self.battle.message_banner = Some(
                legaia_engine_battle_loop::message_banner::BattleMessageBanner {
                    element: legaia_engine_battle_loop::message_banner::TIMED_MESSAGE_ELEMENT,
                    text,
                    hold: WAVE_BANNER_HOLD,
                },
            );
        }
        true
    }

    /// Which monster seats (`0..MAX_FORMATION_SLOTS` within the monster
    /// block) hold a monster still standing; a seat that does not is free.
    pub(in crate::world) fn expansion_standing_seats(&self) -> Vec<bool> {
        use legaia_expansion_format::battle::MAX_FORMATION_SLOTS;
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        (0..MAX_FORMATION_SLOTS)
            .map(|k| {
                let s = (pc + k) as u8;
                self.actors
                    .get(pc + k)
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some())
                    && !self.actor_effectively_defeated(s)
                    && self.raw_status_word(s) & 0x4 == 0
            })
            .collect()
    }

    /// Lay the monster block out for `layout` monsters: the seats are read
    /// off the stage seat table for the new count, and a monster still
    /// standing moves to its new seat.
    pub(in crate::world) fn expansion_relayout_monsters(&mut self, layout: u8) {
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let scripted = self.battle.scripted_fight;
        for k in 0..usize::from(layout) {
            let seat = legaia_engine_battle::battle_seats::monster_seat(layout, k, scripted);
            if let Some(a) = self.actors.get_mut(pc + k) {
                a.move_state.world_x = seat.x;
                a.move_state.world_y = seat.y;
                a.move_state.world_z = seat.z;
                if a.battle.seat.is_some() {
                    a.battle.seat = Some((seat.x, seat.z));
                }
            }
        }
    }

    /// Put arriving monster `id` on actor slot `slot` (monster seat `k`) at
    /// `seat`: a fresh battle actor, its catalog stats, and the per-seat
    /// battle state a fight's entry starts clear.
    pub(in crate::world) fn seat_wave_monster(
        &mut self,
        slot: usize,
        k: usize,
        id: u16,
        seat: legaia_engine_battle::battle_seats::Seat,
        scripted: bool,
    ) {
        let a = &mut self.actors[slot];
        let same_body = a.battle_monster_id == Some(id) && a.tmd_binding.is_some();
        a.active = true;
        a.battle = Default::default();
        a.battle.liveness = 1;
        a.battle.facing_angle = 0x800;
        a.move_state.world_x = seat.x;
        a.move_state.world_y = seat.y;
        a.move_state.world_z = seat.z;
        // The clip set, idle and morphs are the scene host's to install for
        // a seat that has none, as at the battle's own entry; a different
        // monster also needs its mesh bound by the play host.
        a.battle_action_clips = None;
        a.battle_clip_morphs = None;
        a.battle_four_slot_beats = None;
        a.battle_animation = None;
        a.battle_staged_anim = None;
        a.battle_pose = None;
        a.battle_reaction = None;
        a.battle_reaction_entry = None;
        a.battle_reaction_next = None;
        a.battle_effect_script = None;
        a.battle_effect_cursor = 0;
        a.battle_anim_cue_cursor = 0;
        a.battle_pose_history.clear();
        a.battle_element = None;
        a.battle_monster_id = Some(id);
        if !same_body {
            a.tmd_binding = None;
            a.battle_tex_slot = None;
        }
        self.expansion.battle.reseated.push(slot);
        // A seat a new monster takes starts a fresh boss-script run.
        self.expansion.battle.boss_runs.remove(&slot);
        for v in [
            &mut self.battle.speed[slot],
            &mut self.battle.accuracy[slot],
            &mut self.battle.evasion[slot],
        ] {
            *v = 0;
        }
        self.battle.defense_split[slot] = None;
        self.seed_monster_seat_stats(slot, id, scripted);
        let b = &mut self.battle;
        b.attack_base[slot] = b.attack[slot];
        b.defense_base[slot] = b.defense_split[slot];
        b.speed_base[slot] = b.speed[slot];
        b.accuracy_base[slot] = b.accuracy[slot];
        b.status_effects.drop_slot(slot as u8);
        b.monster_ai_state.dat[k] = 0;
        b.monster_ai_state.dat[k + 4] = 0;
        let steal = &mut b.steal;
        if steal.resolved.get(k).copied().unwrap_or(false) {
            steal.resolved[k] = false;
            steal.stolen[k] = 0;
            steal.took[k] = 0;
        }
    }

    /// Actor slots an expansion wave seated a new monster on since the last
    /// call: a play host binds each one's battle mesh, as it does for the
    /// battle's opening monsters at entry. Empty without a pack.
    pub fn take_expansion_reseated_monsters(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.expansion.battle.reseated)
    }
}

/// How long the "Wave N" banner stands, in frames: the timed message's own
/// hold ([`legaia_engine_battle_loop::message_banner::COUNTER_MESSAGE_HOLD`]).
const WAVE_BANNER_HOLD: i32 = legaia_engine_battle_loop::message_banner::COUNTER_MESSAGE_HOLD;
