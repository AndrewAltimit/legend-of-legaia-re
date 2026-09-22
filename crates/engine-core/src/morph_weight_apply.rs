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
//! The **cursor advance** is not the body length. Both passes of this routine
//! step the cursor by `group.n_vert * 0x60` (`0x800217B4..0x800217C4`, and
//! again at `0x80021860..0x8002187C`) - a slot sized off the *object's*
//! vertex count, not off `delta_count`. Since `0xC + delta_count*8` is
//! bounded by `0xC + n_vert*8`, the slot always contains its record with room
//! to spare. Both passes use the same expression, so they stay in phase
//! whatever the slack means.
//!
//! What that pitch is **not** is a property of the buffer. The spawner walks
//! the same block twice more with two further strides - `0xC` for its size
//! sum and `n_vert * 8` for its copy - so three loops over one buffer
//! disagree about where record `n + 1` begins, and only a block of a single
//! record makes them agree (no stride is consumed at all). Every block the
//! disc ships is exactly that: see [`rest_pose_snapshot`] for the census.
//! So the `0x60` reservation is real in the instruction stream and
//! unobservable in the shipped game, and nothing here rationalises it
//! further.
//!
//! ## The retail spawn chain, end to end
//!
//! The row's prerequisite used to read as "a spawn site allocating from
//! descriptor `0x8007068C`", which understates what is already known: that
//! site exists, it is shipped content, and the engine already hosts it.
//!
//! | Link | Where |
//! |---|---|
//! | Field-VM instruction `4C D8` | outer `0x4C` table `0x801CEE60[0xD]` -> `0x801E2AB8`, sub-table `0x801CEFC8[8]` -> arm `0x801E2DD4` |
//! | The arm's call | `jal 0x801D77F4` at `0x801E2E18`, PC += 9 |
//! | The spawner | `FUN_801D77F4` (PROT 0897 file `+0x8FDC`) forms `0x8007068C` at `0x801D7808`/`0x801D7814` and calls `FUN_80020DE0` |
//! | The handler | `0x8007068C + 8` reads `0x8002174C` straight off `SCUS_942.54` |
//!
//! The opcode ships at **17 sites in 5 scenes** (`balden`, `balden2`,
//! `garmel`, `jagaroom`, `juui2`), all in partition 1 record 0 of a scene MAN;
//! the census lives in `engine-core/tests/field_actor_spawn_disc_e2e.rs` and
//! is written up in
//! [`script-vm-menuctrl.md`](../../../docs/subsystems/script-vm-menuctrl.md#where-0x4c-0xd8-occurs-on-the-disc).
//! `FieldHost::op4c_n_d_sub8_call_d77f4` is the live engine hook for it.
//!
//! ### The two buffers, and what fills them
//!
//! `FUN_801D77F4`'s tail (`0x801D7848..0x801D79BC`) is the part the port used
//! to stop short of; it is now [`crate::world::World::spawn_morph_weight_actor`]
//! and [`rest_pose_snapshot`]. The writes themselves are in the function
//! directory
//! ([`functions/renderer.md` § 801D77F4](../../../docs/reference/functions/renderer.md#801d77f4));
//! what this section adds is which of them this module's two arguments are:
//!
//! - `actor+0x4C` <- the **morph block**, and it is a **VDF** body: the
//!   instruction's first operand indexes the VDF buffer at the global
//!   `0x8007B7DC` (asset-dispatcher case 7), `block = base + u32_at(base + 4 +
//!   idx*4)`, and the block opens with its own `u32` record count followed by
//!   the 12-byte record headers this module parses. That is why the record
//!   type here is [`legaia_engine_vm::vdf_morph::VdfMorphRecord`] and not a
//!   shape of its own - one buffer, two walkers.
//! - `actor+0x48` <- the TMD base, read from the resident-object table at
//!   `0x8007C018` by the instruction's second operand (the same table
//!   `FUN_801D8280` walks).
//! - `actor+0x90` <- the **rest-pose stream**, and it is a *snapshot*, not an
//!   asset. The spawner sums `n_vert` over the block's records through the
//!   object table's `0x1C` stride, allocates `sum * 8` bytes through
//!   `FUN_80017888`, and copies each named group's live vertices into it
//!   with unaligned `lwl`/`lwr` + `swl`/`swr` pairs. So the rest pose is
//!   whatever the mesh holds at spawn time - which is why nothing on the disc
//!   carries one.
//! - `actor+0x3C` / `actor+0x3E` <- the instruction's two `u16` immediates,
//!   straight through. The port's field-VM host calls them `kind` and
//!   `variant`; `FUN_8002174C` reads them as this module's `up_rate` and
//!   `down_rate`, so for this spawn they are the envelope's rise and fall
//!   rates and nothing else.
//! - `actor+0x56` (render mode), `+0x68` and `+0x6E` (the live weight) are
//!   all zeroed, so a freshly spawned morph actor starts at rest.
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
//! ## Where the engine runs it
//!
//! The seat is [`crate::world::World::spawn_morph_weight_actor`], which the
//! `4C D8` host arm calls in place of the plain allocator: it builds the
//! snapshot, stamps
//! [`crate::actor_handler::ActorHandler::MorphWeights`] and seats a
//! [`MorphWeightActor`] on the slot. [`crate::world::World::tick_handler_actors`]
//! steps the envelope once per game tick, and both hosts read the blended
//! mesh back through [`crate::world::World::morph_weight_posed_tmd`] - the
//! one place [`apply_morph_weights`] runs, so neither renderer owns any part
//! of the blend.
//!
//! This is a different path from the engine's *other* morph route - per-group
//! VDF staging through [`crate::world::World::stage_actor_group_morph`],
//! which resolves records from the scene VDF table and ramps its weights in
//! the move-VM envelope (`FUN_80020740`). One buffer shape, two producers:
//! that one takes a slot list, this one a block.

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
/// Reached on both hosts through [`crate::world::World::morph_weight_posed_tmd`],
/// the render-time read each one poses a `4C D8` actor's mesh with.
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
    /// Stepped once per game tick by
    /// [`crate::world::World::tick_handler_actors`] over every actor the
    /// `4C D8` allocator seated.
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

/// Everything `FUN_801D77F4` installs on the slot it allocates for a morph
/// actor, held as owned bytes because the engine's pool owns no retail heap:
/// the `actor+0x4C` block, the `actor+0x90` snapshot the spawner builds, and
/// the `+0x3C` / `+0x3E` / `+0x40` / `+0x6E` envelope quad.
///
/// The engine seats one of these from
/// [`crate::world::World::spawn_morph_weight_actor`] and steps it from
/// [`crate::world::World::tick_handler_actors`]; hosts read the blended
/// vertex set back through
/// [`crate::world::World::morph_weight_actor_groups`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MorphWeightActor {
    /// `actor+0x4C` - the VDF morph block the instruction's first operand
    /// resolved to (`0x8007B7DC` + the `+4` offset word).
    pub block: Vec<u8>,
    /// `actor+0x90` - the rest pose, captured from the live mesh at spawn.
    pub rest_pose: Vec<u8>,
    /// `+0x3C` / `+0x3E` / `+0x40` / `+0x6E`.
    pub envelope: MorphWeightEnvelope,
}

/// Build the `actor+0x90` rest-pose snapshot the spawner allocates and fills
/// (`0x801D78B8..0x801D799C`).
///
/// `groups[g]` is object `g`'s **live** vertex buffer as 8-byte GTE vertices:
/// retail reads the object-table entry's `+0` pointer, so what lands in the
/// snapshot is whatever the mesh holds at spawn time, which is why nothing on
/// the disc carries a rest pose.
///
/// PORT: FUN_801D77F4
///
/// ## Three record pitches, one buffer
///
/// Retail walks this block three times with three different record strides,
/// and the port reproduces each loop's own:
///
/// | loop | stride |
/// |---|---|
/// | the spawner's size sum (`0x801D78D0..0x801D7900`) | `0xC` - the record header |
/// | the spawner's copy pass (`0x801D792C..0x801D799C`) | `n_vert * 8` |
/// | the apply pass ([`apply_morph_weights`], both halves) | `n_vert * 0x60` |
///
/// They can only agree on a block of **one** record, where no stride is ever
/// consumed - and that is every block the disc ships: all seventeen `4C D8`
/// sites across `balden` / `balden2` / `garmel` / `jagaroom` / `juui2`
/// resolve to a block whose leading count word is `1`, each naming group `0`
/// (the census is `morph_weight_disc_blocks_are_single_record` in
/// `engine-core/tests/field_actor_spawn_disc_e2e.rs`). So the pitch
/// disagreement is real in the bytes and unobservable in the shipped game;
/// no reading of it is load-bearing, and none is invented here.
pub fn rest_pose_snapshot(block: &[u8], groups: &[&[u8]]) -> Vec<u8> {
    let vertex_count = |gid: u32| -> usize {
        groups
            .get(gid as usize)
            .map(|g| g.len() / VERTEX_BYTES)
            .unwrap_or(0)
    };
    let group_at = |off: usize| -> Option<u32> {
        if off + 4 > block.len() {
            return None;
        }
        Some(u32::from_le_bytes(block[off..off + 4].try_into().unwrap()))
    };
    if block.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_le_bytes(block[0..4].try_into().unwrap());

    // Sum pass: `0xC` per record.
    let mut total = 0usize;
    let mut off = 4usize;
    for _ in 0..count {
        let Some(gid) = group_at(off) else { break };
        total = total.saturating_add(vertex_count(gid) * VERTEX_BYTES);
        off = off.saturating_add(RECORD_HEADER_BYTES);
    }

    // Copy pass: `n_vert * 8` per record, one continuous destination cursor.
    let mut out = vec![0u8; total];
    let mut off = 4usize;
    let mut dst = 0usize;
    for _ in 0..count {
        let Some(gid) = group_at(off) else { break };
        let n = vertex_count(gid);
        if let Some(src) = groups.get(gid as usize) {
            let take = (n * VERTEX_BYTES).min(src.len()).min(out.len() - dst);
            out[dst..dst + take].copy_from_slice(&src[..take]);
            dst = dst.saturating_add(take);
        }
        off = off.saturating_add(n * VERTEX_BYTES);
    }
    out
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
