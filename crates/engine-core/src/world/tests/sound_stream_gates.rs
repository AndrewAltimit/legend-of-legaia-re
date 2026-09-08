//! Field-VM op `0x36`'s two stream gates (`0x801E030C..0x801E0444`).
//!
//! The op's bit-15-set sub-switch and its bit-15-clear XA arm both consult
//! the side-band bank request/acknowledge pair `_DAT_8007BABC` /
//! `_DAT_8007BAA0`, and the dev/dual-mode word `_DAT_8007B868` moves that
//! gate in opposite directions on the two halves. Every case here is a
//! whole-VM run: the bytecode carries a real `36` instruction and the
//! assertion is on the resulting PC, so a halt is retail's "re-run this
//! instruction next frame".

use super::*;
use crate::scus_leaf_kernels::SoundStreamRequest;

/// One `36` instruction followed by a `00` HALT, run once from a fresh
/// world seeded with `pair` / `dual_mode`. Returns the PC afterwards; `0`
/// means the op halted at its own address.
fn run_op36(sel: u16, arg: u16, pair: SoundStreamRequest, dual_mode: i32) -> (World, usize) {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.sound_stream = pair;
    world.dual_mode_gate = dual_mode;
    let s = sel.to_le_bytes();
    let a = arg.to_le_bytes();
    world.load_field_script(vec![0x36, s[0], s[1], a[0], a[1], 0x00]);
    let _ = world.tick();
    let pc = world.field_pc;
    (world, pc)
}

/// Sub-`2` is the barrier: `beq a0,v0` at `0x801E03A8` advances past the
/// 5-byte op, and the fallthrough `j 0x801DEE50` / `move fp,s4` restores
/// the op's own PC.
#[test]
fn sub2_barrier_halts_until_the_pair_settles() {
    let unsettled = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    let (_, pc) = run_op36(0x8002, 0, unsettled, 0);
    assert_eq!(pc, 0, "an unsettled pair re-runs the barrier at its own PC");

    let settled = SoundStreamRequest {
        requested: 12,
        acked: 12,
    };
    let (_, pc) = run_op36(0x8002, 0, settled, 0);
    assert_eq!(pc, 5, "a settled pair advances past the 5-byte op");
}

/// Sub-`1` is not the unconditional store the earlier reading had: the
/// arm at `0x801E0360` takes the store only when the pair is settled **or**
/// the acknowledge cell is the `-1` idle sentinel, and halts otherwise.
#[test]
fn sub1_request_is_guarded_and_the_idle_sentinel_is_the_escape() {
    let settled = SoundStreamRequest {
        requested: 12,
        acked: 12,
    };
    let (world, pc) = run_op36(0x8001, 0x0022, settled, 0);
    assert_eq!(pc, 5);
    assert_eq!(world.sound_stream.requested, 0x22);
    assert!(
        world.sound_stream.is_settled(),
        "the synchronous host settles the request in the same call"
    );

    // Idle acknowledge: the store is taken even though the pair is not
    // settled (`bne a0,-1` at `0x801E037C` is the escape).
    let idle = SoundStreamRequest {
        requested: 12,
        acked: SoundStreamRequest::IDLE,
    };
    let (world, pc) = run_op36(0x8001, 0x0033, idle, 0);
    assert_eq!(pc, 5);
    assert_eq!(world.sound_stream.requested, 0x33);

    // Neither settled nor idle: retail parks on the instruction.
    let busy = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    let (world, pc) = run_op36(0x8001, 0x0044, busy, 0);
    assert_eq!(pc, 0);
    assert_eq!(
        world.sound_stream.requested, 12,
        "a refused request leaves the cell alone"
    );
}

/// Subs `3` and `4` carry no gate at all - the earlier reading put sub `3`
/// under the barrier and left sub `1` out.
#[test]
fn subs_3_and_4_are_ungated() {
    let busy = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    let (_, pc) = run_op36(0x8003, 0, busy, 0);
    assert_eq!(pc, 5, "sub 3 advances with the pair unsettled");

    let (world, pc) = run_op36(0x8004, 0x0030, busy, 0);
    assert_eq!(pc, 5, "sub 4 advances with the pair unsettled");
    assert_eq!(world.sfx_cue_delays.delay(0), Some(0x30));
}

/// Sub-`0`, the SFX enqueue, is gated: `bne a0,v0,0x801DEE4C` at
/// `0x801E0340` restores the PC before `FUN_80035B50` is reached.
#[test]
fn sub0_enqueue_waits_for_the_pair() {
    let busy = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    let (world, pc) = run_op36(0x8000, 0x0011, busy, 0);
    assert_eq!(pc, 0);
    assert_eq!(world.sfx_cue_cursor, 0, "the enqueue never ran");

    let settled = SoundStreamRequest::IDLE_PAIR;
    let (world, pc) = run_op36(0x8000, 0x0011, settled, 0);
    assert_eq!(pc, 5);
    assert_eq!(world.sfx_cue_cursor, 1);
}

/// The bit-15-**clear** arm carries the same barrier, which is the half
/// the earlier reading missed entirely.
#[test]
fn the_xa_arm_waits_for_the_pair_too() {
    let busy = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    let (_, pc) = run_op36(0x0001, 0x0010, busy, 0);
    assert_eq!(pc, 0, "an XA clip start parks while a bank load is pending");

    let (_, pc) = run_op36(0x0001, 0x0010, SoundStreamRequest::IDLE_PAIR, 0);
    assert_eq!(pc, 5);
}

/// `_DAT_8007B868` moves in opposite directions on the two halves: it
/// *skips* the whole bit-15-set arm and *bypasses* the bit-15-clear
/// barrier. Retail boots it `0`, so neither fires in play - the engine
/// keeps `World::dual_mode_gate` at `0` for that reason.
#[test]
fn the_dual_mode_gate_skips_one_arm_and_opens_the_other() {
    let busy = SoundStreamRequest {
        requested: 12,
        acked: 7,
    };
    // Bit-15 set: the sub-switch never runs, so a barrier that would halt
    // advances instead - and so does an enqueue, without enqueuing.
    let (world, pc) = run_op36(0x8002, 0, busy, 1);
    assert_eq!(pc, 5);
    let (world0, pc0) = run_op36(0x8000, 0x0011, busy, 1);
    assert_eq!(pc0, 5);
    assert_eq!(world0.sfx_cue_cursor, 0, "the skipped arm enqueues nothing");
    assert_eq!(world.sound_stream, busy, "and writes no cell");

    // Bit-15 clear: the same word lets the XA arm through.
    let (_, pc) = run_op36(0x0001, 0x0010, busy, 1);
    assert_eq!(pc, 5);
}

/// The engine boots the pair settled. Retail's own field init writes
/// `(8, -1)` and lets `FUN_800243F0` latch it; the port has no in-flight
/// window, so a script's first op-`0x36` must not park.
#[test]
fn a_fresh_world_boots_with_the_pair_settled() {
    let world = World::new();
    assert!(world.sound_stream.is_settled());
    assert_eq!(world.dual_mode_gate, 0);
    assert_eq!(
        SoundStreamRequest::FIELD_INIT,
        SoundStreamRequest {
            requested: 8,
            acked: -1
        }
    );
}
