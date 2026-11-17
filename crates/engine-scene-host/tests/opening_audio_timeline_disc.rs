//! Disc-gated: the New-Game opening's music, call by call, against the
//! retail timeline `autorun_audio_timeline.lua` logs off `s1_newgame_field`.
//!
//! `opstati`'s closing record fades `2063` with op `0x35` sub-op `5 (90)` as
//! it changes scene. Retail's release counts the vsyncs of the frames that
//! run (`FUN_800267FC`, a frame step a frame), so it expires 91 vsyncs after
//! the arm, past the scene change: the score has faded to its floor in
//! `opstati`, plays on into `opurud` and stops there, before `opurud`'s
//! `2064` commit replaces it. A release counted at the frame step a vsync
//! stopped it 47 vsyncs in, mid-fade, still in `opstati`.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start(u16),
    Pause,
    Stop,
    Fade(i16),
    Close,
}

#[derive(Default)]
struct Calls(Vec<Call>);

impl BgmDirector for Calls {
    fn start(&mut self, id: u16, _b: &[u8]) {
        self.0.push(Call::Start(id));
    }
    fn start_owned_vab(&mut self, id: u16, _b: &[u8]) {
        self.0.push(Call::Start(id));
    }
    fn pause(&mut self) {
        self.0.push(Call::Pause);
    }
    fn stop(&mut self) {
        self.0.push(Call::Stop);
    }
    fn timed_release_fade(&mut self, d: i16) {
        self.0.push(Call::Fade(d));
    }
    fn close_field_source(&mut self) {
        self.0.push(Call::Close);
    }
}

#[test]
fn opstatis_release_fades_2063_out_and_stops_it_in_opurud() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.world.begin_new_game();
    host.enter_field_scene(legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE, 0)
        .expect("enter opdeene");
    // (vsync, scene, call) for every call the router makes.
    let mut log: Vec<(u32, String, Call)> = Vec::new();
    let mut vsync = 0u32;
    while vsync < 20_000 && host.world.active_scene_label != "map01" {
        let _ = host.tick();
        vsync += 1;
        let mut calls = Calls::default();
        host.route_bgm_events(&mut calls).expect("route");
        for c in calls.0 {
            log.push((vsync, host.world.active_scene_label.clone(), c));
        }
    }
    let at = |want: &Call| log.iter().find(|(_, _, c)| c == want).cloned();
    let (fade_v, fade_scene, _) = at(&Call::Fade(90)).expect("opstati arms 5 (90)");
    assert_eq!(fade_scene, "opstati");
    let (stop_v, stop_scene, _) = log
        .iter()
        .find(|(v, _, c)| *v > fade_v && *c == Call::Pause)
        .cloned()
        .expect("the release expires");
    assert_eq!(
        stop_scene, "opurud",
        "the faded score plays on past the door"
    );
    assert_eq!(
        stop_v - fade_v,
        92,
        "91 vsyncs counted, the stop on the next"
    );
    let (start_v, _, _) = at(&Call::Start(2064)).expect("opurud starts 2064");
    assert!(stop_v < start_v, "2063 stops before 2064 arrives");
}
