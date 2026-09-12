//! `asset seru-side-effect` / `asset formation-census`: the Seru-magic
//! side-effect table out of PROT 0898, and the disc-wide formation census that
//! tells which fights each monster is met in.

use std::path::Path;

use anyhow::Result;
use legaia_asset::formation_census::FormationCensus;
use legaia_asset::seru_side_effect::{
    SIDE_EFFECT_BANDS, SIDE_EFFECT_TABLE_FILE_OFFSET, SeruSideEffectTable, SideEffectKind,
};

const ELEMENT_NAMES: [&str; 8] = [
    "earth", "water", "fire", "wind", "thunder", "light", "dark", "neutral",
];

pub(crate) fn seru_side_effect_cmd(input: &Path, json: bool) -> Result<()> {
    let bytes = crate::common::read_input(input)?;
    let Some(table) = SeruSideEffectTable::parse(&bytes) else {
        anyhow::bail!(
            "no side-effect table at the pinned offset {:#x} in {} ({} bytes) - is this the \
             raw PROT 0898 battle-action overlay entry?",
            SIDE_EFFECT_TABLE_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&table)?);
        return Ok(());
    }
    println!(
        "Seru-magic side-effect table @ file {:#x} (runtime VA 0x801F6870); \
         amount = percent shaved per hit (light row: cure class); band = (level - 3) >> 1",
        SIDE_EFFECT_TABLE_FILE_OFFSET
    );
    println!(
        "{:<8} {:<6} {:>10} {:>10} {:>10} {:>10}",
        "element", "kind", "lv 3-4", "lv 5-6", "lv 7-8", "lv 9"
    );
    for (e, row) in table.rows().iter().enumerate() {
        let kind = SideEffectKind::for_element(e as u8);
        let mut line = format!("{:<8} {:<6}", ELEMENT_NAMES[e], kind.label());
        for rec in row.iter().take(SIDE_EFFECT_BANDS) {
            line.push_str(&format!(" {:>3} @{:08x}", rec.amount, rec.banner_va));
        }
        println!("{line}");
    }
    Ok(())
}

pub(crate) fn formation_census_cmd(input: &Path, json: bool) -> Result<()> {
    let census = if input.is_dir() {
        let mut paths: Vec<_> = std::fs::read_dir(input)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        let entries: Vec<(usize, Vec<u8>)> = paths
            .iter()
            .enumerate()
            .filter_map(|(i, p)| std::fs::read(p).ok().map(|b| (i, b)))
            .collect();
        FormationCensus::from_entries(entries.iter().map(|(i, b)| (*i, b.as_slice())))
    } else {
        let mut archive = legaia_prot::archive::Archive::open(input)?;
        let metas = archive.entries.clone();
        let mut entries: Vec<(usize, Vec<u8>)> = Vec::with_capacity(metas.len());
        let mut buf = Vec::new();
        for (i, meta) in metas.iter().enumerate() {
            buf.clear();
            if archive.read_entry(meta, &mut buf).is_ok() {
                entries.push((i, buf.clone()));
            }
        }
        FormationCensus::from_entries(entries.iter().map(|(i, b)| (*i, b.as_slice())))
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&census)?);
        return Ok(());
    }
    println!(
        "formation census: {} MAN encounter sections, {} rows; per monster id: rows with a \
         non-zero header (scripted), zero header (clear), region-rollable (random)",
        census.scenes, census.rows
    );
    println!("  id scripted  clear  random  met as    side-effects the loader boost blocks");
    for (id, rows) in &census.monsters {
        let met = match (rows.met_scripted(), rows.met_unflagged()) {
            (true, true) => "both",
            (true, false) => "scripted",
            (false, true) => "unflagged",
            (false, false) => "-",
        };
        let blocked: Vec<&str> = if rows.met_scripted() {
            // Without the record we can only say which stats the scripted
            // profile boosts; the per-record verdict is `asset monster-archive`'s.
            vec!["DEF", "ATK", "INT"]
        } else {
            vec![]
        };
        println!(
            "{:>4} {:>8} {:>6} {:>7}  {:<9} {}",
            id,
            rows.flagged,
            rows.clear,
            rows.random,
            met,
            blocked.join("/")
        );
    }
    Ok(())
}
