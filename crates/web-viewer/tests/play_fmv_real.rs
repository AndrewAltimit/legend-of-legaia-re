//! Disc-gated oracle for the browser play page's **FMV lane**
//! (`site/js/play-fmv.js` over `LegaiaRuntime::play_fmv_*`).
//!
//! Drives the runtime exactly the way the page does: a field-VM FMV trigger
//! parks the world in `SceneMode::Cutscene`, the runtime publishes the
//! movie's raw-sector window (the `MV*.STR` extent narrowed to the `fmv_id`'s
//! frame range - the same `fmv_segment_window` the native window seeks
//! with), the page slices those sectors off the disc bytes and installs them,
//! decodes frames + PCM, and finishes - after which the post-movie hand-off
//! enters the scene retail's dispatch names (`town01` -> fmv 1 -> `town0b`).
//!
//! No Sony bytes are asserted - only structural facts (window geometry, frame
//! size, a non-uniform picture, non-silent audio). Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_asset::fmv_dispatch::SECTORS_PER_FRAME;
use legaia_web_viewer::runtime::LegaiaRuntime;

const RAW_SECTOR: usize = 2352;
const CROSS: u16 = 0x4000;
const START: u16 = 0x0008;

fn disc_bytes() -> Option<Vec<u8>> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    std::fs::read(&disc).ok()
}

fn loaded_in(disc: &[u8], scene: &str) -> LegaiaRuntime {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc.to_vec(), String::new())
        .expect("load disc");
    rt.enter_field(scene).expect("enter field");
    rt
}

#[derive(Debug)]
struct Wanted {
    fmv_id: i16,
    path: String,
    first_sector: u32,
    sector_count: u32,
}

fn wanted(rt: &LegaiaRuntime) -> Option<Wanted> {
    let j = rt.play_fmv_wanted_json();
    if j == "null" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&j).ok()?;
    Some(Wanted {
        fmv_id: v["fmv_id"].as_i64()? as i16,
        path: v["path"].as_str()?.to_string(),
        first_sector: v["first_sector"].as_u64()? as u32,
        sector_count: v["sector_count"].as_u64()? as u32,
    })
}

fn extent(rt: &LegaiaRuntime, path: &str) -> (u32, u32) {
    let v: serde_json::Value =
        serde_json::from_str(&rt.disc_file_extent_json(path)).expect("extent json");
    (
        v["lba"].as_u64().expect("lba") as u32,
        v["size"].as_u64().expect("size") as u32,
    )
}

fn slice_sectors(disc: &[u8], w: &Wanted) -> Vec<u8> {
    let start = w.first_sector as usize * RAW_SECTOR;
    let end = start + w.sector_count as usize * RAW_SECTOR;
    disc[start..end.min(disc.len())].to_vec()
}

/// A decoded frame is a picture, not a flat fill: more than one distinct
/// RGB triple.
fn non_uniform(rgba: &[u8]) -> bool {
    let first = &rgba[..3];
    rgba.as_chunks::<4>().0.iter().any(|px| &px[..3] != first)
}

fn non_silent(pcm: &[i16]) -> bool {
    pcm.iter().filter(|s| s.unsigned_abs() > 256).count() > pcm.len() / 100
}

/// Install the wanted movie off the disc bytes and check the decoded media.
fn install_and_check(rt: &mut LegaiaRuntime, disc: &[u8], w: &Wanted) {
    let sectors = slice_sectors(disc, w);
    assert!(rt.play_fmv_install(&sectors), "install {}", w.path);
    assert!(rt.play_fmv_active());
    assert_eq!(
        rt.play_fmv_wanted_json(),
        "null",
        "installed -> nothing wanted"
    );
    let size = rt.play_fmv_size();
    assert_eq!(
        size,
        vec![320, 224],
        "{}: Legaia movies are 320x224",
        w.path
    );
    let n = rt.play_fmv_frame_count();
    assert!(n > 0);
    let fps = rt.play_fmv_fps();
    assert!((fps - 15.0).abs() < 1.0, "{}: fps {fps}", w.path);
    let frame0 = rt.play_fmv_frame_rgba(0);
    assert_eq!(frame0.len(), 320 * 224 * 4, "{}: frame 0 RGBA size", w.path);
    assert!(non_uniform(&frame0), "{}: frame 0 is a flat fill", w.path);
    let last = rt.play_fmv_frame_rgba(n - 1);
    assert_eq!(last.len(), 320 * 224 * 4, "{}: last frame decodes", w.path);
    assert!(
        rt.play_fmv_frame_rgba(n).is_empty(),
        "past the end is empty"
    );
    assert_eq!(rt.play_fmv_audio_rate(), 37_800, "{}: XA rate", w.path);
    assert_eq!(rt.play_fmv_audio_channels(), 2, "{}: stereo", w.path);
    let pcm = rt.play_fmv_audio_pcm_i16();
    assert!(!pcm.is_empty(), "{}: PCM handed over", w.path);
    assert!(non_silent(&pcm), "{}: PCM is silence", w.path);
    assert!(
        rt.play_fmv_audio_pcm_i16().is_empty(),
        "one-shot PCM hand-off"
    );
    // The segment's video runs as long as its frame range says, give or
    // take the stream's own padding.
    let frames = w.sector_count / SECTORS_PER_FRAME;
    assert!(
        n.abs_diff(frames) <= 2,
        "{}: {n} frames decoded from a {frames}-frame window",
        w.path
    );
}

#[test]
fn mid_game_fmv_holds_the_world_and_hands_off_after_finish() {
    let Some(disc) = disc_bytes() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let mut rt = loaded_in(&disc, "town01");
    rt.play_fmv_set_supported(true);
    assert_eq!(rt.play_fmv_wanted_json(), "null");
    assert!(!rt.play_fmv_active());

    // The field-VM trigger: fmv 1 is `town01`'s own movie (MV2.STR).
    assert!(rt.play_fmv_trigger(1));
    assert_eq!(rt.tick_frame().expect("tick"), "");
    let w = wanted(&rt).expect("movie wanted after the trigger tick");
    assert_eq!(w.fmv_id, 1);
    assert_eq!(w.path, "MOV/MV2.STR");
    let (lba, size) = extent(&rt, "MOV/MV2.STR");
    assert_eq!(
        w.first_sector, lba,
        "fmv 1 starts at frame 1 = the file start"
    );
    assert!(w.sector_count > 0 && w.sector_count <= size.div_ceil(2048));
    assert!(!rt.play_fmv_skippable(), "mid-game movies play out");

    // Waiting for the page: the world holds under Cutscene.
    for _ in 0..30 {
        assert_eq!(rt.tick_frame().expect("tick"), "");
        assert!(wanted(&rt).is_some(), "still wanted while uninstalled");
    }

    install_and_check(&mut rt, &disc, &w);

    // Playing: the world stays held, and a pad edge does NOT abort fmv 1.
    rt.set_pad(CROSS);
    for i in 0..60 {
        assert_eq!(rt.tick_frame().expect("tick"), "", "tick {i}");
        assert!(
            rt.play_fmv_active(),
            "held while the movie plays (tick {i})"
        );
        rt.set_pad(0);
    }

    // The page reports the end: the next tick releases the world and the
    // hand-off enters the scene retail's dispatch names for fmv 1.
    rt.play_fmv_finish();
    let entered = rt.tick_frame().expect("tick");
    assert_eq!(entered, "town0b", "fmv 1's post-play hand-off");
    assert!(!rt.play_fmv_active());
    assert_eq!(rt.play_fmv_wanted_json(), "null");
    let state: serde_json::Value = serde_json::from_str(&rt.state_json()).expect("state");
    assert_eq!(state["scene"].as_str(), Some("town0b"));
}

#[test]
fn mv3_segment_window_seeks_into_the_file_and_decodes() {
    let Some(disc) = disc_bytes() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let mut rt = loaded_in(&disc, "town01");
    rt.play_fmv_set_supported(true);
    // fmv 4 = MV3.STR frames 0x1a5..=0x27b (the dispatch table's third MV3
    // segment): the window must start (0x1a5 - 1) * 10 sectors into the
    // file and run exactly the segment's frames, or the page plays the
    // wrong cutscene.
    assert!(rt.play_fmv_trigger(4));
    assert_eq!(rt.tick_frame().expect("tick"), "");
    let w = wanted(&rt).expect("movie wanted");
    assert_eq!(w.fmv_id, 4);
    assert_eq!(w.path, "MOV/MV3.STR");
    let (lba, size) = extent(&rt, "MOV/MV3.STR");
    assert_eq!(w.first_sector, lba + (0x1a5 - 1) * SECTORS_PER_FRAME);
    assert_eq!(w.sector_count, (0x27b - 0x1a5 + 1) * SECTORS_PER_FRAME);
    assert!(w.first_sector + w.sector_count <= lba + size.div_ceil(2048));
    install_and_check(&mut rt, &disc, &w);
    rt.play_fmv_finish();
    let _ = rt.tick_frame().expect("tick");
    assert!(!rt.play_fmv_active(), "released after finish");
}

#[test]
fn unsupported_page_auto_finishes_with_the_handoff() {
    let Some(disc) = disc_bytes() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    // A cached bundle never declares support: the movie finishes the frame
    // it arms and the hand-off still lands in `town0b`.
    let mut rt = loaded_in(&disc, "town01");
    assert!(rt.play_fmv_trigger(1));
    let entered = rt.tick_frame().expect("tick");
    assert_eq!(entered, "town0b");
    assert!(!rt.play_fmv_active());
    assert_eq!(rt.play_fmv_wanted_json(), "null");
}

#[test]
fn title_attract_plays_mv1_and_a_face_button_skips_it() {
    let Some(disc) = disc_bytes() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc.clone(), String::new())
        .expect("load disc");
    rt.play_fmv_set_supported(true);
    rt.boot_title_start();
    assert!(rt.boot_title_is_active());
    // Press Start onto the menu (retail's `AttractIdle` block is the menu;
    // the countdown only runs there), then idle until it hands the screen
    // to fmv 0.
    assert_eq!(rt.boot_title_step(START), "");
    let mut w = None;
    for _ in 0..4000 {
        assert_eq!(rt.boot_title_step(0), "");
        if let Some(x) = wanted(&rt) {
            w = Some(x);
            break;
        }
    }
    let w = w.expect("the attract armed a movie");
    assert_eq!(w.fmv_id, 0);
    assert_eq!(w.path, "MOV/MV1.STR");
    let (lba, _) = extent(&rt, "MOV/MV1.STR");
    assert_eq!(w.first_sector, lba);
    assert_eq!(
        rt.boot_title_attract_skips(),
        0,
        "a real playback is not a skip"
    );
    // Held while the page installs.
    for _ in 0..10 {
        assert_eq!(rt.boot_title_step(0), "");
        assert!(wanted(&rt).is_some());
    }
    install_and_check(&mut rt, &disc, &w);
    assert!(rt.play_fmv_skippable(), "the attract movie is skippable");
    for _ in 0..30 {
        assert_eq!(rt.boot_title_step(0), "");
        assert!(rt.play_fmv_active(), "held while the attract plays");
    }
    // Retail's abort: a face button ends fmv 0 and re-enters the title.
    assert_eq!(rt.boot_title_step(CROSS), "");
    assert!(
        !rt.play_fmv_active(),
        "Cross edge aborted the attract movie"
    );
    assert!(rt.boot_title_is_active(), "back on the title card");
    assert_eq!(rt.boot_title_attract_skips(), 0);
    assert_eq!(rt.play_fmv_wanted_json(), "null");

    // Without support the same countdown counts as a skip.
    rt.play_fmv_set_supported(false);
    let mut skipped = false;
    for _ in 0..4000 {
        assert_eq!(rt.boot_title_step(0), "");
        if rt.boot_title_attract_skips() == 1 {
            skipped = true;
            break;
        }
        assert!(wanted(&rt).is_none(), "unsupported: never left wanting");
    }
    assert!(skipped, "the unsupported attract was counted as a skip");
    assert!(rt.boot_title_is_active());
}
