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
//!
//! ## Move-VM spawn entry point
//!
//! [`MoveSpawnHost`] (also implemented for [`World`] below) composes the
//! three [`ActorAllocatorHost`] hooks plus a few per-actor state writes into
//! the retail `FUN_80021B04` shape - the move-VM spawn helper that allocates
//! an actor, configures it from the move buffer's leading init word, and
//! kicks the per-actor move VM. See the trait docstrings in
//! [`legaia_engine_vm::move_vm`] for the full spec; the implementation here
//! mirrors the per-submode field writes from
//! `ghidra/scripts/funcs/80021b04.txt`.

use legaia_engine_vm::actor_alloc::{ActorAllocatorHost, ActorHandle, SpawnPosition};
use legaia_engine_vm::move_vm::{MoveSpawnHost, MoveSpawnRequest, SpawnSubmode};

use crate::world::{MOVE_VM_BUDGET, World};

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

/// FUN_80021B04 host hooks. The retail body decomposes into:
///
/// 1. Allocator + pos copy + GTE transform (covered by
///    [`ActorAllocatorHost::spawn_at_position`]).
/// 2. Conditional OBJECT-table rebuild (covered by
///    [`ActorAllocatorHost::rebuild_object_table`]).
/// 3. Per-submode field writes (this trait's
///    [`apply_move_spawn_state`]).
/// 4. Move-VM kick (this trait's [`kick_move_vm`]).
/// 5. world_y → world_y_mirror copy (this trait's [`mirror_world_y`]).
///
/// [`apply_move_spawn_state`]: MoveSpawnHost::apply_move_spawn_state
/// [`kick_move_vm`]: MoveSpawnHost::kick_move_vm
/// [`mirror_world_y`]: MoveSpawnHost::mirror_world_y
impl MoveSpawnHost for World {
    fn apply_move_spawn_state(
        &mut self,
        actor: ActorHandle,
        submode: SpawnSubmode,
        req: &MoveSpawnRequest,
    ) {
        let Some(a) = self.actors.get_mut(actor as usize) else {
            return;
        };
        let ms = &mut a.move_state;

        // Retail FUN_80021B04 sequences these writes in three blocks; the
        // ordering matters because Keyframe / Tween RE-SET the 0x2 bit that
        // the positive-arm AND mask just cleared.
        //
        // Block 1 (retail 0x80021bd0..0x80021c68): per-arm initial flag set.
        //   - Negative arm: clear +0x56/+0x5A, flags |= 0x2.
        //   - Positive arms: rebuild OBJ-table (already done by the caller),
        //     +0x5A = 1, flags |= 0x08000000.
        // Block 2 (retail 0x80021c6c..0x80021cb0): common header writes,
        //   then positive-only `flags &= ~0x2`.
        // Block 3 (retail 0x80021cb4..0x80021d7c): per-arm fine-tune:
        //   - Keyframe / Tween: override +0x5A, RE-SET bit 0x2, clear slots.
        //   - Default fall-through: clear render scratch, write +0x96.
        match submode {
            SpawnSubmode::Negative => {
                ms.move_substate = 0;
                ms.move_submode = 0;
                ms.flags |= 0x2;
            }
            _ => {
                // Positive-arm initial state (Keyframe / Tween / Default
                // all start here before the Block 3 fine-tune).
                ms.move_submode = 1;
                ms.flags |= 0x0800_0000;
            }
        }

        // Block 2: common header + flag 0x4000 + positive-arm bit-2 mask.
        // Retail writes `actor[+0x70] = 2`. The move-VM ActorState stores PC
        // in u16 units (matching retail's `actor[+0x70]` semantic of "u16
        // word index"), so we copy the literal value through. The engine's
        // bytecode loader is responsible for placing the move buffer at the
        // shape this PC expects (retail's first opcode lives at u16 index 2
        // of the move buffer: index 0 is the init word, index 1 is the
        // header pad consumed by retail's per-actor preamble).
        ms.pc = 2;
        ms.flags |= 0x4000;
        if submode != SpawnSubmode::Negative {
            ms.flags &= !0x2u32;
        }

        // Block 3: per-arm fine-tune.
        match submode {
            SpawnSubmode::Negative => {
                // NEG already set everything it needs in Block 1; no extra
                // fine-tune in retail.
            }
            SpawnSubmode::Keyframe => {
                // +0x5A=3, +0x56=0, flags |= 0x2 (re-set after mask),
                // clear keyframe slots +0x9C / +0x9E / +0xA8.
                ms.move_submode = 3;
                ms.move_substate = 0;
                ms.flags |= 0x2;
                ms.field_9c = 0;
                ms.field_9e = 0;
                ms.field_a8 = 0;
            }
            SpawnSubmode::Tween => {
                // +0x5A=5, +0x56=0, flags |= 0x2 (re-set after mask),
                // clear +0x98/+0x9A and anim_block +0xB2/+0xB4/+0xB8,
                // write +0xB0 = 0xFFFF.
                ms.move_submode = 5;
                ms.move_substate = 0;
                ms.flags |= 0x2;
                ms.tween_scale_y = 0;
                ms.tween_scale_z = 0;
                // anim_block byte_offsets relative to +0xAC:
                //   +0xB0 → byte_off 0x04 (index 2) := 0xFFFF
                //   +0xB2 → byte_off 0x06 (index 3) := 0
                //   +0xB4 → byte_off 0x08 (index 4) := 0
                //   +0xB8 → byte_off 0x0C (index 6) := 0
                ms.anim_block_u16_set(0x04, 0xFFFF);
                ms.anim_block_u16_set(0x06, 0);
                ms.anim_block_u16_set(0x08, 0);
                ms.anim_block_u16_set(0x0C, 0);
            }
            SpawnSubmode::Default => {
                // Default fall-through: clear render scratch
                // (+0x80..+0x84, +0x90..+0x9A, +0xC0..+0xCA), then write
                // +0x96 = rot[1] & 0xFFF.
                ms.anim_80 = 0;
                ms.anim_82 = 0;
                ms.anim_84 = 0;
                ms.tween_src_x = 0;
                ms.tween_src_y = 0;
                ms.tween_src_z = 0;
                ms.tween_scale_y = 0;
                ms.tween_scale_z = 0;
                ms.tween_scale_x = (req.rot[1] & 0x0FFF) as i16;
                for off in [0x14, 0x16, 0x18, 0x1A, 0x1C, 0x1E] {
                    ms.anim_block_u16_set(off, 0);
                }
            }
        }

        // Common scratch clear + rot/seq writes (retail 0x80021d80..d8.
        ms.anim_3c = 0;
        ms.anim_3e = 0;
        ms.anim_40 = 0;
        ms.render_24 = req.rot[0] as i16;
        ms.render_26 = req.rot[1] as i16;
        ms.render_28 = req.rot[2] as i16;
        ms.field_72 = req.seq_word;
    }

    fn kick_move_vm(&mut self, actor: ActorHandle) {
        // Retail FUN_80021B04 calls FUN_80023070(actor) unconditionally at
        // the tail. Drive one move-VM tick here so the freshly-spawned
        // actor's first instruction runs in the same frame as the spawn,
        // matching retail order-of-effects. If no bytecode is loaded for
        // this slot, the tick is a no-op - callers stage the move buffer
        // via World::move_bytecode before invoking the spawn.
        let slot = actor as usize;
        let bc = match self.move_bytecode.get(slot) {
            Some(b) if !b.is_empty() => b.clone(),
            _ => return,
        };
        let _ = self.actor_tick_at(slot, &bc, MOVE_VM_BUDGET);
    }

    fn mirror_world_y(&mut self, actor: ActorHandle) {
        // Retail 0x80021dc8..0x80021dd0: `actor[+0x2A] = actor[+0x16]`.
        if let Some(a) = self.actors.get_mut(actor as usize) {
            a.move_state.world_y_mirror = a.move_state.world_y;
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

    // ----------------------------------------------------------------------
    // MoveSpawnHost (FUN_80021B04) tests.
    // ----------------------------------------------------------------------

    use legaia_engine_vm::move_vm::{
        MOVE_SPAWN_POOL_A, MOVE_SPAWN_POOL_B, MoveSpawnRequest, spawn_move_actor,
    };

    fn req(init_word: u16) -> MoveSpawnRequest {
        MoveSpawnRequest {
            pos: SpawnPosition::new(10, 20, 30),
            rot: [0x111, 0x222, 0x333],
            init_word,
            seq_word: 0xCAFE,
        }
    }

    #[test]
    fn move_spawn_default_arm_writes_common_epilogue_fields() {
        let mut w = make_world();
        let h = spawn_move_actor(&mut w, req(0x1234)).expect("alloc");
        let ms = &w.actors[h as usize].move_state;
        // Spawn allocated + wrote pos via spawn_at_position.
        assert_eq!(ms.world_x, 10);
        assert_eq!(ms.world_y, 20);
        assert_eq!(ms.world_z, 30);
        // mirror_world_y: world_y_mirror == world_y.
        assert_eq!(ms.world_y_mirror, 20);
        // Common epilogue: rot triple + seq_word + cleared scratch.
        assert_eq!(ms.render_24, 0x111);
        assert_eq!(ms.render_26, 0x222);
        assert_eq!(ms.render_28, 0x333);
        assert_eq!(ms.field_72, 0xCAFE);
        assert_eq!(ms.anim_3c, 0);
        assert_eq!(ms.anim_3e, 0);
        assert_eq!(ms.anim_40, 0);
        // Default-arm: move_submode = 1, flag 0x08000000 set, flag 0x4000
        // set, flag 0x2 cleared.
        assert_eq!(ms.move_submode, 1);
        assert!(ms.flags & 0x0800_0000 != 0);
        assert!(ms.flags & 0x4000 != 0);
        assert!(ms.flags & 0x2 == 0);
        // Default-arm: tween_scale_x = rot[1] & 0xFFF.
        assert_eq!(ms.tween_scale_x as u16, 0x222 & 0x0FFF);
        // pc = 2 (u16 unit; retail writes literal 2 to actor[+0x70]).
        assert_eq!(ms.pc, 2);
    }

    #[test]
    fn move_spawn_negative_arm_sets_flag_2_and_skips_obj_table() {
        let mut w = make_world();
        let h = spawn_move_actor(&mut w, req(0x8000)).expect("alloc");
        let ms = &w.actors[h as usize].move_state;
        // Negative arm: move_substate/move_submode both cleared, flag 0x2
        // set, flag 0x4000 still set (common epilogue), flag 0x2 NOT cleared
        // (the post-epilogue AND ~0x2 only runs for non-negative arms).
        assert_eq!(ms.move_substate, 0);
        assert_eq!(ms.move_submode, 0);
        assert!(ms.flags & 0x2 != 0, "negative arm preserves bit 0x2");
        assert!(ms.flags & 0x4000 != 0);
        // Renderable bit (0x08000000) is NOT set on the negative arm.
        assert!(ms.flags & 0x0800_0000 == 0);
        // Rotation triple still written.
        assert_eq!(ms.render_24, 0x111);
    }

    #[test]
    fn move_spawn_keyframe_arm_clears_keyframe_slots() {
        let mut w = make_world();
        // Pre-populate keyframe slots so the clear is observable.
        let h0 = w.spawn_at_position(SpawnPosition::default(), 0, 0).unwrap();
        w.actors[h0 as usize].move_state.field_9c = 0x1234;
        w.actors[h0 as usize].move_state.field_9e = 0x5678;
        w.actors[h0 as usize].move_state.field_a8 = -1;
        w.on_actor_cleanup(h0);
        // Now spawn fresh via the move-spawn entry.
        let h = spawn_move_actor(&mut w, req(0x4000)).expect("alloc");
        let ms = &w.actors[h as usize].move_state;
        assert_eq!(ms.move_submode, 3);
        assert!(ms.flags & 0x2 != 0);
        assert_eq!(ms.field_9c, 0);
        assert_eq!(ms.field_9e, 0);
        assert_eq!(ms.field_a8, 0);
    }

    #[test]
    fn move_spawn_tween_arm_sets_b0_marker_and_clears_others() {
        let mut w = make_world();
        let h = spawn_move_actor(&mut w, req(0x4001)).expect("alloc");
        let ms = &w.actors[h as usize].move_state;
        assert_eq!(ms.move_submode, 5);
        assert!(ms.flags & 0x2 != 0);
        // +0xB0 (anim_block byte_off 0x04) = 0xFFFF.
        assert_eq!(ms.anim_block_u16(0x04), 0xFFFF);
        // +0xB2 / +0xB4 / +0xB8 cleared.
        assert_eq!(ms.anim_block_u16(0x06), 0);
        assert_eq!(ms.anim_block_u16(0x08), 0);
        assert_eq!(ms.anim_block_u16(0x0C), 0);
        // tween_scale_y / tween_scale_z cleared.
        assert_eq!(ms.tween_scale_y, 0);
        assert_eq!(ms.tween_scale_z, 0);
    }

    #[test]
    fn move_spawn_returns_none_when_pool_is_full() {
        let mut w = make_world();
        for a in &mut w.actors {
            a.active = true;
        }
        assert_eq!(
            spawn_move_actor(&mut w, req(0)),
            None,
            "full pool yields None (matches retail iVar3==0 branch)"
        );
    }

    #[test]
    fn move_spawn_forwards_retail_pool_selectors() {
        // Sanity-check the constants pass through the trait dispatch. The
        // single-pool clean-room engine ignores them, but the values must
        // match the SCUS dump.
        assert_eq!(MOVE_SPAWN_POOL_A, 0x8007_062C);
        assert_eq!(MOVE_SPAWN_POOL_B, 0x8007_C350);
    }
}
