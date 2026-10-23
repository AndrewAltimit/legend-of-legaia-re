//! LGSF saves over a running `World`: the full save-file round trip
//! ([`WorldSaveState::save_full`] / [`WorldSaveState::load_full`]), the
//! story-flag half a card load hydrates first, and the options screen's words
//! a save carries.
//!
//! The simulation never names this crate. A host (the scene host's resume,
//! the menu runtime's save slots, the play hosts' card and session saves)
//! brings the trait into scope and calls the methods on its `World`, the way
//! `legaia-engine-cheats` applies its cheats. The per-record half a save is
//! built from - `World::save_party` / `World::load_party`, which the tick and
//! New Game also run - stays in `engine-core`.

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;

/// Byte offset of the system-flag bank (`0x80085758`) inside the saved
/// story-flag window (`0x80085600`).
const SYSTEM_FLAG_WINDOW: usize = 0x158;

/// The LGSF save-file round trip, as methods on [`World`].
pub trait WorldSaveState {
    /// Capture the complete engine state (party + globals) into a [`legaia_save::SaveFile`].
    ///
    /// Pairs with [`Self::load_full`]. Use this instead of `World::save_party` when
    /// you need `story_flags`, `money`, and `inventory` to survive a save/load cycle.
    fn save_full(&mut self) -> legaia_save::SaveFile;

    /// Restore engine state from a [`legaia_save::SaveFile`] produced by [`Self::save_full`].
    ///
    /// Party records are applied through `World::load_party`; globals overwrite the
    /// current `story_flags`, `money`, and `inventory`. Sync per-slot
    /// `LevelUpTracker::level` from each loaded record's `+0x100` byte
    /// so reloads don't silently reset every party slot to level 1.
    fn load_full(&mut self, sf: legaia_save::SaveFile);

    /// Restore only the story-flag state of a [`legaia_save::SaveFile`]: the
    /// flag bitmap and the live system-flag bank seeded from it - the part of
    /// [`Self::load_full`] a scene's entry scripts and bind-time prologues
    /// read. A card load hydrates these before the landing scene's field
    /// init, as retail's card load fills the game-state window first.
    fn load_story_flags(&mut self, sf: &legaia_save::SaveFile);

    /// The options screen's nine config words as this session plays them:
    /// the rows the simulation reads off their live mirrors, the rest as the
    /// host last pushed them. What [`Self::save_full`] writes.
    fn retail_options(&self) -> legaia_save::card::RetailOptions;

    /// Set the options screen's words and the simulation's mirrors of them.
    fn set_retail_options(&mut self, options: legaia_save::card::RetailOptions);

    /// The option words the last save load restored, once: a host adopts
    /// them into its options (`OptionsPins::adopt_saved`, where a session
    /// override wins) before it pushes its options back onto the world.
    fn take_loaded_options(&mut self) -> Option<legaia_save::card::RetailOptions>;
}

impl WorldSaveState for World {
    fn save_full(&mut self) -> legaia_save::SaveFile {
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
                // Always the live words, for the same reason: retail's load
                // restores them with the window.
                options: Some(self.retail_options()),
            },
        }
    }

    fn load_full(&mut self, sf: legaia_save::SaveFile) {
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
            // No composition, so no leader word either: a leader left from
            // the session before the load would otherwise keep leading
            // (`World::field_leader`) and dress the player in its body.
            self.party.party_leader_slot = None;
        }
        self.load_story_flags(&sf);
        self.party.money = sf.ext.money;
        LoadFullTail::load_full_tail(self, sf);
    }

    fn load_story_flags(&mut self, sf: &legaia_save::SaveFile) {
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

    fn retail_options(&self) -> legaia_save::card::RetailOptions {
        legaia_save::card::RetailOptions {
            battle_camera: self.toggles.battle_camera as u8,
            select_attack: self.toggles.select_attack as u8,
            battle_command: self.toggles.battle_command as u8,
            field_move: u8::from(self.locomotion.run_default),
            hp_display: self.toggles.field_hp_display as u8,
            ..self.toggles.retail_options
        }
    }

    fn set_retail_options(&mut self, options: legaia_save::card::RetailOptions) {
        use legaia_engine_menus::options::{OptionsSetting, OptionsState};
        let mut o = OptionsState::default();
        for (s, v) in OptionsSetting::ALL.into_iter().zip(options.to_bytes()) {
            s.set(&mut o, v);
        }
        self.toggles.battle_camera = o.battle_camera;
        self.toggles.select_attack = o.battle_select_attack;
        self.toggles.battle_command = o.battle_command;
        self.locomotion.run_default =
            o.field_move == legaia_engine_system::option_values::FieldMoveOpt::Run;
        self.toggles.field_hp_display = o.field_hp_display;
        self.toggles.retail_options = options;
    }

    fn take_loaded_options(&mut self) -> Option<legaia_save::card::RetailOptions> {
        self.toggles.loaded_options.take()
    }
}

/// [`WorldSaveState::load_full`]'s tail, kept off the public trait.
trait LoadFullTail {
    fn load_full_tail(&mut self, sf: legaia_save::SaveFile);
}

impl LoadFullTail for World {
    /// [`WorldSaveState::load_full`] past the party, flags and gold.
    fn load_full_tail(&mut self, sf: legaia_save::SaveFile) {
        // The configured audio level and the voice volume ride in the same
        // live-state window as the gold; retail's card load restores them with
        // it and the next MAN load re-applies the level. A save naming no
        // pair keeps the current one.
        if let Some(levels) = sf.ext_v2.audio_levels {
            self.audio.levels = levels;
        }
        // The options screen's words sit in the same window: a save made
        // running resumes running. The simulation's mirrors take them at
        // once, so a session with no host options (a headless run) plays the
        // save's Field Move; a host adopts them into its own options through
        // `take_loaded_options`, which is where a session override wins.
        if let Some(options) = sf.ext_v2.options {
            self.set_retail_options(options);
            self.toggles.loaded_options = Some(options);
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
            self.party.inventory =
                legaia_engine_menus::item_bag::ItemBag::from_slots(&sf.ext.item_slots);
            self.party.inventory.set_window(window);
        }
        // (The level-up tracker's per-slot XP + level are hydrated from the
        // records inside `load_party`.)
        // V2 ext block - repopulate engine-side trackers.
        self.clock.play_time_seconds = sf.ext_v2.play_time_seconds;
        self.party.saved_chains = sf.ext_v2.saved_chains.clone();
        self.party.per_char_ext = sf.ext_v2.per_char.clone();
        // Reset trackers so reloads don't accumulate stale state.
        self.party.tactical_arts = legaia_engine_battle::tactical_arts::TacticalArtsTracker::new();
        self.seru.log = legaia_engine_battle::seru_learning::SeruCaptureLog::new();
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
