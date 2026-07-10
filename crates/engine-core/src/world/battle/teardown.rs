//! Battle teardown: finish / loot / field restore, and the monster-slot render
//! bridge. Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Resolve a finished battle and return to the field.
    ///
    /// On [`BattleEndCause::MonsterWipe`] applies loot (XP / gold / drops /
    /// level-ups) via [`Self::apply_battle_loot`] against the captured
    /// formation; on [`BattleEndCause::PartyWipe`] raises [`Self::game_over`]
    /// (v0.1 has no defeat screen). Either way the field actor snapshot is
    /// restored, the encounter session drops into its grace window, and the
    /// scene mode flips back to [`SceneMode::Field`].
    pub(in crate::world) fn finish_battle(&mut self) {
        if self.battle_end == Some(BattleEndCause::MonsterWipe)
            && let Some(formation) = self.active_formation.clone()
        {
            // `apply_battle_loot` borrows the catalog while mutating self, so
            // swap it out and back around the call.
            let catalog = std::mem::take(&mut self.monster_catalog);
            let rewards = self.apply_battle_loot(&formation, &catalog);
            self.monster_catalog = catalog;
            self.last_battle_rewards = Some(rewards);
        }
        if self.battle_end == Some(BattleEndCause::PartyWipe) {
            self.game_over = true;
        }
        // Staged-boss bookkeeping: a fled staged fight reverts its transient
        // staged marker (retail boss fights are flee-blocked; without this the
        // entry-script re-run would spawn the post-victory record on an
        // unearned return). A win leaves the marker set - the post-victory
        // record's own `62 xx` script bytes clear it.
        if let Some(marker) = self.active_boss_staged_marker.take()
            && self.battle_escaped
        {
            self.system_flag_clear(marker);
        }
        self.active_formation = None;
        self.battle_end = None;
        self.battle_escaped = false;
        self.battle_guarding = [false; 3];
        // Restore the field track stashed at encounter start (cross-fades
        // back from the battle music). No-op if no swap was active.
        self.restore_field_bgm();
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
        // Stale damage popups + sound cues must not bleed into the next
        // encounter / field.
        self.battle_hit_fx.clear();
        self.battle_sfx_cues.clear();
        // Post-battle grace + suppression on the session.
        self.end_encounter_battle();
        // Restore the field actor table captured at the transition.
        if let Some(ret) = self.field_return.take() {
            self.actors = ret.actors;
            self.player_actor_slot = ret.player_actor_slot;
            self.party_count = ret.party_count;
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
