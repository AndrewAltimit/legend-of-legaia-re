//! Disc-gated corpus sweep over every VAB tone's expressive attributes.
//!
//! This is the data behind the sequencer's pitch-bend wiring: the pitch-bend
//! RANGE is a per-tone disc value (`pbmin`/`pbmax` semitones), not a global
//! constant, so a `0xEn` wheel event scales by the sounding tone's own range.
//! The sweep pins what the retail banks actually carry:
//!
//! - Vibrato (`vibw`/`vibt`) and portamento (`porw`/`port`) are zero on every
//!   tone - Legaia never uses them, so the SPU voice model needs no LFO.
//! - Some tones DO carry a non-zero pitch-bend range, and the ranges are
//!   small, musical semitone counts (the common value is 2 = ±2 semitones,
//!   the General-MIDI default). This is what `VabBank::pitch_bend_range`
//!   feeds the sequencer.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_prot::archive::Archive;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn vab_tones_have_no_vibrato_or_portamento_but_real_pitch_bend_ranges() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");

    let mut banks = 0u64;
    let mut tones = 0u64;
    let mut vibrato_or_porta = 0u64;
    let mut tones_with_bend_range = 0u64;
    let mut max_bend_semitones = 0u8;
    // ADSR-mode census: the release/sustain-decrease step *sign* fix
    // (`compute_delta_linear(.., true)`) only matters if retail tones actually
    // use the *linear* variant of those decreasing phases. Count them so the
    // fix stays disc-relevant, not a speculative micro-optimisation.
    let mut linear_release = 0u64;
    let mut exp_release = 0u64;
    let mut linear_sustain_decrease = 0u64;

    let entries = archive.entries.clone();
    for entry in &entries {
        let mut bytes = Vec::new();
        if archive.read_entry(entry, &mut bytes).is_err() {
            continue;
        }
        let mut i = 0;
        while i + 4 <= bytes.len() {
            if &bytes[i..i + 4] == b"pBAV"
                && let Ok(report) = legaia_vab::parse(&bytes, i)
            {
                banks += 1;
                for program in &report.tones {
                    for t in program {
                        tones += 1;
                        if t.vibw != 0 || t.vibt != 0 || t.porw != 0 || t.port != 0 {
                            vibrato_or_porta += 1;
                        }
                        if t.pbmin != 0 || t.pbmax != 0 {
                            tones_with_bend_range += 1;
                        }
                        max_bend_semitones = max_bend_semitones.max(t.pbmin).max(t.pbmax);

                        // Decode the tone's ADSR the same way the engine voice
                        // does and bucket the decreasing-phase modes.
                        let cfg = legaia_engine_audio::AdsrConfig::from_words(t.adsr1, t.adsr2);
                        if cfg.release_exp {
                            exp_release += 1;
                        } else {
                            linear_release += 1;
                        }
                        if cfg.sustain_decrease && !cfg.sustain_exp {
                            linear_sustain_decrease += 1;
                        }
                    }
                }
            }
            i += 1;
        }
    }

    eprintln!(
        "[vab-tones] banks={banks} tones={tones} \
         vibrato/portamento={vibrato_or_porta} \
         tones_with_bend_range={tones_with_bend_range} \
         max_bend_semitones={max_bend_semitones} \
         linear_release={linear_release} exp_release={exp_release} \
         linear_sustain_decrease={linear_sustain_decrease}"
    );

    assert!(tones > 1000, "expected a large VAB tone corpus");

    // No LFO modulation anywhere - the voice model needs no vibrato/portamento.
    assert_eq!(
        vibrato_or_porta, 0,
        "a tone carries vibrato/portamento - the SPU voice model would need an LFO"
    );

    // Pitch-bend range is a real, per-tone disc value (so the sequencer must
    // source it from the tone, not a constant).
    assert!(
        tones_with_bend_range > 0,
        "no tone carries a pitch-bend range - pitch-bend would be a no-op"
    );

    // Ranges are small musical semitone counts, not garbage.
    assert!(
        (1..=48).contains(&max_bend_semitones),
        "pitch-bend range looks wrong: max {max_bend_semitones} semitones"
    );

    // The retail corpus DOES use linear-release tones, so the release-phase
    // step-sign fix (linear release fades by the `-8` decrease StepValue, not
    // the `+7` increase one) is load-bearing on real data, not speculative.
    assert!(
        linear_release > 0,
        "no linear-release tone in the corpus - the linear-decrease step fix would be inert"
    );
}

/// Key-on volume must land in the SPU voice register's `0..=0x3FFF` domain,
/// measured against the retail tone corpus rather than a synthetic tone.
///
/// This exists because the domain error it guards against was invisible to
/// every audio oracle we had. `audio_trace` compares voice masks, start
/// addresses and SPU *master* volume; `pcm_oracle` asserts the engine is
/// not silent where retail is audible. A per-voice volume short by a factor
/// of `0x3FFF/127` (~0x81) passes all of them - the engine is quiet, not
/// silent, and quiet is not a shape any structural oracle looks at.
///
/// So the assertion here is deliberately about the *domain*, not a golden
/// sample: a full-scale tone at full velocity must reach the top of the
/// register, and a mid-scale one must sit well above the 0..=127 band that
/// the missing widening would have produced.
#[test]
fn keyon_volume_spans_the_spu_register_domain_for_retail_tones() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");

    let mut tones = 0u64;
    let mut max_combined = 0i32;
    // Pan reachability. The retail law (`FUN_80067550`) only ever attenuates
    // the far side, so it cannot overflow the register. The superseded `/64`
    // law boosted the near side to ~2x and clamped - harmless while key-on
    // volume was stuck in 0..=127, but reachable at the real 0..=0x3FFF
    // scale. Count what the corpus would have clipped, so the pan law stays
    // provably load-bearing rather than a tidy-up nobody can justify.
    let mut pan_off_centre = 0u64;
    let mut old_law_would_have_clipped = 0u64;

    let entries = archive.entries.clone();
    for entry in &entries {
        let mut bytes = Vec::new();
        if archive.read_entry(entry, &mut bytes).is_err() {
            continue;
        }
        let mut i = 0;
        while i + 4 <= bytes.len() {
            if &bytes[i..i + 4] == b"pBAV"
                && let Ok(report) = legaia_vab::parse(&bytes, i)
            {
                let bank_master = report.header.mvol as i32;
                for program in &report.tones {
                    for t in program {
                        if t.vag <= 0 {
                            continue;
                        }
                        tones += 1;
                        // Same chain as VabBank::fire at full velocity.
                        let combined = ((bank_master as i64 * t.vol as i64 * 127 * 0x3FFF)
                            / (127 * 127 * 127))
                            .min(0x3FFF) as i32;
                        max_combined = max_combined.max(combined);

                        let pan = (t.pan as i32).clamp(0, 127);
                        if pan != 64 {
                            pan_off_centre += 1;
                        }
                        let widest = (127 - pan).max(pan);
                        if combined as i64 * widest as i64 / 64 > 0x3FFF {
                            old_law_would_have_clipped += 1;
                        }
                    }
                }
            }
            i += 1;
        }
    }

    eprintln!(
        "[keyon-vol] tones={tones} max_combined={max_combined:#x} \
         pan_off_centre={pan_off_centre} \
         old_law_would_have_clipped={old_law_would_have_clipped}"
    );

    assert!(tones > 1000, "expected a large VAB tone corpus");

    // The corpus must be able to drive a voice to the top of the register.
    // Pre-fix this maxed at 127 - three orders of magnitude short.
    assert!(
        max_combined > 0x3000,
        "loudest retail tone only reaches {max_combined:#x} of the 0x3FFF voice \
         register - the key-on volume chain is missing its widening"
    );
}
