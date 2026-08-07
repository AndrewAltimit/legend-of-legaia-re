//! Battle teardown: finish / loot / field restore, and the monster-slot render
//! bridge. Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

/// One line of the post-battle spoils panel's variable block, already
/// resolved against the world's item catalog / roster.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BattleSpoilsBanner {
    pub xp: u32,
    pub gold: u32,
    /// `"<name> drop"` lines - one per item the loot roll surfaced.
    pub drops: Vec<String>,
    /// `"<name>'s level increased!"` lines - one per character that crossed
    /// a threshold in this battle's XP grant. The wording is retail's own,
    /// off the `noa_levelup_banner` framebuffer; the new level is not on
    /// that line (the status screen carries it).
    pub level_ups: Vec<String>,
}

impl World {
    /// How long the post-battle spoils panel stays up, in sim ticks
    /// (~3 s at the 100 Hz sim clock).
    pub const SPOILS_BANNER_FRAMES: u16 = 300;

    /// The post-battle spoils panel a host should be drawing this frame, or
    /// `None` when the panel is not up.
    ///
    /// Resolves drop item ids through [`Self::item_catalog`] and level-up
    /// character slots through [`Self::roster`], so a host needs no table of
    /// its own. Falls back to `Item <id>` / `Member <n>` when a name is
    /// unavailable (a disc-free build's synthetic catalog).
    pub fn battle_spoils_banner(&self) -> Option<BattleSpoilsBanner> {
        if self.battle_spoils_frames == 0 {
            return None;
        }
        let r = self.last_battle_rewards.as_ref()?;
        let drops = r
            .drops
            .iter()
            .map(|&id| {
                self.item_catalog
                    .get(id)
                    .map(|it| it.name.to_string())
                    .unwrap_or_else(|| format!("Item {id}"))
            })
            .collect();
        let level_ups = r
            .level_ups
            .iter()
            .map(|lu| {
                let slot = lu.char_id as usize;
                let name = self
                    .roster
                    .members
                    .get(slot)
                    .map(|m| m.name())
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("Member {}", slot + 1));
                format!("{name}\'s level increased!")
            })
            .collect();
        Some(BattleSpoilsBanner {
            xp: r.xp,
            gold: r.gold,
            drops,
            level_ups,
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
    /// `0x8003B5A0`), the survived-last-battle bit the same block sets on
    /// the surviving exits. Non-wipe endings restore the field actor
    /// snapshot, drop the encounter session into its grace window, and flip
    /// the scene mode back to [`SceneMode::Field`], unchanged.
    // REF: FUN_8003AEB0 (the back-from-battle game-over gate this folds)
    pub(in crate::world) fn finish_battle(&mut self) {
        if self.game_over_hold {
            // The wipe hold parks the scene in Battle mode, so a host that
            // keeps ticking the world re-runs the action SM's wipe scan and
            // re-raises `battle_end` every tick. The fold already happened;
            // consume the repeat and keep the hold frozen.
            self.battle_end = None;
            return;
        }
        if self.battle_end == Some(BattleEndCause::MonsterWipe)
            && let Some(formation) = self.active_formation.clone()
        {
            // `apply_battle_loot` borrows the catalog while mutating self, so
            // swap it out and back around the call.
            let catalog = std::mem::take(&mut self.monster_catalog);
            let rewards = self.apply_battle_loot(&formation, &catalog);
            self.monster_catalog = catalog;
            self.last_battle_rewards = Some(rewards);
            // Arm the spoils panel. The numbers were always applied; nothing
            // ever told the player about them.
            self.battle_spoils_frames = Self::SPOILS_BANNER_FRAMES;
        }
        // `true` only on the wipe-to-title arm; a wipe under the
        // scripted-loss latch takes the ordinary field return below.
        let mut wipe_to_title = false;
        if self.battle_end == Some(BattleEndCause::PartyWipe) {
            // Clear the survived-last-battle flag (story-flag index 1) on
            // either wipe arm - retail `0x8003B5A0` `andi 0xbf`.
            self.system_flag_clear(1);
            if self.system_flag_test(0) {
                // Scripted loss: consume the latch and fall through to the
                // ordinary field return (retail skips the hand-off at
                // `0x8003B5BC` and clears bit 0x80 at `0x8003B608`).
                self.system_flag_clear(0);
            } else {
                self.game_over = true;
                wipe_to_title = true;
            }
        }
        self.active_formation = None;
        self.battle_end = None;
        // Drop the battle seat anchors - the next battle's setup re-seats
        // the actors and the first locomotion tick re-seeds the pair
        // (`World::tick_battle_locomotion`).
        for a in self.actors.iter_mut() {
            a.battle.seat = None;
        }
        self.battle_escaped = false;
        self.battle_no_escape = false;
        self.battle_guarding = [false; 3];
        if wipe_to_title {
            // Retail's wipe hand-off never resumes the field track: the arm
            // pauses the sequencer (`jal 0x800266E0(0x8007052C)` at
            // `0x8003B5EC`, the primitive BGM sub-op 2 wraps - the same call
            // the scripted `4C EA` trigger routes) and the CARD / title flow
            // owns audio from there. Drop the swap bookkeeping so nothing
            // later cross-fades back to the field track.
            self.battle_bgm_active = false;
            self.field_bgm_resume = None;
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
        let buffs = std::mem::take(&mut self.battle_buffs);
        for b in buffs {
            self.add_to_buff_scalar(b.slot, b.stat, -b.applied_delta);
        }
        // Revert any Fury Boost AP-gauge extension (class-5 item) and clear the
        // per-slot flags, so the next battle starts from the base gauge.
        for idx in 0..self.ap_gauges.len() {
            if let Some(delta) = self.fury_boost[idx].take() {
                let gauge = &mut self.ap_gauges[idx];
                gauge.base_ap = gauge.base_ap.saturating_sub(delta);
                gauge.current_ap = gauge.current_ap.min(gauge.ceiling());
            }
        }
        // Bank any captured Seru into learning progress (drains battle_captures).
        self.resolve_captures();
        // Drop any open command / item / spell session - they belong to the
        // finished battle.
        self.battle_command = None;
        self.battle_item_menu = None;
        self.battle_spell_menu = None;
        self.battle_arts_menu = None;
        self.battle_arts_input = None;
        // Stale damage popups + sound cues must not bleed into the next
        // encounter / field.
        self.battle_hit_fx.clear();
        self.battle_sfx_cues.clear();
        self.battle_effect_spawns.clear();
        self.battle_shout_cues.clear();
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
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party_count = ret.party_count;
            self.resync_party_actors_from_roster();
        }
        // Return to the mode the battle was entered from (the field for a
        // field encounter, the overworld for a world-map encounter), then
        // reset the latch so a subsequent direct `enter_battle` defaults back
        // to the field.
        self.mode = self.battle_return_mode;
        self.battle_return_mode = SceneMode::Field;
        // Reset step tracking so the post-battle position doesn't count as a
        // step on the next field tick.
        self.field_last_tile = None;
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
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party_count = ret.party_count;
            self.resync_party_actors_from_roster();
        }
        self.mode = self.battle_return_mode;
        self.battle_return_mode = SceneMode::Field;
        self.field_last_tile = None;
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
        let first_monster = self.party_count as usize;
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
