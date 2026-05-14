//! Static fog-LUT locator for the world-map distance-cue post-process.
//!
//! The world-map overlay's eight per-prim leaves at
//! `0x801F7644..0x801F8690` each apply a per-vertex distance-cue tint
//! whose RGB delta is sampled from a single shared lookup table. The
//! MIPS shape is constant across all leaves:
//!
//! ```mips
//! lw  v0, -0x2bc(gp)        ; v0 = lut_ptr
//! srl s0, s0, 0x5           ; s0 = Z >> 5   (index, 0..2047 for 16-bit Z)
//! sll s0, s0, 0x1           ; s0 = 2*index  (u16 byte stride)
//! addu s0, s0, v0           ; s0 = lut_ptr + 2*index
//! lh  t9, 0x2(s0)           ; t9 = (s16) *(lut_ptr + 2 + 2*index)
//! sll t9, t9, 0x10
//! addu s1, s1, t9           ; s1 += (tint << 16)
//! ```
//!
//! The `+2` offset means the table effectively starts at `lut_ptr + 2`,
//! and 2048 u16 entries (4096 bytes) cover the full `Z >> 5` index range.
//!
//! On retail USA disc the table sits inside `SCUS_942.54` at file offset
//! `0x05FCC0` (vaddr `0x8006FCC0`). The entries form a near-linear ramp
//! from `0x0000` (no fog at near-Z) to `0x01FF` (saturated 9-bit fog
//! strength at far-Z). The runtime adds this scalar to all three RGB
//! channels of the GTE color register via three sequential indexings -
//! effectively a depth-cue brightness contribution that the per-kingdom
//! `fog_color` (at gp-0x2DC) then tints.
//!
//! ## API
//!
//! [`find`] performs a content-based scan rather than hardcoding the
//! offset, so a PAL / JP build (or a regional variant that shifts SCUS
//! offsets) still resolves the LUT. The scan is fast (one linear
//! traversal of SCUS) and is run once per disc load.

const LUT_ENTRIES: usize = 2048;
pub const LUT_BYTES: usize = LUT_ENTRIES * 2;

/// Locate the fog LUT in a SCUS executable. Returns the 4 KiB byte slice
/// (`LUT_BYTES`) starting at the LUT's `entry 0`. Returns `None` if no
/// matching signature surfaces (modded disc, region whose SCUS is
/// shaped differently, or a buffer that isn't SCUS at all).
///
/// Signature requirements:
/// - 2048 consecutive u16 entries
/// - First entry is exactly `0x0000` (no fog at Z=0)
/// - All entries have the STP bit clear (BGR555 with `bit 15 = 0`)
/// - The sequence is monotonically non-decreasing across at least 99%
///   of adjacent pairs (allows for plateau saturation at the far end)
/// - Final entry climbs to at least `0x0080` (the per-channel
///   contribution is non-trivial, ruling out all-zero / near-flat
///   tables)
///
/// Implementation note: the scan starts at every 4-byte boundary,
/// matching MIPS data alignment. SCUS is ~432 KiB so the scan visits
/// ~108K candidates; with a tight inner loop this completes in a few
/// ms even on the WASM main thread.
pub fn find(scus: &[u8]) -> Option<&[u8]> {
    if scus.len() < LUT_BYTES {
        return None;
    }
    let limit = scus.len() - LUT_BYTES;
    let mut off = 0;
    while off <= limit {
        if let Some(slice) = check(scus, off) {
            return Some(slice);
        }
        off += 4;
    }
    None
}

fn check(scus: &[u8], off: usize) -> Option<&[u8]> {
    let slice = &scus[off..off + LUT_BYTES];
    // The first 8 entries of the retail fog LUT are all 0x0000 (the
    // near-Z range produces no fog tint). Requiring the leading 16
    // bytes to be zero filters out windows that slid into a generic
    // zero-padded region adjacent to a different shape.
    if !slice[..16].iter().all(|&b| b == 0) {
        return None;
    }
    // Parse as u16 LE, validating shape as we go. Bail at the first
    // disqualifying signal so most candidate offsets exit cheaply.
    let mut prev = 0u16;
    let mut violations = 0usize;
    let mut last = 0u16;
    let mut nonzero = 0usize;
    for chunk in slice.chunks_exact(2) {
        let v = u16::from_le_bytes([chunk[0], chunk[1]]);
        if v & 0x8000 != 0 {
            return None; // STP bit set; not a fog LUT entry
        }
        // Monotone non-decreasing (allows brief plateau at saturation
        // - retail saturates at 0x01FF for the final few entries).
        if v < prev {
            violations += 1;
            if violations > 16 {
                return None;
            }
        }
        if v != 0 {
            nonzero += 1;
        }
        prev = v;
        last = v;
    }
    if last < 0x80 {
        return None; // didn't climb meaningfully
    }
    // A real LUT has 2030+ nonzero entries (only the leading 8-ish are
    // zero); a sparse table with a single jump at the end doesn't.
    if nonzero < LUT_ENTRIES - 64 {
        return None;
    }
    Some(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 2048-entry LUT that mirrors the retail shape: the first
    /// 8 entries are zero (no fog at near-Z), then a near-linear ramp
    /// climbs to ~0x01FF over the remaining 2040 entries, saturating
    /// for the final handful.
    fn synth_lut() -> Vec<u8> {
        let mut out = Vec::with_capacity(LUT_BYTES);
        for i in 0..LUT_ENTRIES {
            let v = if i < 8 {
                0
            } else {
                let pos = (i - 8) as u32;
                ((pos * 511 / (LUT_ENTRIES as u32 - 9)).min(511)) as u16
            };
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Pack a `vec![non_zero; size]` so the scanner can't slide its
    /// window into an all-zero region adjacent to the LUT and produce
    /// a leading-zeros + ramp pattern. The retail SCUS is densely
    /// populated with code + data, so this matches the production
    /// scan condition rather than masking it.
    fn nonzero_pad(size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size);
        for i in 0..size {
            // STP-bit-set values so the scanner can't extend a window
            // here into a valid LUT shape.
            buf.push(0x80 | ((i * 31) & 0x7F) as u8);
        }
        buf
    }

    #[test]
    fn finds_a_well_formed_synthetic_lut() {
        let mut buf = nonzero_pad(0x10000);
        let target = 0x4000;
        let lut = synth_lut();
        buf[target..target + LUT_BYTES].copy_from_slice(&lut);
        let hit = find(&buf).expect("LUT located");
        assert_eq!(hit.as_ptr(), buf[target..].as_ptr());
        assert_eq!(hit.len(), LUT_BYTES);
    }

    #[test]
    fn rejects_flat_zero_buffer() {
        let buf = vec![0u8; LUT_BYTES + 16];
        assert!(find(&buf).is_none());
    }

    #[test]
    fn rejects_random_buffer() {
        // High-entropy buffer: STP bits should hit early.
        let mut buf = vec![0u8; 0x4000];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((i * 7 + 3) & 0xFF) as u8;
        }
        assert!(find(&buf).is_none());
    }

    #[test]
    fn rejects_lut_that_doesnt_start_at_zero() {
        let mut buf = nonzero_pad(0x10000);
        let mut lut = synth_lut();
        // Shift everything up by 4 so the leading 8 entries (16 bytes)
        // are no longer 0. With nonzero padding, the scanner can't slide
        // into an all-zero region either.
        for chunk in lut.chunks_exact_mut(2) {
            let v = u16::from_le_bytes([chunk[0], chunk[1]]) + 4;
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        buf[0x2000..0x2000 + LUT_BYTES].copy_from_slice(&lut);
        assert!(find(&buf).is_none());
    }

    #[test]
    fn rejects_lut_with_stp_bit_set() {
        let mut buf = nonzero_pad(0x10000);
        let mut lut = synth_lut();
        // Flip the STP bit on entry 100.
        let i = 100 * 2 + 1;
        lut[i] |= 0x80;
        buf[0x3000..0x3000 + LUT_BYTES].copy_from_slice(&lut);
        assert!(find(&buf).is_none());
    }

    /// Uses a real-disc test if the env var is set, mirroring the
    /// other disc-gated tests in this repo. Validates that the
    /// scanner pins a single LUT slice on the retail USA build at
    /// the byte location decoded by the static analysis (SCUS offset
    /// 0x05FCC0).
    #[test]
    fn finds_retail_fog_lut_when_disc_available() {
        let Ok(path) = std::env::var("LEGAIA_DISC_BIN") else {
            return; // skip silently when no disc
        };
        let disc = std::fs::read(&path).expect("read disc");
        let scus = crate::disc::extract_scus(&disc).expect("extract SCUS");
        let hit = find(&scus).expect("LUT located in retail SCUS");
        assert_eq!(hit.len(), LUT_BYTES, "LUT length");
        // Entry 0 must be exact zero (and so must entry 7).
        assert_eq!(&hit[..16], &[0u8; 16], "leading 8 entries are zero");
        // Last entry should saturate around 0x01FF (= 511).
        let last = u16::from_le_bytes([hit[LUT_BYTES - 2], hit[LUT_BYTES - 1]]);
        assert!(
            (0x100..=0x300).contains(&last),
            "last entry climbed into expected saturation range, got 0x{last:04X}"
        );
    }
}
