//! The committed flagship pack (`packs/rebirth/`) loads, validates clean
//! and survives both round trips the tooling uses: the `.lxpack` bundle of
//! its raw tree, and the parsed pack serialised back to files.

use std::path::PathBuf;

use legaia_expansion_format::bundle::{read_bundle, write_bundle};
use legaia_expansion_format::flow;
use legaia_expansion_format::validate::{Severity, validate, validate_files};
use legaia_expansion_format::{ExpansionPack, read_dir_files};

fn rebirth_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth")
}

#[test]
fn rebirth_loads_and_validates_clean() {
    let files = read_dir_files(&rebirth_dir()).unwrap();
    let pack = ExpansionPack::from_files(&files).unwrap();
    // Every issue is a flow lint (a warning the content still owes, see
    // `flow::lint`); nothing else is reported.
    let lints = flow::lint(&pack);
    assert!(lints.iter().all(|i| i.severity == Severity::Warning));
    let issues = validate_files(&files, None);
    let other: Vec<_> = issues.iter().filter(|i| !lints.contains(i)).collect();
    assert!(other.is_empty(), "{other:#?}");
    assert_eq!(pack.manifest.id, "rebirth");
    assert_eq!(pack.manifest.name, "Legend of Legaia: Rebirth");
    assert!(pack.docs.contains_key("README.md"));
    assert_eq!(validate(&pack).len(), lints.len());
}

#[test]
fn rebirth_round_trips_through_a_bundle() {
    let files = read_dir_files(&rebirth_dir()).unwrap();
    let bundle = write_bundle(&files).unwrap();
    assert_eq!(write_bundle(&files).unwrap(), bundle, "deterministic");
    assert_eq!(read_bundle(&bundle).unwrap(), files);
    let pack = ExpansionPack::from_bundle(&bundle).unwrap();
    assert_eq!(pack, ExpansionPack::from_files(&files).unwrap());
}

#[test]
fn rebirth_round_trips_through_to_files() {
    let pack = ExpansionPack::from_dir(&rebirth_dir()).unwrap();
    let exported = pack.to_files().unwrap();
    assert_eq!(ExpansionPack::from_files(&exported).unwrap(), pack);
    assert_eq!(pack.to_files().unwrap(), exported, "deterministic");
    assert!(!legaia_expansion_format::validate::has_errors(
        &validate_files(&exported, None)
    ));
}

/// The share path end to end: the directory bundles, the bundle imports
/// through every gate with its cover, its sources unbundle to the same tree,
/// and that tree bundles to the same bytes.
#[test]
fn rebirth_shares_byte_identically() {
    use legaia_expansion_format::{bundle, share};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let files = legaia_expansion_format::read_dir_files(&root).unwrap();
    let layout: legaia_expansion_format::PackFiles = files
        .into_iter()
        .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
        .collect();
    let bytes = bundle::write_bundle(&layout).unwrap();
    let imported = share::import_bundle(&bytes).unwrap();
    assert_eq!(imported.summary.id, "rebirth");
    assert_eq!(imported.summary.cover.as_deref(), Some("assets/cover.png"));
    assert!(imported.cover().unwrap().starts_with(b"\x89PNG"));
    assert!(imported.pack.manifest.compatibility().is_ok());
    assert_eq!(imported.files, layout);
    assert_eq!(bundle::write_bundle(&imported.files).unwrap(), bytes);
    // An unchanged pack diffs empty against itself.
    let d = legaia_expansion_format::diff::diff_packs(&imported.pack, &imported.pack);
    assert!(d.is_empty());
}
