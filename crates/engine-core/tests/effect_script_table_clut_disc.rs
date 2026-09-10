//! The effect-script walk's table-form **CLUT-stage** lane, against the real
//! overlay: draining `World::battle_effect_spawns` queues each qualifying
//! table-form spawn's `0x801F6418` byte onto `World::battle_clut_stages` -
//! the engine seat of retail `FUN_801DEA50`'s palette arm
//! (`0x801df0d4..0x801df134`), whose two gates the tests pin: only plain
//! codes below `0x32` consult the map, and only a non-zero map byte copies.
//!
//! The map is a CLUT source-x table, not a sound table: the byte is `rect.x`
//! of the 16x1 `RECT` `FUN_80058490` (`MoveImage`) blits onto `(224, 476)`.
//! This file previously asserted the byte reaching `World::battle_sfx_cues`
//! as a sound-cue id - it was asserting the defect.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`; the no-catalog
//! degradation test at the bottom runs disc-free.

use std::path::PathBuf;

use legaia_engine_core::action_effect_script::TABLE_CLUT_GATE;
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
fn draining_table_spawns_stages_the_real_clut_map_bytes_under_the_retail_gates() {
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
    // bytes rather than hardcoding them: one staging id and one no-copy id,
    // both inside the gate.
    let staged = (0..TABLE_CLUT_GATE)
        .find(|&id| aux.effect_clut_x(id).is_some_and(|b| b != 0))
        .expect("the retail CLUT map has at least one staging id below 0x32");
    let no_copy = (0..TABLE_CLUT_GATE)
        .find(|&id| aux.effect_clut_x(id) == Some(0))
        .expect("the retail CLUT map has at least one zero id below 0x32");
    let expected_x = aux.effect_clut_x(staged).unwrap();
    // Every live map byte is a plausible VRAM column, not a cue id.
    assert_eq!(expected_x % 16, 0, "a CLUT source x is 16-pixel aligned");

    // Queue: a staging table spawn, a no-copy table spawn, a table spawn at
    // the gate (>= 0x32 never consults the map - the arm that makes the
    // spreadsheet's 0x4C code copy nothing), and a direct-form spawn (2D
    // pool, which has no palette arm at all).
    world.battle_effect_spawns = vec![
        spawn(staged, false),
        spawn(no_copy, false),
        spawn(TABLE_CLUT_GATE, false),
        spawn(staged, true),
    ];
    world.battle_clut_stages.clear();
    world.battle_sfx_cues.clear();

    let drained = world.drain_battle_effect_spawns();
    assert_eq!(drained.len(), 4, "the drain still returns every spawn");
    assert!(world.battle_effect_spawns.is_empty());

    assert_eq!(
        world.drain_battle_clut_stages(),
        vec![expected_x],
        "exactly the in-gate staging table spawn queues, with the map's byte"
    );
    assert!(
        world.battle_sfx_cues.is_empty(),
        "the palette arm submits no sound - `FUN_80058490` is MoveImage"
    );

    // And the stage itself is a 16x1 row move onto column 224 of row 476.
    let mut vram = legaia_tim::Vram::new();
    let src: Vec<u8> = (0..32).collect();
    vram.write_clut_row(u16::from(expected_x), 476, &src);
    assert!(legaia_engine_core::battle_effect_clut::stage_effect_clut(
        &mut vram, expected_x
    ));
    assert_eq!(vram.pixel(224, 476), u16::from_le_bytes([src[0], src[1]]));
}

#[test]
fn draining_without_a_catalog_returns_the_spawns_and_stages_nothing() {
    // Disc-free degradation: no move-power catalog means no CLUT map, so the
    // drain hands back the spawns and queues nothing - the same shape as the
    // table spawner itself, which stages nothing without the overlay.
    let mut world = World::new();
    world.battle_effect_spawns = vec![spawn(0x01, false), spawn(0x01, true)];
    world.battle_clut_stages.clear();
    let drained = world.drain_battle_effect_spawns();
    assert_eq!(drained.len(), 2);
    assert!(world.battle_clut_stages.is_empty());
}
