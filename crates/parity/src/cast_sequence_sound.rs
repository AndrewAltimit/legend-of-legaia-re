//! The cast-sequence oracle's sound channel: which sound cues each side plays
//! through one cast, matched on id and on the action-SM clock
//! (`docs/tooling/cast-sequence-oracle.md`, "The sound channel").
//!
//! Retail's side is the probe's `sfx.csv`: the cue ring `DAT_8007B6D8[4]`
//! with its countdowns `DAT_8007C338[4]`, sampled every vsync. The drainer
//! `FUN_80016B6C` plays a slot whose countdown is zero and whose id is
//! non-negative (`0x80016BF8..0x80016C20`), and the next frame's ager
//! `FUN_8001698C` clears it, so the edge into that condition is one cue. A
//! row whose `drain` column is `0` sat behind the drainer's own gate
//! (`0x8007BABC != 0x8007BAA0`) and was not played; it is left out.
//!
//! The engine's side is the `play-window` recorder's `sfx.csv`: the cues the
//! audio director's command record says came due each tick (`SfxDue` off the
//! delay scheduler, `RingDue` off the retail ring, `SfxNow`).
//!
//! Both ids are descriptor indices: the ring value is the index the drainer
//! reads `DAT_8006F198 + id * 8` with, or the runtime row `id - 0x200`.
//!
//! # Matching
//!
//! A retail cue at vsync `v` is placed into the engine's time the way a frame
//! is ([`crate::cast_sequence`]): the segment holding `v`, its offset on the
//! game clock, the same offset into the aligned engine segment (clamped to
//! its last tick). It then matches the nearest unmatched engine cue of the
//! same id within [`DEFAULT_TOLERANCE`] ticks. A second pass pairs what is
//! left within [`LOOSE_WINDOW`] ticks and reports those as **timing** misses
//! with their offset; the rest are **missing** (retail only) or **extra**
//! (engine only). The channel's number is `2 * matched / (retail + engine)`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cast_sequence::{Key, Segment, Side, align};

/// Ticks either side of the projected time a cue still matches at.
pub const DEFAULT_TOLERANCE: u64 = 6;

/// Ticks a same-id cue is still reported as a timing miss rather than a
/// missing and an extra one.
pub const LOOSE_WINDOW: u64 = 120;

/// One cue on one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    /// Retail vsync or engine tick.
    pub t: u64,
    pub id: i32,
    pub st7: u8,
    pub phase: u8,
}

/// A retail cue and the engine cue it paired with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuePair {
    pub id: i32,
    pub retail_vsync: u64,
    /// Where the retail cue lands in engine ticks.
    pub projected_tick: u64,
    pub engine_tick: u64,
    /// `engine_tick - projected_tick`.
    pub dt: i64,
    pub key: Option<Key>,
}

/// A cue only one side played.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoneCue {
    pub id: i32,
    pub t: u64,
    /// Engine-time projection of a retail cue (`None` on an engine cue, or a
    /// retail cue in a segment the engine never ran).
    pub projected_tick: Option<u64>,
    pub key: Key,
}

/// The sound channel's result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SoundReport {
    pub tolerance: u64,
    pub retail_cues: usize,
    pub engine_cues: usize,
    pub matched: Vec<CuePair>,
    /// Same id, outside the tolerance but within [`LOOSE_WINDOW`].
    pub timing: Vec<CuePair>,
    pub missing: Vec<LoneCue>,
    pub extra: Vec<LoneCue>,
    /// `2 * matched / (retail + engine)`; `1.0` when neither side plays a cue.
    pub score: f64,
    /// The same over the id multisets alone, whatever the time.
    pub id_score: f64,
}

fn read_cues(path: &Path, retail: bool) -> Result<Vec<Cue>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return Ok(Vec::new());
    };
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let col = |k: &str| cols.iter().position(|c| *c == k);
    let (t_col, id_col) = (
        col(if retail { "vsync" } else { "tick" }).context("sfx.csv: no time column")?,
        col("id").context("sfx.csv: no id column")?,
    );
    let (st_col, ph_col, drain_col) = (col("st7"), col("phase"), col("drain"));
    let mut out = Vec::new();
    for l in lines.filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        let num = |i: Option<usize>| i.and_then(|i| f.get(i)).and_then(|v| v.parse::<i64>().ok());
        if num(drain_col) == Some(0) {
            continue;
        }
        let (Some(t), Some(id)) = (num(Some(t_col)), num(Some(id_col))) else {
            continue;
        };
        out.push(Cue {
            t: t as u64,
            id: id as i32,
            st7: num(st_col).unwrap_or(0) as u8,
            phase: num(ph_col).unwrap_or(0) as u8,
        });
    }
    Ok(out)
}

/// Read both sides' `sfx.csv` and match them. `None` when either side has
/// no `sfx.csv` (a recording made before the channel, or an engine run
/// without an audio director).
pub fn compare_dirs(retail: &Side, engine: &Side, tolerance: u64) -> Result<Option<SoundReport>> {
    let (rp, ep) = (retail.dir.join("sfx.csv"), engine.dir.join("sfx.csv"));
    if !rp.exists() || !ep.exists() {
        return Ok(None);
    }
    Ok(Some(compare(
        retail,
        engine,
        &read_cues(&rp, true)?,
        &read_cues(&ep, false)?,
        tolerance,
    )))
}

/// Match two cue lists (see the module docs).
pub fn compare(
    retail: &Side,
    engine: &Side,
    rcues: &[Cue],
    ecues: &[Cue],
    tolerance: u64,
) -> SoundReport {
    // Retail logs from the inject; the window is the engine's, from cast
    // begin (the first retail frame row) to the end of the recording.
    let first = retail
        .rows
        .iter()
        .find(|r| r.shot.is_some())
        .map_or(0, |r| r.t);
    let retail = Side {
        dir: retail.dir.clone(),
        rows: retail
            .rows
            .iter()
            .filter(|r| r.t >= first)
            .cloned()
            .collect(),
    };
    let rlast = retail.rows.last().map_or(0, |r| r.t);
    let elast = engine.rows.last().map_or(0, |r| r.t);
    let efirst = engine.rows.first().map_or(0, |r| r.t);
    let rcues: Vec<Cue> = rcues
        .iter()
        .copied()
        .filter(|c| c.t >= first && c.t <= rlast)
        .collect();
    let ecues: Vec<Cue> = ecues
        .iter()
        .copied()
        .filter(|c| c.t >= efirst && c.t <= elast)
        .collect();

    let rs = retail.segments();
    let es = engine.segments();
    let to_engine: BTreeMap<usize, usize> = align(&rs, &es).into_iter().collect();
    let clock = retail.game_clock();
    let game = |t: u64| -> u64 {
        clock
            .as_ref()
            .and_then(|c| {
                let i = retail.rows.binary_search_by_key(&t, |r| r.t).ok()?;
                c.get(i).copied()
            })
            .unwrap_or(t)
    };
    let seg_of = |segs: &[Segment], t: u64| segs.iter().position(|s| s.start <= t && t <= s.end);
    let project = |t: u64| -> (Key, Option<u64>) {
        let Some(ri) = seg_of(&rs, t) else {
            return (Key::State(0), None);
        };
        let key = rs[ri].key;
        let Some(&ei) = to_engine.get(&ri) else {
            return (key, None);
        };
        let into = game(t).saturating_sub(game(rs[ri].start));
        (key, Some(es[ei].start + into.min(es[ei].len() - 1)))
    };
    let ekey = |t: u64| seg_of(&es, t).map_or(Key::State(0), |i| es[i].key);

    let projected: Vec<(Cue, Key, Option<u64>)> = rcues
        .iter()
        .map(|c| {
            let (k, p) = project(c.t);
            (*c, k, p)
        })
        .collect();
    let mut taken = vec![false; ecues.len()];
    let mut done = vec![false; projected.len()];
    let mut matched = Vec::new();
    let mut timing = Vec::new();
    for (window, out) in [(tolerance, &mut matched), (LOOSE_WINDOW, &mut timing)] {
        for (i, (c, key, p)) in projected.iter().enumerate() {
            let Some(p) = *p else { continue };
            if done[i] {
                continue;
            }
            let best = ecues
                .iter()
                .enumerate()
                .filter(|(j, e)| !taken[*j] && e.id == c.id && e.t.abs_diff(p) <= window)
                .min_by_key(|(_, e)| e.t.abs_diff(p));
            if let Some((j, e)) = best {
                taken[j] = true;
                done[i] = true;
                out.push(CuePair {
                    id: c.id,
                    retail_vsync: c.t,
                    projected_tick: p,
                    engine_tick: e.t,
                    dt: e.t as i64 - p as i64,
                    key: Some(*key),
                });
            }
        }
    }
    let (matched, timing) = uncross(matched, timing, tolerance);
    let missing: Vec<LoneCue> = projected
        .iter()
        .zip(&done)
        .filter(|(_, d)| !**d)
        .map(|((c, key, p), _)| LoneCue {
            id: c.id,
            t: c.t,
            projected_tick: *p,
            key: *key,
        })
        .collect();
    let extra: Vec<LoneCue> = ecues
        .iter()
        .zip(&taken)
        .filter(|(_, t)| !**t)
        .map(|(e, _)| LoneCue {
            id: e.id,
            t: e.t,
            projected_tick: None,
            key: ekey(e.t),
        })
        .collect();
    let f1 = |m: usize, a: usize, b: usize| {
        if a + b == 0 {
            1.0
        } else {
            2.0 * m as f64 / (a + b) as f64
        }
    };
    let mut ids: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    for c in &rcues {
        ids.entry(c.id).or_default().0 += 1;
    }
    for c in &ecues {
        ids.entry(c.id).or_default().1 += 1;
    }
    let id_matched: usize = ids.values().map(|&(a, b)| a.min(b)).sum();
    SoundReport {
        tolerance,
        retail_cues: rcues.len(),
        engine_cues: ecues.len(),
        score: f1(matched.len(), rcues.len(), ecues.len()),
        id_score: f1(id_matched, rcues.len(), ecues.len()),
        matched,
        timing,
        missing,
        extra,
    }
}

/// Re-deal each id's paired engine ticks in time order. Repeats of one id play
/// in order on both sides, but the matcher takes the nearest free tick per
/// retail cue and its two passes run apart, so two cues can pair crossed: `-6`,
/// `-6` read as `+2`, `-14`. In-order pairing never widens the largest gap, so
/// no pair leaves the tolerance by this.
fn uncross(
    matched: Vec<CuePair>,
    timing: Vec<CuePair>,
    tolerance: u64,
) -> (Vec<CuePair>, Vec<CuePair>) {
    let mut by_id: BTreeMap<i32, Vec<CuePair>> = BTreeMap::new();
    for pair in matched.into_iter().chain(timing) {
        by_id.entry(pair.id).or_default().push(pair);
    }
    let (mut matched, mut timing) = (Vec::new(), Vec::new());
    for pairs in by_id.values_mut() {
        pairs.sort_by_key(|p| p.projected_tick);
        let mut ticks: Vec<u64> = pairs.iter().map(|p| p.engine_tick).collect();
        ticks.sort_unstable();
        for (pair, tick) in pairs.iter_mut().zip(ticks) {
            pair.engine_tick = tick;
            pair.dt = tick as i64 - pair.projected_tick as i64;
            if pair.dt.unsigned_abs() <= tolerance {
                matched.push(pair.clone());
            } else {
                timing.push(pair.clone());
            }
        }
    }
    matched.sort_by_key(|p| p.retail_vsync);
    timing.sort_by_key(|p| p.retail_vsync);
    (matched, timing)
}

/// The report's markdown section.
pub fn markdown(s: &SoundReport) -> String {
    let mut out = format!(
        "\n## Sound\n\n**Cue score {:.3}** (id score {:.3}): retail {} cues, engine {}, matched {} within {} ticks, \
         {} same-id timing misses, {} missing, {} extra.\n\n",
        s.score,
        s.id_score,
        s.retail_cues,
        s.engine_cues,
        s.matched.len(),
        s.tolerance,
        s.timing.len(),
        s.missing.len(),
        s.extra.len()
    );
    let id = |id: i32| format!("0x{id:03X}");
    if !s.timing.is_empty() {
        out.push_str("| timing miss | retail vsync | projected tick | engine tick | dt | key |\n|---|---|---|---|---|---|\n");
        for p in &s.timing {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {:+} | {} |\n",
                id(p.id),
                p.retail_vsync,
                p.projected_tick,
                p.engine_tick,
                p.dt,
                p.key.map_or("-".into(), |k| k.to_string())
            ));
        }
        out.push('\n');
    }
    if !s.missing.is_empty() || !s.extra.is_empty() {
        out.push_str("| lone cue | side | time | projected tick | key |\n|---|---|---|---|---|\n");
        for c in &s.missing {
            out.push_str(&format!(
                "| {} | retail | {} | {} | {} |\n",
                id(c.id),
                c.t,
                c.projected_tick.map_or("-".into(), |p| p.to_string()),
                c.key
            ));
        }
        for c in &s.extra {
            out.push_str(&format!(
                "| {} | engine | {} | - | {} |\n",
                id(c.id),
                c.t,
                c.key
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast_sequence::Row;
    use crate::retail_compare_image::{FRAME_H, FRAME_W};
    use std::path::PathBuf;

    fn side(keys: &[(u8, u8, u64)], t0: u64, frame: bool) -> Side {
        let mut rows = Vec::new();
        let mut t = t0;
        let mut f = 0u32;
        for &(st7, phase, len) in keys {
            for _ in 0..len {
                rows.push(Row {
                    t,
                    st7,
                    phase,
                    step: 1,
                    frame: frame.then_some(f),
                    shot: Some(String::new()),
                    geom: (FRAME_W, FRAME_H, 16),
                });
                t += 1;
                f += 1;
            }
        }
        Side {
            dir: PathBuf::new(),
            rows,
        }
    }

    fn cue(t: u64, id: i32) -> Cue {
        Cue {
            t,
            id,
            st7: 0,
            phase: 0,
        }
    }

    /// A cue pairs on its offset into the aligned segment, not on wall time:
    /// retail's extra state ahead of it shifts nothing.
    #[test]
    fn cues_pair_on_the_segment_clock() {
        let r = side(&[(0x28, 0, 10), (0x29, 0, 20), (0x35, 3, 50)], 1000, true);
        let e = side(&[(0x28, 0, 4), (0x35, 3, 50)], 0, true);
        // 12 into module phase 3 on both sides.
        let rc = [cue(1000 + 30 + 12, 0x205)];
        let ec = [cue(4 + 13, 0x205)];
        let s = compare(&r, &e, &rc, &ec, DEFAULT_TOLERANCE);
        assert_eq!(s.matched.len(), 1);
        assert_eq!(s.matched[0].dt, 1);
        assert_eq!(s.score, 1.0);
    }

    /// Two repeats of one id `6` ticks early on both sides: the nearest-free
    /// matcher pairs the first with the second's tick and leaves the second
    /// `14` off. They play in order, so they pair in order.
    #[test]
    fn repeats_of_one_id_pair_in_order() {
        let r = side(&[(0x35, 3, 100)], 1000, true);
        let e = side(&[(0x35, 3, 100)], 0, true);
        let rc = [cue(1000 + 20, 0x16), cue(1000 + 28, 0x16)];
        let ec = [cue(14, 0x16), cue(22, 0x16)];
        let s = compare(&r, &e, &rc, &ec, DEFAULT_TOLERANCE);
        assert_eq!(s.matched.iter().map(|p| p.dt).collect::<Vec<_>>(), [-6, -6]);
        assert!(s.timing.is_empty());
    }

    /// Same id far off is a timing miss; a wrong id is one missing and one
    /// extra cue.
    #[test]
    fn timing_and_lone_cues_are_kept_apart() {
        let r = side(&[(0x28, 0, 200)], 0, true);
        let e = side(&[(0x28, 0, 200)], 0, true);
        let rc = [cue(10, 0x20), cue(50, 0x21)];
        let ec = [cue(40, 0x20), cue(50, 0x22)];
        let s = compare(&r, &e, &rc, &ec, DEFAULT_TOLERANCE);
        assert!(s.matched.is_empty());
        assert_eq!(s.timing.len(), 1);
        assert_eq!(s.timing[0].dt, 30);
        assert_eq!((s.missing.len(), s.extra.len()), (1, 1));
        assert_eq!(s.missing[0].id, 0x21);
        assert_eq!(s.extra[0].id, 0x22);
        assert_eq!(s.score, 0.0);
        assert_eq!(s.id_score, 0.5);
    }

    /// No cue on either side is a perfect score, not a division by zero.
    #[test]
    fn silence_scores_one() {
        let r = side(&[(0x28, 0, 10)], 0, true);
        let s = compare(&r, &r, &[], &[], DEFAULT_TOLERANCE);
        assert_eq!(s.score, 1.0);
    }
}
