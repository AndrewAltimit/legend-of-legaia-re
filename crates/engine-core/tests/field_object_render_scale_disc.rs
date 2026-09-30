//! Placed-object render scale: a bind record's spawn prologue can leave the
//! object's actor at a non-unit `actor[+0x72]`, and retail's per-actor draw
//! (`FUN_8001ADA4` case 5, `ScaleMatrix` at `0x8001B288`) folds it into the
//! model matrix. town01's horizon backdrop - a `17920 x 9600` plane placed at
//! `(3264, 6744)` behind partition-0 record 26 - draws at `0x400` (a quarter;
//! the value a retail `first_town_interactive` capture holds at the actor for
//! that position). At unit scale the plane stands between the plaza camera and
//! the player and paints the whole frame blue.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::field_env;
use legaia_engine_core::scene::SceneHost;

#[test]
fn town01_horizon_backdrop_draws_at_its_prologue_scale() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("town01", 0).expect("enter town01");

    let scales = host.world.object_render_scales();
    assert_eq!(
        scales.get(&26),
        Some(&0x400),
        "P0[26] (the horizon backdrop) must carry its prologue's 0x400 render scale; \
         listed scales = {scales:?}"
    );

    let (scene, res) = (
        host.scene.as_ref().expect("scene"),
        host.resources.as_ref().expect("resources"),
    );
    let env_tmds = field_env::env_pack_tmd_indices(scene, res);
    let placements = scene
        .field_object_placements(&host.index)
        .expect("placements read")
        .expect("town01 has placed objects");
    let binds = scene
        .field_object_binds(&host.index)
        .expect("binds read")
        .expect("town01 has object binds");
    let floor_lut = scene.field_floor_height_lut(&host.index).ok().flatten();
    let (draws, _) =
        field_env::resolve_placed_env_draws(&env_tmds, &placements, floor_lut, Some(&binds));
    let per_draw = field_env::placed_render_scales(&draws, Some(&binds), &scales);
    assert_eq!(per_draw.len(), draws.len());

    let backdrop = draws
        .iter()
        .position(|d| d.world_x == 3264 && d.world_z == 6744)
        .expect("town01 places the horizon backdrop at (3264, 6744)");
    assert_eq!(
        per_draw[backdrop], 0.25,
        "the backdrop draw must scale by 0x400 / 0x1000"
    );
    // Non-vacuity the other way: an ordinary placement stays at unit scale.
    let unit = per_draw.iter().filter(|&&s| s == 1.0).count();
    assert!(
        unit + 1 >= draws.len(),
        "only the backdrop carries a non-unit scale in a cold town01 ({per_draw:?})"
    );
    println!(
        "town01 render scales: backdrop draw {backdrop} at {}, {unit} of {} draws at unit scale",
        per_draw[backdrop],
        draws.len()
    );
}
