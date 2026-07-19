//! Disc-gated smoke test: drive the ENGINE's live effect path
//! (`World::tick_effects` -> `Pool::tick_retail`; `World::active_effect_sprites`
//! -> `Pool::child_billboards`) over the real runtime effect catalog - PROT
//! entry 0873, `data\battle\efect.dat`. The engine-vm sibling
//! (`effect_vm_real_efect.rs`) drives the pool directly; this one asserts the
//! `World` wiring produces the same faithful playback for every real script.
//!
//! Gating: keys on `extracted/PROT/0873_befect_data.BIN` (the same
//! disc-derived-data skip-pass convention as `LEGAIA_DISC_BIN`; this test has
//! no ISO-layer dependency). Skips silently when the file is missing so CI
//! passes without Sony data.

use std::path::PathBuf;

use legaia_engine_core::world::World;
use legaia_engine_vm::effect_vm::EffectCatalog;

fn efect_dat() -> Option<PathBuf> {
    for prefix in ["extracted/PROT", "../../extracted/PROT"] {
        let p = PathBuf::from(prefix).join("0873_befect_data.BIN");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn world_effect_path_plays_every_real_effect_script() {
    let Some(path) = efect_dat() else {
        eprintln!("[skip] extracted/PROT/0873_befect_data.BIN missing; run legaia-extract");
        return;
    };
    let buf = std::fs::read(&path).expect("read efect.dat PROT entry");
    let catalog = EffectCatalog::from_efect_dat_bytes(&buf);
    assert_eq!(catalog.len(), 33, "pack1 effect-script count");

    let mut effects_with_sprites = 0usize;
    for id in 0..catalog.len() as u8 {
        let mut world = World {
            effect_catalog: catalog.clone(),
            ..World::default()
        };
        world.try_spawn_effect(id, [100, -50, 200], 0x300);
        if world.effect_pool.active_count() == 0 {
            continue; // empty script (child_count 0 stays free)
        }

        let mut saw_sprites = false;
        let mut drained = false;
        // Generous bound: master delays + child frame delays are all u8
        // frames; the retail cadence must terminate well within this.
        for _ in 0..200_000 {
            world.tick_effects();
            let sprites = world.active_effect_sprites();
            if !sprites.is_empty() {
                saw_sprites = true;
                for s in &sprites {
                    // Every billboard resolves a real atlas rect through the
                    // pass-2 sizing, with an in-envelope brightness.
                    assert!(
                        s.uv_size[0] > 0 && s.uv_size[1] > 0,
                        "effect {id}: degenerate atlas rect"
                    );
                    assert!(
                        s.size[0] >= 1.0 && s.size[1] >= 1.0,
                        "effect {id}: degenerate billboard {:?}",
                        s.size
                    );
                    assert!(s.brightness <= 0x80, "effect {id}: brightness envelope");
                }
            }
            if world.effect_pool.active_count() == 0 && world.effect_pool.active_child_count() == 0
            {
                drained = true;
                break;
            }
        }
        assert!(drained, "effect {id}: engine playback did not terminate");
        if saw_sprites {
            effects_with_sprites += 1;
        }
    }
    assert!(
        effects_with_sprites > 0,
        "no effect script produced billboards through the engine path"
    );
}
