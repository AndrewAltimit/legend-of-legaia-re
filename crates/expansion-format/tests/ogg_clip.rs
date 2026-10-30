//! The Ogg Vorbis half of `audio_clip`: a half-second 440 Hz sine, mono at
//! 22050 Hz, encoded with libvorbis and tagged `LOOPSTART=1000`,
//! `LOOPLENGTH=8000` (made with `ffmpeg -f lavfi -i
//! sine=frequency=440:duration=0.5:sample_rate=22050 -c:a libvorbis
//! -metadata LOOPSTART=1000 -metadata LOOPLENGTH=8000`; the project's own
//! tone, no disc audio).

use legaia_expansion_format::audio_clip::{ClipFormat, decode, probe};

const OGG: &[u8] = include_bytes!("fixtures/tone_440.ogg");

#[test]
fn an_ogg_decodes_with_its_comment_loop() {
    assert_eq!(ClipFormat::sniff(OGG), Some(ClipFormat::Ogg));
    let clip = decode(OGG).expect("the Ogg decodes");
    assert_eq!((clip.sample_rate, clip.channels), (22050, 1));
    assert!(
        (11000..=11100).contains(&clip.frames()),
        "{} frames",
        clip.frames()
    );
    assert_eq!(clip.loop_points, Some((1000, 9000)));
    // ffmpeg's sine source plays at 1/8 of full scale.
    assert!((0.11..0.14).contains(&clip.peak()), "peak {}", clip.peak());
    let rms = clip.rms();
    assert!((rms - clip.peak() / 2f32.sqrt()).abs() < 0.01, "rms {rms}");
    // The probe reads the same shape without decoding.
    let info = probe(OGG).expect("the Ogg probes");
    assert_eq!((info.sample_rate, info.channels), (22050, 1));
    assert_eq!(info.frames, clip.frames());
    assert_eq!(info.loop_points, Some((1000, 9000)));
}

#[test]
fn a_truncated_ogg_is_an_error() {
    // A stream cut mid-page decodes what it holds or refuses; never panics.
    let _ = decode(&OGG[..OGG.len() / 3]);
    assert!(decode(&OGG[..40]).is_err());
    assert!(probe(&OGG[..40]).is_err());
}
