//! Key-on census comparison: the engine trace's exact per-frame key-on
//! counts against a retail key-on census CSV.

use super::*;

/// Per-frame **exact** key-on counts off an engine trace: the sum over
/// voices of the [`VoiceTraceFrame::key_ons`] deltas between consecutive
/// frames, so a re-key of a voice that was still sounding counts. Index `i`
/// holds the key-ons that landed between frames `i - 1` and `i`; index `0`
/// is always zero. `None` when any frame lacks the counter (a retail trace,
/// or an engine trace written before the field existed).
pub fn engine_key_on_counts(frames: &[AudioTraceFrame]) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(frames.len());
    let mut prev: Option<&AudioTraceFrame> = None;
    for f in frames {
        let n = match prev {
            None => 0,
            Some(p) => {
                let mut n = 0u32;
                for (a, b) in p.voices.iter().zip(f.voices.iter()) {
                    n = n.wrapping_add(b.key_ons?.wrapping_sub(a.key_ons?));
                }
                n
            }
        };
        if f.voices.iter().any(|v| v.key_ons.is_none()) {
            return None;
        }
        out.push(n);
        prev = Some(f);
    }
    Some(out)
}

/// A retail key-on census, per post-load vsync, as
/// `scripts/pcsx-redux/autorun_keyon_census.lua` logs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyOnCensus {
    /// Score voices keyed per vsync: the voices of every `SpuSetKey` KON
    /// mask the flush wrote, less the SFX-cue voices.
    pub key_ons: Vec<u32>,
    /// Score notes asked for per vsync (`FUN_80066308` calls, SFX cues
    /// excluded). Empty when the census predates the `note` rows.
    pub notes: Vec<u32>,
}

/// Parse the CSV `scripts/pcsx-redux/autorun_keyon_census.lua` writes.
/// Allocator and clock rows, and rows before the capture loop starts (vsync
/// `< 0`), are skipped.
///
/// Only **score** activity counts. A KON row's `records` column names each
/// keyed voice's libsnd note owner (`voice:page/tone/vab/owner`), and owner
/// `0021` is the SFX cue slot (`FUN_80066308`'s `0x21` arm): a field's
/// ambient cues key voices alongside the music, and the engine trace this
/// census is compared with plays the music alone. A KON row without records
/// counts its whole mask; a `note` row with owner `0021` is dropped.
pub fn parse_keyon_census_csv(s: &str) -> Result<KeyOnCensus> {
    parse_keyon_census_csv_owned(s, None)
}

/// [`parse_keyon_census_csv`] restricted to one sequence's notes and voices:
/// `owner` is the libsnd note owner key (`seq | track << 8`, e.g. `0x0001`
/// for the field BGM slot, `0x0003` for a battle or minigame track). A state
/// can hold two scores at once - the Baka Fighter cabinet plays its duel
/// track while the casino floor's is still open - and the engine trace
/// plays one.
pub fn parse_keyon_census_csv_owned(s: &str, owner: Option<u16>) -> Result<KeyOnCensus> {
    let owner_tag = owner.map(|o| format!("{o:04X}"));
    let counts = |own: &str| own != "0021" && owner_tag.as_deref().is_none_or(|t| t == own);
    let mut out = KeyOnCensus::default();
    let bump = |v: &mut Vec<u32>, at: usize, n: u32| {
        if v.len() <= at {
            v.resize(at + 1, 0);
        }
        v[at] += n;
    };
    for (i, line) in s.lines().enumerate().skip(1) {
        let mut cols = line.splitn(5, ',');
        let (Some(vs), Some(mode), Some(mask)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let records = cols.nth(1).unwrap_or("").trim();
        if mode != "1" && mode != "note" {
            continue;
        }
        let vsync: i64 = vs
            .trim()
            .parse()
            .with_context(|| format!("key-on census line {}: vsync {vs:?}", i + 1))?;
        let Ok(v) = usize::try_from(vsync) else {
            continue;
        };
        if mode == "note" {
            // `mask` carries the note's owner key on a note row.
            if counts(mask.trim()) {
                bump(&mut out.notes, v, 1);
            }
            continue;
        }
        let mask = u32::from_str_radix(mask.trim(), 16)
            .with_context(|| format!("key-on census line {}: mask {mask:?}", i + 1))?;
        let n = if records.is_empty() {
            (mask & 0x00FF_FFFF).count_ones()
        } else {
            records
                .split_whitespace()
                .filter(|r| r.rsplit('/').next().is_some_and(counts))
                .count() as u32
        };
        bump(&mut out.key_ons, v, n);
    }
    let n = out.key_ons.len().max(out.notes.len());
    out.key_ons.resize(n, 0);
    if !out.notes.is_empty() {
        out.notes.resize(n, 0);
    }
    Ok(out)
}

/// Engine-vs-retail key-on count over an aligned window, both sides counted
/// exactly (engine: [`engine_key_on_counts`]; retail: the
/// [`parse_keyon_census_csv`] census).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyOnCensusComparison {
    /// Engine frame the window's first vsync lines up with.
    pub offset: usize,
    /// The census vsyncs compared, `first..end`: the span from the first to
    /// the last vsync with score activity. A state captured before its track
    /// starts (a minigame's countdown) or after it stops carries silence the
    /// engine trace, which starts the track at once, has no counterpart for.
    pub window: std::ops::Range<usize>,
    /// Retail key-ons matched one to one with an engine key-on within
    /// [`RETAIL_LAG`].
    pub matched: u32,
    pub engine_total: u32,
    pub retail_total: u32,
    /// `engine_total / retail_total`.
    pub ratio: f64,
}

/// Vsyncs a retail KON may trail (positive) or lead (negative) the engine
/// key-on it is matched with, nearest first. See [`compare_key_on_census`].
pub(super) const RETAIL_LAG: [isize; 5] = [1, 0, 2, 3, -1];
/// The largest trail in [`RETAIL_LAG`]: how far before the window an engine
/// key-on can be claimed.
pub(super) const RETAIL_LAG_MAX: usize = 3;

/// Align a retail key-on census against an engine trace's exact key-on
/// counts and compare key-on totals over the aligned window.
///
/// The alignment is on **timing**, not on sounding pitches: the offset
/// maximises the retail events matched by an engine key-on within one frame
/// (the two clocks quantise a beat onto adjacent vsyncs), exact-frame matches
/// breaking ties. A pitch-Jaccard alignment ([`best_alignment_offset`]) needs
/// a capture of the SPU, and a capture's sounding state is on the host's
/// audio clock.
///
/// Both sides are matched key-on for key-on, one to one, each retail KON
/// taking the nearest unclaimed engine key-on within [`RETAIL_LAG`]. The
/// engine writes a key-on at the flush of the vsync its note fires in;
/// retail's can trail that, as `FUN_8006B854` parks the mask while an SPU
/// transfer is busy (`_DAT_8007AF38 & 1`) - by up to three vsyncs on `rugi`.
/// Matching key-ons (not notes) keeps layered tones, where one note keys two
/// voices on both sides, from penalising the right offset.
///
/// Returns `None` when the engine trace carries no key-on counters or is
/// shorter than the census.
pub fn compare_key_on_census(
    engine: &[AudioTraceFrame],
    census: &KeyOnCensus,
) -> Option<KeyOnCensusComparison> {
    let ek = engine_key_on_counts(engine)?;
    let active = |i: usize| {
        census.key_ons.get(i).copied().unwrap_or(0) > 0
            || census.notes.get(i).copied().unwrap_or(0) > 0
    };
    let len = census.key_ons.len().max(census.notes.len());
    let first = (0..len).find(|&i| active(i))?;
    let end = (0..len).rev().find(|&i| active(i))? + 1;
    let crop = |v: &[u32]| -> Vec<u32> {
        (first..end)
            .map(|i| v.get(i).copied().unwrap_or(0))
            .collect()
    };
    let key_ons = crop(&census.key_ons);
    let series: &[u32] = &key_ons;
    let n = series.len();
    if n == 0 || ek.len() < n {
        return None;
    }
    // One-to-one matching of retail key-ons to engine key-ons, each retail
    // KON taking the nearest unclaimed engine key-on from `RETAIL_LAG`: the
    // engine writes a key-on at the flush of the vsync its note fires in,
    // while retail's KON can trail that by up to three vsyncs when
    // `FUN_8006B854` parks it behind a busy SPU transfer, and either side
    // can land a beat one vsync off the other. Returns (matched, matched
    // with no deferral) - the second breaks score ties.
    let mut claim = vec![0u32; ek.len()];
    let mut matched_at = |off: usize| -> (u32, u32) {
        let lo = off.saturating_sub(RETAIL_LAG_MAX);
        let hi = (off + n + 1).min(ek.len());
        claim[lo..hi].copy_from_slice(&ek[lo..hi]);
        let (mut all, mut prompt) = (0u32, 0u32);
        for (i, &r) in series.iter().enumerate().filter(|(_, r)| **r > 0) {
            let mut want = r;
            for lag in RETAIL_LAG {
                let Some(e) = (off + i).checked_add_signed(-lag) else {
                    continue;
                };
                let Some(have) = claim.get_mut(e) else {
                    continue;
                };
                let take = want.min(*have);
                *have -= take;
                want -= take;
                all += take;
                if (0..=1).contains(&lag) {
                    prompt += take;
                }
                if want == 0 {
                    break;
                }
            }
        }
        (all, prompt)
    };
    // Score each offset by the Jaccard of matched key-ons against both
    // sides' totals, so a dense engine window that merely *contains* every
    // retail event does not outscore the window that plays the same notes
    // and no others.
    let series_total: u32 = series.iter().sum();
    let mut prefix = Vec::with_capacity(ek.len() + 1);
    prefix.push(0u64);
    for &k in &ek {
        prefix.push(prefix.last().copied().unwrap_or(0) + u64::from(k));
    }
    let jaccard = |off: usize, near: u32| -> f64 {
        let window = prefix[off + n] - prefix[off];
        // A matched engine key-on can sit just outside the window, so the
        // union is floored at the larger side.
        let union = (u64::from(series_total) + window)
            .saturating_sub(u64::from(near))
            .max(u64::from(series_total).max(window));
        if union == 0 {
            0.0
        } else {
            f64::from(near) / union as f64
        }
    };
    let mut best: Option<(usize, (u32, u32), f64)> = None;
    for off in 0..=ek.len() - n {
        let m = matched_at(off);
        let score = jaccard(off, m.0);
        // A higher score wins, then more undeferred matches; the earliest
        // offset wins a full tie, so a repeating phrase resolves to its first
        // occurrence.
        let better = match best {
            None => true,
            Some((_, bm, bs)) => score > bs || (score == bs && m.1 > bm.1),
        };
        if better {
            best = Some((off, m, score));
        }
    }
    let (offset, (matched, _), _) = best?;
    let engine_total: u32 = ek[offset..offset + n].iter().sum();
    let retail_total: u32 = key_ons.iter().sum();
    let ratio = if retail_total > 0 {
        engine_total as f64 / retail_total as f64
    } else {
        0.0
    };
    Some(KeyOnCensusComparison {
        offset,
        window: first..end,
        matched,
        engine_total,
        retail_total,
        ratio,
    })
}
