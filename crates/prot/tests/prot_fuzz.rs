//! Panic-hardening regression suite for the `legaia-prot` parser entry points.
//!
//! Bulk scanners and the web disc browser hand arbitrary bytes to the PROT
//! TOC parser (`Archive::from_bytes`), the standalone TIM-pack heuristic
//! (`timpack`), and the CDNAME map parser. A junk image must return `Err` /
//! an empty result, never panic (OOB slice, capacity-overflow from an
//! unverified count, or arithmetic over/underflow in the LBA / sector math).
//!
//! Every input here is SYNTHETIC; the suite runs with `LEGAIA_DISC_BIN` unset.

use legaia_prot::archive::Archive;
use legaia_prot::{cdname, timpack};

/// A spread of adversarial byte buffers for the binary parsers.
fn adversarial_inputs() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0xFF; 7],
        vec![0x00; 12],
        vec![0xFF; 12],
    ];

    // Header-shaped prefixes with bogus huge file_num / header_sectors. A
    // parser that trusts these and allocates a TOC the size of the claim
    // would abort on capacity overflow or read far past EOF.
    for &(file_num, header_sectors) in &[
        (u32::MAX, 1u32),
        (1, u32::MAX),
        (0x10000, 0x10000),
        (0x7FFF_FFFF, 0x7FFF_FFFF),
        (3, 1),
    ] {
        let mut b = vec![0u8; 16];
        b[4..8].copy_from_slice(&file_num.to_le_bytes());
        b[8..12].copy_from_slice(&header_sectors.to_le_bytes());
        v.push(b.clone());
        // Same header but padded out to a couple of sectors.
        b.resize(0x1800, 0xCD);
        v.push(b);
    }

    // TIM-pack-shaped prefixes: marker byte at [3]==0x01 with bogus huge
    // tim_num, plus near-misses around the marker.
    for &tim_num in &[i32::MAX, -1, 0, 1, 0x1000_0000] {
        let mut b = vec![0u8; 12];
        b[2] = 0x05; // < 0x10
        b[3] = 0x01; // marker
        b[4..8].copy_from_slice(&tim_num.to_le_bytes());
        v.push(b.clone());
        b.resize(64, 0x10);
        v.push(b);
    }

    // Constant fills and pseudo-random streams.
    for &n in &[31usize, 64, 0x800, 0x1000, 0x2000] {
        v.push(vec![0x00; n]);
        v.push(vec![0xFF; n]);
        let mut s: u32 = 0xC0FF_EE00 ^ (n as u32);
        v.push(
            (0..n)
                .map(|_| {
                    s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (s >> 16) as u8
                })
                .collect(),
        );
    }

    v
}

#[test]
fn archive_from_bytes_never_panics() {
    for buf in adversarial_inputs() {
        // Most junk returns Err ("header not found"); a coincidental header
        // shape must still parse without panicking and yield bounded entries.
        if let Ok(arc) = Archive::from_bytes(buf.clone()) {
            assert!(arc.entries.len() <= arc.toc.len());
            for e in &arc.entries {
                // Every retained entry must fit inside the image.
                assert!(e.byte_offset + e.size_bytes <= arc.file_len());
            }
        }
    }
}

#[test]
fn timpack_never_panics() {
    for buf in adversarial_inputs() {
        let claimed = timpack::is_tim_pack(&buf);
        // `unpack` must never panic regardless of the heuristic verdict.
        let items = timpack::unpack(&buf);
        if !claimed {
            assert!(items.is_empty(), "non-pack produced items");
        }
        // Each extracted slice must lie within the source buffer.
        let total: usize = items.iter().map(|i| i.len()).sum();
        assert!(total <= buf.len());
        for item in &items {
            let _ = timpack::detected_ext(item);
        }
    }
}

#[test]
fn cdname_parse_str_never_panics() {
    // The CDNAME parser is text-driven; feed it adversarial text shapes.
    let texts = [
        "",
        "#define",
        "#define name",
        "#define name notanumber",
        "#define name 4294967295",
        "#define x 99999999999999999999",
        "#define a 1\n#define b 2\nrandom junk\n#define",
        "\u{0}\u{0}\u{0}garbage\n#define n 0",
    ];
    for t in texts {
        let _ = cdname::parse_str(t);
    }
    // A large repetitive input.
    let big = "#define n 1\n".repeat(100_000);
    let map = cdname::parse_str(&big).expect("infallible on text");
    assert!(map.len() <= 1); // all share index 1
}
