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

use legaia_asset::new_game::{StartingChar, StartingInventory, StartingParty};
use legaia_save::character::{LiveStats, RECORD_CAP_CONSTANT, RecordStats};
use legaia_save::{CharacterRecord, HpMpSp, Party};

use crate::world::World;

/// Build a live 0x414-byte character record from one starting-party template
/// row. The mapping is validated against an early `town01` save state (Vahn):
/// the template's eight `u16` stats fill the HP/MP/SP triplet, the live-stat
/// window, and the record-side stat window; the per-stat cap is the constant
/// the runtime uses ([`RECORD_CAP_CONSTANT`]), and the spirit gauge's *max*
/// (`+0x10C` - AP max is AGL-sized) starts at the template's agility value
/// (Vahn: `100`, the capture-pinned `+0x10C` byte) with the current
/// (`+0x10E`, the status-page AP gauge cell) zeroed.
pub fn starting_record(c: &StartingChar) -> CharacterRecord {
    let mut rec = CharacterRecord::zeroed();
    rec.set_hp_mp_sp(HpMpSp {
        hp_cur: c.hp_max,
        hp_max: c.hp_max,
        mp_cur: c.mp_max,
        mp_max: c.mp_max,
        sp_cur: 0,
        sp_max: c.agl,
    });
    rec.set_live_stats(LiveStats {
        agl: c.agl,
        atk: c.atk,
        udf: c.udf,
        ldf: c.ldf,
        spd: c.spd,
        int: c.intel,
    });
    rec.set_record_stats(RecordStats {
        hp_max: c.hp_max,
        mp_max: c.mp_max,
        cap_constant: RECORD_CAP_CONSTANT,
        agl: c.agl,
        atk: c.atk,
        udf: c.udf,
        ldf: c.ldf,
        spd: c.spd,
        int: c.intel,
    });
    // Seru magic rank starts at 1 (Vahn, validated); gates the first spell tier.
    // (+0x130 doubles as the retail displayed level - LV 1 at a New Game.)
    rec.set_magic_rank(1);
    // Retail New Game records carry cumulative XP 0 (+0x0) and the L2
    // threshold in +0x4 - the Status menu's "Next Level 121" (base curve;
    // slots 1/2 hold the ± sin-divisor-corrected value: Noa 102 / Gala 140).
    rec.set_cumulative_xp(0);
    rec.set_next_level_xp(legaia_save::xp_for_level(2));
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(hms.sp_max, 100, "AP max (+0x10C) is AGL-sized at seed");
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
        // Retail New Game: Experience 0, Next Level 121 (Status-menu capture).
        assert_eq!(rec.cumulative_xp(), 0);
        assert_eq!(rec.next_level_xp(), 121);
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
