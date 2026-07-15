//! Legaia LZS decompressor.
//!
//! PORT: FUN_8001A55C
//!
//! Reverse-engineered from `FUN_8001a55c` in `SCUS_942.54`. The algorithm:
//!
//! - 4096-byte ring buffer initialized to zero, write position starts at 0xFEE.
//! - LSB-first 8-bit control byte; the high bit of the in-register control is a
//!   `0x100` sentinel signalling "byte exhausted, fetch the next one".
//! - Control bit = 1 → emit one literal byte from the source.
//! - Control bit = 0 → read two bytes (b0, b1):
//!     * absolute window position = `b0 | ((b1 & 0xF0) << 4)` (12 bits)
//!     * length = `(b1 & 0x0F) + 3`
//!     * copy `length` bytes out of the ring buffer starting at that position;
//!       each emitted byte is also stored at the current write position, which
//!       advances mod 4096.
//! - The decompressed output size is supplied externally - there is no length
//!   prefix or end-of-stream marker.
//!
//! `.lzs` *files* are containers: a small u32 header table where pairs at
//! offsets `[2k]`/`[2k+1]` give `(decompressed_size, byte_offset_to_stream)`
//! for each section. `decompress_container` parses that.
//!
//! [`compress`] is the inverse used for re-packing edited assets: a
//! lazy-matching LZSS matcher whose output the retail decoder accepts. It is
//! not a bit-exact clone of Sony's packer - it is validated by
//! `decompress(compress(x)) == x`, and it packs tightly enough that a re-packed
//! asset fits its original (often slack-free) footprint for an in-place edit.

use anyhow::{Result, bail};

const WINDOW_SIZE: usize = 0x1000;
const WINDOW_START_POS: usize = 0xFEE;

pub fn decompress(input: &[u8], expected_output_size: usize) -> Result<Vec<u8>> {
    decompress_tracked(input, expected_output_size).map(|(o, _)| o)
}

/// Same as [`decompress`] but also returns the number of input bytes consumed.
/// Useful for validating container sections - a section that consumes more
/// input bytes than the gap to the next section's offset is mis-parsed.
pub fn decompress_tracked(input: &[u8], expected_output_size: usize) -> Result<(Vec<u8>, usize)> {
    let mut window = [0u8; WINDOW_SIZE];
    let mut window_pos: usize = WINDOW_START_POS;
    // `expected_output_size` is attacker-controlled (the container header's
    // per-section size, or a value a bulk scanner derives from junk bytes). A
    // value near `usize::MAX` would make `with_capacity` attempt a multi-GiB
    // up-front allocation (capacity-overflow / OOM) before the decode loop ever
    // runs and bails on EOF. The decoder can never emit more than one output
    // byte per ~1.5 input bytes (a back-ref copies up to 18 bytes from 2 input
    // bytes plus its control bit), so the input length is a tight upper bound on
    // realisable output. Reserve `min(expected, plausible-from-input)` and let
    // the `Vec` grow naturally on the (valid) path where the bound is generous;
    // this is behaviour-preserving for any real stream.
    let reserve_hint = expected_output_size.min(input.len().saturating_mul(18).saturating_add(64));
    let mut out: Vec<u8> = Vec::with_capacity(reserve_hint);
    let mut src = 0usize;
    let mut control: u32 = 0;

    while out.len() < expected_output_size {
        if (control & 0x100) == 0 {
            if src >= input.len() {
                bail!(
                    "EOF reading control byte at out={}/{}, src={}",
                    out.len(),
                    expected_output_size,
                    src
                );
            }
            control = (input[src] as u32) | 0xFF00;
            src += 1;
        }

        if (control & 1) != 0 {
            if src >= input.len() {
                bail!(
                    "EOF reading literal at out={}/{}",
                    out.len(),
                    expected_output_size
                );
            }
            let v = input[src];
            src += 1;
            out.push(v);
            window[window_pos] = v;
            window_pos = (window_pos + 1) & 0xFFF;
        } else {
            if src + 2 > input.len() {
                bail!(
                    "EOF reading back-ref at out={}/{}",
                    out.len(),
                    expected_output_size
                );
            }
            let b0 = input[src] as u32;
            let b1 = input[src + 1] as u32;
            src += 2;
            let base = b0 | ((b1 & 0xF0) << 4);
            let len = ((b1 & 0x0F) + 3) as usize;
            for n in 0..len {
                let read_pos = ((base + n as u32) & 0xFFF) as usize;
                let v = window[read_pos];
                out.push(v);
                window[window_pos] = v;
                window_pos = (window_pos + 1) & 0xFFF;
                if out.len() >= expected_output_size {
                    break;
                }
            }
        }
        control >>= 1;
    }

    Ok((out, src))
}

// --- Encoder -------------------------------------------------------------
//
// The retail game ships only a *decoder* (`FUN_8001A55C`); there is no Sony
// encoder to reverse. To re-pack edited assets (e.g. a disc patcher) we need an
// encoder that produces a stream the retail decoder accepts byte-for-byte, not
// a bit-identical match of whatever tool Sony used. This is a textbook LZSS
// matcher with one-step lazy matching, whose output is validated by
// `decompress(compress(x)) == x`.
//
// Why this maps cleanly onto the ring-buffer decoder: at the moment the decoder
// is about to emit output byte `i`, its write cursor is `window_pos = (0xFEE +
// i) & 0xFFF`, and `window[r]` holds the most recent output byte whose linear
// position has residue `r`. A back-reference at linear distance `d` (i.e. copy
// the run that started `d` bytes earlier) decodes to `base = (0xFEE + i - d) &
// 0xFFF`. As long as `d <= 4096 - MAX_MATCH`, every read within the copy -
// including the self-overlapping RLE case where `d < len` - resolves to exactly
// `output[i - d + n]`, so a plain linear-history match is reproduced exactly.
// We cap distance at `MAX_DIST` to keep that guarantee unambiguous (it avoids
// the residue aliasing that can occur when a copy wraps across `window_pos`).

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 18; // (b1 & 0x0F) + 3, max nibble 15 -> 18
/// Largest back-reference distance we emit. The decoder's window is 4096 bytes;
/// staying `MAX_MATCH` short of that keeps every in-copy read unambiguous.
const MAX_DIST: usize = WINDOW_SIZE - MAX_MATCH; // 4078

const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const NONE: usize = usize::MAX;
/// Cap on hash-chain traversal per position. Real game assets compress well
/// within a shallow walk; this bounds worst-case time on pathological input.
const MAX_CHAIN: usize = 256;

fn hash3(d: &[u8], i: usize) -> usize {
    let h = (d[i] as u32).wrapping_mul(0x9E37_79B1)
        ^ (d[i + 1] as u32).wrapping_mul(0x85EB_CA77)
        ^ (d[i + 2] as u32).wrapping_mul(0xC2B2_AE3D);
    (h >> (32 - HASH_BITS)) as usize & (HASH_SIZE - 1)
}

/// Compress `input` into a Legaia-LZS stream that `decompress(out, input.len())`
/// reproduces exactly. The decompressed length is *not* stored in the stream
/// (the format carries no length prefix or end marker - the caller supplies the
/// size), so a re-packer must record `input.len()` alongside the output.
///
/// This is a matcher, not a bit-exact clone of Sony's packer. It will not always
/// match the original compressed bytes, but it always decodes back to the input,
/// and it does real (not literal-only) compression so re-packed streams fit the
/// slack in fixed-size slots.
///
/// It uses **one-step lazy matching** (defer a match by a byte when the next
/// position yields a strictly longer one), which closes the small, near-constant
/// gap a purely greedy parse leaves versus the retail packer - enough that a
/// re-packed asset fits its original tightly-packed footprint (e.g. a scene MAN,
/// which has no compressed slack). Positions are inserted into the hash chain
/// one at a time as the cursor reaches them so the lazy look-ahead at `i+1` sees
/// position `i`.
pub fn compress(input: &[u8]) -> Vec<u8> {
    compress_with_chain(input, MAX_CHAIN)
}

/// Optimal-parse variant of [`compress`]: a shortest-encoding dynamic program
/// over the token graph (literal = 1 control bit + 1 byte, back-reference =
/// 1 control bit + 2 bytes), instead of the greedy + one-step-lazy parse.
/// Slower (full match search at every position plus a DP walk) but never
/// longer than [`compress`]'s output, and usually a fraction of a percent
/// shorter - enough to close the couple of bytes by which the greedy parse
/// misses Sony's packer on a handful of retail streams whose on-disc
/// footprint has zero slack. Same round-trip contract:
/// `decompress(compress_optimal(x), x.len()) == x`.
pub fn compress_optimal(input: &[u8]) -> Vec<u8> {
    let n = input.len();
    let mut out: Vec<u8> = Vec::with_capacity(n / 2 + 16);
    if n == 0 {
        return out;
    }

    // Longest match (and its distance) at every position, full-depth search.
    // A match's shorter prefixes share its distance, so one (len, dist) pair
    // per position covers every usable length 3..=len at that position.
    let mut head = vec![NONE; HASH_SIZE];
    let mut prev = vec![NONE; n];
    // (len, dist) of the longest history match at each position; dist == 0 is
    // the zero-window sentinel (see below), never a real history distance.
    let mut best_at: Vec<(u16, u16)> = vec![(0, 0); n];
    for i in 0..n {
        let (l, d) = find_match(input, i, &head, &prev, WINDOW_SIZE);
        best_at[i] = (l as u16, d as u16);
        if i + MIN_MATCH <= n {
            let h = hash3(input, i);
            prev[i] = head[h];
            head[h] = i;
        }
    }

    // Zero-run tokens against the decoder's *initial* window. The ring buffer
    // starts zero-filled, so a run of zeros can also be encoded as a
    // back-reference into cells the decoder has not written yet: with the
    // write cursor at `w`, reading from `base = w + 1` stays ahead of every
    // write while `i + len <= 4095` (the first wrap that could alias). Sony's
    // packer uses this on runs near the start of a stream where there is no
    // zero history to reference yet.
    let mut zero_at: Vec<u16> = vec![0; n];
    {
        let mut run = 0usize;
        for i in (0..n).rev() {
            run = if input[i] == 0 { run + 1 } else { 0 };
            let cap = MAX_MATCH.min(run).min(4095usize.saturating_sub(i));
            zero_at[i] = cap as u16;
        }
    }

    // Exact shortest-encoding DP over (position, tokens-emitted mod 8): a
    // fresh control byte costs 1 extra byte on the token that starts a group
    // of 8, so the emitted length depends on the token phase, not just the
    // token count. State cost = exact bytes emitted so far; a literal costs
    // `1 (+1 at phase 0)`, a back-reference `2 (+1 at phase 0)`.
    const PHASES: usize = 8;
    let idx = |i: usize, r: usize| i * PHASES + r;
    let mut cost = vec![u32::MAX; (n + 1) * PHASES];
    // Chosen token length arriving at each state (1 = literal), plus the
    // zero-window flag (base ahead of the write cursor) for back-references.
    let mut step = vec![0u16; (n + 1) * PHASES];
    let mut zero_step = vec![false; (n + 1) * PHASES];
    cost[idx(0, 0)] = 0;
    for i in 0..n {
        for r in 0..PHASES {
            let c = cost[idx(i, r)];
            if c == u32::MAX {
                continue;
            }
            let ctrl = if r == 0 { 1 } else { 0 };
            let r2 = (r + 1) % PHASES;
            // Literal.
            let lc = c + 1 + ctrl;
            if lc < cost[idx(i + 1, r2)] {
                cost[idx(i + 1, r2)] = lc;
                step[idx(i + 1, r2)] = 1;
            }
            // History back-reference of any usable length.
            let (l, _) = best_at[i];
            let max_l = (l as usize).min(n - i);
            let rc = c + 2 + ctrl;
            for take in MIN_MATCH..=max_l {
                let s = idx(i + take, r2);
                if rc < cost[s] {
                    cost[s] = rc;
                    step[s] = take as u16;
                    zero_step[s] = false;
                }
            }
            // Zero-window back-reference (initial ring-buffer zeros).
            let max_z = (zero_at[i] as usize).min(n - i);
            for take in MIN_MATCH..=max_z {
                let s = idx(i + take, r2);
                if rc < cost[s] {
                    cost[s] = rc;
                    step[s] = take as u16;
                    zero_step[s] = true;
                }
            }
        }
    }

    // Best final phase, then reconstruct the token path backwards.
    let end_r = (0..PHASES)
        .min_by_key(|&r| cost[idx(n, r)])
        .expect("nonempty");
    let mut tokens: Vec<(usize, u16, bool)> = Vec::new(); // (pos, take, zero)
    let mut i = n;
    let mut r = end_r;
    while i > 0 {
        let take = step[idx(i, r)] as usize;
        debug_assert!(take >= 1, "unreachable DP state");
        let pr = (r + PHASES - 1) % PHASES;
        tokens.push((i - take, take as u16, zero_step[idx(i, r)]));
        i -= take;
        r = pr;
    }
    tokens.reverse();

    // Emit tokens along the chosen path.
    let mut ctrl_pos = 0usize;
    let mut nbits = 0u32;
    for (pos, take, via_zero) in tokens {
        if nbits == 0 {
            ctrl_pos = out.len();
            out.push(0);
        }
        let bit = nbits;
        let take = take as usize;
        if take == 1 {
            out[ctrl_pos] |= 1u8 << bit;
            out.push(input[pos]);
        } else {
            let base = if via_zero {
                // Zero-window token: read just ahead of the write cursor,
                // where the ring buffer still holds its initial zeros.
                (WINDOW_START_POS + pos + 1) & 0xFFF
            } else {
                let dist = best_at[pos].1 as usize;
                (WINDOW_START_POS + (pos - dist)) & 0xFFF
            };
            let len_code = (take - MIN_MATCH) as u8;
            out.push((base & 0xFF) as u8);
            out.push(((((base >> 8) & 0xF) as u8) << 4) | len_code);
        }
        nbits += 1;
        if nbits == 8 {
            nbits = 0;
        }
    }
    out
}

fn compress_with_chain(input: &[u8], max_chain: usize) -> Vec<u8> {
    let n = input.len();
    let mut out: Vec<u8> = Vec::with_capacity(n / 2 + 16);
    if n == 0 {
        return out;
    }

    let mut head = vec![NONE; HASH_SIZE];
    let mut prev = vec![NONE; n];

    // `inserted` = number of positions already linked into the hash chains, i.e.
    // [0, inserted) are present and searchable. We keep `inserted == i` before a
    // match search at `i` (so the chain holds only earlier positions), then push
    // `i` itself before the lazy look-ahead at `i+1`.
    let mut inserted = 0usize;
    let insert_one = |head: &mut [usize], prev: &mut [usize], p: usize| {
        if p + MIN_MATCH <= n {
            let h = hash3(input, p);
            prev[p] = head[h];
            head[h] = p;
        }
    };

    let mut ctrl_pos = 0usize;
    let mut nbits = 0u32;

    let mut i = 0usize;
    while i < n {
        // Catch the hash chains up to (but not including) `i`.
        while inserted < i {
            insert_one(&mut head, &mut prev, inserted);
            inserted += 1;
        }

        // Reserve a fresh control byte at the start of every group of 8 tokens.
        if nbits == 0 {
            ctrl_pos = out.len();
            out.push(0);
        }
        let bit = nbits;

        let (len1, dist1) = find_match(input, i, &head, &prev, max_chain);

        // Lazy look-ahead: if a non-maximal match here is beaten by the match at
        // `i+1`, emit a literal now and let the longer match land next step.
        let mut emit_literal = len1 < MIN_MATCH;
        if !emit_literal && len1 < MAX_MATCH && i + 1 < n {
            if inserted == i {
                insert_one(&mut head, &mut prev, i);
                inserted = i + 1;
            }
            let (len2, _) = find_match(input, i + 1, &head, &prev, max_chain);
            if len2 > len1 {
                emit_literal = true;
            }
        }

        if !emit_literal {
            // Back-reference token: control bit stays 0.
            let matchpos = i - dist1;
            let base = (WINDOW_START_POS + matchpos) & 0xFFF;
            let len_code = (len1 - MIN_MATCH) as u8; // 0..=15
            let b0 = (base & 0xFF) as u8;
            let b1 = ((((base >> 8) & 0xF) as u8) << 4) | len_code;
            out.push(b0);
            out.push(b1);

            // Insert every position the match covers (those not already linked).
            let end = (i + len1).min(n);
            while inserted < end {
                insert_one(&mut head, &mut prev, inserted);
                inserted += 1;
            }
            i = end;
        } else {
            // Literal token: set the control bit.
            out[ctrl_pos] |= 1u8 << bit;
            out.push(input[i]);
            if inserted == i {
                insert_one(&mut head, &mut prev, i);
                inserted = i + 1;
            }
            i += 1;
        }

        nbits += 1;
        if nbits == 8 {
            nbits = 0;
        }
    }

    out
}

/// Greedy longest-match search at `i` over the hash chain. Returns
/// `(length, distance)`; `length < MIN_MATCH` means "emit a literal".
fn find_match(
    input: &[u8],
    i: usize,
    head: &[usize],
    prev: &[usize],
    max_chain: usize,
) -> (usize, usize) {
    let n = input.len();
    if i + MIN_MATCH > n {
        return (0, 0);
    }
    let max_len = MAX_MATCH.min(n - i);
    let min_pos = i.saturating_sub(MAX_DIST);
    let h = hash3(input, i);
    let mut cand = head[h];
    let mut best_len = 0usize;
    let mut best_dist = 0usize;
    let mut depth = 0usize;
    while cand != NONE && cand >= min_pos && depth < max_chain {
        depth += 1;
        // Cheap reject: to beat the current best, byte at offset best_len must
        // already match.
        if best_len > 0 && (i + best_len >= n || input[cand + best_len] != input[i + best_len]) {
            cand = prev[cand];
            continue;
        }
        let mut l = 0usize;
        while l < max_len && input[cand + l] == input[i + l] {
            l += 1;
        }
        if l > best_len {
            best_len = l;
            best_dist = i - cand;
            if l == max_len {
                break;
            }
        }
        cand = prev[cand];
    }
    (best_len, best_dist)
}

#[derive(Debug)]
pub struct ContainerSection {
    pub size: u32,
    pub byte_offset: u32,
}

#[derive(Debug)]
pub struct Container {
    pub header_meta: [u32; 2],
    pub sections: Vec<ContainerSection>,
}

/// Parse a Legaia `.lzs` container by scanning the header table for plausible
/// `(size, offset)` pairs. The first two u32s are header metadata (purpose
/// undocumented); subsequent pairs are sections. We stop when a pair would
/// overlap the previous section or run off the file.
pub fn parse_container(file: &[u8]) -> Result<Container> {
    if file.len() < 16 {
        bail!("file too small ({}b) for an LZS container", file.len());
    }
    let header_meta = [
        u32::from_le_bytes(file[0..4].try_into().unwrap()),
        u32::from_le_bytes(file[4..8].try_into().unwrap()),
    ];

    let max_pairs = (file.len() / 8).min(64);
    let mut sections = Vec::new();
    let mut last_end: u32 = 0;
    for k in 1..max_pairs {
        let p = k * 8;
        if p + 8 > file.len() {
            break;
        }
        let size = u32::from_le_bytes(file[p..p + 4].try_into().unwrap()) & 0x00FF_FFFF;
        let off = u32::from_le_bytes(file[p + 4..p + 8].try_into().unwrap());
        if size == 0 || off == 0 {
            break;
        }
        if (off as usize) >= file.len() {
            break;
        }
        if off < last_end {
            break;
        }
        sections.push(ContainerSection {
            size,
            byte_offset: off,
        });
        last_end = off;
    }
    if sections.is_empty() {
        bail!("no plausible sections found in header table");
    }
    Ok(Container {
        header_meta,
        sections,
    })
}

pub fn decompress_container(file: &[u8]) -> Result<Vec<Vec<u8>>> {
    let c = parse_container(file)?;
    let mut out = Vec::with_capacity(c.sections.len());
    for s in &c.sections {
        let stream = &file[s.byte_offset as usize..];
        out.push(decompress(stream, s.size as usize)?);
    }
    Ok(out)
}

/// Decompress every section while validating that each section's input
/// consumption stays within its gap to the next section's offset. Returns
/// `Err` if any section overruns or fails to decode - i.e., the file
/// parsed as a container heuristically but isn't actually a real one.
///
/// Use this to avoid the false-positive trap where the loose
/// [`parse_container`] header heuristic matches a non-LZS file (e.g. a
/// flat u32 offset table) and the greedy decoder happily synthesises bytes
/// from whatever follows. Verified against PROT.DAT: this rejects ~16 false
/// positives that the lenient decoder accepts.
pub fn decompress_container_strict(file: &[u8]) -> Result<Vec<Vec<u8>>> {
    let c = parse_container(file)?;
    let mut out = Vec::with_capacity(c.sections.len());
    for (i, sec) in c.sections.iter().enumerate() {
        let start = sec.byte_offset as usize;
        if start >= file.len() {
            bail!("section {} starts past EOF", i);
        }
        let stream = &file[start..];
        let (decoded, consumed) = decompress_tracked(stream, sec.size as usize)?;
        // The next section's start is the upper bound on bytes we may consume.
        // For the last section, allow up to EOF.
        let upper = if i + 1 < c.sections.len() {
            c.sections[i + 1].byte_offset as usize
        } else {
            file.len()
        };
        let max_consume = upper.saturating_sub(start);
        if consumed > max_consume {
            bail!(
                "section {} consumed {} input bytes but only {} available before next section",
                i,
                consumed,
                max_consume
            );
        }
        out.push(decoded);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
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
}
