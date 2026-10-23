//! Disc-gated check that the bonus equipment drop's engine pool
//! (`World::bonus_drop_pool`, built from the executable `World` holds) is the
//! pool the patcher's `--equipment-drops` embeds
//! (`legaia_patcher::equipment::equipment_ids`). Both run the one join in
//! `legaia-gamedata`; this pins that the engine feeds it the same names.
//!
//! Skips without `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_disc_patch::disc::DiscPatcher;
use legaia_engine_core::world::World;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn engine_bonus_pool_is_the_patchers() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = DiscPatcher::open(disc)
        .expect("open disc")
        .read_named_file("SCUS_942.54")
        .expect("SCUS present on disc");
    let mut world = World::default();
    world.tables.scus = Some(std::sync::Arc::from(scus.clone()));
    let engine = world.bonus_drop_pool();
    let patcher = legaia_patcher::equipment::equipment_ids(&scus).expect("patcher pool");
    eprintln!("bonus pool: {} ids", engine.len());
    assert!(engine.len() > 50, "the disc carries a full equipment pool");
    assert_eq!(engine, patcher);
}
