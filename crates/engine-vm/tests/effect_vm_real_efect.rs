//! Disc-gated smoke test: run the faithful effect-VM walker
//! (`Pool::tick_retail` / `Pool::child_billboards`) over the real runtime
//! effect catalog - PROT entry 0873, `data\battle\efect.dat` (the 2-pack
//! wrapper; see `docs/formats/effect.md#runtime-effect-format---2-pack-wrapper`).
//!
//! Every effect id in pack1 is spawned into a fresh pool and ticked to
//! exhaustion; the test asserts the retail cadence terminates, spawns
//! children through the pack0 batches, and that every pass-2 billboard
//! resolves an in-range atlas entry with an in-envelope brightness.
//!
//! Gating: keys on `extracted/PROT/0873_befect_data.BIN` (produced by
//! `legaia-extract` from a user-supplied disc; the same disc-derived-data
//! skip-pass convention as `LEGAIA_DISC_BIN` - this crate has no ISO-layer
//! dependency, so it consumes the extracted entry rather than the `.bin`).
//! Skips silently when the file is missing so CI passes without Sony data.

use std::path::PathBuf;

use legaia_engine_vm::effect_vm::{EffectCatalog, EffectHost, Pool};

fn efect_dat() -> Option<PathBuf> {
    for prefix in ["extracted/PROT", "../../extracted/PROT"] {
        let p = PathBuf::from(prefix).join("0873_befect_data.BIN");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Deterministic LCG host - only `next_random` is consumed by the faithful
/// walker (the mirror bits + the spawn-offset rewrite).
struct LcgHost(u32);

impl EffectHost for LcgHost {
    fn next_random(&mut self) -> i32 {
        // Any LCG works; the walker only takes `% 4` / `% (2*spread)`.
        self.0 = self.0.wrapping_mul(0x0001_9660).wrapping_add(0x3C6E_F35F);
        (self.0 >> 1) as i32
    }
}

#[test]
fn retail_walker_drains_every_real_effect_script() {
    let Some(path) = efect_dat() else {
        eprintln!("[skip] extracted/PROT/0873_befect_data.BIN missing; run legaia-extract");
        return;
    };
    let buf = std::fs::read(&path).expect("read efect.dat PROT entry");
    let catalog = EffectCatalog::from_efect_dat_bytes(&buf);
    // Stable disc invariants (docs/formats/effect.md): 33 pack1 scripts,
    // 14 pack0 batches.
    assert_eq!(catalog.len(), 33, "pack1 effect-script count");
    assert_eq!(catalog.anim_count(), 14, "pack0 anim-batch count");
    assert!(!catalog.atlas().is_empty(), "inline sprite atlas present");

    let mut effects_with_children = 0usize;
    for id in 0..catalog.len() as u8 {
        let mut pool = Pool::new();
        let mut host = LcgHost(0x1234_5678 ^ id as u32);
        let Some(slot) = pool.spawn_by_ui_id(&mut host, id, [100, -50, 200], 0x300, &catalog)
        else {
            continue; // empty script (child_count 0 allocates but stays free)
        };
        let _ = slot;

        let mut saw_children = false;
        let mut drained = false;
        // Generous bound: master delays + child frame delays are all u8
        // frames; the whole cadence must terminate well within this.
        for _ in 0..200_000 {
            pool.tick_retail(&mut host, &catalog, 1);
            if pool.active_child_count() > 0 {
                saw_children = true;
                // Every live child's billboard must resolve: in-range atlas
                // index, brightness within the envelope, sane quad size.
                for b in pool.child_billboards(&catalog) {
                    assert!(
                        (b.atlas_index as usize) < catalog.atlas().len(),
                        "effect {id}: atlas index {} out of range",
                        b.atlas_index
                    );
                    assert!(b.brightness <= 0x80, "effect {id}: brightness envelope");
                    assert!(
                        b.world_w > 0 && b.world_h > 0,
                        "effect {id}: degenerate billboard {}x{}",
                        b.world_w,
                        b.world_h
                    );
                }
            }
            if pool.active_count() == 0 && pool.active_child_count() == 0 {
                drained = true;
                break;
            }
        }
        assert!(drained, "effect {id}: cadence did not terminate");
        if saw_children {
            effects_with_children += 1;
        }
    }
    assert!(
        effects_with_children > 0,
        "no effect script spawned any child through the real pack0 batches"
    );
}
