//! Verify the browser **play** page's render state builds off a live
//! [`SceneHost`] - the pipeline `site/play.html` runs.
//!
//! The play page differs from the static full-map view in where its assets come
//! from: it does not build its own [`SceneResources`], it reads the ones the
//! running host already built at `enter_field_scene`. This test drives that
//! exact path (host -> `build_field_render` + `build_npc_catalog_res`) and
//! asserts the scene the engine is simulating is also the scene the page can
//! draw: an environment pack, placements, a floor, and the MAN's NPCs resolved
//! against the host's own TMD pool.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::SceneHost;
use legaia_web_viewer::field_npc::build_npc_catalog_res;
use legaia_web_viewer::play::build_field_render;
use std::env;

/// A town (dense NPC + placement layers) and a dungeon (elevation + terrain).
const SCENES: &[&str] = &["town01", "rikuroa"];

#[test]
fn play_render_state_builds_from_the_running_host() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");

    for &name in SCENES {
        host.enter_field_scene(name, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene({name}): {e:#}"));

        let scene = host.scene.as_ref().expect("scene loaded");
        let res = host
            .resources
            .as_ref()
            .expect("enter_field_scene builds the scene resources the page draws");

        let f = build_field_render(&host.index, scene, res, false);
        assert!(
            !f.env_tmds.is_empty(),
            "{name}: no environment mesh pack in the host's resources"
        );
        assert!(
            !f.placements.is_empty() && !f.terrain.is_empty(),
            "{name}: placements={} terrain={}",
            f.placements.len(),
            f.terrain.len()
        );
        assert!(
            f.ground.as_ref().is_some_and(|g| g.quad_count() > 0),
            "{name}: no walk-ground heightfield"
        );
        // Every draw indexes the env pack, which is what the page's mesh slot
        // space assumes.
        for d in f.placements.iter().chain(f.terrain.iter()) {
            assert!(
                d.env_slot < f.env_tmds.len(),
                "{name}: draw references env slot {} of {}",
                d.env_slot,
                f.env_tmds.len()
            );
        }

        let npcs = build_npc_catalog_res(&host.index, name, res)
            .unwrap_or_else(|e| panic!("{name}: NPC catalog: {e}"));
        assert!(
            !npcs.entries.is_empty(),
            "{name}: MAN places no drawable actors"
        );
        // Each catalogued actor's model must resolve in the host's TMD pool -
        // the page builds its mesh from exactly that index.
        for e in &npcs.entries {
            assert!(
                res.tmds.get(e.placement.model_index as usize).is_some(),
                "{name}: NPC slot {} names model {} outside the pool ({} TMDs)",
                e.placement.index,
                e.placement.model_index,
                res.tmds.len()
            );
        }

        // The lead's field mesh comes from the global TMD pool that scene entry
        // seeds; without it the page has no player to draw.
        assert!(
            host.world
                .global_tmd_pool
                .first()
                .is_some_and(|s| s.is_some()),
            "{name}: scene entry seeded no lead field mesh"
        );
    }
}
