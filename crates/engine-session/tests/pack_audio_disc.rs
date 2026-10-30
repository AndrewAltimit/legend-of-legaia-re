//! Disc-gated: the Rebirth pack's soundscape on the session both play hosts
//! run, over the device-free sink, measured at the mixer's output.
//!
//! * 5F: the floor's bar tune (`2024`) starts on entry; walking into the
//!   games room cross-fades to the casino's "Wild Declaration" (`2018`) -
//!   the old score fades out, then the new one sounds.
//! * 7F: the inner sanctum hands the music to the pack's own drone (a WAV
//!   under `assets/audio/`): the retail score fades out and leaves, the
//!   drone loops on the PCM bus at a level comparable to the retail score's.
//! * 1F: the ember crystals' crackle sounds on the bus, louder beside a
//!   crystal than across the room.
//!
//! With `LEGAIA_PACK_AUDIO_DUMP=<dir>` the 7F hand-over is also written to
//! `<dir>/rebirth_7f_sanctum.wav` (Sony-derived output; never commit it).
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_audio::{SPU_INTERNAL_RATE, SinkMeasure, TestAudioSink};
use legaia_engine_expansion::audio::PACK_TRACK_BASE;
use legaia_engine_session::audio_director::AudioCommand;
use legaia_engine_session::session::FieldLiveOpts;
use legaia_engine_session::{BootConfig, BootSession};
use legaia_expansion_format::ExpansionPack;

fn session() -> Option<BootSession<TestAudioSink>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host =
        legaia_engine_scene_host::scene_host::SceneHost::open_disc(std::path::Path::new(&disc))
            .expect("open disc");
    let mut s = BootSession::from_host(host, None, &BootConfig::default(), || {
        Ok(TestAudioSink::new(SPU_INTERNAL_RATE))
    })
    .expect("boot session");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
    s.host
        .activate_expansion_pack(Arc::new(pack))
        .expect("the Rebirth pack activates");
    Some(s)
}

/// Every command the director was handed, frame by frame.
#[derive(Default)]
struct Rec {
    cmds: Vec<AudioCommand>,
    seen: u64,
}

impl Rec {
    fn take(&mut self, s: &BootSession<TestAudioSink>) {
        let log = s.bgm.as_ref().expect("audio").commands();
        let new = (log.count() - self.seen) as usize;
        let recent: Vec<_> = log.recent().copied().collect();
        assert!(
            new <= recent.len(),
            "more commands in a frame than the log keeps"
        );
        self.cmds
            .extend(recent.iter().skip(recent.len() - new).copied());
        self.seen = log.count();
    }

    fn has(&self, c: AudioCommand) -> bool {
        self.cmds.contains(&c)
    }
}

/// One host frame: the tick, the audio frame, one video frame of output.
fn step(s: &mut BootSession<TestAudioSink>, rec: &mut Rec) -> SinkMeasure {
    s.host.world.set_pad(0);
    s.tick().expect("tick");
    let _ = s.step_audio_frame();
    rec.take(s);
    s.bgm.as_ref().expect("audio").audio().render_video_frame()
}

fn steps(s: &mut BootSession<TestAudioSink>, rec: &mut Rec, n: usize) -> SinkMeasure {
    let mut m = SinkMeasure::default();
    for _ in 0..n {
        m.merge(step(s, rec));
    }
    m
}

/// Stand the player on map-local tile `(c, r)` of the map at `origin`.
fn stand(s: &mut BootSession<TestAudioSink>, origin: [i32; 2], (c, r): (i32, i32)) {
    let p = &mut s.host.world.actors[0].move_state;
    p.world_x = ((origin[0] + c) * 128 + 64) as i16;
    p.world_z = ((origin[1] + r) * 128 + 64) as i16;
}

#[test]
fn the_games_room_cross_fades_to_the_casino_theme() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_expansion_map_live("rebirth_tower_5f", "from_below", &FieldLiveOpts::default())
        .expect("enter 5F");
    let mut rec = Rec {
        seen: s.bgm.as_ref().expect("audio").commands().count(),
        ..Rec::default()
    };
    let bar = steps(&mut s, &mut rec, 60);
    assert!(rec.has(AudioCommand::BgmStartOwned(2024)), "{:?}", rec.cmds);
    assert!(!bar.is_silent(), "the bar tune sounds");
    // Into the games room (5F `[[audio.zones]]`, 40-frame fade).
    stand(&mut s, [62, 56], (10, 8));
    let fading = steps(&mut s, &mut rec, 20);
    assert!(rec.has(AudioCommand::BgmFade(2018, 40)), "{:?}", rec.cmds);
    assert!(
        s.bgm.as_ref().unwrap().pack_start_deferred(),
        "the casino theme waits for the bar tune's fade-out"
    );
    assert!(fading.mean_abs() < bar.mean_abs(), "fading out");
    let casino = steps(&mut s, &mut rec, 120);
    assert!(!s.bgm.as_ref().unwrap().pack_start_deferred());
    assert!(rec.has(AudioCommand::BgmStartOwned(2018)), "{:?}", rec.cmds);
    assert!(!casino.is_silent(), "the casino theme sounds");
    eprintln!(
        "5F levels (mean |x| per channel): bar {:.0}, fade {:.0}, casino {:.0}",
        bar.mean_abs(),
        fading.mean_abs(),
        casino.mean_abs()
    );
}

#[test]
fn the_sanctum_hands_the_music_to_the_packs_drone() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_expansion_map_live("rebirth_tower_7f", "from_below", &FieldLiveOpts::default())
        .expect("enter 7F");
    let dump = std::env::var_os("LEGAIA_PACK_AUDIO_DUMP");
    let mut pcm: Vec<i16> = Vec::new();
    let mut rec = Rec {
        seen: s.bgm.as_ref().expect("audio").commands().count(),
        ..Rec::default()
    };
    let mut frame = |s: &mut BootSession<TestAudioSink>, pcm: &mut Vec<i16>| {
        s.host.world.set_pad(0);
        s.tick().expect("tick");
        let _ = s.step_audio_frame();
        rec.take(s);
        let sink = s.bgm.as_ref().unwrap().audio();
        let n = sink.frames_per_video_frame();
        let mut m = SinkMeasure::default();
        for _ in 0..n {
            let (l, r) = sink.next_frame();
            if dump.is_some() {
                pcm.extend([l, r]);
            }
            m.merge(SinkMeasure {
                frames: 1,
                peak: i32::from(l).abs().max(i32::from(r).abs()),
                nonzero: usize::from(l != 0 || r != 0),
                sum_abs: (i32::from(l).unsigned_abs() + i32::from(r).unsigned_abs()) as u64,
            });
        }
        m
    };
    let mut retail = SinkMeasure::default();
    for _ in 0..120 {
        retail.merge(frame(&mut s, &mut pcm));
    }
    assert!(!retail.is_silent(), "7F's own score sounds");
    stand(&mut s, [26, 17], (12, 9));
    for _ in 0..120 {
        frame(&mut s, &mut pcm);
    }
    let d = s.bgm.as_ref().unwrap();
    assert!(d.pack_music_playing(), "the drone is on the bus");
    assert!(!d.is_attached(), "the retail score faded out and left");
    let mut drone = SinkMeasure::default();
    for _ in 0..240 {
        drone.merge(frame(&mut s, &mut pcm));
    }
    assert!(
        drone.nonzero * 1000 >= drone.frames * 999,
        "four seconds of a six-second loop, no gap: {drone:?}"
    );
    let ratio = drone.mean_abs() / retail.mean_abs();
    eprintln!(
        "7F levels (mean |x| per channel): retail score {:.0}, pack drone {:.0} (ratio {ratio:.2})",
        retail.mean_abs(),
        drone.mean_abs()
    );
    assert!(
        (0.2..5.0).contains(&ratio),
        "the normalised drone sits near the retail score's level"
    );
    // Back out of the sanctum: the floor's score returns.
    stand(&mut s, [26, 17], (9, 20));
    for _ in 0..200 {
        frame(&mut s, &mut pcm);
    }
    drop(frame);
    assert!(
        rec.has(AudioCommand::PackTrack(PACK_TRACK_BASE, 90)),
        "{:?}",
        rec.cmds
    );
    assert!(rec.has(AudioCommand::BgmFade(2011, 90)), "{:?}", rec.cmds);
    assert!(s.bgm.as_ref().unwrap().is_attached());
    if let Some(dir) = dump {
        let path = std::path::Path::new(&dir).join("rebirth_7f_sanctum.wav");
        let wav = legaia_expansion_format::audio_clip::encode_wav(SPU_INTERNAL_RATE, 2, &pcm, None);
        std::fs::write(&path, wav).expect("write the dump");
        eprintln!("wrote {}", path.display());
    }
}

#[test]
fn the_ember_crystals_crackle_by_distance() {
    let Some(mut s) = session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    s.enter_expansion_map_live("rebirth_tower_1f", "", &FieldLiveOpts::default())
        .expect("enter 1F");
    let mut rec = Rec {
        seen: s.bgm.as_ref().expect("audio").commands().count(),
        ..Rec::default()
    };
    steps(&mut s, &mut rec, 10);
    // Beside the north-west crystal: its loop (and the Warden's Stone hum,
    // two tiles off) on the bus.
    stand(&mut s, [16, 16], (3, 5));
    steps(&mut s, &mut rec, 30);
    let near: Vec<_> = s.host.world.expansion_ambient_levels().to_vec();
    assert!(near.iter().any(|l| l.sound == "torch"), "{near:?}");
    assert!(s.bgm.as_ref().unwrap().pack_voices() >= 1);
    let torch_near = near
        .iter()
        .filter(|l| l.sound == "torch")
        .map(|l| l.pan.0 + l.pan.1)
        .fold(0.0f32, f32::max);
    // Across the room, by the south stair: nothing in reach.
    stand(&mut s, [16, 16], (7, 11));
    steps(&mut s, &mut rec, 30);
    let far: f32 = s
        .host
        .world
        .expansion_ambient_levels()
        .iter()
        .filter(|l| l.sound == "torch")
        .map(|l| l.pan.0 + l.pan.1)
        .fold(0.0, f32::max);
    assert!(far < torch_near, "near {torch_near} far {far}");
}
