//! Disc-gated: what a forced fight does to the music, through the session.
//!
//! The retail timeline (`docs/subsystems/audio.md`, "The battle music
//! timeline"): the field track is keyed off on the intro's first frames, the
//! spin plays under the battle-start cue alone, and the battle track attaches
//! with the fight, `139` vsyncs after the mode-`8` store.

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::BgmDirector;

#[derive(Default)]
struct Log {
    tick: u32,
    events: Vec<(u32, &'static str, u16)>,
}

impl BgmDirector for Log {
    fn start(&mut self, id: u16, _seq: &[u8]) {
        self.events.push((self.tick, "start", id));
    }
    fn start_owned_vab(&mut self, id: u16, _entry: &[u8]) {
        self.events.push((self.tick, "start", id));
    }
    fn pause(&mut self) {
        self.events.push((self.tick, "pause", 0));
    }
    fn stop(&mut self) {
        self.events.push((self.tick, "stop", 0));
    }
}

#[test]
fn the_battle_track_arrives_with_the_fight() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let extracted = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if !extracted.join("PROT.DAT").exists() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    }
    let cfg = legaia_engine_session::session::BootConfig {
        scene: "teien".to_string(),
        enable_audio: false,
    };
    let mut session =
        legaia_engine_session::native::NativeSession::open(&extracted, &cfg).expect("open teien");
    let opts = legaia_engine_session::session::FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    session.enter_field_live("teien", &opts).expect("enter");
    let mut log = Log::default();
    for _ in 0..60 {
        session.tick().expect("tick");
        session.host.route_bgm_events(&mut log).expect("route");
    }
    let field_track = session.host.bgm_track_word;
    let row = session
        .host
        .world
        .registered_formation_ids()
        .into_iter()
        .next()
        .expect("teien has formation rows");
    assert!(session.host.world.force_encounter(row));
    log.events.clear();
    let mut entered = None;
    for t in 0..400 {
        log.tick = t;
        session.tick().expect("tick");
        session.host.route_bgm_events(&mut log).expect("route");
        if session.host.world.mode == SceneMode::Battle {
            entered = Some(t);
            break;
        }
    }
    let entered = entered.expect("the fight starts");
    eprintln!(
        "[timeline] field {field_track:?}, entered {entered}: {:?}",
        log.events
    );
    let pause = log
        .events
        .iter()
        .find(|e| e.1 == "pause")
        .expect("the intro keys the field track off");
    assert!(pause.0 <= 4, "keyed off with the intro's init: {pause:?}");
    let start = log
        .events
        .iter()
        .find(|e| e.1 == "start")
        .expect("the battle track starts");
    assert_eq!(
        start.0, entered,
        "attached with the fight: {:?}",
        log.events
    );
    assert!(
        entered > 100,
        "the spin is over two seconds on the vsync clock: {entered} ticks"
    );
}
