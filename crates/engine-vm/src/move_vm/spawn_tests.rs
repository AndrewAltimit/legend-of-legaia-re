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
