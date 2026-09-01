//! Scene-entry VDF pulse - the **enhancement** arm of the vertex-morph
//! render substitution.
//!
//! Retail arms VDF morph lanes through the ambient move-VM tree: a
//! prescript part with a mesh (`model_sel = pack_slot + 5`) runs op `0x0A`
//! and the ramp envelope (`FUN_80020740`) drives the lane weights the
//! render substitution (`FUN_8001C604`) reads back. town0e and
//! rikuroa/rikuroa2 do exactly that at plain scene entry.
//!
//! jou does **not**: its 17-sub-entry VDF pack (the fused-Juggernaut
//! flesh-ground deltas) is armed only by cutscene installs (op `0x1F` in
//! prescript record 13, P2 chains). This module is the engine's
//! scene-entry stand-in for those set pieces: when a scene's VDF pack is
//! populated but its entry-ambient tree armed nothing, the host installs a
//! rolling pulse - one envelope lane per sub-entry, cascading up and back
//! down forever (`LANE0_SNAP_DOWN` recycle), each lane weighting its
//! sub-entry's deltas onto the pack meshes it targets.
//!
//! The delta *arithmetic* is the retail kernel chain
//! (`legaia_engine_vm::vdf_morph` + `move_buffer::envelope_tick` -
//! Confirmed from `8001c604.txt` / `8005b038.txt` / `80020740.txt`); the
//! *arming* (which lanes, which velocities, at scene entry) is the
//! engine's own and is documented as an enhancement in
//! `docs/subsystems/field-ambient-fx.md`. Sub-entry -> mesh targeting is
//! inferred by the exact-fit rule below; retail binds the pack slot
//! explicitly on the armed part instead.
//!
//! REF: FUN_8001C604, FUN_80020740

use legaia_engine_vm::move_buffer::{self, MoveBufferState, env_flag};
use legaia_engine_vm::vdf_morph;

/// Per-lane ramp velocity (raw envelope units per tick unit). `0x1000 /
/// 0x140 = ~13` envelope steps per rise; with the field `frame_step = 2`
/// delta that is ~0.2 s per lane, a full 17-lane jou wave ~7 s.
const PULSE_VELOCITY: i16 = 0x140;

/// The rolling scene-entry pulse over a populated VDF pack.
#[derive(Debug, Clone)]
pub struct EntryVdfPulse {
    /// One envelope lane per VDF sub-entry (capped at the envelope's 32).
    env: MoveBufferState,
    /// Per sub-entry: the `(pack_slot, group)` pairs its records fit.
    targets: Vec<Vec<(usize, u32)>>,
    /// Weights snapshot from the previous tick (change detection).
    prev: Vec<u16>,
}

/// A parsed view of one sub-entry's records: `(group_id, dst, count)`.
fn record_shapes(entry: &[u8]) -> Vec<(u32, usize, usize)> {
    vdf_morph::parse_vdf_morph_records(entry)
        .iter()
        .map(|r| (r.group_id, r.dst_index as usize, r.len()))
        .collect()
}

impl EntryVdfPulse {
    /// Build the pulse over `sub_entries` (the scene VDF pack, one slice
    /// per sub-entry - see `World::vdf_record_bytes`) against
    /// `pack_objects[pack_slot][group] = vertex_count`.
    ///
    /// Targeting is the **exact-fit** rule: a record fits a pack mesh when
    /// its group id is a real object of that mesh and `dst + count` lands
    /// exactly on the object's vertex count - every authored sub-entry in
    /// the corpus ends exactly at its target's `n_vert`, which is what
    /// makes the fit discriminating. A sub-entry targets every mesh all of
    /// its records fit (same-geometry pack twins morph together; retail
    /// disambiguates by the armed part's `model_sel`, which a plain scene
    /// entry never sets for these packs).
    ///
    /// Returns `None` when nothing fits (no pack, empty pack, or no
    /// matching meshes).
    pub fn build(sub_entries: &[&[u8]], pack_objects: &[Vec<usize>]) -> Option<Self> {
        let count = sub_entries.len().min(move_buffer::MAX_BONES);
        if count == 0 {
            return None;
        }
        let mut targets = Vec::with_capacity(count);
        let mut any = false;
        for entry in sub_entries.iter().take(count) {
            let shapes = record_shapes(entry);
            let mut fits = Vec::new();
            if !shapes.is_empty() {
                for (slot, objs) in pack_objects.iter().enumerate() {
                    let all_fit = shapes
                        .iter()
                        .all(|&(g, dst, n)| objs.get(g as usize).is_some_and(|&nv| dst + n == nv));
                    if all_fit {
                        for &(g, _, _) in &shapes {
                            if !fits.contains(&(slot, g)) {
                                fits.push((slot, g));
                            }
                        }
                    }
                }
            }
            any |= !fits.is_empty();
            targets.push(fits);
        }
        if !any {
            return None;
        }
        let mut env = MoveBufferState {
            bone_count: count as u8,
            env_flags: env_flag::LANE0_SNAP_DOWN,
            ..Default::default()
        };
        env.set_uniform_up_velocity(PULSE_VELOCITY);
        env.set_uniform_down_velocity(PULSE_VELOCITY);
        Some(Self {
            env,
            targets,
            prev: vec![0; count],
        })
    }

    /// Advance the envelope one ambient game tick. Returns the
    /// `(pack_slot, group)` pairs whose blended weight changed.
    pub fn tick(&mut self, frame_delta: u8) -> Vec<(usize, u32)> {
        move_buffer::envelope_tick(&mut self.env, frame_delta);
        let mut dirty = Vec::new();
        for lane in 0..self.env.bone_count as usize {
            let w = self.env.lanes[lane];
            if w != self.prev[lane] {
                self.prev[lane] = w;
                for &t in &self.targets[lane] {
                    if !dirty.contains(&t) {
                        dirty.push(t);
                    }
                }
            }
        }
        dirty
    }

    /// The `(sub_entry_index, weight)` lanes currently targeting
    /// `(pack_slot, group)` - the same shape as a retail part's armed
    /// morph lanes, ready for `vdf_morph::sum_group_deltas`.
    pub fn lanes_for(&self, pack_slot: usize, group: u32) -> Vec<(u8, u16)> {
        (0..self.env.bone_count as usize)
            .filter(|&lane| self.targets[lane].contains(&(pack_slot, group)))
            .map(|lane| (lane as u8, self.env.lanes[lane]))
            .collect()
    }

    /// The full envelope phase - lane weights plus the ramp-direction state -
    /// as a comparable value. Two equal fingerprints mean the pulse is at the
    /// same point of its cycle, which is how a sampler (the glb export's
    /// morph-weight bake) finds the exact loop period.
    pub fn phase_fingerprint(&self) -> (Vec<u16>, u32, u16) {
        (
            self.env.lanes[..self.env.bone_count as usize].to_vec(),
            self.env.done_mask,
            self.env.env_flags,
        )
    }

    /// Every `(pack_slot, group)` any lane targets.
    pub fn all_targets(&self) -> Vec<(usize, u32)> {
        let mut out = Vec::new();
        for t in &self.targets {
            for &p in t {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }
}

/// True when any stager record's move-VM bytecode reaches op `0x0A`
/// (KEYFRAME_LOAD - the retail morph-lane arm) on a linear walk. The pulse
/// installer uses this to stand aside for scenes where retail owns the
/// morphing in *some* story state even if nothing is armed right now
/// (rikuroa's records 69/70 sit behind system-flag gates 0x281/0x282).
///
/// The walk mirrors the `FUN_80023070` dispatch advance table
/// (decode-only; conditional ext branches are walked fall-through +
/// taken is not followed, which for an existence scan is sufficient: the
/// op stream is linear and `0x0A` sits in straight-line record bodies in
/// the corpus).
pub fn stager_records_arm_morphs(
    records: &[legaia_asset::summon_overlay::SummonPart],
    bytes: &[u8],
) -> bool {
    for rec in records {
        let Some(slice) = bytes.get(rec.record_off..rec.bytecode.end.min(bytes.len())) else {
            continue;
        };
        let buf: Vec<u16> = slice
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let mut pc = 2usize;
        for _ in 0..1024 {
            let Some(&op) = buf.get(pc) else { break };
            if op == 0x0A {
                return true;
            }
            let rd = |i: usize| buf.get(pc + i).copied().unwrap_or(0);
            let size: usize = match op {
                0x00 | 0x01 | 0x04 | 0x05 | 0x07 | 0x2B | 0x2D | 0x2E | 0x39 | 0x44 | 0x46 => 4,
                0x02
                | 0x03
                | 0x06
                | 0x09
                | 0x0D..=0x12
                | 0x15..=0x18
                | 0x1A
                | 0x1C
                | 0x1D
                | 0x25
                | 0x28..=0x2A
                | 0x31
                | 0x32
                | 0x38
                | 0x3E
                | 0x3F
                | 0x41 => 2,
                0x0B | 0x19 | 0x1B | 0x22 | 0x30 | 0x33 | 0x3A | 0x3B | 0x43 => 1,
                0x0C => 6,
                0x13 => 0x10,
                0x14 | 0x26 | 0x2C => 5,
                0x1E | 0x1F | 0x45 => 8,
                0x20 | 0x24 | 0x27 | 0x35..=0x37 => 3,
                0x21 | 0x40 => 7,
                0x23 => 0xD,
                0x2F => legaia_engine_vm::move_vm_overlay_ext::canonical_size(rd(1)).unwrap_or(1)
                    as usize,
                0x34 => 9,
                0x3C => 2 + rd(1) as usize * 6,
                0x3D => 3 + rd(2) as usize * 6,
                0x42 => 0xF,
                _ => break, // HALT (0x08) or out-of-range
            };
            pc += size;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[count=1][group][dst][n][n x 8-byte deltas]`.
    fn sub_entry(group: u32, dst: u32, n: u32) -> Vec<u8> {
        let mut b = 1u32.to_le_bytes().to_vec();
        b.extend_from_slice(&group.to_le_bytes());
        b.extend_from_slice(&dst.to_le_bytes());
        b.extend_from_slice(&n.to_le_bytes());
        for i in 0..n {
            b.extend_from_slice(&(i as i16 + 1).to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
            b.extend_from_slice(&[0, 0]);
        }
        b
    }

    #[test]
    fn exact_fit_targets_matching_meshes_only() {
        let a = sub_entry(0, 19, 83); // ends at 102
        let b = sub_entry(0, 0, 50); // ends at 50 - no mesh
        let subs: Vec<&[u8]> = vec![&a, &b];
        // Two 102-vertex twins + one near-miss (103).
        let pack = vec![vec![102], vec![103], vec![102]];
        let pulse = EntryVdfPulse::build(&subs, &pack).expect("fits");
        assert_eq!(pulse.all_targets(), vec![(0, 0), (2, 0)]);
        assert!(pulse.lanes_for(1, 0).is_empty(), "near-miss mesh untouched");
        assert_eq!(pulse.lanes_for(0, 0).len(), 1, "only the fitting lane");
    }

    #[test]
    fn no_fit_builds_nothing() {
        let a = sub_entry(0, 0, 10);
        let subs: Vec<&[u8]> = vec![&a];
        assert!(EntryVdfPulse::build(&subs, &[vec![11]]).is_none());
        assert!(EntryVdfPulse::build(&[], &[vec![10]]).is_none());
    }

    #[test]
    fn pulse_cycles_forever_and_reports_dirty() {
        let a = sub_entry(0, 0, 4);
        let subs: Vec<&[u8]> = vec![&a];
        let mut pulse = EntryVdfPulse::build(&subs, &[vec![4]]).expect("fits");
        let mut peaked = false;
        let mut recycled = false;
        let mut prev = 0u16;
        let mut dirty_ticks = 0;
        for _ in 0..200 {
            let dirty = pulse.tick(2);
            if !dirty.is_empty() {
                dirty_ticks += 1;
                assert_eq!(dirty, vec![(0, 0)]);
            }
            let w = pulse.lanes_for(0, 0)[0].1;
            // The kernel clamps at 0x1000 and starts draining the same tick
            // it latches FINISHING, so the observable peak sits just below.
            if w >= 0xC00 {
                peaked = true;
            }
            if peaked && prev == 0 && w > 0 {
                recycled = true;
            }
            prev = w;
        }
        assert!(peaked, "lane reaches full weight");
        assert!(recycled, "LANE0_SNAP_DOWN recycles the pulse");
        assert!(dirty_ticks > 20, "the weight keeps moving ({dirty_ticks})");
    }
}
