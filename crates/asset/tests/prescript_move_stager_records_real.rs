//! Disc-gated proof that the `scene_event_scripts` / `scene_v12_table` /
//! `scene_scripted_asset_table` prescript records are **summon-stager-format
//! move-VM records** - `[i16 model_sel][u16 flags][move-VM bytecode]`, the same
//! shape the per-summon stagers use - NOT a bespoke "scene event command VM".
//!
//! Runtime chain (pinned from disc + resident kingdom-overworld RAM): the field
//! VM `FUN_801DE840` installs a record by id via `FUN_800252EC`
//! (`record = bundle_base + offsets[id]`) → the part-stager `FUN_80021B04`
//! (`actor[+0x48] = record`) → the move VM `FUN_80023070` runs `record+4`
//! (op `0x08` = Halt). See `scene_event_scripts::move_stager_records` and
//! `docs/formats/scene-bundles.md`.
//!
//! The non-vacuous signal: every record's `model_sel` lead classifies as a valid
//! stager kind (transform node `-1`, library mesh `0..N`, or render-mode node
//! `0x4000/0x4001`) - the exact distribution the summon stagers exhibit - with
//! effectively zero out-of-range "garbage" leads. Random `[count][offsets]`-shaped
//! data would carry arbitrary `model_sel` words and fail this.
//!
//! Skips silently when `extracted/PROT/` is missing.

use legaia_asset::scene_event_scripts::{detect, move_stager_records};
use legaia_asset::summon_overlay::SummonPartKind;
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn bin_entries(prot: &PathBuf) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    v.sort();
    v
}

/// A record's `model_sel` lead is a valid summon-stager kind: transform node
/// (`-1`), library mesh (`0..N`), or render-mode node (`0x4000/0x4001`).
fn is_stager_kind(part: &legaia_asset::summon_overlay::SummonPart) -> bool {
    matches!(
        part.kind(),
        SummonPartKind::TransformNode | SummonPartKind::LibraryMesh
    ) || part.is_render_mode_node()
}

#[test]
fn prescript_records_are_summon_stager_format() {
    let Some(prot) = extracted_prot() else {
        eprintln!("skip: extracted/PROT not found");
        return;
    };

    let mut files = 0usize;
    let mut total = 0usize;
    let mut stager = 0usize;
    let mut transform = 0usize;
    let mut render = 0usize;

    for p in bin_entries(&prot) {
        let buf = std::fs::read(&p).unwrap();
        // Gate on the frame-opener-rated detector so we only score genuine
        // prescript-bearing entries (not random [count][offsets] coincidences).
        if detect(&buf).is_none() {
            continue;
        }
        let Some(recs) = move_stager_records(&buf) else {
            continue;
        };
        files += 1;
        // Record 0's lead is a dispatch/default table, not a stager record - skip it.
        for part in recs.iter().skip(1) {
            total += 1;
            if is_stager_kind(part) {
                stager += 1;
            }
            if part.is_transform_node() {
                transform += 1;
            }
            if part.is_render_mode_node() {
                render += 1;
            }
        }
    }

    if files == 0 {
        eprintln!("skip: no prescript-bearing entries detected");
        return;
    }

    let frac = stager as f32 / total as f32;
    eprintln!(
        "prescript move-stager records: files={files} records={total} \
         valid-stager={stager} ({:.1}%) transform-nodes={transform} render-mode={render}",
        frac * 100.0
    );

    // The records are summon-stager-format: virtually every model_sel lead is a
    // valid stager kind, and transform nodes (-1) dominate (as in the summon
    // stagers). A bespoke command VM / random data would not.
    assert!(
        frac > 0.95,
        "expected >95% valid stager-kind records, got {:.1}% ({stager}/{total})",
        frac * 100.0
    );
    assert!(
        transform * 2 > total,
        "expected transform nodes (model_sel=-1) to dominate, got {transform}/{total}"
    );
    assert!(
        render > 0,
        "expected at least one 0x4000/0x4001 render-mode node across the corpus"
    );
}
