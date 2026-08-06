//! The effect-script walk's table-form SFX lane, against the real overlay:
//! draining `World::battle_effect_spawns` fires each qualifying table-form
//! spawn's `0x801F6418` byte into `World::battle_sfx_cues` - the engine seat
//! of retail `FUN_801DEA50`'s SFX arm (`0x801df0d4..0x801df134`), whose two
//! gates the tests pin: only plain codes below `0x32` consult the map, and
//! only a non-zero map byte fires (`FUN_80058490`'s `0x1DC` packet).
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`; the no-catalog
//! degradation test at the bottom runs disc-free.

use std::path::PathBuf;

use legaia_engine_core::action_effect_script::TABLE_SFX_GATE;
use legaia_engine_core::battle_events::BattleEffectSpawn;
use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::World;
use legaia_prot::archive::Archive;

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn overlay_0898(dir: &std::path::Path) -> Vec<u8> {
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read 0898");
    bytes
}

fn spawn(effect: u8, direct: bool) -> BattleEffectSpawn {
    BattleEffectSpawn {
        actor_slot: 1,
        effect,
        direct,
        at: (100, -20, 300),
        facing: 0x200,
    }
}

#[test]
fn draining_table_spawns_fires_the_real_sfx_map_bytes_under_the_retail_gates() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let overlay = overlay_0898(&dir);
    let mut world = World::new();
    world.move_power = Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("catalog"));
    let aux = world
        .move_power
        .as_ref()
        .unwrap()
        .aux_tables()
        .cloned()
        .expect("aux tables off the real overlay");

    // Pick the fixture ids from the disc's own table so the test tracks the
    // bytes rather than hardcoding them: one sounded id and one silent id,
    // both inside the gate.
    let sounded = (0..TABLE_SFX_GATE)
        .find(|&id| aux.effect_sfx(id).is_some_and(|b| b != 0))
        .expect("the retail SFX map has at least one sounded id below 0x32");
    let silent = (0..TABLE_SFX_GATE)
        .find(|&id| aux.effect_sfx(id) == Some(0))
        .expect("the retail SFX map has at least one silent id below 0x32");
    let expected_kind = u16::from(aux.effect_sfx(sounded).unwrap());

    // Queue: a sounded table spawn, a silent table spawn, a table spawn at
    // the gate (>= 0x32 never consults the map - the arm that makes the
    // spreadsheet's 0x4C cue silent), and a direct-form spawn (2D pool,
    // whose sound rides the pool path, not this map).
    world.battle_effect_spawns = vec![
        spawn(sounded, false),
        spawn(silent, false),
        spawn(TABLE_SFX_GATE, false),
        spawn(sounded, true),
    ];
    world.battle_sfx_cues.clear();

    let drained = world.drain_battle_effect_spawns();
    assert_eq!(drained.len(), 4, "the drain still returns every spawn");
    assert!(world.battle_effect_spawns.is_empty());

    let kinds: Vec<u16> = world.battle_sfx_cues.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![expected_kind],
        "exactly the sounded in-gate table spawn fires, with the map's byte"
    );
    let cue = &world.battle_sfx_cues[0];
    assert_eq!(cue.actor_slot, 1, "seated on the acting actor");
    assert_eq!(cue.target_slot, 1);
    assert_eq!(cue.timing_frames, 0, "fires at spawn time");
}

#[test]
fn draining_without_a_catalog_returns_the_spawns_and_stays_silent() {
    // Disc-free degradation: no move-power catalog means no SFX map, so the
    // drain hands back the spawns and queues nothing - the same shape as the
    // table spawner itself, which stages nothing without the overlay.
    let mut world = World::new();
    world.battle_effect_spawns = vec![spawn(0x01, false), spawn(0x01, true)];
    world.battle_sfx_cues.clear();
    let drained = world.drain_battle_effect_spawns();
    assert_eq!(drained.len(), 2);
    assert!(world.battle_sfx_cues.is_empty());
}
