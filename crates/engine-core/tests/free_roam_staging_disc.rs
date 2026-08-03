//! Free-roam picker staging (`World::seed_free_roam_story_baseline`):
//!
//! - **BGM**: town01's entry script starts the town theme (global id 2016)
//!   and then pauses it while flag `0x225` is clear (`P1[0]` `+0x5D..+0x91` -
//!   the opening's silent dawn, repaired in retail by the opening records'
//!   sub-9 restarts). A cold entry must keep that authored pause (the
//!   new-game chain relies on it); a staged free-roam entry must drop it,
//!   or the picker's music dies half a second in with nothing left to
//!   resume it.
//! - **Scenery**: `town0c` (post-Mist Rim Elm) stages flag `0x147`, seating
//!   the blown-gate rock debris and parking the intact wall/doorway pieces;
//!   staging `town01` afterwards must reset it (no leak between picks).
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::scene::{BgmDirector, SceneHost};

#[derive(Default)]
struct Calls(Vec<String>);

impl BgmDirector for Calls {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.0.push(format!("start({bgm_id})"));
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.0.push(format!("start_owned_vab({bgm_id})"));
    }
    fn pause(&mut self) {
        self.0.push("pause".into());
    }
    fn resume(&mut self) {
        self.0.push("resume".into());
    }
    fn stop(&mut self) {
        self.0.push("stop".into());
    }
}

fn open_host() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = std::path::PathBuf::from(disc);
    path.exists()
        .then(|| SceneHost::open_disc(&path).expect("open disc"))
}

/// Drive `ticks` world ticks, routing BGM events into a call log.
fn run_bgm(host: &mut SceneHost, ticks: usize) -> Vec<String> {
    let mut dir = Calls::default();
    for _ in 0..ticks {
        let _ = host.world.tick();
        let _ = host.route_bgm_events(&mut dir).expect("route");
    }
    dir.0
}

#[test]
fn town01_cold_entry_keeps_the_authored_silent_dawn_pause() {
    let Some(mut host) = open_host() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    host.enter_field_scene("town01", 0).expect("enter");
    let calls = run_bgm(&mut host, 300);
    assert_eq!(
        calls,
        vec!["start_owned_vab(2016)".to_string(), "pause".to_string()],
        "cold entry = the new-game shape: start the theme, then the \
         flag-0x225-clear pause"
    );
}

#[test]
fn town01_free_roam_staging_drops_the_entry_pause() {
    let Some(mut host) = open_host() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    host.world.seed_free_roam_story_baseline("town01");
    host.enter_field_scene("town01", 0).expect("enter");
    let calls = run_bgm(&mut host, 300);
    assert_eq!(
        calls,
        vec!["start_owned_vab(2016)".to_string()],
        "staged entry starts the theme and drops the entry-window pause"
    );
}

#[test]
fn town0c_staging_swaps_gate_records_and_resets_between_picks() {
    let Some(mut host) = open_host() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    // town0c staged: intact wall/doorway (shape A: 9/10/15) parked, rock
    // debris (shape B: 18..21) seated.
    host.world.seed_free_roam_story_baseline("town0c");
    host.enter_field_scene("town0c", 0).expect("enter town0c");
    let hidden = host.world.hidden_object_records();
    for rec in [9usize, 10, 15] {
        assert!(
            hidden.contains(&rec),
            "staged town0c must park the intact doorway P0[{rec}]; hidden = {hidden:?}"
        );
    }
    for rec in [18usize, 19, 20, 21] {
        assert!(
            !hidden.contains(&rec),
            "staged town0c must seat the gate rocks P0[{rec}]; hidden = {hidden:?}"
        );
    }
    // Picking town01 afterwards resets the managed flag: rocks hidden again.
    host.world.seed_free_roam_story_baseline("town01");
    host.enter_field_scene("town01", 0).expect("enter town01");
    let hidden = host.world.hidden_object_records();
    for rec in [18usize, 19, 20, 21] {
        assert!(
            hidden.contains(&rec),
            "staging town01 after town0c must re-hide the rocks P0[{rec}]"
        );
    }
}

#[test]
fn static_story_evaluation_matches_the_live_world() {
    use legaia_engine_core::field_env;
    let Some(mut host) = open_host() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    // The viewer-side static evaluator must agree with the live host's
    // staged entry for both story twins.
    for scene_name in ["town01", "town0c"] {
        host.world.seed_free_roam_story_baseline(scene_name);
        host.enter_field_scene(scene_name, 0).expect("enter");
        let live = host.world.hidden_object_records();
        let scene = host.scene.as_ref().expect("scene");
        let statically = field_env::story_hidden_records_for_scene(scene, &host.index);
        assert_eq!(
            statically, live,
            "{scene_name}: static story evaluation diverges from the live world"
        );
    }
}
