//! Disc-gated level and spectrum check of a one-shot CD-XA cue in the mix.
//!
//! A real arts shout (Vahn's Somersault, `XA2.XA`) goes through the
//! device-free mixer the way the director stages it, at the director's cue
//! gain ([`XA_CUE_GAIN`]), and is measured against the decoded clip:
//!
//! - **level**: retail plays the cue at SPU CD volume `0x75EA` (`0.921`) after
//!   the CD decoder's zig-zag converter, whose coefficient rows pass DC at
//!   about `0.906`, so the mixed RMS sits near `0.835` of the decoded clip's;
//! - **image band** (reported, not asserted): the share of the mix above the
//!   clip's own Nyquist, against a two-point linear interpolation of the same
//!   PCM. Voice has little energy near the top of its band, so on a real clip
//!   both shares are tiny; the converter's image suppression is asserted on a
//!   synthetic tone in `legaia_engine_audio::xa_resample`.
//!
//! The linear reference is computed here, from the decoded PCM, as the shape
//! the mixer had before the converter.
//!
//! Skips (passing) when `LEGAIA_DISC_BIN` is unset, like every disc test.

use std::path::PathBuf;

use legaia_engine_session::audio_director::XA_CUE_GAIN;

fn disc_path() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    let p = PathBuf::from(p);
    p.is_file().then_some(p)
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// Share of `x`'s energy (sampled at 44.1 kHz) above `cut_hz`, from a
/// direct DFT over 2048-sample windows.
fn energy_share_above(x: &[f64], cut_hz: f64) -> f64 {
    const N: usize = 2048;
    let (mut hi, mut all) = (0.0f64, 0.0f64);
    for w in x.as_chunks::<N>().0 {
        for k in 1..N / 2 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (n, &v) in w.iter().enumerate() {
                let ph = 2.0 * std::f64::consts::PI * (k * n) as f64 / N as f64;
                re += v * ph.cos();
                im -= v * ph.sin();
            }
            let p = re * re + im * im;
            all += p;
            if k as f64 * 44_100.0 / N as f64 > cut_hz {
                hi += p;
            }
        }
    }
    hi / all.max(1e-9)
}

#[test]
fn a_cue_mixes_at_retail_cd_volume_through_the_zigzag_converter() {
    let Some(disc) = disc_path() else {
        eprintln!("LEGAIA_DISC_BIN not set - skipping");
        return;
    };
    let mut bank = legaia_engine_session::native::read_arts_shout_bank(&disc)
        .expect("arts-voice shout bank stages from the disc");
    let (_, clip) = bank.shout(0, 0x27).expect("Somersault resolves to a clip");
    let rate = clip.sample_rate;
    let src = clip.pcm.clone();
    assert!(
        rate == 18_900 || rate == 37_800,
        "a CD-XA clip decodes at a CD-XA rate, got {rate}"
    );

    // The mix as the director stages it.
    let mut mix = legaia_engine_audio::OfflineMixer::new(44_100);
    mix.play_xa_shout(src.clone(), rate, legaia_xa::Channels::Mono, XA_CUE_GAIN, 0);
    let out_len = (src.len() as u64 * 44_100 / rate as u64) as usize;
    let mixed: Vec<f64> = (0..out_len).map(|_| mix.next_frame().0 as f64).collect();

    // The pre-converter shape: linear interpolation at unity gain.
    let linear: Vec<f64> = (0..out_len)
        .map(|k| {
            let pos = k as f64 * rate as f64 / 44_100.0;
            let i = (pos as usize).min(src.len() - 1);
            let j = (i + 1).min(src.len() - 1);
            let a = pos - i as f64;
            src[i] as f64 + (src[j] as f64 - src[i] as f64) * a
        })
        .collect();

    // Compare away from the one-shot tail fade.
    let body = ..out_len * 9 / 10;
    let src_f: Vec<f64> = src.iter().map(|&s| s as f64).collect();
    let level_mixed = rms(&mixed[body]) / rms(&src_f);
    let level_linear = rms(&linear[body]) / rms(&src_f);
    let nyquist = rate as f64 / 2.0;
    let image_mixed = energy_share_above(&mixed[body], nyquist);
    let image_linear = energy_share_above(&linear[body], nyquist);
    eprintln!(
        "clip {rate} Hz, {} frames: level mixed {level_mixed:.4} / linear {level_linear:.4}; \
         energy above {nyquist} Hz mixed {image_mixed:.6} / linear {image_linear:.6}",
        src.len()
    );

    assert!(
        (level_mixed - 0.835).abs() < 0.03,
        "cue level {level_mixed:.4} is not CD volume 0x75EA through the converter"
    );
}
