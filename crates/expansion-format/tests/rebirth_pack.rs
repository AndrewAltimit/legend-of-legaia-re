//! The committed flagship pack (`packs/rebirth/`) loads, validates clean
//! and survives both round trips the tooling uses: the `.lxpack` bundle of
//! its raw tree, and the parsed pack serialised back to files.

use std::path::PathBuf;

use legaia_expansion_format::bundle::{read_bundle, write_bundle};
use legaia_expansion_format::validate::{validate, validate_files};
use legaia_expansion_format::{ExpansionPack, read_dir_files};

fn rebirth_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth")
}

#[test]
fn rebirth_loads_and_validates_clean() {
    let files = read_dir_files(&rebirth_dir()).unwrap();
    let issues = validate_files(&files, None);
    assert!(issues.is_empty(), "{issues:#?}");
    let pack = ExpansionPack::from_files(&files).unwrap();
    assert_eq!(pack.manifest.id, "rebirth");
    assert_eq!(pack.manifest.name, "Legend of Legaia: Rebirth");
    assert!(pack.docs.contains_key("README.md"));
    assert!(validate(&pack).is_empty());
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
    assert!(validate_files(&exported, None).is_empty());
}
