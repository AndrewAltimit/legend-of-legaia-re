//! VDF vertex-morph staging: rest-pose copy + weighted per-vertex deltas.
//!
//! PORT: FUN_8001c604, FUN_8005b038
//!
//! The retail mesh-morph ("set_mime") applier. Per animated actor,
//! `FUN_8001C604(actor, group_idx)`:
//!
//! 1. resolves the TMD group's `(vertex_ptr, vertex_count)` pair through the
//!    actor's group table (`actor+0x44`),
//! 2. copies the group's rest-pose GTE vertices (8 bytes each) into a
//!    scratch window at the top of the `_DAT_8007B85C` asset buffer
//!    (`buf + 0x62C00 - count*8`) and retargets the group's vertex pointer
//!    there - the authored rest pose is never mutated,
//! 3. for each of the actor's `+0x6C` morph slots (VDF sub-entry index
//!    byte at `actor+0xB0+slot`, weight `u16` at `actor+0xA0+slot*2`):
//!    walks the VDF sub-entry's records (`0x80083E58` pointer table - see
//!    `docs/reference/memory-map.md`) and, for every record naming this
//!    `group_idx`, applies its packed deltas at the record's destination
//!    vertex index via `FUN_8005B038`.
//!
//! `FUN_8005B038(dst, deltas, count, weight)` is the GTE blend loop: IR0 =
//! `weight`, per delta `GPF sf=1` computes `(weight * delta) >> 12` per
//! component (IR saturation to `i16` range, `lm=0`), and the scaled delta
//! is **added** (wrapping `i16`) onto the destination vertex triple. So a
//! morph slot contributes `delta * weight / 4096` - weight `0x1000` = the
//! full authored delta.
//!
//! VDF sub-entry record layout (word units, from the `FUN_8001C604` walk
//! `puVar6 += puVar6[2]*2 + 3`):
//!
//! ```text
//!   u32 record_count
//!   per record:
//!     u32 group_id       ; TMD group this record morphs
//!     u32 dst_index      ; first destination vertex
//!     u32 delta_count
//!     delta_count x 8 bytes: [i16 dx][i16 dy][i16 dz][pad]
//! ```
//!
//! Provenance: `ghidra/scripts/funcs/8001c604.txt` (disassembly) +
//! `ghidra/scripts/funcs/8005b038.txt`; the record stride and the
//! actor-side slot arrays match the overlay VDF bring-up
//! (`docs/reference/memory-map.md` `0x80083E58`).
//!
//! REF: FUN_801D77F4

/// One parsed VDF morph record (borrowing the delta payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdfMorphRecord<'a> {
    /// TMD group this record targets.
    pub group_id: u32,
    /// First destination vertex index within the group.
    pub dst_index: u32,
    /// Packed 8-byte deltas (`[i16 dx][i16 dy][i16 dz][pad]` each).
    pub deltas: &'a [u8],
}

impl VdfMorphRecord<'_> {
    /// Delta triple `i` of this record.
    pub fn delta(&self, i: usize) -> (i16, i16, i16) {
        let o = i * 8;
        (
            i16::from_le_bytes([self.deltas[o], self.deltas[o + 1]]),
            i16::from_le_bytes([self.deltas[o + 2], self.deltas[o + 3]]),
            i16::from_le_bytes([self.deltas[o + 4], self.deltas[o + 5]]),
        )
    }

    /// Number of deltas.
    pub fn len(&self) -> usize {
        self.deltas.len() / 8
    }

    /// True when the record carries no deltas.
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }
}

/// Parse a VDF sub-entry's morph records (`[u32 count]` then the record
/// stream). Truncated buffers yield the records that fit.
pub fn parse_vdf_morph_records(entry: &[u8]) -> Vec<VdfMorphRecord<'_>> {
    let mut out = Vec::new();
    if entry.len() < 4 {
        return out;
    }
    let count = u32::from_le_bytes(entry[0..4].try_into().unwrap());
    let mut off = 4usize;
    for _ in 0..count {
        if off + 12 > entry.len() {
            break;
        }
        let group_id = u32::from_le_bytes(entry[off..off + 4].try_into().unwrap());
        let dst_index = u32::from_le_bytes(entry[off + 4..off + 8].try_into().unwrap());
        let delta_count = u32::from_le_bytes(entry[off + 8..off + 12].try_into().unwrap()) as usize;
        let body = off + 12;
        let end = body + delta_count * 8;
        if end > entry.len() {
            break;
        }
        out.push(VdfMorphRecord {
            group_id,
            dst_index,
            deltas: &entry[body..end],
        });
        off = end;
    }
    out
}

/// The `FUN_8005B038` blend: `dst[i] += (delta[i] * weight) >> 12`
/// component-wise, GTE `GPF sf=1, lm=0` semantics - the scaled delta
/// saturates to `i16` range before the (wrapping) add.
///
/// `dst` is a slice of 8-byte GTE vertices (`[i16 x][i16 y][i16 z][attr]`);
/// the attr halfword is untouched.
pub fn apply_weighted_deltas(dst: &mut [u8], start: usize, rec: &VdfMorphRecord, weight: i16) {
    let scale = |d: i16| -> i16 {
        let v = (i32::from(weight) * i32::from(d)) >> 12;
        v.clamp(-0x8000, 0x7FFF) as i16
    };
    for i in 0..rec.len() {
        let vo = (start + i) * 8;
        if vo + 6 > dst.len() {
            break;
        }
        let (dx, dy, dz) = rec.delta(i);
        for (c, d) in [(0, dx), (2, dy), (4, dz)] {
            let cur = i16::from_le_bytes([dst[vo + c], dst[vo + c + 1]]);
            let new = cur.wrapping_add(scale(d));
            dst[vo + c..vo + c + 2].copy_from_slice(&new.to_le_bytes());
        }
    }
}

/// The `FUN_8001C604` staging step for one group: clone the rest-pose
/// vertex bytes (the scratch copy retail places at the top of the asset
/// buffer), then apply every matching record of every `(sub_entry, weight)`
/// morph slot. Returns the morphed vertex buffer.
///
/// `slots` mirrors the actor's `+0xB0` index / `+0xA0` weight arrays as
/// `(sub_entry_bytes, weight)` pairs - the caller resolves the index byte
/// through its VDF pointer table (`World::vdf_record_bytes`).
pub fn stage_group_morph(rest_pose: &[u8], group_idx: u32, slots: &[(&[u8], i16)]) -> Vec<u8> {
    let mut work = rest_pose.to_vec();
    for (entry, weight) in slots {
        for rec in parse_vdf_morph_records(entry) {
            if rec.group_id == group_idx {
                apply_weighted_deltas(&mut work, rec.dst_index as usize, &rec, *weight);
            }
        }
    }
    work
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vert(x: i16, y: i16, z: i16) -> [u8; 8] {
        let mut v = [0u8; 8];
        v[0..2].copy_from_slice(&x.to_le_bytes());
        v[2..4].copy_from_slice(&y.to_le_bytes());
        v[4..6].copy_from_slice(&z.to_le_bytes());
        v
    }

    type SynthRecord<'a> = (u32, u32, &'a [(i16, i16, i16)]);

    fn entry(records: &[SynthRecord]) -> Vec<u8> {
        let mut b = (records.len() as u32).to_le_bytes().to_vec();
        for (g, d, deltas) in records {
            b.extend_from_slice(&g.to_le_bytes());
            b.extend_from_slice(&d.to_le_bytes());
            b.extend_from_slice(&(deltas.len() as u32).to_le_bytes());
            for (x, y, z) in *deltas {
                b.extend_from_slice(&x.to_le_bytes());
                b.extend_from_slice(&y.to_le_bytes());
                b.extend_from_slice(&z.to_le_bytes());
                b.extend_from_slice(&[0, 0]);
            }
        }
        b
    }

    #[test]
    fn parses_record_stream_with_stride() {
        let e = entry(&[(2, 5, &[(1, 2, 3), (4, 5, 6)]), (7, 0, &[(-1, -2, -3)])]);
        let recs = parse_vdf_morph_records(&e);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].group_id, 2);
        assert_eq!(recs[0].dst_index, 5);
        assert_eq!(recs[0].len(), 2);
        assert_eq!(recs[0].delta(1), (4, 5, 6));
        assert_eq!(recs[1].group_id, 7);
        assert_eq!(recs[1].delta(0), (-1, -2, -3));
    }

    #[test]
    fn full_weight_applies_the_authored_delta() {
        // weight 0x1000 = 1.0: dst += delta exactly.
        let mut buf = Vec::new();
        buf.extend_from_slice(&vert(100, -50, 7));
        let e = entry(&[(0, 0, &[(10, -20, 30)])]);
        let recs = parse_vdf_morph_records(&e);
        apply_weighted_deltas(&mut buf, 0, &recs[0], 0x1000);
        assert_eq!(&buf[0..2], &110i16.to_le_bytes());
        assert_eq!(&buf[2..4], &(-70i16).to_le_bytes());
        assert_eq!(&buf[4..6], &37i16.to_le_bytes());
    }

    #[test]
    fn half_weight_scales_by_gpf_shift() {
        // weight 0x800 = 0.5, GPF >> 12: floor((0x800 * d) / 0x1000).
        let mut buf = vert(0, 0, 0).to_vec();
        let e = entry(&[(0, 0, &[(101, -101, 3)])]);
        let recs = parse_vdf_morph_records(&e);
        apply_weighted_deltas(&mut buf, 0, &recs[0], 0x800);
        assert_eq!(&buf[0..2], &50i16.to_le_bytes());
        // Arithmetic shift floors negatives: (-101 * 0x800) >> 12 = -51.
        assert_eq!(&buf[2..4], &(-51i16).to_le_bytes());
        assert_eq!(&buf[4..6], &1i16.to_le_bytes());
    }

    #[test]
    fn stage_group_morph_filters_by_group_and_sums_slots() {
        let rest: Vec<u8> = [vert(10, 10, 10), vert(20, 20, 20)].concat();
        // Slot A morphs group 3 vertex 1; slot B names group 9 (ignored).
        let a = entry(&[(3, 1, &[(0x10, 0, 0)])]);
        let b = entry(&[(9, 0, &[(999, 999, 999)])]);
        let out = stage_group_morph(&rest, 3, &[(&a, 0x1000), (&b, 0x1000)]);
        assert_eq!(&out[0..2], &10i16.to_le_bytes(), "vertex 0 untouched");
        assert_eq!(&out[8..10], &36i16.to_le_bytes(), "vertex 1: 20 + 0x10");
        // Two slots on the same record accumulate.
        let out2 = stage_group_morph(&rest, 3, &[(&a, 0x1000), (&a, 0x1000)]);
        assert_eq!(&out2[8..10], &52i16.to_le_bytes());
        // Rest pose is never mutated (retail's scratch copy).
        assert_eq!(&rest[8..10], &20i16.to_le_bytes());
    }

    #[test]
    fn attr_halfword_is_untouched() {
        let mut v = vert(1, 2, 3);
        v[6] = 0xAB;
        v[7] = 0xCD;
        let mut buf = v.to_vec();
        let e = entry(&[(0, 0, &[(5, 5, 5)])]);
        let recs = parse_vdf_morph_records(&e);
        apply_weighted_deltas(&mut buf, 0, &recs[0], 0x1000);
        assert_eq!(&buf[6..8], &[0xAB, 0xCD]);
    }
}
