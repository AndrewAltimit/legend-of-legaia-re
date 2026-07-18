//! Unit tests for the effect VM. Split out of `effect_vm.rs`.

#![allow(clippy::field_reassign_with_default)] // tests are clearer with sequential field writes

use super::*;

/// Recording host. Captures every callback so tests can assert exact
/// dispatch order without spinning up a renderer.
#[derive(Default)]
struct RecHost {
    rng_seq: Vec<i32>,
    rng_pos: usize,
    summon_ids: std::collections::HashSet<u8>,
    events: Vec<HostEvent>,
    advance_outcomes: Vec<StateOutcome>,
    advance_pos: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum HostEvent {
    Random,
    HandleSummon(u8, [i16; 3], u16),
    ChildOffset(usize, u8, i16, i16),
    AdvanceState(usize),
    ChildMotion(usize),
}

impl EffectHost for RecHost {
    fn next_random(&mut self) -> i32 {
        self.events.push(HostEvent::Random);
        let v = self.rng_seq.get(self.rng_pos).copied().unwrap_or(0);
        self.rng_pos += 1;
        v
    }

    fn is_summon_effect(&self, effect_id: u8) -> bool {
        self.summon_ids.contains(&effect_id)
    }

    fn handle_summon(&mut self, effect_id: u8, world_pos: [i16; 3], angle: u16) {
        self.events
            .push(HostEvent::HandleSummon(effect_id, world_pos, angle));
    }

    fn assign_child_random_offset(&mut self, slot: usize, child_idx: u8, dx: i16, dz: i16) {
        self.events
            .push(HostEvent::ChildOffset(slot, child_idx, dx, dz));
    }

    fn accumulate_child_motion(&mut self, slot: usize, _m: &mut MasterSlot) {
        self.events.push(HostEvent::ChildMotion(slot));
    }

    fn advance_state(&mut self, slot: usize, _m: &mut MasterSlot) -> StateOutcome {
        self.events.push(HostEvent::AdvanceState(slot));
        let outcome = self
            .advance_outcomes
            .get(self.advance_pos)
            .copied()
            .unwrap_or(StateOutcome::Continue);
        self.advance_pos += 1;
        outcome
    }
}

#[test]
fn init_zeros_all_slots() {
    let mut pool = Pool::new();
    // Smudge the pool so init has something to clear.
    pool.master_slots[0].child_count = 1;
    pool.children[5].src_x = 0xDEAD_BEEFu32 as i32;

    pool.init(PoolHead {
        param_id: 0x1000,
        param_extra: 0x0A00,
        pack0_base: 0x1234_0000,
        pack1_index_base: 0x1234_4000,
        pack1_body_base: 0x1234_8000,
    });

    assert_eq!(pool.master_slots[0].child_count, 0);
    assert_eq!(pool.children[5].src_x, 0);
    assert_eq!(pool.head.param_id, 0x1000);
    assert_eq!(pool.head.param_extra, 0x0A00);
    assert_eq!(pool.head.pack0_base, 0x1234_0000);
}

#[test]
fn allocate_master_finds_first_empty() {
    let mut pool = Pool::new();
    assert_eq!(pool.allocate_master(), Some(0));

    // Mark slot 0 active; allocator advances.
    pool.master_slots[0].child_count = 3;
    assert_eq!(pool.allocate_master(), Some(1));

    // Fill all slots; allocator returns None.
    for m in &mut pool.master_slots {
        m.child_count = 1;
    }
    assert_eq!(pool.allocate_master(), None);
}

#[test]
fn spawn_routes_summon_to_handler() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.summon_ids.insert(4);
    let script = EffectScript::default();

    let r = pool.spawn(&mut host, 4, [10, 20, 30], 0x123, &script, &[]);
    assert_eq!(r, None);
    assert_eq!(
        host.events,
        vec![HostEvent::HandleSummon(4, [10, 20, 30], 0x123)]
    );
    // No master slot consumed.
    assert_eq!(pool.master_slots[0].child_count, 0);
}

#[test]
fn spawn_initializes_master_slot() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let script = EffectScript {
        child_count: 4,
        flags: 0x00, // no random distribution
        spread: 16,
        body: vec![0u8; 16],
    };

    let slot = pool
        .spawn(&mut host, 7, [100, -50, 200], 0x800, &script, &[])
        .expect("slot");
    assert_eq!(slot, 0);

    let m = &pool.master_slots[0];
    assert_eq!(m.child_count, 4);
    assert_eq!(m.flags, 0x00);
    assert_eq!(m.angle, 0x800);
    assert_eq!(m.pos_x, 100i32 << 8);
    assert_eq!(m.pos_y, (-50i32) << 8);
    assert_eq!(m.pos_z, 200i32 << 8);
    assert_eq!(m.state, 0);

    // No random distribution requested, so no host events.
    assert!(host.events.is_empty());
}

#[test]
fn spawn_distributes_random_children_when_flag_set() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    // Two children, deterministic RNG sequence.
    host.rng_seq = vec![0, 8, 31, 7];

    let script = EffectScript {
        child_count: 2,
        flags: 0x01,
        spread: 16,
        body: vec![],
    };

    let _ = pool
        .spawn(&mut host, 9, [0, 0, 0], 0, &script, &[])
        .unwrap();

    // Expected per-child math: modulus = 32, raw % 32 - 16.
    // child 0: (0 % 32) - 16 = -16, (8 % 32) - 16 = -8.
    // child 1: (31 % 32) - 16 = 15, (7 % 32) - 16 = -9.
    let want: Vec<HostEvent> = vec![
        HostEvent::Random,
        HostEvent::Random,
        HostEvent::ChildOffset(0, 0, -16, -8),
        HostEvent::Random,
        HostEvent::Random,
        HostEvent::ChildOffset(0, 1, 15, -9),
    ];
    assert_eq!(host.events, want);
}

#[test]
fn spawn_angle_is_masked_to_12_bits() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let script = EffectScript {
        child_count: 1,
        ..EffectScript::default()
    };
    let _ = pool
        .spawn(&mut host, 1, [0, 0, 0], 0xF234, &script, &[])
        .unwrap();
    assert_eq!(pool.master_slots[0].angle, 0x0234);
}

#[test]
fn tick_decrements_state_below_8() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    // Mark slot 0 active with a low state.
    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 5;

    pool.tick(&mut host);

    // State < 8 → goes to 0 in one tick (retail clears, doesn't decrement).
    assert_eq!(pool.master_slots[0].state, 0);
    // No advance_state (slot was waiting) - but child motion still
    // integrates every frame, even during the wait (C-EFXANIM).
    assert_eq!(host.events, vec![HostEvent::ChildMotion(0)]);
}

#[test]
fn tick_rebases_state_at_or_above_8() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 16;

    pool.tick(&mut host);

    // State >= 8 → state -= 8.
    assert_eq!(pool.master_slots[0].state, 8);
    // Waiting slot: motion integrates, but no script advance.
    assert_eq!(host.events, vec![HostEvent::ChildMotion(0)]);
}

#[test]
fn tick_calls_advance_state_when_state_is_zero() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.advance_outcomes = vec![StateOutcome::Continue];

    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 0;

    pool.tick(&mut host);

    // Motion integrates first, then the state==0 script advance fires.
    assert_eq!(
        host.events,
        vec![HostEvent::ChildMotion(0), HostEvent::AdvanceState(0)]
    );
    // Slot still active.
    assert_eq!(pool.master_slots[0].child_count, 1);
}

#[test]
fn tick_terminate_clears_slot() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.advance_outcomes = vec![StateOutcome::Terminate];

    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 0;
    pool.master_slots[0].pos_x = 0xDEAD;

    pool.tick(&mut host);

    assert_eq!(pool.master_slots[0].child_count, 0);
    assert_eq!(pool.master_slots[0].pos_x, 0);
}

#[test]
fn tick_wait_encodes_frames_via_state_byte() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.advance_outcomes = vec![StateOutcome::Wait { frames: 3 }];

    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 0;

    pool.tick(&mut host);

    // Wait { frames: 3 } → state = 3 + 8 = 11 (so countdown rebase
    // brings it back to 3 next tick).
    assert_eq!(pool.master_slots[0].state, 11);
}

#[test]
fn tick_skips_inactive_slots() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    // No slot activated → no advance_state ever called.
    pool.tick(&mut host);
    assert!(host.events.is_empty());
}

#[test]
fn tick_iterates_all_active_slots() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.advance_outcomes = vec![StateOutcome::Continue; 5];

    // Activate slots 0, 7, 31.
    for &i in &[0usize, 7, 31] {
        pool.master_slots[i].child_count = 1;
    }

    pool.tick(&mut host);

    // Each active slot integrates motion then advances its state, in
    // slot order.
    let want = vec![
        HostEvent::ChildMotion(0),
        HostEvent::AdvanceState(0),
        HostEvent::ChildMotion(7),
        HostEvent::AdvanceState(7),
        HostEvent::ChildMotion(31),
        HostEvent::AdvanceState(31),
    ];
    assert_eq!(host.events, want);
}

/// C-EFXANIM regression: a waiting slot (`state != 0`) still integrates
/// child motion every frame - retail `FUN_801E0088` runs the per-child
/// position accumulation in its wait-countdown branch, not just the
/// `state == 0` work loop. The hook must fire while `advance_state` stays
/// gated, so a billboard keeps drifting during a wait instead of freezing.
#[test]
fn child_motion_runs_during_wait_state_but_advance_does_not() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.advance_outcomes = vec![StateOutcome::Continue];

    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 16; // waiting (>= 8)

    pool.tick(&mut host);

    assert!(
        host.events.contains(&HostEvent::ChildMotion(0)),
        "child motion must integrate during a wait state"
    );
    assert!(
        !host.events.contains(&HostEvent::AdvanceState(0)),
        "script advance must stay gated on state == 0"
    );
}

/// Pool exhaustion: spawning into a fully-occupied pool returns `None`.
/// Mirrors retail's "no free slot → drop spawn" branch in `FUN_801DFDF8`.
#[test]
fn spawn_returns_none_when_pool_exhausted() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();

    // Mark every master slot in use.
    for m in &mut pool.master_slots {
        m.child_count = 1;
    }

    let r = pool.spawn(&mut host, 10, [0, 0, 0], 0, &EffectScript::default(), &[]);
    assert_eq!(r, None);
    // No host event was recorded - the pool returned before any work.
    assert!(host.events.is_empty());
}

/// Spawn → tick to completion → slot freed → respawn. Validates the
/// full lifecycle of a master slot: terminate clears `child_count`,
/// then the next allocator call returns the same slot index.
#[test]
fn spawn_terminate_respawn_reuses_slot() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let script = EffectScript::default();

    // First spawn - slot 0.
    let s0 = pool
        .spawn(&mut host, 10, [1, 2, 3], 0x111, &script, &[])
        .expect("first spawn ok");
    assert_eq!(s0, 0);
    assert_eq!(pool.master_slots[0].child_count, 0); // EffectScript::default() has 0 children
    // child_count == 0 means the slot is "empty" by allocator rules. To
    // simulate a real spawn that activates the slot, mark it manually.
    pool.master_slots[0].child_count = 1;

    // Tick once - host returns Terminate for this slot.
    host.advance_outcomes = vec![StateOutcome::Terminate];
    pool.master_slots[0].state = 0; // ensure work-path runs
    pool.tick(&mut host);
    assert_eq!(pool.master_slots[0].child_count, 0); // freed

    // Second spawn - should reuse slot 0 since it's the first empty.
    let s1 = pool
        .spawn(&mut host, 11, [4, 5, 6], 0x222, &script, &[])
        .expect("respawn ok");
    assert_eq!(s1, 0);
}

/// Tick a Wait-encoded slot through several frames - each tick subtracts
/// 8 (saturating) until state hits 0, at which point the next tick fires
/// `advance_state`. Mirrors retail's `state -= 8` countdown at
/// `0x801e0130..0x801e015f`.
#[test]
fn wait_state_subtracts_8_per_tick_across_multiple_frames() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();

    // Seed slot 0 with state = 24 - three ticks of `-= 8` before reaching 0.
    pool.master_slots[0].child_count = 1;
    pool.master_slots[0].state = 24;

    // Each wait tick integrates motion but never advances the script.
    // After tick: 24 → 16. No advance_state.
    pool.tick(&mut host);
    assert_eq!(pool.master_slots[0].state, 16);
    assert_eq!(host.events, vec![HostEvent::ChildMotion(0)]);
    assert!(
        !host.events.contains(&HostEvent::AdvanceState(0)),
        "advance_state called too early"
    );
    host.events.clear();

    // After tick: 16 → 8.
    pool.tick(&mut host);
    assert_eq!(pool.master_slots[0].state, 8);
    assert_eq!(host.events, vec![HostEvent::ChildMotion(0)]);
    host.events.clear();

    // After tick: 8 → 0 (still NOT a work tick - saturates).
    pool.tick(&mut host);
    assert_eq!(pool.master_slots[0].state, 0);
    assert_eq!(host.events, vec![HostEvent::ChildMotion(0)]);
    host.events.clear();

    // After tick: state==0 → motion + script advance both fire.
    host.advance_outcomes = vec![StateOutcome::Continue];
    pool.tick(&mut host);
    assert_eq!(
        host.events,
        vec![HostEvent::ChildMotion(0), HostEvent::AdvanceState(0)]
    );
}

#[test]
fn active_count_zero_on_fresh_pool() {
    let pool = Pool::new();
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn active_count_increments_after_spawn_with_children() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let script = EffectScript {
        child_count: 3,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    pool.spawn(&mut host, 1, [0, 0, 0], 0, &script, &[])
        .unwrap();
    assert_eq!(pool.active_count(), 1);
    // A second slot
    pool.spawn(&mut host, 2, [0, 0, 0], 0, &script, &[])
        .unwrap();
    assert_eq!(pool.active_count(), 2);
}

#[test]
fn spawn_by_ui_id_fills_slot_from_catalog() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let script = EffectScript {
        child_count: 2,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    let catalog = EffectCatalog::new(vec![(script, vec![])]);
    assert_eq!(pool.active_count(), 0);
    let slot = pool.spawn_by_ui_id(&mut host, 0, [10, 20, 30], 0x100, &catalog);
    assert_eq!(slot, Some(0));
    assert_eq!(pool.active_count(), 1);
    assert_eq!(pool.master_slots[0].child_count, 2);
    assert_eq!(pool.master_slots[0].angle, 0x100);
}

#[test]
fn spawn_by_ui_id_out_of_range_returns_none() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    let catalog = EffectCatalog::default();
    assert!(
        pool.spawn_by_ui_id(&mut host, 5, [0, 0, 0], 0, &catalog)
            .is_none()
    );
    assert_eq!(pool.active_count(), 0);
}

#[test]
fn catalog_from_pack1_bytes_parses_single_script() {
    // Pack1 with 1 entry:
    // count=1 at [0..4], word_offset[0]=2 at [4..8] (byte 8)
    // entry at [8..14]: child_count=0, flags=0, spread=8 LE, body=[0xAA, 0xBB]
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes()); // count=1
    data.extend_from_slice(&2u32.to_le_bytes()); // offset[0] = word 2 = byte 8
    data.extend_from_slice(&[0u8, 0, 8, 0, 0xAA, 0xBB]); // entry
    let catalog = EffectCatalog::from_pack1_bytes(&data);
    assert_eq!(catalog.len(), 1);
    let (script, children) = catalog.entry(0).unwrap();
    assert_eq!(script.child_count, 0);
    assert_eq!(script.spread, 8);
    assert_eq!(script.body, vec![0xAA, 0xBB]);
    assert!(children.is_empty());
}

#[test]
fn catalog_from_pack1_bytes_empty_on_bad_data() {
    // Implausible count → empty catalog.
    let data = 0xFFFF_FFFFu32.to_le_bytes();
    let catalog = EffectCatalog::from_pack1_bytes(&data);
    assert!(catalog.is_empty());
}

/// The real `efect.dat` 2-pack: header pointers, an inline sprite atlas,
/// pack0 anim batches, and pack1 effect scripts - all with **absolute**
/// file offsets (the shape verified against PROT 0873).
#[test]
fn catalog_from_efect_dat_parses_packs_atlas_and_anims() {
    let mut buf = Vec::new();
    // Reserve header (filled at the end).
    buf.extend_from_slice(&[0u8; 8]);
    // Inline atlas: 2 entries (u, v, w, h, u16 CLUT@+4, u8 tpage@+6, unk).
    buf.extend_from_slice(&[0u8, 0, 32, 32]); // u=0 v=0 w=32 h=32
    buf.extend_from_slice(&0x7680u16.to_le_bytes()); // CLUT (CBA -> row 474)
    buf.extend_from_slice(&[0x25u8, 0]); // tpage byte (page 320,0 4bpp), unk
    buf.extend_from_slice(&[32u8, 0, 32, 32]); // u=32 v=0 w=32 h=32
    buf.extend_from_slice(&0x7680u16.to_le_bytes());
    buf.extend_from_slice(&[0x25u8, 0]);
    let pack0_off = buf.len() as u32; // 8 + 16 = 24

    // pack0: 1 anim batch with 2 frames.
    let p0_table = buf.len();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&[0u8; 4]); // offset[0] placeholder
    let anim0 = buf.len() as u32;
    buf.extend_from_slice(&[2u8, 0x00]); // frame_count=2, flags
    buf.extend_from_slice(&[0u8, 1, 4, 0, 0, 0]); // frame 0 (atlas_index 0)
    buf.extend_from_slice(&[1u8, 1, 4, 0, 0, 0]); // frame 1 (atlas_index 1)
    buf[p0_table + 4..p0_table + 8].copy_from_slice(&anim0.to_le_bytes());
    let pack1_off = buf.len() as u32;

    // pack1: 1 effect script with 2 children.
    let p1_table = buf.len();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&[0u8; 4]); // offset[0] placeholder
    let script0 = buf.len() as u32;
    buf.extend_from_slice(&[2u8, 0x00]); // child_count=2, flags
    buf.extend_from_slice(&0i16.to_le_bytes()); // spread
    // Two 14-byte spawn records: (anim_batch, delay) byte pair then the
    // offset/velocity i16 fields (see docs/formats/effect.md).
    for (batch, delay) in [(2u8, 3u8), (2u8, 0u8)] {
        buf.extend_from_slice(&[batch, delay]);
        buf.extend_from_slice(&5i16.to_le_bytes()); // offset leg A ("width")
        buf.extend_from_slice(&(-7i16).to_le_bytes()); // height
        buf.extend_from_slice(&9i16.to_le_bytes()); // offset leg B ("depth")
        buf.extend_from_slice(&1i16.to_le_bytes()); // vel leg A
        buf.extend_from_slice(&2i16.to_le_bytes()); // vel Y
        buf.extend_from_slice(&3i16.to_le_bytes()); // vel leg B
    }
    buf[p1_table + 4..p1_table + 8].copy_from_slice(&script0.to_le_bytes());

    buf[0..4].copy_from_slice(&pack0_off.to_le_bytes());
    buf[4..8].copy_from_slice(&pack1_off.to_le_bytes());

    let cat = EffectCatalog::from_efect_dat_bytes(&buf);
    assert_eq!(cat.len(), 1, "one effect script");
    assert_eq!(cat.atlas().len(), 2, "two atlas entries");
    assert_eq!(cat.atlas()[1].u, 32);
    assert_eq!(cat.atlas()[0].w, 32);
    assert_eq!(cat.atlas()[0].h, 32);
    assert_eq!(cat.atlas()[0].clut, 0x7680, "CBA is the u16 at atlas+4");
    assert_eq!(cat.atlas()[0].page, 0x25, "tpage is the byte at atlas+6");
    assert_eq!(cat.anim_count(), 1);
    let batch = cat.anim(0).expect("anim batch 0");
    assert_eq!(batch.frames.len(), 2);
    assert_eq!(batch.frames[1].atlas_index, 1);
    let (script, children) = cat.entry(0).unwrap();
    assert_eq!(script.child_count, 2);
    // The anim index is the single byte at +0 - NOT a u16 spanning the
    // delay byte at +1 (the retail walker reads `lbu`).
    assert_eq!(children[0].sprite_id, 2);
    assert_eq!(children[0].delay, 3);
    assert_eq!(children[1].sprite_id, 2);
    assert_eq!(children[1].delay, 0);
    assert_eq!(children[0].width, 5);
    assert_eq!(children[0].height, -7);
    assert_eq!(children[0].depth, 9);
    assert_eq!(children[0].velocity, [1, 2, 3]);
}

#[test]
fn catalog_from_efect_dat_empty_on_truncated() {
    assert!(EffectCatalog::from_efect_dat_bytes(&[0u8; 4]).is_empty());
    // pack0_offset past EOF.
    let mut buf = vec![0u8; 8];
    buf[0..4].copy_from_slice(&0xFFFFu32.to_le_bytes());
    assert!(EffectCatalog::from_efect_dat_bytes(&buf).is_empty());
}

/// `is_summon_effect` short-circuits BEFORE consuming a master slot.
/// Verifies that a summon dispatch leaves the pool fully empty (no
/// allocator call, no child population). Guards against accidentally
/// committing pool state on the summon path.
#[test]
fn summon_path_does_not_consume_master_slot() {
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.summon_ids.insert(4);

    let r = pool.spawn(
        &mut host,
        4,
        [10, 20, 30],
        0x123,
        &EffectScript::default(),
        &[],
    );
    assert_eq!(r, None);

    // Every slot must remain empty.
    for m in &pool.master_slots {
        assert_eq!(m.child_count, 0);
        assert_eq!(m.pos_x, 0);
    }
    // Allocator should still hand out slot 0 on a non-summon spawn.
    assert_eq!(pool.allocate_master(), Some(0));
}
