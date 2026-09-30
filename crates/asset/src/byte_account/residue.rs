//! Range algebra and residue classification.
//! Split out of `byte_account.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Range algebra
// ---------------------------------------------------------------------------

/// Merge overlapping / touching claims into a sorted, disjoint cover, clamped
/// to `size`. Empty and out-of-range claims are dropped.
pub fn merge_ranges(claims: &[Claim], size: usize) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = claims
        .iter()
        .filter_map(|c| {
            let a = c.start.min(size);
            let b = c.end.min(size);
            (b > a).then_some((a, b))
        })
        .collect();
    v.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Complement of a merged cover over `[0, size)`.
pub fn complement(merged: &[(usize, usize)], size: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for &(a, b) in merged {
        if a > cursor {
            out.push((cursor, a));
        }
        cursor = cursor.max(b);
    }
    if cursor < size {
        out.push((cursor, size));
    }
    out
}

// ---------------------------------------------------------------------------
// Residue classification
// ---------------------------------------------------------------------------

/// Shannon entropy of `buf` in bits/byte.
pub fn entropy_bits(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    let mut hist = [0u32; 256];
    for &b in buf {
        hist[b as usize] += 1;
    }
    let n = buf.len() as f32;
    -hist
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f32 / n;
            p * p.log2()
        })
        .sum::<f32>()
}

pub(super) fn repeats_short_pattern(buf: &[u8]) -> bool {
    for period in [1usize, 2, 4, 8, 16] {
        if buf.len() < period * 2 || !buf.len().is_multiple_of(period) {
            continue;
        }
        if buf.chunks_exact(period).all(|c| c == &buf[..period]) {
            return true;
        }
    }
    false
}

/// Classify one run of unclaimed bytes. See [`ResidueShape`] for the order.
pub fn classify_residue(buf: &[u8]) -> ResidueShape {
    if buf.is_empty() || buf.iter().all(|&b| b == 0) {
        return ResidueShape::ZeroPad;
    }
    if buf.len() < ALIGNMENT_MAX {
        return ResidueShape::Alignment;
    }
    if repeats_short_pattern(buf) {
        return ResidueShape::RepeatedFill;
    }
    let n = buf.len() as f32;
    let printable = buf
        .iter()
        .filter(|&&b| b == 0 || b == b'\n' || b == b'\r' || b == b'\t' || (0x20..0x7F).contains(&b))
        .count() as f32
        / n;
    if printable >= ASCII_MIN {
        return ResidueShape::AsciiText;
    }
    let words = buf.len() / 4;
    if words >= 4 {
        let mut ptrs = 0usize;
        let mut plausible = 0usize;
        let mut special = 0usize;
        let mut seen_ops = 0u64;
        for i in 0..words {
            let w = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
            if (0x8000_0000..0x8020_0000).contains(&w) {
                ptrs += 1;
            }
            let op = (w >> 26) as usize;
            if PLAUSIBLE_OPS[op] {
                plausible += 1;
                seen_ops |= 1u64 << op;
            }
            if op == 0 {
                special += 1;
            }
        }
        let wf = words as f32;
        if ptrs as f32 / wf >= PTR_MIN {
            return ResidueShape::PointerDense;
        }
        if is_bgr555(buf) {
            return ResidueShape::Bgr555;
        }
        // The opcode-plausibility test alone is not enough, and the failure is
        // one-sided: a table of small little-endian values has primary opcode
        // `0` (SPECIAL) in every word, so *any* sparse 16-bit table reads as
        // code. Real code mixes primaries - loads, stores, immediates,
        // branches - so require both a spread and a bound on the SPECIAL share.
        if plausible as f32 / wf >= CODE_PLAUSIBLE_MIN
            && (ptrs as f32 / wf) < CODE_PTR_MAX
            && seen_ops.count_ones() >= CODE_MIN_DISTINCT_OPS
            && (special as f32 / wf) < CODE_SPECIAL_MAX
        {
            return ResidueShape::PlausibleMips;
        }
    }
    let e = entropy_bits(buf);
    if e < LOW_ENTROPY_MAX {
        ResidueShape::LowEntropy
    } else if e >= HIGH_ENTROPY_MIN {
        ResidueShape::HighEntropy
    } else {
        ResidueShape::Mixed
    }
}

/// PSX 15-bit colour data: the STP bit is clear in nearly every halfword, and
/// the run carries a wide, high-entropy spread of values.
///
/// The width test alone would also match a sparse small-value table, so the
/// distinct-value floor and the entropy floor are both load-bearing. Real MIPS
/// never passes it - every `lw` / `sw` / `lui`-pair word puts a halfword at or
/// above `0x8000`.
pub(super) fn is_bgr555(buf: &[u8]) -> bool {
    let n = buf.len() / 2;
    if n < 64 {
        return false;
    }
    let mut low = 0usize;
    let mut seen = std::collections::HashSet::new();
    for i in 0..n {
        let h = u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]);
        if h < 0x8000 {
            low += 1;
        }
        if seen.len() < BGR555_MIN_DISTINCT {
            seen.insert(h);
        }
    }
    low as f32 / n as f32 >= BGR555_MIN
        && seen.len() >= BGR555_MIN_DISTINCT
        && entropy_bits(buf) >= BGR555_MIN_ENTROPY
}

pub(super) fn hex_head(buf: &[u8], n: usize) -> String {
    buf.iter().take(n).map(|b| format!("{b:02x}")).collect()
}

pub(super) fn zero_fraction(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    buf.iter().filter(|&&b| b == 0).count() as f32 / buf.len() as f32
}
