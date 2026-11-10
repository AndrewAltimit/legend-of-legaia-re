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
    // A module's seek arm holds the engine's whole frame (`drive_seek_arm`,
    // `CD-seek waits`): its still ticks are the emulator-less wait the image
    // pairing absorbs, and paired one for one they read as a framing error.
    let frozen = |t: u64| -> bool {
        let Ok(i) = engine.rows.binary_search_by_key(&t, |x| x.t) else {
            return false;
        };
        let row = &engine.rows[i];
        // A tick its battle frame overran drains no game time either
        // (`Side::live_clock`).
        row.over
            || (i > 0
                && row.drive.is_some_and(|d| row.phase == d.seek_arm)
                && row.display_frames.is_some()
                && engine.rows[i - 1].display_frames == row.display_frames)
    };
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
        let mut tick = eseg.start;
        let mut live = 0;
        while live < into && tick < eseg.end {
            tick += 1;
            if !frozen(tick) {
                live += 1;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn every_retail_row_pairs_on_the_segment_clock() {
        let root = std::env::temp_dir().join(format!("cam-channel-{}", std::process::id()));
        let (r, e) = (root.join("retail"), root.join("engine"));
        std::fs::create_dir_all(&r).unwrap();
        std::fs::create_dir_all(&e).unwrap();
        write(
            &r,
            "meta.csv",
            "vsync,st7,phase,step,acting,shot\n10,40,0,2,0,s\n11,40,0,2,0,\n12,41,0,2,0,\n13,41,0,2,0,\n",
        );
        write(
            &r,
            "cam.csv",
            "vsync,st7,phase,step,pitch,yaw,trx,try,trz\n\
             10,40,0,2,1,100,0,1280,4000\n11,40,0,2,1,101,0,1280,4000\n\
             12,41,0,2,2,200,0,1280,4000\n13,41,0,2,2,201,0,1280,4000\n",
        );
        write(
            &r,
            "body.csv",
            "vsync,fx,fz\n10,5,6\n11,5,6\n12,7,8\n13,7,8\n",
        );
        write(
            &e,
            "meta.csv",
            "tick,st7,phase,acting\n100,40,0,0\n101,40,0,0\n102,41,0,0\n",
        );
        write(
            &e,
            "cam.csv",
            "tick,st7,phase,pitch,yaw,trx,try,trz,fx,fy,fz,c6da,step\n\
             100,40,0,1,100,0,1280,4000,5,0,6,0,2\n101,40,0,1,101,0,1280,4000,5,0,6,0,2\n\
             102,41,0,2,210,0,1280,4000,7,0,8,0,2\n",
        );
        let out = root.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let (a, b) = (Side::read(&r).unwrap(), Side::read(&e).unwrap());
        assert!(write_camera_csv(&a, &b, &out).unwrap());
        let text = std::fs::read_to_string(out.join("camera.csv")).unwrap();
        let rows: Vec<&str> = text.lines().skip(1).collect();
        assert_eq!(rows.len(), 4);
        assert!(rows[0].starts_with("10,100,state_0x28,0,"));
        // Past the engine segment's end: clamped to its last tick.
        assert!(rows[3].starts_with("13,102,state_0x29,1,"));
        std::fs::remove_dir_all(&root).ok();
    }
}
