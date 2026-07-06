//! Disc-gated: the disc-wide **op-0x49 flag-window census** over every scene
//! MAN's field-VM bytecode.
//!
//! The field-overlay picker widget `FUN_801EF014` (system-actor handler
//! `0x23` in the `PTR_FUN_801f33b4` table, dispatcher `FUN_801F159C`) reads a
//! flag-window descriptor through `_DAT_8007B450` - the pointer field-VM op
//! `0x49` arms at its inline operands: `+1` count, `+2` default index,
//! `+3` rows, `+4..5` u16 base flag. Its writes land on system flags
//! `base + offset`, so a window site can set a flag with **no literal
//! operand** anywhere in the corpus. This census decodes every op-`0x49`
//! site with the real field-VM disassembler ([`legaia_asset::field_disasm`]
//! `LinearWalker` - framed instruction boundaries, not raw byte pairs) and
//! interprets every site's operand window under that descriptor layout, the
//! conservative superset (which sub-op arms handler `0x23` is runtime state).
//!
//! This is the LAST static probe for the spine flag writers after wave 7's
//! corpus-wide literal-operand negatives (all-programs Ghidra sweep + raw
//! byte scan of all PROT entries + `--system-flag-census` +
//! `--motion-flag-census`, all negative for `0x142`/`0x482`). The residual
//! hypothesis it closes: an op-0x49 window whose `base + offset` arithmetic
//! covers `0x142` (dolk clear) or `0x482` (Drake mist walls) - plus the
//! same-orphan-family `0x1BE` and `0x225` (549, the town01 opening one-shot).
//!
//! Pinned corpus shape: only ONE genuine flag-window family exists on the
//! disc - the `kor` / `kor3` / `kor4` sub-`0x04` sites, all
//! `base=0x138 count=8` (window `0x138..=0x13F`, paired per record as
//! `default=0 rows=8` then `default=4 rows=4`). Sub-`0x04` is the exact
//! 6-operand-byte descriptor shape, so these are structural, not desync
//! noise. The window tops out **3 below `0x142`** - a near-miss, NOT a
//! writer. Every other site is a sub-`0x00`/`0x01`/... op whose superset
//! interpretation reads MES/operand bytes (ASCII bases like `0x746E` "nt"),
//! none within the near-miss margin of any target.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::{Op49WindowSite, op49_window_census};
use legaia_engine_core::scene::ProtIndex;
use std::path::PathBuf;

/// The still-orphaned spine / gate flags: dolk-clear, Drake mist walls, and
/// the two same-family orphans swept alongside them in every census.
const SPINE_TARGETS: [u16; 4] = [0x142, 0x482, 0x1BE, 0x225];

/// Near-miss margin: a window covering within this many flag ids of a
/// target is reported (an off-by-small-constant encode would land here).
const NEAR_MISS_MARGIN: u32 = 8;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn run_census() -> Option<Vec<Op49WindowSite>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scenes = index.cdname_scene_names();
    eprintln!("[op49 census] scanning {} CDNAME scenes", scenes.len());
    Some(op49_window_census(&index, &scenes))
}

/// Corpus shape: total site count + the one genuine flag-window family
/// (`kor`-family sub-4, base `0x138`, count 8).
#[test]
fn op49_window_census_pins_the_corpus_shape() {
    let Some(sites) = run_census() else { return };

    let scenes: std::collections::BTreeSet<&str> =
        sites.iter().map(|s| s.scene_name.as_str()).collect();
    eprintln!(
        "[op49 census] {} sites across {} scenes",
        sites.len(),
        scenes.len(),
    );
    for s in &sites {
        eprintln!(
            "  scene={:<10} P{}[{:3}] @0x{:05X} sub=0x{:02X} base=0x{:04X} count={:3} default={:3} rows={:3} window={:?}{}",
            s.scene_name,
            s.partition,
            s.record,
            s.abs_pc,
            s.sub_op,
            s.base_flag,
            s.count,
            s.default_index,
            s.rows,
            s.window(),
            if s.in_footprint {
                ""
            } else {
                "  [past-footprint]"
            },
        );
    }

    // Disc invariant: 209 op-0x49 sites across 62 scene MANs (framed with
    // the corrected 0x4E sub-2/3/9 compare widths; the pre-fix framing
    // under-counted by one resync-shadowed site).
    assert_eq!(sites.len(), 209, "op-0x49 site count changed");
    assert_eq!(scenes.len(), 62, "op-0x49 carrying-scene count changed");

    // The one genuine flag-window family: every sub-0x04 site (the exact
    // 6-operand-byte descriptor shape) is a kor-family base=0x138 count=8
    // window - 24 sites, paired per record (default=0 rows=8 / default=4
    // rows=4), all inside the instruction footprint.
    let sub4: Vec<&Op49WindowSite> = sites.iter().filter(|s| s.sub_op == 0x04).collect();
    assert_eq!(sub4.len(), 24, "sub-0x04 flag-window site count changed");
    for s in &sub4 {
        assert!(
            matches!(s.scene_name.as_str(), "kor" | "kor3" | "kor4"),
            "sub-4 window outside the kor family: {s:?}",
        );
        assert_eq!(
            (s.base_flag, s.count, s.partition),
            (0x138, 8, 2),
            "kor-family window shape changed: {s:?}",
        );
        assert!(s.in_footprint, "kor-family site past footprint: {s:?}");
        assert!(
            matches!((s.default_index, s.rows), (0, 8) | (4, 4)),
            "kor-family default/rows pairing changed: {s:?}",
        );
    }

    // Anchor site, hand-verified: kor P2[17] @0x047A3.
    assert!(
        sub4.iter().any(|s| {
            s.scene_name == "kor" && s.partition == 2 && s.record == 17 && s.abs_pc == 0x047A3
        }),
        "kor P2[17] @0x047A3 anchor window missing",
    );
}

/// The load-bearing negative: no op-0x49 flag window anywhere on the disc
/// CONTAINS a spine flag (`0x142` / `0x482` / `0x1BE` / `0x225`). The only
/// near-miss within +/-8 of any target is the kor-family window
/// `[0x138..0x13F]`, whose top sits 3 below `0x142` - adjacent id space,
/// not a writer. With the literal-operand sweeps already corpus-negative,
/// this closes the last cheap static hypothesis: the spine writers are
/// runtime-computed, capture-only.
#[test]
fn op49_window_census_pins_the_spine_flag_negatives() {
    let Some(sites) = run_census() else { return };

    for target in SPINE_TARGETS {
        let contained: Vec<&Op49WindowSite> = sites.iter().filter(|s| s.covers(target)).collect();
        let near: Vec<&Op49WindowSite> = sites
            .iter()
            .filter(|s| !s.covers(target) && s.min_distance(target) <= NEAR_MISS_MARGIN)
            .collect();
        let nearest = sites.iter().map(|s| s.min_distance(target)).min();
        eprintln!(
            "[op49 census] target 0x{target:04X}: contained {} / near-miss {} / nearest {:?}",
            contained.len(),
            near.len(),
            nearest,
        );
        assert!(
            contained.is_empty(),
            "flag 0x{target:04X} is COVERED by op-0x49 window site(s) - do not \
             over-claim a writer; report for main-session verification: {contained:#?}",
        );
        if target == 0x142 {
            // The kor-family near-miss: all 24 sub-4 sites, distance 3
            // (window top 0x13F vs 0x142). Adjacent flag-id block, not a
            // covering window.
            assert_eq!(near.len(), 24, "0x142 near-miss family changed: {near:#?}",);
            assert!(
                near.iter().all(|s| s.sub_op == 0x04
                    && s.base_flag == 0x138
                    && s.min_distance(target) == 3),
                "0x142 near-miss family is no longer the kor base-0x138 windows: {near:#?}",
            );
        } else {
            assert!(
                near.is_empty(),
                "flag 0x{target:04X} gained op-0x49 near-miss window site(s) \
                 within +/-{NEAR_MISS_MARGIN}: {near:#?}",
            );
        }
    }
}
