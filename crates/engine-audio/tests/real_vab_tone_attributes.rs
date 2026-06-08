//! Disc-gated corpus sweep over every VAB tone's expressive attributes.
//!
//! This is the data behind the sequencer's pitch-bend wiring: the pitch-bend
//! RANGE is a per-tone disc value (`pbmin`/`pbmax` semitones), not a global
//! constant, so a `0xEn` wheel event scales by the sounding tone's own range.
//! The sweep pins what the retail banks actually carry:
//!
//! - Vibrato (`vibw`/`vibt`) and portamento (`porw`/`port`) are zero on every
//!   tone — Legaia never uses them, so the SPU voice model needs no LFO.
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
         max_bend_semitones={max_bend_semitones}"
    );

    assert!(tones > 1000, "expected a large VAB tone corpus");

    // No LFO modulation anywhere — the voice model needs no vibrato/portamento.
    assert_eq!(
        vibrato_or_porta, 0,
        "a tone carries vibrato/portamento — the SPU voice model would need an LFO"
    );

    // Pitch-bend range is a real, per-tone disc value (so the sequencer must
    // source it from the tone, not a constant).
    assert!(
        tones_with_bend_range > 0,
        "no tone carries a pitch-bend range — pitch-bend would be a no-op"
    );

    // Ranges are small musical semitone counts, not garbage.
    assert!(
        (1..=48).contains(&max_bend_semitones),
        "pitch-bend range looks wrong: max {max_bend_semitones} semitones"
    );
}
