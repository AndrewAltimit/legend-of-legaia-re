//! Disc-gated: what follows a DATA_FIELD stream's terminator.
//!
//! `FUN_8002541C` stops at the first zero-size header and reads nothing past
//! it. Over every PROT entry that walks as a clean DATA_FIELD stream, the
//! bytes after the terminator are the tail of the entry's last `0x800`-byte
//! sector - never a whole sector - and they are zero, except in one entry
//! (`1208`), whose tail carries eight bytes of residue. Skips silently when
//! the disc / extracted `PROT.DAT` is absent.

use std::path::PathBuf;

use legaia_container_formats::parse_streaming;
use legaia_prot::archive::Archive;

const SECTOR: usize = 0x800;

fn prot_path() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn data_field_trailers_are_last_sector_fill() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot) = prot_path() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entries = archive.entries.clone();
    let mut streams = Vec::new();
    let mut residue = Vec::new();
    let mut buf = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        buf.clear();
        if archive.read_entry(e, &mut buf).is_err() {
            continue;
        }
        let Ok(r) = parse_streaming(&buf, 4096) else {
            continue;
        };
        // The clean-stream filter `asset extract` / `asset scan-stream` use.
        if !(r.terminated && r.all_known_types && r.all_magic_ok && r.chunks.len() >= 2) {
            continue;
        }
        streams.push(i);
        let trailer = &buf[r.bytes_consumed..];
        assert_eq!(
            buf.len() % SECTOR,
            0,
            "entry {i}: footprint is whole sectors"
        );
        assert!(
            trailer.len() < SECTOR,
            "entry {i}: {} trailer bytes reach past the last sector",
            trailer.len()
        );
        let nz: Vec<usize> = (0..trailer.len()).filter(|&k| trailer[k] != 0).collect();
        if !nz.is_empty() {
            residue.push((i, r.bytes_consumed + nz[0], nz.len()));
        }
    }
    assert_eq!(streams.len(), 34, "clean DATA_FIELD streams: {streams:?}");
    // Entry 1208: `78 FD 00 00 01 00 0D 00` at 0xFD70 - a word equal to its
    // own offset + 8, then halfwords 1 and 13 - in the sector fill.
    assert_eq!(residue, vec![(1208, 0xFD70, 4)], "non-zero trailer bytes");
}
