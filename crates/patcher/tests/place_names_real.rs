//! Disc-gated tests for the **three-site** place rename
//! (`legaia_asset::place_names` + `legaia_patcher::location_name::ManPlaceNames`
//! + `apply::rename_locations_by_target`).
//!
//! The three carriers a place name lives at:
//!   1. `SCUS_942.54` `0x80073B18` - the quick-travel / Door-of-Wind cells
//!      (covered by `location_name_real.rs`);
//!   2. the **world-map location table** trailing every kingdom MAN - the
//!      labels drawn over the map at each place's map position;
//!   3. each scene MAN's **section 2** - the banner on entering the scene.
//!
//! These tests pin the on-disc shape of (2) and (3), then prove a rename
//! reaches all three, leaves near-miss names alone, keeps every touched sector
//! EDC/ECC-valid, and is idempotent. Gates on `LEGAIA_DISC_BIN`; skips + passes
//! when unset.

use legaia_asset::place_names::{WORLD_MAP_NAME_CAPACITY, WORLD_MAP_RECORD_STRIDE};
use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::{is_form2, mode2_form1_sector_is_valid};
use legaia_patcher::apply::{self, RenameTarget};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::location_name::ManPlaceNames;

/// The three kingdom bundles whose MAN trails the world-map location table.
const KINGDOM_ENTRIES: [usize; 3] = [86, 245, 392];

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn carrier(patcher: &DiscPatcher, idx: usize) -> Option<ManPlaceNames> {
    let entry = patcher.read_entry(idx).ok()?;
    let footprint = patcher
        .entry_true_footprint_sectors(idx)
        .map(|s| s as usize * 2048)
        .unwrap_or(entry.len());
    ManPlaceNames::locate(&entry, idx, footprint)
}

#[test]
fn kingdom_mans_carry_one_identical_world_map_table() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");

    let mut tables = Vec::new();
    for idx in KINGDOM_ENTRIES {
        let c = carrier(&patcher, idx).unwrap_or_else(|| panic!("kingdom MAN in PROT {idx}"));
        let table = c
            .world_map
            .clone()
            .unwrap_or_else(|| panic!("PROT {idx} carries no world-map table"));
        assert_eq!(table.locations.len(), 29, "PROT {idx} record count");
        // Records are 0x20 apart and start one byte past the count.
        assert_eq!(
            table.locations[1].record_offset - table.locations[0].record_offset,
            WORLD_MAP_RECORD_STRIDE
        );
        assert_eq!(table.locations[0].record_offset, table.count_offset + 1);
        // The table's *offset* is per-MAN (it trails a differently sized
        // section chain), so identity is over the record fields, not the
        // struct.
        tables.push(
            table
                .locations
                .iter()
                .map(|l| (l.region, l.map_x, l.map_y, l.discovery_flag, l.name.clone()))
                .collect::<Vec<_>>(),
        );
    }
    // Every kingdom carries the *whole* table (it is filtered by `region` at
    // draw time), so all three copies agree row for row.
    assert_eq!(tables[0], tables[1], "map01 vs map02 table");
    assert_eq!(tables[0], tables[2], "map01 vs map03 table");

    // Pinned rows: the first Drake marker, one Sebucus, one Karisto.
    let t = &tables[0];
    assert_eq!(t[0], (0, 93, 24, 0x0484, "Rim Elm".to_string()));
    assert_eq!(t[11].4, "Jeremi");
    assert_eq!(t[11].0, 1);
    assert_eq!(t[19].4, "Sol Tower");
    assert_eq!(t[19].0, 2);
    // The table names 14 places the 16-cell quick-travel table has no room for.
    assert!(t.iter().any(|l| l.4 == "Hunter's Spring"));
    assert!(t.iter().any(|l| l.4 == "Snowdrift Cave"));
}

#[test]
fn scene_banners_decode_and_the_kingdoms_name_their_continent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let inv = apply::list_locations(&patcher).expect("inventory");

    // Section 2 of a town bundle is the town's banner, exactly `strlen + 1`.
    let town01 = carrier(&patcher, 4).expect("town01 MAN");
    let name = town01.scene_name.expect("town01 section 2");
    assert_eq!(name.name, "Rim Elm");
    assert_eq!(
        name.body_len,
        "Rim Elm".len() + 1,
        "no padding in section 2"
    );

    // The kingdom bundles name the continent, which is the banner shown on
    // arriving at the world map.
    assert_eq!(
        carrier(&patcher, 86).unwrap().scene_name.unwrap().name,
        "Drake Kingdom"
    );
    assert_eq!(
        carrier(&patcher, 245).unwrap().scene_name.unwrap().name,
        "Sebucus Islands"
    );
    assert_eq!(
        carrier(&patcher, 392).unwrap().scene_name.unwrap().name,
        "Karisto Kingdom"
    );

    // Multi-scene places share one banner string across their bundles.
    assert_eq!(inv.scene_banners.get("Rim Elm"), Some(&7));
    assert_eq!(inv.scene_banners.get("Sol Tower"), Some(&13));
    assert_eq!(inv.scene_banners.get("Buma"), Some(&2));
    // Near-miss names are distinct strings and must stay distinguishable.
    assert_eq!(inv.scene_banners.get("Conkram"), Some(&3));
    assert_eq!(inv.scene_banners.get("Conkram (Past)"), Some(&3));
}

#[test]
fn rename_reaches_all_three_sites_and_leaves_near_misses_alone() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = apply::list_locations(&patcher).expect("inventory");

    // One index-keyed rename (a place with a quick-travel cell) and one
    // name-keyed (a place that has only a world-map label + banners). Both
    // names grow, so the scene sections resize.
    let report = apply::rename_locations_by_target(
        &mut patcher,
        &[
            (RenameTarget::Index(0), "Elmwood Village".to_string()),
            (
                RenameTarget::Name("Sol Tower".to_string()),
                "Helios Spire".to_string(),
            ),
        ],
    )
    .expect("apply");

    assert_eq!(report.renames.len(), 1, "one landmark cell carries Rim Elm");
    assert_eq!(
        report.renames[0],
        (0, "Rim Elm".into(), "Elmwood Village".into())
    );
    // Both places appear in all three kingdom copies of the world-map table.
    assert_eq!(report.world_map_records, 6);
    // 7 Rim Elm scenes + 13 Sol Tower scenes.
    assert_eq!(report.scene_banners, 20);
    assert_eq!(
        report.entries_changed.len(),
        23,
        "20 scenes + 3 kingdom MANs"
    );
    assert!(
        report.skipped.is_empty(),
        "no MAN failed to fit: {:?}",
        report.skipped
    );
    assert!(report.unmatched.is_empty(), "both names matched");

    // Read every site back off the patched image.
    let after = apply::list_locations(&patcher).expect("patched inventory");
    assert_eq!(after.landmarks[0].1, "Elmwood Village");
    assert_eq!(
        after.landmarks[10].1, "Sol",
        "the `Sol` cell is a different string and must not follow `Sol Tower`"
    );
    assert!(
        after
            .world_map
            .iter()
            .any(|(_, _, _, n)| n == "Elmwood Village")
    );
    assert!(
        after
            .world_map
            .iter()
            .any(|(_, _, _, n)| n == "Helios Spire")
    );
    assert!(!after.world_map.iter().any(|(_, _, _, n)| n == "Rim Elm"));
    assert!(!after.world_map.iter().any(|(_, _, _, n)| n == "Sol Tower"));
    assert_eq!(after.scene_banners.get("Elmwood Village"), Some(&7));
    assert_eq!(after.scene_banners.get("Helios Spire"), Some(&13));
    assert_eq!(after.scene_banners.get("Rim Elm"), None);
    assert_eq!(after.scene_banners.get("Sol Tower"), None);

    // Everything not named is byte-stable in the inventory sense: the other
    // banners keep their counts, including the near-miss "Conkram (Past)".
    for (name, count) in &before.scene_banners {
        if name == "Rim Elm" || name == "Sol Tower" {
            continue;
        }
        assert_eq!(
            after.scene_banners.get(name),
            Some(count),
            "banner {name:?} changed"
        );
    }

    // The world-map table keeps its shape: 29 records, same map positions and
    // discovery flags, only two names differ.
    assert_eq!(after.world_map.len(), before.world_map.len());
    let mut differing = 0;
    for (b, a) in before.world_map.iter().zip(after.world_map.iter()) {
        assert_eq!((b.0, b.1, b.2), (a.0, a.1, a.2), "record placement moved");
        if b.3 != a.3 {
            differing += 1;
        }
    }
    assert_eq!(differing, 2, "only the two renamed rows differ");

    // Every sector of the patched image is still EDC/ECC-valid.
    let image = patcher.image();
    let mut bad = 0usize;
    for lba in 0..image.len() / SECTOR_SIZE {
        let sec = &image[lba * SECTOR_SIZE..(lba + 1) * SECTOR_SIZE];
        if !is_form2(sec) && !mode2_form1_sector_is_valid(sec) {
            bad += 1;
        }
    }
    assert_eq!(bad, 0, "{bad} sectors are EDC/ECC-invalid after the rename");

    // The patched image re-opens with the PROT index space intact.
    let re = DiscPatcher::open(image.to_vec()).expect("patched image re-parses");
    assert_eq!(re.entry_count(), patcher.entry_count());
}

#[test]
fn reapplying_a_rename_changes_nothing() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::rename_locations_by_target(
        &mut patcher,
        &[(RenameTarget::Index(6), "Vidna Town".to_string())],
    )
    .expect("first apply");
    let once = patcher.image().to_vec();

    let report = apply::rename_locations_by_target(
        &mut patcher,
        &[(RenameTarget::Index(6), "Vidna Town".to_string())],
    )
    .expect("second apply");
    assert!(report.is_empty(), "re-applying is a no-op: {report:?}");
    assert!(
        patcher.image() == once.as_slice(),
        "re-applying rewrote bytes"
    );
}

#[test]
fn a_refused_name_leaves_the_disc_untouched() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher.image().to_vec();

    // Past the shared cap (the world-map record's name field), non-ASCII, and
    // an out-of-range cell index are all refused before the first write.
    for bad in [
        "x".repeat(WORLD_MAP_NAME_CAPACITY),
        "Vïdna".to_string(),
        String::new(),
    ] {
        assert!(
            apply::rename_locations_by_target(
                &mut patcher,
                &[(RenameTarget::Index(0), bad.clone())]
            )
            .is_err(),
            "name {bad:?} should be refused"
        );
    }
    assert!(
        apply::rename_locations_by_target(
            &mut patcher,
            &[(RenameTarget::Index(99), "X".to_string())]
        )
        .is_err()
    );
    assert!(
        patcher.image() == before.as_slice(),
        "a refusal wrote bytes"
    );

    // An unknown name is not an error - it is reported as matching nothing.
    let report = apply::rename_locations_by_target(
        &mut patcher,
        &[(
            RenameTarget::Name("Nowhere At All".to_string()),
            "Somewhere".to_string(),
        )],
    )
    .expect("unknown name is reported, not fatal");
    assert_eq!(report.unmatched, vec!["Nowhere At All".to_string()]);
    assert!(report.is_empty());
    assert!(patcher.image() == before.as_slice());
}
