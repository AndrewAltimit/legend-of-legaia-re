//! The **clip morph** a monster action entry's `+0x04` / `+0x08` fields name:
//! per-vertex deltas the battle draw blends onto the monster's mesh while the
//! clip plays, under a four-key trapezoid envelope.
//!
//! The loader `FUN_800542C8` resolves each 1-based index to a pointer
//! (`0x80054750..0x800547D8`, [`MonsterSpell::effect_offset`] /
//! [`MonsterSpell::aux_offset`]). The reader is `FUN_800495C8`, which the per-actor
//! battle draw `FUN_80048A08` calls before it draws the actor
//! (`jal` at `0x80048A7C`) and undoes with `FUN_80049858` after it
//! (`jal` at `0x800492C0`). Read in `ghidra/scripts/funcs/800495c8.txt` and
//! `80049858.txt`:
//!
//! ```text
//! +0x00  u8      unread
//! +0x01  u8      k1   ramp-up start frame
//! +0x02  u8      k2   full-weight frame
//! +0x03  u8      k3   hold end frame
//! +0x04  u8      k4   ramp-down end frame
//! +0x05  u8[3]   unread
//! +0x08  u32     record_count          (lw 0x8 at 0x80049730)
//! +0x0C  record[record_count]:
//!          u32 object                  TMD object index (stride 0x1C)
//!          u32 first_vertex
//!          u32 count
//!          count x 8-byte deltas      (x, y, z, pad) i16
//! ```
//!
//! This is a scene VDF sub-entry (`legaia_scene_formats::vdf_morph`) behind an
//! eight-byte envelope head. Per lane (`+0x04` first, then `+0x08`) and per
//! record, `FUN_800495C8` saves the authored vertices into the per-slot buffer
//! at `gp[+0xA0C] + 0x1060` (`0x80049778..0x800497C4`) and blends the deltas
//! into them in place through `FUN_8005B038` at the lane weight
//! ([`clip_morph_weight`]); `FUN_80049858` copies the saved vertices back,
//! lane `+0x08` first, so the rest pose is what the next frame starts from.
//!
//! NOT WIRED: no host blends these. The port poses monster meshes by the
//! rigid per-part decode only; wiring is the monster mesh build applying each
//! lane's records at [`clip_morph_weight`] of the clip cursor before the
//! draw, against a copy of the rest vertices.

use super::MonsterSpell;

/// One vertex-delta run of a [`ClipMorph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipMorphRecord {
    /// TMD object the run deforms.
    pub object: u32,
    /// First vertex of the run in that object's vertex array.
    pub first_vertex: u32,
    /// Per-vertex `(x, y, z, pad)` deltas, one per vertex from
    /// [`Self::first_vertex`].
    pub deltas: Vec<[i16; 4]>,
}

/// A parsed clip morph descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipMorph {
    /// Envelope keys `[k1, k2, k3, k4]`, in clip frames (bytes `+0x01..=+0x04`).
    pub keys: [u8; 4],
    /// The delta runs, in record order.
    pub records: Vec<ClipMorphRecord>,
}

/// Parse the clip morph at block-relative `offset`. `None` when the header or
/// any record runs past the block.
pub fn parse_clip_morph(block: &[u8], offset: u32) -> Option<ClipMorph> {
    let o = offset as usize;
    let head = block.get(o..o + 8)?;
    let keys = [head[1], head[2], head[3], head[4]];
    let count = legaia_bytes::u32_le(block, o + 8)?;
    let mut at = o + 0xC;
    let mut records = Vec::new();
    for _ in 0..count {
        let object = legaia_bytes::u32_le(block, at)?;
        let first_vertex = legaia_bytes::u32_le(block, at + 4)?;
        let n = legaia_bytes::u32_le(block, at + 8)? as usize;
        let body = at + 0xC;
        let end = body.checked_add(n.checked_mul(8)?)?;
        let bytes = block.get(body..end)?;
        let deltas = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| {
                [
                    i16::from_le_bytes([c[0], c[1]]),
                    i16::from_le_bytes([c[2], c[3]]),
                    i16::from_le_bytes([c[4], c[5]]),
                    i16::from_le_bytes([c[6], c[7]]),
                ]
            })
            .collect();
        records.push(ClipMorphRecord {
            object,
            first_vertex,
            deltas,
        });
        at = end;
    }
    Some(ClipMorph { keys, records })
}

impl MonsterSpell {
    /// The entry's two clip-morph lanes (`+0x04`, `+0x08`), parsed out of
    /// the decoded `block`.
    pub fn clip_morphs(&self, block: &[u8]) -> [Option<ClipMorph>; 2] {
        [
            self.effect_offset.and_then(|o| parse_clip_morph(block, o)),
            self.aux_offset.and_then(|o| parse_clip_morph(block, o)),
        ]
    }
}

/// The q12 lane weight `FUN_800495C8` derives from the clip cursor `phase`
/// (actor `+0x68`, frames in 12.4 fixed point) and the envelope `keys`
/// (`0x80049634..0x80049704`). With `f = phase >> 4`:
///
/// * `k1 <= f < k2` - ramp up, `((phase - k1*16) << 12) / ((k2 - k1) * 16)`;
/// * `k2 <= f <= k3` - full weight `0x1000`;
/// * `k3 < f <= k4` - ramp down, `((k4*16 - phase) << 12) / ((k4 - k3) * 16)`;
/// * otherwise `0`.
///
/// The quotient is truncated to the halfword the blend call takes
/// (`sll a3,s3,0x10; sra a3,a3,0x10` at `0x800497C8`).
///
/// PORT: FUN_800495C8 (the lane weight)
/// NOT WIRED: see the module note - no host blends clip morphs.
pub fn clip_morph_weight(keys: [u8; 4], phase: i16) -> i16 {
    let [k1, k2, k3, k4] = keys.map(i32::from);
    let phase = i32::from(phase);
    let f = phase >> 4;
    let w = if k1 <= f && f < k2 {
        ((phase - (k1 << 4)) << 12) / ((k2 - k1) << 4)
    } else if k2 <= f && f <= k3 {
        0x1000
    } else if k3 < f && f <= k4 {
        (((k4 << 4) - phase) << 12) / ((k4 - k3) << 4)
    } else {
        0
    };
    w as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    // Green Slime's `+0x04` lane on action 0: keys 0, 9, 9, 0x1D.
    const SLIME: [u8; 4] = [0x00, 0x09, 0x09, 0x1D];

    #[test]
    fn weight_ramps_holds_and_falls() {
        // Ramp up over k1..k2 in sixteenths.
        assert_eq!(clip_morph_weight(SLIME, 0), 0);
        assert_eq!(clip_morph_weight(SLIME, 9 * 16 / 2), 0x800);
        // f == k2 == k3 is the one-frame hold.
        assert_eq!(clip_morph_weight(SLIME, 9 * 16), 0x1000);
        assert_eq!(clip_morph_weight(SLIME, 9 * 16 + 15), 0x1000);
        // Ramp down over k3..k4, measured from the full phase.
        assert_eq!(
            clip_morph_weight(SLIME, 10 * 16),
            (((0x1D0 - 0xA0) << 12) / 0x140) as i16
        );
        assert_eq!(clip_morph_weight(SLIME, 0x1D * 16), 0);
        // Past k4.
        assert_eq!(clip_morph_weight(SLIME, 0x1E * 16), 0);
    }

    #[test]
    fn before_k1_is_zero_and_a_flat_edge_skips_its_ramp() {
        // Monster 7 action 7 lane `+0x08`: keys 0x1E, 0x28, 0x28, 0x3C.
        let keys = [0x1E, 0x28, 0x28, 0x3C];
        assert_eq!(clip_morph_weight(keys, 0x1D * 16), 0);
        assert_eq!(clip_morph_weight(keys, 0x1E * 16), 0);
        assert_eq!(clip_morph_weight(keys, 0x23 * 16), 0x800);
        // k1 == k2: no ramp-up frames, so f == k1 is already the hold.
        let step = [0x05, 0x05, 0x08, 0x08];
        assert_eq!(clip_morph_weight(step, 5 * 16), 0x1000);
        assert_eq!(clip_morph_weight(step, 8 * 16 + 15), 0x1000);
        assert_eq!(clip_morph_weight(step, 9 * 16), 0);
    }

    #[test]
    fn parses_the_vdf_sub_entry_behind_the_head() {
        let mut b = vec![0u8; 4];
        b.extend_from_slice(&[0x00, 0x24, 0x27, 0x27, 0x2C, 0, 0, 0]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&10u32.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        for v in [[1i16, -2, 3, 0], [-4, 5, -6, 0]] {
            for c in v {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        let m = parse_clip_morph(&b, 4).unwrap();
        assert_eq!(m.keys, [0x24, 0x27, 0x27, 0x2C]);
        assert_eq!(
            m.records,
            vec![ClipMorphRecord {
                object: 10,
                first_vertex: 3,
                deltas: vec![[1, -2, 3, 0], [-4, 5, -6, 0]],
            }]
        );
        // A truncated run is rejected.
        assert!(parse_clip_morph(&b[..b.len() - 1], 4).is_none());
    }
}
