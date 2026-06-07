//! Disc-gated: a battle move's effect-list `Spawn` entries render through the
//! move-FX scene-graph path ([`World::spawn_move_fx`]).
//!
//! Drives the full chain on real PROT 0898 bytes: install the move-power catalog
//! plus the retained overlay onto a `World`, spawn a known move's effect FX, and
//! assert it stages summon-format move-VM part records whose mesh draws resolve
//! into the PROT 0871 effect-model-library window `global_tmd_pool[3..=32]` (the
//! captured `gp[0x754] = 3` base). Ticks the scene to confirm the move VM drives
//! the parts without an unimplemented opcode. Skips without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::World;
use legaia_prot::archive::Archive;

const EFFECT_MODEL_LIBRARY_BASE: usize = 3;
const EFFECT_MODEL_LIBRARY_COUNT: usize = 30;

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if !prot.is_file() {
            continue;
        }
        let mut archive = Archive::open(&prot).ok()?;
        let entry = archive
            .entries
            .get(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
            .cloned()?;
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

fn world_with_move_power(bytes: &[u8]) -> World {
    let mut world = World::new();
    world.move_power = Some(MovePowerCatalog::from_overlay_0898(bytes).expect("catalog parses"));
    world.move_power_overlay = Some(Arc::from(bytes));
    world
}

#[test]
fn move_fx_spawns_library_mesh_parts_from_real_overlay() {
    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };
    let mut world = world_with_move_power(&bytes);

    // Move id 0x06 (the worked example, record index 3): contact list
    // [0x27, 0x8e, 0x8d], launch list [0x28, 0x64, 0x9d]. The 0x27 / 0x28 bytes
    // are Spawn entries -> 0x801f6324 library-mesh records; the high-bit and
    // FixedFlash bytes don't spawn through this path.
    assert!(
        world.spawn_move_fx(0x06, [0, 0, 0]),
        "move 0x06 has spawnable effect entries"
    );

    let draws = world.active_move_fx_part_draws();
    assert!(
        !draws.is_empty(),
        "the move's Spawn records stage at least one mesh part"
    );
    // Every mesh part resolves into the effect-model-library window
    // global_tmd_pool[3..=32] (model_sel + base, base = 3).
    let lib = EFFECT_MODEL_LIBRARY_BASE..(EFFECT_MODEL_LIBRARY_BASE + EFFECT_MODEL_LIBRARY_COUNT);
    for d in &draws {
        assert!(
            lib.contains(&d.model_index),
            "mesh part resolves into the effect-model library (idx {})",
            d.model_index
        );
    }

    // Presentation fields surface: the trail texpage (0x7700 + record +0x0b) and
    // the sound cue (record +0x0d) match the move's resolved descriptor.
    let fx = world
        .move_power
        .as_ref()
        .unwrap()
        .fx_for_move_id(0x06)
        .expect("move 0x06 resolves an FX descriptor");
    assert_eq!(
        world.active_move_fx_trail_texpage(),
        Some(fx.trail_texpage),
        "spawn surfaces the move's trail texpage"
    );
    assert!(
        fx.trail_texpage >= 0x7700,
        "trail texpage is the 0x7700-based GP0 word"
    );
    // The pending sound cue matches the record's +0x0d (drained once).
    if fx.sound_cue_id != 0 {
        assert_eq!(world.take_pending_move_fx_cue(), Some(fx.sound_cue_id));
        assert_eq!(world.take_pending_move_fx_cue(), None, "drained once");
    } else {
        assert_eq!(world.take_pending_move_fx_cue(), None);
    }

    // The move VM drives the parts for enough frames to drain the scene; once it
    // does, the trail texpage clears.
    for _ in 0..600 {
        world.tick_move_fx(0x100);
        if world.active_move_fx.is_none() {
            break;
        }
    }
    if world.active_move_fx.is_none() {
        assert_eq!(
            world.active_move_fx_trail_texpage(),
            None,
            "trail clears when the scene drains"
        );
    }
}

#[test]
fn move_fx_guards_when_uninstalled_or_inert() {
    // No catalog / overlay installed -> no spawn, no panic.
    let mut bare = World::new();
    assert!(!bare.spawn_move_fx(0x06, [0, 0, 0]));
    assert!(bare.active_move_fx_part_draws().is_empty());
    assert_eq!(bare.active_move_fx_trail_texpage(), None);
    assert_eq!(bare.take_pending_move_fx_cue(), None);
    bare.tick_move_fx(0x100); // no-op

    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (guard-only checks already ran)");
        return;
    };
    let mut world = world_with_move_power(&bytes);
    // An unmapped move id (basic-attack band) has no power record -> no spawn.
    assert!(!world.spawn_move_fx(0x0F, [0, 0, 0]));
}
