//! The engine side of the minigame film
//! (`scripts/pcsx-redux/autorun_minigame_film.lua`,
//! `scripts/ci/minigame-film.py`): take the seeded state's warp, stand the
//! session on the capture's own phase, then hold a pad schedule and write the
//! session's words one row a tick, under the names the retail side reads its
//! RAM words by. A minigame's rules, its clocks and its camera can then be
//! compared with retail game frame by game frame where the corpus compares
//! one still.
//!
//! `LEGAIA_RC_MINIGAME_FILM_OUT` names the CSV, `LEGAIA_RC_MINIGAME_FILM` the
//! pad schedule (`A-B:BTN` spans, ticks from the aligned tick) and
//! `LEGAIA_RC_MINIGAME_FILM_TICKS` its length. Row `vsync = 1` is the tick the
//! session stands where the capture's first vsync does.

use super::minigame_run::RetailMinigame;
use super::*;
use legaia_engine_mode::minigame_entry::MinigameSubId;
use std::io::Write;

/// A pad schedule and where the trace goes.
pub(super) struct MinigameFilm {
    spans: Vec<(u64, u64, u16)>,
    ticks: u64,
    out: std::path::PathBuf,
}

/// The engine pad word's bit of a schedule's button name.
fn button(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "UP" => 0x0010,
        "RIGHT" => 0x0020,
        "DOWN" => 0x0040,
        "LEFT" => 0x0080,
        "TRIANGLE" => 0x1000,
        "CIRCLE" => 0x2000,
        "CROSS" => 0x4000,
        "SQUARE" => 0x8000,
        "START" => 0x0008,
        _ => return None,
    })
}

impl MinigameFilm {
    pub(super) fn from_env() -> Option<Self> {
        let out = std::env::var_os("LEGAIA_RC_MINIGAME_FILM_OUT")?.into();
        let script = std::env::var("LEGAIA_RC_MINIGAME_FILM").unwrap_or_default();
        let spans = script
            .split(',')
            .filter_map(|item| {
                let (range, name) = item.trim().split_once(':')?;
                let (a, b) = range.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?, button(name)?))
            })
            .collect();
        let ticks = std::env::var("LEGAIA_RC_MINIGAME_FILM_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(600);
        Some(Self { spans, ticks, out })
    }

    fn pad_at(&self, t: u64) -> u16 {
        self.spans
            .iter()
            .filter(|&&(a, b, _)| (a..b).contains(&t))
            .fold(0, |p, s| p | s.2)
    }

    /// Film the session from the tick it stands where the capture's first
    /// vsync does. The dance's venue step (`step_dance_tail`) runs after each
    /// tick, as both play hosts run it.
    pub(super) fn run(
        &self,
        session: &mut NativeSession,
        mg: &RetailMinigame,
        aligned_tick: u64,
    ) -> Result<()> {
        let sub = mg.minigame_id().context("sub-id names no overlay")?;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&self.out)?);
        writeln!(f, "vsync,aligned_at,{}", columns(sub).join(","))?;
        let row = |session: &NativeSession| values(sub, &session.host.world);
        writeln!(
            f,
            "1,{aligned_tick},{}",
            row(session)
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )?;
        for t in 1..self.ticks {
            // The tick that writes row `t + 1` holds that row's pad, as
            // retail's row `v` is the vsync the probe held the pad on.
            session.host.world.input.set_pad(self.pad_at(t + 1));
            session.tick()?;
            let _ = session.step_dance_tail();
            writeln!(
                f,
                "{},{aligned_tick},{}",
                t + 1,
                row(session)
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }
        Ok(())
    }
}

/// The columns a minigame's film writes, named as the retail side's words.
pub(super) fn columns(sub: MinigameSubId) -> Vec<&'static str> {
    match sub {
        MinigameSubId::Dance => vec![
            "state", "counter", "go", "lead", "phase", "song", "score0", "score1", "score2",
            "gauge0", "gauge1", "gauge2", "tri0", "tri1", "tri2", "chain0", "chain1", "chain2",
            "latch0", "latcht0", "miss0", "banked1", "banked2", "camkey", "camtimer", "pitch",
            "yaw", "roll", "eye_x", "eye_y", "eye_z",
        ],
        _ => vec!["mode"],
    }
}

/// The session's words in [`columns`]' order; `-1` for a word the session
/// does not hold this tick.
fn values(sub: MinigameSubId, w: &legaia_engine_core::world::World) -> Vec<i64> {
    match sub {
        MinigameSubId::Dance => dance_values(w),
        _ => vec![w.mode as i64],
    }
}

fn dance_values(w: &legaia_engine_core::world::World) -> Vec<i64> {
    use legaia_engine_minigames::dance::CountInStage as S;
    let m = &w.minigames;
    let n = columns(MinigameSubId::Dance).len();
    let Some(g) = m.dance.as_ref().filter(|_| w.mode == SceneMode::Dance) else {
        return vec![-1; n];
    };
    let ci = m.dance_countin.as_ref();
    let state = match ci.map(|c| c.stage()) {
        Some(S::Ready) => 3,
        Some(S::GoIn) => 4,
        Some(S::GoOut) => 5,
        Some(S::Done) => 6,
        Some(S::LeadIn) => 7,
        None if g.finished() => 20,
        None if g.finish_wipe().is_some_and(|w| w > 0) => 12,
        None if g.in_finale() => 11,
        // The lead-in's last run left the count at one; the beat clock's
        // first frame is the one that takes it to zero and enters state 10.
        None if g.song_timer() == 0 && g.beat_index() == 0 && g.intra_beat_phase() == 0 => 7,
        None => 10,
    };
    let go = ci
        .and_then(|c| c.view().go)
        .map(|b| i64::from(b / 2))
        .or(g.finish_wipe().map(i64::from))
        .unwrap_or(0);
    let mut v = vec![
        state,
        ci.map_or(-1, |c| i64::from(c.frame())),
        go,
        ci.map_or(i64::from(state == 7), |c| i64::from(c.lead())),
        i64::from(g.beat_index() * 0x119 + g.intra_beat_phase()),
        i64::from(g.song_timer()),
    ];
    let dancer = |i: usize, f: &dyn Fn(usize) -> i64| {
        if i < g.dancer_count() { f(i) } else { -1 }
    };
    for i in 0..3 {
        v.push(dancer(i, &|i| i64::from(g.dancer_score(i))));
    }
    for i in 0..3 {
        v.push(dancer(i, &|i| i64::from(g.dancer_gauge(i))));
    }
    for i in 0..3 {
        v.push(dancer(i, &|i| i64::from(g.dancer_triangles(i))));
    }
    for i in 0..3 {
        v.push(dancer(i, &|i| i64::from(g.dancer_probe(i).cursor)));
    }
    v.push(dancer(0, &|i| i64::from(g.dancer_probe(i).latch)));
    v.push(dancer(0, &|i| i64::from(g.dancer_probe(i).latch_timer)));
    v.push(dancer(0, &|i| i64::from(g.dancer_probe(i).misses)));
    for i in 1..3 {
        v.push(dancer(i, &|i| i64::from(g.dancer_probe(i).tri_meter)));
    }
    let (key, timer) = g.camera_track().map_or((-1, -1), |c| c.counters());
    v.push(i64::from(key));
    v.push(i64::from(timer));
    match g.camera_pose() {
        Some(p) => {
            v.extend(p.angles.iter().map(|&a| i64::from(a)));
            v.extend(p.eye.iter().map(|&e| i64::from(e)));
        }
        None => v.extend([-1; 6]),
    }
    v
}
