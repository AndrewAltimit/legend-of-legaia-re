//! Disc-gated validation for the house-door warp classifier
//! (`legaia_patcher::house_door`): across the whole retail PROT corpus, the
//! classified door-warp population must match the byte-audited census -
//! per-scene IN / OUT class counts, the structural op signature
//! (`0xA3 0xF8` = cross-context player MOVE_TO), non-sentinel targets, and the
//! runtime-pinned Mei's-house anchor (town01 interior tile `(97, 54)`, the
//! PCSX-Redux `find_writer` capture in
//! `docs/tooling/pcsx-redux-automation.md`). Skips + passes without
//! `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::house_door::{DoorSide, SceneHouseDoors};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Expected per-scene census: `entry_idx -> (in_sites, out_sites, unclassified)`.
///
/// The scene-bundle PROT entries with classified door warps. (Scene labels per
/// CDNAME: 4 = town01, 13 = town0b, 22 = town0c (the Rim Elm variants),
/// 53 = bylon, 166 = geremi, 183 = balden, 192 = conc, 255 = tower,
/// 282 = retockin, 291 = retona, 348 = town0d, 435 = uru.) `unclassified`
/// counts the partition-0 player warps found but not shuffle-eligible: warps
/// without a door-name class (story repositions, e.g. the town01 intro
/// "inside the house" warp; town0d carries a byte-identical twin pair of one
/// such reposition - both target the same tile - which the pinned nibble-8
/// widths surface in full) plus the multi-warp choreography records (the
/// tower's multi-stop elevator-2 pair carries 10 warps per side - floor
/// branches and `(0, 0)` sync repositions the full-width op walk surfaces).
const EXPECTED: &[(usize, (usize, usize, usize))] = &[
    (4, (2, 2, 1)),
    (13, (2, 2, 1)),
    (22, (2, 2, 1)),
    (53, (3, 3, 0)),
    (166, (2, 2, 0)),
    (183, (1, 1, 0)),
    (192, (2, 2, 0)),
    (255, (7, 7, 20)),
    (282, (1, 3, 0)),
    (291, (1, 1, 0)),
    (348, (1, 1, 2)),
    (435, (3, 3, 0)),
];

#[test]
fn classifier_census_matches_the_disc() {
    let Some(image) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(image).expect("open disc");

    let mut found: BTreeMap<usize, (usize, usize, usize)> = BTreeMap::new();
    let mut town01_in_targets: Vec<(u8, u8)> = Vec::new();

    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        let Some(sd) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        let ins = sd.sites.iter().filter(|s| s.side == DoorSide::In).count();
        let outs = sd.sites.len() - ins;
        found.insert(idx, (ins, outs, sd.unclassified));

        // Structural signature + plausibility of every classified site.
        for (s, (xb, zb)) in sd.sites.iter().zip(sd.current_targets()) {
            assert_eq!(
                sd.decoded[s.op_pc], 0xA3,
                "entry {idx} record {}: site op byte must be the cross-context MOVE_TO",
                s.record
            );
            assert_eq!(
                sd.decoded[s.op_pc + 1],
                0xF8,
                "entry {idx} record {}: warp must target the player channel",
                s.record
            );
            let tile = (xb & 0x7F, zb & 0x7F);
            assert_ne!(
                tile,
                (0x7F, 0x7F),
                "entry {idx} record {}: door warp must not target the 'here' sentinel",
                s.record
            );
            if idx == 4 && s.side == DoorSide::In {
                town01_in_targets.push(tile);
            }
        }

        // Per-scene plausibility: a town has between 1 and 8 doors per class.
        assert!(
            (1..=8).contains(&ins.max(outs)) && ins >= 1 && outs >= 1,
            "entry {idx}: implausible door census ({ins} IN / {outs} OUT)"
        );
    }

    // The census is exactly the byte-audited population - no scene gained or
    // lost a classified door warp.
    let expected: BTreeMap<usize, (usize, usize, usize)> = EXPECTED.iter().copied().collect();
    assert_eq!(
        found, expected,
        "classified door-warp census diverged from the audited disc population"
    );

    // Runtime-pinned anchor: the Mei's-house entry in town01 warps the player
    // to interior tile (97, 54) (`0xA3 0xF8 0x61 0x36`, captured live via the
    // PCSX-Redux range write-watch on the player position block).
    assert!(
        town01_in_targets.contains(&(97, 54)),
        "town01 IN-class targets {town01_in_targets:?} must include the captured \
         Mei's-house interior (97, 54)"
    );
}
