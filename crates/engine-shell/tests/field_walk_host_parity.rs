//! Disc-gated: the native session and the browser play page walk the player
//! along the **same** path under the same held pad.
//!
//! Movement is a host-drift surface nothing else pins: both hosts call the
//! same locomotion kernel, but the kernel reads a camera azimuth each host
//! publishes (`World::locomotion.camera_azimuth`, from the host's own
//! `Camera::compass_azimuth_units`), plus four locomotion toggles and a
//! story-flag baseline each host arms at scene entry. A host that published a
//! different compass, or armed a different wall footprint, would walk the
//! player somewhere else with every per-host oracle green.
//!
//! So this drives **both hosts** through `town01` with one pad script - hold
//! Up, then hold Right - and compares the player's world X/Z every tick. The
//! native side is configured the way `play-window` configures its session
//! (`window/run.rs`: free-roam baseline, the four locomotion toggles at their
//! CLI defaults, the follow-distance preset, the retail render-yaw bias); the
//! browser side is the exact object `site/js/play-app.js` drives.
//!
//! The two hosts publish the azimuth at different points of their frame (the
//! session before the world tick, the page after it), so the first tick after
//! entry can take one step on the previous compass; the per-tick bound is one
//! walk step, and the rest positions must agree exactly.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::input::PadButton;
use legaia_engine_shell::{BootConfig, BootSession};
use legaia_web_viewer::runtime::LegaiaRuntime;

const SCENE: &str = "town01";
const TICKS: u32 = 300;
/// One field walk step in world units: the per-tick bound on a trajectory
/// whose only phase difference is the azimuth publish point.
const ONE_STEP: i32 = 16;

fn pad_at(t: u32) -> u16 {
    if (20..160).contains(&t) {
        PadButton::Up.mask()
    } else if (160..285).contains(&t) {
        PadButton::Right.mask()
    } else {
        0
    }
}

fn native_path(disc: &str) -> Vec<(i32, i32)> {
    let cfg = BootConfig {
        scene: SCENE.to_string(),
        enable_audio: false,
    };
    let mut s = BootSession::open_disc(std::path::Path::new(disc), &cfg).expect("open disc");
    // `play-window`'s own arming (`window/run.rs`), CLI defaults.
    s.host.world.toggles.use_vm_dialogue = true;
    s.host.world.locomotion.follow_terrain_height = true;
    s.host.world.locomotion.leading_edge_wall_probes = true;
    s.host.world.npcs.solid = true;
    s.host.world.npcs.animate = true;
    s.host.world.seed_free_roam_story_baseline(SCENE);
    s.enter_field_live(
        SCENE,
        &legaia_engine_shell::boot::FieldLiveOpts {
            live_loop: false,
            player_battle: false,
            ..Default::default()
        },
    )
    .expect("enter field live");
    let options = legaia_engine_core::options::OptionsState::default();
    s.camera.distance = options.camera_distance;
    s.camera.render_yaw_bias = legaia_engine_core::camera_view::retail_field_render_yaw_bias();
    let mut path = Vec::new();
    for t in 1..=TICKS {
        s.host.world.set_pad(pad_at(t));
        s.tick().expect("native tick");
        let slot = s.host.world.player_actor_slot.expect("player actor") as usize;
        let m = &s.host.world.actors[slot].move_state;
        path.push((i32::from(m.world_x), i32::from(m.world_z)));
    }
    path
}

fn web_path(disc_bytes: Vec<u8>) -> Vec<(i32, i32)> {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc_bytes, String::new()).expect("load_disc");
    rt.enter_field(SCENE).expect("enter_field");
    let mut path = Vec::new();
    for t in 1..=TICKS {
        rt.set_pad(pad_at(t));
        rt.tick_frame().expect("page tick");
        let p = rt.player_transform();
        path.push((p[0] as i32, p[2] as i32));
    }
    path
}

#[test]
fn both_hosts_walk_the_same_town01_path() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let native = native_path(&disc);
    let web = web_path(std::fs::read(&disc).expect("read disc"));
    assert_eq!(native.len(), web.len());

    let start = native[0];
    let end = *native.last().unwrap();
    assert!(
        (end.0 - start.0).abs() + (end.1 - start.1).abs() > 256,
        "the pad script moved the native player (from {start:?} to {end:?})"
    );
    let mut worst = (0i32, 0u32);
    for (i, (n, w)) in native.iter().zip(&web).enumerate() {
        let d = (n.0 - w.0).abs().max((n.1 - w.1).abs());
        if d > worst.0 {
            worst = (d, i as u32 + 1);
        }
    }
    eprintln!(
        "[walk] native {start:?} -> {end:?}, page -> {:?}, worst per-tick gap {} at tick {}",
        web.last().unwrap(),
        worst.0,
        worst.1
    );
    assert!(
        worst.0 <= ONE_STEP,
        "the hosts' paths part by {} units at tick {} (native {:?}, page {:?})",
        worst.0,
        worst.1,
        native[worst.1 as usize - 1],
        web[worst.1 as usize - 1]
    );
    assert_eq!(
        end,
        *web.last().unwrap(),
        "both hosts come to rest on the same spot"
    );
}
