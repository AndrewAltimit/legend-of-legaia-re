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

use legaia_asset::new_game::{StartingChar, StartingParty};
use legaia_save::character::{LiveStats, RECORD_CAP_CONSTANT, RecordStats};
use legaia_save::{CharacterRecord, HpMpSp, Party};

use crate::world::World;

/// Build a live 0x414-byte character record from one starting-party template
/// row. The mapping is validated against an early `town01` save state (Vahn):
/// the template's eight `u16` stats fill the HP/MP/SP triplet, the live-stat
/// window, and the record-side stat window; the per-stat cap is the constant
/// the runtime uses ([`RECORD_CAP_CONSTANT`]), and the spirit gauge starts at
/// the template's agility value (Vahn: `100`).
pub fn starting_record(c: &StartingChar) -> CharacterRecord {
    let mut rec = CharacterRecord::zeroed();
    rec.set_hp_mp_sp(HpMpSp {
        hp_cur: c.hp_max,
        hp_max: c.hp_max,
        mp_cur: c.mp_max,
        mp_max: c.mp_max,
        sp_cur: c.agl,
        sp_max: 0,
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
    rec.set_magic_rank(1);
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
        assert_eq!(hms.sp_cur, 100);
        let ls = rec.live_stats();
        assert_eq!(
            (ls.agl, ls.atk, ls.udf, ls.ldf, ls.spd, ls.int),
            (100, 24, 16, 12, 19, 9)
        );
        let rs = rec.record_stats();
        assert_eq!(rs.hp_max, 180);
        assert_eq!(rs.cap_constant, RECORD_CAP_CONSTANT);
        assert_eq!(rec.magic_rank(), 1);
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
