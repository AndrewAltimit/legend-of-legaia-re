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
//! The port blends onto a **clone** of the mesh ([`apply_clip_morphs`]) and
//! poses the clone, which is the same as retail's save / blend / restore: the
//! authored vertices are never changed. The table the indices address is
//! [`clip_morph_table`]; the engine installs it per monster actor and both
//! play hosts draw the blended mesh (`World::battle_morphed_tmd`).

use super::{MIN_RECORD_BYTES, MonsterSpell, decode_block};

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

/// Every clip morph of monster `id`'s block, addressed by the on-disc
/// 1-based lane index an action entry carries at `+0x04` / `+0x08`
/// (`table[index - 1]`): the loader's
/// `block[(index + magic_count + 0x12) * 4]` offset (`0x80054768..0x8005478C`),
/// parsed. The table runs to the largest index any entry names; a slot
/// whose offset does not parse is `None`. `Ok(None)` for a filler slot.
pub fn clip_morph_table(entry: &[u8], id: u16) -> anyhow::Result<Option<Vec<Option<ClipMorph>>>> {
    let Some(block) = decode_block(entry, id)? else {
        return Ok(None);
    };
    if block.len() < MIN_RECORD_BYTES {
        return Ok(None);
    }
    let mc = block[0x4a] as usize;
    let mut max_index = 0u32;
    for i in 0..mc {
        let Some(e) = legaia_bytes::u32_le(&block, 0x4c + i * 4) else {
            break;
        };
        for lane in [4, 8] {
            if let Some(ix) = legaia_bytes::u32_le(&block, e as usize + lane)
                && ix < 0x100
            {
                max_index = max_index.max(ix);
            }
        }
    }
    let table = (1..=max_index as usize)
        .map(|ix| {
            let off = legaia_bytes::u32_le(&block, (ix + mc + 0x12) * 4)?;
            (off != 0).then(|| parse_clip_morph(&block, off)).flatten()
        })
        .collect();
    Ok(Some(table))
}

/// Blend each `(morph, weight)` lane into `tmd`'s vertices, in order: every
/// record adds `(delta * weight) >> 12` (saturated to a halfword, then a
/// wrapping add - `FUN_8005B038`'s `GPF sf=1, lm=0`) to the `count`
/// vertices of its object from `first_vertex` (`0x80049740..0x800497E0`).
/// A zero weight is skipped; a run past its object's vertices is clipped
/// (retail writes it unchecked, and no disc run needs it).
///
/// PORT: FUN_800495C8 (the per-record blend)
pub fn apply_clip_morphs(tmd: &mut legaia_tmd::Tmd, lanes: &[(&ClipMorph, i16)]) {
    let scale = |w: i16, d: i16| -> i16 {
        ((i32::from(w) * i32::from(d)) >> 12).clamp(-0x8000, 0x7FFF) as i16
    };
    for &(m, w) in lanes {
        if w == 0 {
            continue;
        }
        for r in &m.records {
            let Some(o) = tmd.objects.get_mut(r.object as usize) else {
                continue;
            };
            for (k, d) in r.deltas.iter().enumerate() {
                let Some(v) = o.vertices.get_mut(r.first_vertex as usize + k) else {
                    break;
                };
                v.x = v.x.wrapping_add(scale(w, d[0]));
                v.y = v.y.wrapping_add(scale(w, d[1]));
                v.z = v.z.wrapping_add(scale(w, d[2]));
            }
        }
    }
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

    fn one_object_tmd(verts: &[[i16; 3]]) -> legaia_tmd::Tmd {
        legaia_tmd::Tmd {
            header: legaia_tmd::Header {
                id: 0x8000_0002,
                flags: 0,
                nobj: 1,
                flist_bit_set: false,
            },
            objects: vec![legaia_tmd::Object {
                header: legaia_tmd::ObjectHeader {
                    vert_top: 0,
                    n_vert: verts.len() as u32,
                    normal_top: 0,
                    n_normal: 0,
                    prim_top: 0,
                    n_primitive: 0,
                    scale: 0,
                },
                vertices: verts
                    .iter()
                    .map(|v| legaia_tmd::Vector {
                        x: v[0],
                        y: v[1],
                        z: v[2],
                        _pad: 0,
                    })
                    .collect(),
                normals: Vec::new(),
                primitives_byte_offset: 0,
                primitives_byte_size: 0,
                claimed_n_primitive: 0,
                primitives_psx_walk: None,
            }],
        }
    }

    #[test]
    fn blend_adds_weighted_deltas_lane_after_lane() {
        let mut tmd = one_object_tmd(&[[0, 0, 0], [10, 20, 30], [5, 5, 5]]);
        let a = ClipMorph {
            keys: [0, 1, 1, 2],
            records: vec![ClipMorphRecord {
                object: 0,
                first_vertex: 1,
                deltas: vec![[0x100, -0x100, 3, 0], [8, 0, 0, 0]],
            }],
        };
        let b = ClipMorph {
            keys: [0, 1, 1, 2],
            records: vec![ClipMorphRecord {
                object: 0,
                first_vertex: 2,
                // Runs past the object's three vertices: clipped.
                deltas: vec![[0x40, 0, 0, 0], [99, 99, 99, 0]],
            }],
        };
        // Half weight on lane `+0x04`, full on `+0x08`; `>> 12` floors
        // (`3 * 0x800 >> 12 = 1`, `-0x100 * 0x800 >> 12 = -0x80`).
        apply_clip_morphs(&mut tmd, &[(&a, 0x800), (&b, 0x1000)]);
        let v: Vec<[i16; 3]> = tmd.objects[0]
            .vertices
            .iter()
            .map(|v| [v.x, v.y, v.z])
            .collect();
        assert_eq!(
            v,
            vec![[0, 0, 0], [10 + 0x80, 20 - 0x80, 31], [5 + 4 + 0x40, 5, 5]]
        );
        // A zero weight leaves the rest pose.
        let mut rest = one_object_tmd(&[[1, 2, 3]]);
        apply_clip_morphs(&mut rest, &[(&a, 0)]);
        assert_eq!(rest.objects[0].vertices[0].x, 1);
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
