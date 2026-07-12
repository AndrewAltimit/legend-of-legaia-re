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

    // Install a synthetic 32-entry effect catalog so the high-bit (AltEffect)
    // effect-list entries have somewhere to spawn (the real efect.dat / PROT
    // 0873 isn't loaded in this test). Each entry is a 1-child script.
    {
        use legaia_engine_vm::effect_vm::{EffectCatalog, EffectScript};
        let entries: Vec<_> = (0..32)
            .map(|_| {
                (
                    EffectScript {
                        child_count: 1,
                        flags: 0,
                        spread: 0,
                        body: vec![],
                    },
                    vec![],
                )
            })
            .collect();
        world.effect_catalog = EffectCatalog::new(entries);
    }

    // Move id 0x06 (the worked example, record index 3): contact list
    // [0x27, 0x8e, 0x8d], launch list [0x28, 0x64, 0x9d]. The 0x27 / 0x28 bytes
    // are Spawn entries -> 0x801f6324 library-mesh records; the high-bit bytes
    // (0x8e/0x8d/0x9d) are AltEffect entries -> the 2D efect.dat pool; 0x64 is
    // the fixed flash (no pool spawn).
    assert!(
        world.spawn_move_fx(0x06, [0, 0, 0]),
        "move 0x06 has spawnable effect entries"
    );

    // The AltEffect (high-bit) entries spawned through the effect pool: count
    // them off the descriptor and assert the pool got exactly that many.
    let alt_count = {
        use legaia_asset::move_power::EffectListEntry;
        let fx = world
            .move_power
            .as_ref()
            .unwrap()
            .fx_for_move_id(0x06)
            .unwrap();
        fx.contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter(|e| matches!(e.entry, EffectListEntry::AltEffect(_)))
            .count()
    };
    assert!(alt_count > 0, "move 0x06 carries AltEffect entries");
    assert_eq!(
        world.effect_pool.active_count(),
        alt_count,
        "each AltEffect entry spawned one effect-pool master"
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
fn spawnable_move_ids_match_what_actually_renders() {
    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };
    let mut world = world_with_move_power(&bytes);

    let ids = world.move_power.as_ref().unwrap().spawnable_move_ids();

    // The enumeration is non-empty, sorted + unique, and contains the 0x06
    // worked example the debug previewer starts from.
    assert!(!ids.is_empty(), "real overlay has spawnable move FX");
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids sorted + unique");
    assert!(ids.contains(&0x06), "0x06 worked example is spawnable");

    // The query is exactly the set spawn_move_fx can render: every enumerated
    // id stages at least one mesh part.
    for &id in &ids {
        assert!(
            world.spawn_move_fx(id, [0, 0, 0]),
            "spawnable move {id:#04x} renders mesh parts"
        );
        assert!(
            !world.active_move_fx_part_draws().is_empty(),
            "spawnable move {id:#04x} stages parts"
        );
        world.active_move_fx = None;
        world.active_move_fx_trail_texpage = None;
    }
}

#[test]
fn alt_effect_only_moves_still_fire_the_2d_pool() {
    let Some(bytes) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };
    let mut world = world_with_move_power(&bytes);
    {
        use legaia_engine_vm::effect_vm::{EffectCatalog, EffectScript};
        let entries: Vec<_> = (0..128)
            .map(|_| {
                (
                    EffectScript {
                        child_count: 1,
                        flags: 0,
                        spread: 0,
                        body: vec![],
                    },
                    vec![],
                )
            })
            .collect();
        world.effect_catalog = EffectCatalog::new(entries);
    }

    // Scan the real table for moves whose effect lists hold AltEffect entries
    // but no Spawn entry (the alt-only edge case).
    use legaia_asset::move_power::EffectListEntry;
    let alt_only: Vec<(u8, usize)> = {
        let cat = world.move_power.as_ref().unwrap();
        (0x01..=0xFFu8)
            .filter_map(|id| cat.fx_for_move_id(id).map(|fx| (id, fx)))
            .filter_map(|(id, fx)| {
                let entries: Vec<_> = fx
                    .contact_effects
                    .iter()
                    .chain(fx.launch_effects.iter())
                    .map(|e| e.entry)
                    .collect();
                let alts = entries
                    .iter()
                    .filter(|e| matches!(e, EffectListEntry::AltEffect(_)))
                    .count();
                let spawns = entries
                    .iter()
                    .filter(|e| matches!(e, EffectListEntry::Spawn(_)))
                    .count();
                (alts > 0 && spawns == 0).then_some((id, alts))
            })
            .collect()
    };
    let Some(&(id, alt_count)) = alt_only.first() else {
        eprintln!("[skip] no alt-only move in the retail table (edge case is synthetic-only)");
        return;
    };

    // The alt-only move spawns its 2D pool effects (this is the path the old
    // empty-Spawn-set early return used to skip), stages NO 3D scene, and
    // surfaces neither scene-scoped presentation field.
    assert!(
        world.spawn_move_fx(id, [0, 0, 0]),
        "alt-only move {id:#04x} spawns its 2D effects"
    );
    assert_eq!(
        world.effect_pool.active_count(),
        alt_count,
        "each AltEffect entry spawned one effect-pool master"
    );
    assert!(world.active_move_fx_part_draws().is_empty(), "no 3D scene");
    assert_eq!(world.active_move_fx_trail_texpage(), None);
    assert_eq!(world.take_pending_move_fx_cue(), None);
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
