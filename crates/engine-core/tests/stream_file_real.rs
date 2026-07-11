//! Disc-gated oracle for the stream-file API port (`stream_file` module):
//! the retail open/seek/read chain (`FUN_800558FC` / `FUN_80055A5C` /
//! `FUN_800559EC` / `FUN_8003E964`) over the real PROT.DAT must land on the
//! exact bytes the extraction pipeline produced for the same entries.
//!
//! Exercises the two retail consumers' access shapes:
//! - the summon/readef streaming SM (`FUN_801F17F8`): open raw TOC index
//!   `0x37F`/`0x380`, seek `slot * 0x10800` from base, read one slot;
//! - the player-file loader (`FUN_80052770`): open then immediate read with
//!   no intervening seek.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset or `extracted/` is
//! missing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::stream_file::{SeekWhence, StreamFileHost};

/// Retail raw TOC indices (see docs/formats/summon-readef.md).
const SUMMON_RAW: u16 = 0x37F; // extraction 893
const READEF_RAW: u16 = 0x380; // extraction 894
/// Vahn's PLAYER1 battle file (`FUN_80052770`: raw `char + 0x360 + 1`).
const PLAYER1_RAW: u16 = 0x361; // extraction 863

const SLOT_BYTES: usize = 0x10800; // 33 sectors

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file() && p.join("PROT").is_dir())
}

/// Read the extraction pipeline's bytes for entry `idx` (independent oracle:
/// these files were produced by `legaia-extract`, not by `ProtIndex`).
fn extraction_entry_bytes(root: &Path, idx: u16) -> Option<Vec<u8>> {
    let dir = root.join("PROT");
    let prefix = format!("{idx:04}_");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(&prefix))?;
    std::fs::read(dir.join(name)).ok()
}

#[test]
fn stream_file_reads_match_the_extraction_path() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ missing");
        return;
    };
    let prot = Arc::new(ProtIndex::open_extracted(&root).expect("open ProtIndex"));
    let mut host = StreamFileHost::new(prot);

    // --- summon.dat / readef.DAT: the FUN_801F17F8 access shape ----------
    for (label, raw_idx, extraction_idx, slot) in [
        ("summon.dat", SUMMON_RAW, 893u16, 2usize),
        ("readef.DAT", READEF_RAW, 894u16, 1usize),
    ] {
        let oracle = extraction_entry_bytes(&root, extraction_idx)
            .unwrap_or_else(|| panic!("{label}: extracted entry {extraction_idx} missing"));

        // Open by raw TOC index: size = the retail neighbour delta, which
        // for these two entries equals the extraction footprint exactly
        // (they are 0x10800-slot multiples with no over-read tail).
        let sectors = host.open_raw(raw_idx).expect("open");
        assert_eq!(
            sectors as usize * 0x800,
            oracle.len(),
            "{label}: open sector count vs extraction footprint"
        );

        // Streaming-SM shape: seek slot*0x10800 from base, read one slot.
        host.seek((slot * SLOT_BYTES) as u32, SeekWhence::FromBase)
            .expect("seek to slot");
        let mut buf = vec![0u8; SLOT_BYTES];
        let n = host.read(&mut buf).expect("read slot");
        assert_eq!(n, SLOT_BYTES);
        assert_eq!(
            buf,
            &oracle[slot * SLOT_BYTES..(slot + 1) * SLOT_BYTES],
            "{label}: slot {slot} bytes vs extraction"
        );

        // Sequential continuation: the next read (no seek) must be the
        // following slot - the FUN_8003DE7C completion advance.
        let mut buf2 = vec![0u8; SLOT_BYTES];
        host.read(&mut buf2).expect("read next slot");
        assert_eq!(
            buf2,
            &oracle[(slot + 1) * SLOT_BYTES..(slot + 2) * SLOT_BYTES],
            "{label}: sequential read continues at slot {}",
            slot + 1
        );
    }

    // --- PLAYER1: the FUN_80052770 access shape (open, read, no seek) ----
    let oracle = extraction_entry_bytes(&root, 863).expect("extracted entry 863 missing");
    host.open_raw(PLAYER1_RAW).expect("open PLAYER1");
    let mut head = vec![0u8; 0x8000];
    let n = host.read(&mut head).expect("read PLAYER1 head");
    assert_eq!(n, 0x8000);
    assert_eq!(
        head,
        &oracle[..0x8000],
        "PLAYER1: first 0x8000 bytes vs extraction"
    );

    // Relative seek (the loader's `li a2, 0x1` whence): skip one sector
    // forward and verify against the extraction bytes at the shifted offset.
    host.seek(0x800, SeekWhence::FromCurrent).expect("seek cur");
    let mut next = vec![0u8; 0x800];
    host.read(&mut next).expect("read after relative seek");
    assert_eq!(
        next,
        &oracle[0x8800..0x9000],
        "PLAYER1: relative-seek read lands one sector past the head read"
    );
}
