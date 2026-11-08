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

use crate::retail_compare_image::{DARK_LUMA, FRAME_H, FRAME_W, Frame, ImageScore, score};

/// Mean luma above which a retail frame is a white flash; the mirror of
/// [`DARK_LUMA`].
pub const WHITE_LUMA: f64 = 230.0;

/// The longest display lag retail shows (`2 *` the largest frame step the
/// band runs on, `4`).
const MAX_LAG: u64 = 8;

/// Game frames a retail grab's screen fade stands behind the RAM when only
/// the other ordering table links the fade's quad ([`Row::fade_back`]): one,
/// where the scene stands two ([`Side::shown_by_frame`]). The fade actor links
/// its `POLY_F4` into the table of the pass it steps for, so the table holding
/// it places the fade (`retail-compare.md`, "The screen fades read their own
/// table").
const FADE_FRAMES_BACK: u32 = 1;

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
    /// The acting seat in retail's numbering.
    pub acting: u8,
    /// The frame recorded on this row, if any.
    pub shot: Option<String>,
    /// Pixel geometry of a retail shot (`width`, `height`, `bpp`).
    pub geom: (usize, usize, u8),
    /// The engine's display-frame clock (`frames` column): it stands still on
    /// a tick the module's blocking seek holds the whole frame.
    pub display_frames: Option<u64>,
    /// The engine module's CD-seek wait on this tick, when it has one.
    pub drive: Option<DriveRow>,
    /// The engine's screen fade on this tick (`fade_white` / `fade_level`
    /// columns): `Some(level)`, the fade colour's brightest channel, while a
    /// fade actor runs; `None` with none. A recording made before the
    /// `fade_level` column reads `255` for any running fade.
    pub fade: Option<u8>,
    /// Retail's `fade_back` column: the passes the grab's screen fade stands
    /// behind the RAM, by which ordering table links the fade's quad
    /// (`retail_compare_battle::display_lag::fade_passes_back`): `2` (the
    /// current one, the two-pass law), `1` (only the other), `None` for no
    /// fade or a recording made before the column.
    pub fade_back: Option<u8>,
}

/// The engine recorder's drive columns (`drive_seek_arm`, `drive_poll_arm`,
/// `drive_busy`): the module's seek and poll arms
/// (`battle_cast_cue::module_drive_arms`) and the vsyncs the modelled drive
/// still owes after the tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriveRow {
    pub seek_arm: u8,
    pub poll_arm: u8,
    pub busy: u32,
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
                acting: field(r, "acting").unwrap_or(0),
                shot,
                geom: (
                    field(r, "w").unwrap_or(FRAME_W),
                    field(r, "h").unwrap_or(FRAME_H),
                    field(r, "bpp").unwrap_or(16),
                ),
                display_frames: field(r, "frames"),
                fade_back: field::<u8>(r, "fade_back").filter(|&b| b > 0),
                fade: match field::<i32>(r, "fade_white") {
                    Some(w) if w >= 0 => Some(field(r, "fade_level").unwrap_or(255)),
                    _ => None,
                },
                drive: match (
                    field::<i32>(r, "drive_seek_arm"),
                    field::<i32>(r, "drive_poll_arm"),
                    field::<i32>(r, "drive_busy"),
                ) {
                    (Some(s), Some(p), Some(b)) if s >= 0 && p >= 0 => Some(DriveRow {
                        seek_arm: s as u8,
                        poll_arm: p as u8,
                        busy: b.max(0) as u32,
                    }),
                    _ => None,
                },
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

    /// Keep the recording's first action: cut where the next actor takes over
    /// past the Done band. A recording runs its tail on from the Done band and
    /// can hold the next actors' whole turns; left in, the alignment's longest
    /// common subsequence prefers a later actor's complete run of states to the
    /// first actor's partial one (a capture that starts inside the strike loop).
    ///
    /// The first action is the one the first frame shows: a probe that logs
    /// from an inject ahead of the cast can hold an earlier actor's whole turn
    /// before it (`party_basic_attack_vs_gobu_gobu`: Gobu Gobu acts between the
    /// inject and Vahn's cast), and that turn's Done band is not the cut.
    ///
    /// A recording that opens in the Done band is still on the actor before
    /// the cast: a `--monster-pick` run injects at the AI's pick, which can
    /// fall while the previous actor's `0x51` plays out. Those leading rows
    /// are dropped first, or that actor's Done band would end the cast's
    /// recording before it began.
    /// Without the first `n` rows of the recording.
    pub fn drop_leading(mut self, n: usize) -> Self {
        self.rows.drain(..n.min(self.rows.len()));
        self
    }

    pub fn first_action_only(mut self) -> Self {
        let lead = self
            .rows
            .iter()
            .take_while(|r| matches!(r.st7, 0x50 | 0x51 | 0x5A | 0x65))
            .count();
        if lead < self.rows.len() {
            self.rows.drain(..lead);
        }
        let first_shot = self
            .rows
            .iter()
            .find(|r| r.shot.is_some())
            .map_or(0, |r| r.t);
        let mut done: Option<u8> = None;
        let cut = self.rows.iter().position(|r| match done {
            Some(seat) => r.acting != seat || matches!(r.st7, 0x0A | 0x0C),
            None => {
                if r.t >= first_shot && matches!(r.st7, 0x50 | 0x51 | 0x5A | 0x65) {
                    done = Some(r.acting);
                }
                false
            }
        });
        if let Some(cut) = cut {
            self.rows.truncate(cut);
        }
        self
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

    /// The row whose state a frame grabbed on `row` displays, off the frame
    /// counter: the last row of the frame two counter bumps back. A frame
    /// that follows one which ran past its step flips a vsync after its
    /// bump, so on that bump's own vsync the screen still holds the frame
    /// before; an on-time frame flips on its bump (step `1`: every shot is
    /// a new image, two rows back). A frame's pass leaves its
    /// state on its last row, and the double buffer shows what the frame
    /// before last drew ([`Lag::Auto`]). On an ordinary step-`2` frame this
    /// is the `2 * step` rule; counted in frames, a slow frame keeps its
    /// image up for every vsync it ran over. `horn_summon` enters module
    /// phase 9 on frame `1725` (vsync 2099) with frames of seven vsyncs at
    /// step `4`, and the close-up reaches the screen at vsync 2113, the
    /// vsync after frame `1727`'s bump, where `2 * step` paired the phase-8
    /// shot at 2109 with the phase-9 row at 2101; in its phase 7 the image
    /// at 1121 (frame `1600`'s bump) is still the one 1113..1119 show.
    /// `None` without the `frame` column or before the recording holds that
    /// frame.
    fn shown_by_frame(&self, row: &Row) -> Option<u64> {
        self.shown_frames_back(row, 2)
    }

    /// The row `back` game frames before the one `row`'s grab displays the
    /// newest of: [`Self::shown_by_frame`] at `back = 2`, the screen fade's
    /// own table at `back = 1` ([`FADE_FRAMES_BACK`]).
    fn shown_frames_back(&self, row: &Row, back: u32) -> Option<u64> {
        let f = row.frame?;
        let i = self.rows.binary_search_by_key(&row.t, |r| r.t).ok()?;
        // The frame before this one ran past its step: its flip lands a
        // vsync after this frame's bump.
        let late_flip = i > 0 && self.rows[i - 1].frame != Some(f) && {
            let prev = &self.rows[i - 1];
            let len = self.rows[..i]
                .iter()
                .rev()
                .take_while(|r| r.frame == prev.frame)
                .count();
            len > usize::from(prev.step.max(1))
        };
        let want = f.checked_sub(if late_flip { back + 1 } else { back })?;
        self.rows[..=i]
            .iter()
            .rev()
            .find(|r| r.frame.is_some_and(|g| g <= want))
            .filter(|r| r.frame == Some(want))
            .map(|r| r.t)
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
                let px = psx_mednafen::bgr555_to_rgba8(u16::from_le_bytes([p[0], p[1]]));
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
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .context("PNG frame size overflows usize")?
    ];
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
    /// Two game frames back, off the frame counter
    /// (`Side::shown_by_frame`); `2 * frame step` read off the row the frame
    /// was grabbed on where the capture has no counter.
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
    /// A retail frame of a CD-seek wait arm past the engine's live frames
    /// there: retail's drive held longer, inside the window ([`DriveWait`]).
    DriveHold,
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
    /// Paired on the screen fade's row, one game frame younger than the
    /// scene's ([`FADE_FRAMES_BACK`]).
    #[serde(default)]
    pub fade: bool,
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
    /// Engine ticks this segment's pairs were moved by to absorb a CD-seek
    /// wait ([`DriveWait::live_shift`]); `None` outside one's reach.
    #[serde(default)]
    pub drive_shift: Option<i64>,
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
    /// Retail frames of a CD-seek wait the engine's drive answered sooner
    /// ([`FrameClass::DriveHold`]), out of the mean.
    #[serde(default)]
    pub drive_hold_pairs: usize,
    /// Retail frames with no engine counterpart (a segment the engine never
    /// ran, or a frame shown before the recording began).
    pub unpaired_retail: usize,
    /// Pairs whose two sides sit in different action states under one module
    /// phase key.
    pub state_mismatch_pairs: usize,
    /// Pairs taken on the screen fade's row ([`Pair::fade`]).
    #[serde(default)]
    pub fade_pairs: usize,
    pub retail_frames: usize,
    pub engine_frames: usize,
    pub segments: Vec<SegmentReport>,
    pub pairs: Vec<Pair>,
    /// The module's CD-seek wait and the window the pairing absorbed it in.
    #[serde(default)]
    pub drive_wait: Option<DriveWait>,
    /// The mean with the wait paired strictly, where a wait was absorbed.
    #[serde(default)]
    pub strict_mean_within: Option<f64>,
    /// The sound channel ([`crate::cast_sequence_sound`]), when both sides
    /// carry an `sfx.csv`.
    #[serde(default)]
    pub sound: Option<crate::cast_sequence_sound::SoundReport>,
}

/// Pair the two recordings and score every pair, absorbing a CD-seek wait
/// inside its window ([`DriveWait`]).
pub fn compare(label: &str, retail: &Side, engine: &Side, lag: Lag) -> Result<Report> {
    compare_with(label, retail, engine, lag, true)
}

/// The vsyncs past the engine's own wait that a retail wait may still take
/// and be absorbed: the larger of [`MAX_LAG`] and half the engine's wait.
pub fn drive_window_margin(engine_wait: u64) -> u64 {
    MAX_LAG.max(engine_wait / 2)
}

/// A module's CD-seek wait as the two recordings show it, and the window the
/// pairing absorbs it in (`docs/tooling/cast-sequence-oracle.md`, "CD-seek
/// waits").
///
/// The seek arm's `CdlSetloc` blocks the game loop while the drive finishes
/// the seek before it; the engine holds the whole frame there (its display
/// clock stands still), retail's blocked frame overruns its step. The poll
/// arm returns busy until the module's own seek completes, with the rest of
/// the frame running. How long either takes is the drive's seek time, which
/// the engine models once (`battle_cd_drive`, fitted to mednafen) and which
/// differs between emulators and drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveWait {
    pub seek_arm: u8,
    pub poll_arm: u8,
    /// Engine ticks of the seek arm the blocking seek held whole.
    pub engine_frozen: u64,
    /// Engine ticks of the poll arm that returned busy on the drive.
    pub engine_poll_hold: u64,
    /// Retail's wall vsyncs in the two arms past the engine's own passes
    /// there: its wait, measured against the engine's arms.
    pub retail_wait: u64,
    /// The same on retail's game clock: the part of its wait the rest of the
    /// frame ran through (a blocked frame counts only its step).
    pub retail_live_wait: u64,
    /// The waits accepted, vsyncs: `0` up to the engine's (the model's) wait
    /// plus [`drive_window_margin`].
    pub window: (u64, u64),
    /// Retail's wait fell inside the window, so the pairing absorbed it.
    pub accepted: bool,
    /// Engine poll hold less retail's live wait: how far the frame content
    /// that ran through the wait stands ahead in the engine after it.
    pub live_shift: i64,
}

/// [`compare`], with the drive wait's tolerance on or off (`strict`).
pub fn compare_with(
    label: &str,
    retail: &Side,
    engine: &Side,
    lag: Lag,
    tolerate_drive: bool,
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

    // The module's CD-seek wait: its two arms' first aligned segments.
    let drive = engine.rows.iter().find_map(|r| r.drive);
    let mut wait: Option<DriveWait> = None;
    let mut wait_segs: Vec<(usize, usize)> = Vec::new();
    let mut frozen: std::collections::BTreeSet<u64> = Default::default();
    if let Some(d) = drive {
        for arm in [d.seek_arm, d.poll_arm] {
            if let Some(&(ri, ei)) = aligned
                .iter()
                .find(|&&(ri, _)| rs[ri].key == Key::Module(arm))
                && !wait_segs.contains(&(ri, ei))
            {
                wait_segs.push((ri, ei));
            }
        }
        let row_ix = |t: u64| engine.rows.binary_search_by_key(&t, |r| r.t).ok();
        let (mut held, mut hold, mut r_wall, mut r_game) = (0u64, 0u64, 0u64, 0u64);
        for &(ri, ei) in &wait_segs {
            let (rseg, eseg) = (&rs[ri], &es[ei]);
            let (mut seg_frozen, mut seg_hold) = (0u64, 0u64);
            for t in eseg.start..=eseg.end {
                let Some(i) = row_ix(t) else { continue };
                let row = &engine.rows[i];
                let still = i > 0
                    && row.display_frames.is_some()
                    && engine.rows[i - 1].display_frames == row.display_frames;
                if rseg.key == Key::Module(d.seek_arm) && still {
                    seg_frozen += 1;
                    frozen.insert(t);
                } else if rseg.key == Key::Module(d.poll_arm)
                    && row.drive.is_some_and(|x| x.busy > 0)
                {
                    seg_hold += 1;
                }
            }
            let own = eseg.len() - seg_frozen - seg_hold;
            let rgame = game(rseg.end).saturating_sub(game(rseg.start)) + 1;
            r_wall += rseg.len().saturating_sub(own);
            r_game += rgame.saturating_sub(own);
            held += seg_frozen;
            hold += seg_hold;
        }
        let engine_wait = held + hold;
        let window = (0, engine_wait + drive_window_margin(engine_wait));
        if !wait_segs.is_empty() {
            wait = Some(DriveWait {
                seek_arm: d.seek_arm,
                poll_arm: d.poll_arm,
                engine_frozen: held,
                engine_poll_hold: hold,
                retail_wait: r_wall,
                retail_live_wait: r_game,
                window,
                accepted: tolerate_drive && r_wall <= window.1,
                live_shift: hold as i64 - r_game as i64,
            });
        }
    }
    let absorb = wait.as_ref().filter(|w| w.accepted);
    if absorb.is_none() {
        frozen.clear();
    }
    // The engine's clock for the pairing offset: one per tick, less the ticks
    // an absorbed seek held whole.
    let mut live: BTreeMap<u64, u64> = BTreeMap::new();
    let mut n = 0u64;
    for r in &engine.rows {
        live.insert(r.t, n);
        if !frozen.contains(&r.t) {
            n += 1;
        }
    }
    let last_wait_ri = wait_segs.iter().map(|&(ri, _)| ri).max();
    // The engine ticks the absorbed shift may move a pair across: from the
    // wait's first arm to the end of the module band.
    let band = absorb.and_then(|_| {
        let lo = wait_segs.iter().map(|&(_, ei)| es[ei].start).min()?;
        let hi = es
            .iter()
            .filter(|s| matches!(s.key, Key::Module(_)))
            .map(|s| s.end)
            .max()?;
        Some((lo, hi))
    });
    // A post-wait segment pairs on one of two clocks: the module's arm clock
    // (shift `0`), or the clock the rest of the frame ran on through the wait
    // (`-live_shift`). Nothing between: a free search over the span would
    // also fit the one-tick offsets the wait has nothing to do with.
    let shifts: Vec<i64> = match absorb {
        Some(w) if w.live_shift != 0 => vec![0, -w.live_shift],
        _ => vec![0],
    };
    let nearest = |want: u64, lo: u64, hi: u64| -> Option<(u64, &Row)> {
        let before = engine_shots.range(lo..=want).next_back();
        let after = engine_shots.range(want..=hi).next();
        match (before, after) {
            (Some(b), Some(a)) => Some(if want - *b.0 <= *a.0 - want { b } else { a }),
            (b, a) => b.or(a),
        }
        .map(|(&t, &r)| (t, r))
    };

    // What each retail frame pairs with; the frames themselves are read and
    // scored below, on as many threads as there are.
    struct Job<'a> {
        row: &'a Row,
        shown: u64,
        shown_st7: u8,
        key: Key,
        ri: usize,
        into: u64,
        /// The engine frame per candidate shift, `(shift, tick, row)`.
        cands: Vec<(i64, u64, &'a Row)>,
        /// A retail frame of a wait arm past the engine's live frames there.
        hold: bool,
        /// Paired on the screen fade's row ([`FADE_FRAMES_BACK`]).
        fade: bool,
    }
    // The engine frame a retail frame showing row `shown` pairs with; `None`
    // when the row has no aligned engine segment.
    let pair_at = |ix: usize, shown: u64| -> Option<Job<'_>> {
        let row = &retail.rows[ix];
        let (shown_row, ri) = (retail.row_at(shown)?, seg_of(&rs, shown)?);
        let &ei = to_engine.get(&ri)?;
        let (rseg, eseg) = (&rs[ri], &es[ei]);
        let into = game(shown).saturating_sub(game(rseg.start));
        // The engine row `into` live ticks into the segment, clamped to its
        // last tick.
        let base = live[&eseg.start];
        let want = live
            .range(eseg.start..=eseg.end)
            .find(|&(t, &l)| !frozen.contains(t) && l - base >= into)
            .map_or(eseg.end, |(&t, _)| t);
        let live_len = live
            .range(eseg.start..=eseg.end)
            .filter(|&(t, _)| !frozen.contains(t))
            .count() as u64;
        let hold = absorb.is_some() && wait_segs.contains(&(ri, ei)) && into >= live_len;
        let post_wait = matches!(rseg.key, Key::Module(_)) && last_wait_ri.is_some_and(|w| ri > w);
        let mut cands = Vec::new();
        for &s in if post_wait { &shifts[..] } else { &[0][..] } {
            let got = match (s, band) {
                (0, _) | (_, None) => nearest(want, eseg.start, eseg.end),
                (s, Some((lo, hi))) => {
                    let w = (want as i64 + s).clamp(lo as i64, hi as i64) as u64;
                    nearest(w, lo, hi)
                }
            };
            if let Some((t, r)) = got {
                cands.push((s, t, r));
            }
        }
        if cands.is_empty() {
            return None;
        }
        Some(Job {
            row,
            shown,
            shown_st7: shown_row.st7,
            key: rseg.key,
            ri,
            into,
            cands,
            hold,
            fade: false,
        })
    };
    let mut jobs: Vec<Job<'_>> = Vec::new();
    let mut unpaired = 0usize;
    let mut retail_frames = 0usize;
    for (ix, row) in retail
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.shot.is_some())
    {
        retail_frames += 1;
        let shown = match lag {
            Lag::Auto => retail
                .shown_by_frame(row)
                .or_else(|| row.t.checked_sub(2 * u64::from(row.step.max(1)))),
            Lag::Fixed(n) => row.t.checked_sub(n),
        };
        let Some(job) = shown.and_then(|shown| pair_at(ix, shown)) else {
            unpaired += 1;
            continue;
        };
        // The screen fades read their own table: the fade actor links its
        // quad into the table of the pass it steps for, and where only the
        // other table holds it the grab shows the fade one game frame younger
        // than the scene ([`FADE_FRAMES_BACK`]). Where the engine frame the
        // scene pairs with shows a fade, the pair moves to the fade's row.
        let fade_job = (lag == Lag::Auto
            && row.fade_back == Some(FADE_FRAMES_BACK as u8)
            && job.cands[0].2.fade.is_some_and(|level| level > 0))
        .then(|| {
            retail
                .shown_frames_back(row, FADE_FRAMES_BACK)
                .or_else(|| row.t.checked_sub(u64::from(row.step.max(1))))
                .and_then(|shown| pair_at(ix, shown))
        })
        .flatten()
        .filter(|f| f.cands[0].2.fade.is_some());
        jobs.push(match fade_job {
            Some(f) => Job { fade: true, ..f },
            None => job,
        });
    }
    let scored = crate::parallel::parallel_map(
        &jobs,
        crate::parallel::default_jobs(),
        |j| -> Result<Vec<ImageScore>> {
            let r = retail.frame(j.row)?;
            j.cands
                .iter()
                .map(|(_, _, e)| Ok(score(&r, &engine.frame(e)?)))
                .collect()
        },
    )
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    let class_of = |s: &ImageScore| {
        if s.retail_luma < DARK_LUMA {
            FrameClass::Dark
        } else if s.retail_luma > WHITE_LUMA {
            FrameClass::White
        } else {
            FrameClass::Scene
        }
    };
    // Each post-wait segment takes the clock its scene pairs score best on:
    // what a segment shows runs on the module's arm clock (effects the arm
    // spawns) or on the clock that ran through the wait (a fade or a rise
    // spawned before it).
    let mut seg_shift: BTreeMap<usize, i64> = BTreeMap::new();
    if shifts.len() > 1 {
        let mut sums: BTreeMap<usize, Vec<(f64, usize)>> = BTreeMap::new();
        for (j, sc) in jobs.iter().zip(&scored) {
            if j.cands.len() != shifts.len() || class_of(&sc[0]) != FrameClass::Scene {
                continue;
            }
            let v = sums
                .entry(j.ri)
                .or_insert_with(|| vec![(0.0, 0); shifts.len()]);
            for (k, s) in sc.iter().enumerate() {
                v[k].0 += s.within;
                v[k].1 += 1;
            }
        }
        for (ri, v) in sums {
            let best = (0..shifts.len())
                .max_by(|&a, &b| {
                    let m = |k: usize| v[k].0 / v[k].1.max(1) as f64;
                    m(a).total_cmp(&m(b))
                        .then(shifts[b].abs().cmp(&shifts[a].abs()))
                })
                .map_or(0, |k| shifts[k]);
            seg_shift.insert(ri, best);
        }
    }
    let pairs: Vec<Pair> = jobs
        .iter()
        .zip(&scored)
        .map(|(j, sc)| {
            let want = seg_shift.get(&j.ri).copied().unwrap_or(0);
            let k = j.cands.iter().position(|c| c.0 == want).unwrap_or(0);
            let (s, (_, et, erow)) = (&sc[k], &j.cands[k]);
            Pair {
                retail_vsync: j.row.t,
                shown_vsync: j.shown,
                engine_tick: *et,
                key: j.key,
                into: j.into,
                retail_st7: j.shown_st7,
                engine_st7: erow.st7,
                class: if j.hold {
                    FrameClass::DriveHold
                } else {
                    class_of(s)
                },
                within: s.within,
                mae: s.mae,
                retail_luma: s.retail_luma,
                engine_luma: s.engine_luma,
                fade: j.fade,
            }
        })
        .collect();

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
                    drive_shift: None,
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
            drive_shift: seg_shift.get(&i).copied(),
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
            drive_shift: None,
        });
    }

    let strict_mean_within = if absorb.is_some() {
        Some(compare_with(label, retail, engine, lag, false)?.mean_within)
    } else {
        None
    };
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
        drive_hold_pairs: pairs
            .iter()
            .filter(|p| p.class == FrameClass::DriveHold)
            .count(),
        unpaired_retail: unpaired,
        state_mismatch_pairs: pairs
            .iter()
            .filter(|p| p.retail_st7 != p.engine_st7)
            .count(),
        fade_pairs: pairs.iter().filter(|p| p.fade).count(),
        retail_frames,
        engine_frames: engine_shots.len(),
        segments,
        pairs,
        drive_wait: wait,
        strict_mean_within,
        sound: None,
    })
}

/// The report for a human reader.
pub fn markdown(r: &Report, worst: usize, strip: &[Pair]) -> String {
    let mut s = format!("# Cast sequence: {}\n\n", r.label);
    s.push_str(&format!(
        "**Mean `within` {:.3}** over {} scene pairs (min {:.3}). White-flash pairs: {} (mean {}); \
         dark pairs: {}; retail frames with no engine counterpart: {}; pairs in different action \
         states: {}; pairs on the screen fade's row: {}. Retail frames {}, engine frames {}, \
         display lag {:?}, retail paced on {}.\n\n",
        r.mean_within,
        r.valid_pairs,
        r.min_within,
        r.white_pairs,
        r.white_mean_within
            .map_or("-".into(), |m| format!("{m:.3}")),
        r.dark_pairs,
        r.unpaired_retail,
        r.state_mismatch_pairs,
        r.fade_pairs,
        r.retail_frames,
        r.engine_frames,
        r.lag,
        if r.game_clock {
            "the game clock"
        } else {
            "wall vsyncs (no frame counter in the capture)"
        },
    ));
    if let Some(w) = &r.drive_wait {
        s.push_str(&format!(
            "**CD-seek wait** (module arms {} seek / {} poll): the engine held {} ticks whole on the \
             seek and {} on the poll; retail's wait read {} vsyncs ({} on the game clock) against a \
             window of {}..={}. {}\n\n",
            w.seek_arm,
            w.poll_arm,
            w.engine_frozen,
            w.engine_poll_hold,
            w.retail_wait,
            w.retail_live_wait,
            w.window.0,
            w.window.1,
            if w.accepted {
                format!(
                    "Absorbed: live shift {:+} ticks, {} retail hold frames out of the mean; strict \
                     pairing mean {}.",
                    w.live_shift,
                    r.drive_hold_pairs,
                    r.strict_mean_within.map_or("-".into(), |m| format!("{m:.3}"))
                )
            } else {
                "Outside the window or `--strict-drive`: paired strictly.".to_string()
            }
        ));
    }
    s.push_str("## Segments\n\n`retail game` is the segment on retail's game clock; `slow frames` counts the retail frames that ran past their step and the vsyncs they ran over by; `shift` is the engine ticks an absorbed CD-seek wait moved the segment's pairs by.\n\n| key | retail vsyncs | retail game | slow frames | engine ticks | pairs | mean within | shift |\n|---|---|---|---|---|---|---|---|\n");
    for g in &r.segments {
        let o = |v: Option<u64>| v.map_or("-".to_string(), |v| v.to_string());
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
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
            g.mean_within.map_or("-".into(), |m| format!("{m:.3}")),
            g.drive_shift.map_or("-".into(), |d| format!("{d:+}"))
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
    /// Sound channel: ticks either side of its projected time a cue still
    /// matches at.
    #[arg(long, default_value_t = crate::cast_sequence_sound::DEFAULT_TOLERANCE)]
    pub sound_tolerance: u64,
    /// Pair a module's CD-seek wait strictly, with no window
    /// ([`DriveWait`]).
    #[arg(long)]
    pub strict_drive: bool,
    /// Engine rows to drop from the recording's start: a gate film's ticks
    /// still on the state the retail side's first row has already left
    /// (the driver counts them from `cam.csv`, [`Side::drop_leading`]).
    #[arg(long, default_value_t = 0)]
    pub engine_skip: usize,
}

pub fn run(a: CastSequenceArgs) -> Result<()> {
    let retail = Side::read(&a.retail)?.first_action_only();
    let engine = Side::read(&a.engine)?.drop_leading(a.engine_skip);
    let mut report = compare_with(&a.label, &retail, &engine, a.retail_lag, !a.strict_drive)?;
    report.sound = crate::cast_sequence_sound::compare_dirs(&retail, &engine, a.sound_tolerance)?;
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
    let mut md = markdown(&report, a.worst, &strip);
    if let Some(s) = &report.sound {
        md.push_str(&crate::cast_sequence_sound::markdown(s));
        println!(
            "{}: sound cue score {:.3} (id {:.3}): retail {} engine {} matched {} timing {} missing {} extra {}",
            report.label,
            s.score,
            s.id_score,
            s.retail_cues,
            s.engine_cues,
            s.matched.len(),
            s.timing.len(),
            s.missing.len(),
            s.extra.len()
        );
    }
    std::fs::write(a.out.join("report.md"), md)?;
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
    if let Some(w) = &report.drive_wait {
        println!(
            "{}: CD-seek wait arms {}/{}: engine {} held + {} poll, retail {} ({} live), window {}..={}: {}",
            report.label,
            w.seek_arm,
            w.poll_arm,
            w.engine_frozen,
            w.engine_poll_hold,
            w.retail_wait,
            w.retail_live_wait,
            w.window.0,
            w.window.1,
            if w.accepted {
                format!(
                    "absorbed, live shift {:+}, hold frames {}, strict mean {:.3}",
                    w.live_shift,
                    report.drive_hold_pairs,
                    report.strict_mean_within.unwrap_or(0.0)
                )
            } else {
                "paired strictly".to_string()
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(key: Key, start: u64, end: u64) -> Segment {
        Segment { key, start, end }
    }

    #[test]
    fn a_recording_is_cut_where_the_next_actor_takes_over() {
        let row = |t: u64, st7: u8, acting: u8| Row {
            t,
            st7,
            phase: 0,
            step: 2,
            frame: None,
            acting,
            shot: None,
            geom: (FRAME_W, FRAME_H, 16),
            display_frames: None,
            drive: None,
            fade: None,
            fade_back: None,
        };
        let side = Side {
            dir: PathBuf::new(),
            rows: vec![
                row(1, 0x1F, 4),
                row(2, 0x51, 4),
                row(3, 0x5A, 4),
                row(4, 0x0C, 1),
                row(5, 0x1E, 1),
            ],
        };
        assert_eq!(side.first_action_only().rows.len(), 3);
        // An earlier actor's turn ahead of the first frame is not the cut.
        let shot = |mut r: Row| {
            r.shot = Some(String::new());
            r
        };
        let side = Side {
            dir: PathBuf::new(),
            rows: vec![
                row(1, 0x1E, 3),
                row(2, 0x5A, 3),
                row(3, 0x0A, 0),
                shot(row(4, 0x28, 0)),
                row(5, 0x51, 0),
                row(6, 0x0A, 3),
            ],
        };
        assert_eq!(side.first_action_only().rows.len(), 5);
        // Opened in the previous actor's Done band: that band is not the
        // cast's, and the cast's own run is kept whole.
        let side = Side {
            dir: PathBuf::new(),
            rows: vec![
                row(1, 0x51, 2),
                row(2, 0x5A, 2),
                row(3, 0x0A, 3),
                row(4, 0x0C, 3),
                row(5, 0x28, 3),
                row(6, 0x51, 3),
                row(7, 0x0A, 0),
            ],
        };
        let kept = side.first_action_only().rows;
        assert_eq!(kept.iter().map(|r| r.t).collect::<Vec<_>>(), [3, 4, 5, 6]);
    }

    #[test]
    fn a_gate_film_can_drop_its_leading_rows_on_the_state() {
        let kept = side(&[(0, 3, 5)]).drop_leading(3);
        assert_eq!(kept.rows.len(), 2);
        assert_eq!(kept.rows[0].t, 103);
        assert!(side(&[(0, 3, 2)]).drop_leading(3).rows.is_empty());
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
                    acting: 0,
                    shot: None,
                    geom: (FRAME_W, FRAME_H, 16),
                    display_frames: None,
                    drive: None,
                    fade: None,
                    fade_back: None,
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

    /// A shot shows the last row of the frame two counter bumps back, so a
    /// slow frame holds the image for all its vsyncs.
    #[test]
    fn auto_lag_counts_frames_not_vsyncs() {
        let s = side(&[(1, 2, 2), (2, 4, 7), (3, 4, 7), (4, 4, 7)]);
        let rows_of = |f: u32| s.rows.iter().filter(move |r| r.frame == Some(f));
        let last_of = |f: u32| rows_of(f).next_back().unwrap().t;
        // Frame 3 past its bump shows frame 1's last row, all seven vsyncs.
        for r in rows_of(3).skip(1) {
            assert_eq!(s.shown_by_frame(r), Some(last_of(1)));
        }
        // Frame 4 follows a slow frame: its bump vsync still shows frame 1,
        // the vsync after shows frame 2.
        let mut f4 = rows_of(4);
        assert_eq!(s.shown_by_frame(f4.next().unwrap()), Some(last_of(1)));
        assert_eq!(s.shown_by_frame(f4.next().unwrap()), Some(last_of(2)));
        // Frame 2 has no frame two back in the recording.
        assert_eq!(s.shown_by_frame(&s.rows[3]), None);
        // On-time frames flip on their bump.
        let t = side(&[(1, 1, 1), (2, 1, 1), (3, 1, 1)]);
        assert_eq!(t.shown_by_frame(&t.rows[2]), Some(t.rows[0].t));
        // The fade's own table stands one frame back: frame 3 past its bump
        // shows frame 2's fade.
        for r in rows_of(3).skip(1) {
            assert_eq!(s.shown_frames_back(r, FADE_FRAMES_BACK), Some(last_of(2)));
        }
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
