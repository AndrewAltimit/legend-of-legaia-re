//! Disc-gated: a pack map entered over a running retail scene is the same
//! map as one entered cold.
//!
//! The page always has a scene running when its pack library plays a map,
//! and a native window reaches the same entry through a pack entrance on a
//! retail scene or a pack reload; the native `--map` launch enters cold. A
//! pack map's base scene carries no MAN, so nothing replaced the previous
//! scene's MAN-installed state: Rim Elm's (`town01`) ambient wanderers kept
//! walking on the pack map, publishing positions for placement slots the map
//! does not have, and their `rand()` draws parted the run. Found by the
//! `pack_*` scenes of `scripts/ci/host-pair.mjs`, where the window enters
//! cold and the page over town01 (12% of every retail-mode frame off).
//! Skips when `LEGAIA_DISC_BIN` is unset.

use crate::runtime::LegaiaRuntime;

fn sample_pack_bundle() -> Vec<u8> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/sample");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    legaia_expansion_format::bundle::write_bundle(&files).expect("bundle")
}

fn runtime(disc: &std::ffi::OsStr) -> LegaiaRuntime {
    let mut rt = LegaiaRuntime::new();
    assert!(
        rt.load_disc(std::fs::read(disc).expect("read the disc"), String::new())
            .is_ok(),
        "load the disc"
    );
    rt
}

/// The trace lines that describe the map's world, leaving out what the
/// warm run legitimately carries from Rim Elm: how long the runtime has run
/// (frame counters, the seat-edge count), the story flags Rim Elm's scripts
/// set, the audio director's command record (it stopped Rim Elm's BGM),
/// and the bindings digest, whose lead walk-clip cursor carries across every
/// scene entry. The NPC clip cursors that digest also folds are compared
/// directly below.
fn world_lines(trace: &str) -> Vec<&str> {
    let mut in_audio = false;
    trace
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            // The `audio:` channel (the director's command count and digest)
            // runs to the `switches:` line.
            if t.starts_with("audio:") {
                in_audio = true;
            } else if t.starts_with("switches:") {
                in_audio = false;
            }
            !(in_audio
                || t.starts_with("frames:")
                || t.starts_with("world_frame:")
                || t.starts_with("seat_edges:")
                || t.starts_with("flags:")
                || t.starts_with("bindings:")
                || t.starts_with("bindings_parts:")
                || t.starts_with("audio_recent:"))
        })
        .collect()
}

#[test]
fn a_pack_map_entered_over_a_scene_matches_a_cold_entry() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cold = runtime(&disc);
    let mut warm = runtime(&disc);
    warm.enter_field("town01").expect("enter Rim Elm");
    for _ in 0..60 {
        warm.frame_ticks(1, 0, 0).expect("tick Rim Elm");
    }
    assert!(
        !warm
            .scene_host
            .host_mut()
            .expect("host")
            .world
            .npcs
            .ambient
            .is_empty(),
        "Rim Elm's wanderers run before the pack map"
    );
    for rt in [&mut cold, &mut warm] {
        let v: serde_json::Value =
            serde_json::from_str(&rt.load_expansion_core(&sample_pack_bundle())).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert!(rt.debug_seed_rng(0x1234_5678));
        rt.enter_expansion_map_core("isles_village", "")
            .expect("enter the pack map");
        assert!(rt.debug_seed_rng(0x1234_5678));
    }
    let down = legaia_engine_system::input::PadButton::Down.mask();
    for n in 0..240u32 {
        let pad = if (30..=110).contains(&n) { down } else { 0 };
        let edge = if n == 30 { down } else { 0 };
        cold.frame_ticks(1, edge, pad).expect("tick cold");
        warm.frame_ticks(1, edge, pad).expect("tick warm");
        if n % 20 == 19 {
            let (a, b) = (cold.debug_host_trace_text(), warm.debug_host_trace_text());
            let (la, lb) = (world_lines(&a), world_lines(&b));
            let first = la.iter().zip(&lb).position(|(x, y)| x != y);
            let clips = |rt: &mut LegaiaRuntime| -> Vec<u8> {
                let w = &rt.scene_host.host_mut().expect("host").world;
                w.npcs.clip_cursors.keys().copied().collect()
            };
            assert_eq!(
                clips(&mut cold),
                clips(&mut warm),
                "tick {n}: NPC clip cursors"
            );
            assert!(
                la.len() == lb.len() && first.is_none(),
                "tick {n}: the map entered over town01 parts from the cold entry at line {first:?}: \
                 {:?} | {:?}",
                first.map(|i| la[i]),
                first.map(|i| lb[i]),
            );
        }
    }
}
