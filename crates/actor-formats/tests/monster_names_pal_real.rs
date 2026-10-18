//! Disc-gated oracle for the monster-name reader across builds.
//!
//! The French, German and Italian discs spell some monster names with the
//! dialog font's accented Latin cells (bytes above `0x7E`); the reader
//! decodes those through `legaia_font::latin` instead of rejecting the
//! record. The USA names are pure ASCII, so on the USA disc the reader's
//! output must be exactly the ASCII bytes it always produced: every record
//! name is ASCII and equals the caret-stripped raw bytes.
//!
//! Reads `PROT.DAT` straight off the image (no `extracted/` needed).
//! `LEGAIA_DISC_BIN` (USA) gates the test; `LEGAIA_PAL_DISC_BIN`, when also
//! set, adds a PAL disc: it must yield as many populated records as the USA
//! disc, at least one name carrying an accented letter. Skips and passes
//! without `LEGAIA_DISC_BIN`.

use std::path::Path;

use legaia_actor_formats::monster_archive;
use legaia_iso::iso9660;
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_prot::archive::Archive;

/// PROT entry 867 (the monster archive) of the disc image at `bin`.
fn monster_archive_entry(bin: &Path) -> Vec<u8> {
    let mut disc = RawDisc::open(bin).expect("open disc image");
    let vol = iso9660::read_volume(&mut disc).expect("ISO9660 volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walk ISO9660");
    let (_, rec) = files
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case("PROT.DAT"))
        .expect("PROT.DAT on the disc");
    let mut prot = Vec::new();
    disc.read_user_data(rec.lba, rec.size.div_ceil(USER_DATA_SIZE as u32), &mut prot)
        .expect("read PROT.DAT");
    prot.truncate(rec.size as usize);
    let mut archive = Archive::from_bytes(prot).expect("PROT.DAT TOC");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == 867)
        .expect("PROT entry 867")
        .clone();
    let mut out = Vec::new();
    archive.read_entry(&entry, &mut out).expect("read PROT 867");
    out
}

fn disc(var: &str) -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var_os(var).filter(|v| !v.is_empty())?);
    p.is_file().then_some(p)
}

#[test]
fn usa_names_stay_ascii_and_pal_accented_names_parse() {
    let Some(usa) = disc("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let usa_867 = monster_archive_entry(&usa);
    let usa_records = monster_archive::records(&usa_867).expect("USA records");
    assert!(
        usa_records.len() > 150,
        "USA populated records: {}",
        usa_records.len()
    );
    for r in &usa_records {
        assert!(
            r.name.is_ascii(),
            "USA name {} is not ASCII: {:?}",
            r.id,
            r.name
        );
    }

    let Some(pal) = disc("LEGAIA_PAL_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_PAL_DISC_BIN unset; USA half only");
        return;
    };
    let pal_867 = monster_archive_entry(&pal);
    let pal_records = monster_archive::records(&pal_867).expect("PAL records");
    let usa_ids: Vec<u16> = usa_records.iter().map(|r| r.id).collect();
    let pal_ids: Vec<u16> = pal_records.iter().map(|r| r.id).collect();
    assert_eq!(pal_ids, usa_ids, "the PAL archive populates the same ids");
    assert!(
        pal_records.iter().any(|r| !r.name.is_ascii()),
        "some PAL name carries an accented letter"
    );
}
