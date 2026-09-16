//! Disc-gated: the native window's **lazy cast-voice staging** reads one
//! channel span straight off the disc image, the way the director does the
//! first time a cast names a clip (`AudioBgmDirector::play_xa_clip` ->
//! `read_xa_channel_span`).
//!
//! The two live captures are the oracle: PROT 0903 (Gimard) starts
//! `FUN_8003D53C(6, 4, 686)` and PROT 0905 (Vera) `(6, 1, 568)` - `XA7.XA`
//! channels 4 and 1, read spans of 686 and 568 vsyncs. The starter stops the
//! drive `(dur * 150 + 149) / 60` sectors past the file start, so the audio
//! a channel yields over that span is `dur / 60` seconds long to within one
//! of the channel's own sectors - which is what this pins, at the channel's
//! subheader rate and channel layout, whatever they are.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_audio::xa_clip_bank::read_span_sectors;
use legaia_engine_shell::bgm::read_xa_channel_span;
use std::path::PathBuf;

fn disc() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Seconds of audio one channel yields over `dur` vsyncs, and the slack of
/// one of its own sectors, from the decoded clip's own coding.
fn seconds_and_slack(clip: &legaia_engine_audio::XaClip, width: u8, dur: u32) -> (f64, f64, f64) {
    let per_sector = if clip.stereo {
        legaia_engine_audio::xa_clip_bank::STEREO_FRAMES_PER_SECTOR
    } else {
        legaia_engine_audio::xa_clip_bank::MONO_FRAMES_PER_SECTOR
    } as f64;
    let own_sectors = f64::from(read_span_sectors(dur)) / f64::from(width);
    let expected = own_sectors * per_sector / f64::from(clip.sample_rate);
    let got = clip.frames() as f64 / f64::from(clip.sample_rate);
    let slack = per_sector / f64::from(clip.sample_rate);
    (got, expected, slack)
}

#[test]
fn the_two_captured_casts_stage_their_channel_span_off_the_disc() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    for &(slot, channel, dur, who) in &[(6u8, 4u8, 686u32, "Gimard"), (6, 1, 568, "Vera")] {
        let (clip, width) =
            read_xa_channel_span(&disc, slot, channel, dur).expect("XA7.XA channel decodes");
        let (got, expected, slack) = seconds_and_slack(&clip, width, dur);
        eprintln!(
            "[ok] {who}: XA{}.XA ch {channel} span {dur} vsyncs -> {got:.2} s at {} Hz \
             ({}; {width} channels interleaved; expected {expected:.2} s, span {:.2} s)",
            u32::from(slot) + 1,
            clip.sample_rate,
            if clip.stereo { "stereo" } else { "mono" },
            f64::from(dur) / 60.0
        );
        assert!(
            (got - expected).abs() <= slack + 1e-6,
            "{who}: the read span must yield its own share of sectors: got {got:.3} s, \
             expected {expected:.3} s (+-{slack:.3})"
        );
        // The span the dispatcher hands the starter is the clip's length in
        // vsyncs (cast-module.md: the table spans reproduce the demuxed
        // clip lengths on all seven XA7 channels), so the decoded audio
        // covers the span to within one own sector.
        assert!(
            (got - f64::from(dur) / 60.0).abs() <= slack + 0.05,
            "{who}: {got:.2} s of audio for a {:.2} s span",
            f64::from(dur) / 60.0
        );
        let peak = clip.pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(
            peak > 256,
            "{who}: the channel decodes to real audio (peak {peak})"
        );
    }
    // A slot the disc has no file for stays unstaged, silently.
    assert!(read_xa_channel_span(&disc, 0x30, 0, 60).is_none());
}
