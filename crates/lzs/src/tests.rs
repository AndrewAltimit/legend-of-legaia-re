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

// --- Production decoder against the reference ring decoder ----------------

/// SplitMix64: deterministic, and no dependency for one test module.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// Bytes, `consumed`, and the error text on a failed decode: all equal.
fn assert_matches_reference(input: &[u8], expected: usize) {
    let new = decompress_tracked(input, expected).map_err(|e| e.to_string());
    let old = reference_decoder::decode(input, expected);
    match (&new, &old) {
        (Ok((n, nc)), Ok((o, oc))) => {
            assert_eq!(
                nc,
                oc,
                "consumed, {} input bytes, size {expected}",
                input.len()
            );
            if n != o {
                let at = n.iter().zip(o).position(|(a, b)| a != b);
                panic!(
                    "output differs at {at:?}, lengths {} and {}, {} input bytes, size {expected}",
                    n.len(),
                    o.len(),
                    input.len()
                );
            }
        }
        (Err(n), Err(o)) => assert_eq!(n, o, "{} input bytes, size {expected}", input.len()),
        _ => panic!(
            "one failed: new {:?}, old {:?}, {} input bytes, size {expected}",
            new.as_ref().map(|r| r.1),
            old.as_ref().map(|r| r.1),
            input.len()
        ),
    }
}

/// A stream built token by token.
#[derive(Default)]
struct Stream {
    bytes: Vec<u8>,
    control_at: usize,
    bits: u32,
}

impl Stream {
    fn control_bit(&mut self, literal: bool) {
        if self.bits == 8 || self.bytes.is_empty() {
            self.control_at = self.bytes.len();
            self.bytes.push(0);
            self.bits = 0;
        }
        if literal {
            self.bytes[self.control_at] |= 1 << self.bits;
        }
        self.bits += 1;
    }
    fn literal(&mut self, v: u8) {
        self.control_bit(true);
        self.bytes.push(v);
    }
    fn reference(&mut self, base: usize, len: usize) {
        self.control_bit(false);
        self.bytes.push(base as u8);
        self.bytes
            .push((((base >> 8) as u8) << 4) | (len - 3) as u8);
    }
}

/// Every position at every interesting cursor: both ends of the distance range
/// (a position equal to the cursor is the 4096 case), reads that straddle the
/// start of the output, and copies that wrap the ring either way.
#[test]
fn reference_positions_match_the_ring_at_each_cursor() {
    let mut rng = Rng(0x11);
    for written in [
        0usize, 1, 2, 5, 17, 18, 4076, 4077, 4078, 4079, 4095, 4096, 4097, 4114, 8192, 8210,
    ] {
        let mut prefix = Stream::default();
        for _ in 0..written {
            prefix.literal(rng.next() as u8);
        }
        let cursor = (0xFEE + written) & 0xFFF;
        let every_position = written < 32;
        for base in 0..0x1000usize {
            let near = |at: usize| {
                (base.wrapping_sub(at) & 0xFFF).min(at.wrapping_sub(base) & 0xFFF) <= 24
            };
            let (near_cursor, near_seam) = (near(cursor), near(0xFEE));
            if !(every_position || near_cursor || near_seam || base % 61 == 0) {
                continue;
            }
            for len in [3usize, 4, 17, 18] {
                let mut s = Stream {
                    bytes: prefix.bytes.clone(),
                    control_at: prefix.control_at,
                    bits: prefix.bits,
                };
                s.reference(base, len);
                s.literal(0xA5);
                // The whole stream, a stop at every point inside the copy, and
                // one more byte than the stream can produce.
                assert_matches_reference(&s.bytes, written + len + 1);
                for stop in 1..len {
                    assert_matches_reference(&s.bytes, written + stop);
                }
                assert_matches_reference(&s.bytes, written + len + 2);
            }
        }
    }
}

/// LZS decodes most input without error, so garbage is the common case for a
/// scanner: random bytes, at sizes that finish, stop short or run out.
#[test]
fn random_garbage_matches_the_reference() {
    let mut rng = Rng(0x22);
    for _ in 0..4000 {
        let len = rng.below(1200);
        let input = rng.bytes(len);
        for expected in [
            0,
            1,
            rng.below(32),
            rng.below(len + 1),
            rng.below(len * 3 + 1),
            rng.below(len * 18 + 65),
            rng.below(0x4000),
        ] {
            assert_matches_reference(&input, expected);
        }
    }
    // Sizes past anything the input can yield, up to the allocation guard.
    for _ in 0..200 {
        let len = rng.below(64);
        let input = rng.bytes(len);
        for expected in [
            len * 18 + 63,
            len * 18 + 64,
            len * 18 + 65,
            1 << 24,
            1 << 40,
            usize::MAX,
        ] {
            assert_matches_reference(&input, expected);
        }
    }
}

/// Reference-heavy streams long enough to wrap the ring many times, with the
/// mix of zeros, near copies and far copies random bytes rarely give.
#[test]
fn random_reference_streams_wrap_the_ring_like_the_reference() {
    let mut rng = Rng(0x33);
    for _ in 0..120 {
        let literal_percent = [0, 3, 25, 60][rng.below(4)];
        let tokens = 200 + rng.below(6000);
        let mut s = Stream::default();
        let mut written = 0usize;
        for _ in 0..tokens {
            if rng.below(100) < literal_percent {
                s.literal(rng.next() as u8);
                written += 1;
            } else {
                let len = 3 + rng.below(16);
                let cursor = (0xFEE + written) & 0xFFF;
                let base = match rng.below(4) {
                    0 => cursor.wrapping_sub(1 + rng.below(32)) & 0xFFF,
                    1 => (cursor + rng.below(32)) & 0xFFF,
                    _ => rng.below(0x1000),
                };
                s.reference(base, len);
                written += len;
            }
        }
        assert_matches_reference(&s.bytes, written);
        assert_matches_reference(&s.bytes, rng.below(written + 1));
        assert_matches_reference(&s.bytes, written + 1 + rng.below(32));
        for _ in 0..16 {
            let cut = rng.below(s.bytes.len());
            assert_matches_reference(&s.bytes[..cut], written);
            // Flip bytes: the same shape, with the tokens re-dealt from there on.
            let mut flipped = s.bytes.clone();
            let at = rng.below(flipped.len());
            flipped[at] ^= 1 + rng.below(255) as u8;
            assert_matches_reference(&flipped, written);
        }
    }
}

/// Every truncation and every size of a real encoder stream.
#[test]
fn encoder_streams_truncated_everywhere_match_the_reference() {
    let mut rng = Rng(0x44);
    let mut plain = Vec::new();
    while plain.len() < 6000 {
        match rng.below(3) {
            0 => plain.extend(std::iter::repeat_n(rng.next() as u8, 1 + rng.below(64))),
            1 => {
                let n = 1 + rng.below(32);
                plain.extend(rng.bytes(n));
            }
            _ => {
                let from = rng.below(plain.len() + 1);
                let n = (plain.len() - from).min(1 + rng.below(40));
                plain.extend_from_within(from..from + n);
            }
        }
    }
    for packed in [compress(&plain), compress_optimal(&plain)] {
        assert_matches_reference(&packed, plain.len());
        assert_eq!(decompress(&packed, plain.len()).unwrap(), plain);
        for cut in 0..packed.len() {
            assert_matches_reference(&packed[..cut], plain.len());
        }
        for size in 0..plain.len() {
            assert_matches_reference(&packed, size);
        }
    }
}
