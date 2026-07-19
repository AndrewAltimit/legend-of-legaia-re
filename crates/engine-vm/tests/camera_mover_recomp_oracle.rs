//! Frame-level oracle: the op-`0x45` camera-mover law
//! ([`legaia_engine_vm::camera_mover`]) replayed against per-frame captures of
//! the retail camera globals (`0x8007B790` angle trio / `0x800840B8` eye trio
//! / `0x80089118` focus trio / `0x8007B6F4` H) from a static-recomp run of the
//! New Game opening chain.
//!
//! Sony-derived: the captures hold game camera values, so they live OUTSIDE
//! the repo and are supplied via `LEGAIA_RECOMP_TRACE_DIR` (a directory
//! containing the canonical frame-tagged JSONL traces - see
//! `docs/tooling/recomp-differential.md`). With the var unset every test
//! skip-passes, exactly like the disc-gated integration tests - CI never
//! needs the captures. Precedent: `rtpt_matches_recomp_cop2_capture`.
//!
//! What the beats cover (staged op-`0x45` params are decoded from the scene
//! MAN scripts; `curve = op0 >> 2`):
//!
//! - **snap** (`apply == 0`): the town01 arrival establishing shot lands in
//!   one captured frame.
//! - **mode 1 = linear on ALL ten slots, the angles included** - the town01
//!   `P2[3] +0x0361` pitch/yaw/eye glide and the opdeene `P2[18] +0x04B2`
//!   2000+-frame yaw dolly. A negative guard shows the falsified per-axis
//!   split ("mode 1 eases the angles out") misses the capture by an order of
//!   magnitude.
//! - **mode 2 = quadratic ease-out on all slots** - the map01 Rim Elm fly-in
//!   descent (`apply` 900), glided from the same-tick aerial snap pose.
//! - **mode 4 = quadratic ease-in-out on all slots** - the town01 arrival pan
//!   (`apply` 460) and the arrival H glide (`apply` 600, `op0 0x13`, H
//!   412 -> 512). A negative guard rejects reading that beat as mode 2.
//!
//! Comparison is per display frame with a small tick-skew window: retail's
//! mover adds the frame-skip factor `DAT_1F800393` (2-3 display frames per
//! logic tick) to its progress BEFORE evaluating, and holds the outputs
//! between logic ticks, so the captured value at display frame `f` equals the
//! law at some `t' = f - commit + skew`, `skew` in `-3..=+6`. Within that
//! window every beat below reproduces bit-exact (worst error 0-1 integer
//! units); the assertions allow 2.
//!
//! REF: FUN_801DC0BC

use legaia_engine_vm::camera_mover::axis_value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One captured frame of the retail camera globals, in raw retail units
/// (angles are the raw u16 globals, widened to i32; eye/focus/H as captured).
#[derive(Debug, Clone)]
struct Cam {
    scene: Option<String>,
    /// Mover-slot order: 0 pitch, 1 yaw, 2 roll, 3..5 eye, 6..8 focus, 9 H.
    slots: [Option<i32>; 10],
}

/// Angle slots (pitch/yaw/roll) compare wrap-aware in the 12-bit space.
fn is_angle(slot: u8) -> bool {
    slot <= 2
}

/// Wrap-aware difference: angles reduce modulo 4096 to `(-2048, 2048]`,
/// everything else is a plain difference.
fn slot_diff(slot: u8, a: i32, b: i32) -> i32 {
    let d = a - b;
    if is_angle(slot) {
        let m = d.rem_euclid(4096);
        if m > 2048 { m - 4096 } else { m }
    } else {
        d
    }
}

fn load_capture(dir: &std::path::Path, file: &str) -> BTreeMap<i64, Cam> {
    let path = dir.join(file);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read capture {}: {e}", path.display()));
    let mut out = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).expect("capture line is JSON");
        let frame = v["frame"].as_i64().expect("frame");
        let cam = &v["cam"];
        if cam.is_null() {
            continue;
        }
        let g = |k: &str| cam[k].as_i64().map(|x| x as i32);
        let arr = |k: &str, i: usize| {
            cam[k]
                .as_array()
                .and_then(|a| a.get(i)?.as_i64())
                .map(|x| x as i32)
        };
        out.insert(
            frame,
            Cam {
                scene: v["scene"].as_str().map(str::to_owned),
                slots: [
                    g("pitch"),
                    g("yaw"),
                    g("roll"),
                    arr("eye", 0),
                    arr("eye", 1),
                    arr("eye", 2),
                    arr("focus", 0),
                    arr("focus", 1),
                    arr("focus", 2),
                    g("h"),
                ],
            },
        );
    }
    assert!(!out.is_empty(), "capture {file} is empty");
    out
}

/// A staged glide beat: the slots it re-targets (mover slot index, target in
/// raw capture units) plus its `apply` duration and curve nibble.
struct Beat {
    targets: &'static [(u8, i32)],
    apply: i32,
    mode: i16,
}

/// Worst per-frame error of the mover law over `commit..=end`, with the
/// tick-skew window and per-slot curve selection (`curve_for` lets the
/// negative guards evaluate the falsified per-axis split). `start` is the
/// pose the glide re-seeds from. Frames missing from the capture are skipped.
/// Bails early once the running worst exceeds `bail_above`.
#[allow(clippy::too_many_arguments)]
fn beat_worst_err(
    cap: &BTreeMap<i64, Cam>,
    commit: i64,
    end: i64,
    start: &[(u8, i32)],
    beat: &Beat,
    curve_for: &dyn Fn(u8) -> i16,
    bail_above: i32,
) -> i32 {
    let start_of = |slot: u8| {
        start
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("start pose missing slot {slot}"))
    };
    let mut worst = 0i32;
    for (&f, cam) in cap.range(commit..=end) {
        let t = (f - commit) as i32;
        // Best over the tick-skew window: the mover adds dt (2-3) before
        // evaluating and holds between logic ticks.
        let mut best_this_frame = i32::MAX;
        for skew in -3i32..=6 {
            let tt = (t + skew).clamp(0, beat.apply);
            let mut max_err = 0i32;
            for &(slot, target) in beat.targets {
                let Some(got) = cam.slots[slot as usize] else {
                    continue;
                };
                let want = axis_value(start_of(slot), target, tt, beat.apply, curve_for(slot));
                max_err = max_err.max(slot_diff(slot, got, want).abs());
            }
            best_this_frame = best_this_frame.min(max_err);
        }
        worst = worst.max(best_this_frame);
        if worst > bail_above {
            return worst;
        }
    }
    worst
}

/// Start pose read straight from the capture at `commit`.
fn start_from_capture(cap: &BTreeMap<i64, Cam>, commit: i64, beat: &Beat) -> Vec<(u8, i32)> {
    let cam = &cap[&commit];
    beat.targets
        .iter()
        .map(|&(slot, _)| {
            (
                slot,
                cam.slots[slot as usize]
                    .unwrap_or_else(|| panic!("capture frame {commit} missing slot {slot}")),
            )
        })
        .collect()
}

/// Scan a commit-frame window and return the best `(commit, worst_err)` -
/// arrival-anchored commits are only exact to a few frames (an ease-out tail
/// reaches its integer targets slightly early), so the oracle self-anchors.
fn best_commit(
    cap: &BTreeMap<i64, Cam>,
    commits: impl Iterator<Item = i64>,
    end_of: &dyn Fn(i64) -> i64,
    start_of: &dyn Fn(i64) -> Vec<(u8, i32)>,
    beat: &Beat,
) -> (i64, i32) {
    let mut best = (0i64, i32::MAX);
    for c in commits {
        if !cap.contains_key(&c) {
            continue;
        }
        let err = beat_worst_err(
            cap,
            c,
            end_of(c),
            &start_of(c),
            beat,
            &|_| beat.mode,
            best.1.saturating_sub(1),
        );
        if err < best.1 {
            best = (c, err);
        }
    }
    assert_ne!(best.1, i32::MAX, "no commit candidate present in capture");
    best
}

fn trace_dir() -> Option<PathBuf> {
    match std::env::var("LEGAIA_RECOMP_TRACE_DIR") {
        Ok(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => {
            eprintln!(
                "SKIP camera_mover_recomp_oracle: set LEGAIA_RECOMP_TRACE_DIR to the \
                 external recomp camera-capture directory to run the frame-level cross-check"
            );
            None
        }
    }
}

/// The town01 arrival triple (run-2 frame-exact capture): snap, mode-4 pan,
/// mode-4 H glide. Pins the arrival H glide as **mode 4 ease-in-out**
/// (`op0 0x13 >> 2`), not mode 2 - the negative guard rejects ease-out.
#[test]
fn town01_arrival_snap_pan_and_h_glide_match_frame_exact() {
    let Some(dir) = trace_dir() else { return };
    let cap = load_capture(&dir, "town01_arrival_hglide_run2.jsonl");

    // Beat +0x0091 (apply 0): the establishing snap. Lands in ONE frame.
    let snap: &[(u8, i32)] = &[
        (0, 250),
        (1, 65138),
        (3, 1735),
        (4, 74),
        (5, 32100),
        (6, -3940),
        (8, -2014),
        (9, 412),
    ];
    let matches_pose =
        |cam: &Cam, pose: &[(u8, i32)]| pose.iter().all(|&(s, v)| cam.slots[s as usize] == Some(v));
    let f_snap = *cap
        .iter()
        .find(|(_, c)| matches_pose(c, snap))
        .expect("the arrival snap pose appears in the capture")
        .0;
    let before = cap
        .range(..f_snap)
        .next_back()
        .expect("capture starts before the snap");
    assert!(
        !matches_pose(before.1, snap),
        "apply == 0 lands the full pose in a single frame (frame {f_snap})"
    );

    // Beat +0x00A6 (apply 460, mode 4): the pan. Commits the same tick as the
    // snap (no yield between), so scan a few frames from the snap.
    let pan = Beat {
        targets: &[
            (0, 250),
            (1, 65378),
            (3, -165),
            (4, 74),
            (5, 32100),
            (6, -3940),
            (8, -2014),
            (9, 412),
        ],
        apply: 460,
        mode: 4,
    };
    let (pan_commit, pan_err) = best_commit(
        &cap,
        f_snap..=f_snap + 4,
        &|c| c + 460,
        &|c| start_from_capture(&cap, c, &pan),
        &pan,
    );
    assert!(
        pan_err <= 2,
        "mode-4 pan diverges: worst err {pan_err} (commit {pan_commit})"
    );

    // Beat +0x00C4 (apply 600, MODE 4, op0 0x13): the arrival H glide,
    // H 412 -> 512 participating like every other slot. Commits when the
    // pan's 460-frame wait drains.
    let h_glide = Beat {
        targets: &[
            (0, 186),
            (1, 65366),
            (3, -2585),
            (4, 3994),
            (5, -380),
            (6, -3940),
            (8, -2014),
            (9, 512),
        ],
        apply: 600,
        mode: 4,
    };
    let (h_commit, h_err) = best_commit(
        &cap,
        pan_commit + 455..=pan_commit + 465,
        &|c| c + 600,
        &|c| start_from_capture(&cap, c, &h_glide),
        &h_glide,
    );
    assert!(
        h_err <= 2,
        "mode-4 H glide diverges: worst err {h_err} (commit {h_commit})"
    );

    // Negative guard: the same beat under mode 2 (quadratic ease-out) misses
    // the capture mid-flight by orders of magnitude - the beat is NOT mode 2.
    let as_mode2 = beat_worst_err(
        &cap,
        h_commit,
        h_commit + 600,
        &start_from_capture(&cap, h_commit, &h_glide),
        &h_glide,
        &|_| 2,
        i32::MAX,
    );
    assert!(
        as_mode2 > 100,
        "non-vacuous: reading the H glide as mode 2 must fail (err {as_mode2})"
    );

    eprintln!(
        "town01 arrival: snap@{f_snap} exact; pan commit {pan_commit} worst {pan_err}; \
         H glide commit {h_commit} worst {h_err}; mode-2 misread err {as_mode2}"
    );
}

/// Mode 1 is linear on EVERY slot, the pitch/yaw angles included - two
/// independent beats (a 120-frame pan with both angles moving, and the
/// opdeene multi-thousand-frame yaw dolly). The falsified per-axis split
/// (angles quad-ease-out under mode 1) fails both by an order of magnitude.
#[test]
fn mode1_glides_are_linear_on_all_slots_including_angles() {
    let Some(dir) = trace_dir() else { return };

    // town01 P2[3] +0x0361: apply 120, mode 1, pitch + yaw + eye all staged.
    let cap = load_capture(&dir, "town01_prologue_cam.jsonl");
    let b361 = Beat {
        targets: &[
            (0, 232),
            (1, 710),
            (3, 160),
            (4, 392),
            (5, 5920),
            (6, -5184),
            (8, -13504),
            (9, 512),
        ],
        apply: 120,
        mode: 1,
    };
    let arrival = *cap
        .iter()
        .find(|(_, c)| {
            b361.targets
                .iter()
                .all(|&(s, v)| c.slots[s as usize] == Some(v))
        })
        .expect("the +0x0361 target pose appears in the capture")
        .0;
    let (c361, e361) = best_commit(
        &cap,
        arrival - 126..=arrival - 114,
        &|c| c + 120,
        &|c| start_from_capture(&cap, c, &b361),
        &b361,
    );
    assert!(
        e361 <= 2,
        "mode-1 town01 +0x0361 glide diverges: worst err {e361} (commit {c361})"
    );
    let start361 = start_from_capture(&cap, c361, &b361);
    let split361 = beat_worst_err(
        &cap,
        c361,
        c361 + 120,
        &start361,
        &b361,
        &|slot| if is_angle(slot) { 2 } else { 1 },
        i32::MAX,
    );
    assert!(
        split361 > 20,
        "non-vacuous: quad-out angles under mode 1 must fail (err {split361})"
    );

    // opdeene P2[18] +0x04B2: apply 4800, mode 1, yaw + eye - a dolly that
    // runs to the scene change (retail never arrives; the capture pins over
    // 2000 frames of constant-velocity travel, yaw included).
    let chain = load_capture(&dir, "chain_cam_full.jsonl");
    let opdeene_end = *chain
        .iter()
        .rfind(|(_, c)| c.scene.as_deref() == Some("opdeene"))
        .expect("chain capture covers opdeene")
        .0;
    let dolly = Beat {
        targets: &[
            (0, 180),
            (1, 61716),
            (2, 0),
            (3, 2400),
            (4, 842),
            (5, 18432),
            (6, -8640),
            (8, -10304),
            (9, 792),
        ],
        apply: 4800,
        mode: 1,
    };
    let commits: Vec<i64> = chain
        .iter()
        .filter(|(_, c)| c.scene.as_deref() == Some("opdeene"))
        .map(|(&f, _)| f)
        .collect();
    let (cd, ed) = best_commit(
        &chain,
        commits.into_iter(),
        &|_| opdeene_end,
        &|c| start_from_capture(&chain, c, &dolly),
        &dolly,
    );
    let span = opdeene_end - cd;
    assert!(
        span > 2000,
        "non-vacuous: the dolly must cover a long capture span (got {span})"
    );
    assert!(
        ed <= 2,
        "mode-1 opdeene yaw dolly diverges: worst err {ed} (commit {cd}, {span} frames)"
    );
    let startd = start_from_capture(&chain, cd, &dolly);
    let splitd = beat_worst_err(
        &chain,
        cd,
        opdeene_end,
        &startd,
        &dolly,
        &|slot| if is_angle(slot) { 2 } else { 1 },
        i32::MAX,
    );
    assert!(
        splitd > 100,
        "non-vacuous: quad-out angles on the yaw dolly must fail (err {splitd})"
    );

    eprintln!(
        "mode 1 linear: +0x0361 commit {c361} worst {e361} (split-guard {split361}); \
         opdeene dolly commit {cd} worst {ed} over {span} frames (split-guard {splitd})"
    );
}

/// Mode 2 (quadratic ease-out on all slots): the map01 Rim Elm fly-in
/// descent, glided FROM the same-tick aerial snap pose (`+0x0109`) - the two
/// beats execute with no yield between, so the mover's start is the snapped
/// pose, not the pre-beat camera.
#[test]
fn map01_flyin_mode2_descent_matches_frame_exact() {
    let Some(dir) = trace_dir() else { return };
    let chain = load_capture(&dir, "chain_cam_full.jsonl");
    // The staged +0x0109 snap pose (apply 0), which the descent re-seeds from.
    let aerial: &[(u8, i32)] = &[
        (0, 735),
        (1, 93),
        (3, -1268),
        (4, -3756),
        (5, 18784),
        (6, -12162),
        (8, -3510),
        (9, 368),
    ];
    let descent = Beat {
        targets: &[
            (0, 355),
            (1, 333),
            (3, 412),
            (4, -2336),
            (5, 12384),
            (6, -12162),
            (8, -3510),
            (9, 368),
        ],
        apply: 900,
        mode: 2,
    };
    let commits: Vec<i64> = chain
        .iter()
        .filter(|(_, c)| c.scene.as_deref() == Some("map01"))
        .map(|(&f, _)| f)
        .collect();
    assert!(!commits.is_empty(), "chain capture covers map01");
    let last = *commits.last().unwrap();
    let (c, e) = best_commit(
        &chain,
        commits.into_iter(),
        &|c| (c + 900).min(last),
        &|_| aerial.to_vec(),
        &descent,
    );
    assert!(
        e <= 2,
        "mode-2 fly-in descent diverges: worst err {e} (commit {c})"
    );
    eprintln!("map01 fly-in: commit {c} worst {e}");
}
