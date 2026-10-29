//! Battle teardown: finish / loot / field restore, and the monster-slot render
//! bridge.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// How long the post-battle spoils panel stays up, in sim ticks
    /// (~3 s at the 100 Hz sim clock).
    pub const SPOILS_BANNER_FRAMES: u16 = 300;

    /// A dialog line as display text: printable ASCII as itself, each
    /// `0xC1 k` name escape spliced with roster record `k`'s name (`0x63` =
    /// the party leader), every other `0xC0..=0xCF` escape dropped with its
    /// operand.
    pub fn expand_name_escapes(&self, raw: &[u8]) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < raw.len() {
            let b = raw[i];
            if (0xC0..=0xCF).contains(&b) {
                let op = raw.get(i + 1).copied().unwrap_or(0);
                if b == 0xC1 {
                    let slot = if op == 0x63 {
                        self.party_roster_slot(0)
                    } else {
                        usize::from(op)
                    };
                    let name = self
                        .party
                        .roster
                        .members
                        .get(slot)
                        .map(|m| m.name())
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| format!("Member {}", slot + 1));
                    out.push_str(&name);
                }
                i += 2;
                continue;
            }
            if (0x20..0x7F).contains(&b) {
                out.push(b as char);
            }
            i += 1;
        }
        out
    }

    /// The battle exit's party loop, run on every exit before the party's
    /// HP is persisted: the status words clear unless the special-battle word
    /// carries the arena bit, and a member at 0 HP stands up at 1
    /// ([`vm::battle_formulas::battle_exit_party_reset`]). The cleared word is
    /// what reaches the character record, so the party's statuses do not
    /// outlive an ordinary battle.
    ///
    /// The engine keeps no status in the record: an arena leg's statuses
    /// stay in the tracker instead, which is where the next battle reads
    /// them from.
    fn battle_exit_party_reset(&mut self) {
        let word = self.special_battle_word();
        let n = usize::from(self.party.party_count).min(self.actors.len());
        for member in 0..n {
            // The tracker holds the word; any non-zero stand-in asks the
            // kernel whether it survives.
            let hp0 = self.actors[member].battle.hp;
            let (status, hp) =
                legaia_engine_battle_vm::battle_formulas::battle_exit_party_reset(word, 1, hp0);
            if status == 0 {
                self.battle.status_effects.drop_slot(member as u8);
            }
            let b = &mut self.actors[member].battle;
            if hp != b.hp {
                b.hp = hp;
                b.liveness = 1;
            }
        }
    }

    /// Resolve a finished battle and return to the field.
    ///
    /// On [`BattleEndCause::MonsterWipe`] applies loot (XP / gold / drops /
    /// level-ups) via [`Self::apply_battle_loot`] against the captured
    /// formation. On [`BattleEndCause::PartyWipe`] the retail gate in MAIN
    /// INIT's back-from-battle arm (`FUN_8003AEB0` `0x8003B598..0x8003B5F0`)
    /// forks on story-flag index 0, the scripted-loss latch (`0x80085758`
    /// bit `0x80` = system flag 0 in the port's MSB-first bank):
    ///
    /// - latch **set** (a scripted-loss battle, e.g. the Rim Elm ambush):
    ///   the wipe returns to the field like any battle end and MAIN INIT
    ///   consumes the latch (`andi 0x7f` at `0x8003B608`) - the story
    ///   continues, no game over.
    /// - latch **clear**: the hand-off stores `game_mode = 0x16` (CARD
    ///   INIT) + `_DAT_8007BB00 = 1` and pauses the BGM
    ///   (`jal 0x800266E0(0x8007052C)` at `0x8003B5EC` - the same primitive
    ///   as BGM sub-op 2). The port raises [`Self::game_over`], queues the
    ///   pause instead of the field-BGM restore, and **defers** the field
    ///   restore ([`Self::game_over_hold`]) so hosts hold the frozen battle
    ///   frame until [`Self::resolve_game_over_hold`].
    ///
    /// Both wipe arms clear story-flag index 1 (`andi 0xbf` at
    /// `0x8003B5A0`); every other ending **sets** it (`ori 0x40` at
    /// `0x8003B58C`) - the script-readable battle outcome
    /// ([`legaia_engine_battle::battle_return_flags`]). Non-wipe endings restore the field
    /// actor snapshot, drop the encounter session into its grace window, and
    /// flip the scene mode back to [`SceneMode::Field`].
    // REF: FUN_8003AEB0 (the back-from-battle game-over gate this folds)
    pub(in crate::world) fn finish_battle(&mut self) {
        if self.game_over_hold {
            // The wipe hold parks the scene in Battle mode, so a host that
            // keeps ticking the world re-runs the action SM's wipe scan and
            // re-raises `battle_end` every tick. The fold already happened;
            // consume the repeat and keep the hold frozen.
            self.battle.end = None;
            return;
        }
        // A battle that ran the end-of-battle presentation credited its
        // rewards on the results frame (`world::battle::victory`); a direct
        // caller (the runner path, tests) credits them here and arms the
        // aging spoils window instead.
        // A Muscle Dome leg returns to the arena: the exit selector
        // `FUN_80046A20` stores mode `0x18` for a word carrying `0x100`
        // (`0x80046E50`) instead of the field's `0x2`, so the field's
        // back-from-battle arm (`FUN_8003AEB0`: the story-flag stores, the
        // game-over gate, the escape-clock penalty) never runs for it.
        let dome_leg = self.battle.return_mode == SceneMode::MuscleDome;
        let dome_end = if self.battle.escaped {
            Some(BattleEndCause::Escaped)
        } else {
            self.battle.end
        };
        let loot_done = std::mem::replace(&mut self.battle.loot_applied, false);
        if !loot_done
            && self.battle.end == Some(BattleEndCause::MonsterWipe)
            && let Some(formation) = self.battle.active_formation.clone()
        {
            // `apply_battle_loot` borrows the catalog while mutating self, so
            // swap it out and back around the call.
            let catalog = std::mem::take(&mut self.tables.monster_catalog);
            let rewards = self.apply_battle_loot(&formation, &catalog);
            self.tables.monster_catalog = catalog;
            self.battle.last_rewards = Some(rewards);
            // Arm the spoils panel. The numbers were always applied; nothing
            // ever told the player about them. A special battle opens no
            // result window (`0x8004F614`), so it arms none.
            if self.special_battle_word() == 0 {
                self.battle.spoils_frames = Self::SPOILS_BANNER_FRAMES;
            }
        }
        self.battle.victory = None;
        // The fade actor dies with the battle scene: the held black of the
        // exit / escape template (`holds_at_end`) comes down here, never in
        // the world tick.
        self.presentation.fade = None;
        self.presentation.module_fades.clear();
        self.battle.entry_fade = None;
        self.casting.module_pres = Default::default();
        // MAIN INIT's back-from-battle flag stores, run for every ending: the
        // party-survived bit `DAT_8007BD60 & 0x80` is clear only after a
        // party wipe, so every other end - a monster wipe, an escape, a
        // scripted stage exit - raises story flag 1, the outcome a scene
        // script tests after the fight; a wipe clears it. Flag 0 (the
        // scripted-loss latch) is consumed either way, and a wipe without it
        // is the game over. `wipe_to_title` is `true` only on that arm; a
        // wipe under the latch takes the ordinary field return below.
        let survived = self.battle.end != Some(BattleEndCause::PartyWipe);
        let wipe_to_title = !dome_leg && {
            let ret = legaia_engine_battle::battle_return_flags::apply_battle_return_flags(
                &mut WorldFlagBank(self),
                survived,
            );
            ret.game_over
        };
        if wipe_to_title {
            self.game_over = true;
        }
        // The same arm takes twenty seconds off a running escape clock
        // (`0x8003B62C..0x8003B648`), on every arm of it.
        if !dome_leg {
            self.battle.escape_timer.field_return_penalty();
        }
        self.battle.active_formation = None;
        self.battle.end = None;
        // Drop the battle seat anchors - the next battle's setup re-seats
        // the actors and the first locomotion tick re-seeds the pair
        // (`World::tick_battle_locomotion`).
        for a in self.actors.iter_mut() {
            a.battle.seat = None;
        }
        // The monster seats' ailments die with their combatants: retail builds
        // each battle's monster actors afresh, and the tracker is indexed by
        // slot, so a status left here would land on the next battle's monster
        // in the same slot.
        for slot in
            self.party.party_count..legaia_engine_battle_vm::battle_action::ACTOR_SLOTS as u8
        {
            self.battle.status_effects.drop_slot(slot);
        }
        self.battle_exit_party_reset();
        self.battle.escaped = false;
        self.battle.no_escape = false;
        self.battle.scripted_fight = false;
        self.battle.guarding = [false; 3];
        if wipe_to_title {
            // Retail's wipe hand-off never resumes the field track: the arm
            // pauses the sequencer (`jal 0x800266E0(0x8007052C)` at
            // `0x8003B5EC`, the primitive BGM sub-op 2 wraps - the same call
            // the scripted `4C EA` trigger routes) and the CARD / title flow
            // owns audio from there. Drop the swap bookkeeping so nothing
            // later cross-fades back to the field track.
            self.audio.battle_bgm_active = false;
            self.audio.field_bgm_resume = None;
            self.pending_field_events
                .push(legaia_engine_field::field_events::FieldEvent::Bgm {
                    text_id: 0,
                    sub_op: 2,
                });
        } else {
            // Restore the field track stashed at encounter start (cross-fades
            // back from the battle music). No-op if no swap was active.
            self.restore_field_bgm();
        }
        // Revert any lingering buff deltas so the per-slot scalars return to
        // base, then drop the trackers + captured-id log (a new battle re-inits
        // these).
        let buffs = std::mem::take(&mut self.battle.buffs);
        for b in buffs {
            self.battle
                .add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        // Revert any Fury Boost AP-gauge extension (class-5 item) and clear the
        // per-slot flags, so the next battle starts from the base gauge.
        for idx in 0..self.battle.ap_gauges.len() {
            if let Some(delta) = self.battle.fury_boost[idx].take() {
                let gauge = &mut self.battle.ap_gauges[idx];
                gauge.base_ap = gauge.base_ap.saturating_sub(delta);
                gauge.current_ap = gauge.current_ap.min(gauge.ceiling());
                // ... and the `+0x1F9` flag the round boundary reads.
                if let Some(a) = self.actors.get_mut(idx) {
                    a.battle.spirit_shield = 0;
                }
            }
        }
        // Bank any captured Seru into learning progress (drains battle_captures).
        self.resolve_captures();
        // Drop any open command / item / spell session - they belong to the
        // finished battle.
        self.battle.command = None;
        self.battle.item_menu = None;
        self.battle.spell_menu = None;
        self.battle.arts_menu = None;
        self.battle.arts_input = None;
        self.battle.arts_list_window = None;
        // Stale damage popups + sound cues must not bleed into the next
        // encounter / field.
        self.battle.hit_fx.clear();
        self.battle.hit_events.clear();
        self.audio.battle_sfx_cues.clear();
        self.battle.clut_stages.clear();
        self.battle.effect_spawns.clear();
        self.audio.battle_shout_cues.clear();
        // Every battle effect dies with the battle: retail's mode switch back
        // to the field resets the whole actor pool (`FUN_8001E1B4`), and the
        // `efect.dat` walker is battle-overlay code. A wipe to the title holds
        // the frozen battle frame, so its effects come down with the hold
        // instead ([`Self::resolve_game_over_hold`]).
        if !wipe_to_title {
            self.teardown_battle_effects();
        }
        // Post-battle grace + suppression on the session.
        self.end_encounter_battle();
        // Persist the battle's party HP / MP into the roster records BEFORE the
        // field actor table is restored. The battle mutates the `BattleActor`
        // mirrors on `self.actors`, and the restore below overwrites the whole
        // table with the pre-battle clone - so without this every fight ended
        // with the party back at full health and a party wipe was unobservable.
        // `persist_battle_party_hp` is the party-band-scoped sibling of
        // `save_party` - scoped precisely because the actor slots past the
        // party band are monsters while a battle is up.
        self.persist_battle_party_hp();
        if wipe_to_title {
            // Defer the field restore: the scene stays in Battle mode with
            // the battle actor table live, so hosts hold the final battle
            // frame through the game-over hand-off (retail freezes the wipe
            // frame while mode 22 CARD INIT streams the menu overlay off the
            // disc). `resolve_game_over_hold` performs the restore when the
            // host's `GameOverSession` resolves.
            self.game_over_hold = true;
            return;
        }
        // Restore the field actor table captured at the transition, then push
        // the just-persisted HP / MP back onto the restored party actors so the
        // field-side mirrors agree with the records (the clone carries the
        // pre-battle values).
        if let Some(active) = self.battle.battle_party_restore.take() {
            self.party.active_party = active;
        }
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party.party_count = ret.party_count;
            self.resync_party_actors_from_roster();
        }
        // Return to the mode the battle was entered from (the field for a
        // field encounter, the overworld for a world-map encounter), then
        // reset the latch so a subsequent direct `enter_battle` defaults back
        // to the field.
        self.mode = self.battle.return_mode;
        self.battle.return_mode = SceneMode::Field;
        // Reset step tracking so the post-battle position doesn't count as a
        // step on the next field tick.
        self.terrain.last_tile = None;
        self.terrain.step_tile = None;
        if dome_leg {
            self.finish_muscle_battle_leg(dome_end);
        }
        // The field init the battle exit re-enters (`FUN_801D6704`) re-centres
        // the camera window on the saved player tile on the back-from-battle
        // word (`_DAT_8007B8B8 == 2`, `0x801D6E0C..0x801D6ECC`, `FUN_80017DD4`
        // at `pos >> 7`) after its unconditional camera reset `FUN_80025C24`
        // (`0x801D698C`). The window half is the world's; the camera half is
        // the host camera's, which takes the latch in
        // `frame_step::camera_after_world_tick`.
        if matches!(self.mode, SceneMode::Field | SceneMode::WorldMap) {
            self.recentre_field_window_on_player();
            self.battle.field_camera_reseat = true;
        }
    }

    /// Take the battle exit's camera re-seat latch
    /// ([`legaia_engine_battle_loop::battle_state::BattleState::field_camera_reseat`]): `true` once per
    /// return to the field or the overworld, on the tick
    /// [`Self::finish_battle`] restored it.
    pub fn take_field_camera_reseat(&mut self) -> bool {
        std::mem::take(&mut self.battle.field_camera_reseat)
    }

    /// Complete the field restore [`Self::finish_battle`]'s party-wipe arm
    /// deferred ([`Self::game_over_hold`]): restore the field actor snapshot,
    /// flip the scene mode back to the battle's entry mode, and reset step
    /// tracking. Hosts call this when their `GameOverSession` resolves
    /// (before handing the screen to the title session) so a subsequent
    /// Continue / New Game starts from a consistent field-shaped world.
    /// No-op when no hold is pending.
    pub fn resolve_game_over_hold(&mut self) {
        if !self.game_over_hold {
            return;
        }
        self.game_over_hold = false;
        if let Some(active) = self.battle.battle_party_restore.take() {
            self.party.active_party = active;
        }
        self.teardown_battle_effects();
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party.party_count = ret.party_count;
            self.resync_party_actors_from_roster();
        }
        self.mode = self.battle.return_mode;
        self.battle.return_mode = SceneMode::Field;
        self.terrain.last_tile = None;
        self.terrain.step_tile = None;
    }

    /// Abandon a battle that is still live when the world is sent somewhere
    /// else - a scene load, a New Game - without it having ended.
    ///
    /// Retail has no such path: the battle overlay leaves only through its
    /// exit (MAIN INIT's back-from-battle arm, [`Self::finish_battle`]). The
    /// port's hosts can (a scene picker, a debug seat, a test driving several
    /// fights on one runtime), and a battle left up carried its results
    /// sequencer, command sessions and field snapshot into whatever came
    /// next. This drops them with no outcome: no loot, no story-flag stores,
    /// no HP persisted - the fight did not happen. A pending game-over hold
    /// resolves first, which is the same field restore. No-op when no battle
    /// is live.
    pub fn abandon_live_battle(&mut self) {
        if self.game_over_hold {
            self.resolve_game_over_hold();
        }
        if !matches!(self.mode, SceneMode::Battle) && self.battle.victory.is_none() {
            return;
        }
        self.battle.victory = None;
        self.battle.end = None;
        self.battle.loot_applied = false;
        self.battle.spoils_frames = 0;
        self.battle.result_windows_glide = None;
        self.battle.inflight_seed = None;
        self.battle.active_formation = None;
        self.battle.command = None;
        self.battle.item_menu = None;
        self.battle.spell_menu = None;
        self.battle.arts_menu = None;
        self.battle.arts_input = None;
        self.battle.arts_list_window = None;
        self.battle.hit_fx.clear();
        self.battle.hit_events.clear();
        self.battle.clut_stages.clear();
        self.battle.effect_spawns.clear();
        self.audio.battle_sfx_cues.clear();
        self.audio.battle_shout_cues.clear();
        self.audio.battle_bgm_active = false;
        self.audio.field_bgm_resume = None;
        self.presentation.fade = None;
        self.presentation.module_fades.clear();
        self.battle.entry_fade = None;
        self.casting.module_pres = Default::default();
        let buffs = std::mem::take(&mut self.battle.buffs);
        for b in buffs {
            self.battle
                .add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        self.teardown_battle_effects();
        if let Some(active) = self.battle.battle_party_restore.take() {
            self.party.active_party = active;
        }
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party.party_count = ret.party_count;
            self.resync_party_actors_from_roster();
        }
        for a in self.actors.iter_mut() {
            a.battle.seat = None;
        }
        if self.battle.return_mode == SceneMode::MuscleDome {
            // An abandoned dome leg leaves the arena with its leg undecided;
            // the arena's own exit reports it.
            self.minigames.muscle_leg_in_battle = false;
            self.minigames.muscle_leg_ran = false;
            self.battle.special_word = 0;
            if let Some(id) = self.minigames.muscle_saved_map_id.take() {
                self.battle.map_id = id;
            }
        }
        self.mode = self.battle.return_mode;
        self.battle.return_mode = SceneMode::Field;
        self.terrain.last_tile = None;
        self.terrain.step_tile = None;
    }

    /// Active enemy actors in the current battle as `(actor_index,
    /// monster_id, battle_slot)`, where `battle_slot` is the 0-based monster
    /// index the battle texture loader keys VRAM placement on (feed it to
    /// `legaia_actor_formats::monster_archive::MonsterMesh::battle_render_mesh`).
    /// Empty unless the world is in [`SceneMode::Battle`].
    ///
    /// A renderer uses this to bridge each decoded monster mesh into its draw
    /// list: the engine itself never loads the archive, so the actor only
    /// carries the id - the host resolves it to a mesh.
    pub fn battle_monster_slots(&self) -> Vec<(usize, u16, u8)> {
        if !matches!(self.mode, SceneMode::Battle) {
            return Vec::new();
        }
        let first_monster = self.party.party_count as usize;
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(idx, a)| {
                let id = a.battle_monster_id?;
                let slot = idx.checked_sub(first_monster)? as u8;
                Some((idx, id, slot))
            })
            .collect()
    }
}

/// The world's system-flag bank as the back-from-battle arm sees it
/// (`DAT_80085758`, MSB-first).
struct WorldFlagBank<'a>(&'a mut World);

impl legaia_engine_battle::battle_return_flags::FlagBank for WorldFlagBank<'_> {
    fn test(&self, idx: u16) -> bool {
        self.0.system_flag_test(idx)
    }
    fn set(&mut self, idx: u16) {
        self.0.system_flag_set(idx);
    }
    fn clear(&mut self, idx: u16) {
        self.0.system_flag_clear(idx);
    }
}
