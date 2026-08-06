//! The FMV ladder: play every retail `fmv_id` through the **retail** STR chain.
//!
//! This is the reach-report ladder the `mdec` crate never had. The existing
//! oracles each drive one layer - `st_ring_real_str` the demuxer, `av_decode_oracle`
//! the [`StrFrameAssembler`](legaia_mdec::str_sector::StrFrameAssembler) +
//! [`MdecDecoder`] pair, `str_player_segment` the play loop over synthetic
//! sectors - and none of them drives the whole chain the way a player reaches
//! it: dispatch slot -> file seek -> [`StrPlayer`] -> [`MdecDecoder`] -> pad
//! skip. That chain is what this file walks, once per retail `fmv_id`, off the
//! user's own disc image.
//!
//! Why it exists as a *ladder* rather than as another oracle: the runtime reach
//! report (`scripts/ci/replay-port-coverage.py`) joins coverage for a union of
//! pad-driven ladders, and no member of that union plays a movie - the headless
//! `engine-shell` ladders have no cutscene rung and the browser play page's FMV
//! arm auto-skips. Every address in `mdec` therefore read *never entered*, which
//! is a fact about the harness rather than about the port.
//!
//! ## What the ladder does and does not prove
//!
//! Each rung is labelled by which retail routine it stands in for. Two of the
//! module's addresses have **no decode-path caller in the port at all** and the
//! ladder does not pretend otherwise:
//!
//! - `FUN_801CF56C` ([`slice_word_count`] / `DecodeEnv::advance_slice`) and
//!   `FUN_801CFD84` ([`mdec_output_control`]) are the MDEC-hardware half of the
//!   play loop. The port decodes a frame whole and presents it as a texture, so
//!   nothing programs the MDEC registers or walks a DMA-0 slice cursor.
//! - Their real host is the `mdec str-plan` subcommand, which lives in a `bin/`
//!   target. [`str_plan_host_walks_the_slice_cursor`] drives that host as a
//!   subprocess rather than re-implementing its walk inline, so what runs under
//!   coverage is the shipped host and not a copy of it.
//!
//! Disc-gated throughout: skip-passes when `LEGAIA_DISC_BIN` is unset, per the
//! repo convention. Nothing here writes disc bytes anywhere.

use legaia_mdec::MdecDecoder;
use legaia_mdec::st_ring::StStatus;
use legaia_mdec::str_player::{
    Bitstream, FmvSlot, PumpIdle, SECTORS_PER_FRAME, SKIP_PAD_MASK, StrPlayer, skip_requested,
};

/// STR/MDEC overlay load base (`docs/formats/str-fmv-table.md`).
const STR_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// FMV dispatch table VA; 23 slots of 32 bytes.
const FMV_TABLE_VA: u32 = 0x801D_0A6C;
const FMV_SLOT_STRIDE: usize = 0x20;
/// PROT entry carrying the STR/MDEC overlay.
const STR_OVERLAY_PROT_INDEX: usize = 970;
/// The nine `fmv_id`s that address a movie on the released disc.
const RETAIL_FMV_IDS: std::ops::RangeInclusive<usize> = 0..=8;

/// One dispatch slot, resolved to something playable.
struct Segment {
    fmv_id: usize,
    /// Bare movie filename, e.g. `MV3.STR`.
    movie: String,
    slot: FmvSlot,
}

fn disc_set() -> bool {
    std::env::var_os("LEGAIA_DISC_BIN").is_some()
}

fn extracted_dir() -> Option<std::path::PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = std::path::PathBuf::from(c);
        if d.join("PROT.DAT").is_file() {
            return Some(d);
        }
    }
    None
}

/// Raw PROT 0970 bytes - the overlay image the dispatch table lives in.
fn str_overlay() -> Option<Vec<u8>> {
    let prot = extracted_dir()?.join("PROT.DAT");
    let mut archive = legaia_prot::archive::Archive::open(&prot).ok()?;
    let entry = archive.entries.get(STR_OVERLAY_PROT_INDEX)?.clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).ok()?;
    Some(buf)
}

/// NUL-terminated printable ASCII at `off`.
fn read_cstr(buf: &[u8], off: usize) -> Option<String> {
    let rest = buf.get(off..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    let s = &rest[..end];
    if s.is_empty() || !s.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return None;
    }
    Some(s.iter().map(|&b| b as char).collect())
}

/// Decode the nine retail dispatch slots straight out of the overlay.
///
/// The slot decode itself is [`FmvSlot::from_record`]; only the `+0x00` path
/// pointer is resolved here, because it is an overlay VA the `mdec` crate
/// deliberately has no way to follow (it does not depend on `legaia-asset`).
fn retail_segments(overlay: &[u8]) -> Vec<Segment> {
    let table_off = (FMV_TABLE_VA - STR_OVERLAY_BASE_VA) as usize;
    let mut out = Vec::new();
    for fmv_id in RETAIL_FMV_IDS {
        let rec_off = table_off + fmv_id * FMV_SLOT_STRIDE;
        let Some(rec) = overlay.get(rec_off..rec_off + FMV_SLOT_STRIDE) else {
            break;
        };
        let rec: &[u8; FMV_SLOT_STRIDE] = rec.try_into().expect("32-byte window");
        let ptr = u32::from_le_bytes(rec[0..4].try_into().unwrap());
        let Some(path) = ptr
            .checked_sub(STR_OVERLAY_BASE_VA)
            .and_then(|o| read_cstr(overlay, o as usize))
        else {
            continue;
        };
        // `\MOV\MV3.STR;1` -> `MV3.STR`
        let movie = path
            .split(';')
            .next()
            .unwrap_or(&path)
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&path)
            .to_string();
        out.push(Segment {
            fmv_id,
            movie,
            slot: FmvSlot::from_record(rec),
        });
    }
    out
}

/// One movie's extent on the disc: `(bare filename, lba, size)`.
type Extent = (String, u32, u32);

/// Movie extents on the user's disc, keyed by bare filename.
fn movie_extents() -> Option<(legaia_iso::raw::RawDisc, Vec<Extent>)> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let path = legaia_iso::raw::resolve_disc_path(std::path::Path::new(&path)).ok()?;
    let mut disc = legaia_iso::raw::RawDisc::open(&path).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut disc).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut disc, &volume.root).ok()?;
    let mut out = Vec::new();
    for (p, rec) in files {
        let upper = p.to_ascii_uppercase();
        if !upper.ends_with(".STR") {
            continue;
        }
        let base = upper
            .split(';')
            .next()
            .unwrap_or(&upper)
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&upper)
            .to_string();
        out.push((base, rec.lba, rec.size));
    }
    Some((disc, out))
}

/// What one played rung observed.
#[derive(Debug, Default)]
struct Played {
    frames: Vec<u32>,
    decoded: usize,
    /// The end-of-stream latch fired on the segment's own `end_frame`.
    reached_end: bool,
    /// Sectors the ring rejected as not belonging to the video stream.
    non_video: usize,
}

/// Play a segment the way `FUN_801CF098` does: seek `(start - 1) * 10` sectors
/// into the file, feed the ring, pump frames, decode the first `decode_budget`
/// of them, and stop at `sector_budget` sectors or at the segment end.
fn play_segment(
    disc: &mut legaia_iso::raw::RawDisc,
    lba: u32,
    size: u32,
    slot: FmvSlot,
    sector_budget: u32,
    decode_budget: usize,
) -> Played {
    let total = size.div_ceil(2048);
    let mut player = StrPlayer::open(slot, Bitstream::Iki);
    // The retail seek: the play loop adds this to the file's start LBA before
    // `CdControl(CdlSetloc)`, and the ring's armed start-frame drop then trims
    // whatever the sector-granular seek overshot.
    let seek = player.seek_sector_offset().max(0) as u32;
    let mut out = Played::default();
    let decoder = MdecDecoder::new(slot.width, slot.height);

    let mut i = seek;
    let end = (seek + sector_budget).min(total);
    while i < end {
        let Ok(sector) = disc.read_sector(lba + i) else {
            break;
        };
        i += 1;
        // Retail's frame poll re-programs the decode rects from the *sector
        // header's* dimensions every frame, so a slot/movie disagreement is
        // resolved in the movie's favour. `StFrame` does not carry them (the
        // port's own disclosed gap), so the header is read here.
        if u16::from_le_bytes([sector[2], sector[3]]) == 0x8001 {
            let w = u16::from_le_bytes([sector[0x10], sector[0x11]]);
            let h = u16::from_le_bytes([sector[0x12], sector[0x13]]);
            if w != 0 && h != 0 {
                player.env_mut().apply_frame_dimensions(w, h);
            }
        }
        let step = player.deliver_sector(&sector);
        match step.status {
            StStatus::NotForStream => out.non_video += 1,
            StStatus::RingFull | StStatus::RingWrapBlocked => {
                panic!("ring stalled at sector {i}: {:?}", step.status)
            }
            _ => {}
        }
        loop {
            match player.next_frame() {
                Ok(frame) => {
                    if frame.is_last {
                        out.reached_end = true;
                    }
                    if out.decoded < decode_budget {
                        let rgba = decoder
                            .decode_frame(&frame.bitstream)
                            .unwrap_or_else(|e| panic!("frame {}: {e}", frame.frame_number));
                        assert_eq!(
                            rgba.len(),
                            slot.width as usize * slot.height as usize * 4,
                            "frame {}: wrong RGBA length",
                            frame.frame_number
                        );
                        let first = &rgba[0..3];
                        assert!(
                            rgba.chunks_exact(4).any(|p| p[0..3] != *first),
                            "frame {}: decoded to a single flat colour",
                            frame.frame_number
                        );
                        out.decoded += 1;
                    }
                    out.frames.push(frame.frame_number);
                }
                Err(PumpIdle::NeedSectors) => break,
                Err(PumpIdle::Finished) => return out,
            }
        }
    }
    out
}

/// Rung 1: every retail `fmv_id` plays through the dispatch slot it is
/// addressed by - `FUN_801CF988` open, `FUN_801CF8B0` decode-env init,
/// `FUN_801CFA14` frame pump, `FUN_801CF740` end latch, `FUN_801D0378` +
/// `FUN_801D0604` bitstream decode.
#[test]
fn every_retail_fmv_id_plays_from_its_dispatch_slot() {
    if !disc_set() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(overlay) = str_overlay() else {
        eprintln!("[skip] extracted/PROT.DAT missing - run `legaia-extract` first");
        return;
    };
    let Some((mut disc, extents)) = movie_extents() else {
        eprintln!("[skip] LEGAIA_DISC_BIN does not resolve to a readable disc image");
        return;
    };
    let segments = retail_segments(&overlay);
    assert_eq!(
        segments.len(),
        9,
        "the nine retail fmv_id slots decode out of the dispatch table"
    );

    // Enough sectors for a few frames each; the decode is software MDEC, so the
    // budget is what keeps the ladder a ladder rather than a transcode.
    const SECTOR_BUDGET: u32 = 12 * SECTORS_PER_FRAME as u32;
    const DECODE_BUDGET: usize = 3;

    let mut played = 0usize;
    for seg in &segments {
        let Some((_, lba, size)) = extents.iter().find(|(b, _, _)| *b == seg.movie) else {
            panic!("fmv {}: {} is not on this disc", seg.fmv_id, seg.movie);
        };
        let out = play_segment(
            &mut disc,
            *lba,
            *size,
            seg.slot,
            SECTOR_BUDGET,
            DECODE_BUDGET,
        );
        assert!(
            !out.frames.is_empty(),
            "fmv {}: {} produced no frames",
            seg.fmv_id,
            seg.movie
        );
        // The armed seek is the whole point of a multi-segment movie: the first
        // frame handed over is the slot's own start frame, not the file's.
        assert_eq!(
            out.frames[0], seg.slot.start_frame,
            "fmv {}: {} started on the wrong frame",
            seg.fmv_id, seg.movie
        );
        assert!(
            out.decoded > 0,
            "fmv {}: no frame reached the MDEC decoder",
            seg.fmv_id
        );
        eprintln!(
            "[fmv {}] {} frames {}..={} ({} decoded, {} non-video sectors)",
            seg.fmv_id,
            seg.movie,
            out.frames[0],
            out.frames[out.frames.len() - 1],
            out.decoded,
            out.non_video
        );
        played += 1;
    }
    assert_eq!(played, 9);
}

/// Rung 2: a segment played to its own `end_frame` latches end-of-stream.
///
/// `FUN_801CF740` raises the latch on the frame whose number *reaches* the
/// slot's `+0x0C`, and the play loop exits only after that frame has been
/// handed over - so the shortest retail segment is the cheap way to see the
/// exit path a movie normally leaves through.
#[test]
fn the_shortest_segment_plays_to_its_end_frame() {
    if !disc_set() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (Some(overlay), Some((mut disc, extents))) = (str_overlay(), movie_extents()) else {
        eprintln!("[skip] disc data unavailable");
        return;
    };
    let segments = retail_segments(&overlay);
    let seg = segments
        .iter()
        .filter(|s| s.slot.end_frame > s.slot.start_frame)
        .min_by_key(|s| s.slot.end_frame - s.slot.start_frame)
        .expect("a bounded retail segment");
    let (_, lba, size) = extents
        .iter()
        .find(|(b, _, _)| *b == seg.movie)
        .expect("segment movie on disc");

    let span = seg.slot.end_frame - seg.slot.start_frame + 1;
    // +2 frames of slack: the sector-granular seek lands at or before the
    // segment's first frame.
    let budget = (span + 2) * SECTORS_PER_FRAME as u32;
    let out = play_segment(&mut disc, *lba, *size, seg.slot, budget, 0);

    assert!(
        out.reached_end,
        "fmv {}: {} frames {}..={} never latched end-of-stream (saw {} frames)",
        seg.fmv_id,
        seg.movie,
        seg.slot.start_frame,
        seg.slot.end_frame,
        out.frames.len()
    );
    assert_eq!(
        *out.frames.last().expect("frames"),
        seg.slot.end_frame,
        "the end frame is inclusive - it is decoded, then playback stops"
    );
    eprintln!(
        "[fmv {}] {} latched end-of-stream on frame {}",
        seg.fmv_id, seg.movie, seg.slot.end_frame
    );
}

/// Rung 3: the pad skip. `FUN_801CF098` consults the pad only while the live
/// `fmv_id` is zero, so the intro aborts and every mid-game movie plays out.
#[test]
fn the_intro_pad_skip_aborts_playback_and_nothing_else_does() {
    if !disc_set() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (Some(overlay), Some((mut disc, extents))) = (str_overlay(), movie_extents()) else {
        eprintln!("[skip] disc data unavailable");
        return;
    };
    let segments = retail_segments(&overlay);
    let intro = segments.first().expect("fmv_id 0");
    let (_, lba, size) = extents
        .iter()
        .find(|(b, _, _)| *b == intro.movie)
        .expect("intro movie on disc");

    // The per-frame pad word: nothing held for the first frames, then the skip
    // chord (`_DAT_8007B850 & 0x1F0`).
    let held = SKIP_PAD_MASK;
    assert!(
        !skip_requested(intro.fmv_id as i16, 0),
        "no button, no skip"
    );
    assert!(
        !skip_requested(1, held),
        "a mid-game fmv_id ignores the pad entirely"
    );

    let mut player = StrPlayer::open(intro.slot, Bitstream::Iki);
    let seek = player.seek_sector_offset().max(0) as u32;
    let total = size.div_ceil(2048);
    let mut pumped = 0usize;
    let mut aborted_at = None;
    let mut i = seek;
    while i < (seek + 200).min(total) {
        let Ok(sector) = disc.read_sector(lba + i) else {
            break;
        };
        i += 1;
        player.deliver_sector(&sector);
        while let Ok(frame) = player.next_frame() {
            pumped += 1;
            // Press the skip chord once a couple of frames are on screen -
            // exactly the beat a player skips the logo crawl at.
            if pumped == 3 && skip_requested(intro.fmv_id as i16, held) {
                player.abort();
                aborted_at = Some(frame.frame_number);
            }
        }
        if player.finished() {
            break;
        }
    }

    let at = aborted_at.expect("the skip chord aborted the intro");
    assert!(player.finished(), "abort ends playback");
    // The pump is closed afterwards, however many sectors still arrive.
    for _ in 0..4 {
        let Ok(sector) = disc.read_sector(lba + i) else {
            break;
        };
        i += 1;
        player.deliver_sector(&sector);
        assert_eq!(player.next_frame().unwrap_err(), PumpIdle::Finished);
    }
    eprintln!("[fmv 0] pad skip aborted the intro on frame {at}");
}

/// Rung 4: the MDEC-hardware half, driven through its **real** host.
///
/// `FUN_801CF56C` (the DMA-0 slice callback) and `FUN_801CFD84` (the MDEC
/// output-control word) have no caller on the port's decode path - the port
/// decodes a frame whole and uploads it as a texture, so there are no slice
/// completions to service and no MDEC registers to program. Their host is the
/// `mdec str-plan` subcommand, and it lives in a `bin/` target that no `#[test]`
/// can call into.
///
/// So the ladder *runs the host*: `CARGO_BIN_EXE_mdec` is the same instrumented
/// binary the coverage build produced, and its profile output is collected with
/// the test's. Re-implementing the walk inline would have entered the same two
/// addresses while proving nothing about the shipped host.
#[test]
fn str_plan_host_walks_the_slice_cursor() {
    if !disc_set() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let movie = extracted.join("MOV").join("MV1.STR");
    if !movie.is_file() {
        eprintln!(
            "[skip] {} missing - run `legaia-extract` first",
            movie.display()
        );
        return;
    }

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mdec"))
        .arg("str-plan")
        .arg(&movie)
        .arg("--colour")
        .arg("--fb-y")
        .arg("8")
        .arg("--start-frame")
        .arg("1")
        .arg("--end-frame")
        .arg("225")
        .arg("--slices")
        .arg("64")
        .output()
        .expect("run the mdec str-plan host");
    assert!(
        out.status.success(),
        "mdec str-plan failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    // The three lines that are the two addresses' output: the MDEC control
    // word, the slice walk, and the per-column word count.
    assert!(
        text.contains("mdec:   control word"),
        "no control word:\n{text}"
    );
    assert!(text.contains("LoadImage"), "no slice walk:\n{text}");
    assert!(
        text.contains("buffer complete, rects flipped"),
        "the walk never flipped a frame buffer:\n{text}"
    );
    assert!(text.contains("words"), "no column word count:\n{text}");
    eprintln!("[str-plan] host walked the slice cursor over MV1.STR");
}
