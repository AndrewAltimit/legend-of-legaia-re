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
        // Adopt each record's stored display name (`+0x2A7`) so a loaded save's
        // custom names reach the dialog renderer's `0xC1 XX` substitutions.
        //
        // Non-shrinking and skip-empty on purpose: a cold-boot save has a
        // one-member roster, and truncating here would drop the Noa / Gala /
        // Terra defaults `seed_starting_party` installs for the slots that have
        // not joined yet.
        for (slot, rec) in self.roster.members.iter().enumerate() {
            let name = rec.name();
            if name.is_empty() {
                continue;
            }
            if self.party_names.len() <= slot {
                self.party_names.resize(slot + 1, String::new());
            }
            self.party_names[slot] = name;
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

    /// Write the **battle party's** live HP / MP into their roster records.
    ///
    /// The narrow sibling of [`Self::save_party`], for
    /// [`Self::finish_battle`]. Two differences, both deliberate:
    ///
    /// - It stops at [`Self::party_count`]. In battle the actor slots past
    ///   the party band hold *monsters*, and `save_party`'s identity default
    ///   walks the whole roster - so running it verbatim at battle end would
    ///   copy a monster's HP into the fourth character's record.
    /// - It writes `hp_cur` / `mp_cur` only, never `hp_max`. Max HP does not
    ///   move during a fight, and the level-up applier has already written
    ///   the post-victory maxima into the records by the time this runs.
    pub(in crate::world) fn persist_battle_party_hp(&mut self) {
        let n = (self.party_count as usize).min(self.actors.len());
        for member in 0..n {
            let rslot = self.party_roster_slot(member);
            let (hp, mp) = {
                let a = &self.actors[member].battle;
                (a.hp, a.mp)
            };
            if let Some(rec) = self.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = hp;
                hms.mp_cur = mp;
                rec.set_hp_mp_sp(hms);
            }
        }
    }

    /// Project each present-party record's HP / MP back onto its party actor's
    /// [`BattleActor`] mirrors - the inverse of the [`Self::save_party`]
    /// resync, over the same `party_roster_slot` mapping.
    ///
    /// Used by [`Self::finish_battle`] after the field actor table is restored
    /// from the pre-battle snapshot: the snapshot's mirrors are stale, and the
    /// records (just written by [`Self::persist_battle_party_hp`]) hold the
    /// post-battle truth.
    ///
    /// Bounded by [`Self::party_count`]: in the restored *field* table the
    /// slots past the party band are NPCs, and pushing a character record's
    /// HP onto an NPC's mirrors is never right.
    pub fn resync_party_actors_from_roster(&mut self) {
        let members = (self.party_count as usize).min(self.actors.len()).min(
            if self.active_party.is_empty() {
                self.roster.members.len()
            } else {
                self.active_party.len()
            },
        );
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let Some(hms) = self.roster.members.get(rslot).map(|r| r.hp_mp_sp()) else {
                continue;
            };
            let a = &mut self.actors[member];
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
        }
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

        // The system-flag bank (retail `DAT_80085758`, the partition-2 gate
        // bitmap the field VM's 0x50/0x60/0x70 ops write) overlaps the saved
        // story-flag window at byte offset `0x158` (`0x80085758 - 0x80085600`).
        // Mirror the live bank into that window so gate/progression state
        // survives a save (the LGX3 block stores a u16-length bitmap, so a
        // bank longer than the retail 512-byte window still fits).
        let mut story_flag_bits = self.story_flag_bits.clone();
        if !self.system_flags.is_empty() {
            let need = 0x158 + self.system_flags.len();
            if story_flag_bits.len() < need {
                story_flag_bits.resize(need, 0);
            }
            for (k, b) in self.system_flags.iter().enumerate() {
                story_flag_bits[0x158 + k] |= b;
            }
        }
        legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt {
                story_flags: self.story_flags,
                story_flag_bits,
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
        // Seed the live system-flag bank from the saved bitmap's `+0x158`
        // window (the retail overlap `save_full` mirrors into) so partition-2
        // record gates - story-progression one-shots, door cutscene beats -
        // resolve the same after a reload. OR-merge: a retail SC import that
        // populated `story_flag_bits` alone seeds the bank the same way.
        self.system_flags.clear();
        if self.story_flag_bits.len() > 0x158 {
            let window = self.story_flag_bits[0x158..].to_vec();
            self.system_flags = window;
        }
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

/// Per-scene save permission and the pause menu's entry-context kind - the two
/// gate inputs the retail root command picker reads before it lets a row
/// through.
impl World {
    /// Seed [`World::scene_save_allowed`] from the scene MAN just loaded.
    ///
    /// Retail's MAN loader does this inline, one instruction after it takes
    /// the header's status word: it reads byte `+1` of the resident MAN
    /// buffer `_DAT_8007B898`, masks bit `0`, and stores the result **byte
    /// wide** into the per-scene save-allow flag `_DAT_8007B6A8`.
    ///
    /// ```text
    /// 8003af48  lbu   v0,0x1(v1)        ; v1 = _DAT_8007B898 (the MAN)
    /// 8003af4c  lbu   s7,0x0(v1)
    /// 8003af50  andi  v0,v0,0x1
    /// 8003af54  sb    v0,-0x4958(a0)    ; a0 = 0x80080000 -> 0x8007B6A8
    /// ```
    ///
    /// `None` (the scene carries no MAN, or its MAN did not parse) clears the
    /// flag, which is the state retail's own init leaves the byte in
    /// (`FUN_80025980` zeroes it) - no MAN, no permission.
    ///
    /// On the retail disc the bit is set on exactly the three kingdom
    /// world-map scenes and clear on every field scene, which is why saving
    /// outside a scripted save point is a world-map-only affordance.
    ///
    /// PORT: FUN_8003aeb0 (`0x8003AF48..0x8003AF54`)
    pub fn install_scene_save_permission(
        &mut self,
        man: Option<&legaia_asset::man_section::ManFile>,
    ) {
        self.scene_save_allowed = man.is_some_and(|m| m.header.low_flag);
    }

    /// Kind byte of the op-`0x49` entry context the pause menu tests - the
    /// engine's read of retail `*_DAT_8007B450`.
    ///
    /// Retail parks the field VM on op `0x49` by storing the **operand
    /// pointer** into `_DAT_8007B450` (`sw s6,-0x4bb0(s0)` in the op's Idle
    /// arm), and that operand opens on its sub-op byte (`lbu v0,0x0(s6)` two
    /// instructions earlier). So the "kind byte" every consumer dereferences
    /// is just the armed sub-op: `1` is a field save point (which enters the
    /// card driver directly), `0x0D` is the context that blocks the menu's
    /// Load row and turns its cancel into a Yes/No confirm.
    ///
    /// The port has no single global to read - it tags each park with the
    /// context that armed it ([`crate::field_submode_screen::Op49ParkOwner`])
    /// and resolves three sub-ops through dedicated host paths - so the kind
    /// is recovered from those paths instead: an armed inline shop is sub-op
    /// `0` and an installed tile board is sub-op `5`. Both are `!= 0x0D`, so
    /// they take the same allow branch retail takes, and a park whose sub-op
    /// the engine does not track reads `None` (also the allow branch). The
    /// blocking kind is therefore reachable only once the op-`0x49` arm
    /// records its sub-op, which is the one edit that would make this
    /// function able to return `0x0D`.
    ///
    /// REF: FUN_801de840 (op `0x49` Idle arm, `_DAT_8007B450 = operand`)
    pub fn menu_entry_context_kind(&self) -> Option<u8> {
        if self.field_shop_armed {
            return Some(0);
        }
        if self.tile_board_armed {
            return Some(5);
        }
        None
    }
}
