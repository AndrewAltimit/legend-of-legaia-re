//! Disc-gated: the engine resolves **per-scene inn costs from the disc** -
//! the op-`0x4E` gold-gate + `0x3A` debit pair scanned from the scene MAN at
//! load ([`legaia_asset::inn_costs`], cached as
//! `SceneHost::scene_gold_charges`) - and `MenuRuntime::open_scene_inn`
//! opens the inn prompt with that scanned cost instead of a host-supplied
//! constant.
//!
//! Anchored on `retock` (the 240 G stay - the single paired charge in its
//! script, shared ground truth with `crates/asset/tests/inn_costs_disc.rs`)
//! plus the free-rest baseline `town01` (Rim Elm's bed: no gate + debit pair,
//! so no inn session opens). Skips without `LEGAIA_DISC_BIN` (CLAUDE.md
//! convention).

use legaia_engine_core::menu_runtime::{MenuRuntime, MenuState};
use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

#[test]
fn scene_inn_cost_resolves_from_disc_and_opens_the_prompt() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&path).expect("open disc");

    // `retock`: one scripted charge, the 240 G inn stay.
    host.load_scene("retock").expect("load retock");
    assert_eq!(
        host.scene_inn_cost(),
        Some(240),
        "retock's scripted inn cost is 240 G"
    );
    let charges = host.scene_gold_charges();
    assert_eq!(charges.len(), 1, "retock has a single paired charge");
    assert_eq!(charges[0].sub_op, 3, "inn-class (u16) gate");

    // The production inn-open path: MenuRuntime::open_scene_inn installs the
    // session with the scanned cost and enters InnConfirm.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut runtime = MenuRuntime::new(tmp.path().to_path_buf());
    assert_eq!(runtime.open_scene_inn(&host), Some(240));
    assert_eq!(
        runtime.inn_session.as_ref().map(|s| s.cost),
        Some(240),
        "session carries the scene's scripted cost"
    );
    assert_eq!(runtime.ctx.state, MenuState::InnConfirm.as_byte());

    // Free-rest baseline: Rim Elm's bed charges nothing, so its scene has no
    // gate + debit pair and no inn session opens.
    host.load_scene("town01").expect("load town01");
    assert_eq!(host.scene_inn_cost(), None, "town01 rest is free");
    let mut runtime = MenuRuntime::new(tmp.path().to_path_buf());
    assert_eq!(runtime.open_scene_inn(&host), None);
    assert!(runtime.inn_session.is_none(), "no session installed");
}
