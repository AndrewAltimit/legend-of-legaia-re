//! Disc-gated: a **walk-touch door** re-centres the field window.
//!
//! Most house doors are not scene changes: the interior is a sub-area of the
//! same collision grid, and the door object's record cross-context teleports
//! the player (`0x23 | 0x80` into channel `0xF8`, or the `4C 51` run form).
//! Retail's `0x23` arm, when its target actor is the player context, calls the
//! camera re-centre `FUN_80017EC8(op[0] & 0x7F, op[1] & 0x7F, ..)`
//! (`0x801DEC74..0x801DECA0`), which re-latches the region box at the landing
//! tile and re-plans the windowed static-object list (`FUN_801D7B50`).
//!
//! The engine models that teleport in the walk-touch dispatch
//! (`WalkTouchEvent::PlayerMoveTo`), which must make the same re-centre. When
//! it did not, a door out of a house kept the interior's box, and under retail
//! windowing (the play hosts' default) every window-owned placement outside
//! it stayed hidden: Jeremi's street slabs vanished the moment the player
//! stepped out of the inn, and Rim Elm lost its outdoor placements after a
//! house visit until a scene change (the cave) re-ran the entry install.
//!
//! Asserted per door crossing: the static window's box is the one
//! `FUN_800180EC` latches at the landing tile, and no window-owned placement
//! whose anchor lies in that box is hidden.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_field::field_regions::{self, RegionTable};
use legaia_engine_scene::field_env::{self, PlacedWindowKey};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::scene_staging::placed_draw_live;

fn open_disc() -> Option<std::path::PathBuf> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    let path = std::path::PathBuf::from(disc);
    path.exists().then_some(path)
}

fn player(host: &SceneHost) -> (i16, i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    let ms = &host.world.actors[s].move_state;
    (ms.world_x, ms.world_z)
}

fn seat(host: &mut SceneHost, x: i16, z: i16) {
    let s = host.world.player_actor_slot.expect("player installed") as usize;
    host.world.actors[s].move_state.world_x = x;
    host.world.actors[s].move_state.world_z = z;
}

/// Hold `pad` until the player's position jumps (a door landed), mashing
/// Cross through any record that takes the pad. `None` if nothing fired.
fn walk_until_door(host: &mut SceneHost, pad: u16, frames: u32) -> Option<(i16, i16)> {
    let mut prev = player(host);
    for f in 0..frames {
        let held = if host.world.cutscene_timeline_active() || host.world.dialogue_owns_input() {
            if f.is_multiple_of(2) {
                PadButton::Cross.mask()
            } else {
                0
            }
        } else {
            pad
        };
        host.world.set_pad(held);
        host.tick().expect("tick");
        let p = player(host);
        if (p.0 - prev.0).abs() > 256 || (p.1 - prev.1).abs() > 256 {
            host.world.set_pad(0);
            return Some(p);
        }
        prev = p;
    }
    host.world.set_pad(0);
    None
}

/// The scene's placed draws and their window keys (`None` = bound).
fn placed_keys(host: &SceneHost) -> Vec<(field_env::EnvDraw, Option<PlacedWindowKey>)> {
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
    draws
        .into_iter()
        .map(|d| {
            let k = field_env::placed_window_key(&d, Some(&binds));
            (d, k)
        })
        .collect()
}

/// The re-centre's postcondition at the landing `(x, z)`.
fn assert_recentred(host: &SceneHost, label: &str, landing: (i16, i16)) -> usize {
    let w = &host.world;
    let table = RegionTable::parse(&w.terrain.map_region_block);
    let tile = |v: i16| (i32::from(v) - 0x40) >> 7;
    let (_, attrs) = field_regions::refresh_region_attributes(
        table.as_ref(),
        tile(landing.0),
        tile(landing.1),
        false,
    );
    let sw = &w.terrain.static_window;
    assert_eq!(
        sw.window,
        Some(attrs.box_bytes),
        "{label}: the door landing at {landing:?} must re-latch the region box \
         at its tile (FUN_80017EC8), not keep the box it left"
    );
    assert!(
        sw.retail_windowing,
        "{label}: the probe runs under retail windowing"
    );
    // What the player sees: no window-owned placement near the landing is
    // left hidden by a stale list (the street slabs around a door are).
    let (lx, lz) = (i32::from(landing.0) >> 7, i32::from(landing.1) >> 7);
    let hidden_near: Vec<(i32, i32)> = placed_keys(host)
        .into_iter()
        .filter(|(_, k)| !placed_draw_live(k.as_ref(), sw))
        .map(|(d, _)| (d.world_x, d.world_z))
        .filter(|&(x, z)| ((x >> 7) - lx).abs() <= 6 && ((z >> 7) - lz).abs() <= 6)
        .collect();
    assert!(
        hidden_near.is_empty(),
        "{label}: placements within six tiles of the landing are hidden: {hidden_near:?}"
    );
    sw.spawns.len()
}

/// Jeremi: the picker seats the player in the inn; walking out through its
/// door lands on the street, whose ground slabs are window-owned placements.
#[test]
fn jeremi_inn_exit_relatches_the_street_window() {
    let Some(disc) = open_disc() else { return };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("geremi", 0).expect("enter geremi");
    host.world.terrain.static_window.retail_windowing = true;
    let before = host.world.terrain.static_window.window;
    let landed = walk_until_door(&mut host, PadButton::Down.mask(), 360)
        .expect("geremi: walking out of the inn never crossed its door");
    assert_ne!(
        host.world.terrain.static_window.window, before,
        "geremi: the street is a different sub-area from the inn"
    );
    let spawns = assert_recentred(&host, "geremi inn exit", landed);
    eprintln!("geremi: inn exit landed at {landed:?}, {spawns} window actors");
}

/// Rim Elm: in through Mei's house door and back out. The exit must restore
/// the outdoor window, not leave the interior's.
#[test]
fn rim_elm_house_round_trip_restores_the_outdoor_window() {
    let Some(disc) = open_disc() else { return };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world.terrain.static_window.retail_windowing = true;
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    // Mei's house: the IN door object's contact centre (see
    // `rim_elm_door_roundtrip_disc.rs`), approached from two tiles short.
    let (tx, tz) = (2240, 3728);
    seat(&mut host, tx, tz - 2 * 128);
    let inside = walk_until_door(&mut host, PadButton::Up.mask(), 240)
        .expect("town01: Mei's door never fired");
    assert_recentred(&host, "town01 house entry", inside);
    let interior_box = host.world.terrain.static_window.window;
    let outside = walk_until_door(&mut host, PadButton::Down.mask(), 240)
        .expect("town01: Mei's house exit never fired");
    let spawns = assert_recentred(&host, "town01 house exit", outside);
    assert_ne!(
        host.world.terrain.static_window.window, interior_box,
        "town01: the exit leaves the interior's sub-area"
    );
    eprintln!("town01: house exit landed at {outside:?}, {spawns} window actors");
}
