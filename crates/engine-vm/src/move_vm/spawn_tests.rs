use super::*;
use crate::actor_alloc::{ActorAllocatorHost, ActorHandle, SpawnPosition};
use std::cell::RefCell;

/// Recording host. Tracks every trait dispatch in call-order so tests
/// can verify the FUN_80021B04 sequence.
#[derive(Default)]
struct RecSpawnHost {
    spawn_calls: RefCell<Vec<(SpawnPosition, u32, u32)>>,
    rebuild_calls: RefCell<Vec<ActorHandle>>,
    apply_calls: RefCell<Vec<(ActorHandle, SpawnSubmode, MoveSpawnRequest)>>,
    kick_calls: RefCell<Vec<ActorHandle>>,
    mirror_calls: RefCell<Vec<ActorHandle>>,
    /// Set to `None` to model allocator failure.
    next_handle: RefCell<Option<ActorHandle>>,
}

impl RecSpawnHost {
    fn new() -> Self {
        Self {
            next_handle: RefCell::new(Some(7)),
            ..Default::default()
        }
    }
}

impl ActorAllocatorHost for RecSpawnHost {
    fn spawn_at_position(
        &mut self,
        position: SpawnPosition,
        pool_a: u32,
        pool_b: u32,
    ) -> Option<ActorHandle> {
        self.spawn_calls
            .borrow_mut()
            .push((position, pool_a, pool_b));
        *self.next_handle.borrow()
    }

    fn rebuild_object_table(&mut self, actor: ActorHandle) -> bool {
        self.rebuild_calls.borrow_mut().push(actor);
        true
    }
}

impl MoveSpawnHost for RecSpawnHost {
    fn apply_move_spawn_state(
        &mut self,
        actor: ActorHandle,
        submode: SpawnSubmode,
        req: &MoveSpawnRequest,
    ) {
        self.apply_calls.borrow_mut().push((actor, submode, *req));
    }

    fn kick_move_vm(&mut self, actor: ActorHandle) {
        self.kick_calls.borrow_mut().push(actor);
    }

    fn mirror_world_y(&mut self, actor: ActorHandle) {
        self.mirror_calls.borrow_mut().push(actor);
    }
}

fn req(init_word: u16) -> MoveSpawnRequest {
    MoveSpawnRequest {
        pos: SpawnPosition::new(10, 20, 30),
        rot: [0x100, 0x200, 0x300],
        init_word,
        seq_word: 0xABCD,
    }
}

#[test]
fn classify_dispatches_each_arm() {
    assert_eq!(SpawnSubmode::classify(0x8000), SpawnSubmode::Negative);
    assert_eq!(SpawnSubmode::classify(0xFFFF), SpawnSubmode::Negative);
    assert_eq!(SpawnSubmode::classify(0x4000), SpawnSubmode::Keyframe);
    assert_eq!(SpawnSubmode::classify(0x4001), SpawnSubmode::Tween);
    assert_eq!(SpawnSubmode::classify(0), SpawnSubmode::Default);
    assert_eq!(SpawnSubmode::classify(0x1234), SpawnSubmode::Default);
    // 0x3FFF (non-negative, below 0x4000) is Default, not Keyframe.
    assert_eq!(SpawnSubmode::classify(0x3FFF), SpawnSubmode::Default);
    // 0x4002 (non-negative, just past 0x4001) is Default.
    assert_eq!(SpawnSubmode::classify(0x4002), SpawnSubmode::Default);
}

#[test]
fn spawn_default_arm_runs_full_sequence() {
    let mut host = RecSpawnHost::new();
    let r = req(0x1234);
    let h = spawn_move_actor(&mut host, r).expect("non-failing allocator");
    assert_eq!(h, 7);
    assert_eq!(
        host.spawn_calls.borrow().as_slice(),
        &[(r.pos, MOVE_SPAWN_POOL_A, MOVE_SPAWN_POOL_B)],
    );
    // Default arm rebuilds OBJECT table.
    assert_eq!(host.rebuild_calls.borrow().as_slice(), &[7]);
    assert_eq!(
        host.apply_calls.borrow().as_slice(),
        &[(7, SpawnSubmode::Default, r)],
    );
    assert_eq!(host.kick_calls.borrow().as_slice(), &[7]);
    assert_eq!(host.mirror_calls.borrow().as_slice(), &[7]);
}

#[test]
fn spawn_negative_arm_skips_rebuild() {
    let mut host = RecSpawnHost::new();
    let r = req(0x8000);
    let h = spawn_move_actor(&mut host, r).unwrap();
    assert_eq!(h, 7);
    // Negative arm: spawn + apply + kick + mirror; NO rebuild.
    assert_eq!(host.spawn_calls.borrow().len(), 1);
    assert!(
        host.rebuild_calls.borrow().is_empty(),
        "no OBJ-table rebuild"
    );
    assert_eq!(
        host.apply_calls.borrow().as_slice(),
        &[(7, SpawnSubmode::Negative, r)],
    );
    assert_eq!(host.kick_calls.borrow().as_slice(), &[7]);
    assert_eq!(host.mirror_calls.borrow().as_slice(), &[7]);
}

#[test]
fn spawn_keyframe_arm_dispatches_correctly() {
    let mut host = RecSpawnHost::new();
    let r = req(0x4000);
    spawn_move_actor(&mut host, r).unwrap();
    // 0x4000 is non-negative -> rebuild runs.
    assert_eq!(host.rebuild_calls.borrow().as_slice(), &[7]);
    assert_eq!(
        host.apply_calls.borrow().as_slice(),
        &[(7, SpawnSubmode::Keyframe, r)],
    );
}

#[test]
fn spawn_tween_arm_dispatches_correctly() {
    let mut host = RecSpawnHost::new();
    let r = req(0x4001);
    spawn_move_actor(&mut host, r).unwrap();
    assert_eq!(host.rebuild_calls.borrow().as_slice(), &[7]);
    assert_eq!(
        host.apply_calls.borrow().as_slice(),
        &[(7, SpawnSubmode::Tween, r)],
    );
}

#[test]
fn allocator_failure_short_circuits() {
    let mut host = RecSpawnHost::default(); // next_handle = None
    let r = req(0);
    assert_eq!(spawn_move_actor(&mut host, r), None);
    // Allocator was called.
    assert_eq!(host.spawn_calls.borrow().len(), 1);
    // Every later stage is skipped.
    assert!(host.rebuild_calls.borrow().is_empty());
    assert!(host.apply_calls.borrow().is_empty());
    assert!(host.kick_calls.borrow().is_empty());
    assert!(host.mirror_calls.borrow().is_empty());
}

#[test]
fn spawn_forwards_retail_pool_constants() {
    let mut host = RecSpawnHost::new();
    spawn_move_actor(&mut host, req(0)).unwrap();
    let (_, a, b) = host.spawn_calls.borrow()[0];
    assert_eq!(a, MOVE_SPAWN_POOL_A);
    assert_eq!(b, MOVE_SPAWN_POOL_B);
    // The constants must match the SCUS dump literals.
    assert_eq!(MOVE_SPAWN_POOL_A, 0x8007_062C);
    assert_eq!(MOVE_SPAWN_POOL_B, 0x8007_C350);
}

// ---------------------------------------------------------------------------
// FUN_80050E74 - part-actor pool flush.
// ---------------------------------------------------------------------------

/// A pool with `seated` actors at the given slot indices, over `n` slots.
fn seated_pool<'a>(
    actors: &'a mut [crate::move_vm::ActorState],
    at: &[usize],
    n: usize,
) -> Vec<Option<&'a mut crate::move_vm::ActorState>> {
    let mut pool: Vec<Option<&mut crate::move_vm::ActorState>> = (0..n).map(|_| None).collect();
    for (slot, actor) in at.iter().zip(actors.iter_mut()) {
        pool[*slot] = Some(actor);
    }
    pool
}

fn parked_actor() -> crate::move_vm::ActorState {
    crate::move_vm::ActorState {
        wait_timer: 0x40,
        field_8c: 3,
        flags: 0x1000,
        ..Default::default()
    }
}

#[test]
fn pool_flush_empties_every_seated_slot() {
    let mut actors = [parked_actor(), parked_actor(), parked_actor()];
    let last = PART_POOL_SLOTS - 1;
    let mut pool = seated_pool(&mut actors, &[0, 5, last], PART_POOL_SLOTS);

    assert_eq!(flush_part_actor_pool(&mut pool), 3);
    assert!(pool.iter().all(|s| s.is_none()), "every slot is nulled");
    for a in &actors {
        assert_eq!(a.wait_timer, 0, "a parked WAIT_SET would outlive the flush");
        assert_eq!(a.field_8c, 0, "an open 0x18/0x19 loop would re-enter");
        assert_eq!(
            a.flags,
            0x1000 | PART_ACTOR_HALT_FLAG,
            "the halt bit is ORed in, other flags survive"
        );
    }
}

#[test]
fn pool_flush_is_unconditional_unlike_the_teardown_collect() {
    // FUN_800480D8 releases only seats whose actor already carries the halt
    // bit; FUN_80050E74 releases all of them and raises the bit itself.
    let mut already_halted = crate::move_vm::ActorState {
        flags: PART_ACTOR_HALT_FLAG,
        ..Default::default()
    };
    let mut running = parked_actor();
    let mut pool: Vec<Option<&mut crate::move_vm::ActorState>> =
        vec![Some(&mut already_halted), Some(&mut running)];
    assert_eq!(flush_part_actor_pool(&mut pool), 2);
    assert_eq!(running.flags, 0x1000 | PART_ACTOR_HALT_FLAG);
}

#[test]
fn pool_flush_stops_at_the_retail_slot_count() {
    let mut actors: Vec<crate::move_vm::ActorState> =
        (0..PART_POOL_SLOTS + 4).map(|_| parked_actor()).collect();
    let mut pool: Vec<Option<&mut crate::move_vm::ActorState>> =
        actors.iter_mut().map(Some).collect();
    assert_eq!(flush_part_actor_pool(&mut pool), PART_POOL_SLOTS);
    assert!(
        pool[PART_POOL_SLOTS..].iter().all(|s| s.is_some()),
        "the walk is bounded by `slti a1, 0x80`"
    );
}

#[test]
fn the_halt_bit_is_the_one_move_vm_op_08_sets() {
    assert_eq!(PART_ACTOR_HALT_FLAG, 0x8);
    assert_eq!(PART_POOL_SLOTS, 0x80);
}
