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
        rand_seed: Option<u32>,
    ) -> Result<()> {
        let sub = mg.minigame_id().context("sub-id names no overlay")?;
        // Harness seat: the pond and the cabinet draw every roll off their
        // own streams, which stand on the capture's `rand()` word at the
        // aligned tick.
        if let Some(seed) = rand_seed {
            let m = &mut session.host.world.minigames;
            match sub {
                MinigameSubId::Fishing => {
                    if let Some(p) = m.fishing.as_mut() {
                        p.seat_rng(seed);
                    }
                }
                MinigameSubId::BakaFighter => {
                    if let Some(f) = m.baka_fighter.as_mut() {
                        f.seat_rng(seed);
                    }
                }
                _ => {}
            }
        }
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
        if let Some(path) = std::env::var_os("LEGAIA_RC_MINIGAME_FILM_REPLAY") {
            return replay_pond(session, sub, &path, aligned_tick, &mut f);
        }
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
        MinigameSubId::MuscleDome => vec![
            "arm",
            "backdrop",
            "intro",
            "card",
            "scale",
            "hold",
            "card_hold",
            "emode",
        ],
        MinigameSubId::Fishing => vec![
            "state", "power", "record", "height", "band", "bandcd", "tension", "casts", "rng",
            "lx", "lz", "species", "strength", "pull", "sway", "points",
        ],
        MinigameSubId::BakaFighter => vec![
            "state", "scene", "stimer", "intro", "round", "mphase", "hp0", "hp1", "wins0", "wins1",
            "type0", "type1", "combo0", "combo1", "rng",
        ],
        _ => vec!["mode"],
    }
}

/// The session's words in [`columns`]' order; `-1` for a word the session
/// does not hold this tick.
fn values(sub: MinigameSubId, w: &legaia_engine_core::world::World) -> Vec<i64> {
    match sub {
        MinigameSubId::Dance => dance_values(w),
        MinigameSubId::MuscleDome => hub_values(w),
        MinigameSubId::Fishing => fishing_values(w),
        MinigameSubId::BakaFighter => baka_values(w),
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

fn hub_values(w: &legaia_engine_core::world::World) -> Vec<i64> {
    use legaia_engine_minigames::muscle_ringside::FirstVisitArm as A;
    // The engine's own scene mode, for the hand-over to the fight (no
    // retail column: retail's mode word is `0x19` / `0x14` / `0x15`).
    let emode = w.mode as i64;
    let Some(h) = w.minigames.muscle_hub.first_visit.as_ref() else {
        let mut v = vec![-1; columns(MinigameSubId::MuscleDome).len() - 1];
        v.push(emode);
        return v;
    };
    let arm = match h.arm() {
        A::IntroIn => 0,
        A::IntroHold => 1,
        A::IntroOut => 2,
        A::TitleZoom => 3,
        A::CardIn => 4,
        A::CardHold => 5,
        A::Drain => 6,
        A::Return => 0x14,
        A::RoundIn => 0x15,
        A::RoundOut => 0x16,
        A::Done => 0x17,
    };
    let [backdrop, intro, card, scale, hold, card_hold] = h.words();
    let mut v = [arm, backdrop, intro, card, scale, hold, card_hold]
        .map(i64::from)
        .to_vec();
    v.push(emode);
    v
}

fn fishing_values(w: &legaia_engine_core::world::World) -> Vec<i64> {
    use legaia_engine_fishing::fishing::PondPhase as P;
    let Some(p) = w.minigames.fishing.as_ref() else {
        return vec![-1; columns(MinigameSubId::Fishing).len()];
    };
    // The driver's state word for each phase: the wait and the fight are
    // both state `0x19`, the lure in the water.
    let state = match p.phase() {
        P::Idle => 0xC,
        P::WindUp => 0xD,
        P::Power => 0x14,
        P::Flight => 0x15,
        P::Waiting | P::Hooked => 0x19,
        P::Landed => 0x1E,
        P::Snapped => 0x28,
        _ => -1,
    };
    let lure = p.lure_actor();
    vec![
        state,
        i64::from(p.cast_power()),
        i64::from(p.line_record()),
        lure.map_or(-1, |l| i64::from(l.y_fixed)),
        i64::from(p.band()),
        i64::from(p.band_countdown()),
        i64::from(p.tension()),
        i64::from(p.persist().casts),
        i64::from(p.rng_state()),
        lure.map_or(-1, |l| i64::from(l.x_fixed)),
        lure.map_or(-1, |l| i64::from(l.z_fixed)),
        p.hooked().map_or(0, |s| s.index as i64),
        i64::from(p.strength()),
        lure.map_or(0, |l| i64::from(l.pull)),
        lure.map_or(0, |l| i64::from(l.sway.0)),
        i64::from(p.persist().record.points),
    ]
}

/// `LEGAIA_RC_MINIGAME_FILM_REPLAY`: a rules differential over a retail film.
/// The pond is ticked once per retail game frame (a move of the master frame
/// driver's counter `_DAT_8007B798`, `gp + 0x480` at the head of
/// `FUN_80016B6C`) on that frame's step `DAT_1F800393` and its pad words
/// (held `_DAT_8007B850`, new-press `_DAT_8007B874`), so the venue's measured
/// frame step and its pad timing come from the capture and what the film
/// compares is the rules. Each row is labelled with the retail vsync it
/// stands beside.
fn replay_pond(
    session: &mut NativeSession,
    sub: MinigameSubId,
    path: &std::ffi::OsStr,
    aligned_tick: u64,
    f: &mut impl Write,
) -> Result<()> {
    anyhow::ensure!(
        sub == MinigameSubId::Fishing,
        "the replay drives the fishing pond only"
    );
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let head: Vec<&str> = lines
        .next()
        .context("empty retail film")?
        .split(',')
        .collect();
    let col = |name: &str| head.iter().position(|h| *h == name);
    let (Some(vs), Some(gf), Some(dt), Some(held), Some(edge)) = (
        col("vsync"),
        col("gframe"),
        col("dt"),
        col("held"),
        col("edge"),
    ) else {
        anyhow::bail!("the retail film lacks vsync / gframe / dt / held / edge");
    };
    // One `(vsync, step, held, new-press)` per game frame: the row the
    // frame driver's counter moved on carries that frame's pad pump, and the
    // frame's results stand on the row the counter moves on next.
    let repcd = col("repcd");
    // The driver moves the counter, waits out a vsync and only then stores
    // the frame's step: the step a frame runs on is the one the vsync after
    // the counter's move reads.
    let mut frames: Vec<[i64; 5]> = Vec::new();
    let mut last = None;
    let mut step_due = false;
    for line in lines {
        let v: Vec<i64> = line.split(',').map(|x| x.parse().unwrap_or(0)).collect();
        if step_due && let Some(fr) = frames.last_mut() {
            fr[1] = v[dt];
        }
        step_due = false;
        if last != Some(v[gf]) {
            last = Some(v[gf]);
            frames.push([v[vs], v[dt], v[held], v[edge], repcd.map_or(0, |c| v[c])]);
            step_due = true;
        }
    }
    // Harness seat: the auto-repeat clock's phase, off the capture.
    if let (Some(_), Some(first), Some(p)) = (
        repcd,
        frames.first(),
        session.host.world.minigames.fishing.as_mut(),
    ) {
        p.seat_repeat_countdown(first[4] as i32);
    }
    for w in frames.windows(2).skip(1) {
        // The pad is the pump's at the frame's start.
        let [_, step, held, edge, _] = w[0];
        // The packed layout is the engine word with its bytes swapped.
        let held_e = (held as u16).rotate_left(8);
        let edge_e = (edge as u16).rotate_left(8);
        let input =
            legaia_engine_fishing::fishing::PondInput::from_engine_pad(held_e, held_e & !edge_e);
        if let Some(p) = session.host.world.minigames.fishing.as_mut() {
            p.tick(
                input,
                step.max(1) as i32,
                legaia_engine_fishing::fishing::CAST_POWER_STEP,
            );
            session.host.world.minigames.fishing_events = p.take_events();
        }
        // The venue's actors on the same game frame, as both play hosts run
        // them after the world tick: the lead's re-target roll and the
        // landed fish's trail draw from the pond's `rand()` stream.
        session.host.world.input.set_pad(held_e);
        legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(&mut session.host);
        let vals = values(sub, &session.host.world);
        writeln!(
            f,
            "{},{aligned_tick},{}",
            w[1][0],
            vals.iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )?;
    }
    Ok(())
}

fn baka_values(w: &legaia_engine_core::world::World) -> Vec<i64> {
    let Some(f) = w.minigames.baka_fighter.as_ref() else {
        return vec![-1; columns(MinigameSubId::BakaFighter).len()];
    };
    let words = f.cabinet().retail_words();
    let word = |addr: u32| {
        words
            .iter()
            .find(|(a, _)| *a == addr)
            .map_or(-1, |&(_, v)| i64::from(v))
    };
    let chosen = |slot| f.chosen(slot).map_or(0, |a| i64::from(a.type_id()));
    vec![
        word(0x801D_BF44),
        word(0x801D_C128),
        word(0x801D_BF88),
        word(0x801D_BE94),
        word(0x801D_BF8C),
        word(0x801D_BF78),
        i64::from(f.hp(0)),
        i64::from(f.hp(1)),
        i64::from(f.round_wins(0)),
        i64::from(f.round_wins(1)),
        chosen(0),
        chosen(1),
        i64::from(f.combo(0)),
        i64::from(f.combo(1)),
        i64::from(f.rng_state()),
    ]
}
