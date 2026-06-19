//! Disc-gated end-to-end test for the intra-town (house / interior) door
//! shuffle: per-scene, class-preserving shuffle of the player door-warp target
//! tiles on a scratch copy of the disc, then re-decode every patched scene MAN
//! straight off the patched image and confirm - the per-scene IN-class and
//! OUT-class target multisets are each preserved (so every house entry still
//! lands in some interior and every exit still lands at some exterior doorstep
//! - no off-map placement, no interior-to-interior softlock), every patched
//! MAN re-parses through disc → ISO → PROT → LZS, every touched sector stays
//! EDC/ECC-valid, the image size is unchanged, and a fixed seed is
//! byte-deterministic. Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::house_door::{DoorSide, SceneHouseDoors};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Per-scene, per-class sorted door-warp target multisets:
/// `entry -> (sorted IN targets, sorted OUT targets)`.
#[allow(clippy::type_complexity)]
fn per_scene_class_targets(
    patcher: &DiscPatcher,
) -> BTreeMap<usize, (Vec<(u8, u8)>, Vec<(u8, u8)>)> {
    let mut by_scene: BTreeMap<usize, (Vec<(u8, u8)>, Vec<(u8, u8)>)> = BTreeMap::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        let Some(sd) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        let slot = by_scene.entry(idx).or_default();
        for (s, t) in sd.sites.iter().zip(sd.current_targets()) {
            match s.side {
                DoorSide::In => slot.0.push(t),
                DoorSide::Out => slot.1.push(t),
            }
        }
        slot.0.sort_unstable();
        slot.1.sort_unstable();
    }
    by_scene
}

#[test]
fn shuffle_house_doors_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x484F_5553_4544_4F4F; // "HOUSEDOO"

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = per_scene_class_targets(&base);
    assert!(
        before.len() >= 10,
        "expected the audited door-scene census, got {} scenes",
        before.len()
    );

    let mut patcher = DiscPatcher::open(original.clone()).expect("open scratch");
    let report =
        apply::randomize_house_doors(&mut patcher, seed, DropMode::Shuffle).expect("shuffle");
    assert!(
        report.sites_changed >= 30,
        "every multi-door class should reshuffle; changed only {}",
        report.sites_changed
    );
    assert!(
        report.skipped.is_empty(),
        "same-size operand edits must always re-pack, skipped {:?}",
        report.skipped
    );
    let patched = patcher.into_image();
    assert_eq!(
        patched.len(),
        original.len(),
        "image size must be unchanged"
    );

    // Re-decode the patched disc and check the per-scene, per-class target
    // multisets are preserved everywhere: interior landings stay interior
    // landings, exterior doorsteps stay exterior doorsteps.
    let after_patcher = DiscPatcher::open(patched.clone()).expect("re-open patched");
    let after = per_scene_class_targets(&after_patcher);
    assert_eq!(
        before, after,
        "per-scene IN / OUT door-warp target multisets are preserved across the shuffle"
    );

    // Every touched sector stays EDC/ECC-valid.
    let mut bad = 0usize;
    let mut checked = 0usize;
    let mut sector = 0usize;
    while (sector + 1) * SECTOR_SIZE <= patched.len() {
        let b = sector * SECTOR_SIZE;
        let span = b..b + SECTOR_SIZE;
        if original[span.clone()] != patched[span.clone()] {
            checked += 1;
            if !mode2_form1_sector_is_valid(&patched[b..b + SECTOR_SIZE]) {
                bad += 1;
            }
        }
        sector += 1;
    }
    assert!(checked > 0, "expected some changed sectors");
    assert_eq!(
        bad, 0,
        "{bad} of {checked} changed sectors are EDC/ECC-invalid"
    );

    // Determinism.
    let mut again = DiscPatcher::open(original).expect("open");
    apply::randomize_house_doors(&mut again, seed, DropMode::Shuffle).expect("shuffle again");
    assert_eq!(
        again.into_image(),
        patched,
        "a fixed seed is byte-deterministic"
    );
}
