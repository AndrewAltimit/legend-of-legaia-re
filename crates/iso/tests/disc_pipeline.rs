//! End-to-end integration test against a real Legaia (USA) disc image.
//!
//! Set the `LEGAIA_DISC_BIN` env var to the absolute path of a Mode2/2352 .bin.
//! If the var isn't set, the test prints a one-line skip notice and returns OK
//! so it doesn't fail in environments without the disc (CI, others' machines).
//!
//! What it covers:
//! - SHA-256 of the .bin matches the known-good NA hash
//! - ISO9660 walk yields the expected file count
//! - SCUS_942.54, CDNAME.TXT, SYSTEM.CNF, PROT.DAT, DMY.DAT all extract with
//!   their known sizes and hashes
//!
//! Hashes are pinned to the project author's dump. Different tools / settings
//! may produce slightly different files; in that case, regenerate the
//! expected hashes locally and update them here.

use std::io::Read;
use std::path::PathBuf;

use legaia_iso::iso9660;
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use sha2::{Digest, Sha256};

const KNOWN_BIN_SHA256: &str = "e6120a5d70716dd2f026a2da32d0171d52651971b52c4347a68541299f75258c";

/// Files we expect to find with known sizes (in bytes) and SHA-256 hashes.
const EXPECTED_FILES: &[(&str, u64, &str)] = &[
    (
        "SYSTEM.CNF",
        65,
        "3c841f5e9d9e3a68f23a857e2af2dd59363317e250dc8a0e346780ca222ddf6b",
    ),
    (
        "SCUS_942.54",
        442_368,
        "292256e2e66db42727f613406785e444254d3f699569e611f65fcf1c6d2f3482",
    ),
    (
        "CDNAME.TXT",
        2_551,
        "105616fad5d3524d7607425e629d94fec43de828dc5090985e27edeea3484914",
    ),
    (
        "PROT.DAT",
        121_253_888,
        "97469d80432465676f1bcadadd5a4fc3a6140c2cbb7c548e3b4ef20c3617e9f2",
    ),
    (
        "DMY.DAT",
        36_975_078,
        "bda6c3d4faead7a3b6b85da4c0b8fcea7de81cfdb2feeb60ff82f6be61c48627",
    ),
];

/// Total file count expected on the NA disc.
const EXPECTED_FILE_COUNT: usize = 45;

fn disc_bin_path() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)
}

#[test]
fn pipeline_against_real_disc() {
    let Some(bin) = disc_bin_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping pipeline test");
        return;
    };
    if !bin.exists() {
        panic!("LEGAIA_DISC_BIN={} does not exist", bin.display());
    }

    // 1. SHA-256 of the .bin
    let actual_hash = sha256_file(&bin).expect("hashing .bin");
    assert_eq!(
        actual_hash, KNOWN_BIN_SHA256,
        ".bin SHA-256 mismatch — your dump may have been produced with different \
         tooling or is a different region. Update KNOWN_BIN_SHA256 in this test \
         if you intend to support a new dump."
    );

    // 2. Open + walk the disc
    let mut disc = RawDisc::open(&bin).expect("opening disc");
    let vol = iso9660::read_volume(&mut disc).expect("reading volume");
    let files = iso9660::walk_files(&mut disc, &vol.root).expect("walking files");
    assert_eq!(
        files.len(),
        EXPECTED_FILE_COUNT,
        "expected {} files on disc, found {}",
        EXPECTED_FILE_COUNT,
        files.len()
    );

    // Index by path (basename for top-level, full path for nested)
    let by_name: std::collections::HashMap<&str, &iso9660::DirectoryRecord> =
        files.iter().map(|(p, e)| (p.as_str(), e)).collect();

    // 3. For each expected file, extract & verify
    let mut buf = Vec::new();
    for (name, want_size, want_hash) in EXPECTED_FILES {
        let entry = by_name
            .get(name)
            .unwrap_or_else(|| panic!("expected file {} not found in disc walk", name));
        assert_eq!(
            entry.size as u64, *want_size,
            "{}: expected size {} bytes, got {}",
            name, want_size, entry.size
        );

        let sector_count = entry.size.div_ceil(USER_DATA_SIZE as u32);
        disc.read_user_data(entry.lba, sector_count, &mut buf)
            .unwrap_or_else(|e| panic!("read {} failed: {}", name, e));
        buf.truncate(entry.size as usize);

        let mut hasher = Sha256::new();
        hasher.update(&buf);
        let got_hash = format!("{:x}", hasher.finalize());
        assert_eq!(
            got_hash, *want_hash,
            "{}: SHA-256 mismatch. Expected {}, got {}",
            name, want_hash, got_hash
        );
    }
}

fn sha256_file(path: &PathBuf) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
