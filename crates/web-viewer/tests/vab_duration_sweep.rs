//! Sweep every VAB sample on the disc, decode each, and report the
//! distribution of audible durations. Used to decide whether the audio
//! page's "VAB samples" preview is worth keeping (it would only be
//! worth keeping if a non-trivial fraction of samples decode to more
//! than a few hundred milliseconds without the SPU's loop+ADSR machinery).
//!
//! Skips when LEGAIA_DISC_BIN is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::audio::{decode_vag_sample, enumerate_vabs};
use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use std::env;
use std::fs;

#[test]
fn vab_sample_duration_distribution() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping VAB sweep");
        return;
    };
    let disc = fs::read(&path).expect("disc");
    let prot = extract_prot_dat(&disc).expect("PROT");
    let entries = parse_prot_toc(&prot).expect("TOC");
    let vabs = enumerate_vabs(&prot, &entries);

    let mut buckets = [0usize; 7]; // <10, 10-25, 25-50, 50-100, 100-250, 250-1000, >=1000 ms
    let mut total = 0usize;
    let mut longest: (u32, u32, u32) = (0, 0, 0); // (prot_index, sample_idx, duration_ms)

    for vab in &vabs {
        for i in 0..vab.sample_count {
            if let Some(pcm) = decode_vag_sample(&prot, &entries, vab.prot_index, vab.vab_offset, i)
            {
                let dur_ms = pcm.len() as u32 * 1000 / 22050;
                total += 1;
                let b = match dur_ms {
                    0..=9 => 0,
                    10..=24 => 1,
                    25..=49 => 2,
                    50..=99 => 3,
                    100..=249 => 4,
                    250..=999 => 5,
                    _ => 6,
                };
                buckets[b] += 1;
                if dur_ms > longest.2 {
                    longest = (vab.prot_index, i, dur_ms);
                }
            }
        }
    }

    let labels = [
        "<10ms",
        "10-25ms",
        "25-50ms",
        "50-100ms",
        "100-250ms",
        "250-1000ms",
        ">=1000ms",
    ];
    eprintln!(
        "[vab-sweep] {total} samples across {} VAB banks",
        vabs.len()
    );
    for (i, label) in labels.iter().enumerate() {
        let pct = if total > 0 {
            buckets[i] as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        eprintln!("[vab-sweep]   {label:>11}: {:6} ({:5.1}%)", buckets[i], pct);
    }
    eprintln!(
        "[vab-sweep] longest: PROT {} sample #{} = {} ms",
        longest.0, longest.1, longest.2
    );
}
