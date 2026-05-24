//! Disc-gated test: per-track XA pacing is data-driven, not guessed.
//!
//! Set `LEGAIA_DISC_BIN` to the absolute path of a Mode2/2352 `.bin`. When the
//! var isn't set the test prints a skip notice and returns OK, so it never
//! fails in environments without the disc (CI, others' machines).
//!
//! What it covers: walking the ISO9660 tree finds at least one `*.XA` file,
//! and every demuxed `(file_no, ch_no)` channel reports a real CD-XA sample
//! rate (18 900 or 37 800 Hz) and a 4- or 8-bit width read straight from the
//! per-sector subheaders. This is the invariant that makes the demux path
//! correct where the Form-1 `convert` path has to guess a single global rate.

use std::path::PathBuf;

fn disc_bin_path() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

#[test]
fn every_demuxed_channel_has_a_real_cdxa_rate() {
    let Some(bin) = disc_bin_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping XA demux test");
        return;
    };
    if !bin.exists() {
        panic!("LEGAIA_DISC_BIN={} does not exist", bin.display());
    }

    let files = legaia_xa::demux::demux_disc_all(&bin).expect("demux all XA off the disc");
    let with_audio: Vec<_> = files.iter().filter(|f| !f.streams.is_empty()).collect();
    assert!(
        !with_audio.is_empty(),
        "expected at least one .XA file with Form-2 audio sectors"
    );

    let mut total_channels = 0usize;
    let mut rates_seen = std::collections::BTreeSet::new();
    for f in &with_audio {
        for s in &f.streams {
            assert!(
                s.sample_rate == 18_900 || s.sample_rate == 37_800,
                "{} file{} ch{}: sample rate {} is not a CD-XA rate",
                f.path,
                s.file_no,
                s.ch_no,
                s.sample_rate
            );
            assert!(
                s.bits_per_sample == 4 || s.bits_per_sample == 8,
                "{} file{} ch{}: {} bits/sample is not 4 or 8",
                f.path,
                s.file_no,
                s.ch_no,
                s.bits_per_sample
            );
            assert!(
                s.audio.len().is_multiple_of(legaia_xa::SOUND_GROUP_BYTES),
                "{} file{} ch{}: audio not a whole number of sound groups",
                f.path,
                s.file_no,
                s.ch_no
            );
            rates_seen.insert(s.sample_rate);
            total_channels += 1;
        }
    }
    eprintln!(
        "[ok] {} XA file(s), {} channel(s), rates seen: {:?}",
        with_audio.len(),
        total_channels,
        rates_seen
    );
}
