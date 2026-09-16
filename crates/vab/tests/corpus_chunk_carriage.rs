//! Disc-gated: a retail VAB is carried as two chunks, and the VAG bodies are
//! the second of them.
//!
//! Two claims, both over every VAB in the extracted PROT corpus:
//!
//! 1. The word immediately before `pBAV` is a DATA_FIELD chunk header of type
//!    `0x00` whose payload length is the VAB's header part
//!    (`legaia_vab::header_part_size`).
//! 2. [`legaia_vab::vag_body_origin`] lands on a self-consistent SPU-ADPCM
//!    block grid - every 16-byte block's high nibble is a legal filter index
//!    `0..=4` - while the origin [`legaia_vab::parse`] reports does not. That
//!    is the measurement, not the assumption: an illegal filter index is what
//!    stops the decoder, so a 100 % legal grid at one origin and a
//!    coin-flip rate at the other separates them without decoding anything.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_vab::{header_part_size, parse, vag_body_origin};

fn extracted_prot_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c).join("PROT");
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// Share of 16-byte blocks at `off` whose filter nibble is legal (`0..=4`).
fn legal_filter_share(buf: &[u8], off: usize, blocks_max: usize) -> f64 {
    let blocks = ((buf.len().saturating_sub(off)) / 16).min(blocks_max);
    if blocks == 0 {
        return 0.0;
    }
    let good = (0..blocks)
        .filter(|b| (buf[off + b * 16] >> 4) <= 4)
        .count();
    good as f64 / blocks as f64
}

#[test]
fn every_corpus_vab_is_carried_as_two_chunks() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
        return;
    };

    let mut checked = 0usize;
    let mut displaced = 0usize;
    for entry in std::fs::read_dir(&prot).expect("read extracted/PROT") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("BIN") {
            continue;
        }
        let data = std::fs::read(&path).expect("read PROT entry");
        // Only the VABs that begin a chunk stream at offset 0 - the multi-bank
        // archive's banks start on their own sectors and are covered by
        // `legaia_asset::vab_multi_bank`.
        if data.len() < 0x40 || &data[4..8] != b"pBAV" {
            continue;
        }
        let Ok(report) = parse(&data, 4) else {
            continue;
        };
        let head = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(
            head >> 24,
            0,
            "{}: chunk 0 type byte is not 0x00",
            path.display()
        );
        assert_eq!(
            (head & 0x00FF_FFFF) as usize,
            header_part_size(report.header.ps as usize),
            "{}: chunk 0 payload is not the VAB header part",
            path.display()
        );

        let origin = vag_body_origin(&data, 0).expect("the body chunk is in this stream");
        let parse_origin = 4 + header_part_size(report.header.ps as usize);
        // Probe only the body chunk's own blocks: past its end the stream
        // carries the SEQ, which is not an ADPCM grid and would drag the share
        // down wherever the body is the shorter of the two.
        let body_blocks =
            (report.header.fsize as usize - header_part_size(report.header.ps as usize)) / 16;
        assert!(
            origin > parse_origin,
            "{}: the body chunk cannot precede the header part",
            path.display()
        );
        if origin != parse_origin + 4 {
            displaced += 1;
        }
        // Prove it by contrast: the origin `parse` reports is not a block grid.
        assert!(
            legal_filter_share(&data, parse_origin, body_blocks) < 0.99,
            "{}: `parse`'s body origin {parse_origin:#x} also reads as a clean grid - \
             the two origins are indistinguishable here",
            path.display()
        );
        assert!(
            legal_filter_share(&data, origin, body_blocks) > 0.99,
            "{}: no ADPCM grid at the chunk-derived body origin {origin:#x}",
            path.display()
        );
        checked += 1;
    }

    assert!(
        checked > 100,
        "expected the extracted corpus to surface many top-level VABs, found {checked}"
    );
    assert!(
        displaced > 0,
        "expected some entries to put another chunk between the two halves"
    );
    eprintln!(
        "[ok]    {checked} top-level VABs carried as two chunks; {displaced} with a chunk between the halves"
    );
}
