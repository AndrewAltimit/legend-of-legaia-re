//! The per-cast frame-sequence oracle: a whole cast scored against retail
//! frame by frame (`docs/tooling/cast-sequence-oracle.md`).
//!
//! The retail comparison corpus scores one frame per save state, and a save
//! state holds one instant of a cast that runs for hundreds of vsyncs. Here
//! both sides record the whole cast instead: retail through the PCSX-Redux
//! probe `autorun_cast_sequence.lua`, the engine through `play-window`'s
//! `LEGAIA_DIAG_CAST_SEQ` recorder. Each side writes a directory holding
//! `meta.csv` (one row per vsync / tick: the action-SM state `ctx[+0x07]`,
//! the module phase `ctx[+0x279]`, the acting seat, retail's frame step and
//! main-loop frame counter) and its frames.
//!
//! # Pairing
//!
//! Frames are paired on the action-SM clock, not on wall time:
//!
//! 1. Each side's rows fold into **segments**, runs of one [`Key`]: the
//!    action state, except inside the slot-B module (`0x35` / `0x36`) and the
//!    capture module (`0x70`), where the key is the module phase - the module
//!    paces its own arms there, and which of the two band states an arm runs
//!    in is a separate observable from what it draws.
//! 2. The two segment lists are aligned by longest common subsequence on
//!    their keys, so a state one side visits and the other skips leaves the
//!    rest aligned.
//! 3. A retail frame grabbed on vsync `v` shows the frame built `lag` vsyncs
//!    earlier (retail double-buffers its packet pools; the displayed frame is
//!    two game frames, `2 * *(0x1F800393)` vsyncs, older than the RAM - see
//!    `retail-compare.md`, "Replayed casts"). It is keyed on the row at
//!    `v - lag`, `k` vsyncs of game time into its segment
//!    ([`Side::game_clock`]: a retail frame that ran past its step counts
//!    its step, not the vsyncs it took), and paired with the engine frame
//!    `k` ticks into the aligned engine segment, clamped to that segment's
//!    end. The engine renders the tick it just ran: no lag.
//!
//! # Score
//!
//! Each pair is scored with the retail-compare frame metric
//! ([`crate::retail_compare_image::score`], `within` = fraction of `8 x 8`
//! blocks within `24`). A pair whose retail frame is near-black (mean luma
//! below [`DARK_LUMA`]) or near-white (above [`WHITE_LUMA`], the summon
//! flash) is reported in its own count and left out of the mean: both sides
//! flashing white scores `1.0` and says nothing about what is under the flash.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::retail_compare_image::{DARK_LUMA, FRAME_H, FRAME_W, Frame, ImageScore, luma, score};

/// Mean luma above which a retail frame is a white flash; the mirror of
/// [`DARK_LUMA`].
pub const WHITE_LUMA: f64 = 230.0;

/// The longest display lag retail shows (`2 *` the largest frame step the
/// band runs on, `4`).
const MAX_LAG: u64 = 8;

/// What a segment is keyed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Key {
    /// An action-SM state outside the module bands.
    State(u8),
    /// Slot-B module phase `ctx[+0x279]`, states `0x35` / `0x36`.
    Module(u8),
    /// Capture-class module phase, state `0x70`.
    Capture(u8),
}

impl Key {
    pub fn of(st7: u8, phase: u8) -> Self {
        match st7 {
            0x35 | 0x36 => Self::Module(phase),
            0x70 => Self::Capture(phase),
            s => Self::State(s),
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::State(s) => write!(f, "state 0x{s:02X}"),
            Self::Module(p) => write!(f, "module phase {p}"),
            Self::Capture(p) => write!(f, "capture phase {p}"),
        }
    }
}

/// One metadata row: a retail vsync or an engine tick.
#[derive(Debug, Clone)]
pub struct Row {
    pub t: u64,
    pub st7: u8,
    pub phase: u8,
    /// Retail's adaptive frame step `*(0x1F800393)`; `0` on the engine side.
    pub step: u8,
    /// Retail's main-loop frame counter `gp+0x480` (`frame` column), when
    /// the probe recorded it: the vsyncs between two values are what one
    /// game frame really took.
    pub frame: Option<u32>,
    /// The frame recorded on this row, if any.
    pub shot: Option<String>,
    /// Pixel geometry of a retail shot (`width`, `height`, `bpp`).
    pub geom: (usize, usize, u8),
}

/// One side's recording.
#[derive(Debug, Clone)]
pub struct Side {
    pub dir: PathBuf,
    pub rows: Vec<Row>,
}

/// A run of one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub key: Key,
    /// First and last row time (inclusive).
    pub start: u64,
    pub end: u64,
}

impl Segment {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
    pub fn is_empty(&self) -> bool {
        false
    }
}

fn parse_csv(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut lines = text.lines();
    let header: Vec<String> = lines
        .next()
        .context("empty meta.csv")?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    Ok(lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            header
                .iter()
                .cloned()
                .zip(l.split(',').map(|s| s.trim().to_string()))
                .collect()
        })
        .collect())
}

fn field<T: std::str::FromStr>(r: &BTreeMap<String, String>, k: &str) -> Option<T> {
    r.get(k).and_then(|v| v.parse().ok())
}

impl Side {
    /// Read a recording directory: the retail probe's (`vsync` column, `r_*.raw`
    /// frames) or the engine recorder's (`tick` column, `e_*.png` frames).
    pub fn read(dir: &Path) -> Result<Self> {
        let recs = parse_csv(&dir.join("meta.csv"))?;
        let mut rows = Vec::with_capacity(recs.len());
        for r in &recs {
            let Some(t) = field::<u64>(r, "vsync").or_else(|| field(r, "tick")) else {
                continue;
            };
            let shot = r.get("shot").filter(|s| !s.is_empty()).cloned();
            rows.push(Row {
                t,
                st7: field(r, "st7").unwrap_or(0),
                phase: field(r, "phase").unwrap_or(0),
                step: field(r, "step").unwrap_or(0),
                frame: field(r, "frame"),
                shot,
                geom: (
                    field(r, "w").unwrap_or(FRAME_W),
                    field(r, "h").unwrap_or(FRAME_H),
                    field(r, "bpp").unwrap_or(16),
                ),
            });
        }
        if rows.is_empty() {
            bail!("{}: no metadata rows", dir.display());
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            rows,
        })
    }

    /// The rows folded into runs of one key.
    pub fn segments(&self) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for r in &self.rows {
            let k = Key::of(r.st7, r.phase);
            match out.last_mut() {
                Some(s) if s.key == k && r.t == s.end + 1 => s.end = r.t,
                _ => out.push(Segment {
                    key: k,
                    start: r.t,
                    end: r.t,
                }),
            }
        }
        out
    }

    /// The **game clock** at every row: vsyncs of game time, the frame step
    /// summed over the game frames run so far.
    ///
    /// A game frame lasts `step` vsyncs when it keeps to the step its timers
    /// drain by, and longer when it overruns: the frame driver's `VSync(step)`
    /// (`0x800171C0`) then returns on the next vsync after the late frame
    /// ends, so the frame took `L > step` vsyncs while every drain still
    /// counted `step`. The game clock advances `step` per frame however long
    /// the frame took, spread evenly over its rows, so a slow retail frame
    /// costs no game time. The engine ticks once a vsync with no overrun, so
    /// its clock is its tick.
    ///
    /// `None` without the `frame` column (the engine, or an older capture).
    pub fn game_clock(&self) -> Option<Vec<u64>> {
        let runs = self.frame_runs()?;
        let mut out = Vec::with_capacity(self.rows.len());
        let mut base = 0u64;
        for (i, j, step) in runs {
            let len = (j - i) as u64;
            let step = u64::from(step);
            for k in 0..len {
                out.push(base + k * step / len);
            }
            base += step;
        }
        Some(out)
    }

    /// The rows cut into game frames: `(first row, end row, step)` per run of
    /// one frame-counter value, `None` without the `frame` column.
    ///
    /// A run is the vsyncs between two counter bumps. The frame driver bumps
    /// the counter at its head (`0x80016BA8`), computes the next step into
    /// `0x1F800393`, waits `VSync(*(0x1F800392))` - the step the frame just
    /// drained by - and only then copies the new step down (`0x800171C0` /
    /// `0x800171D8`). So the wall time between two bumps is the wait of the
    /// frame *before* the run plus this frame's work, and the step it is
    /// paced on is the one the previous run's last row reads.
    fn frame_runs(&self) -> Option<Vec<(usize, usize, u8)>> {
        if self.rows.iter().any(|r| r.frame.is_none()) {
            return None;
        }
        let mut out = Vec::new();
        let mut i = 0;
        let mut paced: Option<u8> = None;
        while i < self.rows.len() {
            let f = self.rows[i].frame;
            let mut j = i;
            while j < self.rows.len()
                && self.rows[j].frame == f
                && self.rows[j].t == self.rows[i].t + (j - i) as u64
            {
                j += 1;
            }
            let own = self.rows[j - 1].step.max(1);
            out.push((i, j, paced.unwrap_or(own)));
            paced = Some(own);
            i = j;
        }
        Some(out)
    }

    /// Each game frame's real length against its step: `(first row time,
    /// vsyncs taken, step)` for every frame whose rows the recording holds
    /// whole. Empty without the `frame` column.
    pub fn frames(&self) -> Vec<(u64, u64, u8)> {
        let Some(runs) = self.frame_runs() else {
            return Vec::new();
        };
        let n = runs.len();
        runs.into_iter()
            .enumerate()
            // The first and last frame may be cut by the recording's ends.
            .filter(|&(k, _)| k > 0 && k + 1 < n)
            .map(|(_, (i, j, step))| (self.rows[i].t, (j - i) as u64, step))
            .collect()
    }

    fn row_at(&self, t: u64) -> Option<&Row> {
        let i = self.rows.binary_search_by_key(&t, |r| r.t).ok()?;
        self.rows.get(i)
    }

    /// Load the frame named on a row.
    pub fn frame(&self, row: &Row) -> Result<Frame> {
        let name = row.shot.as_deref().context("row has no frame")?;
        let path = self.dir.join(name);
        if name.ends_with(".png") {
            return read_png_frame(&path);
        }
        let data = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        retail_raw_frame(&data, row.geom.0, row.geom.1, row.geom.2)
            .with_context(|| format!("decode {}", path.display()))
    }
}

/// A PCSX-Redux `takeScreenShot()` buffer (the displayed area, BGR555 or
/// BGR888) cropped to the compared `320 x 224`: the top rows of the display,
/// as the corpus crops the VRAM display rectangle.
pub fn retail_raw_frame(data: &[u8], w: usize, h: usize, bpp: u8) -> Result<Frame> {
    if w != FRAME_W || h < FRAME_H {
        bail!("display is {w}x{h}, not a {FRAME_W}-wide area with {FRAME_H} rows");
    }
    let mut rgb = Vec::with_capacity(FRAME_W * FRAME_H * 3);
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            if bpp == 24 {
                let o = (y * w + x) * 3;
                let p = data.get(o..o + 3).context("short 24-bit buffer")?;
                rgb.extend_from_slice(&[p[2], p[1], p[0]]);
            } else {
                let o = (y * w + x) * 2;
                let p = data.get(o..o + 2).context("short 16-bit buffer")?;
                let px = legaia_mednafen::bgr555_to_rgba8(u16::from_le_bytes([p[0], p[1]]));
                rgb.extend_from_slice(&px[..3]);
            }
        }
    }
    Ok(Frame { rgb })
}

/// An engine recorder frame: a `320 x 224` RGB / RGBA PNG.
pub fn read_png_frame(path: &Path) -> Result<Frame> {
    let dec = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?,
    ));
    let mut reader = dec.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    if (info.width as usize, info.height as usize) != (FRAME_W, FRAME_H) {
        bail!(
            "{}: {}x{}, not {FRAME_W}x{FRAME_H}",
            path.display(),
            info.width,
            info.height
        );
    }
    let n = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => bail!("unsupported PNG colour type {other:?}"),
    };
    let rgb = buf[..FRAME_W * FRAME_H * n]
        .chunks_exact(n)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    Ok(Frame { rgb })
}

/// Align two segment lists on their keys (longest common subsequence):
/// `(retail index, engine index)` for each aligned pair.
pub fn align(a: &[Segment], b: &[Segment]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i].key == b[j].key {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if a[i].key == b[j].key {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// How a retail frame is shifted back to the row it displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lag {
    /// `2 * frame step` read off the row the frame was grabbed on.
    Auto,
    Fixed(u64),
}

impl std::str::FromStr for Lag {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        if s == "auto" {
            Ok(Self::Auto)
        } else {
            Ok(Self::Fixed(
                s.parse().context("lag is `auto` or a vsync count")?,
            ))
        }
    }
}

/// What a scored pair's retail frame shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameClass {
    Scene,
    White,
    Dark,
}

/// One retail frame and the engine frame paired with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    /// Vsync the retail frame was grabbed on.
    pub retail_vsync: u64,
    /// The row it displays (`retail_vsync - lag`).
    pub shown_vsync: u64,
    pub engine_tick: u64,
    pub key: Key,
    /// How far into the retail segment the shown row sits: game-clock
    /// vsyncs ([`Side::game_clock`]) when the capture carries the frame
    /// counter, wall vsyncs otherwise. The engine frame paired with it is
    /// the same count of ticks into its segment.
    pub into: u64,
    pub retail_st7: u8,
    pub engine_st7: u8,
    pub class: FrameClass,
    pub within: f64,
    pub mae: f64,
    pub retail_luma: f64,
    pub engine_luma: f64,
}

/// One aligned (or unaligned) segment with its timing on both sides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentReport {
    pub key: Key,
    pub retail_len: Option<u64>,
    /// The retail segment's length on the game clock: its wall length less
    /// the vsyncs its slow frames ran past their step.
    #[serde(default)]
    pub retail_game_len: Option<u64>,
    /// Retail game frames inside the segment that took longer than their
    /// step, and the vsyncs they ran over by in all.
    #[serde(default)]
    pub overrun_frames: usize,
    #[serde(default)]
    pub overrun_vsyncs: u64,
    pub engine_len: Option<u64>,
    pub pairs: usize,
    /// Mean `within` over the segment's scene pairs.
    pub mean_within: Option<f64>,
}

/// The whole cast's comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub label: String,
    pub lag: Lag,
    /// Whether retail frames were placed on the game clock
    /// ([`Side::game_clock`]) rather than on wall vsyncs.
    #[serde(default)]
    pub game_clock: bool,
    /// Mean `within` over the scene pairs (neither near-white nor near-black):
    /// the cast's single number.
    pub mean_within: f64,
    pub min_within: f64,
    pub valid_pairs: usize,
    pub white_pairs: usize,
    /// Mean `within` over the white pairs, for watching the flash level.
    pub white_mean_within: Option<f64>,
    pub dark_pairs: usize,
    /// Retail frames with no engine counterpart (a segment the engine never
    /// ran, or a frame shown before the recording began).
    pub unpaired_retail: usize,
    /// Pairs whose two sides sit in different action states under one module
    /// phase key.
    pub state_mismatch_pairs: usize,
    pub retail_frames: usize,
    pub engine_frames: usize,
    pub segments: Vec<SegmentReport>,
    pub pairs: Vec<Pair>,
}

/// Pair the two recordings and score every pair. `keep` receives each
/// scored pair's frames (for the strip), and may drop them.
pub fn compare(
    label: &str,
    retail: &Side,
    engine: &Side,
    lag: Lag,
    mut keep: impl FnMut(&Pair, &Frame, &Frame),
) -> Result<Report> {
    // The retail probe logs from the inject, which can sit an action or two
    // before the cast begins; only the rows its frames can display matter.
    let first = retail
        .rows
        .iter()
        .find(|r| r.shot.is_some())
        .map_or(0, |r| r.t.saturating_sub(MAX_LAG));
    let retail = &Side {
        dir: retail.dir.clone(),
        rows: retail
            .rows
            .iter()
            .filter(|r| r.t >= first)
            .cloned()
            .collect(),
    };
    let rs = retail.segments();
    let es = engine.segments();
    // Retail's game clock, where the capture carries the frame counter: a
    // slow frame (one that ran past its step) then advances the pairing by
    // the step it drained, not by the vsyncs it took, so retail's overrun is
    // not scored as an engine that ran ahead.
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
    let frames = retail.frames();
    let aligned = align(&rs, &es);
    let to_engine: BTreeMap<usize, usize> = aligned.iter().copied().collect();
    let engine_shots: BTreeMap<u64, &Row> = engine
        .rows
        .iter()
        .filter(|r| r.shot.is_some())
        .map(|r| (r.t, r))
        .collect();
    let seg_of = |segs: &[Segment], t: u64| segs.iter().position(|s| s.start <= t && t <= s.end);

    let mut pairs = Vec::new();
    let mut unpaired = 0usize;
    let mut retail_frames = 0usize;
    for row in retail.rows.iter().filter(|r| r.shot.is_some()) {
        retail_frames += 1;
        let l = match lag {
            Lag::Auto => 2 * u64::from(row.step.max(1)),
            Lag::Fixed(n) => n,
        };
        let Some(shown) = row.t.checked_sub(l) else {
            unpaired += 1;
            continue;
        };
        let (Some(shown_row), Some(ri)) = (retail.row_at(shown), seg_of(&rs, shown)) else {
            unpaired += 1;
            continue;
        };
        let Some(&ei) = to_engine.get(&ri) else {
            unpaired += 1;
            continue;
        };
        let (rseg, eseg) = (&rs[ri], &es[ei]);
        let into = game(shown).saturating_sub(game(rseg.start));
        let want = eseg.start + into.min(eseg.len() - 1);
        // The engine frame nearest the wanted tick inside the segment.
        let before = engine_shots.range(eseg.start..=want).next_back();
        let after = engine_shots.range(want..=eseg.end).next();
        let Some((&et, erow)) = (match (before, after) {
            (Some(b), Some(a)) => Some(if want - *b.0 <= *a.0 - want { b } else { a }),
            (b, a) => b.or(a),
        }) else {
            unpaired += 1;
            continue;
        };
        let rf = retail.frame(row)?;
        let ef = engine.frame(erow)?;
        let s: ImageScore = score(&rf, &ef);
        let class = if s.retail_luma < DARK_LUMA {
            FrameClass::Dark
        } else if s.retail_luma > WHITE_LUMA {
            FrameClass::White
        } else {
            FrameClass::Scene
        };
        let p = Pair {
            retail_vsync: row.t,
            shown_vsync: shown,
            engine_tick: et,
            key: rseg.key,
            into,
            retail_st7: shown_row.st7,
            engine_st7: erow.st7,
            class,
            within: s.within,
            mae: s.mae,
            retail_luma: luma(&rf),
            engine_luma: luma(&ef),
        };
        keep(&p, &rf, &ef);
        pairs.push(p);
    }

    let scene: Vec<&Pair> = pairs
        .iter()
        .filter(|p| p.class == FrameClass::Scene)
        .collect();
    let white: Vec<&Pair> = pairs
        .iter()
        .filter(|p| p.class == FrameClass::White)
        .collect();
    let mean = |v: &[&Pair]| {
        (!v.is_empty()).then(|| v.iter().map(|p| p.within).sum::<f64>() / v.len() as f64)
    };

    let mut segments = Vec::new();
    let mut ej = 0usize;
    for (i, s) in rs.iter().enumerate() {
        if let Some(&e) = to_engine.get(&i) {
            // Engine-only segments that fall before this aligned one.
            while ej < e {
                segments.push(SegmentReport {
                    key: es[ej].key,
                    retail_len: None,
                    retail_game_len: None,
                    overrun_frames: 0,
                    overrun_vsyncs: 0,
                    engine_len: Some(es[ej].len()),
                    pairs: 0,
                    mean_within: None,
                });
                ej += 1;
            }
            ej = e + 1;
        }
        let in_seg: Vec<&Pair> = scene
            .iter()
            .copied()
            .filter(|p| s.start <= p.shown_vsync && p.shown_vsync <= s.end)
            .collect();
        let over: Vec<&(u64, u64, u8)> = frames
            .iter()
            .filter(|(t, len, step)| s.start <= *t && *t <= s.end && *len > u64::from(*step))
            .collect();
        segments.push(SegmentReport {
            key: s.key,
            retail_len: Some(s.len()),
            retail_game_len: clock
                .as_ref()
                .map(|_| game(s.end).saturating_sub(game(s.start)) + 1),
            overrun_frames: over.len(),
            overrun_vsyncs: over
                .iter()
                .map(|(_, len, step)| len - u64::from(*step))
                .sum(),
            engine_len: to_engine.get(&i).map(|&e| es[e].len()),
            pairs: pairs
                .iter()
                .filter(|p| s.start <= p.shown_vsync && p.shown_vsync <= s.end)
                .count(),
            mean_within: mean(&in_seg),
        });
    }
    for e in &es[ej.min(es.len())..] {
        segments.push(SegmentReport {
            key: e.key,
            retail_len: None,
            retail_game_len: None,
            overrun_frames: 0,
            overrun_vsyncs: 0,
            engine_len: Some(e.len()),
            pairs: 0,
            mean_within: None,
        });
    }

    Ok(Report {
        label: label.to_string(),
        lag,
        game_clock: clock.is_some(),
        mean_within: mean(&scene).unwrap_or(0.0),
        min_within: scene.iter().map(|p| p.within).fold(1.0, f64::min),
        valid_pairs: scene.len(),
        white_pairs: white.len(),
        white_mean_within: mean(&white),
        dark_pairs: pairs.iter().filter(|p| p.class == FrameClass::Dark).count(),
        unpaired_retail: unpaired,
        state_mismatch_pairs: pairs
            .iter()
            .filter(|p| p.retail_st7 != p.engine_st7)
            .count(),
        retail_frames,
        engine_frames: engine_shots.len(),
        segments,
        pairs,
    })
}

/// The report for a human reader.
pub fn markdown(r: &Report, worst: usize, strip: &[Pair]) -> String {
    let mut s = format!("# Cast sequence: {}\n\n", r.label);
    s.push_str(&format!(
        "**Mean `within` {:.3}** over {} scene pairs (min {:.3}). White-flash pairs: {} (mean {}); \
         dark pairs: {}; retail frames with no engine counterpart: {}; pairs in different action \
         states: {}. Retail frames {}, engine frames {}, display lag {:?}, retail paced on {}.\n\n",
        r.mean_within,
        r.valid_pairs,
        r.min_within,
        r.white_pairs,
        r.white_mean_within
            .map_or("-".into(), |m| format!("{m:.3}")),
        r.dark_pairs,
        r.unpaired_retail,
        r.state_mismatch_pairs,
        r.retail_frames,
        r.engine_frames,
        r.lag,
        if r.game_clock {
            "the game clock"
        } else {
            "wall vsyncs (no frame counter in the capture)"
        },
    ));
    s.push_str("## Segments\n\n`retail game` is the segment on retail's game clock; `slow frames` counts the retail frames that ran past their step and the vsyncs they ran over by.\n\n| key | retail vsyncs | retail game | slow frames | engine ticks | pairs | mean within |\n|---|---|---|---|---|---|---|\n");
    for g in &r.segments {
        let o = |v: Option<u64>| v.map_or("-".to_string(), |v| v.to_string());
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            g.key,
            o(g.retail_len),
            o(g.retail_game_len),
            if g.overrun_frames == 0 {
                "-".to_string()
            } else {
                format!("{} (+{})", g.overrun_frames, g.overrun_vsyncs)
            },
            o(g.engine_len),
            g.pairs,
            g.mean_within.map_or("-".into(), |m| format!("{m:.3}"))
        ));
    }
    let mut w: Vec<&Pair> = r
        .pairs
        .iter()
        .filter(|p| p.class == FrameClass::Scene)
        .collect();
    w.sort_by(|a, b| a.within.total_cmp(&b.within));
    s.push_str("\n## Worst pairs\n\n| retail vsync (shown) | engine tick | key | into | within | mae | luma r/e |\n|---|---|---|---|---|---|---|\n");
    for p in w.iter().take(worst) {
        s.push_str(&pair_row(p));
    }
    if !strip.is_empty() {
        s.push_str("\n## Strip\n\n`strip.png`, one `retail | engine | |diff|` row per pair, top to bottom:\n\n| row | retail vsync (shown) | engine tick | key | into | within | mae | luma r/e |\n|---|---|---|---|---|---|---|---|\n");
        for (i, p) in strip.iter().enumerate() {
            s.push_str(&format!("| {i} {}", pair_row(p)));
        }
    }
    s
}

fn pair_row(p: &Pair) -> String {
    format!(
        "| {} ({}) | {} | {} | {} | {:.3} | {:.1} | {:.0}/{:.0} |\n",
        p.retail_vsync,
        p.shown_vsync,
        p.engine_tick,
        p.key,
        p.into,
        p.within,
        p.mae,
        p.retail_luma,
        p.engine_luma
    )
}

/// The strip's pairs: the worst scene pairs plus evenly spaced ones, in time
/// order.
pub fn strip_pairs(r: &Report, n: usize) -> Vec<Pair> {
    if n == 0 || r.pairs.is_empty() {
        return Vec::new();
    }
    let mut scene: Vec<&Pair> = r
        .pairs
        .iter()
        .filter(|p| p.class == FrameClass::Scene)
        .collect();
    scene.sort_by(|a, b| a.within.total_cmp(&b.within));
    // The worst pair of each segment, worst segments first: a segment the
    // engine gets wrong for its whole length would otherwise fill the strip.
    let mut seen = std::collections::BTreeSet::new();
    let mut pick: BTreeMap<u64, Pair> = scene
        .iter()
        .filter(|p| seen.insert(p.key))
        .take(n / 2)
        .map(|p| (p.retail_vsync, (*p).clone()))
        .collect();
    let all: Vec<&Pair> = r
        .pairs
        .iter()
        .filter(|p| p.class == FrameClass::Scene)
        .collect();
    let step = (all.len() as f64 / (n - pick.len()).max(1) as f64).max(1.0);
    let mut i = 0.0;
    while pick.len() < n && (i as usize) < all.len() {
        let p = all[i as usize];
        pick.entry(p.retail_vsync).or_insert_with(|| p.clone());
        i += step;
    }
    pick.into_values().collect()
}

/// Write `rows` (`retail | engine | |diff|` each) stacked into one PNG.
pub fn write_strip(path: &Path, rows: &[(Frame, Frame)]) -> Result<()> {
    let w = FRAME_W * 3;
    let mut rgb = Vec::with_capacity(w * FRAME_H * rows.len() * 3);
    for (r, e) in rows {
        for y in 0..FRAME_H {
            let span = y * FRAME_W * 3..(y + 1) * FRAME_W * 3;
            let (lr, le) = (&r.rgb[span.clone()], &e.rgb[span]);
            rgb.extend_from_slice(lr);
            rgb.extend_from_slice(le);
            for (a, b) in lr.as_chunks::<3>().0.iter().zip(le.as_chunks::<3>().0) {
                let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
                let v = d.saturating_mul(2);
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        w as u32,
        (FRAME_H * rows.len()) as u32,
    );
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

/// `legaia-engine cast-sequence-score`: pair a retail and an engine cast
/// recording on the action-SM clock and score them frame by frame. Writes
/// `report.md`, `report.json` and `strip.png` under `--out`, which holds
/// retail pixels and must stay out of the repo. See
/// `docs/tooling/cast-sequence-oracle.md`.
#[derive(clap::Args, Debug)]
pub struct CastSequenceArgs {
    /// Retail recording (`autorun_cast_sequence.lua --out-dir`).
    #[arg(long)]
    pub retail: PathBuf,
    /// Engine recording (`play-window` with `LEGAIA_DIAG_CAST_SEQ`).
    #[arg(long)]
    pub engine: PathBuf,
    /// Report directory.
    #[arg(long)]
    pub out: PathBuf,
    /// Report title.
    #[arg(long, default_value = "cast")]
    pub label: String,
    /// Retail display lag in vsyncs, or `auto` (`2 * frame step`).
    #[arg(long, default_value = "auto")]
    pub retail_lag: Lag,
    /// Pairs in `strip.png` (half the worst, half evenly spaced).
    #[arg(long, default_value_t = 12)]
    pub strip: usize,
    /// Worst pairs listed in the report.
    #[arg(long, default_value_t = 10)]
    pub worst: usize,
}

pub fn run(a: CastSequenceArgs) -> Result<()> {
    let retail = Side::read(&a.retail)?;
    let engine = Side::read(&a.engine)?;
    let report = compare(&a.label, &retail, &engine, a.retail_lag, |_, _, _| {})?;
    let strip = strip_pairs(&report, a.strip);
    std::fs::create_dir_all(&a.out)?;
    if !strip.is_empty() {
        let mut rows = Vec::new();
        for p in &strip {
            let r = retail.row_at(p.retail_vsync).context("strip retail row")?;
            let e = engine.row_at(p.engine_tick).context("strip engine row")?;
            rows.push((retail.frame(r)?, engine.frame(e)?));
        }
        write_strip(&a.out.join("strip.png"), &rows)?;
    }
    std::fs::write(
        a.out.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    std::fs::write(a.out.join("report.md"), markdown(&report, a.worst, &strip))?;
    println!(
        "{}: mean within {:.3} over {} scene pairs (min {:.3}); white {} dark {} unpaired {}",
        report.label,
        report.mean_within,
        report.valid_pairs,
        report.min_within,
        report.white_pairs,
        report.dark_pairs,
        report.unpaired_retail
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(key: Key, start: u64, end: u64) -> Segment {
        Segment { key, start, end }
    }

    #[test]
    fn module_states_key_on_the_phase() {
        assert_eq!(Key::of(0x35, 4), Key::of(0x36, 4));
        assert_eq!(Key::of(0x33, 9), Key::State(0x33));
        assert_eq!(Key::of(0x70, 2), Key::Capture(2));
    }

    #[test]
    fn alignment_skips_a_state_one_side_never_visits() {
        let a = [
            seg(Key::State(0x28), 0, 1),
            seg(Key::State(0x29), 2, 5),
            seg(Key::State(0x32), 6, 9),
            seg(Key::Module(1), 10, 20),
        ];
        let b = [
            seg(Key::State(0x28), 0, 0),
            seg(Key::State(0x32), 1, 3),
            seg(Key::Module(1), 4, 4),
        ];
        assert_eq!(align(&a, &b), vec![(0, 0), (2, 1), (3, 2)]);
    }

    fn side(frames: &[(u32, u8, u64)]) -> Side {
        // (frame counter, step, vsyncs the frame took)
        let mut rows = Vec::new();
        let mut t = 100;
        for &(f, step, len) in frames {
            for _ in 0..len {
                rows.push(Row {
                    t,
                    st7: 0x35,
                    phase: 5,
                    step,
                    frame: Some(f),
                    shot: None,
                    geom: (FRAME_W, FRAME_H, 16),
                });
                t += 1;
            }
        }
        Side {
            dir: PathBuf::new(),
            rows,
        }
    }

    /// A frame that ran past its step adds its step, not its length, to the
    /// game clock; one that kept to its step adds what it took.
    #[test]
    fn game_clock_counts_the_step_not_the_overrun() {
        // Each run is paced on the step the run before it reads.
        let s = side(&[(1, 2, 2), (2, 4, 4), (3, 4, 5), (4, 4, 3), (5, 1, 4)]);
        let c = s.game_clock().unwrap();
        // run 1: own step 2 over 2; run 2: paced 2 over 4; run 3: 4 over 5;
        // run 4: 4 over 3 (the catch-up after an overrun); run 5: 4 over 4.
        assert_eq!(
            c,
            vec![0, 1, 2, 2, 3, 3, 4, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15]
        );
        // The whole frames between the cut ends, with the step they ran on.
        assert_eq!(s.frames(), vec![(102, 4, 2), (106, 5, 4), (111, 3, 4)]);
    }

    /// No frame counter, no game clock: the pairing stays on wall vsyncs.
    #[test]
    fn game_clock_needs_the_frame_column() {
        let mut s = side(&[(1, 2, 2), (2, 2, 2)]);
        s.rows[1].frame = None;
        assert!(s.game_clock().is_none());
        assert!(s.frames().is_empty());
    }

    #[test]
    fn retail_raw_crops_the_top_rows() {
        // 320 x 228, BGR555: row 0 pure red, the rest black.
        let mut d = vec![0u8; 320 * 228 * 2];
        for x in 0..320 {
            d[x * 2..x * 2 + 2].copy_from_slice(&0x001Fu16.to_le_bytes());
        }
        let f = retail_raw_frame(&d, 320, 228, 16).unwrap();
        assert_eq!(&f.rgb[..3], &[255, 0, 0]);
        assert_eq!(f.rgb.len(), FRAME_W * FRAME_H * 3);
        assert!(retail_raw_frame(&d, 256, 228, 16).is_err());
    }
}
