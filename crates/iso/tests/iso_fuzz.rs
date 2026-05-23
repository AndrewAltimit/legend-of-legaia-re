//! Panic-hardening regression tests for the ISO9660 walker and raw sector
//! reader.
//!
//! Every byte fed here is hand-constructed (synthetic); there is no real disc
//! content. These tests assert that malformed / truncated / cyclic disc images
//! produce a clean `Err` or a bounded `Ok`, never a panic or an unbounded
//! allocation.

use std::io::Write;
use std::path::PathBuf;

use legaia_iso::iso9660::{self, DirectoryRecord};
use legaia_iso::raw::{RawDisc, SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};

/// A growable in-memory disc image laid out as 2352-byte Mode2/2352 sectors.
/// User data (2048 bytes) lives at offset 24 within each sector.
struct DiscBuilder {
    sectors: Vec<[u8; SECTOR_SIZE]>,
}

impl DiscBuilder {
    fn new(num_sectors: usize) -> Self {
        Self {
            sectors: vec![[0u8; SECTOR_SIZE]; num_sectors],
        }
    }

    /// Write `bytes` into the user-data area of the sector at `lba`.
    fn user_data(&mut self, lba: usize, bytes: &[u8]) {
        let s = &mut self.sectors[lba];
        let end = USER_DATA_OFFSET + bytes.len().min(USER_DATA_SIZE);
        s[USER_DATA_OFFSET..end].copy_from_slice(&bytes[..end - USER_DATA_OFFSET]);
    }

    fn write_to_temp(&self, name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        for s in &self.sectors {
            f.write_all(s).unwrap();
        }
        f.flush().unwrap();
        path
    }
}

/// Build a 34-byte directory record at the front of a buffer.
fn dir_record(lba: u32, size: u32, is_dir: bool, name: &[u8]) -> Vec<u8> {
    let name_len = name.len();
    let total = 33 + name_len;
    let mut r = vec![0u8; total];
    r[0] = total as u8; // record length
    r[2..6].copy_from_slice(&lba.to_le_bytes());
    r[10..14].copy_from_slice(&size.to_le_bytes());
    r[25] = if is_dir { 0x02 } else { 0x00 };
    r[32] = name_len as u8;
    r[33..33 + name_len].copy_from_slice(name);
    r
}

/// Build a minimal valid PVD (sector 16) whose root record points at `root_lba`
/// with size `root_size`.
fn make_pvd(root_lba: u32, root_size: u32) -> Vec<u8> {
    let mut pvd = vec![0u8; USER_DATA_SIZE];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    // volume_id at 40..72 (left as spaces -> trims to empty)
    for b in &mut pvd[40..72] {
        *b = b' ';
    }
    // Root directory record at offset 156, 34 bytes.
    let root = dir_record(root_lba, root_size, true, &[0]); // name "\0" => "."
    pvd[156..156 + root.len()].copy_from_slice(&root);
    pvd
}

#[test]
fn read_volume_rejects_non_iso9660_image() {
    // A disc whose sector 16 has neither the type byte nor the CD001 magic.
    let disc = DiscBuilder::new(20);
    let path = disc.write_to_temp("legaia_iso_fuzz_noniso.bin");
    let mut raw = RawDisc::open(&path).unwrap();
    assert!(iso9660::read_volume(&mut raw).is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn list_directory_rejects_absurd_extent_size_without_oom() {
    // Directory record claims a near-u32::MAX extent. Must Err on the sanity
    // limit rather than try to reserve/read terabytes.
    let dir = DirectoryRecord {
        lba: 20,
        size: u32::MAX,
        is_dir: true,
        name: "BIG".into(),
    };
    let disc = DiscBuilder::new(24);
    let path = disc.write_to_temp("legaia_iso_fuzz_bigdir.bin");
    let mut raw = RawDisc::open(&path).unwrap();
    let res = iso9660::list_directory(&mut raw, &dir);
    assert!(res.is_err(), "absurd extent size must be rejected");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn list_directory_handles_malformed_records_without_panic() {
    // Root directory at LBA 18 holding records with various corruptions:
    //  - a record whose declared name_len overruns its record length,
    //  - a zero-length record (block padding),
    //  - a record pointing past the end of the disc.
    let mut disc = DiscBuilder::new(40);

    let mut dir_buf = Vec::new();
    // Valid file record.
    dir_buf.extend_from_slice(&dir_record(30, 2048, false, b"GOOD.BIN"));
    // Record whose name_len byte lies (claims 200 bytes in a small record).
    {
        let mut bad = dir_record(25, 100, false, b"X");
        bad[32] = 200; // name_len far beyond the record length
        dir_buf.extend_from_slice(&bad);
    }
    // File record pointing at an LBA past the disc end (read happens lazily;
    // listing should still parse the record).
    dir_buf.extend_from_slice(&dir_record(9999, 2048, false, b"FAR.BIN"));

    disc.user_data(18, &dir_buf);
    let path = disc.write_to_temp("legaia_iso_fuzz_malformed.bin");
    let mut raw = RawDisc::open(&path).unwrap();

    let dir = DirectoryRecord {
        lba: 18,
        size: dir_buf.len() as u32,
        is_dir: true,
        name: "ROOT".into(),
    };
    // The malformed name_len record makes parse_record return Err; the call
    // surfaces that as Err rather than panicking on an OOB slice.
    let res = iso9660::list_directory(&mut raw, &dir);
    assert!(res.is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn list_directory_stops_at_truncated_trailing_record() {
    // A directory whose last record's length runs past the buffer end must be
    // skipped (break), not slice out of bounds.
    let mut disc = DiscBuilder::new(40);
    let mut dir_buf = Vec::new();
    dir_buf.extend_from_slice(&dir_record(30, 2048, false, b"A.BIN"));
    // Append a record claiming length 80 but only provide 10 bytes of it.
    let mut trunc = dir_record(31, 2048, false, b"B.BIN");
    trunc[0] = 80; // lie: declared length 80
    dir_buf.extend_from_slice(&trunc[..10]); // truncated tail
    disc.user_data(18, &dir_buf);

    let path = disc.write_to_temp("legaia_iso_fuzz_trunc.bin");
    let mut raw = RawDisc::open(&path).unwrap();
    let dir = DirectoryRecord {
        lba: 18,
        size: dir_buf.len() as u32,
        is_dir: true,
        name: "ROOT".into(),
    };
    let entries = iso9660::list_directory(&mut raw, &dir).unwrap();
    // Only the first well-formed record survives; the truncated tail is
    // dropped without panicking.
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "A.BIN");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn walk_files_terminates_on_self_referential_directory() {
    // A directory that contains a subdirectory record pointing back at the
    // directory's own LBA would loop forever without cycle detection.
    let mut disc = DiscBuilder::new(40);

    // Root dir at LBA 18 contains one subdir "SUB" that points at LBA 19.
    let mut root_buf = Vec::new();
    root_buf.extend_from_slice(&dir_record(19, USER_DATA_SIZE as u32, true, b"SUB"));
    disc.user_data(18, &root_buf);

    // Subdir at LBA 19 contains a record "LOOP" pointing back at LBA 18, and a
    // self-pointer at LBA 19.
    let mut sub_buf = Vec::new();
    sub_buf.extend_from_slice(&dir_record(18, USER_DATA_SIZE as u32, true, b"LOOP"));
    sub_buf.extend_from_slice(&dir_record(19, USER_DATA_SIZE as u32, true, b"SELF"));
    sub_buf.extend_from_slice(&dir_record(30, 2048, false, b"LEAF.BIN"));
    disc.user_data(19, &sub_buf);

    let path = disc.write_to_temp("legaia_iso_fuzz_cycle.bin");
    let mut raw = RawDisc::open(&path).unwrap();

    let root = DirectoryRecord {
        lba: 18,
        size: root_buf.len() as u32,
        is_dir: true,
        name: String::new(),
    };
    // Must terminate (cycle detection) and surface the single leaf file.
    let files = iso9660::walk_files(&mut raw, &root).unwrap();
    assert!(files.iter().any(|(p, _)| p.ends_with("LEAF.BIN")));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn read_user_data_does_not_overallocate_for_huge_count() {
    // Request more sectors than the disc holds: the reader must error on the
    // missing sector read rather than reserve gigabytes up front.
    let disc = DiscBuilder::new(4);
    let path = disc.write_to_temp("legaia_iso_fuzz_overcount.bin");
    let mut raw = RawDisc::open(&path).unwrap();
    let mut out = Vec::new();
    // count far beyond the 4-sector disc.
    let res = raw.read_user_data(0, 1_000_000, &mut out);
    assert!(res.is_err());
    // The reserve was bounded by the on-disc size, not the requested count.
    assert!(out.capacity() < 1_000_000 * USER_DATA_SIZE);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn full_synthetic_volume_round_trips() {
    // Sanity: a well-formed synthetic volume still parses + lists correctly,
    // proving the hardening didn't break the happy path.
    let mut disc = DiscBuilder::new(40);
    disc.user_data(16, &make_pvd(18, USER_DATA_SIZE as u32));

    let mut root_buf = Vec::new();
    root_buf.extend_from_slice(&dir_record(30, 4096, false, b"FILE1.DAT"));
    root_buf.extend_from_slice(&dir_record(31, 8192, false, b"FILE2.DAT"));
    disc.user_data(18, &root_buf);

    let path = disc.write_to_temp("legaia_iso_fuzz_good.bin");
    let mut raw = RawDisc::open(&path).unwrap();
    let vol = iso9660::read_volume(&mut raw).unwrap();
    let entries = iso9660::list_directory(&mut raw, &vol.root).unwrap();
    assert_eq!(entries.len(), 2);
    let files = iso9660::walk_files(&mut raw, &vol.root).unwrap();
    assert_eq!(files.len(), 2);
    let _ = std::fs::remove_file(&path);
}
