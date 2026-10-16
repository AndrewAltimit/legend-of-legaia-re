//! The cast-sequence oracle's pairing rule over synthetic recordings
//! (`legaia_parity::cast_sequence`, `docs/tooling/cast-sequence-oracle.md`).
//! Disc-free: both sides are written here, each frame a solid grey whose
//! level names the row it was drawn on, so a pair's score says which rows
//! the rule matched.

use std::fmt::Write as _;
use std::path::Path;

use legaia_parity::cast_sequence::{FrameClass, Key, Lag, Side, compare};

/// A BGR555 grey `v` (5-bit) over the 320 x 228 display.
fn retail_raw(v5: u16) -> Vec<u8> {
    let w = v5 | (v5 << 5) | (v5 << 10);
    w.to_le_bytes().repeat(320 * 228)
}

/// A 320 x 224 RGB PNG of grey `v` (8-bit).
fn engine_png(path: &Path, v: u8) {
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), 320, 224);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&vec![v; 320 * 224 * 3])
        .unwrap();
}

/// The 8-bit level a 5-bit grey decodes to.
fn expand(v5: u16) -> u8 {
    legaia_mednafen::bgr555_to_rgba8(v5 | (v5 << 5) | (v5 << 10))[0]
}

#[test]
fn frames_pair_on_the_action_clock_with_retail_display_lag() {
    let dir = tempfile::tempdir().unwrap();
    let (rd, ed) = (dir.path().join("retail"), dir.path().join("engine"));
    std::fs::create_dir_all(&rd).unwrap();
    std::fs::create_dir_all(&ed).unwrap();

    // Retail: cast begin 0x28 for 4 vsyncs, module phase 3 (state 0x35) for
    // 10, module phase 5 (state 0x36) for 6. Frame step 1, so the displayed
    // frame is 2 vsyncs older than the row it is grabbed on. Row `t` draws
    // grey level `(t - 100) * 3 / 2` in 5 bits, distinct per row, and the
    // grab on `t` shows row `t - 2`'s.
    let mut meta = String::from("vsync,st7,phase,step,shot,w,h,bpp\n");
    let level = |t: u64| ((t - 100) * 3 / 2) as u16;
    for t in 100..120u64 {
        let (st7, phase) = match t {
            100..=103 => (0x28, 0),
            104..=113 => (0x35, 3),
            _ => (0x36, 5),
        };
        let name = format!("r_{t:05}.raw");
        // What the display shows on vsync `t` was built two vsyncs earlier.
        std::fs::write(rd.join(&name), retail_raw(level(t.max(102) - 2))).unwrap();
        writeln!(meta, "{t},{st7},{phase},1,{name},320,228,16").unwrap();
    }
    std::fs::write(rd.join("meta.csv"), meta).unwrap();

    // Engine: the same segments, but module phase 3 runs only 3 ticks and
    // phase 5 runs in state 0x35 rather than 0x36. Its frame on each tick is
    // retail's level for the row it should pair with, so a correct pairing
    // scores 1.0.
    let mut meta = String::from("tick,st7,phase,acting,shot\n");
    let rows: Vec<(u64, u8, u8, u64)> = vec![
        // (tick, st7, phase, the retail row it mirrors)
        (10, 0x28, 0, 100),
        (11, 0x28, 0, 101),
        (12, 0x28, 0, 102),
        (13, 0x28, 0, 103),
        (14, 0x35, 3, 104),
        (15, 0x35, 3, 105),
        (16, 0x35, 3, 106),
        (17, 0x35, 5, 114),
        (18, 0x35, 5, 115),
        (19, 0x35, 5, 116),
        (20, 0x35, 5, 117),
        (21, 0x35, 5, 118),
        (22, 0x35, 5, 119),
    ];
    for &(t, st7, phase, mirror) in &rows {
        let name = format!("e_{t:05}.png");
        engine_png(&ed.join(&name), expand(level(mirror)));
        writeln!(meta, "{t},{st7},{phase},0,{name}").unwrap();
    }
    std::fs::write(ed.join("meta.csv"), meta).unwrap();

    let retail = Side::read(&rd).unwrap();
    let engine = Side::read(&ed).unwrap();
    let r = compare("synthetic", &retail, &engine, Lag::Auto, |_, _, _| {}).unwrap();

    // The first two grabs display rows before the recording: unpaired.
    assert_eq!(r.unpaired_retail, 2);
    let by_vsync = |v: u64| r.pairs.iter().find(|p| p.retail_vsync == v).unwrap();
    // Grabbed on 106, it shows row 104: phase 3's first vsync, engine tick 14.
    let p = by_vsync(106);
    assert_eq!(
        (p.shown_vsync, p.key, p.into, p.engine_tick),
        (104, Key::Module(3), 0, 14)
    );
    assert_eq!(p.within, 1.0);
    // Row 110 is 6 vsyncs into phase 3; the engine's phase 3 is 3 ticks
    // long, so the pair clamps to its last tick and the frames differ.
    let p = by_vsync(112);
    assert_eq!((p.into, p.engine_tick), (6, 16));
    assert!(p.within < 1.0);
    // Phase 5 pairs across the 0x36 / 0x35 split and counts the mismatch.
    let p = by_vsync(117);
    assert_eq!((p.key, p.into, p.engine_tick), (Key::Module(5), 1, 18));
    assert_eq!(p.within, 1.0);
    assert!(r.state_mismatch_pairs >= 4);
    assert!(r.pairs.iter().all(|p| p.class != FrameClass::White));
    let seg = r.segments.iter().find(|s| s.key == Key::Module(3)).unwrap();
    assert_eq!((seg.retail_len, seg.engine_len), (Some(10), Some(3)));
}

/// A retail capture with the frame counter pairs on the game clock: a frame
/// that ran past its step advances the pairing by its step, so the engine
/// frame that matches is the one at the same game time, not the same vsync.
#[test]
fn an_overrunning_retail_frame_costs_no_game_time() {
    let dir = tempfile::tempdir().unwrap();
    let (rd, ed) = (dir.path().join("retail"), dir.path().join("engine"));
    std::fs::create_dir_all(&rd).unwrap();
    std::fs::create_dir_all(&ed).unwrap();

    // Retail: one module phase, step 2 throughout, frames of 2, 4 (the
    // overrun), 2, 2 and 2 vsyncs. The game clock over those rows is
    // 0 1 | 2 2 3 3 | 4 5 | 6 7 | 8 9, and every grab shows its own row
    // (fixed lag 0) drawn at grey `3 * game time`.
    let frame_of = |t: u64| match t {
        100..=101 => 1,
        102..=105 => 2,
        106..=107 => 3,
        108..=109 => 4,
        _ => 5,
    };
    let game = [0u16, 1, 2, 2, 3, 3, 4, 5, 6, 7, 8, 9];
    let mut meta = String::from("vsync,st7,phase,step,shot,w,h,bpp,frame\n");
    for t in 100..112u64 {
        let name = format!("r_{t:05}.raw");
        let g = game[(t - 100) as usize];
        std::fs::write(rd.join(&name), retail_raw(g * 3)).unwrap();
        writeln!(meta, "{t},53,3,2,{name},320,228,16,{}", frame_of(t)).unwrap();
    }
    std::fs::write(rd.join("meta.csv"), meta).unwrap();

    // Engine: the same phase, one tick a vsync of game time.
    let mut meta = String::from("tick,st7,phase,acting,shot\n");
    for k in 0..10u16 {
        let t = 10 + u64::from(k);
        let name = format!("e_{t:05}.png");
        engine_png(&ed.join(&name), expand(k * 3));
        writeln!(meta, "{t},53,3,0,{name}").unwrap();
    }
    std::fs::write(ed.join("meta.csv"), meta).unwrap();

    let retail = Side::read(&rd).unwrap();
    let engine = Side::read(&ed).unwrap();
    let r = compare("overrun", &retail, &engine, Lag::Fixed(0), |_, _, _| {}).unwrap();
    assert!(r.game_clock);
    assert_eq!(r.pairs.len(), 12);
    for p in &r.pairs {
        assert_eq!(
            p.within, 1.0,
            "vsync {} paired with tick {}",
            p.retail_vsync, p.engine_tick
        );
    }
    let seg = r.segments.iter().find(|s| s.key == Key::Module(3)).unwrap();
    assert_eq!(
        (seg.retail_len, seg.retail_game_len, seg.engine_len),
        (Some(12), Some(10), Some(10))
    );
    // The whole frames between the cut ends: the 4-vsync one ran 2 over.
    assert_eq!((seg.overrun_frames, seg.overrun_vsyncs), (1, 2));
}
