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
//! - fanfare banks (`XA1`+`XA27` / `XA3`+`XA28` / `XA5`+`XA29`): these
//!   are the Hyper / Super / Miracle cue beds and the Seru-magic
//!   fanfare streams - 3-7 second stereo music-plus-voice, NOT one-shot
//!   lines. They follow the arts-voice mode: retail when `Original`,
//!   silent when `Removed`, and transposed toward the sibling
//!   (pitch/formant only, see [`crate::delilas_voice_fx::fanfare_fx`])
//!   when `Adjusted`. Pasting a quarter-second grunt over one is what
//!   made every Super and Hyper Art silent - a Hyper fires no shout
//!   from the `XA2`/`XA4`/`XA6` pool, so its fanfare is all it has;
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

/// Encode mono PCM into SPU-ADPCM blocks - the exact inverse of
/// `legaia_vab::decode_vag`'s recurrence (prediction
/// `(p1*F0 + p2*F1 + 32) >> 6`, gain `(nibble << 12) >> shift`, low
/// nibble first): per 28-sample block an exhaustive (filter, shift)
/// search minimizes squared error against the real decoder state.
/// Interior flag bytes are zero; the caller places terminal flags.
fn spu_encode(pcm: &[i16]) -> Vec<u8> {
    const F0: [i32; 5] = [0, 60, 115, 98, 122];
    const F1: [i32; 5] = [0, 0, -52, -55, -60];
    let mut out = Vec::with_capacity(pcm.len().div_ceil(28) * 16);
    let (mut p1, mut p2) = (0i32, 0i32);
    for chunk in pcm.chunks(28) {
        let mut best: Option<(u64, usize, i32, [i8; 28], i32, i32)> = None;
        for f in 0..5 {
            for shift in 0..=12i32 {
                let (mut q1, mut q2) = (p1, p2);
                let mut nibs = [0i8; 28];
                let mut err = 0u64;
                for (i, &x) in chunk.iter().enumerate() {
                    let pred = (q1 * F0[f] + q2 * F1[f] + 32) >> 6;
                    let d = x as i32 - pred;
                    let scale = 1i32 << (12 - shift);
                    let n = if d >= 0 {
                        (d + scale / 2) / scale
                    } else {
                        -((-d + scale / 2) / scale)
                    }
                    .clamp(-8, 7);
                    let rec = (((n << 12) >> shift) + pred).clamp(i16::MIN as i32, i16::MAX as i32);
                    err += ((rec - x as i32) * (rec - x as i32)) as u64;
                    nibs[i] = n as i8;
                    q2 = q1;
                    q1 = rec;
                }
                if best.as_ref().map(|b| err < b.0).unwrap_or(true) {
                    best = Some((err, f, shift, nibs, q1, q2));
                }
            }
        }
        let (_, f, shift, nibs, q1, q2) = best.expect("non-empty search");
        p1 = q1;
        p2 = q2;
        let mut block = [0u8; 16];
        block[0] = ((f as u8) << 4) | (shift as u8);
        for i in 0..14 {
            block[2 + i] = (nibs[i * 2] as u8 & 0x0F) | ((nibs[i * 2 + 1] as u8 & 0x0F) << 4);
        }
        out.extend_from_slice(&block);
    }
    out
}

/// Cut the first utterance out of a voice-reel channel: skip leading
/// silence, then stop at the first sustained (0.3 s) quiet gap after
/// 0.2 s of speech; fall back to a 2.5 s cap.
fn first_utterance(pcm: &[i16], rate: u32) -> Vec<i16> {
    let loud = 400i16;
    let quiet = 300i16;
    let start = pcm
        .iter()
        .position(|s| s.unsigned_abs() > loud as u16)
        .unwrap_or(0);
    let min_len = rate as usize / 5;
    let gap_len = rate as usize * 3 / 10;
    let mut quiet_run = 0usize;
    let mut end = (start + rate as usize * 5 / 2).min(pcm.len());
    for (i, s) in pcm.iter().enumerate().skip(start + min_len) {
        if s.unsigned_abs() < quiet as u16 {
            quiet_run += 1;
            if quiet_run >= gap_len {
                end = i - gap_len + rate as usize / 20;
                break;
            }
        } else {
            quiet_run = 0;
        }
        if i >= start + rate as usize * 5 / 2 {
            break;
        }
    }
    pcm[start..end.min(pcm.len())].to_vec()
}

/// XA channels carrying a sibling's OWN victory line in retail - found
/// by ear on the soundboard: the jukebox reel `XA21.XA` channel 6 is
/// Lu's game-over victory bark (the duel bosses' win lines live in the
/// same bank as the heroes'). `None` = no line found yet; the victory
/// voice falls back to the bank sample ([`victory_vag_pick`]).
fn victory_xa_pick(sibling: crate::delilas_party::Sibling) -> Option<(&'static str, u8)> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => Some(("XA/XA21.XA", 6)),
        Sibling::Gi | Sibling::Che => None,
    }
}

/// The sibling's Spirit cue audio, when an XA line beats their own
/// bank. `None` (all siblings today) = the cast-cue channel falls back
/// to the sibling's own bark grunt from the pool - the user found a
/// borrowed sting (Lu wearing Noa's Spirit) odder than her own voice.
fn spirit_xa_pick(sibling: crate::delilas_party::Sibling) -> Option<(&'static str, u8)> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu | Sibling::Gi | Sibling::Che => None,
    }
}

/// The sibling's signature special-attack soundtrack: `XA20.XA` holds
/// the Delilas attack-sequence reels (ear-mapped); channel 2 is Lu's
/// Plasma Strike. It becomes the fanfare audio of the Hyper art the
/// party swap reskins as "Plasma Strike" (Vahn slot: Burning Flare's
/// channel pair 4/7). Capped at 12 s - the fanfare channel span and the
/// cue's duration table bound playback anyway.
fn special_xa_pick(sibling: crate::delilas_party::Sibling) -> Option<(&'static str, u8)> {
    use crate::delilas_party::Sibling;
    match sibling {
        Sibling::Lu => Some(("XA/XA20.XA", 2)),
        Sibling::Gi | Sibling::Che => None,
    }
}

/// Per-slot sibling voice lines, captured from the XA reels BEFORE the
/// voice passes mute them. Index = replaced-hero slot (Vahn, Noa,
/// Gala).
pub struct VictoryLines {
    lines: [Option<(Vec<i16>, u32)>; 3],
    spirit: [Option<(Vec<i16>, u32)>; 3],
    special: [Option<(Vec<i16>, u32)>; 3],
}

impl VictoryLines {
    /// Length in seconds of the special-attack excerpt captured for a
    /// hero slot (0 Vahn / 1 Noa / 2 Gala) - what the fanfare duration
    /// table must cover for the audio to complete.
    pub fn special_secs(&self, slot: usize) -> Option<f64> {
        self.special
            .get(slot)?
            .as_ref()
            .map(|(pcm, rate)| pcm.len() as f64 / *rate as f64)
    }
}

/// Capture every mapped sibling's XA lines (victory bark, Spirit sting,
/// special-attack soundtrack) off the (still retail) image. MUST run
/// before any XA mute touches the reels.
pub fn capture_victory_lines(patcher: &DiscPatcher, mapping: &PartyMapping) -> VictoryLines {
    let siblings = [mapping.vahn, mapping.noa, mapping.gala];
    let mut lines: [Option<(Vec<i16>, u32)>; 3] = [None, None, None];
    let mut spirit: [Option<(Vec<i16>, u32)>; 3] = [None, None, None];
    let mut special: [Option<(Vec<i16>, u32)>; 3] = [None, None, None];
    for (slot, sibling) in siblings.iter().enumerate() {
        if let Some((file, chan)) = victory_xa_pick(*sibling)
            && let Ok((pcm, rate)) = patcher.read_xa_channel_pcm(file, chan)
        {
            let cut = first_utterance(&pcm, rate);
            if cut.len() > rate as usize / 10 {
                lines[slot] = Some((cut, rate));
            }
        }
        if let Some((file, chan)) = spirit_xa_pick(*sibling)
            && let Ok((pcm, rate)) = patcher.read_xa_channel_pcm(file, chan)
            && pcm.len() > rate as usize / 10
        {
            spirit[slot] = Some((pcm, rate));
        }
        if let Some((file, chan)) = special_xa_pick(*sibling)
            && let Ok((mut pcm, rate)) = patcher.read_xa_channel_pcm(file, chan)
        {
            // Cut at the reel's natural end (trailing silence trimmed),
            // capped to the destination fanfare pair's channel capacity
            // (`write_xa_channel` fills only the channel's own sectors,
            // so anything past it is silently dropped - the old flat
            // 12 s cap wrote ~7 s and cut mid-decay). A short fade-out
            // makes the excerpt COMPLETE instead of stopping abruptly.
            let hero_bank = legaia_art::hyper_fanfare::FANFARE_XA_FILE[slot];
            let cap = [4u8, 7]
                .iter()
                .filter_map(|&c| {
                    patcher
                        .read_xa_channel_pcm(&format!("XA/{hero_bank}"), c)
                        .ok()
                        .map(|(p, _)| p.len())
                })
                .min()
                .unwrap_or(rate as usize * 12);
            let win = (rate as usize / 4).max(1);
            let quiet = |w: &[i16]| {
                (w.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / w.len() as f64).sqrt()
                    < 300.0
            };
            let mut end = pcm.len();
            while end > win && quiet(&pcm[end - win..end]) {
                end -= win;
            }
            pcm.truncate(end.min(cap));
            let fade = (rate as usize * 3 / 5).min(pcm.len());
            let n = pcm.len();
            for i in 0..fade {
                let k = (fade - i) as f64 / fade as f64;
                pcm[n - fade + i] = (pcm[n - fade + i] as f64 * k) as i16;
            }
            if pcm.len() > rate as usize / 10 {
                special[slot] = Some((pcm, rate));
            }
        }
    }
    VictoryLines {
        lines,
        spirit,
        special,
    }
}

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
/// or loop backward into the body), and the body terminates with an
/// END-without-REPEAT block (flags `0x01`), which releases the voice's
/// envelope to silence. Retail's own idiom is a silent SELF-LOOPING
/// terminal (`0x07`), but that keeps the voice's envelope OPEN while
/// it loops - retail's bodies fill their clips so the voice is keyed
/// off first, while these bodies are much shorter, and the field-scene
/// load then uploads fresh sample data over the still-looping region:
/// an audible garbage burst right after the victory pose. The mute
/// terminal makes the voice inert the moment the line ends. The rest
/// of the span zero-fills.
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
        dst[n + 1] = 0x01;
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
    lines: &VictoryLines,
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
        // Prefer the sibling's REAL XA victory line (captured pre-mute)
        // over the bank sample; the line is PCM and gets SPU-encoded
        // per clip, resampled down when a clip's span is shorter than
        // the line.
        let line = lines.lines[slot].as_ref();
        let bank_voice = if line.is_none() {
            Some(sibling_victory_voice(&snd, *sibling)?)
        } else {
            None
        };
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
                // Body + the (center, shift) that plays it at its true
                // rate when the clip keys its one-note range.
                let (t, dst_tone) = vab
                    .tones
                    .iter()
                    .flatten()
                    .enumerate()
                    .find(|(_, tone)| tone.vag as usize == vi + 1)
                    .ok_or_else(|| {
                        anyhow::anyhow!("clip {clip:#x}: no tone keys vag {}", vi + 1)
                    })?;
                let (center, shift) = match (line, &bank_voice) {
                    (Some((pcm, rate)), _) => {
                        let cap_blocks = vag.size / 16;
                        let max_samples = cap_blocks.saturating_sub(1) * 28;
                        let (body_pcm, eff_rate) = if max_samples > 0 && pcm.len() > max_samples {
                            let out_rate =
                                (*rate as u64 * max_samples as u64 / pcm.len() as u64) as u32;
                            (
                                legaia_xa::encode::resample_linear(pcm, *rate, out_rate),
                                out_rate,
                            )
                        } else {
                            (pcm.clone(), *rate)
                        };
                        let body = spu_encode(&body_pcm);
                        write_victory_body(&mut patched[dst], &body);
                        // note - (center + shift/128) = 12*log2(rate/44100)
                        let total = dst_tone.min as f64 - 12.0 * (eff_rate as f64 / 44100.0).log2();
                        let mut center = total.floor();
                        let mut shift = ((total - center) * 128.0).round();
                        if shift >= 128.0 {
                            center += 1.0;
                            shift = 0.0;
                        }
                        (center.clamp(0.0, 127.0) as u8, shift as u8)
                    }
                    (None, Some(voice)) => {
                        write_victory_body(&mut patched[dst], &voice.body);
                        let center = (voice.center as i32 + dst_tone.min as i32 - voice.note as i32)
                            .clamp(0, 127) as u8;
                        (center, voice.shift)
                    }
                    (None, None) => unreachable!("one victory source always set"),
                };
                let attr_off = span.start
                    + 4
                    + legaia_vab::VAB_HEADER_SIZE
                    + legaia_vab::PROGRAMS_TABLE_SIZE
                    + t * legaia_vab::TONE_SIZE;
                patched[attr_off + 4] = center;
                patched[attr_off + 5] = shift;
            }
        }
        notes.push(format!(
            "{}: victory voice clips {:#x}..={:#x} carry {}'s {}",
            ["Vahn", "Noa", "Gala"][slot],
            lo,
            hi,
            sibling.display_name(),
            if line.is_some() {
                "real XA victory line"
            } else {
                "bank victory sample"
            },
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

/// The retail arts-shout banks (XA2/XA4/XA6), captured per channel
/// BEFORE the whole-file mutes wipe them - the `adjusted` arts-voice
/// mode re-voices exactly this audio.
pub struct HeroShoutCapture {
    /// Per-art SHOUT banks (XA2/4/6).
    /// `[hero slot][..] = (channel, mono pcm, sample rate)`.
    pub banks: [Vec<(u8, Vec<i16>, u32)>; 3],
    /// Hyper / Super / Miracle **fanfare** banks (XA1/3/5). These are
    /// 3-7 second stereo cue beds, not one-shot voice lines.
    pub fanfare: [Vec<(u8, Vec<i16>, u32)>; 3],
    /// Seru-magic fanfare streams (XA27/28/29).
    pub staged2: [Vec<(u8, Vec<i16>, u32)>; 3],
}

/// Best-effort capture of every retail bank the swap overwrites (a
/// channel that fails to demux is skipped - it was never voiced).
pub fn capture_hero_shouts(patcher: &DiscPatcher) -> HeroShoutCapture {
    let grab = |files: [&str; 3]| -> [Vec<(u8, Vec<i16>, u32)>; 3] {
        let mut out: [Vec<(u8, Vec<i16>, u32)>; 3] = Default::default();
        for (slot, file) in files.iter().enumerate() {
            let Ok(chans) = patcher.xa_channels(file) else {
                continue;
            };
            for chan in chans {
                if let Ok((pcm, rate)) = patcher.read_xa_channel_pcm(file, chan) {
                    out[slot].push((chan, pcm, rate));
                }
            }
        }
        out
    };
    HeroShoutCapture {
        banks: grab(["XA/XA2.XA", "XA/XA4.XA", "XA/XA6.XA"]),
        fanfare: grab(["XA/XA1.XA", "XA/XA3.XA", "XA/XA5.XA"]),
        staged2: grab(["XA/XA27.XA", "XA/XA28.XA", "XA/XA29.XA"]),
    }
}

/// Fill every muted hero voice slot with the mapped siblings' grunts.
/// `arts_voice` picks what the arts-shout banks (XA2/4/6) carry:
/// `Original` leaves them retail (they were never muted), `Removed`
/// keeps them silent, `Adjusted` re-voices the captured retail shouts
/// toward each slot's sibling via [`crate::delilas_voice_fx`].
pub fn fill_hero_xa_voices(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    lines: &VictoryLines,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
    shouts: &HeroShoutCapture,
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

        // Arts-shout bank: what plays when the character calls an art.
        match arts_voice {
            crate::delilas_voice_fx::ArtsVoiceMode::Original => {
                notes.push(format!(
                    "{}: arts shouts left retail (original mode)",
                    ["Vahn", "Noa", "Gala"][slot]
                ));
            }
            crate::delilas_voice_fx::ArtsVoiceMode::Removed => {
                // the bank was muted whole; arts stay voiceless, the
                // spliced SPU grunts remain the attack voice
                notes.push(format!(
                    "{}: arts shouts removed (bank stays silent)",
                    ["Vahn", "Noa", "Gala"][slot]
                ));
            }
            crate::delilas_voice_fx::ArtsVoiceMode::Adjusted => {
                let fx = crate::delilas_voice_fx::voice_map(slot, *sibling);
                let reference = (longest.pcm.as_slice(), longest.rate);
                let mut adjusted = 0usize;
                for (chan, pcm, rate) in &shouts.banks[slot] {
                    let out =
                        crate::delilas_voice_fx::process_channel(pcm, *rate, fx, Some(reference));
                    if out.iter().all(|&s| s == 0) {
                        continue; // channel carried no voiced clip
                    }
                    let g = Grunt {
                        vag: 0,
                        pcm: out,
                        rate: *rate,
                    };
                    if write_grunt(patcher, shout_banks[slot], *chan, &g, &mut notes)? {
                        adjusted += 1;
                        filled += 1;
                    }
                }
                notes.push(format!(
                    "{}: {adjusted} arts-shout channels pitch-mapped toward {} (adjusted mode)",
                    ["Vahn", "Noa", "Gala"][slot],
                    sibling.display_name()
                ));
            }
        }
        let spirit_line = lines.spirit[slot].as_ref().map(|(pcm, rate)| Grunt {
            vag: 0,
            pcm: pcm.clone(),
            rate: *rate,
        });
        let special_line = lines.special[slot].as_ref().map(|(pcm, rate)| Grunt {
            vag: 0,
            pcm: pcm.clone(),
            rate: *rate,
        });
        // XA1/3/5 are the Hyper / Super / Miracle FANFARE banks and
        // XA27/28/29 the Seru-magic fanfare streams: 3-7 second stereo
        // CUE BEDS, not one-shot voice lines (`legaia_art::arts_voice`
        // warns that mis-ID by name). Pasting a quarter-second monster
        // grunt over one leaves 90%+ of the cue window in hard silence -
        // which is exactly what made every Super and Hyper Art silent,
        // and a Hyper has no other audio at all (it fires no shout from
        // the XA2/4/6 pool). So these banks follow the same three-way
        // contract the shout banks do.
        match arts_voice {
            crate::delilas_voice_fx::ArtsVoiceMode::Original => {
                notes.push(format!(
                    "{}: Super/Hyper fanfare left retail (original mode)",
                    ["Vahn", "Noa", "Gala"][slot]
                ));
            }
            crate::delilas_voice_fx::ArtsVoiceMode::Removed => {
                notes.push(format!(
                    "{}: Super/Hyper fanfare removed (banks stay silent)",
                    ["Vahn", "Noa", "Gala"][slot]
                ));
            }
            crate::delilas_voice_fx::ArtsVoiceMode::Adjusted => {
                // A cue bed is music with a voice over it, so it gets
                // transposed - pitch and formant only - rather than run
                // through the timbre/carrier/graft stages that shape a
                // bare shout into someone else's voice.
                let fx = crate::delilas_voice_fx::fanfare_fx(slot, *sibling);
                let mut done = 0usize;
                for (bank, captured) in [
                    (staged_banks[slot][0], &shouts.fanfare[slot]),
                    (staged_banks[slot][1], &shouts.staged2[slot]),
                ] {
                    for (chan, pcm, rate) in captured {
                        // The ear-picked one-shots keep their channels:
                        // Spirit fires through the cast-cue band, and the
                        // reskinned special's soundtrack owns the pair
                        // {4, 7} of the fanfare bank.
                        let picked = if bank != staged_banks[slot][0] {
                            None
                        } else if *chan == 0 {
                            spirit_line.as_ref()
                        } else if *chan == 4 || *chan == 7 {
                            special_line.as_ref()
                        } else {
                            None
                        };
                        if let Some(g) = picked {
                            if write_grunt(patcher, bank, *chan, g, &mut notes)? {
                                filled += 1;
                            }
                            continue;
                        }
                        let out = crate::delilas_voice_fx::process_channel(pcm, *rate, &fx, None);
                        if out.iter().all(|&s| s == 0) {
                            continue;
                        }
                        let g = Grunt {
                            vag: 0,
                            pcm: out,
                            rate: *rate,
                        };
                        if write_grunt(patcher, bank, *chan, &g, &mut notes)? {
                            done += 1;
                            filled += 1;
                        }
                    }
                }
                notes.push(format!(
                    "{}: {done} Super/Hyper fanfare channels re-pitched toward {} (adjusted mode)",
                    ["Vahn", "Noa", "Gala"][slot],
                    sibling.display_name()
                ));
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
        // Victory barks: the sibling's real XA victory line when
        // captured, else the longest grunt.
        let bark_line = lines.lines[slot].as_ref().map(|(pcm, rate)| Grunt {
            vag: 0,
            pcm: pcm.clone(),
            rate: *rate,
        });
        let bark: &Grunt = bark_line.as_ref().unwrap_or(longest);
        for &chan in &victory_chans[slot] {
            if write_grunt(patcher, "XA/XA21.XA", chan, bark, &mut notes)? {
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
    // the Vahn-slot sibling's XA victory line when captured, else their
    // longest grunt.
    let lead = sibling_grunts(&snd, mapping.vahn.monster_id())?;
    let lead_line = lines.lines[0].as_ref().map(|(pcm, rate)| Grunt {
        vag: 0,
        pcm: pcm.clone(),
        rate: *rate,
    });
    let lead_longest = lead
        .iter()
        .max_by(|a, b| {
            (a.pcm.len() as f64 / a.rate as f64).total_cmp(&(b.pcm.len() as f64 / b.rate as f64))
        })
        .expect("non-empty");
    let lead_pick: &Grunt = lead_line.as_ref().unwrap_or(lead_longest);
    for file in ["XA/XA20.XA", "XA/XA22.XA"] {
        write_grunt(patcher, file, 7, lead_pick, &mut notes)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The SPU encoder must round-trip through the real decoder with
    /// speech-grade fidelity and legal block structure.
    #[test]
    fn spu_encode_round_trips_through_the_decoder() {
        // A synthetic voice-ish signal: two tones + an amplitude sweep.
        let n = 28 * 200;
        let pcm: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f64 / 18900.0;
                let env = 1.0 - (i as f64 / n as f64);
                (((t * 440.0 * std::f64::consts::TAU).sin() * 0.5
                    + (t * 1310.0 * std::f64::consts::TAU).sin() * 0.3)
                    * env
                    * 20000.0) as i16
            })
            .collect();
        let body = spu_encode(&pcm);
        assert_eq!(body.len() % 16, 0);
        for block in body.chunks_exact(16) {
            assert!(block[0] >> 4 <= 4, "illegal filter");
            assert!(block[0] & 0x0F <= 12, "illegal shift");
            assert_eq!(block[1], 0, "encoder must leave flags to the caller");
        }
        let decoded = legaia_vab::decode_vag(&body).expect("decodes");
        let m = pcm.len().min(decoded.len());
        assert!(m >= pcm.len() - 28);
        let (mut se, mut sp) = (0f64, 0f64);
        for i in 0..m {
            let d = decoded[i] as f64 - pcm[i] as f64;
            se += d * d;
            sp += (pcm[i] as f64) * (pcm[i] as f64);
        }
        let snr = 10.0 * (sp / se.max(1.0)).log10();
        assert!(snr > 30.0, "SPU encode SNR {snr:.1} dB below speech grade");
    }

    /// The utterance cutter keeps the loud head and drops a long tail.
    #[test]
    fn first_utterance_cuts_at_the_gap() {
        let rate = 18900u32;
        let mut pcm = vec![0i16; rate as usize / 10];
        pcm.extend(std::iter::repeat_n(8000i16, rate as usize / 2));
        pcm.extend(std::iter::repeat_n(0i16, rate as usize));
        pcm.extend(std::iter::repeat_n(8000i16, rate as usize / 2));
        let cut = first_utterance(&pcm, rate);
        let secs = cut.len() as f64 / rate as f64;
        assert!(
            (0.4..=0.9).contains(&secs),
            "cut {secs:.2}s should keep the first burst only"
        );
    }
}
