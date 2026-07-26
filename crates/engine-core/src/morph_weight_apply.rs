//! Morph-weight **apply pass** - the per-frame actor tick that restores a
//! mesh's rest pose and re-blends its morph deltas at the live weight.
//!
//! (The `PORT:` tags sit on [`apply_morph_weights`] and
//! [`MorphWeightEnvelope::tick`], the two items that implement the body.)
//!
//! `FUN_8002174C` is a spawn-descriptor handler like the rest of its family:
//! descriptor `0x8007068C`'s `+0x8` word reads `0x8002174C` straight off
//! `extracted/SCUS_942.54`, which is the same evidence
//! [`crate::actor_handler`] uses for the handlers it names. Its `actor+0x0C`
//! identity is already in the engine as
//! [`crate::actor_handler::VA_MORPH_WEIGHTS`].
//!
//! ## Two passes, and why the order is observable
//!
//! The actor carries a morph block at `actor+0x4C` (`[u32 count]` then one
//! slot per record) and a concatenated rest-pose vertex stream at
//! `actor+0x90`. The TMD object table is at `actor[+0x48] + 0xC` - the base
//! pointer plus the 12-byte TMD header - with the standard `0x1C`-byte
//! `OBJECT` stride, so `+0` is the group's vertex pointer and `+4` its
//! vertex count.
//!
//! Pass one (`0x80021794..0x8002180C`) walks every record and copies that
//! group's whole rest pose - `n_vert` 8-byte GTE vertices, moved with
//! unaligned `lwl`/`lwr` + `swl`/`swr` pairs - out of the `actor+0x90`
//! stream, whose cursor advances *continuously across records*. Pass two
//! (`0x80021824..0x8002187C`) then walks the same records again and applies
//! each one's deltas at the live weight `actor+0x6E` through the GTE blend
//! `FUN_8005B038`.
//!
//! Splitting them is not stylistic. Two records naming the same group are
//! legal, and a fused loop would have the second record's rest-pose restore
//! wipe the first record's deltas. All restores happen before any blend, so
//! both contribute - see
//! [`two_records_on_one_group_both_survive`](self#tests).
//!
//! ## The slot stride
//!
//! Each record's header is three words - `[u32 group_id][u32 first_vertex]
//! [u32 delta_count]` - and its deltas follow at `+0xC` as 8-byte
//! `[i16 dx][i16 dy][i16 dz][pad]` triples. That body is the same shape the
//! VDF morph stager `FUN_8001C604` walks
//! ([`legaia_engine_vm::vdf_morph::VdfMorphRecord`]), which is why this
//! module reuses that record type rather than declaring its own.
//!
//! The **cursor advance** is not the body length. Both passes step the
//! cursor by `group.n_vert * 0x60` (`0x800217B4..0x800217C4`, and again at
//! `0x80021860..0x8002187C`) - a fixed slot sized off the *object's* vertex
//! count, not off `delta_count`. Since `0xC + delta_count*8` is bounded by
//! `0xC + n_vert*8`, the slot always contains its record with room to spare;
//! it is a fixed-pitch slab, not a packed stream. Both passes use the same
//! expression, so they stay in phase whatever the slack means. This is
//! disassembly-grounded and deliberately not rationalised further: what the
//! remaining `0x60`-per-vertex reservation is for is not established.
//!
//! ## The weight envelope
//!
//! The tail (`0x80021880..0x8002190C`) is a ping-pong ramp, not a one-shot:
//! `actor+0x6E` moves by `actor[+0x3C] * DAT_1F800393` while the direction
//! halfword `actor+0x40` is zero and by `actor[+0x3E] * DAT_1F800393` while
//! it is not, and each clamp *flips the direction* - underflow sets weight
//! `0` and direction `0` (rising), overflow sets weight `0x1000` and
//! direction `1` (falling). So an actor left alone oscillates between the
//! rest pose and the full morph at two independent rates.
//!
//! ## NOT WIRED
//!
//! The engine's [`crate::world::Actor`] has no morph-block pointer
//! (`actor+0x4C`) and no rest-pose stream (`actor+0x90`): the only morph
//! path it carries is the *other* one - per-group VDF staging through
//! [`crate::world::World::stage_actor_group_morph`], which resolves records
//! from the scene VDF table and ramps its weights in the move-VM envelope
//! (`FUN_80020740`), not from this actor's `+0x3C/+0x3E/+0x40` triple. What
//! has to exist first is an actor whose morph set is a *block* rather than a
//! slot list, and the spawn site that allocates from descriptor
//! `0x8007068C` to fill it; until then there is no live block to walk.
//! [`crate::actor_handler::ActorHandler::MorphWeights`] already carries the
//! identity, so wiring is one dispatch arm once a producer exists.

pub use legaia_engine_vm::vdf_morph::VdfMorphRecord;
use legaia_engine_vm::vdf_morph::apply_weighted_deltas;

/// Bytes an 8-byte GTE vertex occupies.
pub const VERTEX_BYTES: usize = 8;

/// Bytes of morph-block slot reserved per vertex of the record's TMD group.
/// The cursor advance in both passes, `n_vert * 0x60`.
pub const SLOT_BYTES_PER_VERTEX: usize = 0x60;

/// Record header: `[u32 group_id][u32 first_vertex][u32 delta_count]`.
pub const RECORD_HEADER_BYTES: usize = 12;

/// The weight at which a record contributes its authored delta in full.
/// `FUN_8005B038` computes `(weight * delta) >> 12`.
pub const WEIGHT_FULL: i16 = 0x1000;

/// Bytes one record occupies in the morph block, given its group's vertex
/// count.
pub fn slot_span(group_vertex_count: usize) -> usize {
    group_vertex_count * SLOT_BYTES_PER_VERTEX
}

/// Walk the morph block at `actor+0x4C`.
///
/// `group_vertex_counts[g]` is object `g`'s `n_vert` from the TMD object
/// table - the value that sizes each slot. A record naming a group outside
/// that table ends the walk, matching the retail loop's inability to advance
/// past an unresolvable stride.
pub fn parse_apply_records<'a>(
    block: &'a [u8],
    group_vertex_counts: &[usize],
) -> Vec<VdfMorphRecord<'a>> {
    let mut out = Vec::new();
    if block.len() < 4 {
        return out;
    }
    let count = u32::from_le_bytes(block[0..4].try_into().unwrap());
    let mut off = 4usize;
    for _ in 0..count {
        if off + RECORD_HEADER_BYTES > block.len() {
            break;
        }
        let group_id = u32::from_le_bytes(block[off..off + 4].try_into().unwrap());
        let dst_index = u32::from_le_bytes(block[off + 4..off + 8].try_into().unwrap());
        let delta_count = u32::from_le_bytes(block[off + 8..off + 12].try_into().unwrap()) as usize;
        let Some(&n_vert) = group_vertex_counts.get(group_id as usize) else {
            break;
        };
        let body = off + RECORD_HEADER_BYTES;
        let end = body
            .saturating_add(delta_count * VERTEX_BYTES)
            .min(block.len());
        out.push(VdfMorphRecord {
            group_id,
            dst_index,
            deltas: &block[body..end],
        });
        // The slab stride - the object's vertex count, not the record's.
        off = off.saturating_add(slot_span(n_vert));
    }
    out
}

/// One apply pass: restore every named group's rest pose, then blend every
/// record's deltas at `weight`.
///
/// `groups[g]` is object `g`'s live vertex buffer (8-byte GTE vertices), the
/// engine's stand-in for the object table's `+0` pointer; its length gives
/// the `n_vert` retail reads at `+4`. `rest_pose` is the `actor+0x90`
/// stream, read with one cursor across all records in record order.
///
/// Returns the number of records walked.
///
/// PORT: FUN_8002174C
///
/// NOT WIRED: no engine actor carries a morph block (`actor+0x4C`) or a
/// rest-pose stream (`actor+0x90`) - the ported morph path is the per-group
/// VDF slot list instead. A spawn site allocating from descriptor
/// `0x8007068C` is the prerequisite; see the module docs.
pub fn apply_morph_weights(
    block: &[u8],
    groups: &mut [Vec<u8>],
    rest_pose: &[u8],
    weight: i16,
) -> usize {
    let counts: Vec<usize> = groups.iter().map(|g| g.len() / VERTEX_BYTES).collect();
    let records = parse_apply_records(block, &counts);

    // Pass one: rest-pose restore, one continuous cursor into `actor+0x90`.
    let mut src = 0usize;
    for rec in &records {
        let Some(group) = groups.get_mut(rec.group_id as usize) else {
            continue;
        };
        let want = group.len();
        let available = rest_pose.len().saturating_sub(src).min(want);
        group[..available].copy_from_slice(&rest_pose[src..src + available]);
        src = src.saturating_add(want);
    }

    // Pass two: weighted deltas over the restored pose.
    for rec in &records {
        let Some(group) = groups.get_mut(rec.group_id as usize) else {
            continue;
        };
        apply_weighted_deltas(group, rec.dst_index as usize, rec, weight);
    }

    records.len()
}

/// The ping-pong weight ramp in the tail of `FUN_8002174C`.
///
/// `weight` is `actor+0x6E`, `up_rate` `actor+0x3C`, `down_rate`
/// `actor+0x3E` and `descending` the direction halfword `actor+0x40`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MorphWeightEnvelope {
    /// `actor+0x6E` - the live blend weight, `0..=0x1000`.
    pub weight: i16,
    /// `actor+0x3C` - units of weight per frame while rising.
    pub up_rate: i16,
    /// `actor+0x3E` - units of weight per frame while falling.
    pub down_rate: i16,
    /// `actor+0x40` - non-zero selects the falling rate.
    pub descending: bool,
}

impl MorphWeightEnvelope {
    /// Advance one frame. `dt` is `DAT_1F800393`, the adaptive frame-skip
    /// factor, read as an unsigned byte and multiplied by the signed rate.
    ///
    /// PORT: FUN_8002174C
    ///
    /// NOT WIRED: shares the apply pass's gap - nothing spawns the actor
    /// whose `+0x3C/+0x3E/+0x40` triple this ramps. See the module docs.
    pub fn tick(&mut self, dt: u8) {
        let dt = i32::from(dt);
        // `lhu` the weight, add/subtract the 32-bit product, `sh` it back:
        // 16-bit wrap, which is how the low clamp is ever reached at all.
        let step = if self.descending {
            i32::from(self.down_rate) * dt
        } else {
            i32::from(self.up_rate) * dt
        };
        let acc = if self.descending {
            (self.weight as u16).wrapping_sub(step as u16)
        } else {
            (self.weight as u16).wrapping_add(step as u16)
        };
        self.weight = acc as i16;
        if self.weight < 0 {
            self.weight = 0;
            self.descending = false;
        }
        if self.weight > WEIGHT_FULL {
            self.weight = WEIGHT_FULL;
            self.descending = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: i16, y: i16, z: i16) -> [u8; VERTEX_BYTES] {
        let mut v = [0u8; VERTEX_BYTES];
        v[0..2].copy_from_slice(&x.to_le_bytes());
        v[2..4].copy_from_slice(&y.to_le_bytes());
        v[4..6].copy_from_slice(&z.to_le_bytes());
        v
    }

    fn read(buf: &[u8], i: usize) -> (i16, i16, i16) {
        let o = i * VERTEX_BYTES;
        (
            i16::from_le_bytes([buf[o], buf[o + 1]]),
            i16::from_le_bytes([buf[o + 2], buf[o + 3]]),
            i16::from_le_bytes([buf[o + 4], buf[o + 5]]),
        )
    }

    /// `(group_id, first_vertex, deltas)` - one authored record.
    type TestRecord = (u32, u32, Vec<(i16, i16, i16)>);

    /// Build a morph block whose records sit at the retail slab pitch.
    fn block(records: &[TestRecord], counts: &[usize]) -> Vec<u8> {
        let mut out = (records.len() as u32).to_le_bytes().to_vec();
        for (gid, first, deltas) in records {
            let slot_start = out.len();
            out.extend_from_slice(&gid.to_le_bytes());
            out.extend_from_slice(&first.to_le_bytes());
            out.extend_from_slice(&(deltas.len() as u32).to_le_bytes());
            for (dx, dy, dz) in deltas {
                out.extend_from_slice(&vertex(*dx, *dy, *dz));
            }
            out.resize(slot_start + slot_span(counts[*gid as usize]), 0);
        }
        out
    }

    fn scene() -> (Vec<u8>, Vec<Vec<u8>>, Vec<usize>) {
        // Two TMD groups: 3 vertices and 2 vertices.
        let counts = vec![3usize, 2];
        let rest: Vec<u8> = [
            vertex(10, 20, 30),
            vertex(11, 21, 31),
            vertex(12, 22, 32),
            vertex(100, 200, 300),
            vertex(101, 201, 301),
        ]
        .concat();
        let groups = counts.iter().map(|n| vec![0u8; n * VERTEX_BYTES]).collect();
        (rest, groups, counts)
    }

    /// The property the routine exists to hold: at weight `0` an apply pass
    /// is exactly a rest-pose restore, whatever the records say. Anything
    /// that leaked a delta through - a mis-scaled blend, a fused pass, a
    /// stale buffer - shows up here.
    #[test]
    fn weight_zero_reproduces_the_rest_pose_exactly() {
        let (rest, mut groups, counts) = scene();
        let blk = block(
            &[
                (0, 0, vec![(1000, -1000, 500), (7, 8, 9), (-1, -2, -3)]),
                (1, 0, vec![(-4000, 4000, 0), (1, 1, 1)]),
            ],
            &counts,
        );
        assert_eq!(apply_morph_weights(&blk, &mut groups, &rest, 0), 2);
        assert_eq!(groups.concat(), rest);
    }

    /// And at `0x1000` it is the rest pose plus the authored delta, one for
    /// one - the other end of the blend kernel's `(weight * delta) >> 12`.
    #[test]
    fn full_weight_reproduces_the_authored_delta_exactly() {
        let (rest, mut groups, counts) = scene();
        let deltas_a = vec![(1000i16, -1000i16, 500i16), (7, 8, 9), (-1, -2, -3)];
        let deltas_b = vec![(-4000i16, 4000i16, 0i16), (1, 1, 1)];
        let blk = block(
            &[(0, 0, deltas_a.clone()), (1, 0, deltas_b.clone())],
            &counts,
        );
        apply_morph_weights(&blk, &mut groups, &rest, WEIGHT_FULL);

        for (i, (dx, dy, dz)) in deltas_a.iter().enumerate() {
            let base = read(&rest, i);
            assert_eq!(read(&groups[0], i), (base.0 + dx, base.1 + dy, base.2 + dz));
        }
        for (i, (dx, dy, dz)) in deltas_b.iter().enumerate() {
            let base = read(&rest, 3 + i);
            assert_eq!(read(&groups[1], i), (base.0 + dx, base.1 + dy, base.2 + dz));
        }
    }

    /// Half weight is half the delta, truncated by the GTE's `>> 12` - so a
    /// delta of `2` at weight `0x800` contributes `1`, and a delta of `1`
    /// contributes nothing at all.
    #[test]
    fn half_weight_truncates_toward_zero() {
        let counts = vec![1usize];
        let rest = vertex(0, 0, 0).to_vec();
        let mut groups = vec![vec![0u8; VERTEX_BYTES]];
        let blk = block(&[(0, 0, vec![(2, 1, 4096)])], &counts);
        apply_morph_weights(&blk, &mut groups, &rest, 0x800);
        assert_eq!(read(&groups[0], 0), (1, 0, 2048));
    }

    /// Why the two passes are separate: a fused loop would restore group 0's
    /// rest pose a second time when it reached the second record, discarding
    /// the first record's contribution.
    #[test]
    fn two_records_on_one_group_both_survive() {
        let counts = vec![2usize];
        let rest: Vec<u8> = [vertex(0, 0, 0), vertex(0, 0, 0)].concat();
        let mut groups = vec![vec![0u8; 2 * VERTEX_BYTES]];
        let blk = block(
            &[(0, 0, vec![(100, 0, 0)]), (0, 1, vec![(0, 200, 0)])],
            &counts,
        );
        assert_eq!(
            apply_morph_weights(&blk, &mut groups, &rest, WEIGHT_FULL),
            2
        );
        assert_eq!(read(&groups[0], 0), (100, 0, 0));
        assert_eq!(read(&groups[0], 1), (0, 200, 0));
    }

    /// The `actor+0x90` cursor is shared: record `k` reads the rest pose that
    /// follows every earlier record's group, so a block naming groups out of
    /// table order still restores each one from its own stream slice.
    #[test]
    fn the_rest_pose_cursor_runs_across_records_in_record_order() {
        let counts = vec![3usize, 2];
        // Group 1 named first, so it must take the stream's first two
        // vertices even though it is the second entry in the object table.
        let rest: Vec<u8> = [
            vertex(1, 1, 1),
            vertex(2, 2, 2),
            vertex(3, 3, 3),
            vertex(4, 4, 4),
            vertex(5, 5, 5),
        ]
        .concat();
        let mut groups = vec![vec![0u8; 3 * VERTEX_BYTES], vec![0u8; 2 * VERTEX_BYTES]];
        let blk = block(&[(1, 0, vec![]), (0, 0, vec![])], &counts);
        apply_morph_weights(&blk, &mut groups, &rest, WEIGHT_FULL);
        assert_eq!(read(&groups[1], 0), (1, 1, 1));
        assert_eq!(read(&groups[1], 1), (2, 2, 2));
        assert_eq!(read(&groups[0], 0), (3, 3, 3));
        assert_eq!(read(&groups[0], 2), (5, 5, 5));
    }

    /// A record's deltas land at its `first_vertex`, leaving earlier vertices
    /// at the rest pose.
    #[test]
    fn deltas_apply_from_the_records_first_vertex() {
        let (rest, mut groups, counts) = scene();
        let blk = block(&[(0, 2, vec![(50, 60, 70)])], &counts);
        apply_morph_weights(&blk, &mut groups, &rest, WEIGHT_FULL);
        assert_eq!(read(&groups[0], 0), (10, 20, 30));
        assert_eq!(read(&groups[0], 1), (11, 21, 31));
        assert_eq!(read(&groups[0], 2), (12 + 50, 22 + 60, 32 + 70));
    }

    /// The envelope is a ping-pong, not a one-shot: it reaches both rails and
    /// turns around at each, and it never leaves `0..=0x1000` after a tick.
    #[test]
    fn the_weight_envelope_ping_pongs_between_the_rails() {
        let mut env = MorphWeightEnvelope {
            weight: 0,
            up_rate: 300,
            down_rate: 700,
            descending: false,
        };
        let mut turns = 0;
        let mut was_descending = env.descending;
        for _ in 0..400 {
            env.tick(2);
            assert!((0..=WEIGHT_FULL).contains(&env.weight), "{env:?}");
            if env.descending != was_descending {
                // Every turn happens *at* a rail, never mid-travel.
                let rail = if env.descending { WEIGHT_FULL } else { 0 };
                assert_eq!(env.weight, rail, "turned away from a rail: {env:?}");
                turns += 1;
                was_descending = env.descending;
            }
        }
        assert!(turns > 4, "only {turns} turns in 400 frames");
    }

    /// The two rates are independent - a fast rise with a slow fall spends
    /// most of its cycle descending. The fall also shows the low rail is
    /// reached by **underflow**, not by landing on zero: an exact `0` is
    /// still descending, and it takes one more frame to turn around.
    #[test]
    fn the_two_rates_are_independent_and_zero_is_not_the_turn() {
        let mut env = MorphWeightEnvelope {
            weight: 0,
            up_rate: 0x2000,
            down_rate: 0x100,
            descending: false,
        };
        env.tick(1);
        assert_eq!(
            env.weight, WEIGHT_FULL,
            "one frame of a 0x2000 rate tops out"
        );
        assert!(env.descending);

        // 0x1000 / 0x100 = 16 frames to land exactly on zero...
        for _ in 0..16 {
            env.tick(1);
        }
        assert_eq!(env.weight, 0);
        assert!(env.descending, "an exact zero is not the turn");
        // ...and one more to underflow and turn.
        env.tick(1);
        assert_eq!(env.weight, 0);
        assert!(!env.descending);
    }

    /// A zero rate parks the weight where it is instead of drifting.
    #[test]
    fn a_zero_rate_holds_the_weight() {
        let mut env = MorphWeightEnvelope {
            weight: 0x400,
            ..Default::default()
        };
        for _ in 0..8 {
            env.tick(3);
        }
        assert_eq!(env.weight, 0x400);
    }

    /// The slab pitch, not the delta length, is what advances the cursor: a
    /// record with no deltas at all still consumes its group's whole slot,
    /// so the record after it parses correctly.
    #[test]
    fn the_cursor_steps_by_the_slab_pitch_not_the_delta_length() {
        let counts = vec![4usize, 1];
        let blk = block(&[(0, 0, vec![]), (1, 0, vec![(9, 9, 9)])], &counts);
        assert_eq!(blk.len(), 4 + slot_span(4) + slot_span(1));
        let recs = parse_apply_records(&blk, &counts);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].group_id, 0);
        assert!(recs[0].is_empty());
        assert_eq!(recs[1].group_id, 1);
        assert_eq!(recs[1].len(), 1);
        assert_eq!(recs[1].delta(0), (9, 9, 9));
    }

    /// A record naming a group the object table does not have cannot be
    /// stepped past - there is no stride for it - so the walk stops there
    /// rather than desynchronising.
    #[test]
    fn an_out_of_table_group_ends_the_walk() {
        let counts = vec![1usize];
        let mut blk = 2u32.to_le_bytes().to_vec();
        blk.extend_from_slice(&0u32.to_le_bytes());
        blk.extend_from_slice(&0u32.to_le_bytes());
        blk.extend_from_slice(&0u32.to_le_bytes());
        blk.resize(4 + slot_span(1), 0);
        blk.extend_from_slice(&9u32.to_le_bytes()); // group 9: not in the table
        blk.extend_from_slice(&0u32.to_le_bytes());
        blk.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse_apply_records(&blk, &counts).len(), 1);
    }
}
