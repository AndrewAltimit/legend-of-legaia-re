//! New Game seeding: turn the `SCUS_942.54` starting-party template
//! ([`legaia_game_tables::new_game`]) into live engine party records.
//!
//! This is the engine side of the New Game boot chain (see
//! `docs/subsystems/boot.md` and `docs/formats/new-game-table.md`). The title
//! screen's NEW GAME confirm clears the world to a fresh slate
//! ([`crate::World::begin_new_game`]) and seeds the starting party from the
//! template the executable carries; this module applies that conversion to a
//! `World` so a caller with the disc's SCUS image can stand up a faithful
//! starting roster without any committed Sony bytes. The conversion itself -
//! the record a template row seeds, the picker party's starter loadout, the
//! cold-boot defaults bundle and the cold-reset values - names no `World` and
//! is [`legaia_engine_menus::new_game`].
//!
//! At a true New Game only Vahn (template slot 0) has actually joined; the
//! other template rows (Noa / Gala / Terra) are the records the game uses as
//! each character is later introduced, so [`World::seed_starting_party`] seeds
//! Vahn alone.

use legaia_engine_menus::new_game::{
    NewGameDefaults, PICKER_PARTY_SLOTS, starter_loadout, starting_record,
};
use legaia_game_tables::new_game::{StartingInventory, StartingParty};
use legaia_save::{CharacterRecord, Party};

use crate::world::World;

impl World {
    /// The whole New Game slate in one call: [`World::begin_new_game`], then
    /// the SCUS starting party and starting bag when the host has them.
    ///
    /// Both hosts' New Game entries run exactly this before entering the
    /// opening scene ([`legaia_engine_scene_host::resume::enter_new_game`]); each used to spell
    /// the three steps out for itself. `None` for either seed leaves the
    /// default scaffold (party) or an empty bag (inventory).
    pub fn begin_new_game_seeded(
        &mut self,
        party: Option<&StartingParty>,
        inventory: Option<&StartingInventory>,
    ) {
        self.begin_new_game();
        if let Some(starting) = party {
            self.tables.starting_party = Some(starting.clone());
            self.seed_starting_party(starting);
        }
        if let Some(inv) = inventory {
            self.seed_starting_inventory(inv);
        }
    }

    /// Seed the starting party for a New Game from the SCUS starting-party
    /// template. Loads Vahn (template slot 0) into party slot 0 - the only
    /// member who has joined at a true New Game - and folds his equipment-free
    /// live stats into the battle mirrors via [`World::seed_party_battle_stats`].
    ///
    /// A no-op when the template has no slot-0 record. Intended to run right
    /// after [`World::begin_new_game`], which establishes the fresh slate this
    /// roster drops into.
    pub fn seed_starting_party(&mut self, starting: &StartingParty) {
        let Some(vahn) = starting.member(0) else {
            return;
        };
        self.load_party(Party {
            members: vec![starting_record(vahn)],
        });
        // Seed EVERY template row's display name, not just the lead's.
        //
        // Retail's seed routine copies all four rows' names into their live
        // records at `+0x2A7` (see [`crate::save`] / `docs/subsystems/boot.md`),
        // so Noa / Gala / Terra carry their defaults from New Game onward -
        // long before they join the roster. That matters because the dialog
        // renderer's `0xC1 XX` escape reads the *record*, not a side table
        // (`docs/formats/mes.md`), and the game substitutes those names in
        // cutscenes the player reaches well before the characters do.
        //
        // Truncating this to the lead is what made a scene-selector cutscene
        // render an empty string where a name belongs: `0xC1 01` resolved to
        // nothing and the panel emits nothing for an absent entry. The roster
        // deliberately stays Vahn-alone (retail's New Game party), so this
        // touches display names only - `party_count` is untouched.
        self.party.party_names = starting.members().iter().map(|m| m.name.clone()).collect();
        self.seed_party_battle_stats();
    }

    /// Seed roster records for a present party that names members the story
    /// has not introduced yet - the harness path behind `play-window --party`.
    ///
    /// [`Self::seed_starting_party`] is retail's New Game roster and stays
    /// Vahn-alone, because at a true New Game he is the only member who has
    /// joined. That leaves every other battle ordinal resolving (through
    /// [`crate::world::World::party_roster_slot`]) to a roster slot that does
    /// not exist, so the HUD reads `0/0` with a placeholder name, the battle
    /// actor never gets a live HP / SPD mirror, and
    /// [`Self::seed_party_battle_stats`] skips the slot outright. A
    /// three-member fight staged for testing is then unplayable.
    ///
    /// This fills the gap from the **same** SCUS template
    /// ([`legaia_game_tables::new_game::StartingParty`], `0x80078C4C`) whose rows
    /// retail seeds Noa / Gala / Terra from when they later join, so each
    /// named member arrives with its own level-1 record and template name
    /// rather than a fabricated one. The roster is grown to cover the highest
    /// named slot; slots that are addressable but not named stay zeroed (no
    /// battle ordinal resolves to them).
    ///
    /// A slot already carrying a real record - non-zero max HP, i.e. a loaded
    /// save or an earlier seed - is left untouched, so this can never clobber
    /// progress. Returns how many slots it seeded.
    pub fn seed_party_members(&mut self, starting: &StartingParty, slots: &[u8]) -> usize {
        let Some(&highest) = slots.iter().max() else {
            return 0;
        };
        let need = highest as usize + 1;
        while self.party.roster.members.len() < need {
            self.party.roster.members.push(CharacterRecord::zeroed());
        }
        let mut seeded = 0usize;
        for &slot in slots {
            let idx = slot as usize;
            if self.party.roster.members[idx].hp_mp_sp().hp_max != 0 {
                continue;
            }
            let Some(tpl) = starting.member(idx) else {
                continue;
            };
            self.party.roster.members[idx] = starting_record(tpl);
            if self.party.party_names.len() <= idx {
                self.party.party_names.resize(idx + 1, String::new());
            }
            if self.party.party_names[idx].is_empty() {
                self.party.party_names[idx] = tpl.name.clone();
            }
            seeded += 1;
        }
        if seeded > 0 {
            // Re-run `load_party`'s projection over the grown roster: it is
            // what activates the actor, copies HP / MP into the battle
            // mirrors and hydrates the level-up tracker. Cloning the roster
            // through it keeps this module from re-stating that mapping.
            let roster = self.party.roster.clone();
            self.load_party(roster);
        }
        seeded
    }

    /// Seed the **scene-picker party**: Vahn, Noa and Gala
    /// ([`PICKER_PARTY_SLOTS`]) as the present battle party, each from their
    /// own row of the SCUS starting-party template ([`starting_record`] - the
    /// same rows retail seeds them from when they join) and wearing the
    /// [`starter_loadout`] when an equipment table is reachable (`equip`, else
    /// the world's installed one).
    ///
    /// The one helper both picker hosts reach, through
    /// [`Self::seed_cold_boot_defaults`] with
    /// [`NewGameDefaults::picker_party`] set. It is **not** the New Game
    /// roster - [`Self::seed_starting_party`] keeps that Vahn-alone. Replaces
    /// whatever roster the world held, so callers gate it themselves (the
    /// cold-boot guard only fires on an empty roster). Returns how many
    /// members it seeded (`0` on an empty template).
    pub fn seed_picker_party(
        &mut self,
        starting: &StartingParty,
        equip: Option<&legaia_game_tables::equip_stats::EquipStatTable>,
    ) -> usize {
        let table = equip.cloned().or_else(|| self.tables.equip_stats.clone());
        let mut members = Vec::new();
        for slot in PICKER_PARTY_SLOTS {
            let Some(tpl) = starting.member(slot as usize) else {
                break;
            };
            let mut rec = starting_record(tpl);
            if let Some(t) = table.as_ref() {
                rec.set_equipment(starter_loadout(t, slot));
            }
            members.push(rec);
        }
        let n = members.len();
        if n == 0 {
            return 0;
        }
        self.load_party(Party { members });
        // Every template row's display name, as the New Game seed does, so
        // Terra's `0xC1 03` substitution still resolves.
        self.party.party_names = starting.members().iter().map(|m| m.name.clone()).collect();
        self.set_active_party((0..n as u8).collect());
        self.seed_party_battle_stats();
        n
    }

    /// Seed the New Game starting inventory from the SCUS seed
    /// ([`StartingInventory`], `FUN_80034A6C`). Vanilla retail is the single
    /// slot Healing Leaf (`0x77`) ×5; the starting-item randomizer rewrites the
    /// seed so this reflects whatever the patched disc grants. Runs right after
    /// [`World::begin_new_game`] (which clears the bag), so the engine begins a
    /// New Game with the same items the real game would. Counts are merged into
    /// any existing slot of the same id (stack), matching the bag's semantics.
    pub fn seed_starting_inventory(&mut self, inv: &StartingInventory) {
        for &(id, count) in inv.items() {
            if id == 0 || count == 0 {
                continue;
            }
            let slot = self.party.inventory.entry(id).or_insert(0);
            *slot = slot.saturating_add(count);
        }
    }

    /// Cold-boot guard: seed the retail new-game defaults **only when no
    /// party or save was ever loaded** (the roster is empty). Fired by
    /// [`legaia_engine_scene_host::scene_host::SceneHost::enter_field_scene`] when the host carries a
    /// [`NewGameDefaults`], so entering a scene directly - native
    /// `play-window --scene X` or the browser play page's scene picker -
    /// never leaves the pause menu reading a zeroed scaffold party.
    ///
    /// Seeds, in order:
    /// - the starting party ([`Self::seed_starting_party`] - Vahn alone, the
    ///   retail New Game roster),
    /// - the starting bag ([`Self::seed_starting_inventory`]) when the bag is
    ///   still empty,
    /// - the retail starting gold ([`crate::world::NEW_GAME_STARTING_GOLD`],
    ///   the `FUN_80034A6C` hardcode) when no gold was granted yet.
    ///
    /// Unlike [`crate::world::World::begin_new_game`] this clears **nothing**:
    /// story flags, pending transitions, and every other live field are left
    /// as they are, so a mid-session door transition (which re-runs the scene
    /// entry path) can never reset state - the roster guard makes the seed
    /// once-only anyway. Returns `true` when the seed fired.
    pub fn seed_cold_boot_defaults(&mut self, defaults: &NewGameDefaults) -> bool {
        if !self.party.roster.members.is_empty() {
            return false;
        }
        if defaults.picker_party {
            self.seed_picker_party(&defaults.party, defaults.equip_stats.as_ref());
        } else {
            self.seed_starting_party(&defaults.party);
        }
        if self.party.roster.members.is_empty() {
            // Empty template (unreadable SCUS) - nothing was seeded.
            return false;
        }
        if self.party.inventory.is_empty()
            && let Some(inv) = &defaults.inventory
        {
            self.seed_starting_inventory(inv);
        }
        if self.party.money == 0 {
            self.party.money = crate::world::NEW_GAME_STARTING_GOLD;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_game_tables::new_game::StartingChar;

    fn vahn() -> StartingChar {
        StartingChar {
            name: "Vahn".into(),
            hp_max: 180,
            mp_max: 20,
            agl: 100,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        }
    }

    #[test]
    fn seed_starting_party_loads_vahn_only() {
        let starting = StartingParty::from_members(vec![
            vahn(),
            StartingChar {
                name: "Noa".into(),
                hp_max: 150,
                ..Default::default()
            },
        ]);
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&starting);
        assert_eq!(
            world.party.party_count, 1,
            "only Vahn has joined at a New Game"
        );
        assert_eq!(world.party.roster.members.len(), 1);
        assert_eq!(world.actors[0].battle.max_hp, 180);
        assert!(world.actors[0].active);
    }

    #[test]
    fn seeded_new_game_party_is_the_walking_field_player() {
        // End-to-end (synthetic, no disc): after a New Game seeds Vahn into
        // slot 0, the scene-entry player install makes that same slot the
        // free-movement field player, so pressing a direction walks Vahn.
        use legaia_engine_mode::mode::SceneMode;
        use legaia_engine_system::input::PadButton;

        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&StartingParty::from_members(vec![vahn()]));
        // Scene entry configures party-leader slot 0 as the field player
        // (mirrors enter_field_scene -> install_field_player(0)).
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        // Open floor (no walls) + camera facing forward.
        world.locomotion.camera_azimuth = 0;

        let z0 = world.actors[0].move_state.world_z;
        world.set_pad(PadButton::Up as u16);
        world.step_field_locomotion();
        assert!(
            world.actors[0].move_state.world_z > z0,
            "Vahn should advance forward (Z+) when Up is held; z0={z0}, z1={}",
            world.actors[0].move_state.world_z
        );
    }

    #[test]
    fn name_entry_overwrites_the_seeded_lead_name() {
        use legaia_engine_menus::name_entry::{NameEntryInput, NameEntryState};
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&StartingParty::from_members(vec![vahn()]));
        assert_eq!(
            world.party_name(0),
            "Vahn",
            "template seeds the default name"
        );

        // Open the overlay, backspace the whole default, then type "Noa".
        world.open_name_entry(0);
        assert!(world.name_entry_active());
        for _ in 0..4 {
            world.step_name_entry(NameEntryInput {
                cancel: true,
                ..Default::default()
            });
        }
        // 'N' = row 2 col 3 = cell 37; 'o' = row 2 col 10 = cell 44;
        // 'a' = row 0 col 6 = cell 6. Drive the cursor + confirm each.
        let typed = [(37usize, 'N'), (44, 'o'), (6, 'a')];
        for (cell, g) in typed {
            world.party.name_entry.as_mut().unwrap().cursor = cell;
            assert_eq!(
                world.party.name_entry.as_ref().unwrap().glyph_at(cell),
                Some(g)
            );
            world.step_name_entry(NameEntryInput {
                confirm: true,
                ..Default::default()
            });
        }
        // Move to End and confirm twice (End -> Confirm -> Done).
        let end = legaia_engine_menus::name_entry::CHAR_CELLS + 16;
        world.party.name_entry.as_mut().unwrap().cursor = end;
        world.step_name_entry(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(
            world.party.name_entry.as_ref().unwrap().state,
            NameEntryState::Confirm
        );
        // The prompt opens on No (retail); Up moves the hand to Yes.
        world.step_name_entry(NameEntryInput {
            up: true,
            ..Default::default()
        });
        let committed = world.step_name_entry(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert!(committed, "Yes-confirm commits + closes the overlay");
        assert!(!world.name_entry_active());
        assert_eq!(world.party_name(0), "Noa");
        // Retail's carrier is the record, so the committed name has to land
        // there too or it is lost across a save/load round trip.
        assert_eq!(world.party.roster.members[0].name(), "Noa");
    }

    #[test]
    fn begin_new_game_seeded_resets_then_seeds_both() {
        let mut world = World::new();
        world.party.money = 9999;
        world.party.inventory.insert(0x10, 3);
        world.begin_new_game_seeded(
            Some(&StartingParty::from_members(vec![vahn()])),
            Some(&StartingInventory::from_items(vec![(0x77, 5)])),
        );
        assert_eq!(world.party.party_count, 1);
        assert_eq!(world.party.inventory.get(&0x10), None, "old bag cleared");
        assert_eq!(world.party.inventory.get(&0x77).copied(), Some(5));
        assert_ne!(world.party.money, 9999, "purse reset to the New Game gold");
        // No seeds: the reset alone.
        let mut bare = World::new();
        bare.party.inventory.insert(0x10, 3);
        bare.begin_new_game_seeded(None, None);
        assert!(bare.party.inventory.is_empty());
    }

    #[test]
    fn seed_starting_inventory_fills_the_bag() {
        let mut world = World::new();
        world.begin_new_game();
        assert!(world.party.inventory.is_empty(), "new game clears the bag");
        // Vanilla-shaped single slot.
        world.seed_starting_inventory(&StartingInventory::from_items(vec![(0x77, 5)]));
        assert_eq!(world.party.inventory.get(&0x77).copied(), Some(5));
        assert_eq!(world.party.inventory.len(), 1);
    }

    #[test]
    fn seed_starting_inventory_multi_slot_and_skips_empties() {
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_inventory(&StartingInventory::from_items(vec![
            (0x80, 2),
            (0x00, 9), // id 0 skipped
            (0x8a, 1),
            (0x7e, 0), // count 0 skipped
        ]));
        let mut got: Vec<(u8, u8)> = world
            .party
            .inventory
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        got.sort_unstable();
        assert_eq!(got, vec![(0x80, 2), (0x8a, 1)]);
    }

    #[test]
    fn cold_boot_defaults_seed_once_and_only_when_nothing_is_loaded() {
        let defaults = NewGameDefaults {
            party: StartingParty::from_members(vec![vahn()]),
            inventory: Some(StartingInventory::from_items(vec![(0x77, 5)])),
            picker_party: false,
            equip_stats: None,
        };
        // Fresh world: seed fires - party + bag + gold.
        let mut world = World::new();
        assert!(world.party.roster.members.is_empty());
        assert!(world.seed_cold_boot_defaults(&defaults));
        assert_eq!(world.party.party_count, 1);
        assert_eq!(world.party.roster.members.len(), 1);
        assert_eq!(world.actors[0].battle.max_hp, 180);
        assert_eq!(world.party.inventory.get(&0x77).copied(), Some(5));
        assert_eq!(world.party.money, crate::world::NEW_GAME_STARTING_GOLD);
        // Second entry (a door transition re-runs scene entry): no-op.
        world.party.money = 42;
        world.party.inventory.insert(0x80, 1);
        assert!(!world.seed_cold_boot_defaults(&defaults));
        assert_eq!(world.party.money, 42, "re-entry must not reset gold");
        assert_eq!(world.party.inventory.get(&0x77).copied(), Some(5));

        // A world with a loaded party (a save) never seeds.
        let mut loaded = World::new();
        loaded.load_party(Party {
            members: vec![CharacterRecord::zeroed()],
        });
        loaded.party.money = 9999;
        assert!(!loaded.seed_cold_boot_defaults(&defaults));
        assert_eq!(loaded.party.money, 9999);
        assert!(
            loaded.party.inventory.is_empty(),
            "bag untouched on a loaded save"
        );
    }

    fn template(name: &str, hp: u16) -> StartingChar {
        StartingChar {
            name: name.into(),
            hp_max: hp,
            ..vahn()
        }
    }

    fn trio() -> StartingParty {
        StartingParty::from_members(vec![
            vahn(),
            template("Noa", 150),
            template("Gala", 220),
            template("Terra", 99),
        ])
    }

    /// `+7` byte per category: Body `0x00`, Head `0x20`, Weapon `0x40`,
    /// Footwear `0x60`; `| 1` marks Ra-Seru gear.
    fn bonus(atk: u8, udf: u8, mask: u8, cat: u8) -> [u8; 8] {
        let none = legaia_game_tables::equip_stats::PASSIVE_NONE;
        [0, atk, udf, 0, 0, none, mask, cat]
    }

    fn synthetic_equip_table() -> legaia_game_tables::equip_stats::EquipStatTable {
        legaia_game_tables::equip_stats::EquipStatTable::from_entries(&[
            // Weapons: Vahn-only 10, Vahn-only 4 (weakest), Noa-only 6,
            // Gala-only 9, a Ra-Seru weapon with 1 that must never be picked.
            (0x10, bonus(10, 0, 1, 0x40)),
            (0x11, bonus(4, 0, 1, 0x40)),
            (0x12, bonus(6, 0, 2, 0x40)),
            (0x13, bonus(9, 0, 4, 0x40)),
            (0x14, bonus(1, 0, 7, 0x41)),
            // Armour for everyone, head for everyone, boots Vahn-only.
            (0x20, bonus(0, 5, 7, 0x00)),
            (0x21, bonus(0, 3, 7, 0x00)),
            (0x30, bonus(0, 2, 7, 0x20)),
            (0x40, bonus(0, 1, 1, 0x60)),
            // A zero-stat placeholder is never picked.
            (0x41, bonus(0, 0, 7, 0x60)),
        ])
    }

    #[test]
    fn picker_cold_boot_seeds_vahn_noa_and_gala() {
        let defaults = NewGameDefaults {
            party: trio(),
            inventory: Some(StartingInventory::from_items(vec![(0x77, 5)])),
            picker_party: true,
            equip_stats: Some(synthetic_equip_table()),
        };
        let mut world = World::new();
        assert!(world.seed_cold_boot_defaults(&defaults));
        assert_eq!(world.party.party_count, 3);
        assert_eq!(world.party.active_party, vec![0, 1, 2]);
        assert_eq!(world.party.roster.members.len(), 3);
        assert_eq!(world.party_name(1), "Noa");
        assert_eq!(world.party_name(2), "Gala");
        // Terra's name still seeds for the dialog substitution.
        assert_eq!(world.party_name(3), "Terra");
        let hp: Vec<u16> = world
            .party
            .roster
            .members
            .iter()
            .map(|r| r.hp_mp_sp().hp_max)
            .collect();
        assert_eq!(hp, vec![180, 150, 220]);
        for r in &world.party.roster.members {
            assert_eq!(r.hp_mp_sp().hp_cur, r.hp_mp_sp().hp_max);
        }
        assert_eq!(world.party.roster.members[1].equipment().slots[3], 0x12);
        assert_eq!(world.party.money, crate::world::NEW_GAME_STARTING_GOLD);
        assert_eq!(world.party.inventory.get(&0x77).copied(), Some(5));
        // The battle mirrors cover all three ordinals.
        assert_eq!(world.actors[1].battle.max_hp, 150);
        assert_eq!(world.actors[2].battle.max_hp, 220);
        // Once-only, like the retail seed.
        assert!(!world.seed_cold_boot_defaults(&defaults));
    }

    #[test]
    fn new_game_after_a_picker_party_is_vahn_alone() {
        let mut world = World::new();
        world.seed_picker_party(&trio(), None);
        assert_eq!(world.party.party_count, 3);
        world.begin_new_game_seeded(Some(&trio()), None);
        assert_eq!(world.party.party_count, 1, "a New Game stays retail");
        assert_eq!(world.party.roster.members.len(), 1);
    }

    #[test]
    fn cold_boot_defaults_with_empty_template_are_a_no_op() {
        let defaults = NewGameDefaults {
            party: StartingParty::from_members(vec![]),
            inventory: Some(StartingInventory::from_items(vec![(0x77, 5)])),
            picker_party: false,
            equip_stats: None,
        };
        let mut world = World::new();
        assert!(!world.seed_cold_boot_defaults(&defaults));
        assert!(world.party.roster.members.is_empty());
        assert!(
            world.party.inventory.is_empty(),
            "no bag seed without a party seed"
        );
        assert_eq!(world.party.money, 0);
    }

    #[test]
    fn seed_starting_party_empty_template_loads_nothing() {
        // An empty template (e.g. SCUS unreadable) leaves the roster as it
        // was - nothing is loaded - so a caller falls back to whatever party
        // the world already held rather than crashing.
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&StartingParty::from_members(vec![]));
        assert!(world.party.roster.members.is_empty());
    }

    /// The four-row template, for the multi-member seeding cases.
    fn four_row_template() -> StartingParty {
        let mk = |name: &str, hp: u16, mp: u16, atk: u16| StartingChar {
            name: name.into(),
            hp_max: hp,
            mp_max: mp,
            agl: 100,
            atk,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        };
        StartingParty::from_members(vec![
            vahn(),
            mk("Noa", 150, 10, 21),
            mk("Gala", 210, 40, 27),
            mk("Terra", 120, 60, 15),
        ])
    }

    /// A New Game seeds EVERY template row's display name, not just the
    /// lead's - and does so without growing the roster.
    ///
    /// Retail copies all four rows' names into their records at `+0x2A7`, and
    /// the dialog renderer's `0xC1 XX` escape reads the record. Truncating the
    /// name list to the lead made any cutscene that names Noa / Gala / Terra
    /// render an empty string, which is reachable from the play page's scene
    /// selector long before those characters join.
    #[test]
    fn new_game_seeds_every_template_name_without_growing_the_roster() {
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&four_row_template());

        assert_eq!(world.party_name(0), "Vahn");
        assert_eq!(world.party_name(1), "Noa");
        assert_eq!(world.party_name(2), "Gala");
        assert_eq!(world.party_name(3), "Terra");

        // Retail's New Game party is Vahn alone; names are display state, not
        // membership, so neither the roster nor the party count may move.
        assert_eq!(world.party.roster.members.len(), 1);
        assert_eq!(world.party.party_count, 1);
    }

    /// A loaded save's custom names reach the display list, and loading a
    /// one-member roster does not clobber the not-yet-joined defaults.
    ///
    /// The record (`+0x2A7`) is retail's name carrier, so `load_party` has to
    /// adopt it; and because a cold-boot save carries only Vahn, the adoption
    /// must not shrink the list back to one entry.
    #[test]
    fn a_loaded_roster_name_reaches_the_display_list() {
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&four_row_template());

        let mut rec = world.party.roster.members[0].clone();
        rec.set_name("Zed");
        world.load_party(Party { members: vec![rec] });

        assert_eq!(world.party_name(0), "Zed", "the record's name wins");
        assert_eq!(
            world.party_name(1),
            "Noa",
            "a one-member roster must not truncate the seeded defaults"
        );
    }

    /// A present party that names members the story has not introduced gets
    /// real records for them, from the template rows retail seeds those
    /// characters from.
    ///
    /// Before this, `--seed-party` left the roster at Vahn alone (retail's New
    /// Game roster) and every other battle ordinal resolved to a roster slot
    /// that did not exist: the HUD drew `P2 0/0` / `P3 0/0` and the battle
    /// actors never got HP mirrors.
    /// A member joining a New Game party (the `0x3C` party add, which
    /// installs the present-party list) takes its template row: retail's New
    /// Game seeded all four live records, so Noa joins with her level-1
    /// record, not a zeroed one that fights at 0 / 0 HP and keeps the battle
    /// from ever seeing the party wiped.
    #[test]
    fn a_member_joining_a_new_game_party_gets_its_template_record() {
        let mut world = World::new();
        world.begin_new_game_seeded(Some(&four_row_template()), None);
        assert_eq!(world.party.roster.members.len(), 1);
        world.install_present_party_list(vec![0, 1]);
        assert_eq!(world.party.party_count, 2);
        let noa = world.party.roster.members[1].hp_mp_sp();
        assert_eq!((noa.hp_max, noa.hp_cur), (150, 150));
        assert_eq!(world.actors[1].battle.max_hp, 150);
        assert_eq!(world.actors[1].battle.liveness, 1);
        // Vahn's progress is not re-seeded by the join.
        assert_eq!(world.party.roster.members[0].hp_mp_sp().hp_max, 180);
    }

    #[test]
    fn a_named_present_party_gets_its_template_records() {
        let template = four_row_template();
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&template);
        assert_eq!(
            world.party.roster.members.len(),
            1,
            "retail New Game is Vahn only"
        );

        assert_eq!(world.seed_party_members(&template, &[0, 1, 2]), 2);
        assert_eq!(world.party.roster.members.len(), 3);
        for (slot, hp, mp) in [(0usize, 180u16, 20u16), (1, 150, 10), (2, 210, 40)] {
            let hms = world.party.roster.members[slot].hp_mp_sp();
            assert_eq!(hms.hp_max, hp, "slot {slot} max HP");
            assert_eq!(hms.hp_cur, hp, "slot {slot} starts at full");
            assert_eq!(hms.mp_max, mp, "slot {slot} max MP");
        }
        assert_eq!(world.party.party_names[1], "Noa");
        assert_eq!(world.party.party_names[2], "Gala");
        // `load_party`'s projection ran, so the actors carry live mirrors.
        // On the field only the walking player's slot is raised: slots 1..2
        // are the scene's, and a raise there spawned a phantom actor
        // (`World::party_mirror_activates`); the battle seats them itself.
        for slot in 0..3 {
            assert_eq!(world.actors[slot].active, slot == 0, "actor {slot} raise");
            assert_ne!(world.actors[slot].battle.max_hp, 0, "actor {slot} max HP");
            assert_eq!(world.actors[slot].battle.liveness, 1);
        }
    }

    /// Seeding must never overwrite a record that already carries stats - a
    /// loaded save, or a member the caller seeded some other way.
    #[test]
    fn seeding_a_present_party_leaves_real_records_alone() {
        let template = four_row_template();
        let mut world = World::new();
        let mut party = legaia_save::Party::zeroed(2);
        let mut hms = party.members[1].hp_mp_sp();
        hms.hp_max = 999;
        hms.hp_cur = 42;
        party.members[1].set_hp_mp_sp(hms);
        world.load_party(party);

        assert_eq!(
            world.seed_party_members(&template, &[0, 1]),
            1,
            "only the empty slot 0 is seeded"
        );
        assert_eq!(world.party.roster.members[1].hp_mp_sp().hp_max, 999);
        assert_eq!(world.party.roster.members[1].hp_mp_sp().hp_cur, 42);
        assert_eq!(world.party.roster.members[0].hp_mp_sp().hp_max, 180);
    }

    /// A slot the spec skips stays addressable but unseeded, and a spec with
    /// nothing in it is a no-op.
    #[test]
    fn seeding_grows_the_roster_only_as_far_as_the_spec_reaches() {
        let template = four_row_template();
        let mut world = World::new();
        assert_eq!(world.seed_party_members(&template, &[]), 0);
        assert!(world.party.roster.members.is_empty());

        assert_eq!(world.seed_party_members(&template, &[0, 2]), 2);
        assert_eq!(world.party.roster.members.len(), 3, "grown to cover slot 2");
        assert_eq!(
            world.party.roster.members[1].hp_mp_sp().hp_max,
            0,
            "the unnamed middle slot is not seeded"
        );
        assert_eq!(world.party.roster.members[2].hp_mp_sp().hp_max, 210);
    }
}
