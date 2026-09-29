//! Disc-gated: the **windowed static-object list** - retail's sub-area window
//! sweep `FUN_801D7B50`, run live from the scene-entry camera-window install
//! (`FUN_80017DD4`) and every mid-scene re-centre (`FUN_80017EC8`).
//!
//! What this pins, per scene:
//!
//! 1. **The sweep runs on entry.** `SceneHost::enter_field_scene` leaves a list
//!    planned over the region box latched at the seat tile, and every actor on
//!    it sits inside that box on an anchor tile without `CELL_BIND_OWNED`.
//! 2. **The two sweeps partition the drawn placements.** Every placed draw the
//!    shared resolver produces is either bound (the init sweep's actor, alive
//!    for the whole scene) or a window-sweep actor over the full grid - never
//!    both, never neither. So the whole-map draw is exactly the union of the
//!    two sweeps' actors.
//! 3. **Retail windowing only ever hides the window sweep's placements** that
//!    the current list does not hold, and re-centring on such a placement's
//!    tile brings it back - the sub-area pop-in, and nothing else. With the
//!    option off (the default) nothing is hidden.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use std::collections::HashSet;

use legaia_engine_core::field_env::{self, PlacedWindowKey};
use legaia_engine_core::field_regions::{self, CELL_BIND_OWNED, RegionTable};
use legaia_engine_core::scene::SceneHost;

/// Rim Elm (both variants), a casino town, and three scenes whose region boxes
/// leave some window-sweep placements outside every sub-area box.
const SCENES: &[&str] = &["town01", "town0c", "koin3", "retona", "vell", "geremi"];

fn open_disc() -> Option<std::path::PathBuf> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = std::path::PathBuf::from(disc);
    path.exists().then_some(path)
}

struct Entered {
    host: SceneHost,
    draws: Vec<field_env::EnvDraw>,
    keys: Vec<Option<PlacedWindowKey>>,
    /// Every window-sweep actor over the full grid, from the scene's `.MAP`
    /// after the load-time grid-mark refresh.
    full_grid: Vec<field_regions::WindowSpawn>,
}

fn enter(disc: &std::path::Path, name: &str) -> Entered {
    let mut host = SceneHost::open_disc(disc).expect("open disc");
    host.enter_field_scene(name, 0).expect("enter scene");
    let scene = host.scene.as_ref().expect("scene");
    let res = host.resources.as_ref().expect("resources");
    let env = field_env::env_pack_tmd_indices(scene, res);
    let placements = scene
        .field_object_placements(&host.index)
        .expect("placements read")
        .expect("field map");
    let binds = scene
        .field_object_binds(&host.index)
        .expect("binds read")
        .expect("field map + man");
    let lut = scene.field_floor_height_lut(&host.index).ok().flatten();
    let (draws, _) = field_env::resolve_placed_env_draws(&env, &placements, lut, Some(&binds));
    let keys = draws
        .iter()
        .map(|d| field_env::placed_window_key(d, Some(&binds)))
        .collect();
    let idx = scene.field_map_index(&host.index).expect("field map index");
    let mut map = host.index.entry_bytes_extended(idx).expect("map bytes");
    field_regions::refresh_object_grid_marks(&mut map);
    let full_grid = field_regions::window_rebuild_spawns(
        &map,
        (0, 0, 0x80, 0x80),
        host.world.terrain.floor_height_lut,
    );
    Entered {
        host,
        draws,
        keys,
        full_grid,
    }
}

#[test]
fn scene_entry_plans_the_window_list_over_the_seat_box() {
    let Some(disc) = open_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for name in SCENES {
        let e = enter(&disc, name);
        let w = &e.host.world;
        let sw = &w.terrain.static_window;
        let window = sw
            .window
            .unwrap_or_else(|| panic!("{name}: no window planned"));
        assert!(
            !sw.descriptors.is_empty(),
            "{name}: descriptors not resident"
        );
        // The box is the one `FUN_800180EC` latches at the seat tile.
        let slot = w.player_actor_slot.expect("player") as usize;
        let ms = &w.actors[slot].move_state;
        let table = RegionTable::parse(&w.terrain.map_region_block);
        let (_, attrs) = field_regions::refresh_region_attributes(
            table.as_ref(),
            i32::from(ms.world_x) >> 7,
            i32::from(ms.world_z) >> 7,
            false,
        );
        assert_eq!(window, attrs.box_bytes, "{name}: box latched off the seat");
        for s in &sw.spawns {
            assert!(
                (window[0]..window[2]).contains(&s.tile.0)
                    && (window[1]..window[3]).contains(&s.tile.1),
                "{name}: {s:?} outside the box {window:?}"
            );
            let cell =
                w.terrain.object_cells[usize::from(s.anchor.0) + usize::from(s.anchor.1) * 0x80];
            assert_eq!(
                cell & CELL_BIND_OWNED,
                0,
                "{name}: {s:?} on a bind-owned anchor"
            );
        }
        assert_eq!(sw.spawn_count as usize, sw.spawns.len());
        eprintln!(
            "{name}: seat box {window:?} -> {} window actors ({} over the full grid)",
            sw.spawns.len(),
            e.full_grid.len()
        );
    }
}

#[test]
fn the_two_sweeps_partition_every_drawn_placement() {
    let Some(disc) = open_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for name in SCENES {
        let e = enter(&disc, name);
        let spawn_keys: HashSet<PlacedWindowKey> =
            e.full_grid.iter().map(PlacedWindowKey::of_spawn).collect();
        let (mut bound, mut windowed) = (0usize, 0usize);
        for (d, k) in e.draws.iter().zip(&e.keys) {
            let own = PlacedWindowKey::of_draw(d);
            match k {
                // Bound: the init sweep's actor, which the window sweep skips.
                None => {
                    bound += 1;
                    assert!(
                        !spawn_keys.contains(&own),
                        "{name}: bound draw {own:?} is also a window actor"
                    );
                }
                // Unbound: exactly one window-sweep actor over the full grid.
                Some(k) => {
                    windowed += 1;
                    assert!(
                        spawn_keys.contains(k),
                        "{name}: unbound draw {k:?} has no window actor - neither sweep spawns it"
                    );
                }
            }
        }
        assert_eq!(bound + windowed, e.draws.len());
        assert!(bound > 0, "{name}: no bound draws - partition vacuous");
        eprintln!("{name}: {bound} init-sweep draws + {windowed} window-sweep draws");
    }
}

#[test]
fn retail_windowing_hides_only_window_placements_the_list_lacks() {
    let Some(disc) = open_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut any_hidden = false;
    for name in SCENES {
        let mut e = enter(&disc, name);
        // Default: the whole map draws, list or no list.
        let sw = &e.host.world.terrain.static_window;
        assert!(
            !sw.retail_windowing,
            "{name}: retail windowing on by default"
        );
        assert!(
            e.keys
                .iter()
                .all(|k| field_env::placed_draw_live(k.as_ref(), sw)),
            "{name}: a draw is hidden with windowing off"
        );
        // Retail: only window-sweep placements the list does not hold drop.
        let opts = legaia_engine_core::options::OptionsState {
            retail_static_window: true,
            ..Default::default()
        };
        opts.apply_to_world(&mut e.host.world);
        let sw = &e.host.world.terrain.static_window;
        let hidden: Vec<PlacedWindowKey> = e
            .keys
            .iter()
            .filter(|k| !field_env::placed_draw_live(k.as_ref(), sw))
            .map(|k| k.expect("a bound draw was hidden"))
            .collect();
        for k in &hidden {
            assert!(!sw.draws(k), "{name}: {k:?} hidden but on the list");
        }
        eprintln!(
            "{name}: retail windowing hides {} of {} placed draws at the seat",
            hidden.len(),
            e.draws.len()
        );
        // Re-centring on a hidden placement's own tile brings it in, unless
        // its descriptor never draws (draw kind 0).
        if let Some(k) = hidden.first() {
            any_hidden = true;
            let spawn = e
                .full_grid
                .iter()
                .find(|s| PlacedWindowKey::of_spawn(s) == *k)
                .copied()
                .expect("hidden draw has a window actor");
            e.host
                .world
                .recentre_field_window(i32::from(spawn.tile.0), i32::from(spawn.tile.1));
            let sw = &e.host.world.terrain.static_window;
            assert_eq!(
                field_env::placed_draw_live(Some(k), sw),
                spawn.drawn(),
                "{name}: re-centre on {:?} (box {:?})",
                spawn.tile,
                sw.window
            );
        }
    }
    assert!(
        any_hidden,
        "no scene hid anything - the retail path is vacuous"
    );
}
