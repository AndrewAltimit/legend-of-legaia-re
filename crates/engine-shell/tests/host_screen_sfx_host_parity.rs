//! Disc-gated: a cue queued under the party-wipe hold sounds on the same
//! frame on both play hosts.
//!
//! The hold is a host-held screen: it owns its ticks and the field's frame
//! tail does not run, so the tick's audio frame does not either. The native
//! window steps the SFX scheduler alone under every host-held screen
//! (`tick_menu_sfx`, the session's `step_overlay_sfx`); the page's party-wipe
//! and Boss Rush arms stepped nothing, so a cue still on the scheduler when
//! the party fell - a delayed strike cue, a death cry - stayed there for the
//! length of the hold and sounded over whatever came next.
//!
//! Both hosts are put in the hold, one cue is queued on each director, and
//! the directors' command records are compared after every display frame.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_session::audio_director::RETAIL_MENU_CONFIRM_CUE;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_web_play::runtime::LegaiaRuntime;

fn commands<S: legaia_engine_audio::AudioSink>(
    s: &legaia_engine_session::BootSession<S>,
) -> (u64, Vec<String>) {
    let log = s.bgm.as_ref().expect("a director").commands();
    (
        log.count(),
        log.recent().map(|c| format!("{c:?}")).collect(),
    )
}

#[test]
fn a_cue_under_the_party_wipe_hold_sounds_on_both_hosts() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for steps in [1u32, 2] {
        let mut w = HeadlessPlayWindow::open(std::path::Path::new(&disc), "town01").expect("open");
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(std::fs::read(&disc).expect("read disc"), String::new())
            .expect("load_disc");
        assert!(rt.debug_attach_audio(), "page audio director");
        rt.enter_field("town01").expect("enter_field");

        // Settle, raise the wipe, and let each host's probe take it: the
        // window's ahead of its next tick, the page's at the end of one.
        w.display_frame(0, 4);
        rt.frame_ticks(4, 0, 0).expect("ticks");
        w.session_mut().host.world.game_over = true;
        rt.debug_session_mut()
            .expect("session")
            .host
            .world
            .game_over = true;
        w.display_frame(0, 1);
        rt.frame_ticks(1, 0, 0).expect("ticks");
        assert!(w.boot_ui_active(), "the window holds");
        assert!(rt.is_game_over(), "the page holds");

        w.session_mut()
            .bgm
            .as_mut()
            .expect("native director")
            .queue_ui_cue(RETAIL_MENU_CONFIRM_CUE);
        rt.debug_session_mut()
            .expect("session")
            .bgm
            .as_mut()
            .expect("page director")
            .queue_ui_cue(RETAIL_MENU_CONFIRM_CUE);
        let n0 = commands(w.session()).0;
        let p0 = commands(rt.debug_session().expect("session")).0;
        for frame in 0..3 {
            w.display_frame(0, steps);
            rt.frame_ticks(steps, 0, 0).expect("ticks");
            let (n, n_recent) = commands(w.session());
            let (p, p_recent) = commands(rt.debug_session().expect("session"));
            assert_eq!(
                n - n0,
                p - p0,
                "{steps} tick(s) a frame, frame {frame} of the hold: commands since the cue\n  \
                 native: {n_recent:?}\n  page:   {p_recent:?}"
            );
            if frame == 0 {
                assert!(
                    n > n0,
                    "the window's scheduler sounded nothing under the hold"
                );
            }
        }
    }
}
