//! The cast-sequence oracle's camera channel: the two sides' `cam.csv` read on
//! the pairing's own clock (`docs/tooling/cast-sequence-oracle.md`, "The camera
//! channel").
//!
//! The image score pairs retail frames and reads the camera only at those
//! grabs. This channel pairs **every** retail vsync row with an engine tick:
//! RAM against RAM, so no display lag, with the segment alignment and the game
//! clock the image pairs use. A row writes both cameras to `camera.csv`; the
//! ranker `scripts/ci/cast-camera-rank.py` reads the file.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;

use crate::cast_sequence::{Side, align, field, parse_csv};

/// One side's camera on one row.
#[derive(Debug, Clone, Copy, Default)]
struct Pose {
    pitch: i32,
    yaw: i32,
    tr: [i32; 3],
    focus: Option<[i32; 2]>,
    step: i32,
}

fn poses(dir: &Path, retail: bool) -> Result<BTreeMap<u64, Pose>> {
    let cam = dir.join("cam.csv");
    let mut out = BTreeMap::new();
    if !cam.exists() {
        return Ok(out);
    }
    let key = if retail { "vsync" } else { "tick" };
    for r in parse_csv(&cam)? {
        let Some(t) = field::<u64>(&r, key) else {
            continue;
        };
        let g = |k: &str| field::<i32>(&r, k).unwrap_or(0);
        out.insert(
            t,
            Pose {
                pitch: g("pitch"),
                yaw: g("yaw"),
                tr: [g("trx"), g("try"), g("trz")],
                focus: field::<i32>(&r, "fx")
                    .zip(field::<i32>(&r, "fz"))
                    .map(|(x, z)| [x, z]),
                step: g("step"),
            },
        );
    }
    // Retail's focus is in `body.csv`, one row per vsync.
    if retail && dir.join("body.csv").exists() {
        for r in parse_csv(&dir.join("body.csv"))? {
            if let (Some(t), Some(x), Some(z)) = (
                field::<u64>(&r, "vsync"),
                field::<i32>(&r, "fx"),
                field::<i32>(&r, "fz"),
            ) && let Some(p) = out.get_mut(&t)
            {
                p.focus = Some([x, z]);
            }
        }
    }
    Ok(out)
}

/// Write `camera.csv` under `out`: every retail row paired with the engine tick
/// that many game-clock vsyncs into the aligned segment, clamped to its end.
/// `Ok(false)` when a side has no `cam.csv`.
pub fn write_camera_csv(retail: &Side, engine: &Side, out: &Path) -> Result<bool> {
    let (rp, ep) = (poses(&retail.dir, true)?, poses(&engine.dir, false)?);
    if rp.is_empty() || ep.is_empty() {
        return Ok(false);
    }
    // The same window the image pairs score: from the first frame's own row.
    let first = retail
        .rows
        .iter()
        .find(|r| r.shot.is_some())
        .map_or(0, |r| r.t.saturating_sub(crate::cast_sequence::MAX_LAG));
    let r = Side {
        dir: retail.dir.clone(),
        rows: retail
            .rows
            .iter()
            .filter(|r| r.t >= first)
            .cloned()
            .collect(),
    };
    let (rs, es) = (r.segments(), engine.segments());
    let clock = r.game_clock();
    let game = |t: u64| -> u64 {
        clock
            .as_ref()
            .and_then(|c| {
                c.get(r.rows.binary_search_by_key(&t, |x| x.t).ok()?)
                    .copied()
            })
            .unwrap_or(t)
    };
    let to_engine: BTreeMap<usize, usize> = align(&rs, &es).into_iter().collect();
    let mut csv = String::from(
        "vsync,tick,key,into,r_step,e_step,r_pitch,r_yaw,r_trx,r_try,r_trz,r_fx,r_fz,\
         e_pitch,e_yaw,e_trx,e_try,e_trz,e_fx,e_fz\n",
    );
    for row in &r.rows {
        let Some(ri) = rs.iter().position(|s| s.start <= row.t && row.t <= s.end) else {
            continue;
        };
        let Some(&ei) = to_engine.get(&ri) else {
            continue;
        };
        let (rseg, eseg) = (&rs[ri], &es[ei]);
        let into = game(row.t).saturating_sub(game(rseg.start));
        let tick = (eseg.start + into).min(eseg.end);
        let (Some(a), Some(b)) = (rp.get(&row.t), ep.get(&tick)) else {
            continue;
        };
        let focus = |f: Option<[i32; 2]>| f.map_or(",".to_string(), |[x, z]| format!("{x},{z}"));
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            row.t,
            tick,
            format!("{}", rseg.key).replace(' ', "_"),
            into,
            row.step,
            b.step,
            a.pitch,
            a.yaw,
            a.tr[0],
            a.tr[1],
            a.tr[2],
            focus(a.focus),
            b.pitch,
            b.yaw,
            b.tr[0],
            b.tr[1],
            b.tr[2],
            focus(b.focus),
        )?;
    }
    std::fs::write(out.join("camera.csv"), csv)?;
    Ok(true)
}
