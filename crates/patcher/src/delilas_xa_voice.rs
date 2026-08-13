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
//! - `XA20`/`XA22` channel 7 (special-sequence barks): the Vahn-slot
//!   sibling's longest grunt.
//!
//! Stereo channels (the staged-event banks and victory barks ship
//! stereo) take a dual-mono encode; only non-4-bit channels are skipped
//! (none ship on the retail disc).
//!
//! The ORDINARY victory pose's voice is none of the above: it is an
//! SPU sample streamed from `monster.snd` itself - see
//! [`fill_hero_victory_clips`], which replaces those clips with the
//! siblings' grunts (verbatim SPU-ADPCM copy, no re-encode needed).

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

/// Hero victory-voice clip bands inside `monster.snd` (clip id ranges,
/// inclusive), slot order Vahn / Noa / Gala. The battle results
/// sequencer picks a pose action (`0x11..=0x18`, HP-tier table at SCUS
/// `0x800788A0`), maps it to a voice clip byte via the SCUS table at
/// `0x80078867 + action + char_id*8` (clip = byte - 1) and streams that
/// clip's sectors straight out of `monster.snd`'s own sector TOC to the
/// SPU (`FUN_8003e104`, runtime file id 0x37D = this entry). Pinned by
/// recomp capture: a forced weak victory read exactly clip `0xC1`'s
/// sectors at pose time. The bands cover every clip the three heroes'
/// table rows reference plus the in-band gaps (KO/damage siblings).
const VICTORY_CLIP_BANDS: [(usize, usize); 3] = [(0xB8, 0xBC), (0xC4, 0xCB), (0xBD, 0xC3)];

/// One sibling's grunt set as RAW SPU-ADPCM byte ranges within
/// `monster.snd` (same filter as [`sibling_grunts`]), longest first.
fn sibling_grunt_spans(snd: &[u8], monster_id: u16) -> Result<Vec<std::ops::Range<usize>>> {
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
        if span.size > 512 {
            out.push(span.byte_offset..span.byte_offset + span.size);
        }
    }
    if out.is_empty() {
        anyhow::bail!("monster id {monster_id}: no usable grunt spans");
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.len()));
    Ok(out)
}

/// Write one victory voice body over another, both on the TRUE ADPCM
/// block grid. The copied region's interior flags strip to zero (a
/// monster grunt's own END/REPEAT/LOOP flags would stop playback early
/// or loop backward into the body), and the body terminates with
/// retail's own idiom: a silent self-looping block (flags `0x07` =
/// END + REPEAT + LOOP-START), which sustains silence until the
/// sequencer keys the voice off. The rest of the span zero-fills.
fn write_victory_body(dst: &mut [u8], src: &[u8]) {
    let n = (src.len().min(dst.len().saturating_sub(16)) / 16) * 16;
    dst[..n].copy_from_slice(&src[..n]);
    for block in dst[..n].chunks_exact_mut(16) {
        block[1] = 0;
    }
    for b in &mut dst[n..] {
        *b = 0;
    }
    if dst.len() >= n + 16 {
        dst[n + 1] = 0x07;
    }
}

/// Replace the heroes' victory-voice clips in `monster.snd` with the
/// mapped siblings' own grunts. Each clip is a **mini VAB**
/// (`[u32 header_size]["VABp" header + attr tables + VAG size table]
/// [VAG bodies]`) that the results sequencer REGISTERS at victory time,
/// so the header and every table byte stay retail - only the VAG voice
/// bodies swap. NB `legaia_vab::parse` spans are the documented **4
/// bytes before the real ADPCM grid** (see `decode_vag_aligned`'s doc);
/// both source and destination shift `+4` here - writing at the raw
/// span misaligns every block, which reads as random loop/end flags on
/// real SPU hardware and hangs the victory sequence.
pub fn fill_hero_victory_clips(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let snd = patcher
        .read_entry(MONSTER_SND_ENTRY)
        .context("read monster.snd")?;
    let rd32 = |b: &[u8], o: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            b.get(o..o + 4)
                .ok_or_else(|| anyhow::anyhow!("monster.snd TOC truncated"))?
                .try_into()
                .unwrap(),
        ) as usize)
    };
    let count = rd32(&snd, 4)?;
    if count < 0xCE {
        anyhow::bail!("monster.snd TOC has {count} clips, expected >= 0xCE");
    }
    let clip_span = |c: usize| -> Result<std::ops::Range<usize>> {
        let s = rd32(&snd, (c + 2) * 4)? * 2048;
        let e = rd32(&snd, (c + 3) * 4)? * 2048;
        if s >= e || e > snd.len() {
            anyhow::bail!("clip {c:#x}: bad span {s:#x}..{e:#x}");
        }
        Ok(s..e)
    };

    let mut patched = snd.clone();
    let siblings = [mapping.vahn, mapping.noa, mapping.gala];
    for (slot, sibling) in siblings.iter().enumerate() {
        let grunts = sibling_grunt_spans(&snd, sibling.monster_id())?;
        let (lo, hi) = VICTORY_CLIP_BANDS[slot];
        let mut cursor = 0usize;
        for clip in lo..=hi {
            let span = clip_span(clip)?;
            if snd.get(span.start + 4..span.start + 8) != Some(&b"pBAV"[..]) {
                anyhow::bail!("clip {clip:#x}: no VABp header at clip start");
            }
            let vab = legaia_vab::parse(&snd, span.start + 4)
                .with_context(|| format!("parse victory clip {clip:#x} mini VAB"))?;
            if vab.vag_samples.is_empty() {
                anyhow::bail!("clip {clip:#x}: mini VAB has no VAG bodies");
            }
            for vag in &vab.vag_samples {
                // +4: the parser's spans sit one word before the real
                // block grid (documented in `decode_vag_aligned`).
                let dst = vag.byte_offset + 4..vag.byte_offset + 4 + vag.size;
                if dst.end > span.end {
                    anyhow::bail!("clip {clip:#x}: VAG body escapes the clip span");
                }
                let g = grunts[cursor % grunts.len()].clone();
                if g.end + 4 > snd.len() {
                    anyhow::bail!("sibling grunt span escapes monster.snd");
                }
                let src = snd[g.start + 4..g.end + 4].to_vec();
                cursor += 1;
                write_victory_body(&mut patched[dst], &src);
            }
        }
        notes.push(format!(
            "{}: victory voice clips {:#x}..={:#x} carry {}'s grunts",
            ["Vahn", "Noa", "Gala"][slot],
            lo,
            hi,
            sibling.display_name(),
        ));
    }
    patcher
        .patch_prot_entry(MONSTER_SND_ENTRY, 0, &patched)
        .context("write monster.snd victory clips")?;
    Ok(notes)
}

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
    // XA30 is a ten-channel short-vocalization bank; the traced swing
    // site anchors Vahn at 0, Noa at 4, Gala at 6, and the remaining
    // channels hold the same speakers' other short vocals (the weak
    // "barely won" pant among them - it survived every other bank's
    // mute). Grouped around the anchors.
    let xa30_chans: [&[u8]; 3] = [&[0u8, 1, 2, 3], &[4, 5], &[6, 7, 8, 9]];
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
        // XA30 short vocals: the anchor channel takes the shortest
        // grunt (the swing), the rest cycle.
        for (i, &chan) in xa30_chans[slot].iter().enumerate() {
            let pcm = if i == 0 {
                &shortest
            } else {
                &grunts[i % grunts.len()]
            };
            if write_grunt(patcher, "XA/XA30.XA", chan, pcm, &mut notes)? {
                filled += 1;
            }
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
