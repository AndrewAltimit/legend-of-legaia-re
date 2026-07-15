//! Parity oracle: the browser play page's render state vs the native
//! `legaia-engine play-window` field pipeline, over the same [`SceneHost`].
//!
//! The play-window resolves the placed-object layer through
//! `field_env::resolve_placed_env_draws` **with the scene's object binds**
//! (so multi-object props carry the clip that poses them), filters the
//! `FLAG_PLACED` records out of the terrain sweep (they are already drawn -
//! posed - by the placement layer), and draws every MAN partition-1
//! placement that is not parked at the off-map hide box, including the
//! `model >= 0xF0` global-pool specials (save points / party heads). This
//! test drives the web viewer's `build_field_render` + `build_npc_catalog_play`
//! over the host's own resources and asserts each of those invariants, so the
//! browser host draws the same scene content as the native window.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::SceneHost;
use legaia_web_viewer::field_npc::build_npc_catalog_play;
use legaia_web_viewer::play::build_field_render;
use std::env;

/// A town (dense NPC + placement + bind layers), a dungeon (elevation,
/// no-ANM story actors), and a second town (Jeremi) with interiors in-scene.
const SCENES: &[&str] = &["town01", "rikuroa", "geremi"];

#[test]
fn play_render_state_matches_native_field_pipeline() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");

    for &name in SCENES {
        host.enter_field_scene(name, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene({name}): {e:#}"));
        let scene = host.scene.as_ref().expect("scene loaded");
        let res = host.resources.as_ref().expect("resources built");

        // ------------------------------------------------ placement layer
        let f = build_field_render(&host.index, scene, res, false);

        // Native reference: the play-window's exact resolver calls.
        let env_tmds = legaia_engine_core::field_env::env_pack_tmd_indices(scene, res);
        let floor_lut = scene.field_floor_height_lut(&host.index).ok().flatten();
        let placements = scene
            .field_object_placements(&host.index)
            .ok()
            .flatten()
            .unwrap_or_default();
        let binds = scene.field_object_binds(&host.index).ok().flatten();
        let (native_placed, _) = legaia_engine_core::field_env::resolve_placed_env_draws(
            &env_tmds,
            &placements,
            floor_lut,
            binds.as_ref(),
        );
        assert_eq!(
            f.placements, native_placed,
            "{name}: placement draws diverge from the native play-window resolver"
        );
        let with_clip = f.placements.iter().filter(|d| d.anim_id != 0).count();
        eprintln!(
            "{name}: {} placements ({} posed by an object-bind clip)",
            f.placements.len(),
            with_clip
        );
        if name == "town01" {
            assert!(
                with_clip > 0,
                "town01's cupboards/doors carry object-bind clips; the play page \
                 must see nonzero anim ids or every multi-object prop draws as a \
                 heap of parts"
            );
        }

        // Terrain: the native window excludes FLAG_PLACED records (already
        // drawn - posed - by the placement layer).
        let native_terrain: Vec<_> = scene
            .field_terrain_tiles(&host.index)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
            .collect();
        let (native_terrain_draws, _) =
            legaia_engine_core::field_env::resolve_env_draws(&env_tmds, &native_terrain, floor_lut);
        assert_eq!(
            f.terrain, native_terrain_draws,
            "{name}: terrain draws diverge (FLAG_PLACED records must not be \
             double-stamped unposed on top of their placement draw)"
        );

        // ------------------------------------------------------- NPC layer
        let npcs = build_npc_catalog_play(&host.index, name, res, &host.world.global_tmd_pool)
            .unwrap_or_else(|e| panic!("{name}: NPC catalog: {e}"));

        // Native reference: every classified placement draws unless parked at
        // the off-map hide box or its model resolves nowhere.
        let man = scene
            .field_man_payload(&host.index)
            .ok()
            .flatten()
            .expect("scene has a MAN");
        let mf = legaia_asset::man_section::parse(&man).expect("MAN parses");
        let mut native_slots: Vec<usize> = Vec::new();
        for (p, _) in legaia_engine_core::man_field_scripts::classify_placements(&mf, &man) {
            let resolvable = if p.special_model {
                host.world
                    .global_tmd_pool
                    .get((p.model_index - 0xF0) as usize)
                    .is_some_and(|s| s.is_some())
            } else {
                res.tmds.get(p.model_index as usize).is_some()
            };
            if resolvable {
                native_slots.push(p.index);
            }
        }
        let catalog_slots: Vec<usize> = npcs.entries.iter().map(|e| e.placement.index).collect();
        assert_eq!(
            catalog_slots, native_slots,
            "{name}: the play catalog must list every placement the native \
             window draws (specials + clipless multi-object actors included)"
        );
        let specials = npcs.entries.iter().filter(|e| e.special).count();
        let conditionals = npcs.entries.iter().filter(|e| e.conditional).count();
        eprintln!(
            "{name}: {} NPC draws ({} global-pool specials, {} conditional spawns), \
             anm bundle {:?}",
            npcs.entries.len(),
            specials,
            conditionals,
            npcs.anm_prot
        );

        // The scene-ANM bundle the poses come from must be the one the native
        // window resolves (entry-major, descriptor-seed [3,5,6,7] minor).
        let native_bundle = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7].into_iter().find_map(|desc| {
                legaia_asset::player_anm::find_in_entry(&e.bytes, desc)
                    .into_iter()
                    .next()
                    .map(|b| (e.idx, desc, b))
            })
        });
        match (&native_bundle, npcs.anm_prot) {
            (Some((idx, _, _)), Some(prot)) => assert_eq!(
                *idx, prot,
                "{name}: catalog resolves the ANM bundle from a different PROT \
                 entry than the native window"
            ),
            (None, None) => {}
            (a, b) => panic!("{name}: bundle presence diverges (native {a:?} vs play {b:?})"),
        }
    }
}
