//! Disc-gated: a mid-scene start that lands under the script's pause stays
//! silent until the script's replay.
//!
//! The swap poller `FUN_800243F0` attaches the installed score at stage 6
//! only while pause bit 1 of `_DAT_8007B750` is clear (`andi v0,v0,0x2` /
//! `bne` at `0x8002479C..0x800247A0`, around `jal 0x80026478`). `vozz`
//! `P1[7]`, the Genesis Tree's revival, starts `2035` with `35 F3 07 01`,
//! pauses the slot with `35 00 00 02` in the same slice, and replays it with
//! `35 00 00 04` after the burst: retail's theme is first heard at the replay.
//! Attached at stage 6 regardless, the port's theme started at the install
//! and restarted at the replay.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start(u16),
    Stop,
    Pause,
    Resume,
}

#[derive(Default)]
struct Calls(Vec<(usize, Call)>, usize);

impl BgmDirector for Calls {
    fn start(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push((self.1, Call::Start(id)));
    }
    fn start_owned_vab(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push((self.1, Call::Start(id)));
    }
    fn stop(&mut self) {
        self.0.push((self.1, Call::Stop));
    }
    fn pause(&mut self) {
        self.0.push((self.1, Call::Pause));
    }
    fn resume(&mut self) {
        self.0.push((self.1, Call::Resume));
    }
}

#[test]
fn a_start_installed_under_the_pause_waits_for_the_replay() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    host.set_entry_seat_tile(10, 10);
    host.enter_field_scene("kor4", 0).expect("enter");
    let mut calls = Calls::default();
    for _ in 0..30 {
        let _ = host.tick();
        host.route_bgm_events(&mut calls).expect("route");
    }
    // P1[7]'s words: start 2035, pause in the same slice, a long wait, the
    // replay.
    host.world.load_field_script_at(
        vec![
            0x35, 0xF3, 0x07, 0x01, 0x35, 0x00, 0x00, 0x02, 0x4A, 0xF0, 0x00, 0x35, 0x00, 0x00,
            0x04, 0x21,
        ],
        0,
    );
    calls.0.clear();
    for frame in 0..400 {
        calls.1 = frame;
        let _ = host.tick();
        host.route_bgm_events(&mut calls).expect("route");
    }
    eprintln!("{:?}", calls.0);
    let start = calls
        .0
        .iter()
        .position(|(_, c)| *c == Call::Start(2035))
        .expect("2035 installs");
    let (at, _) = calls.0[start];
    assert_eq!(
        calls.0.get(start + 1),
        Some(&(at, Call::Pause)),
        "the install under the pause holds the slot paused in the same frame"
    );
    let resume = calls
        .0
        .iter()
        .position(|(_, c)| *c == Call::Resume)
        .expect("the replay");
    assert!(resume > start, "the replay comes after the install");
    assert!(
        calls.0[start + 2..resume]
            .iter()
            .all(|(_, c)| matches!(c, Call::Pause)),
        "nothing sounds between: {:?}",
        calls.0
    );
}
