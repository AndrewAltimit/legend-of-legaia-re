//! Temporary investigation tool: census every battle formation on the disc
//! (both MAN carriers), sum the distinct monster ids' decoded block sizes
//! (PROT 867 slot prefix), and report the largest totals retail ever loads.
//! Run: LEGAIA_DISC_BIN=... cargo run --release -p legaia-patcher --example formation_census

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::encounter::SceneEncounters;
use std::collections::BTreeSet;

const SLOT_STRIDE: usize = 0x14000;

fn main() -> anyhow::Result<()> {
    let disc = std::env::var("LEGAIA_DISC_BIN")?;
    let image = std::fs::read(&disc)?;
    let patcher = DiscPatcher::open(image)?;

    // Decoded block size per monster id from the archive slot prefix.
    let archive = patcher.read_entry(867)?;
    let nslots = archive.len() / SLOT_STRIDE;
    let mut size_of = vec![0u32; nslots + 2];
    for i in 0..nslots {
        let off = i * SLOT_STRIDE;
        let sz = u32::from_le_bytes(archive[off..off + 4].try_into().unwrap());
        if sz > 0 && sz < 0x0010_0000 {
            size_of[i + 1] = sz;
        }
    }

    // (scene entry, formation idx, ids, distinct-sum, total-sum, random?)
    let mut rows: Vec<(usize, usize, Vec<u8>, u64, u64, bool)> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let mut scenes: Vec<SceneEncounters> = Vec::new();
        if let Some(s) = SceneEncounters::locate(&entry, idx) {
            scenes.push(s);
        }
        scenes.extend(SceneEncounters::locate_streaming_mans(&entry, idx));
        for s in scenes {
            for f in 0..s.formation_count() {
                let ids = s.formation_ids(f);
                let ids: Vec<u8> = ids.into_iter().filter(|&i| i != 0).collect();
                if ids.is_empty() {
                    continue;
                }
                let distinct: BTreeSet<u8> = ids.iter().copied().collect();
                let dsum: u64 = distinct
                    .iter()
                    .map(|&i| size_of.get(i as usize).copied().unwrap_or(0) as u64)
                    .sum();
                let tsum: u64 = ids
                    .iter()
                    .map(|&i| size_of.get(i as usize).copied().unwrap_or(0) as u64)
                    .sum();
                rows.push((idx, f, ids, dsum, tsum, s.is_random_formation(f)));
            }
        }
    }

    println!("total formations: {}", rows.len());
    rows.sort_by_key(|r| std::cmp::Reverse(r.3));
    println!("\ntop 25 by DISTINCT-id decoded-size sum (the RAM cost if blocks load once per id):");
    for (idx, f, ids, dsum, tsum, rnd) in rows.iter().take(25) {
        println!(
            "  entry {idx:4} formation {f:2}  ids {:?}  distinct-sum {:6.1} KB  total-sum {:6.1} KB  {}",
            ids,
            *dsum as f64 / 1024.0,
            *tsum as f64 / 1024.0,
            if *rnd { "random" } else { "SCRIPTED" }
        );
    }

    // Any multi-enemy formation containing a >90KB block?
    println!("\nmulti-enemy formations containing any id with block > 90 KB:");
    let mut found = 0;
    for (idx, f, ids, dsum, _tsum, rnd) in &rows {
        let distinct: BTreeSet<u8> = ids.iter().copied().collect();
        if ids.len() >= 2
            && distinct
                .iter()
                .any(|&i| size_of.get(i as usize).copied().unwrap_or(0) > 90 * 1024)
        {
            println!(
                "  entry {idx:4} formation {f:2}  ids {:?}  distinct-sum {:.1} KB  {}",
                ids,
                *dsum as f64 / 1024.0,
                if *rnd { "random" } else { "SCRIPTED" }
            );
            found += 1;
        }
    }
    if found == 0 {
        println!("  (none)");
    }

    // Largest distinct-COUNT formations.
    println!("\ntop 10 by distinct-id count:");
    let mut by_count = rows.clone();
    by_count.sort_by_key(|r| {
        let d: BTreeSet<u8> = r.2.iter().copied().collect();
        std::cmp::Reverse((d.len(), r.3))
    });
    for (idx, f, ids, dsum, _tsum, rnd) in by_count.iter().take(10) {
        println!(
            "  entry {idx:4} formation {f:2}  ids {:?}  distinct-sum {:6.1} KB  {}",
            ids,
            *dsum as f64 / 1024.0,
            if *rnd { "random" } else { "SCRIPTED" }
        );
    }
    Ok(())
}
