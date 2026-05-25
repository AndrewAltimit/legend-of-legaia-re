//! Smoke check for the in-browser STR (FMV) video path used by
//! site/media.html: the in-memory demux must recover the movie's dimensions,
//! a plausible frame count, and a ~15 fps rate, and each assembled frame must
//! decode to a full RGBA8 buffer of the right size.
//!
//! Skips when `LEGAIA_DISC_BIN` isn't set. Disc-gated.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::audio::{decode_str_frame_rgba, demux_str_video, enumerate_xa_files};
use std::env;
use std::fs;
use std::path::PathBuf;

#[test]
fn in_memory_str_video_demux_recovers_frames_for_mv1() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping STR video smoke test");
        return;
    };
    let disc = fs::read(PathBuf::from(&path)).expect("disc image");

    let mv1 = enumerate_xa_files(&disc)
        .into_iter()
        .find(|f| f.path.ends_with("MV1.STR"))
        .expect("MV1.STR on disc");
    eprintln!("[str] MV1.STR @ lba={} size={}", mv1.lba, mv1.size);

    let video = demux_str_video(&disc, mv1.lba, mv1.size);
    eprintln!(
        "[str] {}x{} {} frames @ {:.2} fps",
        video.width,
        video.height,
        video.frames.len(),
        video.fps
    );

    // The six Legaia movies are 320x224, 15 fps (10 sectors/frame at 2x).
    assert_eq!(video.width, 320, "MV1 width");
    assert_eq!(video.height, 224, "MV1 height");
    assert!(video.frames.len() > 1000, "MV1 should have >1000 frames");
    assert!(
        (video.fps - 15.0).abs() < 1.0,
        "MV1 fps should be ~15, got {}",
        video.fps
    );

    // The first frame must decode to a full RGBA8 buffer (w*h*4) and not be
    // entirely zero (a black frame would still be all-zero alpha-included? no -
    // RGBA black has alpha 255, so any decoded frame has nonzero bytes).
    let frame0 = decode_str_frame_rgba(&video.frames[0]);
    assert_eq!(
        frame0.len(),
        (video.width * video.height * 4) as usize,
        "frame 0 RGBA size"
    );
    assert!(
        frame0.iter().any(|&b| b != 0),
        "frame 0 decoded to all zeros (decode failed?)"
    );

    // The last frame also decodes cleanly (exercises the tail of the stream).
    let last = decode_str_frame_rgba(video.frames.last().unwrap());
    assert_eq!(last.len(), (video.width * video.height * 4) as usize);
}
