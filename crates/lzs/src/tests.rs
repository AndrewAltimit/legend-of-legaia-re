use super::*;

#[test]
fn empty_input_zero_output() {
    let v = decompress(&[], 0).unwrap();
    assert!(v.is_empty());
}

#[test]
fn pure_literals() {
    // control byte 0xFF (8 literals), then 8 bytes
    let input = [0xFF, b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H'];
    let out = decompress(&input, 8).unwrap();
    assert_eq!(out, b"ABCDEFGH");
}

#[test]
fn tracked_reports_input_consumption() {
    // 8 literals consume 1 control + 8 bytes = 9 input bytes.
    let input = [
        0xFF, b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', 0xDE, 0xAD,
    ];
    let (out, consumed) = decompress_tracked(&input, 8).unwrap();
    assert_eq!(out, b"ABCDEFGH");
    assert_eq!(
        consumed, 9,
        "should not eat trailing bytes past the target output"
    );
}

#[test]
fn strict_container_rejects_overrun() {
    // Build a fake container header where section 0 claims to decode
    // 100 bytes but only has 4 input bytes before section 1's offset.
    // The greedy decoder will read past - strict must reject.
    let mut file = Vec::new();
    // meta: [0, 0]
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    // pair 1: size=100, off=24
    file.extend_from_slice(&100u32.to_le_bytes());
    file.extend_from_slice(&24u32.to_le_bytes());
    // pair 2: size=10, off=28  (only 4 bytes after section 0 starts)
    file.extend_from_slice(&10u32.to_le_bytes());
    file.extend_from_slice(&28u32.to_le_bytes());
    // padding to offset 24
    while file.len() < 24 {
        file.push(0);
    }
    // section 0 stream: 4 bytes that the greedy decoder happily synthesizes from
    file.extend_from_slice(&[0xFF, b'A', b'B', b'C']);
    // section 1 stream: more bytes after
    file.extend(std::iter::repeat_n(0, 200));
    // Lenient decoder accepts (greedy):
    assert!(decompress_container(&file).is_ok(), "lenient should accept");
    // Strict rejects because section 0 wants 100 bytes but only has 4 input bytes.
    assert!(
        decompress_container_strict(&file).is_err(),
        "strict must reject overrun"
    );
}

// --- Panic-hardening regression tests ---------------------------------
//
// Bulk scanners feed ARBITRARY PROT-entry bytes at `parse_container` /
// `decompress_container*`, and `decompress` is called with externally
// supplied target sizes. Junk / truncated input must return `Err`, never
// panic (OOB slice, capacity overflow, integer over/underflow).

#[test]
fn decompress_empty_with_nonzero_target_is_err_not_panic() {
    assert!(decompress(&[], 16).is_err());
}

#[test]
fn decompress_truncated_backref_is_err_not_panic() {
    // Control byte 0x00 selects a back-ref, but only one of the two
    // back-ref bytes follows.
    assert!(decompress(&[0x00, 0xEE], 8).is_err());
}

#[test]
fn decompress_one_byte_control_then_eof_is_err_not_panic() {
    // Control byte requests a literal but the source is already exhausted.
    assert!(decompress(&[0x01], 4).is_err());
}

#[test]
fn parse_container_empty_is_err_not_panic() {
    assert!(parse_container(&[]).is_err());
}

#[test]
fn parse_container_one_byte_is_err_not_panic() {
    assert!(parse_container(&[0xAB]).is_err());
}

#[test]
fn parse_container_bogus_huge_size_offset_is_err_not_panic() {
    // 16-byte minimum so we get past the size gate, then a section pair
    // with an enormous size and an offset past EOF.
    let mut file = Vec::new();
    file.extend_from_slice(&0u32.to_le_bytes()); // meta[0]
    file.extend_from_slice(&0u32.to_le_bytes()); // meta[1]
    file.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // size
    file.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // offset (past EOF)
    // No plausible section survives the heuristic -> Err, no panic.
    assert!(parse_container(&file).is_err());
}

#[test]
fn decompress_container_offset_past_eof_does_not_panic() {
    // A header that yields a section whose offset is in-bounds but whose
    // claimed decompressed size far exceeds the available stream. Greedy
    // decode must hit EOF and return Err rather than slicing OOB.
    let mut file = Vec::new();
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&100_000u32.to_le_bytes()); // size
    file.extend_from_slice(&16u32.to_le_bytes()); // offset = 16 (in-bounds)
    // A few junk stream bytes after the header.
    file.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    // Either parse_container rejects or decode bails on EOF; never panics.
    let _ = decompress_container(&file);
    let _ = decompress_container_strict(&file);
}

#[test]
fn parse_container_all_junk_does_not_panic() {
    // 64 bytes of 0xFF: max_pairs heuristic walks but every offset is past
    // EOF, so no section is accepted.
    let file = vec![0xFFu8; 64];
    assert!(parse_container(&file).is_err());
}

#[test]
fn decompress_huge_target_with_tiny_input_does_not_alloc_bomb() {
    // A container/scanner can hand `decompress` an enormous target size
    // derived from junk bytes. The up-front capacity reservation must be
    // bounded by the (tiny) input, so this returns Err on EOF promptly
    // rather than attempting a multi-GiB allocation.
    let input = [0xFFu8, b'A', b'B'];
    let r = decompress(&input, usize::MAX / 2);
    assert!(r.is_err(), "should bail on EOF, not OOM");
}

#[test]
fn decompress_huge_target_still_decodes_available_literals() {
    // Behaviour-preserving check: the capped reservation must not change
    // what valid input decodes to. 8 literals with a huge target still
    // emit exactly the 8 literal bytes before hitting EOF.
    let input = [0xFF, b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H'];
    // Target larger than reachable output -> Err, but the loop still grows
    // `out` correctly up to EOF (no panic on the small initial capacity).
    assert!(decompress(&input, 1_000_000).is_err());
    // Exact target decodes cleanly.
    assert_eq!(decompress(&input, 8).unwrap(), b"ABCDEFGH");
}

// --- Encoder round-trip tests -----------------------------------------

/// Assert `decompress(compress(x), x.len()) == x`.
fn assert_roundtrip(data: &[u8]) {
    let packed = compress(data);
    let unpacked = decompress(&packed, data.len()).expect("re-decode must succeed");
    assert_eq!(unpacked, data, "round-trip mismatch (len {})", data.len());
}

#[test]
fn compress_empty() {
    assert!(compress(&[]).is_empty());
    assert_roundtrip(&[]);
}

#[test]
fn compress_short_inputs_below_min_match() {
    assert_roundtrip(&[0x42]);
    assert_roundtrip(&[0x01, 0x02]);
    assert_roundtrip(b"AB");
}

#[test]
fn compress_literals_only() {
    assert_roundtrip(b"ABCDEFGH");
    assert_roundtrip(b"The quick brown fox jumps over the lazy dog");
}

#[test]
fn compress_long_zero_run_uses_rle() {
    // A long zero run must compress well via overlapping (d=1) back-refs.
    let data = vec![0u8; 4096];
    let packed = compress(&data);
    assert!(
        packed.len() < data.len() / 4,
        "zero run should compress hard, got {} from {}",
        packed.len(),
        data.len()
    );
    assert_roundtrip(&data);
}

#[test]
fn compress_repeated_pattern() {
    let mut data = Vec::new();
    for _ in 0..1000 {
        data.extend_from_slice(b"ABCABCAB");
    }
    let packed = compress(&data);
    assert!(packed.len() < data.len() / 4, "repetition should compress");
    assert_roundtrip(&data);
}

#[test]
fn compress_overlapping_run_of_repeated_byte() {
    // RLE via d=1 self-overlap: 0xAA repeated.
    assert_roundtrip(&[0xAA; 500]);
    // Mixed: short period that exercises overlap with d=2/3.
    let data: Vec<u8> = (0..600).map(|i| (i % 3) as u8).collect();
    assert_roundtrip(&data);
}

#[test]
fn compress_pseudorandom_incompressible() {
    // A simple LCG produces incompressible-looking bytes; the encoder must
    // still round-trip (falling back to literals where no match exists).
    let mut x: u32 = 0x1234_5678;
    let data: Vec<u8> = (0..20_000)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 16) as u8
        })
        .collect();
    assert_roundtrip(&data);
}

#[test]
fn compress_long_input_crossing_window_boundary() {
    // Larger than the 4 KB window, with structure, to exercise the
    // distance cap and chain eviction.
    let mut data = Vec::new();
    let mut x: u32 = 0xDEAD_BEEF;
    for _ in 0..50_000 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        // Bias toward a small alphabet so matches are common.
        data.push(((x >> 20) & 0x0F) as u8);
    }
    assert_roundtrip(&data);
}

#[test]
fn compress_all_byte_values_boundary() {
    let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    assert_roundtrip(&data);
}

/// `compress_optimal` round-trips and is never longer than the greedy
/// parse, across the same shapes the greedy tests use.
#[test]
fn compress_optimal_round_trips_and_never_beats_greedy_backwards() {
    let mut cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0x42],
        b"AB".to_vec(),
        b"The quick brown fox jumps over the lazy dog".to_vec(),
        vec![0u8; 4096],
        vec![0xAA; 500],
        (0..600).map(|i| (i % 3) as u8).collect(),
        (0..=255u8).cycle().take(5000).collect(),
    ];
    let mut x: u32 = 0x1234_5678;
    cases.push(
        (0..20_000)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (x >> 16) as u8
            })
            .collect(),
    );
    for data in cases {
        let opt = compress_optimal(&data);
        assert_eq!(
            decompress(&opt, data.len()).expect("optimal decodes"),
            data,
            "optimal round-trip (len {})",
            data.len()
        );
        let greedy = compress(&data);
        assert!(
            opt.len() <= greedy.len(),
            "optimal ({}) must never exceed greedy ({}) for len {}",
            opt.len(),
            greedy.len(),
            data.len()
        );
    }
}

/// A zero run at the very start of a stream has no history to reference;
/// the optimal parser must encode it via the decoder's initial zero
/// window (and decode back exactly).
#[test]
fn compress_optimal_uses_initial_zero_window() {
    let mut data = vec![0u8; 40];
    data.extend_from_slice(b"tail text after the zeros");
    let opt = compress_optimal(&data);
    assert_eq!(decompress(&opt, data.len()).unwrap(), data);
    // 40 leading zeros must cost far less than 40 literals.
    let literal_floor = 40 + 40 / 8;
    assert!(
        opt.len() < literal_floor,
        "zero prefix should back-reference the virgin window ({} bytes)",
        opt.len()
    );
}

#[test]
fn back_reference_reads_zeros_from_initial_window() {
    // control 0x00 -> 8 back-refs. First back-ref: b0=0xEE, b1=0xF0
    //   abs window pos = 0xEE | ((0xF0 & 0xF0) << 4) = 0xEE | 0xF00 = 0xFEE
    //   length = (0xF0 & 0xF) + 3 = 3
    // Reading from window[0xFEE..0xFF1] = three zero bytes.
    let input = [0x00, 0xEE, 0xF0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let out = decompress(&input, 3).unwrap();
    assert_eq!(out, &[0, 0, 0]);
}
