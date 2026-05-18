//! Actor allocator host-trait abstractions.
//!
//! PORT: FUN_80024C88, FUN_80024D78, FUN_80024DFC
//!
//! Three small SCUS helpers around the per-actor allocator pipeline.
//! All three live in `SCUS_942.54` and are siblings of the larger
//! `FUN_80021B04` (move-VM spawn) and `FUN_801D77F4` (overlay-resident
//! actor allocator). They are exposed as host-trait methods so the
//! engine layer can implement the underlying allocator + TMD-table
//! semantics in its actor pool.
//!
//! - [`ActorAllocatorHost::spawn_at_position`] (FUN_80024C88, 29
//!   instr): allocates an actor from a caller-supplied pool, copies a
//!   3-component world position into `actor[+0x14..+0x18]`, runs the
//!   5-op GTE transform at `FUN_8003D344(actor+0x14, actor+0x2C)`.
//!   The world-overview viewer uses this as its per-actor allocator
//!   (see `docs/subsystems/world-overview-viewer.md`).
//! - [`ActorAllocatorHost::rebuild_object_table`] (FUN_80024D78, 31
//!   instr): per-actor OBJECT-table rebuild. Reads `actor[+0x64]` as a
//!   TMD index into the global `DAT_8007C018` pool, stamps the TMD's
//!   `group_count` and per-group descriptor pointers into the actor's
//!   `+0x44`-rooted pointer table, sets the `0x08000000` "renderable"
//!   bit in `actor[+0x10]`. See [`docs/subsystems/renderer.md`].
//! - [`ActorAllocatorHost::on_actor_cleanup`] (FUN_80024DFC, 3 instr):
//!   per-actor cleanup hook called from `FUN_8002519C` while freeing
//!   an actor. The retail body reads `actor[+0x56]` and masks the low
//!   nibble into `v1`, but never sets `v0` - the function is
//!   effectively an empty side-effect-free hook in retail. Engines
//!   that want to drop per-actor allocator metadata on free override
//!   this; the default implementation is a no-op (matching SCUS).
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The three
//! reference dumps (`ghidra/scripts/funcs/80024c88.txt`,
//! `80024d78.txt`, `80024dfc.txt`) are the *spec*. The trait
//! declarations IS the port; the engine layer supplies the
//! underlying allocator + per-actor OBJECT-table semantics.
//!
//! REF: FUN_80020DE0, FUN_8003D344, FUN_8002519C, FUN_80021B04, FUN_801D77F4

/// Position passed to [`ActorAllocatorHost::spawn_at_position`].
/// Mirrors the three u16 reads at `*param_1`, `param_1[1]`,
/// `param_1[2]` in FUN_80024C88; engines treat them as the actor's
/// world `(x, y, z)` in their own coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpawnPosition {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl SpawnPosition {
    pub const fn new(x: i16, y: i16, z: i16) -> Self {
        Self { x, y, z }
    }
}

/// Opaque actor handle the host returns from
/// [`ActorAllocatorHost::spawn_at_position`]. Retail returns a raw
/// pointer; engines re-host it as an index into their actor pool.
/// `None` on allocation failure (the retail `iVar1 == 0` branch).
pub type ActorHandle = u32;

/// Engine-side allocator hooks the runtime needs to construct + tear
/// down actors. Default implementations are no-ops where retail's
/// SCUS body is itself a no-op; the spawn / OBJECT-table methods
/// have no useful default and engines must override them.
pub trait ActorAllocatorHost {
    /// Allocate a fresh actor at `position` from the pool keyed by
    /// `(pool_a, pool_b)`. Mirrors `FUN_80024C88(pos_ptr, pool_a,
    /// pool_b)` which thunks through `FUN_80020DE0(pool_a, pool_b)`,
    /// copies the position vec3, and runs the GTE transform at
    /// `FUN_8003D344`. Returns `None` on allocation failure.
    ///
    /// `pool_a` and `pool_b` are forwarded verbatim from the retail
    /// caller's `a1` / `a2`; engines that maintain a single global
    /// pool can ignore them.
    fn spawn_at_position(
        &mut self,
        position: SpawnPosition,
        pool_a: u32,
        pool_b: u32,
    ) -> Option<ActorHandle>;

    /// Rebuild the per-actor OBJECT-pointer table at `actor[+0x44]`
    /// from the actor's TMD at `DAT_8007C018[actor[+0x64].i16]`.
    /// Mirrors `FUN_80024D78(actor)`:
    ///
    /// 1. `tmd_ptr = global_tmd_table[actor.tmd_idx]`.
    /// 2. `(*actor.object_table)[0] = tmd_ptr.group_count`.
    /// 3. For each group `i` in `0..group_count`:
    ///    `(*actor.object_table)[i + 1] = tmd_ptr + 0x0C + i * 0x1C`.
    /// 4. Set `actor[+0x10] |= 0x08000000`.
    /// 5. Return success (retail returns `1`).
    ///
    /// Engines re-host the global TMD pool however they choose; the
    /// invariants the trait method captures are the per-actor
    /// pointer-table layout (group_count at slot 0, per-group
    /// descriptors at slots 1..=N at stride `0x1C` inside the TMD)
    /// and the "renderable" bit set on completion.
    fn rebuild_object_table(&mut self, actor: ActorHandle) -> bool;

    /// Per-actor cleanup hook called from the actor-tick walker
    /// (`FUN_8002519C`) when an actor is being freed. Mirrors
    /// `FUN_80024DFC`. The retail body is effectively a no-op (reads
    /// `actor[+0x56]` low nibble into `v1` and returns without
    /// setting `v0`), so the default impl here is also a no-op.
    /// Engines that maintain per-actor allocator metadata (free
    /// lists, reverse pool indices) override this to drop the
    /// metadata on free.
    fn on_actor_cleanup(&mut self, actor: ActorHandle) {
        let _ = actor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Host that records every allocator call. Used by the smoke
    /// tests below to verify the trait dispatches as expected.
    #[derive(Default)]
    struct RecHost {
        spawned: Vec<(SpawnPosition, u32, u32)>,
        rebuilt: Vec<ActorHandle>,
        cleaned: Vec<ActorHandle>,
        next_handle: ActorHandle,
    }

    impl ActorAllocatorHost for RecHost {
        fn spawn_at_position(
            &mut self,
            position: SpawnPosition,
            pool_a: u32,
            pool_b: u32,
        ) -> Option<ActorHandle> {
            self.spawned.push((position, pool_a, pool_b));
            let h = self.next_handle;
            self.next_handle += 1;
            Some(h)
        }

        fn rebuild_object_table(&mut self, actor: ActorHandle) -> bool {
            self.rebuilt.push(actor);
            true
        }

        fn on_actor_cleanup(&mut self, actor: ActorHandle) {
            self.cleaned.push(actor);
        }
    }

    #[test]
    fn spawn_position_is_copy_default() {
        let p = SpawnPosition::default();
        let _q = p;
        let r = SpawnPosition::new(1, 2, 3);
        assert_eq!(r.x, 1);
        assert_eq!(r.y, 2);
        assert_eq!(r.z, 3);
    }

    #[test]
    fn host_spawn_records_position_and_pool() {
        let mut h = RecHost::default();
        let pos = SpawnPosition::new(100, 0, -200);
        let handle = h.spawn_at_position(pos, 0xAAAA, 0xBBBB);
        assert_eq!(handle, Some(0));
        assert_eq!(h.spawned, vec![(pos, 0xAAAA, 0xBBBB)]);
    }

    #[test]
    fn host_rebuild_object_table_records_actor() {
        let mut h = RecHost::default();
        let ok = h.rebuild_object_table(42);
        assert!(ok);
        assert_eq!(h.rebuilt, vec![42]);
    }

    #[test]
    fn host_cleanup_records_actor() {
        let mut h = RecHost::default();
        h.on_actor_cleanup(7);
        assert_eq!(h.cleaned, vec![7]);
    }

    #[test]
    fn cleanup_default_is_no_op() {
        // A host that doesn't override on_actor_cleanup compiles +
        // dispatches without panicking.
        struct NoOverride;
        impl ActorAllocatorHost for NoOverride {
            fn spawn_at_position(
                &mut self,
                _: SpawnPosition,
                _: u32,
                _: u32,
            ) -> Option<ActorHandle> {
                None
            }
            fn rebuild_object_table(&mut self, _: ActorHandle) -> bool {
                false
            }
        }
        let mut h = NoOverride;
        // The default body should be callable and return ().
        h.on_actor_cleanup(0);
        h.on_actor_cleanup(255);
    }
}
