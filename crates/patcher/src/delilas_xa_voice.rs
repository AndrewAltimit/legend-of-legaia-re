//! Fill the muted hero XA voice slots with the mapped siblings' REAL
//! grunts: the siblings' `monster.snd` SPU samples decode to PCM,
//! resample to each channel's CD-XA rate, encode through
//! `legaia_xa::encode` and write over the silenced channels - so a
//! swapped character's arts shouts, swing grunts, staged-event lines
//! and victory barks all speak with the Delilas voice instead of
//! silence.
//!
//! Coverage map per replaced character (slot 0 = Vahn, 1 = Noa,
//! 2 = Gala), grunts cycled per channel so different cues pick
//! different vocalizations:
//!
//! - arts-shout bank (`XA2`/`XA4`/`XA6`): every channel;
//! - staged-event banks (`XA1`+`XA27` / `XA3`+`XA28` / `XA5`+`XA29`):
//!   every channel (item use, Spirit, cut-ins, KO, victory lines);
//! - `XA30` swing-grunt channel (0/4/6): the shortest grunt;
//! - `XA21` victory barks (channels 2/3, 4/5, 6/7): the longest grunt;
//! - `XA20`/`XA22` channel 7 (the close-call victory): the Vahn-slot
//!   sibling's longest grunt.
//!
//! Stereo channels (the staged-event banks and victory barks ship
//! stereo) take a dual-mono encode; only non-4-bit channels are skipped
//! (none ship on the retail disc).

use anyhow::{Context, Result};

use crate::delilas_party::PartyMapping;
use crate::delilas_voice::{MONSTER_SND_ENTRY, monster_vab_offset};
use crate::disc::DiscPatcher;

/// Assumed natural sample rate of the `monster.snd` VAG bodies. The SPU
/// plays them pitch-modulated; 22050 Hz is the audition rate the VAB
/// tooling uses and matches the banks by ear.
const VAG_RATE: u32 = 22050;

/// Peak-normalization target (fraction of i16 full scale).
const PEAK: f64 = 0.60;

/// One sibling's grunt set, decoded and peak-normalized at `VAG_RATE`.
fn sibling_grunts(snd: &[u8], monster_id: u16) -> Result<Vec<Vec<i16>>> {
    let off = monster_vab_offset(snd, monster_id)?;
    let vab = legaia_vab::parse(snd, off).context("parse sibling VAB")?;
    let page = vab
        .tones
        .first()
        .ok_or_else(|| anyhow::anyhow!("sibling VAB has no tone page"))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for tone in page {
        let vag = tone.vag as usize;
        if tone.vol == 0 || vag == 0 || vag > vab.vag_samples.len() || !seen.insert(vag) {
            continue;
        }
        let span = vab.vag_samples[vag - 1];
        let body = &snd[span.byte_offset..span.byte_offset + span.size];
        let mut pcm = legaia_vab::decode_vag_aligned(body).context("decode sibling VAG")?;
        // Peak-normalize: the raw sample level varies per bank and the
        // XA path plays it without the SPU's per-tone volume.
        let peak = pcm.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0);
        if peak > 0 {
            let gain = PEAK * i16::MAX as f64 / peak as f64;
            for s in pcm.iter_mut() {
                *s = ((*s as f64) * gain)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            }
        }
        if pcm.len() > 256 {
            out.push(pcm);
        }
    }
    if out.is_empty() {
        anyhow::bail!("monster id {monster_id}: no audible grunts");
    }
    Ok(out)
}

/// Encode one grunt for a channel's coding and write it at the channel
/// head. Returns `false` (with a note) when the channel is not 4-bit
/// mono.
fn write_grunt(
    patcher: &mut DiscPatcher,
    file: &str,
    chan: u8,
    pcm: &[i16],
    notes: &mut Vec<String>,
) -> Result<bool> {
    let coding = patcher.xa_channel_coding(file, chan)?;
    let stereo = coding & 0x03 != 0;
    if coding & 0x30 != 0 {
        notes.push(format!(
            "{file} channel {chan}: coding {coding:#04x} not 4-bit; left silent"
        ));
        return Ok(false);
    }
    let rate = if (coding >> 2) & 0x03 == 1 {
        18900
    } else {
        37800
    };
    let resampled = legaia_xa::encode::resample_linear(pcm, VAG_RATE, rate);
    let groups = if stereo {
        // Stereo bank, mono source: dual-mono.
        legaia_xa::encode::encode_stereo_4bit_dualmono(&resampled)
    } else {
        legaia_xa::encode::encode_mono_4bit(&resampled)
    };
    patcher
        .write_xa_channel(file, chan, &groups)
        .with_context(|| format!("write {file} channel {chan}"))?;
    Ok(true)
}

/// Fill every muted hero voice slot with the mapped siblings' grunts.
pub fn fill_hero_xa_voices(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let snd = patcher
        .read_entry(MONSTER_SND_ENTRY)
        .context("read monster.snd")?;

    let siblings = [mapping.vahn, mapping.noa, mapping.gala];
    let shout_banks = ["XA/XA2.XA", "XA/XA4.XA", "XA/XA6.XA"];
    let staged_banks = [
        ["XA/XA1.XA", "XA/XA27.XA"],
        ["XA/XA3.XA", "XA/XA28.XA"],
        ["XA/XA5.XA", "XA/XA29.XA"],
    ];
    let swing_chan = [0u8, 4, 6];
    let victory_chans: [[u8; 2]; 3] = [[2, 3], [4, 5], [6, 7]];

    for (slot, sibling) in siblings.iter().enumerate() {
        let grunts = sibling_grunts(&snd, sibling.monster_id())?;
        let longest = grunts
            .iter()
            .max_by_key(|g| g.len())
            .expect("non-empty")
            .clone();
        let shortest = grunts
            .iter()
            .min_by_key(|g| g.len())
            .expect("non-empty")
            .clone();
        let mut filled = 0usize;

        // Arts shouts + staged-event banks: every channel, grunts cycled.
        for file in std::iter::once(shout_banks[slot]).chain(staged_banks[slot]) {
            for (i, chan) in patcher.xa_channels(file)?.into_iter().enumerate() {
                if write_grunt(patcher, file, chan, &grunts[i % grunts.len()], &mut notes)? {
                    filled += 1;
                }
            }
        }
        // Swing grunt.
        if write_grunt(
            patcher,
            "XA/XA30.XA",
            swing_chan[slot],
            &shortest,
            &mut notes,
        )? {
            filled += 1;
        }
        // Victory barks.
        for &chan in &victory_chans[slot] {
            if write_grunt(patcher, "XA/XA21.XA", chan, &longest, &mut notes)? {
                filled += 1;
            }
        }
        notes.push(format!(
            "{}: {} voice channels carry {}'s grunts ({} samples)",
            ["Vahn", "Noa", "Gala"][slot],
            filled,
            sibling.display_name(),
            grunts.len(),
        ));
    }

    // The close-call victory bark (speaker not statically attributable):
    // the Vahn-slot sibling's longest grunt.
    let lead = sibling_grunts(&snd, mapping.vahn.monster_id())?;
    let lead_longest = lead.iter().max_by_key(|g| g.len()).expect("non-empty");
    for file in ["XA/XA20.XA", "XA/XA22.XA"] {
        write_grunt(patcher, file, 7, lead_longest, &mut notes)?;
    }
    Ok(notes)
}
