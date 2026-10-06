//! Battle teardown: finish / loot / field restore, and the monster-slot render
//! bridge. Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

/// One line of the post-battle spoils panel's variable block, already
/// resolved against the world's item catalog / roster.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BattleSpoilsBanner {
    /// The per-member EXP share ([`crate::world::BattleRewards::xp_share`]) -
    /// what retail's result window prints, not the pool.
    pub xp: u32,
    pub gold: u32,
    /// The report window's drop line, one per item the loot roll surfaced
    /// (retail's roll surfaces at most one): the executable's template with
    /// the item name spliced in ([`World::drop_line`]), or `Got <name>` on a
    /// host without it.
    pub drops: Vec<String>,
    /// The level-up window's line - empty when nobody levelled. Retail
    /// opens one window, element `0x44 + mask` (bit `k` = character `k`),
    /// whose string names every character that crossed a threshold in one
    /// line, or nobody when all three did; the port reads the seven strings
    /// off the user's executable ([`Self::level_up_line`]). Without them (a
    /// disc-free host) it falls back to one `"<name>'s level increased!"`
    /// line per character. The new level is not on the line (the status
    /// screen carries it).
    pub level_ups: Vec<String>,
    /// Who the victory line names: the lead alone when the second party seat
    /// is empty, else the lead's team (`FUN_801D84C0`'s two build arms).
    pub subject: vm::battle_party_panel::ResultSubject,
    /// `(level_up, report)` rows below rest - the windows' raise glide
    /// ([`crate::battle_hud::battle_result_windows_dy`]).
    pub slide: (i32, i32),
}

/// The loss window's content ([`World::battle_defeat_banner`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BattleDefeatBanner {
    /// The one line the window shows, or `None` without the disc pool.
    pub line: Option<String>,
    /// Rows below rest - the window's raise glide, the report window's.
    pub slide_y: i32,
}

impl World {
    /// How long the post-battle spoils panel stays up, in sim ticks
    /// (~3 s at the 100 Hz sim clock).
    pub const SPOILS_BANNER_FRAMES: u16 = 300;

    /// The party leader whose name opens the post-battle spoils line: the
    /// member in party slot 0, or `Vahn` when that record carries no name
    /// (a disc-free build's blank roster). Both hosts used to resolve it
    /// with a copy of this lookup each.
    pub fn battle_spoils_leader(&self) -> String {
        self.party
            .roster
            .members
            .get(self.party_roster_slot(0))
            .map(|m| m.name())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Vahn".to_string())
    }

    /// The post-battle spoils panel a host should be drawing this frame, or
    /// `None` when the panel is not up.
    ///
    /// Resolves drop item ids through [`crate::world::DiscTables::item_catalog`] and level-up
    /// character slots through [`crate::world::PartyState::roster`], so a host needs no table of
    /// its own. Falls back to `Item <id>` / `Member <n>` when a name is
    /// unavailable (a disc-free build's synthetic catalog).
    pub fn battle_spoils_banner(&self) -> Option<BattleSpoilsBanner> {
        // Up for the results frame through the exit while the end-of-battle
        // sequence runs (retail's windows come down with the battle), and for
        // the aging window a direct `finish_battle` still arms.
        if self.battle.spoils_frames == 0 && !self.battle_result_screen_active() {
            return None;
        }
        // A special battle opens no result window (`FUN_8004E568` skips
        // `FUN_801D8DE8(0x41)` at `0x8004F614` while `_DAT_8007BAC0 != 0`),
        // and a wipe opens the loss window instead
        // ([`Self::battle_defeat_banner`]) - `last_rewards` is never cleared,
        // so without the cause test a wipe after a win re-showed that win's
        // spoils over the loss.
        if self
            .battle
            .victory
            .is_some_and(|v| !v.window_opened || v.cause != BattleEndCause::MonsterWipe)
        {
            return None;
        }
        let r = self.battle.last_rewards.as_ref()?;
        let drops = r
            .drops
            .iter()
            .map(|&id| {
                let name = self
                    .tables
                    .item_catalog
                    .get(id)
                    .map(|it| it.name.to_string())
                    .unwrap_or_else(|| format!("Item {id}"));
                self.drop_line(&name)
                    .unwrap_or_else(|| format!("Got {name}"))
            })
            .collect();
        let mask = r
            .level_ups
            .iter()
            .filter(|lu| lu.char_id < 3)
            .fold(0u8, |m, lu| m | (1 << lu.char_id));
        let level_ups = if let Some(line) = self.level_up_line(mask) {
            vec![line]
        } else {
            r.level_ups
                .iter()
                .map(|lu| {
                    let slot = lu.char_id as usize;
                    let name = self
                        .party
                        .roster
                        .members
                        .get(slot)
                        .map(|m| m.name())
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| format!("Member {}", slot + 1));
                    format!("{name}\'s level increased!")
                })
                .collect()
        };
        Some(BattleSpoilsBanner {
            xp: r.xp_share,
            gold: r.gold,
            drops,
            level_ups,
            subject: self.battle_result_subject(),
            slide: crate::battle_hud::battle_result_windows_dy(self),
        })
    }

    /// The report window's drop line for an item named `item`: the
    /// executable's template (`legaia_asset::screen_elements::DROP_LINE_PTR_VA`)
    /// with its `0xC2` item escape spliced, less the leading `0x7C` break
    /// that puts it on the window's third row (the port keeps rows as
    /// lines). `None` without the template.
    ///
    /// REF: FUN_8004E568 (`0x8004F5C4..0x8004F600`)
    pub fn drop_line(&self, item: &str) -> Option<String> {
        let raw = self.menu.text.as_ref()?.drop_line.as_ref()?;
        let mut out = String::new();
        let mut i = 0;
        while i < raw.len() {
            let b = raw[i];
            if (0xC0..=0xCF).contains(&b) {
                if matches!(b, 0xC2 | 0xC4) {
                    out.push_str(item);
                }
                i += 2;
                continue;
            }
            if (0x20..0x7F).contains(&b) && b != 0x7C {
                out.push(b as char);
            }
            i += 1;
        }
        Some(out)
    }

    /// The level-up window's line for `mask` (bit `k` = character `k`): the
    /// string record `0x44 + mask` points at, its `0xC1 k` name escapes
    /// spliced with record `k`'s name (`0x63` = the party leader). `None`
    /// for an empty mask or without the strings.
    ///
    /// REF: FUN_8004E568 (`0x8004F6F8..0x8004F728`, the mask and the raise)
    pub fn level_up_line(&self, mask: u8) -> Option<String> {
        let raw = self
            .menu
            .text
            .as_ref()?
            .level_up_lines
            .as_ref()?
            .get(usize::from(mask).checked_sub(1)?)?;
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
        Some(out)
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
            let (status, hp) = vm::battle_formulas::battle_exit_party_reset(word, 1, hp0);
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

    /// Who the battle-result messages name: participant ids in panel order,
    /// `0` for an empty seat - the shape of retail's `0x8007BD10` list the
    /// two build arms of `FUN_801D84C0` key on.
    fn battle_result_subject(&self) -> vm::battle_party_panel::ResultSubject {
        let seats: [u8; 3] = std::array::from_fn(|i| {
            if i < usize::from(self.party.party_count) {
                (self.party_roster_slot(i) as u8).wrapping_add(1)
            } else {
                0
            }
        });
        vm::battle_party_panel::result_subject(seats)
    }

    /// The loss window a host should be drawing this frame, or `None`.
    ///
    /// Retail's wipe arm of the results frame opens screen element `0x42`
    /// (`FUN_801D8DE8(0x42, 0)` at `0x8004F900`) unless the special-battle
    /// word is set (`0x8004F8F0`) - the same gate as the win arm's result
    /// window, carried by `VictorySequence::window_opened`. The element's
    /// placement record is the win window's twin (same frame, same seat), and
    /// its string is the defeat buffer `FUN_801D84C0` built at battle start:
    /// the lead's name plus the solo suffix, or the lead's team line.
    ///
    /// Up from the results frame through the exit, like the win window. The
    /// line is `None` on a disc-free host (no PROT 0898 pool), where the
    /// window opens empty.
    pub fn battle_defeat_banner(&self) -> Option<BattleDefeatBanner> {
        let v = self.battle.victory?;
        if v.cause != BattleEndCause::PartyWipe || !v.window_opened || !v.results_shown() {
            return None;
        }
        let subject = self.battle_result_subject();
        let lead = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(0))
            .map(|m| m.name())
            .filter(|n| !n.trim().is_empty());
        let line = self
            .tables
            .defeat_text
            .as_ref()
            .zip(lead)
            .map(|(t, lead)| t.compose(subject, &lead));
        Some(BattleDefeatBanner {
            line,
            slide_y: crate::battle_hud::battle_result_windows_dy(self).1,
        })
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
    /// ([`crate::battle_return_flags`]). Non-wipe endings restore the field
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
        // MAIN INIT's back-from-battle flag stores, run for every ending: the
        // party-survived bit `DAT_8007BD60 & 0x80` is clear only after a
        // party wipe, so every other end - a monster wipe, an escape, a
        // scripted stage exit - raises story flag 1, the outcome a scene
        // script tests after the fight; a wipe clears it. Flag 0 (the
        // scripted-loss latch) is consumed either way, and a wipe without it
        // is the game over. `wipe_to_title` is `true` only on that arm; a
        // wipe under the latch takes the ordinary field return below.
        let survived = self.battle.end != Some(BattleEndCause::PartyWipe);
        let ret = crate::battle_return_flags::apply_battle_return_flags(
            &mut WorldFlagBank(self),
            survived,
        );
        let wipe_to_title = ret.game_over;
        if wipe_to_title {
            self.game_over = true;
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
        for slot in self.party.party_count..vm::battle_action::ACTOR_SLOTS as u8 {
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
                .push(crate::field_events::FieldEvent::Bgm {
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
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        // Revert any Fury Boost AP-gauge extension (class-5 item) and clear the
        // per-slot flags, so the next battle starts from the base gauge.
        for idx in 0..self.battle.ap_gauges.len() {
            if let Some(delta) = self.battle.fury_boost[idx].take() {
                let gauge = &mut self.battle.ap_gauges[idx];
                gauge.base_ap = gauge.base_ap.saturating_sub(delta);
                gauge.current_ap = gauge.current_ap.min(gauge.ceiling());
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
        if let Some(active) = self.battle.solo_spar_restore.take() {
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
        if let Some(active) = self.battle.solo_spar_restore.take() {
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

    /// Active enemy actors in the current battle as `(actor_index,
    /// monster_id, battle_slot)`, where `battle_slot` is the 0-based monster
    /// index the battle texture loader keys VRAM placement on (feed it to
    /// `legaia_asset::monster_archive::MonsterMesh::battle_render_mesh`).
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

impl crate::battle_return_flags::FlagBank for WorldFlagBank<'_> {
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

#[cfg(test)]
mod level_up_line_tests {
    use super::*;

    /// The window's line is the mask's string with its `0xC1` name escapes
    /// spliced: record `k` for operand `k`, the leader for `0x63`. Synthetic
    /// strings stand in for the executable's.
    #[test]
    fn the_level_up_line_splices_the_mask_string() {
        let mut w = World::default();
        w.party.roster = legaia_save::Party::zeroed(3);
        for (k, name) in ["Ana", "Bo", "Cy"].iter().enumerate() {
            w.party.roster.members[k].set_name(name);
        }
        let mut lines = vec![b"x".to_vec(); 7];
        lines[0] = [&[0xC1, 0x00][..], b" up"].concat();
        lines[2] = [&[0xC1, 0x00][..], b" & ", &[0xC1, 0x01], b" up"].concat();
        lines[6] = b"all up".to_vec();
        w.menu.text = Some(crate::pause_screens::MenuTextTables {
            level_up_lines: Some(lines),
            ..Default::default()
        });
        assert_eq!(w.level_up_line(1).as_deref(), Some("Ana up"));
        assert_eq!(w.level_up_line(3).as_deref(), Some("Ana & Bo up"));
        assert_eq!(w.level_up_line(7).as_deref(), Some("all up"));
        assert_eq!(w.level_up_line(0), None, "nobody levelled");
        w.menu.text = None;
        assert_eq!(
            w.level_up_line(1),
            None,
            "no strings: the caller falls back"
        );
    }

    /// The drop line is the template with its item escape spliced and the
    /// leading row break dropped.
    #[test]
    fn the_drop_line_splices_the_item() {
        let mut w = World::default();
        assert_eq!(w.drop_line("Leaf"), None);
        w.menu.text = Some(crate::pause_screens::MenuTextTables {
            drop_line: Some([&b"|Got the "[..], &[0xC2, 0x01], b"."].concat()),
            ..Default::default()
        });
        assert_eq!(w.drop_line("Leaf").as_deref(), Some("Got the Leaf."));
    }
}
