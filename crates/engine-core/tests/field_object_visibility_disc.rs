//! Story-hidden placed objects: the `.MAP` placement table says where a
//! placed object *can* stand; the object-bind record's spawn prologue says
//! whether it currently *does*. town01's south-gate rock pile (`P0[18..21]`)
//! parks at the off-map hide box until system flag `0x147` sets - a cold
//! entry must not draw it, while the shape-A siblings (`P0[9]/[10]/[15]`,
//! visible early / hidden late) must stay visible.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::SceneHost;

fn open_town01() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = std::path::PathBuf::from(disc);
    if !path.exists() {
        return None;
    }
    let mut host = SceneHost::open_disc(&path).expect("open disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    Some(host)
}

#[test]
fn town01_cold_entry_hides_gate_rocks_and_keeps_early_scenery() {
    let Some(host) = open_town01() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let hidden = host.world.hidden_object_records();
    // Shape B ("hidden early, placed late"): the gate rock pile's four bind
    // records park at the hide box while flag 0x147 is clear.
    for rec in [18usize, 19, 20, 21] {
        assert!(
            hidden.contains(&rec),
            "cold town01 must story-hide P0[{rec}] (gate rocks); hidden set = {hidden:?}"
        );
    }
    // Shape A ("visible early, hidden late"): these prologues skip the park
    // while 0x147 is clear, so they must NOT be hidden on a cold entry.
    for rec in [9usize, 10, 15] {
        assert!(
            !hidden.contains(&rec),
            "cold town01 must keep P0[{rec}] visible; hidden set = {hidden:?}"
        );
    }
}

#[test]
fn town01_hidden_records_drop_placed_draws() {
    use legaia_engine_core::field_env;

    let Some(host) = open_town01() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
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
    let (mut draws, _) =
        field_env::resolve_placed_env_draws(&env_tmds, &placements, floor_lut, Some(&binds));
    let before = draws.len();
    field_env::retain_visible_placed_draws(&mut draws, &binds, &host.world.hidden_object_records());
    assert!(
        draws.len() < before,
        "the story-hidden gate rocks must remove at least one placed draw \
         ({before} before, {} after)",
        draws.len()
    );
    // Non-vacuity the other way: the filter must not empty the scene.
    assert!(
        !draws.is_empty(),
        "town01 keeps its visible placed objects after the story filter"
    );
}
