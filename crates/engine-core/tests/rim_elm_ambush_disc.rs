//! Disc-gated oracle: the **Rim Elm ambush** (`town0c`, field-VM op
//! `3E FF 03`) enters the engine the way a PCSX-Redux capture of the
//! `rim_elm_queen_bee_battle` state reads retail
//! (`scripts/pcsx-redux/autorun_rim_elm_ambush_seats.lua`):
//!
//! - `DAT_8007BD60 = 0x00100003`: bit 7 clear, so `ctx+0x287 = 0` - the fight
//!   is not scripted-flagged and Run stays open. The row's header byte is `0`
//!   and the op's arm (`0x801E070C..0x801E0788`) writes no flag of its own.
//! - The seat loop fetches monster row index 8 (count 4 + the map arm's 4):
//!   `(0,1000) (-600,800) (600,800) (0,600)`.
//! - The formation roll `FUN_80051D84` runs and its map-gated arm raises
//!   `_DAT_8007BAC0` to `0x200`.
//!
//! The contrast is garmel's Zeto row (header byte `1`): flagged, so Run is
//! refused, the roll is skipped and the word stays `0`.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::battle_input::BattleCommand;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;
use legaia_engine_vm::battle_formulas::SPECIAL_RASERU_FORBIDDEN;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(extracted)
}

/// Enter `scene`, fire `3E FF <row>`, and tick until the battle flips in.
fn enter_scripted(extracted: &PathBuf, scene: &str, row: u8) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene(scene, 0).expect("enter scene");
    assert!(
        host.world.trigger_scripted_battle(row),
        "{scene} row {row} is a registered formation"
    );
    for _ in 0..600 {
        if host.world.mode == SceneMode::Battle {
            break;
        }
        host.tick().expect("tick");
    }
    assert_eq!(
        host.world.mode,
        SceneMode::Battle,
        "{scene} row {row} entered battle"
    );
    host
}

/// The retail capture's monster seats, row index 8 of `0x80077608`.
const AMBUSH_SEATS: [(i32, i32); 4] = [(0, 1000), (-600, 800), (600, 800), (0, 600)];

#[test]
fn rim_elm_ambush_matches_the_retail_capture() {
    let Some(extracted) = gated() else { return };
    let host = enter_scripted(&extracted, "town0c", 3);
    let w = &host.world;

    assert_eq!(w.battle.map_id, 0x15, "town0c's raw CDNAME define");
    let ids: Vec<u16> = w
        .battle
        .active_formation
        .as_ref()
        .expect("active formation")
        .slots
        .iter()
        .map(|s| s.monster_id)
        .collect();
    assert_eq!(
        ids,
        vec![0x3F, 0x3E, 0x3E, 0x3E],
        "town0c row 3 off the disc"
    );

    // ctx+0x287 = 0: escapable, random-encounter boost profile.
    assert!(!w.battle.scripted_fight, "header byte 0 -> not scripted");
    assert_eq!(w.battle_ctx.scripted_fight, 0, "ctx+0x287");
    assert!(!w.battle.no_escape, "retail lets the party run");
    assert!(BattleCommand::Run.available(w.battle.no_escape));

    // The formation roll ran; its map-gated arm raised the Ra-Seru bit.
    assert_eq!(
        w.battle.special_word, SPECIAL_RASERU_FORBIDDEN,
        "_DAT_8007BAC0"
    );
    assert_ne!(w.special_battle_word(), 0, "every != 0 reader sees it");

    // Seats: the alternate count-4 row. The round-start recentre moves the
    // whole formation by one offset, so compare each monster against the
    // first one.
    let pc = usize::from(w.party.party_count);
    let pos: Vec<(i32, i32)> = (0..4)
        .map(|k| {
            let m = &w.actors[pc + k].move_state;
            (i32::from(m.world_x), i32::from(m.world_z))
        })
        .collect();
    let (x0, z0) = pos[0];
    let (ax0, az0) = AMBUSH_SEATS[0];
    for (k, (&(x, z), &(ax, az))) in pos.iter().zip(AMBUSH_SEATS.iter()).enumerate() {
        assert_eq!(
            (x - x0, z - z0),
            (ax - ax0, az - az0),
            "monster {k} sits on row 8 (got {pos:?})"
        );
    }
    eprintln!(
        "[ok] town0c 3E FF 03: ids {ids:02X?}, ctx+0x287 0, word {:#x}, seats {pos:?}",
        w.battle.special_word
    );
}

#[test]
fn a_flagged_boss_row_still_refuses_run() {
    let Some(extracted) = gated() else { return };
    let host = enter_scripted(&extracted, "garmel", 9);
    let w = &host.world;
    assert!(
        w.battle.scripted_fight,
        "garmel row 9 carries header byte 1"
    );
    assert_eq!(w.battle_ctx.scripted_fight, 4, "(BD60 >> 5) & 4");
    assert!(w.battle.no_escape);
    assert!(!BattleCommand::Run.available(w.battle.no_escape));
    assert_eq!(
        w.battle.special_word, 0,
        "the roll is skipped and garmel has no map arm"
    );
    eprintln!("[ok] garmel 3E FF 09: flagged, Run refused, word 0");
}
