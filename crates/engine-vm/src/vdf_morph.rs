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
//! ## Where the two inputs come from
//!
//! Both arrays `FUN_8001C604` reads off the actor are authored by ported
//! code, which is what makes [`stage_group_morph_for_actor`] a real
//! composition rather than a synthetic one:
//!
//! * move-VM op `0x0A` writes the slot count (`+0x6C`) and one
//!   `(sub_entry_index, up_curve, down_curve)` triple per slot - the index
//!   at `+0xB0 + i` (a **byte** stride), the two curves at `+0xB8 + i*2`
//!   and `+0xC8 + i*2`;
//! * the ramp envelope [`crate::move_buffer::envelope_tick`]
//!   (`FUN_80020740`) moves each slot's weight at `+0xA0 + i*2` up to
//!   `0x1000` and back down at that slot's own two velocities.
//!
//! `legaia_engine_core::world::World::stage_actor_group_morph` is the host:
//! it pairs the actor's `MoveBufferState` with the scene's VDF buffer
//! (`World::vdf_record_bytes`, retail's `0x80083E58` walk) and returns the
//! staged vertex buffer. What is still missing for a *visible* morph is on
//! the renderer's side - actor meshes draw by walking `Actor::tmd_ref`'s
//! object vertices directly, and nothing yet substitutes the staged buffer
//! for a group's authored vertices on the frame it is built.
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

/// Stage a group's morph straight off a live actor's ramp-envelope state -
/// the composition `FUN_8001C604` performs, with the two arrays it reads
/// supplied by the structs the ported VM already writes:
///
/// * `state.bone_count` is the actor's `+0x6C` slot count, set by move-VM
///   op `0x0A`;
/// * `state.vdf_slot[i]` is the `+0xB0 + i` VDF sub-entry index, also set by
///   op `0x0A`;
/// * `state.lanes[i]` is the `+0xA0 + i*2` weight, ramped every frame by
///   [`crate::move_buffer::envelope_tick`].
///
/// `resolve` is the host's VDF pointer-table lookup (retail `0x80083E58`;
/// the engine's is `World::vdf_record_bytes`). Slots whose index does not
/// resolve are skipped, matching the retail bail-through.
///
/// Weights are `0..=0x1000` unsigned in the envelope and signed in the GTE
/// blend; the cast here is the same reinterpretation retail's `lhu` into an
/// `IR0` write performs.
pub fn stage_group_morph_for_actor<'a>(
    rest_pose: &[u8],
    group_idx: u32,
    state: &crate::move_buffer::MoveBufferState,
    mut resolve: impl FnMut(u8) -> Option<&'a [u8]>,
) -> Vec<u8> {
    let slots: Vec<(&[u8], i16)> = (0..state.bone_count as usize)
        .filter_map(|i| {
            let entry = resolve(state.vdf_slot[i])?;
            Some((entry, state.lanes[i] as i16))
        })
        .collect();
    stage_group_morph(rest_pose, group_idx, &slots)
}

/// Retail lane-array capacity on the actor record: the weights live at
/// `+0xA0..+0xB0` (8 u16 halfwords) and the sub-entry index bytes at
/// `+0xB0..+0xB8`, so a part can drive at most 8 morph lanes.
pub const ACTOR_MORPH_LANES: usize = 8;

/// Read the morph-lane weight halfword at `actor + 0xA0 + lane*2` off the
/// retail overlap storage ([`crate::move_vm::ActorState`] keeps the
/// `+0xA0..` window split across `keyframe_desc` / `field_a8` /
/// `anim_block`, mirroring the retail record layout - see
/// `ActorState::zero_keyframe_weight`).
pub fn keyframe_weight(state: &crate::move_vm::ActorState, lane: usize) -> u16 {
    match 0xA0 + lane * 2 {
        off @ 0xA0..=0xA6 => state.keyframe_desc[(off - 0xA0) / 2],
        0xA8 => (state.field_a8 as u32 & 0xFFFF) as u16,
        0xAA => (state.field_a8 as u32 >> 16) as u16,
        off => state.anim_block_u16(off - 0xAC),
    }
}

/// Write the morph-lane weight halfword at `actor + 0xA0 + lane*2` (the
/// mutating sibling of [`keyframe_weight`]).
pub fn set_keyframe_weight(state: &mut crate::move_vm::ActorState, lane: usize, value: u16) {
    match 0xA0 + lane * 2 {
        off @ 0xA0..=0xA6 => state.keyframe_desc[(off - 0xA0) / 2] = value,
        0xA8 => {
            state.field_a8 = ((state.field_a8 as u32 & 0xFFFF_0000) | u32::from(value)) as i32;
        }
        0xAA => {
            state.field_a8 =
                ((state.field_a8 as u32 & 0x0000_FFFF) | (u32::from(value) << 16)) as i32;
        }
        off => state.anim_block_u16_set(off - 0xAC, value),
    }
}

/// The morph lanes a move-VM part carries: `(vdf_sub_entry_index, weight)`
/// per armed lane, read off the retail record arrays op `0x0A` writes
/// (`+0x6C` count, `+0xB0 + lane` index byte, `+0xA0 + lane*2` weight).
pub fn actor_morph_lanes(state: &crate::move_vm::ActorState) -> Vec<(u8, u16)> {
    let count = (state.keyframe_count as usize).min(ACTOR_MORPH_LANES);
    (0..count)
        .map(|lane| {
            (
                state.anim_block_u8(0x04 + lane), // +0xB0 + lane
                keyframe_weight(state, lane),
            )
        })
        .collect()
}

/// Run the retail ramp envelope (`FUN_80020740`,
/// [`crate::move_buffer::envelope_tick`]) over a move-VM part's morph
/// lanes, bridging the [`crate::move_vm::ActorState`] overlap storage into
/// the envelope's [`crate::move_buffer::MoveBufferState`] field view:
///
/// * `bone_count`   = `+0x6C` (`keyframe_count`, op `0x0A`)
/// * `lanes[i]`     = `+0xA0 + i*2` ([`keyframe_weight`])
/// * `up/down[i]`   = `+0xB8 + i*2` / `+0xC8 + i*2` (anim block)
/// * `done_mask`    = `+0x7C` (`field_7c`)
/// * `env_flags`    = `+0x62` (`local_flags` - the records steer the
///   envelope through move-VM ops `0x31`/`0x32`, e.g. town0e's
///   `0x32 [0x1000]` LANE0_SNAP_DOWN cycle and rikuroa's `0x32 [0x400]`
///   HOLD)
///
/// Retail runs the envelope from the per-frame actor tick gated on the
/// `+0x10` flag bit `0x1000`
/// ([`crate::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE`], the same bit op
/// `0x0A` sets and the render substitution tests); callers mirror that
/// gate.
///
/// PORT: FUN_80020740 (via `move_buffer::envelope_tick`; this is the
/// ActorState storage bridge)
pub fn envelope_tick_actor(state: &mut crate::move_vm::ActorState, frame_delta: u8) {
    use crate::move_buffer::MoveBufferState;
    let mut env = MoveBufferState {
        bone_count: (state.keyframe_count).min(ACTOR_MORPH_LANES as u8),
        done_mask: state.field_7c,
        env_flags: state.local_flags,
        ..Default::default()
    };
    for lane in 0..env.bone_count as usize {
        env.lanes[lane] = keyframe_weight(state, lane);
        env.up_velocity[lane] = state.anim_block_u16(0x0C + lane * 2) as i16;
        env.down_velocity[lane] = state.anim_block_u16(0x1C + lane * 2) as i16;
    }
    crate::move_buffer::envelope_tick(&mut env, frame_delta);
    for lane in 0..env.bone_count as usize {
        set_keyframe_weight(state, lane, env.lanes[lane]);
    }
    state.field_7c = env.done_mask;
    state.local_flags = env.env_flags;
}

/// Summed weighted morph deltas for one TMD group: the [`stage_group_morph`]
/// arithmetic applied to a zero rest pose, read back as one `[dx, dy, dz]`
/// triple per vertex. Adding the triple (wrapping) onto the authored vertex
/// reproduces the staged buffer exactly - the wrapping add is associative,
/// and the GTE saturation applies to the scaled per-record delta on both
/// paths.
pub fn sum_group_deltas(n_verts: usize, group_idx: u32, slots: &[(&[u8], i16)]) -> Vec<[i16; 3]> {
    let staged = stage_group_morph(&vec![0u8; n_verts * 8], group_idx, slots);
    (0..n_verts)
        .map(|i| {
            let o = i * 8;
            [
                i16::from_le_bytes([staged[o], staged[o + 1]]),
                i16::from_le_bytes([staged[o + 2], staged[o + 3]]),
                i16::from_le_bytes([staged[o + 4], staged[o + 5]]),
            ]
        })
        .collect()
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

    /// The retail record chain on a move-VM **part** (the ambient-tree
    /// shape): op `0x0A` arms the lanes + flags bit `0x1000`, op `0x32`
    /// steers the envelope flags, and the ActorState envelope bridge ramps
    /// the `+0xA0` weights the render substitution reads back.
    #[test]
    fn op0a_arms_actor_state_and_envelope_bridge_ramps_weights() {
        use crate::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE;
        use crate::move_vm::{ActorState, StepResult};

        struct NullHost;
        impl crate::move_vm::MoveHost for NullHost {}

        // town0e record 11's arm: `0A 00 02 (0A 800 800) (0B 800 800)`.
        let bc: Vec<u16> = vec![
            0x0A, 0x0000, 0x0002, 0x000A, 0x0800, 0x0800, 0x000B, 0x0800, 0x0800, 0x32,
            0x1000, // env_flags |= LANE0_SNAP_DOWN
            0x08,   // HALT
        ];
        let mut st = ActorState::new();
        let mut host = NullHost;
        while let StepResult::Advance = crate::move_vm::step(&mut host, &mut st, &bc) {}
        assert_ne!(
            st.flags & STATUS_FLAG_ENVELOPE_ACTIVE,
            0,
            "op 0A arms +0x10 bit 0x1000"
        );
        assert_eq!(
            actor_morph_lanes(&st),
            vec![(0x0A, 0), (0x0B, 0)],
            "lane indices land in the +0xB0 byte array, weights start 0"
        );

        // Envelope: lane 0 ramps first (cascade - lane 1 waits on lane 0's
        // peak). Op 0x0A pre-scales the record's velocity by the host's
        // curve multiplier (default 0x10, the retail scratchpad scalar), so
        // 0x800 raw is a one-tick rise here.
        envelope_tick_actor(&mut st, 1);
        assert!(keyframe_weight(&st, 0) > 0, "lane 0 ramps on tick 1");
        assert_eq!(keyframe_weight(&st, 1), 0, "lane 1 cascades behind lane 0");
        envelope_tick_actor(&mut st, 1);
        envelope_tick_actor(&mut st, 1);
        assert_eq!(keyframe_weight(&st, 0), 0x1000, "lane 0 clamped at peak");
        // Keep ticking: lane 1 peaks, the down-ramp drains, and with
        // LANE0_SNAP_DOWN the cycle restarts instead of retiring.
        let mut weights = Vec::new();
        for _ in 0..16 {
            envelope_tick_actor(&mut st, 1);
            weights.push((keyframe_weight(&st, 0), keyframe_weight(&st, 1)));
        }
        assert!(
            weights.iter().any(|&(w0, _)| w0 == 0),
            "down-ramp drains lane 0: {weights:x?}"
        );
        assert!(
            weights.windows(2).any(|w| w[0].0 == 0 && w[1].0 > 0),
            "LANE0_SNAP_DOWN recycles the pulse: {weights:x?}"
        );
    }

    #[test]
    fn sum_group_deltas_matches_staged_minus_rest() {
        let rest: Vec<u8> = [vert(100, 200, 300), vert(-5, -6, -7)].concat();
        let e = entry(&[(3, 0, &[(10, -20, 30), (1, 2, 3)])]);
        let slots: Vec<(&[u8], i16)> = vec![(&e, 0x800)];
        let staged = stage_group_morph(&rest, 3, &slots);
        let deltas = sum_group_deltas(2, 3, &slots);
        for (i, d) in deltas.iter().enumerate() {
            let o = i * 8;
            let rest_x = i16::from_le_bytes([rest[o], rest[o + 1]]);
            let staged_x = i16::from_le_bytes([staged[o], staged[o + 1]]);
            assert_eq!(rest_x.wrapping_add(d[0]), staged_x, "vertex {i} x");
        }
        assert_eq!(deltas[0], [5, -10, 15]);
    }

    /// The full retail chain on one actor: move-VM op `0x0A` installs the
    /// morph slots, the ramp envelope moves the weights, and the stager
    /// blends the deltas at whatever weight the envelope reached. Each of
    /// the three steps is a separate ported kernel; this pins them together
    /// the way `FUN_8001C604` sees them.
    #[test]
    fn op0a_then_envelope_then_stage_is_one_chain() {
        use crate::move_buffer::{MoveBufferState, envelope_tick};

        let rest = [vert(0, 0, 0), vert(0, 0, 0)].concat();
        // Slot 0 morphs group 3 vertex 0 by +0x100 at full weight.
        let record = entry(&[(3, 0, &[(0x100, 0, 0)])]);

        // What move-VM op 0x0A leaves behind for one slot: count, the VDF
        // sub-entry index, and the lane's up-ramp velocity.
        let mut state = MoveBufferState {
            bone_count: 1,
            ..Default::default()
        };
        state.vdf_slot[0] = 7;
        state.up_velocity[0] = 0x400;

        // Zero weight -> the rest pose is returned untouched.
        let out =
            stage_group_morph_for_actor(&rest, 3, &state, |i| (i == 7).then_some(&record[..]));
        assert_eq!(&out[0..2], &0i16.to_le_bytes());

        // Two envelope frames put the lane at 0x800 - half weight, so the
        // 0x100 delta lands as 0x80.
        envelope_tick(&mut state, 1);
        envelope_tick(&mut state, 1);
        assert_eq!(state.lanes[0], 0x800);
        let out =
            stage_group_morph_for_actor(&rest, 3, &state, |i| (i == 7).then_some(&record[..]));
        assert_eq!(&out[0..2], &0x80i16.to_le_bytes());

        // An index the host cannot resolve is skipped, not fatal.
        let out = stage_group_morph_for_actor(&rest, 3, &state, |_| None);
        assert_eq!(&out[0..2], &0i16.to_le_bytes());
    }
}
