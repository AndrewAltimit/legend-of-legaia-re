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
//! - arts-shout bank (`XA2`/`XA4`/`XA6`): every channel, cycling the
//!   sibling's attack-voice pool ([`art_voice_vags`]);
//! - staged-event banks (`XA1`+`XA27` / `XA3`+`XA28` / `XA5`+`XA29`):
//!   every channel (item use, Spirit, cut-ins, KO, victory lines);
//! - `XA30` swing-grunt channel (0/4/6): the sibling's swing pick
//!   ([`swing_vag_pick`], default shortest);
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

/// Playback rate a tone's pitch attrs imply, from the PsyQ convention:
/// keying the center note plays the sample at 44100 Hz, and these banks
/// key each tone at its own one-note range (`min == max`), so
/// `rate = 44100 * 2^((note - center - shift/128) / 12)`. The sibling
/// banks split into two families: the 44100 voice barks (center == note)
/// and the ~17-22 kHz lines (center 12-16.5 semitones above the keyed
/// note). Exporting or resampling at a flat rate plays the other family
/// an octave off - the "squeak" class of bug.
fn tone_rate(center: u8, shift: u8, note: u8) -> u32 {
    let semis = note as f64 - center as f64 - shift as f64 / 128.0;
    (44100.0 * (semis / 12.0).exp2()).round() as u32
}

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

/// One sibling's victory voice: the raw SPU-ADPCM body (true block grid)
/// plus the pitch attrs of the tone that keys it in the sibling's own
/// bank, so a destination clip can be re-pitched to play it at its
/// recorded rate.
struct VictoryVoice {
    body: Vec<u8>,
    center: u8,
    shift: u8,
    note: u8,
}

/// The vag id (1-based, within the sibling's `monster.snd` bank) of each
/// sibling's victory voice line, picked by ear from the tone-rate-correct
/// audition set. Lu's tag-`0x22` victory entry fires sound cue `1` and her
/// bank keys vags 1-2 as the 44100 Hz bark family (the "humph" heard when
/// the Delilas win their own retail fights - the plasma-strike-tail
/// sibling take); Gi's victory cue byte is `0` (silent) and Che carries no
/// tag-`0x22` entry at all, so theirs are the most victory-like lines from
/// their voice pools.
fn victory_vag_pick(sibling: crate::delilas_party::Sibling) -> usize {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => 2,
        Sibling::Gi => 3,
        Sibling::Che => 2,
    }
}

/// Extract one sibling's victory voice (see [`victory_vag_pick`]) from
/// `monster.snd`.
fn sibling_victory_voice(
    snd: &[u8],
    sibling: crate::delilas_party::Sibling,
) -> Result<VictoryVoice> {
    let off = monster_vab_offset(snd, sibling.monster_id())?;
    let vab = legaia_vab::parse(snd, off).context("parse sibling VAB")?;
    let pick = victory_vag_pick(sibling);
    let tone = vab
        .tones
        .iter()
        .flatten()
        .find(|t| t.vag as usize == pick)
        .ok_or_else(|| anyhow::anyhow!("{sibling:?}: no tone keys vag {pick}"))?;
    let span = *vab
        .vag_samples
        .get(pick - 1)
        .ok_or_else(|| anyhow::anyhow!("{sibling:?}: bank has no vag {pick}"))?;
    // +4: the parser's spans sit one word before the real block grid
    // (documented in `decode_vag_aligned`).
    let body = snd
        .get(span.byte_offset + 4..span.byte_offset + 4 + span.size)
        .ok_or_else(|| anyhow::anyhow!("{sibling:?}: vag {pick} span escapes monster.snd"))?
        .to_vec();
    Ok(VictoryVoice {
        body,
        center: tone.center,
        shift: tone.shift,
        note: tone.min,
    })
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
        let voice = sibling_victory_voice(&snd, *sibling)?;
        let (lo, hi) = VICTORY_CLIP_BANDS[slot];
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
            for (vi, vag) in vab.vag_samples.iter().enumerate() {
                // +4: the parser's spans sit one word before the real
                // block grid (documented in `decode_vag_aligned`).
                let dst = vag.byte_offset + 4..vag.byte_offset + 4 + vag.size;
                if dst.end > span.end {
                    anyhow::bail!("clip {clip:#x}: VAG body escapes the clip span");
                }
                write_victory_body(&mut patched[dst], &voice.body);
                // Re-pitch the destination tone so the body plays at its
                // recorded rate: the clip keys its tone at the tone's own
                // one-note range, so matching the source's note-to-center
                // interval reproduces the source rate exactly. Without
                // this, a body recorded in the other pitch family plays
                // an octave off (the "squeak").
                let (t, dst_tone) = vab
                    .tones
                    .iter()
                    .flatten()
                    .enumerate()
                    .find(|(_, tone)| tone.vag as usize == vi + 1)
                    .ok_or_else(|| {
                        anyhow::anyhow!("clip {clip:#x}: no tone keys vag {}", vi + 1)
                    })?;
                let attr_off = span.start
                    + 4
                    + legaia_vab::VAB_HEADER_SIZE
                    + legaia_vab::PROGRAMS_TABLE_SIZE
                    + t * legaia_vab::TONE_SIZE;
                let center =
                    (voice.center as i32 + dst_tone.min as i32 - voice.note as i32).clamp(0, 127);
                patched[attr_off + 4] = center as u8;
                patched[attr_off + 5] = voice.shift;
            }
        }
        notes.push(format!(
            "{}: victory voice clips {:#x}..={:#x} carry {}'s victory line",
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

/// One decoded sibling voice sample: its 1-based vag id in the bank,
/// peak-normalized PCM, and the playback rate its tone implies
/// (see [`tone_rate`]).
struct Grunt {
    vag: usize,
    pcm: Vec<i16>,
    rate: u32,
}

/// One sibling's grunt set, in bank tone order.
fn sibling_grunts(snd: &[u8], monster_id: u16) -> Result<Vec<Grunt>> {
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
            out.push(Grunt {
                vag,
                pcm,
                rate: tone_rate(tone.center, tone.shift, tone.min),
            });
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
    grunt: &Grunt,
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
    let resampled = legaia_xa::encode::resample_linear(&grunt.pcm, grunt.rate, rate);
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
        // Duration in seconds, not sample count - the pitch families mix
        // 44100 and ~17-22 kHz recordings.
        let secs = |g: &Grunt| g.pcm.len() as f64 / g.rate as f64;
        let longest = grunts
            .iter()
            .max_by(|a, b| secs(a).total_cmp(&secs(b)))
            .expect("non-empty");
        let shortest = grunts
            .iter()
            .min_by(|a, b| secs(a).total_cmp(&secs(b)))
            .expect("non-empty");
        // The pool the ATTACK-side cues cycle over: the sibling's
        // by-ear move-voice picks where known (a hit squeal cycled
        // into an arts-shout channel plays "she got damaged" on her
        // own art), the full bank otherwise.
        let by_vag = |id: usize| grunts.iter().find(|g| g.vag == id);
        let pool: Vec<&Grunt> = match art_voice_vags(*sibling) {
            Some(ids) => ids.iter().filter_map(|&id| by_vag(id)).collect(),
            None => grunts.iter().collect(),
        };
        let pool = if pool.is_empty() {
            grunts.iter().collect::<Vec<_>>()
        } else {
            pool
        };
        let swing = swing_vag_pick(*sibling)
            .and_then(by_vag)
            .unwrap_or(shortest);
        let mut filled = 0usize;

        // Voice casting. When the sibling's samples are ear-labeled
        // ([`voice_cast`]), the channels the battle engine is KNOWN to
        // key get role-matched samples instead of a blind cycle
        // (`docs/subsystems/battle-action.md` § battle voice cues):
        //
        // - arts-shout bank: live in-battle arts observe channels
        //   14/15 (the `(0,0)` pool variant's members) - those carry
        //   the LINE (her long attack call);
        // - fanfare/staged bank 1 (`XA1`-family): channel 1 is the
        //   Super/Miracle cue -> the line; channels 2..=7 are the
        //   Hyper fanfare pairs {2,5} {3,6} {4,7} -> each pair carries
        //   one consistent voice (line / effort / effort); channel 0
        //   (id 0x100, the cast-cue dispatcher's band - Spirit lives
        //   here) -> a composed bark, the focus "hmph";
        // - staged bank 2: minor-event lines (cut-ins, KO, item) ->
        //   barks + efforts cycled, never the big line.
        let cast = voice_cast(*sibling);
        let shout_pick = |i: usize, chan: u8| -> &Grunt {
            if let Some(c) = &cast
                && chan >= 14
                && let Some(g) = by_vag(c.line)
            {
                return g;
            }
            pool[i % pool.len()]
        };
        for (i, chan) in patcher
            .xa_channels(shout_banks[slot])?
            .into_iter()
            .enumerate()
        {
            if write_grunt(
                patcher,
                shout_banks[slot],
                chan,
                shout_pick(i, chan),
                &mut notes,
            )? {
                filled += 1;
            }
        }
        let fanfare_pick = |i: usize, chan: u8| -> &Grunt {
            if let Some(c) = &cast {
                let vag = match chan {
                    0 => Some(c.barks[0]),
                    1 => Some(c.line),
                    2..=7 => Some([c.line, c.efforts[0], c.efforts[1]][(chan as usize - 2) % 3]),
                    _ => None,
                };
                if let Some(g) = vag.and_then(by_vag) {
                    return g;
                }
            }
            pool[i % pool.len()]
        };
        for (i, chan) in patcher
            .xa_channels(staged_banks[slot][0])?
            .into_iter()
            .enumerate()
        {
            if write_grunt(
                patcher,
                staged_banks[slot][0],
                chan,
                fanfare_pick(i, chan),
                &mut notes,
            )? {
                filled += 1;
            }
        }
        let staged2_pick = |i: usize| -> &Grunt {
            if let Some(c) = &cast {
                let ids = [c.barks[0], c.barks[1], c.efforts[0], c.efforts[1]];
                if let Some(g) = by_vag(ids[i % ids.len()]) {
                    return g;
                }
            }
            pool[i % pool.len()]
        };
        for (i, chan) in patcher
            .xa_channels(staged_banks[slot][1])?
            .into_iter()
            .enumerate()
        {
            if write_grunt(
                patcher,
                staged_banks[slot][1],
                chan,
                staged2_pick(i),
                &mut notes,
            )? {
                filled += 1;
            }
        }
        // XA30 short vocals: the anchor channel takes the swing grunt
        // (the per-swing cue arts chain from), the rest cycle the pool.
        for (i, &chan) in xa30_chans[slot].iter().enumerate() {
            let pcm = if i == 0 { swing } else { pool[i % pool.len()] };
            if write_grunt(patcher, "XA/XA30.XA", chan, pcm, &mut notes)? {
                filled += 1;
            }
        }
        // Victory barks.
        for &chan in &victory_chans[slot] {
            if write_grunt(patcher, "XA/XA21.XA", chan, longest, &mut notes)? {
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
    let lead_longest = lead
        .iter()
        .max_by(|a, b| {
            (a.pcm.len() as f64 / a.rate as f64).total_cmp(&(b.pcm.len() as f64 / b.rate as f64))
        })
        .expect("non-empty");
    for file in ["XA/XA20.XA", "XA/XA22.XA"] {
        write_grunt(patcher, file, 7, lead_longest, &mut notes)?;
    }
    Ok(notes)
}

/// By-ear attack-voice vag ids per sibling (bank order, cycled across
/// the arts-shout / staged-event channels). Lu's pool leads with her
/// long special-attack line (vag 3) and her effort barks, and EXCLUDES
/// vag 6 - the short squeal that reads as her damage cry (cycled into
/// an art's shout channel it played "she got hit" at the start of her
/// own somersault). `None` = no ear-confirmed labels yet; cycle the
/// whole bank.
fn art_voice_vags(sibling: crate::delilas_party::Sibling) -> Option<&'static [usize]> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => Some(&[3, 4, 5, 1, 2]),
        Sibling::Gi | Sibling::Che => None,
    }
}

/// The vag carried by the XA30 swing-anchor channel (fires on every
/// ordinary attack swing, so an arts chain opens with it). Default is
/// the sibling's shortest grunt; Lu's shortest is her damage squeal,
/// so she swings with an effort bark instead.
fn swing_vag_pick(sibling: crate::delilas_party::Sibling) -> Option<usize> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => Some(4),
        Sibling::Gi | Sibling::Che => None,
    }
}

/// Ear-labeled voice roles (bank vag ids) driving the semantic channel
/// casting in [`fill_hero_xa_voices`]. `line` = the long attack call
/// (arts / Super / Hyper material), `barks` = the short composed
/// 44100 Hz vocalizations (Spirit focus, minor events), `efforts` =
/// the mid-length attack efforts. `None` = no labels yet; those
/// siblings keep the generic cycle.
struct VoiceCast {
    line: usize,
    barks: [usize; 2],
    efforts: [usize; 2],
}

fn voice_cast(sibling: crate::delilas_party::Sibling) -> Option<VoiceCast> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => Some(VoiceCast {
            line: 3,
            barks: [1, 2],
            efforts: [4, 5],
        }),
        Sibling::Gi | Sibling::Che => None,
    }
}
