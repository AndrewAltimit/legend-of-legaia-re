//! Disc-gated integration test for [`ProtCdDmaHost`].
//!
//! Drives the host against the real retail `PROT.DAT` and asserts:
//!
//! - `prot_index_size_lookup` returns the same LBA count the SCUS
//!   loader-site retail formula `toc[idx+3] - toc[idx+2]` yields.
//! - `prot_one_shot_load` synchronously stages the first `count` LBAs
//!   of an entry into the host's synthetic main-RAM buffer, byte-equal
//!   to a direct `prot_dat_raw_bytes` slice at the same `(byte_offset,
//!   len)`.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! doesn't carry a `PROT.DAT` (same convention as the other disc-gated
//! integration tests).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::cd_dma::{CdDmaHost, DestAddr, LoadFlags, ProtCdDmaHost, SECTOR_BYTES};
use legaia_engine_core::scene::ProtIndex;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_prot() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let extracted = extracted_dir()?;
    let prot = ProtIndex::open_extracted(&extracted).ok()?;
    Some(Arc::new(prot))
}

#[test]
fn size_lookup_matches_retail_formula_across_corpus() {
    let Some(prot) = open_prot() else {
        return;
    };
    let mut host = ProtCdDmaHost::new(prot.clone());

    // Walk the first ~64 entries plus a handful of late-corpus ones.
    let mut indices: Vec<u16> = (0..64).collect();
    indices.extend([100u16, 500, 900, 1000, 1200]);

    for idx in indices {
        if (idx as usize) >= prot.entry_count() {
            continue;
        }
        let host_count = host.prot_index_size_lookup(idx, false);
        let retail_count = prot
            .entry_lba_count_retail(idx)
            .expect("retail formula must resolve for in-range entries");
        assert_eq!(
            host_count, retail_count,
            "host size_lookup vs retail formula diverged at PROT entry {idx}"
        );
        let retail_start = prot
            .entry_start_lba_retail(idx)
            .expect("retail start LBA must resolve");
        assert_eq!(
            host.last_start_lba(),
            retail_start,
            "host last_start_lba vs retail formula diverged at PROT entry {idx}"
        );
        assert_eq!(host.last_prot_idx(), idx);
    }
}

#[test]
fn one_shot_load_populates_main_ram_byte_equal_to_raw_read() {
    let Some(prot) = open_prot() else {
        return;
    };
    let mut host = ProtCdDmaHost::new(prot.clone());

    // PROT entry 0 ("init_data" in retail) is a small, dense entry good
    // for an end-to-end byte-equality check. Skip if entry 0 is missing
    // or implausibly sized.
    if prot.entry_count() == 0 {
        return;
    }
    let count = prot
        .entry_lba_count_retail(0)
        .expect("PROT entry 0 retail count");
    assert!(count > 0, "PROT entry 0 LBA count must be > 0");
    let start_lba = prot
        .entry_start_lba_retail(0)
        .expect("PROT entry 0 start LBA");

    let dst: DestAddr = 0x8010_0000;
    let n = host.prot_one_shot_load(0, dst, LoadFlags::SYNCHRONOUS);
    assert_eq!(n, count, "one_shot_load LBA count matches size_lookup");

    let len_bytes = (count as usize) * (SECTOR_BYTES as usize);
    let host_slice = host
        .read(dst, len_bytes)
        .expect("host main-RAM read covers entire copy");

    let raw_bytes = prot
        .prot_dat_raw_bytes((start_lba as u64) * (SECTOR_BYTES as u64), len_bytes)
        .expect("raw PROT.DAT read");

    assert_eq!(
        host_slice, raw_bytes,
        "host main-RAM bytes diverge from direct PROT.DAT read"
    );
}
