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
    pool.children[5].pos[0] = 0xDEAD_BEEFu32 as i32;

    pool.init(PoolHead {
        motion_scale: 0x1000,
        sprite_scale: 0x0A00,
        atlas_base: 0x1234_0000,
        pack0_base: 0x1234_4000,
        pack1_base: 0x1234_8000,
    });

    assert_eq!(pool.master_slots[0].child_count, 0);
    assert_eq!(pool.children[5].pos[0], 0);
    assert_eq!(pool.head.motion_scale, 0x1000);
    assert_eq!(pool.head.sprite_scale, 0x0A00);
    assert_eq!(pool.head.atlas_base, 0x1234_0000);
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

// ---------------------------------------------------------------------------
// Faithful walker tests - `Pool::tick_retail` (pass 1) and
// `Pool::child_billboards` (pass 2), against the algebra in
// `docs/subsystems/effect-vm.md#the-extracted-pass-1-state-algebra`.
// ---------------------------------------------------------------------------

/// One synthetic spawn record: `sprite_id` selects the pack0 batch, `delay`
/// is the master's post-spawn wait (frames).
fn rec(sprite_id: u16, delay: u8) -> ChildSprite {
    ChildSprite {
        sprite_id,
        delay,
        ..ChildSprite::default()
    }
}

/// Build a catalog: one effect script per `effects` entry (flags + spawn
/// records, spread 16), one pack0 batch per `anims` entry (frames as
/// `(atlas_index, delay, speed)`), plus the sprite atlas.
fn walker_catalog(
    effects: &[(u8, &[ChildSprite])],
    anims: &[&[(u8, u8, u8)]],
    atlas: &[SpriteAtlasEntry],
) -> EffectCatalog {
    let entries = effects
        .iter()
        .map(|(flags, recs)| {
            (
                EffectScript {
                    child_count: recs.len() as u8,
                    flags: *flags,
                    spread: 16,
                    body: vec![],
                },
                recs.to_vec(),
            )
        })
        .collect();
    let anims = anims
        .iter()
        .map(|frames| AnimBatch {
            flags: 0,
            frames: frames
                .iter()
                .map(|&(a, d, s)| AnimFrame {
                    atlas_index: a,
                    timing: [d, s, 0, 0, 0],
                })
                .collect(),
        })
        .collect();
    EffectCatalog::from_parts(entries, atlas.to_vec(), anims)
}

/// Master spawn cadence: a record's delay arms the 5.3 wait counter
/// (`delay << 3`), each tick subtracts 8, and the next record is consumed
/// only at zero. Consuming the final record frees the master.
#[test]
fn retail_master_cadence_walks_records_on_5_3_countdown() {
    let records = [rec(0, 3), rec(0, 9)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    // Tick 1: record 0 consumed (child seeded), wait = 3 << 3 = 24.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.master_slots[0].spawn_cursor, 1);
    assert_eq!(pool.master_slots[0].state, 24);
    assert_eq!(pool.active_child_count(), 1);

    // Ticks 2..4: countdown 24 -> 16 -> 8 -> 0, no record consumed.
    for want in [16u8, 8, 0] {
        pool.tick_retail(&mut host, &catalog, 1);
        assert_eq!(pool.master_slots[0].state, want);
        assert_eq!(pool.master_slots[0].spawn_cursor, 1);
    }

    // Tick 5: wait hit zero -> record 1 consumed; it was the last record, so
    // the master frees itself and forces wait = 8 to exit the burst loop.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.master_slots[0].state, 8);
    assert_eq!(pool.active_child_count(), 2);
}

/// A wait already below 8 clamps to zero (no spawn on the clamping tick).
#[test]
fn retail_master_wait_below_8_clamps_without_spawning() {
    let records = [rec(0, 1)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");
    pool.master_slots[0].state = 5; // sub-8 residue

    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.master_slots[0].state, 0, "clamped, not wrapped");
    assert_eq!(pool.master_slots[0].spawn_cursor, 0, "no record consumed");
    assert_eq!(pool.active_child_count(), 0);
}

/// Zero-delay records spawn as one burst within a single tick.
#[test]
fn retail_zero_delay_records_burst_in_one_tick() {
    let records = [rec(0, 0), rec(0, 0), rec(0, 5)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    pool.tick_retail(&mut host, &catalog, 1);
    // All three records consumed in one sweep (the third was the last, so
    // the master freed itself); three children live.
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.active_child_count(), 3);
}

/// The `frame_skip` catch-up factor runs pass 1 that many times per call.
#[test]
fn retail_frame_skip_replays_the_sweep() {
    let records = [rec(0, 3), rec(0, 9)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    // One call at frame_skip 4 = spawn record 0 (wait 24) + three countdown
    // sweeps -> state back to 0.
    pool.tick_retail(&mut host, &catalog, 4);
    assert_eq!(pool.master_slots[0].state, 0);
    assert_eq!(pool.master_slots[0].spawn_cursor, 1);
    assert_eq!(pool.active_child_count(), 1);
}

/// On pool exhaustion the spawn record is still consumed with no child -
/// the effect degrades rather than stalling.
#[test]
fn retail_pool_exhaustion_consumes_record_without_child() {
    let records = [rec(5, 1)];
    let frames: &[(u8, u8, u8)] = &[(0, 10, 0)];
    let catalog = walker_catalog(&[(0, &records)], &[frames; 6], &[]); // batches 0..=5
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    // Fill every child slot with a long-waiting occupant.
    for c in &mut pool.children {
        c.frame_count = 1;
        c.wait = 200;
        c.anim_id = 0;
    }
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    pool.tick_retail(&mut host, &catalog, 1);
    // Record consumed -> single-record master freed; no slot was reseeded.
    assert_eq!(pool.active_count(), 0);
    assert!(pool.children.iter().all(|c| c.anim_id != 5));
}

/// Child seeding reads the anim index as the single byte at record +0 and
/// the master delay from the byte at +1 (the `ChildSprite` u16-read
/// regression), and arms the child wait from the first frame's delay.
#[test]
fn retail_child_seed_uses_anim_byte_and_first_frame_delay() {
    let records = [rec(1, 2)];
    let catalog = walker_catalog(
        &[(0, &records)],
        &[&[(0, 9, 0)], &[(0, 4, 0), (0, 1, 0)]],
        &[],
    );
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    pool.tick_retail(&mut host, &catalog, 1);
    // Master armed from the record's delay byte (2 << 3 = 16)... but the
    // single-record master frees itself instead (state = 8). The child must
    // come from batch 1, not batch (1 | 2 << 8).
    let c = &pool.children[0];
    assert_eq!(c.anim_id, 1);
    assert_eq!(c.frame_count, 2);
    // Seeded wait = batch 1 frame 0 delay << 3 = 32, then the same sweep's
    // child walk already counted it down once: 32 - 8 = 24.
    assert_eq!(c.wait, 24);
    assert_eq!(c.frame_cursor, 0);
}

/// Child frame advance: each frame holds `delay << 3` in 5.3, the cursor
/// steps at zero, and reaching `frame_count` retires the slot.
#[test]
fn retail_child_walk_advances_frames_and_retires() {
    // Batch 0: three frames, delays 1 / 2 / 1.
    let records = [rec(0, 0)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 1, 0), (1, 2, 0), (2, 1, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    // Tick 1: seed (wait = 1 << 3 = 8), then the child walk counts 8 -> 0.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].frame_cursor, 0);
    assert_eq!(pool.children[0].wait, 0);
    // Tick 2: advance to frame 1, wait = 2 << 3 = 16.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].frame_cursor, 1);
    assert_eq!(pool.children[0].wait, 16);
    // Ticks 3-4: countdown 16 -> 8 -> 0.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].wait, 8);
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].wait, 0);
    // Tick 5: advance to frame 2, wait = 8.
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].frame_cursor, 2);
    assert_eq!(pool.children[0].wait, 8);
    // Tick 6: countdown to 0. Tick 7: cursor would reach frame_count -> retire.
    pool.tick_retail(&mut host, &catalog, 1);
    pool.tick_retail(&mut host, &catalog, 1);
    assert_eq!(pool.children[0].frame_count, 0, "slot retired");
    assert_eq!(pool.active_child_count(), 0);
}

/// Motion: `pos += vel * frame.speed * motion_scale * 8 >> 15`, which at the
/// retail scale (0x1000) is exactly `vel * speed` - and it integrates on
/// countdown ticks too (drift during a frame hold).
#[test]
fn retail_child_motion_reduces_to_vel_times_speed_and_drifts_during_wait() {
    // One record, vertical velocity 16; batch frames all speed 2, delays 1.
    let mut r = rec(0, 0);
    r.velocity = [0, 16, 0];
    let records = [r];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 1, 2), (0, 1, 2), (0, 1, 2)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");

    // Per-tick motion steps: every tick (advance or countdown) takes exactly
    // one step of 16 * 2 = 32 (16.8 units) until retirement.
    let mut last = 0i32;
    let mut steps = 0;
    for _ in 0..16 {
        pool.tick_retail(&mut host, &catalog, 1);
        if pool.children[0].frame_count == 0 {
            break;
        }
        let y = pool.children[0].pos[1];
        assert_eq!(y - last, 32, "one vel*speed step per logic frame");
        last = y;
        steps += 1;
    }
    assert!(steps >= 4, "drifted across advance AND countdown ticks");
}

/// Spawn-record planar legs rotate by the master angle through the 4096-entry
/// trig tables: at angle 0 the width leg maps to +X (exactly `width << 8` in
/// 16.8) and at 0x400 (90 degrees) to -Z, with the table's one-index skew
/// (`table[0xFFF - a]`) leaking a tiny cross-axis term.
#[test]
fn retail_child_offsets_rotate_by_master_angle() {
    let mut r = rec(0, 1);
    r.width = 16;
    r.height = 2;
    let records = [r];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);

    // Angle 0.
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [1000, 500, 2000], 0, &catalog)
        .expect("spawn");
    pool.tick_retail(&mut host, &catalog, 1);
    let c = &pool.children[0];
    assert_eq!(c.pos[0], (1000 << 8) + (16 << 8), "width -> +X at angle 0");
    assert_eq!(c.pos[1], (500 << 8) - (2 << 8), "height subtracts from Y");
    assert_eq!(c.pos[2], (2000 << 8) - 6, "cross-axis skew from sin[0xFFF]");

    // Angle 0x400 (90 degrees).
    let mut pool = Pool::new();
    pool.spawn_by_ui_id(&mut host, 0, [1000, 500, 2000], 0x400, &catalog)
        .expect("spawn");
    pool.tick_retail(&mut host, &catalog, 1);
    let c = &pool.children[0];
    assert_eq!(c.pos[0], (1000 << 8) - 6, "cross-axis skew from cos[0xBFF]");
    assert_eq!(
        c.pos[2],
        (2000 << 8) - (16 << 8),
        "width -> -Z at 90 degrees"
    );
}

/// Brightness envelope at key ticks: ramp-in over the first eighth
/// (`0x80 * (cursor + 1) / n`), ramp-out over the rest, clamp at 0x80.
#[test]
fn pass2_brightness_envelope_key_ticks() {
    // frame_count = 16 -> n = 2.
    assert_eq!(pass2_brightness(16, 0), 0x40); // (0+1)*0x80/2
    assert_eq!(pass2_brightness(16, 1), 0x80); // (1+1)*0x80/2
    assert_eq!(pass2_brightness(16, 2), 0x80); // (16-2)*0x80/14 -> clamp
    assert_eq!(pass2_brightness(16, 8), 0x49); // (16-8)*0x80/14 = 73
    assert_eq!(pass2_brightness(16, 15), 0x09); // (16-15)*0x80/14 = 9
    // frame_count < 8 -> n = 0: pure ramp-out over the whole batch.
    assert_eq!(pass2_brightness(4, 0), 0x80);
    assert_eq!(pass2_brightness(4, 3), 0x20);
}

/// Pass 2 resolves each live child's current frame to its atlas entry,
/// scales the quad by the pool sprite scale (x10 at the retail 0xA00), and
/// exposes the random UV-mirror bits.
#[test]
fn retail_child_billboards_resolve_atlas_scale_and_mirror() {
    let atlas = [
        SpriteAtlasEntry {
            u: 8,
            v: 16,
            w: 4,
            h: 6,
            clut: 0x7680,
            page: 0x25,
            unk: 0,
        },
        SpriteAtlasEntry {
            u: 64,
            v: 0,
            w: 8,
            h: 8,
            clut: 0x7680,
            page: 0x25,
            unk: 0,
        },
    ];
    let records = [rec(0, 1)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(1, 4, 0), (0, 4, 0)]], &atlas);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    host.rng_seq = vec![3]; // mirror bits = 3 -> both bits SET = unflipped
    pool.spawn_by_ui_id(&mut host, 0, [10, 20, 30], 0, &catalog)
        .expect("spawn");
    pool.tick_retail(&mut host, &catalog, 1);

    let bills = pool.child_billboards(&catalog);
    assert_eq!(bills.len(), 1);
    let b = &bills[0];
    // Frame 0's atlas index is 1.
    assert_eq!(b.atlas_index, 1);
    assert_eq!(b.entry, atlas[1]);
    // 8x8 texels * 0xA00 >> 8 = x10.
    assert_eq!((b.world_w, b.world_h), (80, 80));
    // frame_count = 2 -> n = 0 -> (2-0)*0x80/2 = 0x80.
    assert_eq!(b.brightness, 0x80);
    // Mirror bits SET = the retail "unswapped" corner order.
    assert!(!b.flip_h);
    assert!(!b.flip_v);
    // Position: 16.8 master origin >> 8 (angle 0, zero legs => tiny Z skew
    // truncates away: 30<<8 - 0 legs; width/depth are 0 here).
    assert_eq!(b.pos, [10, 20, 30]);

    // Clear-bit mirror reads as flipped.
    let mut pool2 = Pool::new();
    let mut host2 = RecHost::default(); // rng defaults to 0 -> bits clear
    pool2
        .spawn_by_ui_id(&mut host2, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");
    pool2.tick_retail(&mut host2, &catalog, 1);
    let b2 = pool2.child_billboards(&catalog);
    assert!(b2[0].flip_h);
    assert!(b2[0].flip_v);
}

/// The randomized planar legs (`flags & 1`) recorded at spawn replace the
/// record's width/depth when the walker seeds the child.
#[test]
fn retail_randomized_offsets_override_record_legs() {
    let mut r = rec(0, 1);
    r.width = 100; // authored legs, must be ignored under flags & 1
    r.depth = 100;
    let records = [r];
    let catalog = walker_catalog(&[(1, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    // Spawn rewrites: dx = 20 % 32 - 16 = 4, dz = 24 % 32 - 16 = 8.
    // The walker then consumes one more sample for the mirror bits.
    host.rng_seq = vec![20, 24, 0];
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");
    assert_eq!(pool.master_slots[0].child_offsets, vec![(4, 8)]);

    pool.tick_retail(&mut host, &catalog, 1);
    let c = &pool.children[0];
    // Angle 0: X = dx << 8 (width leg), Z = dz << 8 + the sin[0xFFF] skew of
    // the width leg (4 * -6 >> 4 = -2, floored).
    assert_eq!(c.pos[0], 4 << 8);
    assert_eq!(c.pos[2], (8 << 8) + ((4 * -6) >> 4));
}

/// `field_14` ages once per `tick_retail` call per active master (a
/// port-side aid for age-based render fades; retail leaves +0x14 unwritten).
#[test]
fn tick_retail_ages_active_masters() {
    let records = [rec(0, 10), rec(0, 10)];
    let catalog = walker_catalog(&[(0, &records)], &[&[(0, 10, 0)]], &[]);
    let mut pool = Pool::new();
    let mut host = RecHost::default();
    pool.spawn_by_ui_id(&mut host, 0, [0, 0, 0], 0, &catalog)
        .expect("spawn");
    for want in 1..=3 {
        pool.tick_retail(&mut host, &catalog, 1);
        assert_eq!(pool.master_slots[0].field_14, want);
    }
}
