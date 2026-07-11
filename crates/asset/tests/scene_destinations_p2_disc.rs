//! Disc-gated: the merged P1+P2 scene-destination scan
//! ([`legaia_asset::man_edit::scene_destinations`]) sees the doors the
//! partition-1 table pass alone misses.
//!
//! The P1 pass ([`legaia_asset::man_edit::partition1_destinations`]) walks
//! partition-1 scripts plus the trailing destination-table blob with the
//! recovering `LinearWalker`, but under-reports doors carried **only by
//! partition-2 records** (the walker is desynced when its over-walk crosses a
//! P2 record's SJIS-name header). The concrete retail case pinned here:
//! `town01`'s (Rim Elm's) exit door to the Drake overworld `map01` is P2-only:
//! absent from the P1 pass, present in the merged scan. (The corpus class:
//! town/dungeon exit doors are P2 door-choreography records; 13 scenes carry
//! P2-only destinations, e.g. `retockin`->`retona`, `geremi`->`map02`/`tower`.)
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::man_edit::{SceneDestination, partition1_destinations, scene_destinations};
use legaia_asset::{man_section, scene_asset_table};

fn extracted_prot() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn disc_gated() -> bool {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return false;
    }
    true
}

/// Locate + LZS-decode the MAN sub-asset of a scene-bundle PROT entry.
fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = scene_asset_table::detect(bytes)?;
    let man = table.used().iter().find(|d| d.type_byte == 0x03).copied()?;
    if man.size == 0 || man.data_offset == 0 {
        return None;
    }
    let body = bytes.get(man.data_offset as usize..)?;
    let (decoded, _) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
    (decoded.len() == man.size as usize).then_some(decoded)
}

fn names(dests: &[SceneDestination]) -> BTreeSet<&str> {
    dests.iter().map(|d| d.scene_name.as_str()).collect()
}

#[test]
fn town01_exit_door_to_map01_is_p2_only() {
    if !disc_gated() {
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    // Find the town01 bundle that carries the scene MAN.
    let mut found = false;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.ends_with("_town01.BIN"))
        })
        .collect();
    paths.sort();
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let mf = man_section::parse(&man).expect("parse town01 MAN");
        let p1 = partition1_destinations(&mf, &man);
        let merged = scene_destinations(&mf, &man);
        eprintln!(
            "[{}] p1={:?} merged={:?}",
            path.file_name().unwrap().to_string_lossy(),
            names(&p1),
            names(&merged),
        );

        // The merged scan extends the P1 pass in place (same first-seen
        // prefix), so existing consumers keep their destinations.
        assert_eq!(&merged[..p1.len()], &p1[..]);

        // The pinned asymmetry: Rim Elm's exit door to the Drake overworld
        // is carried only by a partition-2 door-choreography record -
        // invisible to the P1 table pass, folded in by the P2 pass.
        assert!(
            !names(&p1).contains("map01"),
            "P1 pass now sees map01 - the P2-only pin moved; got {:?}",
            names(&p1)
        );
        let exit = merged
            .iter()
            .find(|d| d.scene_name == "map01")
            .unwrap_or_else(|| {
                panic!(
                    "merged scan is missing the P2-only town01->map01 exit door; got {:?}",
                    names(&merged)
                )
            });
        assert_eq!(exit.index, 85, "town01->map01 exit index is stable");
        found = true;
    }
    assert!(found, "no town01 bundle with a MAN found under {prot:?}");
}

#[test]
fn merged_scan_is_a_superset_across_the_scene_corpus() {
    if !disc_gated() {
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    // Every scene-bundle label present on disc (the extraction filenames are
    // CDNAME block labels) - the clean-name universe a genuine destination
    // must land in.
    let mut labels: BTreeSet<String> = BTreeSet::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    paths.sort();
    for p in &paths {
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            && let Some((_, label)) = stem.split_once('_')
        {
            labels.insert(label.to_string());
        }
    }

    let mut scenes_with_man = 0usize;
    let mut scenes_with_p2_additions = 0usize;
    let mut total_additions = 0usize;
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let Ok(mf) = man_section::parse(&man) else {
            continue;
        };
        scenes_with_man += 1;
        let p1 = partition1_destinations(&mf, &man);
        let merged = scene_destinations(&mf, &man);

        // Additive: the merged scan preserves the P1 pass verbatim as its
        // prefix and only appends.
        assert!(
            merged.len() >= p1.len() && merged[..p1.len()] == p1[..],
            "{}: merged scan is not a P1-prefix superset",
            path.display()
        );

        let additions = &merged[p1.len()..];
        if !additions.is_empty() {
            scenes_with_p2_additions += 1;
            total_additions += additions.len();
            eprintln!(
                "[add] {}: {:?}",
                path.file_name().unwrap().to_string_lossy(),
                additions
                    .iter()
                    .map(|d| (d.scene_name.as_str(), d.index))
                    .collect::<Vec<_>>()
            );
        }
        // Every P2-folded destination must be a real scene label on disc -
        // the clean-name gate must not let a desync phantom through.
        for d in additions {
            assert!(
                labels.contains(&d.scene_name),
                "{}: P2-folded destination {:?} (index {}) is not a disc scene label",
                path.display(),
                d.scene_name,
                d.index
            );
        }
    }

    eprintln!(
        "[corpus] {scenes_with_man} scene MANs, {scenes_with_p2_additions} scenes gained \
         {total_additions} P2-only destinations"
    );
    assert!(scenes_with_man > 50, "corpus is vacuous: {scenes_with_man}");
    assert!(
        scenes_with_p2_additions > 0,
        "no scene gained a P2-only destination - the fold is vacuous"
    );
}
