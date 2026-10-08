//! Party load / save and full LGSF save-file round-trip. Split out of
//! `world.rs` as an additional `impl World` block.

use super::*;

/// Byte offset of the system-flag bank (`0x80085758`) inside the saved
/// story-flag window (`0x80085600`).
const SYSTEM_FLAG_WINDOW: usize = 0x158;

/// Scratchpad `_DAT_1F800394` bit 10, the system lock: while set the player
/// tick skips the pad controller outright (`0x801D16A8..0x801D16B8`).
pub const FIELD_SYSTEM_LOCK_BIT: u32 = 0x400;
/// Scratchpad `_DAT_1F800394` bit 27, the menu lock: a menu-button press
/// buzzes instead of opening (`0x801D02C0..0x801D02E4`).
pub const FIELD_MENU_LOCK_BIT: u32 = 0x0800_0000;
/// The locked press's deny buzz (`li a0,0x23` at `0x801D02DC`).
pub const FIELD_MENU_DENY_CUE: i16 = 0x23;

impl World {
    /// Whether actor slots `0..party_count` hold the party's own combatants,
    /// so their HP / MP mirrors are live state a save must fold back into
    /// the records: in a battle (the seats `enter_battle` builds), and on the
    /// title screen, where no scene owns the table and a synthetic host
    /// drives the mirrors directly. Every other mode runs over a scene's
    /// actor table (field, overworld, the pause menu and minigames that
    /// suspend one), whose slots past the walking player are not the party.
    pub(crate) fn party_actors_are_party(&self) -> bool {
        matches!(self.mode, SceneMode::Battle | SceneMode::Title)
    }

    /// Whether projecting a party record onto actor `slot`
    /// ([`Self::load_party`], [`Self::set_active_party`]) may raise the slot.
    ///
    /// On a field or the overworld the actor table is not the party: the
    /// walking player's slot (slot 0 until a host installs another) is the
    /// party's, and every other slot belongs to the scene - spawned records,
    /// clones, effects, and the idle slots `init_scene_animations` leaves
    /// pre-bound to scene-pack meshes. Raising one of those spawned a phantom
    /// actor: drawn at the origin with whatever mesh the slot was pre-bound to
    /// (uru's sky and cliff pack across the frame after a card load of a
    /// four-member save), and held out of the spawn allocator's free list.
    /// Retail's card load copies the records into `0x80084708` and touches no
    /// actor; a battle seats its party itself ([`Self::enter_battle`]). The
    /// HP / MP mirrors are still written - only the raise is withheld.
    pub(crate) fn party_mirror_activates(&self, slot: usize) -> bool {
        if !matches!(self.mode, SceneMode::Field | SceneMode::WorldMap) {
            return true;
        }
        usize::from(self.player_actor_slot.unwrap_or(0)) == slot
    }

    /// Load a `Party` (per-character roster) into the world's actor table.
    ///
    /// Per-character record 0 maps to actor slot 0, record 1 to slot 1, …
    /// up to `party.len()` (capped by `MAX_ACTORS`). For each loaded slot
    /// the world:
    ///
    /// - activates the actor - except on a field or the overworld, where
    ///   only the walking player's slot is the party's
    ///   ([`Self::party_mirror_activates`]),
    /// - copies HP / MP from the record's [`HpMpSp`] block into the
    ///   `BattleActor` mirrors,
    /// - stows the full record bytes via [`crate::world::PartyState::roster`] for later
    ///   round-trip via [`World::save_party`].
    ///
    /// The `legaia-save` crate's [`legaia_save::CharacterRecord::parse`] is
    /// the lossless deserializer; this method is the runtime-side glue that
    /// projects the persistent record into the per-VM actor state.
    ///
    /// [`HpMpSp`]: legaia_save::HpMpSp
    pub fn load_party(&mut self, party: legaia_save::Party) {
        // A loaded roster starts from no party-op history.
        self.party.field_list_emptied = false;
        let n = party.members.len().min(self.actors.len());
        for (slot, rec) in party.members.iter().take(n).enumerate() {
            let hms = rec.hp_mp_sp();
            let activate = self.party_mirror_activates(slot);
            let a = &mut self.actors[slot];
            if activate {
                a.active = true;
            }
            a.battle.hp = hms.hp_cur;
            a.battle.max_hp = hms.hp_max;
            a.battle.mp = hms.mp_cur;
            a.battle.liveness = if hms.hp_cur > 0 { 1 } else { 0 };
            // Seed the per-slot turn-order SPD from the record's live stats so
            // a battle's next-actor selector can run the initiative scheme.
            // A zeroed record leaves SPD at 0 -> round-robin fallback.
            if let Some(s) = self.battle.speed.get_mut(slot) {
                *s = rec.live_stats().spd;
            }
        }
        self.party.party_count = n as u8;
        self.party.roster = party;
        // Hydrate the level-up tracker's per-slot cumulative XP and level
        // from the installed records. Without this the tracker keeps its
        // default 0-XP / level-1 state even when the record has the party
        // deep into the game, and the next grant would re-run the whole
        // curve from L1. Level is the record's `+0x130` byte - the same cell
        // for engine LGSF saves and records lifted from retail cards.
        for (slot, rec) in self.party.roster.members.iter().enumerate() {
            if slot < self.party.level_up_tracker.level.len() {
                self.party.level_up_tracker.xp[slot] = rec.cumulative_xp();
                self.party.level_up_tracker.level[slot] = rec.level().max(1);
            }
        }
        // Adopt each record's stored display name (`+0x2A7`) so a loaded save's
        // custom names reach the dialog renderer's `0xC1 XX` substitutions.
        //
        // Non-shrinking and skip-empty on purpose: a cold-boot save has a
        // one-member roster, and truncating here would drop the Noa / Gala /
        // Terra defaults `seed_starting_party` installs for the slots that have
        // not joined yet.
        for (slot, rec) in self.party.roster.members.iter().enumerate() {
            let name = rec.name();
            if name.is_empty() {
                continue;
            }
            if self.party.party_names.len() <= slot {
                self.party.party_names.resize(slot + 1, String::new());
            }
            self.party.party_names[slot] = name;
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
        // Outside a fight the records are the pools: retail keeps a member's
        // HP / MP only at `0x80084708 + n*0x414`, and every field heal, the
        // inn's `4C 82` restore and the card load write the record first. The
        // actor table there is the scene's - slot 0 walks, every other slot
        // is a P1 placement, clone or effect a scene script may re-stamp from
        // a zeroed node - so a resync read an NPC's empty mirror into the
        // second and third records. `opurud` does exactly that, and the next
        // load took the zeroed record for an unjoined member and re-seeded
        // it from the New Game template.
        if !self.party_actors_are_party() {
            return self.party.roster.clone();
        }
        // Actor slot -> roster record follows the present-party composition:
        // under an [`crate::world::PartyState::active_party`] mapping, actor ordinal `i` mirrors
        // the character at `active_party[i]`, and characters NOT in the
        // present party keep their record values untouched. The identity
        // default resyncs every record from its same-index actor, the
        // historical behaviour.
        let members = if self.party.active_party.is_empty() {
            self.party.roster.members.len().min(self.actors.len())
        } else {
            self.party.active_party.len().min(self.actors.len())
        };
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let a = &self.actors[member];
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = a.battle.hp;
                hms.hp_max = a.battle.max_hp;
                hms.mp_cur = a.battle.mp;
                rec.set_hp_mp_sp(hms);
            }
        }
        self.party.roster.clone()
    }

    /// Write the **battle party's** live HP / MP / AP into their roster records.
    ///
    /// The narrow sibling of [`Self::save_party`], for
    /// [`Self::finish_battle`]. Two differences, both deliberate:
    ///
    /// - It stops at [`crate::world::PartyState::party_count`]. In battle the actor slots past
    ///   the party band hold *monsters*, and `save_party`'s identity default
    ///   walks the whole roster - so running it verbatim at battle end would
    ///   copy a monster's HP into the fourth character's record.
    /// - It writes `hp_cur` / `mp_cur` / `sp_cur` only, never `hp_max`. Max HP does not
    ///   move during a fight, and the level-up applier has already written
    ///   the post-victory maxima into the records by the time this runs.
    pub(in crate::world) fn persist_battle_party_hp(&mut self) {
        let n = (self.party.party_count as usize).min(self.actors.len());
        for member in 0..n {
            let rslot = self.party_roster_slot(member);
            let (hp, mp, ap) = {
                let a = &self.actors[member].battle;
                (a.hp, a.mp, a.spirit_gauge)
            };
            if let Some(rec) = self.party.roster.members.get_mut(rslot) {
                let mut hms = rec.hp_mp_sp();
                hms.hp_cur = hp;
                hms.mp_cur = mp;
                // The Spirit (AP) gauge goes back to the record's `+0x10E`
                // with MP: both results arms of `FUN_8004E568` store
                // `+0x150 -> +0x10A` and `+0x170 -> +0x10E` per member
                // (`0x8004F1E8..0x8004F220`, `0x8004FBE8..0x8004FC20`).
                hms.sp_cur = ap;
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
    /// Bounded by [`crate::world::PartyState::party_count`]: in the restored *field* table the
    /// slots past the party band are NPCs, and pushing a character record's
    /// HP onto an NPC's mirrors is never right.
    pub fn resync_party_actors_from_roster(&mut self) {
        let members = (self.party.party_count as usize)
            .min(self.actors.len())
            .min(if self.party.active_party.is_empty() {
                self.party.roster.members.len()
            } else {
                self.party.active_party.len()
            });
        for member in 0..members {
            let rslot = self.party_roster_slot(member);
            let Some(hms) = self.party.roster.members.get(rslot).map(|r| r.hp_mp_sp()) else {
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
        // Both views of the bag. `item_slots` is retail's physical array -
        // slot order, holes intact - and is what a reload restores from;
        // `inventory` is the id-sorted compact list the v1 prelude carries,
        // so a file written here still reads on a consumer that only knows
        // the list.
        let item_slots: Vec<(u8, u8)> = self.party.inventory.slots().to_vec();
        let mut inventory: Vec<(u8, u8)> = self
            .party
            .inventory
            .iter()
            .map(|(&id, &count)| (id, count))
            .collect();
        inventory.sort_by_key(|&(id, _)| id);

        // Build per-character extension records from live world state.
        // The present-party composition persists when installed; the
        // identity default serialises as the roster order up to the live
        // party count - the full roster order when every record is in the
        // party (the historical encoding, which `load_full` treats as
        // identity), a prefix when fewer are. Retail keeps the same pair as
        // the count at `0x80084594` and the member list at `0x80084598`; a
        // full-roster encoding for a one-member party is what used to reload
        // a Vahn-alone save as a four-member party.
        let active_party: Vec<u8> = if self.party.active_party.is_empty() {
            let n = match usize::from(self.party.party_count) {
                0 => party.members.len(),
                n => n.min(party.members.len()),
            };
            (0..n as u8).collect()
        } else {
            self.party.active_party.clone()
        };
        let mut per_char: Vec<(u8, legaia_save::CharSaveExt)> = Vec::new();
        for slot in 0..party.members.len() as u8 {
            let mut ce = legaia_save::CharSaveExt::default();
            // Learned arts: derive from TacticalArtsTracker - bit i is
            // set when art id i has crossed the learn threshold.
            for art_id in 0..32u8 {
                if self.party.tactical_arts.is_learned(slot, art_id) {
                    ce.learned_arts_mask |= 1u32 << art_id;
                }
            }
            // Spells: the per-character learned spell list from the seru log.
            ce.spells = self.seru.log.learned_spells(slot).to_vec();
            // Seru captures: export the live log's per-Seru capture-point
            // progress (real seru_id -> points) so sub-threshold progress
            // survives a save/load. Sorted for deterministic output.
            ce.seru_captures = self
                .seru
                .log
                .iter_rows()
                .filter(|(s, _, _)| *s == slot)
                .map(|(_, sid, row)| (sid, row.points))
                .collect();
            ce.seru_captures.sort_by_key(|&(sid, _)| sid);
            // Shiny spells: spell ids this character learned from a shiny
            // capture (+35% damage). Persisted in the LGSF v4 LGX4 block.
            ce.shiny_spells = self
                .seru
                .log
                .iter_shiny()
                .filter(|(s, _)| *s == slot)
                .map(|(_, spell_id)| spell_id)
                .collect();
            ce.shiny_spells.sort_unstable();
            // Active-chain selection still lives in the per-char ext mirror.
            if let Some((_, src)) = self.party.per_char_ext.iter().find(|(s, _)| *s == slot) {
                ce.active_chains = src.active_chains;
            }
            per_char.push((slot, ce));
        }

        // The system-flag bank (retail `DAT_80085758`, the partition-2 gate
        // bitmap the field VM's 0x50/0x60/0x70 ops write) overlaps the saved
        // story-flag window at byte offset `0x158` (`0x80085758 - 0x80085600`).
        // Mirror the live bank into that window so gate/progression state
        // survives a save (the LGX3 block stores a u16-length bitmap, so a
        // bank longer than the retail window still fits).
        //
        // The live bank is authoritative for the whole `+0x158..` span: the
        // load seeds it from exactly that span, so a bit the bank no longer
        // holds is a flag the game cleared since the load. An OR over the
        // loaded bytes resurrected every such flag on the next save.
        // Where the player stands - retail's position snapshot
        // (`0x80084568` / `0x8008456C`), which `FUN_80016230` takes as the
        // field run hands over to the menu, so a save from the pause menu
        // carries the spot it was opened on. Only a walking (or paused)
        // world has one: a battle's actor slots hold the battle seats.
        let field_position = match self.mode {
            SceneMode::Field | SceneMode::WorldMap | SceneMode::Menu => {
                self.player_field_position()
            }
            _ => None,
        };
        let mut story_flag_bits = self.flags.story_flag_bits.clone();
        if !self.flags.system_flags.is_empty() || story_flag_bits.len() > SYSTEM_FLAG_WINDOW {
            let need = SYSTEM_FLAG_WINDOW + self.flags.system_flags.len();
            if story_flag_bits.len() < need {
                story_flag_bits.resize(need, 0);
            }
            let window = &mut story_flag_bits[SYSTEM_FLAG_WINDOW..];
            window.fill(0);
            window[..self.flags.system_flags.len()].copy_from_slice(&self.flags.system_flags);
        }
        legaia_save::SaveFile {
            party,
            ext: legaia_save::SaveExt {
                story_flags: self.flags.story_flags,
                story_flag_bits,
                money: self.party.money,
                inventory,
                item_slots,
                minigames: legaia_save::MinigameSave {
                    casino_coins: self.minigames.casino_coins,
                    point_card: self.minigames.point_card,
                    fishing_points: self.minigames.fishing_points,
                    fishing_lure: self.minigames.fishing_lure,
                    fishing_rod: self.minigames.fishing_rod,
                    fishing_best_points: self.minigames.fishing_best_points,
                    fishing_best_fish: self.minigames.fishing_best_fish,
                    fishing_casts: self.minigames.fishing_casts,
                    fishing_prizes_purchased: self.minigames.fishing_prizes_purchased,
                },
            },
            ext_v2: legaia_save::SaveExtV2 {
                play_time_seconds: self.clock.play_time_seconds,
                active_party,
                per_char,
                saved_chains: self.party.saved_chains.clone(),
                field_position,
                // Always the live pair: a block composed from scratch must not
                // read back as the all-zero pair (silence) on retail's load.
                audio_levels: Some(self.audio.levels),
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
        self.load_party(sf.party.clone());
        // Restore the present-party composition. The full-roster identity
        // order (what `save_full` writes when no composition is installed)
        // stays the identity default rather than a 3-cap reorder, so legacy
        // saves keep their historical party_count.
        let identity: Vec<u8> = (0..self.party.roster.members.len() as u8).collect();
        if sf.ext_v2.active_party != identity {
            self.set_active_party(sf.ext_v2.active_party.clone());
            // The same list is the field party - retail's `0x80084598`
            // member list and its `0x80084597` leader (the head, as the
            // leader swap keeps it) - which the field VM's party ops edit.
            if !sf.ext_v2.active_party.is_empty() {
                self.party.party_actor_slots = sf
                    .ext_v2
                    .active_party
                    .iter()
                    .take(4)
                    .map(|&id| Some(id))
                    .collect();
                self.party.party_leader_slot = sf.ext_v2.active_party.first().copied();
            }
        } else {
            self.party.active_party.clear();
        }
        self.load_story_flags(&sf);
        self.party.money = sf.ext.money;
        self.load_full_tail(sf);
    }

    /// Restore only the story-flag state of a [`legaia_save::SaveFile`]: the
    /// flag bitmap and the live system-flag bank seeded from it - the part of
    /// [`Self::load_full`] a scene's entry scripts and bind-time prologues
    /// read. A card load hydrates these before the landing scene's field
    /// init, as retail's card load fills the game-state window first.
    pub fn load_story_flags(&mut self, sf: &legaia_save::SaveFile) {
        self.flags.story_flags = sf.ext.story_flags;
        self.flags.story_flag_bits = sf.ext.story_flag_bits.clone();
        // Seed the live system-flag bank from the saved bitmap's `+0x158`
        // window (the retail overlap `save_full` mirrors into) so partition-2
        // record gates - story-progression one-shots, door cutscene beats -
        // resolve the same after a reload. OR-merge: a retail SC import that
        // populated `story_flag_bits` alone seeds the bank the same way.
        //
        // The span is the whole bank: `legaia_save`'s story window runs to the
        // item array (`0x80085958`), which is where the `0x200`-byte bank
        // ends, so flags `0x000..=0xFFF` all arrive - retail's card load
        // restores the same span in its one `0x1A18`-byte copy.
        self.flags.system_flags.clear();
        if self.flags.story_flag_bits.len() > SYSTEM_FLAG_WINDOW {
            let window = self.flags.story_flag_bits[SYSTEM_FLAG_WINDOW..].to_vec();
            self.flags.system_flags = window;
        }
    }

    /// [`Self::load_full`] past the party, flags and gold.
    fn load_full_tail(&mut self, sf: legaia_save::SaveFile) {
        // The configured audio level and the voice volume ride in the same
        // live-state window as the gold; retail's card load restores them with
        // it and the next MAN load re-applies the level. A save naming no
        // pair keeps the current one.
        if let Some(levels) = sf.ext_v2.audio_levels {
            self.audio.levels = levels;
        }
        // The minigame purses and records - retail keeps all of them in the
        // live-state window a save block mirrors (`legaia_save::minigame_save`).
        let m = sf.ext.minigames;
        self.minigames.casino_coins = m.casino_coins;
        self.minigames.point_card = m.point_card;
        self.minigames.fishing_points = m.fishing_points;
        self.minigames.fishing_lure = m.fishing_lure;
        self.minigames.fishing_rod = m.fishing_rod;
        self.minigames.fishing_best_points = m.fishing_best_points;
        self.minigames.fishing_best_fish = m.fishing_best_fish;
        self.minigames.fishing_casts = m.fishing_casts;
        self.minigames.fishing_prizes_purchased = m.fishing_prizes_purchased;
        // Prefer the physical array: it is the only form that carries slot
        // order and the holes a played-through bag has, and PROT 0941's Steal
        // samples both. A file written before the `LGX6` block - or an
        // importer that only had the compact list - seeds densely from the
        // list instead, which is what every load did before.
        if sf.ext.item_slots.is_empty() {
            self.party.inventory.clear();
            for (id, count) in sf.ext.inventory {
                if count > 0 {
                    self.party.inventory.insert(id, count);
                }
            }
        } else {
            let window = self.party.inventory.window();
            self.party.inventory = crate::world::ItemBag::from_slots(&sf.ext.item_slots);
            self.party.inventory.set_window(window);
        }
        // (The level-up tracker's per-slot XP + level are hydrated from the
        // records inside `load_party`.)
        // V2 ext block - repopulate engine-side trackers.
        self.clock.play_time_seconds = sf.ext_v2.play_time_seconds;
        self.party.saved_chains = sf.ext_v2.saved_chains.clone();
        self.party.per_char_ext = sf.ext_v2.per_char.clone();
        // Reset trackers so reloads don't accumulate stale state.
        self.party.tactical_arts = TacticalArtsTracker::new();
        self.seru.log = crate::seru_learning::SeruCaptureLog::new();
        for (slot, ce) in &sf.ext_v2.per_char {
            // Re-mark learned arts so the tracker doesn't re-fire the
            // "first time learned" event for arts the save already has.
            for art_id in 0..32u8 {
                if ce.learned_arts_mask & (1u32 << art_id) != 0 {
                    self.party.tactical_arts.mark_known(*slot, art_id);
                }
            }
            // Restore per-Seru capture-point progress. When the registry is
            // installed, a row that's already over threshold restores as
            // learned (with its spell), so a later capture doesn't re-fire
            // the learn event.
            for &(sid, pts) in &ce.seru_captures {
                let def = self.seru.registry.get(sid);
                let learned = def.is_some_and(|d| pts >= d.learn_threshold);
                let spell_id = def.map(|d| d.spell_id);
                self.seru
                    .log
                    .restore_row(*slot, sid, pts, 0, learned, spell_id);
            }
            // Ensure every persisted learned spell lands in the learned list,
            // even with no registry installed: map it back to its teaching
            // Seru when known, else key by the spell id as a surrogate.
            for &spell_id in &ce.spells {
                if let Some(def) = self.seru.registry.seru_for_spell(spell_id) {
                    self.seru.log.mark_learned(*slot, def.id, spell_id);
                } else {
                    self.seru.log.mark_learned(*slot, spell_id as u16, spell_id);
                }
            }
            // Restore the shiny set (+35% damage spells).
            for &spell_id in &ce.shiny_spells {
                self.seru.log.mark_shiny(*slot, spell_id);
            }
        }
        // The accessory passive mask (`DAT_80074358`) is derived state, not
        // part of the saved live-state window: re-derive it from the loaded
        // equipment now, so the field's own readers - the encounter-rate
        // modifiers and the passive-ability badge column - see the loaded
        // party's accessories before the first battle entry would have
        // rebuilt it. A no-op without the disc's passive table.
        self.refresh_party_ability_bits();
    }
}

/// Per-scene save permission and the pause menu's entry-context kind - the two
/// gate inputs the retail root command picker reads before it lets a row
/// through.
impl World {
    /// Seed [`crate::world::PartyState::scene_save_allowed`] from the scene MAN just loaded.
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
        self.party.scene_save_allowed = man.is_some_and(|m| m.header.low_flag);
    }

    /// Release an op-`0x49` **menu-entry-context** park once the pause menu
    /// has closed. Returns whether a park was released.
    ///
    /// A scripted menu press ([`World::scripted_menu_open_pending`]) ends the
    /// way retail's does: the pause-menu session's last phase clears the
    /// cursor context's completion gate (`sh zero,0x3e(v0)` at `0x801ED52C`),
    /// and the dispatcher `FUN_801F159C` then retires the subsystem actor and,
    /// because the park is still live, writes the Done sentinel into it
    /// (`0x801F1678` `lw v1,-0x4bb0(a2)`, `bne v1,zero,0x801F16A8`,
    /// `sw 1,-0x4bb0(a2)`). So the parked op resumes once - a save point's
    /// `49 01` takes its Done arm, a `49 0D` advances - rather than re-arming
    /// and pressing the menu again. The port raises the screen's Done for the
    /// park's own context and drops the kind byte.
    ///
    /// Any other park is dropped without a Done, as before.
    ///
    /// The three-instruction leaf `FUN_8003540C` (`sw zero,0x148(gp)` /
    /// `sw zero,0x138(gp)` / `jr ra`) is **not** this release: it has no
    /// reference of any form on the disc (`find-address-word-refs.py 8003540c
    /// --prot`), so nothing ever zeroes the park on a menu close. Its sibling
    /// `FUN_800353E0`, which makes the same two stores, is reached only from
    /// the scene loaders (`0x8003B2C8`, `0x80055FC8`).
    ///
    /// REF: FUN_801F159C (retire arm), FUN_801ED308 (phase 5)
    pub fn release_menu_entry_context_park(&mut self) -> bool {
        let Some(kind) = self.field_vm.submode_screen.park_sub_op else {
            return false;
        };
        if crate::field_submode_screen::OP49_PARK_PRESERVING_SUB_OPS.contains(&kind) {
            self.field_vm.submode_screen.done = true;
        }
        self.clear_op49_park();
        true
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
    /// The port has no single global to read, because it tags each park with
    /// the context that armed it
    /// ([`crate::field_submode_screen::Op49ParkOwner`]), so it keeps the byte
    /// itself instead - on
    /// [`crate::field_submode_screen::SubmodeScreen::park_sub_op`], written
    /// by the arm ([`World::record_op49_park`]) and cleared by the resume.
    /// That covers every sub-op, including the three the port resolves
    /// through dedicated host paths, because retail's store happens before
    /// those paths diverge.
    ///
    /// The two legacy derivations below it stay as a fallback for a host
    /// that arms a shop or a tile board **without** going through the field
    /// VM (`World::try_arm_field_shop` and `World::try_install_tile_board`
    /// are both callable directly, and several tests do exactly that). They
    /// agree with the park by construction - an inline shop is sub-op `0`
    /// and a tile board sub-op `5`.
    ///
    /// Retail's own consumers of this byte, and which value selects each:
    ///
    /// | kind | consumer |
    /// |---|---|
    /// | `0` | save/menu driver opens sub-screen `0x1A` |
    /// | `1` | opens `0x19` - a field save point goes straight to the card |
    /// | `7` | opens `0x20` - the casino prize exchange |
    /// | `0x0D` | opens `4`, blocks the root Load row, arms the leave confirm |
    ///
    /// (`FUN_801DC6B4` at `0x801dc88c..0x801dc8e4`.) Which of those a real
    /// disc ever arms is measured by
    /// `crates/engine-core/tests/op49_sub_op_census.rs`.
    ///
    /// REF: FUN_801de840 (op `0x49` Idle arm, `_DAT_8007B450 = operand`)
    /// REF: FUN_801dc6b4 (the routing consumer)
    pub fn menu_entry_context_kind(&self) -> Option<u8> {
        if let Some(sub_op) = self.field_vm.submode_screen.park_sub_op {
            return Some(sub_op);
        }
        if self.shops.shop_armed {
            return Some(0);
        }
        if self.board.armed {
            return Some(5);
        }
        None
    }

    /// Whether this scene mode is one whose player is walked by the field
    /// locomotion controller - and therefore one whose pad reaches the
    /// **menu-open accept**.
    ///
    /// Retail has no global Start handler. The accept is a leg of the
    /// pre-movement header inside `FUN_801D01B0`
    /// (`0x801D0250..0x801D032C`): it tests the newly-pressed pad word
    /// `_DAT_8007B874` against the configurable menu-button mask
    /// `_DAT_800846D8`, and on a hit plays cue `0x20`, raises the player's
    /// engaged bit `+0x10 |= 0x80000`, and spawns the menu actor through the
    /// shared allocator `FUN_80020DE0(&DAT_8007065C, _DAT_8007C34C)`. So
    /// "can the menu open here" is exactly "does this scene run
    /// `FUN_801D01B0`".
    ///
    /// The **overworld runs it too**, and that is what makes the Save row
    /// reachable at all. All three kingdom overworlds are ordinary
    /// `game_mode 0x03` field-run scenes driven by the same
    /// `FUN_801D1344` -> `FUN_801D01B0` chain as a town, so the same accept
    /// covers both. Retail's own proof sits inside the controller: the
    /// base-step selector's `s4 = 5` arm at `0x801D0354` is taken exactly
    /// when the world-map flag `_DAT_8007B6A8` is set, which would be dead
    /// code if a `_DAT_8007B6A8` scene never entered this function.
    ///
    /// `FUN_801E76D4` is **not** a second controller that would need its own
    /// arm - it is the top-view debug renderer, and it branches straight to
    /// its epilogue (`0x801E9B14`) whenever `DAT_801F2B94 == 0`. Entering
    /// top view at all needs the debug flag `_DAT_8007B98C`, which retail
    /// leaves clear. See [`crate::world_map::WorldMapController`].
    ///
    /// The port splits the one retail mode in two -
    /// [`SceneMode::Field`](crate::world::SceneMode::Field) for towns and
    /// fields, [`SceneMode::WorldMap`](crate::world::SceneMode::WorldMap)
    /// for the kingdom overworlds - so the predicate has to name both. Every
    /// other mode (battle, cutscene, the minigames, an already-open menu)
    /// suspends field dispatch and never reaches the accept.
    ///
    /// REF: FUN_801D01B0 (`0x801D0250..0x801D032C`, the menu-open accept)
    /// REF: FUN_801D1344 (the per-frame tick that calls it, `0x801D16F4`)
    /// REF: FUN_801E76D4 (`0x801E779C`, the not-top-view branch to the epilogue)
    pub fn scene_mode_takes_menu_open(&self) -> bool {
        matches!(
            self.mode,
            crate::world::SceneMode::Field | crate::world::SceneMode::WorldMap
        )
    }

    /// The whole menu-open precondition, in the order retail tests it.
    ///
    /// Both shipped hosts and the shared `BootSession` driver must route
    /// their Start edge through this one predicate rather than spelling the
    /// mode test out locally - a host that writes its own copy is how the
    /// overworld lost the pause menu in the first place.
    ///
    /// 1. The engaged bit. `FUN_801D01B0`'s **first** test (`0x801D01F0`)
    ///    branches past the entire header when `player+0x10 & 0x80000` is
    ///    set, so a talking player's Start opens nothing and does not even
    ///    buzz. The engine's stand-in is
    ///    [`World::dialogue_owns_input`](crate::world::World::dialogue_owns_input).
    ///    A conversation is only one of the bit's raisers. The per-actor
    ///    script runner `FUN_80039B7C` raises it too, on every frame it steps
    ///    an engaged context (`0x80039DB8..0x80039DD4`, with the running
    ///    count at `_DAT_801C6EA4+0xA`), and clears it only once that count
    ///    drains on a `0x21` yield (`0x80039EE8..0x80039F14`). So while a
    ///    spawned record is parked mid-script - a walk-on beat's camera
    ///    move or `ExecMove` wait - the caller `FUN_801D1344` never calls
    ///    the controller (`0x801D1694`) and the menu button does nothing.
    ///    A Door of Light that lands on `map01`'s cave-mouth trigger tile
    ///    runs `P2[9]` this way and holds the menu for its whole run. The
    ///    engine's counterpart is
    ///    [`World::script_context_engages_player`](crate::world::World::script_context_engages_player):
    ///    the modal timeline or any live concurrent helper record.
    /// 2. The scene mode - [`Self::scene_mode_takes_menu_open`].
    ///
    /// Note this is the *open* gate only. Whether the opened menu's **Save**
    /// row then accepts is a separate, per-scene question answered by
    /// [`crate::world::PartyState::scene_save_allowed`](crate::world::PartyState::scene_save_allowed)
    /// at the row's confirm, exactly as retail keeps `_DAT_800846D8` (which
    /// button opens the menu) and `_DAT_8007B6A8` (whether Save is legal
    /// here) as two independent globals.
    ///
    /// 3. The successor the menu button's subsystem actor picks
    ///    ([`Self::field_menu_button_state`]) is the pause-menu session.
    ///    Only a debug build holding the pad's `0x100` bit picks anything else.
    ///
    /// 4. No shop / prize counter and no narration crawl or title card holds
    ///    the screen. A shop runs at game mode `0x17` with the menu overlay
    ///    resident and the field overlay swapped out, so the locomotion
    ///    controller whose header reads Start is not running at all; the
    ///    crawl owns the scene and freezes the pad. The native window used to
    ///    spell both out beside this call (`menu_runtime.is_open()` and its
    ///    `narration` local) while the browser page asked only this
    ///    predicate - and read the pause-menu Start before its shop - so on
    ///    the page Start opened the pause menu over an open shop.
    ///
    /// 5. The player tick calls the pad controller at all. `FUN_801D1344`
    ///    skips `FUN_801D01B0` - and with it the menu accept - while the
    ///    scratchpad system lock `_DAT_1F800394 & 0x400` is set
    ///    (`0x801D16A8..0x801D16B8`), while a kind-0 warp's timer
    ///    `_DAT_8007B6B0` runs, and while its post-warp pad hold
    ///    `_DAT_8007B6B4` drains (`0x801D16C8..0x801D16E4`,
    ///    [`legaia_engine_vm::field_warp_tile::pad_suppressed`]). The engine
    ///    already held the player still through the warp; the menu button
    ///    stayed live through it. No shipped script issues `2E 0A`, so the
    ///    lock's writer is not a field-VM op.
    ///
    /// 6. The menu lock `_DAT_1F800394 & 0x8000000` is clear. Unlike every
    ///    gate above it is tested *inside* the accept, after the press
    ///    (`0x801D02C0..0x801D02E4`): a locked press plays the deny buzz
    ///    `0x23` through `FUN_80035BD0` instead of opening anything - see
    ///    [`Self::field_menu_press_denied`]. `town0e` and `urudre1` raise it
    ///    (`2E 1B`); `urudre1`, `edteien` and `edbalden` drop it (`2F 1B`).
    ///
    /// 7. The New Game opening chain is not playing
    ///    ([`crate::world::CutsceneState::opening_chain_active`]). Its legs are
    ///    script records end to end, so retail's engaged bit stands through
    ///    all of them; the engine seats each leg's entry record one tick
    ///    after the scene loads, and this keeps that tick shut the way the
    ///    record does (`tests/opening_chain_menu_refusal_disc.rs`).
    ///
    /// REF: FUN_801D01B0 (`0x801D01F0` engaged bit, `0x801D0250` accept)
    /// REF: FUN_80039B7C (the script runner's engaged-bit raise and clear)
    /// REF: FUN_801D1344 (`0x801D16A8..0x801D16E4`, the pad-controller gates)
    pub fn field_menu_open_allowed(&self) -> bool {
        self.field_menu_press_reaches_accept() && self.flags.story_flags & FIELD_MENU_LOCK_BIT == 0
    }

    /// A menu-button press the accept takes but the menu lock refuses
    /// (`0x801D02C0..0x801D02E4`): queues the deny buzz
    /// [`FIELD_MENU_DENY_CUE`] on the SFX ring (`FUN_80035BD0(0x23)`, the
    /// overwrite producer) and returns `true`. Every other refusal is silent -
    /// it never reaches the accept - and returns `false` without a cue.
    ///
    /// Hosts call this on a menu-button edge that
    /// [`Self::field_menu_open_allowed`] refused.
    ///
    /// REF: FUN_801D01B0 (`0x801D02C0..0x801D02E4`, the locked-press buzz)
    /// REF: FUN_80035BD0
    pub fn field_menu_press_denied(&mut self) -> bool {
        if self.field_menu_press_reaches_accept()
            && self.flags.story_flags & FIELD_MENU_LOCK_BIT != 0
        {
            self.replace_last_sfx_cue(FIELD_MENU_DENY_CUE);
            return true;
        }
        false
    }

    /// Every gate in front of the menu accept's own lock test - items 1 to 5
    /// and 7 of [`Self::field_menu_open_allowed`].
    fn field_menu_press_reaches_accept(&self) -> bool {
        self.scene_mode_takes_menu_open()
            && self.flags.story_flags & FIELD_SYSTEM_LOCK_BIT == 0
            && !legaia_engine_vm::field_warp_tile::pad_suppressed(&self.locomotion.warp)
            && !self.cutscene.opening_chain_active
            && !self.dialogue_owns_input()
            && !self.script_context_engages_player()
            && !self.shops.shop_open
            && !self.shops.prize_exchange_open
            && !self.cutscene_narration_active()
            && self.cutscene.card.is_none()
            && self.field_menu_button_state() == legaia_engine_vm::field_state_pick::STATE_NORMAL
    }

    /// The handler id the menu button's subsystem actor moves on to.
    ///
    /// Retail's menu-open accept spawns the field overlay's subsystem actor
    /// (`FUN_80020DE0(0x8007065C, ..)` at `0x801D0324`); its installer
    /// `FUN_801F1278` stores handler id `7` (`0x801F140C`), and handler `7`
    /// is `FUN_801F1F4C`, which picks the successor: `0x30`, the pause-menu
    /// session `FUN_801ED308`, unless a script is parked on op `0x49`, the
    /// debug word `_DAT_8007B98C` is set and the packed pad holds `0x100` -
    /// then `0x13`, a debug screen the engine does not have. The engine's
    /// debug word is the overworld controller's
    /// [`crate::world_map::WorldMapController::debug_enabled`] (the same
    /// `_DAT_8007B98C` gate the top-view toggle reads); in a field scene it is
    /// retail's zero.
    ///
    /// PORT driver for FUN_801F1F4C (the kernel is
    /// `legaia_engine_vm::field_state_pick::state_pick`)
    pub fn field_menu_button_state(&self) -> u16 {
        use legaia_engine_vm::field_state_pick::{StatePickInputs, state_pick};
        let debug = self
            .world_map
            .ctrl
            .as_ref()
            .is_some_and(|c| c.debug_enabled);
        let inputs = StatePickInputs {
            script_resume_slot: u32::from(self.field_vm.submode_screen.park_sub_op.is_some()),
            debug_mode: u32::from(debug),
            pad_mask: u32::from(crate::world_map_panel_host::packed_pad(self.input.pad())),
        };
        state_pick(
            inputs,
            legaia_engine_vm::field_subsystem_enter::STATE_PICK_HANDLER,
        )
        .actor_state
    }
}
