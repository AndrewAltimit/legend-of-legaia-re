//! [`legaia_engine_vm::actor_alloc::ActorAllocatorHost`] implementation for
//! [`crate::world::World`].
//!
//! Wires the three retail-allocator helpers from
//! `engine-vm::actor_alloc` (FUN_80024C88 / FUN_80024D78 / FUN_80024DFC)
//! onto the World's existing actor pool + DAT_8007C018 mirror:
//!
//! - `spawn_at_position` → find a free [`crate::world::Actor`] slot,
//!   activate it, copy the vec3 position into [`MoveActorState::world_x`]
//!   / `world_y` / `world_z`. The GTE world→view transform retail runs
//!   at `FUN_8003D344` is folded into the per-frame renderer pass in the
//!   clean-room engine, so the spawn-time call is a no-op here.
//! - `rebuild_object_table` → the retail "stamp +0x44 pointer table"
//!   semantic collapses in the clean-room because rendering reaches
//!   into [`Actor::tmd_ref`]`.tmd.objects` directly. The method returns
//!   `true` iff the actor has a populated TMD reference (set by the
//!   field-VM `0x4C 0xD8` host hook from [`World::global_tmd`]) and
//!   marks the slot active so the renderer picks it up. The retail
//!   `actor[+0x10] |= 0x08000000` "renderable" bit maps to
//!   [`Actor::active`].
//! - `on_actor_cleanup` → the retail body is effectively a no-op
//!   ([`ghidra/scripts/funcs/80024dfc.txt`] reads `actor[+0x56]` low
//!   nibble and returns); the clean-room hook drops the per-actor TMD
//!   reference + deactivates the slot so the pool can recycle it.
//!
//! ## Pool keying (`pool_a` / `pool_b`)
//!
//! Retail's `FUN_80020DE0(pool_a, pool_b)` selects one of three actor
//! pools by the supplied `(a, b)` discriminator. The clean-room engine
//! maintains a single [`World::actors`] vector and ignores the
//! discriminator (the trait declaration explicitly notes this is fine
//! for hosts that maintain a single global pool).
//!
//! [`MoveActorState::world_x`]: legaia_engine_vm::move_vm::ActorState::world_x
//! [`Actor::tmd_ref`]: crate::world::Actor::tmd_ref
//! [`Actor::active`]: crate::world::Actor::active

use legaia_engine_vm::actor_alloc::{ActorAllocatorHost, ActorHandle, SpawnPosition};

use crate::world::World;

impl ActorAllocatorHost for World {
    fn spawn_at_position(
        &mut self,
        position: SpawnPosition,
        _pool_a: u32,
        _pool_b: u32,
    ) -> Option<ActorHandle> {
        // Find the first inactive slot. Retail's FUN_80020DE0 walks a
        // free-list inside the per-pool linked list at gp+0x148; the
        // clean-room engine's simpler vector pool reduces to a linear
        // scan since per-frame churn is bounded by MAX_ACTORS.
        let slot = self.actors.iter().position(|a| !a.active)?;
        let actor = &mut self.actors[slot];
        actor.active = true;
        actor.move_state.world_x = position.x;
        actor.move_state.world_y = position.y;
        actor.move_state.world_z = position.z;
        // Keep world_y_mirror in sync (retail uses both for the move-VM
        // pre-tick + collision query - see crates/engine-vm/src/move_vm.rs).
        actor.move_state.world_y_mirror = position.y;
        Some(slot as ActorHandle)
    }

    fn rebuild_object_table(&mut self, actor: ActorHandle) -> bool {
        let Some(a) = self.actors.get_mut(actor as usize) else {
            return false;
        };
        // Retail equivalent: reads actor[+0x64] as a TMD index into
        // DAT_8007C018, stamps group_count + per-group descriptor
        // pointers into actor's +0x44-rooted pointer table, sets the
        // 0x08000000 "renderable" bit in actor[+0x10].
        //
        // Clean-room: rendering walks `tmd_ref.tmd.objects` directly,
        // so the +0x44 table doesn't exist. The "renderable" bit maps
        // to `Actor::active`. The method succeeds iff the actor was
        // populated with a TMD reference upstream (the field-VM
        // 0x4C 0xD8 host hook reads from World::global_tmd).
        if a.tmd_ref.is_some() {
            a.active = true;
            true
        } else {
            false
        }
    }

    fn on_actor_cleanup(&mut self, actor: ActorHandle) {
        // Retail FUN_80024DFC body is a no-op (reads actor[+0x56] low
        // nibble into v1, returns without setting v0). The clean-room
        // hook drops the per-actor TMD reference + deactivates the
        // slot so a subsequent spawn_at_position can claim it.
        if let Some(a) = self.actors.get_mut(actor as usize) {
            a.tmd_ref = None;
            a.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{GlobalTmd, World};
    use std::sync::Arc;

    fn make_world() -> World {
        World::new()
    }

    fn make_stub_global_tmd() -> Arc<GlobalTmd> {
        // Minimum legaia_tmd::Tmd that compiles - the allocator host
        // only checks `tmd_ref.is_some()`, never dereferences the
        // Tmd. Same shape as world.rs:4222's existing test stub.
        Arc::new(GlobalTmd {
            tmd: legaia_tmd::Tmd {
                header: legaia_tmd::Header {
                    id: 0x8000_0002,
                    flags: 1,
                    nobj: 0,
                    flist_bit_set: true,
                },
                objects: Vec::new(),
            },
            raw: Vec::new(),
        })
    }

    #[test]
    fn spawn_at_position_activates_first_free_slot_and_writes_pos() {
        let mut w = make_world();
        let pos = SpawnPosition::new(123, -45, 678);
        let h = w.spawn_at_position(pos, 0xAAAA, 0xBBBB).expect("spawn");
        assert_eq!(h, 0, "first spawn lands in slot 0");
        let a = &w.actors[0];
        assert!(a.active);
        assert_eq!(a.move_state.world_x, 123);
        assert_eq!(a.move_state.world_y, -45);
        assert_eq!(a.move_state.world_z, 678);
        assert_eq!(a.move_state.world_y_mirror, -45);
    }

    #[test]
    fn spawn_skips_already_active_slots() {
        let mut w = make_world();
        w.spawn_actor(0);
        w.spawn_actor(1);
        // Slots 0/1 are now active; next spawn should land in slot 2.
        let h = w
            .spawn_at_position(SpawnPosition::default(), 0, 0)
            .expect("spawn");
        assert_eq!(h, 2);
        assert!(w.actors[2].active);
        assert!(w.actors[0].active);
        assert!(w.actors[1].active);
    }

    #[test]
    fn spawn_returns_none_when_pool_is_full() {
        let mut w = make_world();
        for a in &mut w.actors {
            a.active = true;
        }
        assert_eq!(
            w.spawn_at_position(SpawnPosition::default(), 0, 0),
            None,
            "full pool yields None (matches retail iVar1==0 branch)"
        );
    }

    #[test]
    fn rebuild_object_table_succeeds_only_when_tmd_ref_populated() {
        let mut w = make_world();
        let h = w.spawn_at_position(SpawnPosition::default(), 0, 0).unwrap();
        // No TMD ref yet -> rebuild fails.
        assert!(!w.rebuild_object_table(h));
        // Populate the TMD ref the same way the field-VM 0x4C 0xD8
        // hook would and re-run.
        w.actors[h as usize].tmd_ref = Some(make_stub_global_tmd());
        assert!(w.rebuild_object_table(h));
        // Renderable bit (= Actor::active) was set as a side effect.
        assert!(w.actors[h as usize].active);
    }

    #[test]
    fn rebuild_object_table_out_of_range_returns_false() {
        let mut w = make_world();
        assert!(!w.rebuild_object_table(9999));
    }

    #[test]
    fn cleanup_clears_tmd_ref_and_deactivates_slot() {
        let mut w = make_world();
        let h = w.spawn_at_position(SpawnPosition::default(), 0, 0).unwrap();
        w.actors[h as usize].tmd_ref = Some(make_stub_global_tmd());
        assert!(w.actors[h as usize].tmd_ref.is_some());
        assert!(w.actors[h as usize].active);
        w.on_actor_cleanup(h);
        assert!(w.actors[h as usize].tmd_ref.is_none());
        assert!(!w.actors[h as usize].active);
    }

    #[test]
    fn cleanup_then_spawn_reuses_the_slot() {
        let mut w = make_world();
        let first = w
            .spawn_at_position(SpawnPosition::new(1, 2, 3), 0, 0)
            .unwrap();
        w.on_actor_cleanup(first);
        let second = w
            .spawn_at_position(SpawnPosition::new(9, 9, 9), 0, 0)
            .unwrap();
        assert_eq!(first, second, "freed slot is reused");
        assert_eq!(w.actors[second as usize].move_state.world_x, 9);
    }

    #[test]
    fn cleanup_out_of_range_is_silent_no_op() {
        let mut w = make_world();
        // Should neither panic nor mutate anything observable.
        w.on_actor_cleanup(9999);
    }
}
