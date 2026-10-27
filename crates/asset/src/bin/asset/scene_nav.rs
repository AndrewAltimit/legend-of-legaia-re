//! `asset scene-nav` - the navigation facts of a field scene, as JSON, for an
//! offline route planner (`scripts/pcsx-redux/plan_walk_route.py`).
//!
//! Per CDNAME scene label it reads the block's `.MAP` (the first entry,
//! exactly `0x12000` bytes; [`legaia_scene_formats::field_map`]) and the
//! scene bundle's MAN, and emits:
//!
//! * `grid` - the `+0x4000` collision grid, 128 x 128 bytes row-major (high
//!   nibble = the four sub-cell wall bits);
//! * `trig_primary` / `trig_fallback` - every trigger record of the `.MAP`'s
//!   `+0x10000` block and of the fallback window retail's lookup scans
//!   second (the block's next entry), as `{kind, rec}`;
//! * `doors` - every `0x3F` scene-change site of the MAN with the
//!   partition-2 slot that carries it (the record a kind-1 gate-1 trigger
//!   names), its destination scene and entry tile;
//! * `p1` - the partition-1 placements (index, tile, world position, model).
//!
//! The output is derived from disc bytes: keep it under `captures/` or a
//! scratch directory, never in the repository.

use anyhow::{Context, Result};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;
use legaia_scene_formats::{field_map, man_edit, man_section, scene_asset_table};
use serde_json::{Value, json};
use std::path::Path;

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// Every record of the four trigger sub-tables of one trigger block (header:
/// `s16` offset at `+4k+2`, `s16` count at `+4k+4`, strides `[4, 4, 4, 8]`).
fn trigger_rows(block: &[u8]) -> Vec<Value> {
    let rd = |o: usize| -> Option<i16> {
        Some(i16::from_le_bytes([*block.get(o)?, *block.get(o + 1)?]))
    };
    let mut out = Vec::new();
    for (k, &stride) in field_map::TRIGGER_KIND_STRIDES.iter().enumerate() {
        let (Some(off), Some(cnt)) = (rd(4 * k + 2), rd(4 * k + 4)) else {
            continue;
        };
        if off < 0 || cnt <= 0 || cnt > 0x400 {
            continue;
        }
        for i in 0..cnt as usize {
            let at = off as usize + i * stride;
            if let Some(r) = block.get(at..at + stride) {
                out.push(json!({ "kind": k, "rec": r }));
            }
        }
    }
    out
}

fn scene_json(ar: &mut Archive, map: &cdname::IndexMap, name: &str) -> Result<Value> {
    let raw = *map
        .iter()
        .find(|(_, v)| v.as_str() == name)
        .with_context(|| format!("no CDNAME block named {name}"))?
        .0;
    let next = map
        .range(raw + 1..)
        .next()
        .map(|(k, _)| *k)
        .unwrap_or(raw + 20);
    let first = raw.saturating_sub(cdname::RAW_TOC_INDEX_OFFSET) as usize;
    let end = (next.saturating_sub(cdname::RAW_TOC_INDEX_OFFSET) as usize).min(ar.entries.len());
    let mut o = serde_json::Map::new();
    let mut buf = Vec::new();
    let map_entry = ar.entries[first].clone();
    ar.read_entry(&map_entry, &mut buf)?;
    if let Some(fm) = field_map::detect(&buf) {
        o.insert("grid".into(), json!(fm.collision_grid()));
        o.insert(
            "trig_primary".into(),
            json!(trigger_rows(fm.trigger_bytes())),
        );
        if let Some(sib) = ar.entries.get(first + 1).cloned() {
            let mut s = Vec::new();
            ar.read_entry(&sib, &mut s)?;
            o.insert("trig_fallback".into(), json!(trigger_rows(&s)));
        }
    }
    for idx in first..end {
        let entry = ar.entries[idx].clone();
        ar.read_entry(&entry, &mut buf)?;
        let Some(table) = scene_asset_table::detect(&buf) else {
            continue;
        };
        let Some(mi) = table.descriptor_index(MAN_TYPE) else {
            continue;
        };
        let d = table.used()[mi];
        if d.size == 0 || d.data_offset == 0 {
            continue;
        }
        let Some(body) = buf.get(d.data_offset as usize..) else {
            continue;
        };
        let Ok((man, _)) = legaia_lzs::decompress_tracked(body, d.size as usize) else {
            continue;
        };
        let Ok(mf) = man_section::parse(&man) else {
            continue;
        };
        let sites = man_edit::scene_change_sites(&man);
        let slots = man_edit::site_p2_slots(&man, &sites);
        let doors: Vec<Value> = sites
            .iter()
            .zip(slots)
            .map(|(s, slot)| {
                json!({ "slot": slot, "dest": s.name, "ex": s.entry_x, "ez": s.entry_z,
                        "dir": s.dir, "part": s.partition, "pc": s.op_pc })
            })
            .collect();
        let p1: Vec<Value> = mf
            .actor_placements(&man)
            .iter()
            .map(|a| {
                json!({ "i": a.index, "tx": a.tile_x, "tz": a.tile_z, "x": a.world_x,
                        "z": a.world_z, "model": a.model_index, "off": a.record_offset })
            })
            .collect();
        o.insert("bundle_entry".into(), json!(idx));
        o.insert("doors".into(), json!(doors));
        o.insert("p1".into(), json!(p1));
        break;
    }
    Ok(Value::Object(o))
}

pub(crate) fn scene_nav_cmd(prot: &Path, cdname_path: &Path, scenes: &[String]) -> Result<()> {
    let text = std::fs::read_to_string(cdname_path)
        .with_context(|| format!("reading {}", cdname_path.display()))?;
    let map = cdname::parse_str(&text)?;
    let mut ar = Archive::open(prot)?;
    let mut all = serde_json::Map::new();
    for name in scenes {
        all.insert(name.clone(), scene_json(&mut ar, &map, name)?);
    }
    println!("{}", Value::Object(all));
    Ok(())
}
