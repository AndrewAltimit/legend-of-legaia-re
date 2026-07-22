//! New Game seeding: turn the `SCUS_942.54` starting-party template
//! ([`legaia_asset::new_game`]) into live engine party records.
//!
//! This is the engine side of the New Game boot chain (see
//! `docs/subsystems/boot.md` and `docs/formats/new-game-table.md`). The title
//! screen's NEW GAME confirm clears the world to a fresh slate
//! ([`crate::World::begin_new_game`]) and seeds the starting party from the
//! template the executable carries; this module performs that conversion so a
//! caller with the disc's SCUS image can stand up a faithful starting roster
//! without any committed Sony bytes.
//!
//! At a true New Game only Vahn (template slot 0) has actually joined; the
//! other template rows (Noa / Gala / Terra) are the records the game uses as
//! each character is later introduced, so [`World::seed_starting_party`] seeds
//! Vahn alone.

use legaia_asset::new_game::{
    LIVE_RECORD_0_XP_OFFSET, StartingChar, StartingInventory, StartingParty, seed_live_records,
};
use legaia_save::{CharacterRecord, Party};

use crate::world::World;

/// The retail new-game defaults a scene host seeds on a **cold** scene boot -
/// entering a scene directly (native `play-window --scene X`, the site play
/// page's scene picker) without a New Game confirm or a loaded save. Bundles
/// the `SCUS_942.54` starting-party template with the starting-inventory seed
/// so [`World::seed_cold_boot_defaults`] can stand up a faithful slate and the
/// pause menu always has valid party data to read.
#[derive(Debug, Clone)]
pub struct NewGameDefaults {
    /// Starting-party template (`legaia_asset::new_game::StartingParty`,
    /// SCUS `0x80078C4C`).
    pub party: StartingParty,
    /// Starting-inventory seed (`FUN_80034A6C`; vanilla = Healing Leaf ×5).
    /// `None` when the seed couldn't be decoded - the bag stays empty.
    pub inventory: Option<StartingInventory>,
}

/// Build a live 0x414-byte character record from one starting-party template
/// row.
///
/// The stat halfwords are **not** re-derived here: they come from
/// [`seed_live_records`], the port of retail's seed routine `FUN_800560B4`,
/// applied at their record-relative offsets. That routine's stores are the
/// specification for which template field lands in which live cell, so routing
/// through it keeps the engine's record byte-identical to a retail New Game
/// record without this module re-stating the mapping.
///
/// The one cell where re-stating it went wrong is `+0x10C` (and its max-block
/// twin `+0x120`): retail writes the **literal** `100` there for every roster
/// slot (`800561b8 li v0,0x64` / `800561bc sh v0,0x6d4(s0)` /
/// `800561c0 sh v0,0x6e8(s0)`, inside the four-iteration loop), not the
/// template's agility field. Vahn's agility is also `100`, so a Vahn-only
/// capture cannot tell the two apart - Noa (`120`) and Gala (`80`) can, and
/// they seed `100`.
///
/// The seed routine's non-halfword writes stay here, because
/// [`seed_live_records`] deliberately does not carry them: the level /
/// magic-rank bytes, the XP pair, and the template-name `strcpy`.
pub fn starting_record(c: &StartingChar) -> CharacterRecord {
    let mut rec = CharacterRecord::zeroed();
    // Slot 0's live record base is `SC + LIVE_RECORD_0_XP_OFFSET`, so a
    // record-relative offset is the seed's SC offset less that base.
    let one = StartingParty::from_members(vec![c.clone()]);
    for store in seed_live_records(&one) {
        let off = (store.sc_offset - LIVE_RECORD_0_XP_OFFSET) as usize;
        rec.raw[off..off + 2].copy_from_slice(&store.value.to_le_bytes());
    }
    // Seru magic rank starts at 1 (Vahn, validated); gates the first spell tier.
    // (+0x130 doubles as the retail displayed level - LV 1 at a New Game.)
    rec.set_magic_rank(1);
    // Retail New Game records carry cumulative XP 0 (+0x0) and the L2
    // threshold in +0x4 - the Status menu's "Next Level 121" (base curve;
    // slots 1/2 hold the ± sin-divisor-corrected value: Noa 102 / Gala 140).
    rec.set_cumulative_xp(0);
    rec.set_next_level_xp(legaia_save::xp_for_level(2));
    // The template's 10-byte name field seeds the record's display name
    // (record +0x2A7) - retail shows it until the name-entry overlay
    // overwrites it.
    rec.set_name(&c.name);
    rec
}

impl World {
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
        // Seed the display name from the template; the name-entry overlay
        // overwrites slot 0 when the player names the lead character.
        self.party_names = vec![vahn.name.clone()];
        self.seed_party_battle_stats();
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
            let slot = self.inventory.entry(id).or_insert(0);
            *slot = slot.saturating_add(count);
        }
    }

    /// Cold-boot guard: seed the retail new-game defaults **only when no
    /// party or save was ever loaded** (the roster is empty). Fired by
    /// [`crate::scene::SceneHost::enter_field_scene`] when the host carries a
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
        if !self.roster.members.is_empty() {
            return false;
        }
        self.seed_starting_party(&defaults.party);
        if self.roster.members.is_empty() {
            // Empty template (unreadable SCUS) - nothing was seeded.
            return false;
        }
        if self.inventory.is_empty()
            && let Some(inv) = &defaults.inventory
        {
            self.seed_starting_inventory(inv);
        }
        if self.money == 0 {
            self.money = crate::world::NEW_GAME_STARTING_GOLD;
        }
        true
    }
}

/// PORT: FUN_8001FFA4
///
/// Game-state **cold-reset** defaults - the boot-time initializer one layer
/// above the new-game data-init `FUN_80034A6C`. Retail:
///
/// 1. zero-fills the `0x1A18`-byte live game-state block at `0x80084140`
///    (inventory pages, party fields, story flags - everything the save
///    block snapshots),
/// 2. seeds the non-zero boot defaults tabulated below,
/// 3. chains into `FUN_80034A6C` (new-game data-init: gold = 500,
///    starting-party template expansion) and `FUN_8002614C(0)` (audio-context
///    volume re-apply).
///
/// | Field | Retail global | Boot value |
/// |---|---|---|
/// | `flag_at_0x434` | `_DAT_80084574` | `1` |
/// | `brightness_ref` | `_DAT_8008457C` | `0xD7` (full-brightness reference the battle fades clamp against) |
/// | `voice_volume` | `_DAT_80084580` | `200` (voice/SFX volume config) |
/// | `screen_brightness` | `_DAT_8007B910` | `0xD7` (live brightness, ramped by battle-action fades) |
/// | `bgm_volume_raw` | `DAT_8007B6EC` | `-1` (field-BGM volume; see [`crate::scene::bgm_reattach_volume`]) |
///
/// `DAT_8007B750` and `_DAT_8007BAD0` are also cleared to `0` (already
/// covered by the engine's default state). The engine analog of steps 1 + 3
/// is [`World::begin_new_game`] + [`World::seed_cold_boot_defaults`]; this
/// struct carries the step-2 values so hosts seed them from one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStateColdReset {
    /// `_DAT_80084574` boot value.
    pub flag_at_0x434: u32,
    /// `_DAT_8008457C` - full-brightness reference.
    pub brightness_ref: i32,
    /// `_DAT_80084580` - voice/SFX volume config.
    pub voice_volume: i32,
    /// `_DAT_8007B910` - live screen brightness.
    pub screen_brightness: i32,
    /// `DAT_8007B6EC` - field-BGM volume raw global.
    pub bgm_volume_raw: i32,
}

/// The retail boot values `FUN_8001FFA4` stores (see [`GameStateColdReset`]).
pub const GAME_STATE_COLD_RESET: GameStateColdReset = GameStateColdReset {
    flag_at_0x434: 1,
    brightness_ref: 0xD7,
    voice_volume: 200,
    screen_brightness: 0xD7,
    bgm_volume_raw: -1,
};

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::character::RECORD_CAP_CONSTANT;

    /// FUN_8001FFA4's five non-zero boot stores, as read off the
    /// disassembly (`li` + `sw` pairs at `0x8001FFD4..0x80020004`).
    #[test]
    fn cold_reset_defaults_match_retail_stores() {
        let d = GAME_STATE_COLD_RESET;
        assert_eq!(d.flag_at_0x434, 1);
        assert_eq!(d.brightness_ref, 0xD7);
        assert_eq!(d.voice_volume, 200);
        // Boot: live brightness starts at the full-brightness reference
        // (both stores source the same 0xD7 register).
        assert_eq!(d.screen_brightness, d.brightness_ref);
        assert_eq!(d.bgm_volume_raw, -1);
        // The level the BGM re-attach (FUN_80019898) derives from the
        // boot-value raw global.
        assert_eq!(crate::scene::bgm_reattach_volume(d.bgm_volume_raw), -1);
    }

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
    fn starting_record_maps_template_stats() {
        let rec = starting_record(&vahn());
        let hms = rec.hp_mp_sp();
        assert_eq!(hms.hp_cur, 180);
        assert_eq!(hms.hp_max, 180);
        assert_eq!(hms.mp_cur, 20);
        assert_eq!(hms.mp_max, 20);
        // +0x10C is the seed routine's literal 100, not Vahn's agility - which
        // happens to be 100 too. `cap_cells_take_the_retail_literal` is the
        // test that can actually tell those apart.
        assert_eq!(hms.sp_max, 100);
        assert_eq!(hms.sp_cur, 0, "status-page AP gauge (+0x10E) seeds to 0");
        let ls = rec.live_stats();
        assert_eq!(
            (ls.agl, ls.atk, ls.udf, ls.ldf, ls.spd, ls.int),
            (100, 24, 16, 12, 19, 9)
        );
        let rs = rec.record_stats();
        assert_eq!(rs.hp_max, 180);
        assert_eq!(rs.cap_constant, RECORD_CAP_CONSTANT);
        assert_eq!(rec.magic_rank(), 1);
        assert_eq!(rec.name(), "Vahn", "template name stamps the record field");
        // Retail New Game: Experience 0, Next Level 121 (Status-menu capture).
        assert_eq!(rec.cumulative_xp(), 0);
        assert_eq!(rec.next_level_xp(), 121);
    }

    /// Non-Vahn template rows are the only ones that can distinguish the
    /// seed routine's cap literal from the template's agility field, because
    /// Vahn's agility *is* `100`. Retail writes `100` into both cap cells for
    /// every roster slot (`FUN_800560B4`, `li v0,0x64` inside the loop), so
    /// Noa's record must read `100` at `+0x10C` / `+0x120` and `120` only in
    /// the agility cells.
    #[test]
    fn cap_cells_take_the_retail_literal_not_agility() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        let rec = starting_record(&noa);
        assert_eq!(
            rec.hp_mp_sp().sp_max,
            legaia_asset::new_game::SEEDED_CAP_CONSTANT,
            "+0x10C is the seed literal, not the template agility"
        );
        assert_eq!(
            rec.stat_cap(),
            legaia_asset::new_game::SEEDED_CAP_CONSTANT,
            "+0x120 is the same literal"
        );
        assert_eq!(rec.live_stats().agl, 120, "agility still lands at +0x110");
        assert_eq!(rec.record_stats().agl, 120, "and at +0x122");
    }

    /// The record's stat halfwords must be exactly the seed model's stores -
    /// this is what makes [`seed_live_records`] the source of truth rather
    /// than a parallel description of it.
    #[test]
    fn every_seeded_halfword_lands_in_the_record() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        for c in [vahn(), noa] {
            let rec = starting_record(&c);
            let one = StartingParty::from_members(vec![c.clone()]);
            let stores = seed_live_records(&one);
            assert!(!stores.is_empty());
            for s in stores {
                let off = (s.sc_offset - LIVE_RECORD_0_XP_OFFSET) as usize;
                let got = u16::from_le_bytes([rec.raw[off], rec.raw[off + 1]]);
                assert_eq!(got, s.value, "record +{off:#x} disagrees with the seed");
            }
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
        assert_eq!(world.party_count, 1, "only Vahn has joined at a New Game");
        assert_eq!(world.roster.members.len(), 1);
        assert_eq!(world.actors[0].battle.max_hp, 180);
        assert!(world.actors[0].active);
    }

    #[test]
    fn seeded_new_game_party_is_the_walking_field_player() {
        // End-to-end (synthetic, no disc): after a New Game seeds Vahn into
        // slot 0, the scene-entry player install makes that same slot the
        // free-movement field player, so pressing a direction walks Vahn.
        use crate::input::PadButton;
        use crate::world::SceneMode;

        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&StartingParty::from_members(vec![vahn()]));
        // Scene entry configures party-leader slot 0 as the field player
        // (mirrors enter_field_scene -> install_field_player(0)).
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        // Open floor (no walls) + camera facing forward.
        world.field_camera_azimuth = 0;

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
        use crate::name_entry::{NameEntryInput, NameEntryState};
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
            world.name_entry.as_mut().unwrap().cursor = cell;
            assert_eq!(world.name_entry.as_ref().unwrap().glyph_at(cell), Some(g));
            world.step_name_entry(NameEntryInput {
                confirm: true,
                ..Default::default()
            });
        }
        // Move to End and confirm twice (End -> Confirm -> Done).
        let end = crate::name_entry::CHAR_CELLS + 16;
        world.name_entry.as_mut().unwrap().cursor = end;
        world.step_name_entry(NameEntryInput {
            confirm: true,
            ..Default::default()
        });
        assert_eq!(
            world.name_entry.as_ref().unwrap().state,
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
    }

    #[test]
    fn seed_starting_inventory_fills_the_bag() {
        let mut world = World::new();
        world.begin_new_game();
        assert!(world.inventory.is_empty(), "new game clears the bag");
        // Vanilla-shaped single slot.
        world.seed_starting_inventory(&StartingInventory::from_items(vec![(0x77, 5)]));
        assert_eq!(world.inventory.get(&0x77).copied(), Some(5));
        assert_eq!(world.inventory.len(), 1);
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
        let mut got: Vec<(u8, u8)> = world.inventory.iter().map(|(k, v)| (*k, *v)).collect();
        got.sort_unstable();
        assert_eq!(got, vec![(0x80, 2), (0x8a, 1)]);
    }

    #[test]
    fn cold_boot_defaults_seed_once_and_only_when_nothing_is_loaded() {
        let defaults = NewGameDefaults {
            party: StartingParty::from_members(vec![vahn()]),
            inventory: Some(StartingInventory::from_items(vec![(0x77, 5)])),
        };
        // Fresh world: seed fires - party + bag + gold.
        let mut world = World::new();
        assert!(world.roster.members.is_empty());
        assert!(world.seed_cold_boot_defaults(&defaults));
        assert_eq!(world.party_count, 1);
        assert_eq!(world.roster.members.len(), 1);
        assert_eq!(world.actors[0].battle.max_hp, 180);
        assert_eq!(world.inventory.get(&0x77).copied(), Some(5));
        assert_eq!(world.money, crate::world::NEW_GAME_STARTING_GOLD);
        // Second entry (a door transition re-runs scene entry): no-op.
        world.money = 42;
        world.inventory.insert(0x80, 1);
        assert!(!world.seed_cold_boot_defaults(&defaults));
        assert_eq!(world.money, 42, "re-entry must not reset gold");
        assert_eq!(world.inventory.get(&0x77).copied(), Some(5));

        // A world with a loaded party (a save) never seeds.
        let mut loaded = World::new();
        loaded.load_party(Party {
            members: vec![CharacterRecord::zeroed()],
        });
        loaded.money = 9999;
        assert!(!loaded.seed_cold_boot_defaults(&defaults));
        assert_eq!(loaded.money, 9999);
        assert!(
            loaded.inventory.is_empty(),
            "bag untouched on a loaded save"
        );
    }

    #[test]
    fn cold_boot_defaults_with_empty_template_are_a_no_op() {
        let defaults = NewGameDefaults {
            party: StartingParty::from_members(vec![]),
            inventory: Some(StartingInventory::from_items(vec![(0x77, 5)])),
        };
        let mut world = World::new();
        assert!(!world.seed_cold_boot_defaults(&defaults));
        assert!(world.roster.members.is_empty());
        assert!(
            world.inventory.is_empty(),
            "no bag seed without a party seed"
        );
        assert_eq!(world.money, 0);
    }

    #[test]
    fn seed_starting_party_empty_template_loads_nothing() {
        // An empty template (e.g. SCUS unreadable) leaves the roster as it
        // was - nothing is loaded - so a caller falls back to whatever party
        // the world already held rather than crashing.
        let mut world = World::new();
        world.begin_new_game();
        world.seed_starting_party(&StartingParty::from_members(vec![]));
        assert!(world.roster.members.is_empty());
    }
}
