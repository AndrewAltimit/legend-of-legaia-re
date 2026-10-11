//! Parts A and B: the disc-sourced chapter-1 closure and the per-scene frontier ladder,
//! with the ratchet baseline they read.

use super::*;

// ---------------------------------------------------------------------------
// Baseline
// ---------------------------------------------------------------------------

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/chapter1_frontier_baseline.toml")
}

/// Parse `key = N` out of the baseline file. A missing file or key reads as
/// `0`, so a fresh clone starts from "no progress claimed" rather than
/// failing on a file it cannot see.
pub(super) fn read_baseline(key: &str) -> usize {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return 0;
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            l.strip_prefix(key)?
                .trim()
                .strip_prefix('=')?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Part A: the graph
// ---------------------------------------------------------------------------

/// The chapter-1 closure is disc-sourced and terminates at a kingdom
/// boundary, and the boundary is real (a chapter-2 overworld is named by some
/// chapter-1 scene, and it is not expanded).
#[test]
fn part_a_chapter1_closure_is_disc_sourced_and_bounded() {
    let Some(host) = open_host() else {
        return;
    };
    let Some(_) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Closure {
        order,
        edges,
        boundary,
    } = chapter1_closure(&host.index);
    assert!(
        order.len() <= MAX_CLOSURE,
        "closure hit the {MAX_CLOSURE} cap - the destination decode is naming phantoms"
    );
    assert_eq!(order.first().map(String::as_str), Some(CH1_ROOT));
    assert!(
        order.iter().any(|s| s == CH1_OVERWORLD),
        "the closure must reach the Drake overworld"
    );
    assert!(
        order.iter().any(|s| s == "keikoku"),
        "the closure must reach the Ravine - the frontier this ladder extends past"
    );
    // Past the Ravine: the boss chain and the castle interior are in the
    // closure, reached by the disc's own destination tables and not by a
    // hand-written list.
    for past in ["rikuroa", "jou", "jouina"] {
        assert!(
            order.iter().any(|s| s == past),
            "the closure must reach {past} (past keikoku); got {order:?}"
        );
    }
    eprintln!(
        "[graph] chapter-1 closure = {} scenes from {CH1_ROOT}",
        order.len()
    );
    for s in &order {
        let outs = edges.get(s).map(BTreeSet::len).unwrap_or(0);
        eprintln!("  {s:<10} -> {outs} destination(s)");
    }
    eprintln!("[graph] kingdom boundary edges (not expanded):");
    for (from, to) in &boundary {
        eprintln!("  {from} -> {to}");
    }
    assert!(
        !boundary.is_empty(),
        "chapter 1 must name at least one other kingdom overworld - a closure \
         with no boundary means the bound is not being exercised"
    );
    // The Drake kingdom hands off to Sebucus at exactly one place, and it is
    // Jiji Village. Pinning the edge (rather than just "some boundary
    // exists") is what makes the bound a measurement: a decode change that
    // opened a second handoff, or moved this one, trips here.
    assert!(
        boundary.contains(&("jiji".to_string(), "map02".to_string())),
        "the Drake -> Sebucus handoff is jiji -> map02; got {boundary:?}"
    );
}

// ---------------------------------------------------------------------------
// Part B: the per-scene ladder
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)]
fn part_b_chapter1_scene_frontier_ladder() {
    let Some(mut host) = open_host() else {
        return;
    };
    let Some(_) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Closure {
        order, boundary, ..
    } = chapter1_closure(&host.index);

    // Every scene is scored from the same starting state: the flag banks are
    // snapshotted and restored, so a verdict is a property of the scene and
    // not of whatever the previous scene latched.
    let flags0 = host.world.flags.system_flags.clone();
    let story0 = host.world.flags.story_flags;

    let mut verdicts: Vec<Verdict> = Vec::new();
    for name in &order {
        host.world.flags.system_flags = flags0.clone();
        host.world.flags.story_flags = story0;
        verdicts.push(score_scene(&mut host, name));
    }

    // -- The table --------------------------------------------------------
    eprintln!();
    eprintln!("chapter-1 scene frontier ladder (closure of {CH1_ROOT} over the disc's 0x3F table)");
    eprintln!(
        "rungs: 1 {} | 2 {} | 3 {} | 4 {} | 5 {} | 6 {}",
        RUNGS[0], RUNGS[1], RUNGS[2], RUNGS[3], RUNGS[4], RUNGS[5]
    );
    eprintln!("marks: * cleared   X failed   . not attempted   ~ n/a (script departs)");
    eprintln!();
    eprintln!("{:<10} 123456  score  note", "scene");
    eprintln!("{}", "-".repeat(96));
    for v in &verdicts {
        let glyphs: String = v.marks.iter().map(|m| m.glyph()).collect();
        eprintln!("{:<10} {glyphs}  {:>5}  {}", v.scene, v.score, v.note);
    }
    eprintln!("{}", "-".repeat(96));

    // -- The summary ------------------------------------------------------
    let scenes = verdicts.len();
    let rung_total: usize = verdicts.iter().map(|v| v.score).sum();
    let full_ladder = verdicts.iter().filter(|v| v.score == RUNGS.len()).count();
    let departed = verdicts.iter().filter(|v| v.departed).count();
    let landed = verdicts
        .iter()
        .filter(|v| v.landed_on_entry_tile == Some(true))
        .count();
    let exit_fired = verdicts
        .iter()
        .filter(|v| v.landed_on_entry_tile.is_some())
        .count();
    let mut stopped_at: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for v in &verdicts {
        if v.score < RUNGS.len() && !v.departed {
            stopped_at
                .entry(v.score)
                .or_default()
                .push(v.scene.as_str());
        }
    }

    eprintln!("scenes             {scenes}");
    eprintln!("rung_total         {rung_total} / {}", scenes * RUNGS.len());
    eprintln!("full_ladder        {full_ladder}");
    eprintln!("scripted departure {departed} (ladder capped at rung 4 by design)");
    eprintln!("arrival tile match {landed} / {exit_fired} exits that fired");
    eprintln!("kingdom boundary   {} edge(s)", boundary.len());
    for (score, scenes) in &stopped_at {
        let rung = RUNGS.get(*score).copied().unwrap_or("?");
        eprintln!(
            "  stopped before rung {} ({rung}): {}",
            score + 1,
            scenes.join(" ")
        );
    }

    // -- The ratchet ------------------------------------------------------
    let b_scenes = read_baseline("scenes");
    let b_total = read_baseline("rung_total");
    let b_full = read_baseline("full_ladder");
    if scenes > b_scenes || rung_total > b_total || full_ladder > b_full {
        eprintln!();
        eprintln!("baseline can be raised in {}:", baseline_path().display());
        eprintln!("  scenes = {scenes}");
        eprintln!("  rung_total = {rung_total}");
        eprintln!("  full_ladder = {full_ladder}");
    }
    assert!(
        scenes >= b_scenes,
        "chapter-1 closure shrank: {scenes} < baseline {b_scenes} - a destination \
         decode regression drops scenes off the graph"
    );
    assert!(
        rung_total >= b_total,
        "chapter-1 frontier score regressed: {rung_total} < baseline {b_total}"
    );
    assert!(
        full_ladder >= b_full,
        "scenes clearing the whole ladder regressed: {full_ladder} < baseline {b_full}"
    );
}
