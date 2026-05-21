//! Parse the real arts-name table out of `extracted/SCUS_942.54` if present.
//! Skips and passes when the executable isn't on disk - same gating pattern as
//! the disc-dependent integration tests so CI doesn't need Sony bytes.

use legaia_art::arts_table::{self, ArtTableEntry};
use legaia_art::queue::{Character, Command};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

fn find<'a>(arts: &'a [ArtTableEntry], ch: Character, name: &str) -> &'a ArtTableEntry {
    arts.iter()
        .find(|a| a.character == ch && a.name == name)
        .unwrap_or_else(|| panic!("art {name:?} not found for {ch:?}"))
}

#[test]
fn decodes_the_arts_name_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let arts = arts_table::parse_from_scus(&bytes).expect("parse arts table");

    // 15 arts per character (1 Miracle at index 0 + 14 regular) = 45.
    assert_eq!(arts.len(), 45, "art record count");
    for ch in Character::all() {
        assert_eq!(
            arts.iter().filter(|a| a.character == ch).count(),
            15,
            "{ch:?} art count"
        );
    }

    // Each character's index-0 entry is the Miracle Art.
    assert!(find(&arts, Character::Vahn, "Hyper Elbow").index == 14);
    assert!(
        arts.iter().filter(|a| a.is_miracle).all(|a| a.index == 0),
        "miracle arts are the index-0 rows"
    );
    assert_eq!(arts.iter().filter(|a| a.is_miracle).count(), 3);

    // Pinned commands + AP (byte-exact from SCUS; arrow glyphs -> directions).
    use Command::*;
    let burning_flare = find(&arts, Character::Vahn, "Burning Flare");
    assert_eq!(burning_flare.ap, 50);
    assert_eq!(
        burning_flare.commands,
        vec![Right, Down, Left, Down, Left],
        "Burning Flare command"
    );

    let hyper_elbow = find(&arts, Character::Vahn, "Hyper Elbow");
    assert_eq!(hyper_elbow.ap, 18);
    // On-disc command is L,R,L - note this differs from the curated gamedata
    // table (which lists the third input as High); the executable is the
    // ground truth.
    assert_eq!(hyper_elbow.commands, vec![Left, Right, Left]);

    let hurricane_kick = find(&arts, Character::Noa, "Hurricane Kick");
    assert_eq!(hurricane_kick.ap, 70);
    assert_eq!(
        hurricane_kick.commands,
        vec![Left, Up, Up, Up, Up, Down, Right]
    );

    // Every non-Miracle art has at least one decoded direction.
    for a in arts.iter().filter(|a| !a.is_miracle) {
        assert!(!a.commands.is_empty(), "{} has no decoded commands", a.name);
    }
}
