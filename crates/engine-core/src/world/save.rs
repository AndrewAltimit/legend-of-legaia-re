//! Party load / save and full LGSF save-file round-trip. Split out of
//! `world.rs` as an additional `impl World` block.

use super::*;

impl World {
    /// Load a `Party` (per-character roster) into the world's actor table.
    ///
    /// Per-character record 0 maps to actor slot 0, record 1 to slot 1, …
    /// up to `party.len()` (capped by `MAX_ACTORS`). For each loaded slot
    /// the world:
    ///
    /// - activates the actor,
    /// - copies HP / MP from the record's [`HpMpSp`] block into the
    ///   `BattleActor` mirrors,
    /// - stows the full record bytes via [`World::roster`] for later
    ///   round-trip via [`World::save_party`].
    ///
    /// The `legaia-save` crate's [`legaia_save::CharacterRecord::parse`] is
    /// the lossless deserializer; this method is the runtime-side glue that
    /// projects the persistent record into the per-VM actor state.
    ///
    /// [`HpMpSp`]: legaia_save::HpMpSp
    pub fn load_party(&mut self, party: legaia_save::Party) {
        let n = party.members.len().min(self.actors.len());
        for (slot, rec) in party.members.iter().take(n).enumerate() {
            let hms = rec.hp_mp_sp();
            let a = &mut self.actors[slot];
            a.active = true;
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            // Seed the per-slot turn-order SPD from the record's live stats so
            // a battle's next-actor selector can run the initiative scheme.
            // A zeroed record leaves SPD at 0 -> round-robin fallback.
            if let Some(s) = self.battle_speed.get_mut(slot) {
                *s = rec.live_stats().spd;
            }
        }
        self.party_count = n as u8;
        self.roster = party;
        // Hydrate the level-up tracker's per-slot cumulative XP and level
        // from the installed records. Without this the tracker keeps its
        // default 0-XP / level-1 state even when the record has the party
        // deep into the game, and the next grant would re-run the whole
        // curve from L1. Level prefers the engine cell (+0x100), falling
        // back to the retail displayed-level byte (+0x130) for records
        // lifted from retail saves.
        for (slot, rec) in self.roster.members.iter().enumerate() {
            if slot < self.level_up_tracker.level.len() {
                self.level_up_tracker.xp[slot] = rec.cumulative_xp();
                self.level_up_tracker.level[slot] = rec.level().max(rec.magic_rank()).max(1);
            }
        }
    }

    /// Capture the world's current actor state back into a `Party`. The
    /// roster bytes are returned verbatim except for the HP / MP / max-HP
    /// fields, which are resynced from the live `BattleActor` mirrors so
    /// in-battle damage / heals end up in the saved record.
    ///
    /// Round-trip: `world.load_party(p); world.save_party() == p` modulo
    /// the HP/MP resync (which is a no-op when no battle has run yet).
    pub fn save_party(&mut self) -> legaia_save::Party {
        // Actor slot -> roster record follows the present-party composition:
        // under an [`World::active_party`] mapping, actor ordinal `i` mirrors
        // the character at `active_party[i]`, and characters NOT in the
        // present party keep their record values untouched. The identity
        // default resyncs every record from its same-index actor, the
        // historical behaviour.
        let members = if self.active_party.is_empty() {
            self.roster.members.len().min(self.actors.len())
        } else {
            self.active_party.len().min(self.actors.len())
        };
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let a = &self.actors[member];
            if let Some(rec) = self.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = a.battle.hp;
                hms.hp_max = a.battle.max_hp;
                hms.mp_cur = a.battle.mp;
                rec.set_hp_mp_sp(hms);
            }
        }
        self.roster.clone()
    }

    /// Capture the complete engine state (party + globals) into a [`legaia_save::SaveFile`].
    ///
    /// Pairs with [`World::load_full`]. Use this instead of [`World::save_party`] when
    /// you need `story_flags`, `money`, and `inventory` to survive a save/load cycle.
    pub fn save_full(&mut self) -> legaia_save::SaveFile {
        let party = self.save_party();
        let mut inventory: Vec<(u8, u8)> = self
            .inventory
            .iter()
            .map(|(&id, &count)| (id, count))
            .collect();
        inventory.sort_by_key(|&(id, _)| id);

        // Build per-character extension records from live world state.
        // The present-party composition persists when installed; the
        // identity default serialises as the full roster order (the
        // historical encoding, which `load_full` treats as identity).
        let active_party: Vec<u8> = if self.active_party.is_empty() {
            (0..party.members.len() as u8).collect()
        } else {
            self.active_party.clone()
        };
        let mut per_char: Vec<(u8, legaia_save::CharSaveExt)> = Vec::new();
        for slot in 0..party.members.len() as u8 {
            let mut ce = legaia_save::CharSaveExt::default();
            // Learned arts: derive from TacticalArtsTracker - bit i is
            // set when art id i has crossed the learn threshold.
            for art_id in 0..32u8 {
                if self.tactical_arts.is_learned(slot, art_id) {
                    ce.learned_arts_mask |= 1u32 << art_id;
                }
            }
            // Spells: the per-character learned spell list from the seru log.
            ce.spells = self.seru_log.learned_spells(slot).to_vec();
            // Seru captures: export the live log's per-Seru capture-point
            // progress (real seru_id -> points) so sub-threshold progress
            // survives a save/load. Sorted for deterministic output.
            ce.seru_captures = self
                .seru_log
                .iter_rows()
                .filter(|(s, _, _)| *s == slot)
                .map(|(_, sid, row)| (sid, row.points))
                .collect();
            ce.seru_captures.sort_by_key(|&(sid, _)| sid);
            // Shiny spells: spell ids this character learned from a shiny
            // capture (+35% damage). Persisted in the LGSF v4 LGX4 block.
            ce.shiny_spells = self
                .seru_log
                .iter_shiny()
                .filter(|(s, _)| *s == slot)
                .map(|(_, spell_id)| spell_id)
                .collect();
            ce.shiny_spells.sort_unstable();
            // Active-chain selection still lives in the per-char ext mirror.
            if let Some((_, src)) = self.per_char_ext.iter().find(|(s, _)| *s == slot) {
                ce.active_chains = src.active_chains;
            }
            per_char.push((slot, ce));
        }

        legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt {
                story_flags: self.story_flags,
                story_flag_bits: self.story_flag_bits.clone(),
                money: self.money,
                inventory,
            },
            ext_v2: legaia_save::SaveExtV2 {
                play_time_seconds: self.play_time_seconds,
                active_party,
                per_char,
                saved_chains: self.saved_chains.clone(),
            },
        }
    }

    /// Restore engine state from a [`legaia_save::SaveFile`] produced by [`World::save_full`].
    ///
    /// Party records are applied through [`World::load_party`]; globals overwrite the
    /// current `story_flags`, `money`, and `inventory`. Sync per-slot
    /// [`LevelUpTracker::level`] from each loaded record's `+0x100` byte
    /// so reloads don't silently reset every party slot to level 1.
    pub fn load_full(&mut self, sf: legaia_save::SaveFile) {
        self.load_party(sf.party);
        // Restore the present-party composition. The full-roster identity
        // order (what `save_full` writes when no composition is installed)
        // stays the identity default rather than a 3-cap reorder, so legacy
        // saves keep their historical party_count.
        let identity: Vec<u8> = (0..self.roster.members.len() as u8).collect();
        if sf.ext_v2.active_party != identity {
            self.set_active_party(sf.ext_v2.active_party.clone());
        } else {
            self.active_party.clear();
        }
        self.story_flags = sf.ext.story_flags;
        self.story_flag_bits = sf.ext.story_flag_bits;
        self.money = sf.ext.money;
        self.inventory.clear();
        for (id, count) in sf.ext.inventory {
            if count > 0 {
                self.inventory.insert(id, count);
            }
        }
        // (The level-up tracker's per-slot XP + level are hydrated from the
        // records inside `load_party`.)
        // V2 ext block - repopulate engine-side trackers.
        self.play_time_seconds = sf.ext_v2.play_time_seconds;
        self.saved_chains = sf.ext_v2.saved_chains.clone();
        self.per_char_ext = sf.ext_v2.per_char.clone();
        // Reset trackers so reloads don't accumulate stale state.
        self.tactical_arts = TacticalArtsTracker::new();
        self.seru_log = crate::seru_learning::SeruCaptureLog::new();
        for (slot, ce) in &sf.ext_v2.per_char {
            // Re-mark learned arts so the tracker doesn't re-fire the
            // "first time learned" event for arts the save already has.
            for art_id in 0..32u8 {
                if ce.learned_arts_mask & (1u32 << art_id) != 0 {
                    self.tactical_arts.mark_known(*slot, art_id);
                }
            }
            // Restore per-Seru capture-point progress. When the registry is
            // installed, a row that's already over threshold restores as
            // learned (with its spell), so a later capture doesn't re-fire
            // the learn event.
            for &(sid, pts) in &ce.seru_captures {
                let def = self.seru_registry.get(sid);
                let learned = def.is_some_and(|d| pts >= d.learn_threshold);
                let spell_id = def.map(|d| d.spell_id);
                self.seru_log
                    .restore_row(*slot, sid, pts, 0, learned, spell_id);
            }
            // Ensure every persisted learned spell lands in the learned list,
            // even with no registry installed: map it back to its teaching
            // Seru when known, else key by the spell id as a surrogate.
            for &spell_id in &ce.spells {
                if let Some(def) = self.seru_registry.seru_for_spell(spell_id) {
                    self.seru_log.mark_learned(*slot, def.id, spell_id);
                } else {
                    self.seru_log.mark_learned(*slot, spell_id as u16, spell_id);
                }
            }
            // Restore the shiny set (+35% damage spells).
            for &spell_id in &ce.shiny_spells {
                self.seru_log.mark_shiny(*slot, spell_id);
            }
        }
    }
}
