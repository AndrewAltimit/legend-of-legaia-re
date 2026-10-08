//! Field-VM op `0x4B` on a field actor: the actor's **VDF vertex-morph**
//! lanes, the field-script sibling of the move VM's op `0x0A`.
//!
//! The arm (`0x801E0820..0x801E08C0` in `FUN_801DE840`) writes, for each of
//! `count` lanes, the VDF sub-entry index `base_id + i` to `+0xB0 + i`, the
//! up / down ramp velocities (two `u16`s per lane, read through
//! `FUN_8003CE9C`) to `+0xB8 + i*2` / `+0xC8 + i*2` and a zero weight to
//! `+0xA0 + i*2`; then raises `+0x10 |= 0x1000`, zeroes the lane-done mask
//! `+0x7C`, rewrites `+0x62 = (+0x62 | 0x1000) & 0xD3FF` and stores the count
//! at `+0x6C`. Nothing there selects a clip. The actor tick's anim step
//! (`FUN_800204F8`) runs the ramp envelope `FUN_80020740` first whenever
//! `+0x10 & 0x1000` is up, and the draw substitutes the weighted deltas into
//! the mesh (`FUN_8001C604`) - the same chain the ambient move-VM parts ride
//! ([`crate::world::ambient`]).
//!
//! Two kinds of field actor run it: a MAN partition-1 placement (an NPC,
//! keyed here by placement slot) and a `.MAP` placed object bound to a
//! partition-0 record (`FUN_8003A55C`, keyed by the flat record index). The
//! second is how `rikuroa`'s Genesis tree withers: three objects bound to
//! `P0[2..4]`, whose bind-time prologues run `4B 07 00 ..` / `4B 01 08 ..` /
//! `4B 01 07 ..` while story flag `0x142` is clear and raise the HOLD bit
//! (`2B 0A`, `+0x62 & 0x400`) behind it, so the envelope primes every lane to
//! `0x1000` and holds it. The `rikuroa_pre_caruban` capture holds exactly
//! that (`+0x62 = 0x415`); `rikuroa_post_genesis_tree` holds the lanes at `0`.
//!
//! PORT: FUN_801DE840 (the op-`0x4B` arm)
//! REF: FUN_800204F8, FUN_80020740, FUN_8001C604, FUN_8003A55C

use super::*;
use legaia_engine_vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE;
use legaia_engine_vm::move_vm::ActorState;
use legaia_engine_vm::vdf_morph;

/// Which field actor a morph belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MorphOwner {
    /// A MAN partition-1 placement, by placement slot.
    Placement(u8),
    /// A `.MAP` placed object, by the flat MAN record index its bind names.
    Object(u16),
}

/// Write an op-`0x4B` payload into `st`: `count` lanes (capped at the eight
/// the actor record has room for), sub-entries `base_id + i`, `frames` the
/// `count * 4` bytes of `(up: u16, down: u16)` pairs.
fn arm_lanes(st: &mut ActorState, count: u8, base_id: u8, frames: &[u8]) {
    let lanes = usize::from(count).min(vdf_morph::ACTOR_MORPH_LANES);
    for i in 0..lanes {
        let at = i * 4;
        let half = |o: usize| {
            frames
                .get(o..o + 2)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
        };
        st.anim_block_u8_set(0x04 + i, base_id.wrapping_add(i as u8));
        st.anim_block_u16_set(0x0C + i * 2, half(at));
        st.anim_block_u16_set(0x1C + i * 2, half(at + 2));
        vdf_morph::set_keyframe_weight(st, i, 0);
    }
    st.keyframe_count = lanes as u8;
    st.flags |= STATUS_FLAG_ENVELOPE_ACTIVE;
    st.field_7c = 0;
}

fn lane_weights(st: &ActorState) -> Vec<u16> {
    vdf_morph::actor_morph_lanes(st)
        .iter()
        .map(|&(_, w)| w)
        .collect()
}

impl World {
    /// Arm `owner`'s morph lanes from an op-`0x4B` payload.
    pub fn arm_field_morph(&mut self, owner: MorphOwner, count: u8, base_id: u8, frames: &[u8]) {
        let st = self.npcs.morphs.entry(owner).or_default();
        arm_lanes(st, count, base_id, frames);
        self.mark_morph_dirty(owner);
    }

    /// Capture alignment for the retail comparison's `play-window` child
    /// (`LEGAIA_SEAT_MORPHS`): write a retail state's live envelope over the
    /// morph of the field actor whose flat MAN index (`+0x50`) is `flat` -
    /// the lane weights `+0xA0 + i*2`, the lane-done mask `+0x7C` and the
    /// envelope control word `+0x62`. Where an envelope stands is time since
    /// the op-`0x4B` arm (`town01`'s shoreline objects run a tide that
    /// carries the sea up the beach and back), which no seed replays. Only a
    /// morph the engine armed itself is written; the lanes' sub-entries and
    /// ramp rates stay the engine's.
    pub fn seed_field_morph(&mut self, flat: u16, weights: &[u16], done_mask: u32, env: u16) {
        let Some(ci) = self
            .field_vm
            .channels
            .iter()
            .position(|c| c.ctx.script_id == flat)
        else {
            return;
        };
        let c = &self.field_vm.channels[ci];
        let owner = if c.object_bind {
            MorphOwner::Object(flat)
        } else {
            match u8::try_from(c.placement_index) {
                Ok(s) => MorphOwner::Placement(s),
                Err(_) => return,
            }
        };
        let Some(st) = self.npcs.morphs.get_mut(&owner) else {
            return;
        };
        let lanes = usize::from(st.keyframe_count).min(weights.len());
        for (i, &w) in weights.iter().take(lanes).enumerate() {
            vdf_morph::set_keyframe_weight(st, i, w);
        }
        st.field_7c = done_mask;
        st.local_flags = env;
        self.field_vm.channels[ci].ctx.local_flags = env;
        self.mark_morph_dirty(owner);
    }

    /// The channel context that owns `owner` (its `+0x10` / `+0x62` words).
    fn morph_channel(&self, owner: MorphOwner) -> Option<usize> {
        self.field_vm.channels.iter().position(|c| match owner {
            MorphOwner::Placement(s) => !c.object_bind && c.placement_index == usize::from(s),
            MorphOwner::Object(r) => c.object_bind && c.ctx.script_id == r,
        })
    }

    /// Record that `owner`'s deltas moved: a placement for the NPC-mesh
    /// rebuild ([`Self::take_npc_morph_dirty`]), an object as the
    /// `(pack_slot, group)` pairs its bound draws use, beside the ambient
    /// parts' ([`Self::take_morph_dirty_slots`]).
    fn mark_morph_dirty(&mut self, owner: MorphOwner) {
        match owner {
            MorphOwner::Placement(s) => {
                self.npcs.morph_dirty.insert(s);
            }
            MorphOwner::Object(r) => {
                let Some(st) = self.npcs.morphs.get(&owner) else {
                    return;
                };
                let slots = self
                    .npcs
                    .object_pack_slots
                    .get(&r)
                    .cloned()
                    .unwrap_or_default();
                let mut dirty = Vec::new();
                for &(vdf_idx, _) in &vdf_morph::actor_morph_lanes(st) {
                    let Some(entry) = self.vdf_record_bytes(vdf_idx) else {
                        continue;
                    };
                    for rec in vdf_morph::parse_vdf_morph_records(entry) {
                        for &slot in &slots {
                            dirty.push((slot, rec.group_id));
                        }
                    }
                }
                self.ambient.morph_dirty_slots.extend(dirty);
            }
        }
    }

    /// One actor-tick of every armed morph's ramp envelope - the
    /// `FUN_80020740` pre-step of `FUN_800204F8`. The envelope's control word
    /// is the owner channel's `+0x62` (the same word the clip cursor and the
    /// script's `2B` / `2C` ops address), read in and written back; the gate
    /// is the channel's own `+0x10 & 0x1000`, which a script can drop again.
    pub fn tick_npc_morphs(&mut self) {
        if self.npcs.morphs.is_empty() {
            return;
        }
        let owners: Vec<MorphOwner> = self.npcs.morphs.keys().copied().collect();
        for owner in owners {
            let Some(ci) = self.morph_channel(owner) else {
                continue;
            };
            let (flags, local) = {
                let ctx = &self.field_vm.channels[ci].ctx;
                (ctx.flags, ctx.local_flags)
            };
            let Some(st) = self.npcs.morphs.get_mut(&owner) else {
                continue;
            };
            let active = flags & STATUS_FLAG_ENVELOPE_ACTIVE != 0;
            let was_active = st.flags & STATUS_FLAG_ENVELOPE_ACTIVE != 0;
            if !active {
                st.flags &= !STATUS_FLAG_ENVELOPE_ACTIVE;
                if was_active {
                    self.mark_morph_dirty(owner);
                }
                continue;
            }
            st.flags |= STATUS_FLAG_ENVELOPE_ACTIVE;
            let before = lane_weights(st);
            st.local_flags = local;
            vdf_morph::envelope_tick_actor(st, 1);
            let changed = lane_weights(st) != before || !was_active;
            let out = st.local_flags;
            self.field_vm.channels[ci].ctx.local_flags = out;
            if changed {
                self.mark_morph_dirty(owner);
            }
        }
    }

    /// `owner`'s live lanes, or `None` when it carries no armed lane with a
    /// weight - its mesh then draws the authored pose.
    fn live_morph_lanes(&self, owner: MorphOwner) -> Option<Vec<(u8, u16)>> {
        let st = self.npcs.morphs.get(&owner)?;
        if st.flags & STATUS_FLAG_ENVELOPE_ACTIVE == 0 {
            return None;
        }
        let lanes = vdf_morph::actor_morph_lanes(st);
        (!lanes.iter().all(|&(_, w)| w == 0)).then_some(lanes)
    }

    /// The summed weighted VDF deltas for placement `slot`'s TMD object
    /// `group` (`n_verts` vertices), or `None` when it carries no live lane.
    pub fn npc_morph_deltas(&self, slot: u8, group: u32, n_verts: usize) -> Option<Vec<[i16; 3]>> {
        let lanes = self.live_morph_lanes(MorphOwner::Placement(slot))?;
        Some(self.morph_deltas_for(&lanes, group, n_verts))
    }

    /// Whether placement `slot` carries a live morph lane (armed, envelope
    /// up, some weight non-zero) - the cheap test a host runs before asking
    /// for [`Self::npc_morphed_tmd`].
    pub fn npc_morph_live(&self, slot: u8) -> bool {
        self.live_morph_lanes(MorphOwner::Placement(slot)).is_some()
    }

    /// Placement `slot`'s live morph staged onto a copy of its mesh - the
    /// `FUN_8001C604` substitution for an NPC: each TMD object (group) takes
    /// its weighted deltas in object-local space, so the caller poses the
    /// returned mesh exactly as it would the authored one (the bone transform
    /// runs on the substituted vertices, as retail's per-group draw does).
    /// `None` when the slot carries no live lane - the authored mesh draws.
    ///
    /// The one staging kernel both hosts' NPC draws go through: the native
    /// play-window re-poses its clip / rest mesh from it, the browser play
    /// page rebuilds the catalog entry's base positions from it.
    // REF: FUN_8001C604, FUN_8005B038
    pub fn npc_morphed_tmd(&self, slot: u8, tmd: &legaia_tmd::Tmd) -> Option<legaia_tmd::Tmd> {
        let lanes = self.live_morph_lanes(MorphOwner::Placement(slot))?;
        let mut out = tmd.clone();
        let mut any = false;
        for (group, obj) in out.objects.iter_mut().enumerate() {
            let deltas = self.morph_deltas_for(&lanes, group as u32, obj.vertices.len());
            for (v, d) in obj.vertices.iter_mut().zip(deltas.iter()) {
                if *d == [0, 0, 0] {
                    continue;
                }
                v.x = v.x.wrapping_add(d[0]);
                v.y = v.y.wrapping_add(d[1]);
                v.z = v.z.wrapping_add(d[2]);
                any = true;
            }
        }
        any.then_some(out)
    }

    /// The live lanes of every placed-object morph whose bound draws use
    /// env-pack slot `pack_slot` - folded into
    /// [`Self::current_morph_deltas`] beside the ambient parts'.
    pub(crate) fn object_morph_lanes_for(&self, pack_slot: usize) -> Vec<(u8, u16)> {
        // Retail stages each bound actor's lanes onto that actor's own draw.
        // The port's draws of one pack slot share one mesh, so the slot takes
        // one owner's lanes - summing them multiplied the delta by the number
        // of objects (town01's four shoreline objects all bind slot 82 and
        // run the same tide envelope).
        for (&record, slots) in &self.npcs.object_pack_slots {
            if !slots.contains(&pack_slot) {
                continue;
            }
            if let Some(l) = self.live_morph_lanes(MorphOwner::Object(record)) {
                return l;
            }
        }
        Vec::new()
    }

    /// Whether a placed object whose draws use env-pack slot `pack_slot`
    /// carries op-`0x4B` morph lanes at all (armed, whatever their current
    /// weight) - a retail carrier owns that mesh's morph.
    pub(crate) fn slot_has_object_morph_owner(&self, pack_slot: usize) -> bool {
        self.npcs.object_pack_slots.iter().any(|(&record, slots)| {
            slots.contains(&pack_slot) && self.npcs.morphs.contains_key(&MorphOwner::Object(record))
        })
    }

    /// Drain the placement slots whose morph deltas moved since the last
    /// call; a host rebuilds just those NPC meshes.
    pub fn take_npc_morph_dirty(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.npcs.morph_dirty)
            .into_iter()
            .collect()
    }

    /// `owner`'s lanes as `(sub_entry, weight)`, armed or not (diagnostics
    /// and tests).
    pub fn field_morph_lanes(&self, owner: MorphOwner) -> Option<Vec<(u8, u16)>> {
        self.npcs
            .morphs
            .get(&owner)
            .map(vdf_morph::actor_morph_lanes)
    }

    /// Seat the `.MAP` placed-object binds' env-pack slots, flat record ->
    /// the pack slots of every placed draw whose anchor tile binds it. Scene
    /// entry calls this beside [`Self::seed_object_channels`], so a bound
    /// object's morph reaches the pack meshes its draws instance.
    pub fn set_object_morph_targets(
        &mut self,
        targets: std::collections::BTreeMap<u16, Vec<usize>>,
    ) {
        self.npcs.object_pack_slots = targets;
    }
}
