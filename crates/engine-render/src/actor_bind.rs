//! Field / world-map / battle **actor render binding** - the pass that resolves
//! an actor's mesh-pool index off its scene placement record and allocates the
//! render node the draw path walks.
//!
//! PORT: FUN_80020f88
//!
//! NOT WIRED: this crate holds no actor state. Retail's inputs are three fields
//! of the `0xD8`-byte scene actor (`+0x10` flag word, `+0x60` placement slot,
//! `+0x56` kind) and the per-scene `.MAP` record table at `_DAT_1F8003EC`; its
//! outputs are four more actor fields plus a `0x9C`-byte heap allocation. The
//! actor pool lives in `legaia_engine_core` (scene host + actor list) and the
//! engine resolves scene geometry per placement record at scene-build time
//! rather than per actor per frame, so nothing here owns the struct to write
//! into. What closes the gap is the scene host calling [`bind_actor_render`]
//! when it spawns an actor and honouring [`ActorBind::render_node`], which is
//! `legaia_engine_core`'s file scope, not this crate's.
//!
//! The **mesh-index rule** the pass establishes is what matters most and is
//! already load-bearing elsewhere: an object's mesh id is its placement
//! record's `+0x10` field plus the kingdom-TMD prefix, *not* its position in
//! the pack. See `docs/subsystems/renderer.md` (the falsified positional rule)
//! and `docs/subsystems/world-map.md`.
//!
//! REF: FUN_80024d78 - builds the actor's mesh chain (`actor + 0x44`) from
//! `DAT_8007C018[actor + 0x64]`, i.e. it consumes the index this pass writes.
//! REF: FUN_800204a4 - links the bound actor into the draw list.
//! REF: FUN_80017888 - the general allocator behind the `0x9C`-byte node.
//!
//! # What the pass does
//!
//! Two independent refresh gates read the same `0x20`-byte placement record:
//!
//! | actor `+0x10` bit | effect |
//! |---|---|
//! | `0x8000` | refresh from `record[actor + 0x60]`, masking `record[+0x12]` with `0x3E8` |
//! | `0x100000` | pick the kind from `record[actor + 0x64] & 3`, then refresh with mask `0x380` |
//!
//! The `0x100000` arm is the subtle one: it indexes the record table by the
//! actor's **mesh index** (`+0x64`, the output of the previous refresh), not by
//! its placement slot (`+0x60`), and only then re-derives `+0x64` from `+0x60`
//! again. Bit `0x40000` clear also clears bit `0x2`.
//!
//! Then, for kinds `1..=5`, `7` and `8` - note `6` is excluded even though the
//! `0x100000` arm can *set* `6` - the pass allocates the render node once
//! (idempotent under bit `0x800`), seeds its four tail fields, and unless bit
//! `0x40000` is set runs the mesh-chain build / draw-list link chain. A failed
//! allocation resets the kind to `0`, raises `0x4000` in the global error word
//! and returns `-1`.
//!
//! Source: `ghidra/scripts/funcs/80020f88.txt` (disassembly).

/// `actor + 0x10` bit: refresh the binding from the placement record.
pub const FLAG_REFRESH: u32 = 0x8000;
/// `actor + 0x10` bit: re-pick the actor kind from the record's low two bits.
pub const FLAG_REPICK_KIND: u32 = 0x0010_0000;
/// `actor + 0x10` bit: the render node is already allocated.
pub const FLAG_NODE_READY: u32 = 0x0800;
/// `actor + 0x10` bit: suppress the mesh-chain build / draw-list link, and hold
/// bit [`FLAG_BIT1`].
pub const FLAG_NO_CHAIN: u32 = 0x0004_0000;
/// `actor + 0x10` bit cleared whenever [`FLAG_NO_CHAIN`] is clear.
pub const FLAG_BIT1: u32 = 0x0002;

/// Size of the render node retail allocates for a bound actor.
pub const RENDER_NODE_BYTES: usize = 0x9C;
/// Error bit raised in the global status word when the node allocation fails.
pub const ALLOC_FAIL_STATUS_BIT: u32 = 0x4000;

/// Mask applied to `record[+0x12]` on the [`FLAG_REFRESH`] path.
pub const REFRESH_FLAG_MASK: u16 = 0x03E8;
/// Mask applied to `record[+0x12]` on the [`FLAG_REPICK_KIND`] path.
pub const REPICK_FLAG_MASK: u16 = 0x0380;

/// Actor kinds that own a render node. `0` and `6` do not - and `6` is exactly
/// what a record whose low two bits are `1` selects, so a record can bind an
/// actor to a kind that then declines a node.
pub const NODE_KINDS: [u16; 7] = [1, 2, 3, 4, 5, 7, 8];

/// Kind chosen by `record[+0x12] & 3`, indexed by that value.
pub const KIND_BY_RECORD_BITS: [u16; 4] = [0, 6, 7, 8];

/// The three fields of a `0x20`-byte scene placement record this pass reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlacementRecord {
    /// `+0x10` - the mesh-pool index *before* the prefix is added.
    pub mesh_id: u16,
    /// `+0x12` - the flag halfword the two arms mask differently.
    pub flags: u16,
    /// `+0x1E` - copied straight into actor `+0x58`.
    pub sub_id: u8,
}

/// The actor fields the pass reads and writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorBinding {
    /// `+0x10` flag word.
    pub flags: u32,
    /// `+0x52` masked record flags.
    pub record_flags: u16,
    /// `+0x56` actor kind.
    pub kind: u16,
    /// `+0x58` sub id.
    pub sub_id: u16,
    /// `+0x60` placement slot - the record-table index. Read only.
    pub slot: u16,
    /// `+0x64` mesh-pool index (`record.mesh_id + prefix`).
    pub mesh_index: u16,
}

/// What the pass asks the host to do after the field updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorNodeAction {
    /// Kind owns no render node; nothing else happens. Retail returns `0`.
    None,
    /// A node is already present (bit [`FLAG_NODE_READY`] was set); reseed its
    /// tail fields.
    Reseed,
    /// Allocate a [`RENDER_NODE_BYTES`]-byte node, then reseed.
    Allocate,
}

/// Whether the pass follows through into the mesh-chain build + draw-list link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorChainAction {
    /// Bit [`FLAG_NO_CHAIN`] was set - retail skips the chain entirely.
    Skip,
    /// Run `FUN_80024d78`; on a zero result run `FUN_800204a4`, otherwise clear
    /// [`FLAG_NO_CHAIN`].
    Build,
}

/// The whole pass's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorBind {
    /// The actor after every field update.
    pub actor: ActorBinding,
    /// Render-node request.
    pub render_node: ActorNodeAction,
    /// Mesh-chain follow-through, when a node is involved.
    pub chain: ActorChainAction,
    /// `true` when the pass's debug bound check would have fired (mesh index
    /// above the pool high-water mark). Retail only prints; the engine surfaces
    /// it so a bad scene record is visible instead of silent.
    pub mesh_index_out_of_range: bool,
}

/// The four tail fields retail seeds on the render node every time the kind
/// owns one: `+0x94`/`+0x96`/`+0x98` zero, `+0x9A` all-ones. Retail also zeroes
/// the actor's own `+0x7C` in the same block, which the engine's actor does not
/// model.
pub const RENDER_NODE_SEED: [(usize, u16); 4] = [(0x94, 0), (0x96, 0), (0x98, 0), (0x9A, 0xFFFF)];

/// Run the binding pass.
///
/// * `actor` - the actor fields on entry.
/// * `records` - the per-scene placement table (`_DAT_1F8003EC`, stride `0x20`),
///   indexed by placement slot.
/// * `mesh_prefix` - `DAT_8007B6F8`, the count of party-character TMDs that
///   precede the scene bundle's own in the mesh pool.
/// * `mesh_pool_len` - `_DAT_8007BB38`, the pool high-water mark the debug
///   bound check compares against.
pub fn bind_actor_render(
    actor: ActorBinding,
    records: &[PlacementRecord],
    mesh_prefix: u16,
    mesh_pool_len: u32,
) -> ActorBind {
    let mut a = actor;
    let rec = |i: u16| -> PlacementRecord { records.get(i as usize).copied().unwrap_or_default() };

    // Refresh arm.
    if a.flags & FLAG_REFRESH != 0 {
        let r = rec(a.slot);
        a.sub_id = u16::from(r.sub_id);
        a.mesh_index = r.mesh_id.wrapping_add(mesh_prefix);
        a.record_flags = r.flags & REFRESH_FLAG_MASK;
    }

    // Retail's debug bound check: signed load, unsigned compare, so a negative
    // index fails it too.
    let signed = a.mesh_index as i16;
    let mesh_index_out_of_range = (signed as u32) > mesh_pool_len;

    // Re-pick arm. Note the record is indexed by the *mesh index* here.
    if a.flags & FLAG_REPICK_KIND != 0 {
        let bits = (rec(a.mesh_index).flags & 3) as usize;
        a.kind = KIND_BY_RECORD_BITS[bits];
        let r = rec(a.slot);
        a.mesh_index = r.mesh_id.wrapping_add(mesh_prefix);
        a.sub_id = u16::from(r.sub_id);
        a.record_flags = r.flags & REPICK_FLAG_MASK;
    }

    if a.flags & FLAG_NO_CHAIN == 0 {
        a.flags &= !FLAG_BIT1;
    }

    let mut out = ActorBind {
        actor: a,
        render_node: ActorNodeAction::None,
        chain: ActorChainAction::Skip,
        mesh_index_out_of_range,
    };

    if !NODE_KINDS.contains(&a.kind) {
        return out;
    }

    out.render_node = if a.flags & FLAG_NODE_READY == 0 {
        ActorNodeAction::Allocate
    } else {
        ActorNodeAction::Reseed
    };
    out.actor.flags |= FLAG_NODE_READY;
    out.chain = if out.actor.flags & FLAG_NO_CHAIN == 0 {
        ActorChainAction::Build
    } else {
        ActorChainAction::Skip
    };
    out
}

/// The actor state retail leaves behind when the `0x9C`-byte allocation fails:
/// kind reset to `0`, the caller's status word raised, `-1` returned.
pub fn on_render_node_alloc_failure(actor: &mut ActorBinding, status: &mut u32) -> i32 {
    actor.kind = 0;
    *status |= ALLOC_FAIL_STATUS_BIT;
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recs() -> Vec<PlacementRecord> {
        vec![
            PlacementRecord {
                mesh_id: 84,
                flags: 0x03FF,
                sub_id: 9,
            },
            PlacementRecord {
                mesh_id: 109,
                flags: 0x0001,
                sub_id: 3,
            },
            PlacementRecord {
                mesh_id: 7,
                flags: 0x0002,
                sub_id: 1,
            },
            PlacementRecord {
                mesh_id: 7,
                flags: 0x0003,
                sub_id: 1,
            },
        ]
    }

    fn actor(flags: u32, slot: u16, kind: u16) -> ActorBinding {
        ActorBinding {
            flags,
            slot,
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn mesh_index_is_the_records_field_plus_the_prefix() {
        // The falsified positional rule mapped object 114 to pack 109; the
        // record resolves it to the textured pack 84.
        let b = bind_actor_render(actor(FLAG_REFRESH, 0, 0), &recs(), 5, 0x400);
        assert_eq!(b.actor.mesh_index, 84 + 5);
        assert_eq!(b.actor.sub_id, 9);
    }

    #[test]
    fn refresh_and_repick_mask_the_flag_halfword_differently() {
        let r = recs();
        let refresh = bind_actor_render(actor(FLAG_REFRESH, 0, 0), &r, 0, 0x400);
        assert_eq!(refresh.actor.record_flags, 0x03FF & REFRESH_FLAG_MASK);
        let repick = bind_actor_render(actor(FLAG_REPICK_KIND, 0, 0), &r, 0, 0x400);
        assert_eq!(repick.actor.record_flags, 0x03FF & REPICK_FLAG_MASK);
    }

    #[test]
    fn repick_indexes_the_table_by_mesh_index_not_by_slot() {
        let r = recs();
        // Slot 0 (flags 0x3FF -> bits 3), but mesh index 2 (flags 0x2 ->
        // bits 2). The kind must come from the mesh index's record.
        let mut a = actor(FLAG_REPICK_KIND, 0, 0);
        a.mesh_index = 2;
        let b = bind_actor_render(a, &r, 0, 0x400);
        assert_eq!(b.actor.kind, KIND_BY_RECORD_BITS[2]);
        // ... and the mesh index is then re-derived from the slot.
        assert_eq!(b.actor.mesh_index, 84);
    }

    #[test]
    fn record_bits_one_selects_kind_six_which_owns_no_node() {
        let r = recs();
        let mut a = actor(FLAG_REPICK_KIND, 1, 0);
        a.mesh_index = 1; // record flags 0x0001 -> bits 1 -> kind 6
        let b = bind_actor_render(a, &r, 0, 0x400);
        assert_eq!(b.actor.kind, 6);
        assert_eq!(b.render_node, ActorNodeAction::None);
        assert_eq!(b.chain, ActorChainAction::Skip);
    }

    #[test]
    fn every_node_kind_asks_for_an_allocation_and_the_rest_do_not() {
        for kind in 0u16..=9 {
            let b = bind_actor_render(actor(0, 0, kind), &recs(), 0, 0x400);
            let want = if NODE_KINDS.contains(&kind) {
                ActorNodeAction::Allocate
            } else {
                ActorNodeAction::None
            };
            assert_eq!(b.render_node, want, "kind {kind}");
        }
    }

    #[test]
    fn node_ready_bit_makes_the_pass_idempotent() {
        let b = bind_actor_render(actor(FLAG_NODE_READY, 0, 1), &recs(), 0, 0x400);
        assert_eq!(b.render_node, ActorNodeAction::Reseed);
        assert_ne!(b.actor.flags & FLAG_NODE_READY, 0);
    }

    #[test]
    fn no_chain_bit_both_holds_bit1_and_skips_the_chain() {
        let a = actor(FLAG_NO_CHAIN | FLAG_BIT1, 0, 1);
        let b = bind_actor_render(a, &recs(), 0, 0x400);
        assert_ne!(b.actor.flags & FLAG_BIT1, 0);
        assert_eq!(b.chain, ActorChainAction::Skip);

        let a = actor(FLAG_BIT1, 0, 1);
        let b = bind_actor_render(a, &recs(), 0, 0x400);
        assert_eq!(b.actor.flags & FLAG_BIT1, 0);
        assert_eq!(b.chain, ActorChainAction::Build);
    }

    #[test]
    fn bound_check_fires_above_the_pool_mark_and_on_a_negative_index() {
        let mut a = actor(0, 0, 0);
        a.mesh_index = 0x0401;
        assert!(bind_actor_render(a, &recs(), 0, 0x400).mesh_index_out_of_range);
        a.mesh_index = 0x0400;
        assert!(!bind_actor_render(a, &recs(), 0, 0x400).mesh_index_out_of_range);
        a.mesh_index = 0xFFFF; // -1 as i16
        assert!(bind_actor_render(a, &recs(), 0, 0x400).mesh_index_out_of_range);
    }

    #[test]
    fn alloc_failure_resets_the_kind_and_raises_the_status_bit() {
        let mut a = actor(0, 0, 5);
        let mut status = 0u32;
        assert_eq!(on_render_node_alloc_failure(&mut a, &mut status), -1);
        assert_eq!(a.kind, 0);
        assert_eq!(status, ALLOC_FAIL_STATUS_BIT);
    }

    #[test]
    fn missing_record_binds_to_the_prefix_alone() {
        // Retail reads past the table; the engine substitutes zeros, so the
        // index degenerates to the prefix rather than to scene garbage.
        let b = bind_actor_render(actor(FLAG_REFRESH, 900, 0), &recs(), 5, 0x400);
        assert_eq!(b.actor.mesh_index, 5);
        assert_eq!(b.actor.record_flags, 0);
    }
}
