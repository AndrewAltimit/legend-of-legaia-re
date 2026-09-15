//! Op-level oracle for the scripted-motion VM's non-facing arms
//! (`FUN_80038158` ops `0x02`, `0x06`..`0x0C`, `0x0E`..`0x16`).
//!
//! Every assertion here is read off the retail disassembly, not off the
//! port: the operand decodes, the "consumes the tick" answer per arm, and
//! the guards each arm applies before it writes anything.

use legaia_engine_vm::ambient_motion::{AmbientMotion, AmbientTick, NeverBlocks, RAMP_DEST_SCALE};
use legaia_engine_vm::ambient_motion_ops::{
    ACTOR_FLAG_TRANSLUCENT, ACTOR_FLAG_Y_OVERRIDE, AmbientEffect, BitTarget, ModelBank,
};

fn vm() -> AmbientMotion {
    AmbientMotion::new(1, 0x000)
}

/// `0x07` / `0x08` write the named flag and nothing else, and neither
/// consumes the tick - so a stream can raise a flag and keep running in the
/// same frame (`0x800390D0` / `0x80039114` both end at `0x80039B34`, the
/// plain PC advance).
#[test]
fn flag_ops_emit_one_effect_and_do_not_yield() {
    // `[07 34 12][08 34 12][05 02]` - set 0x1234, clear it, then wait.
    let code = [0x07u8, 0x34, 0x12, 0x08, 0x34, 0x12, 0x05, 0x02];
    let mut m = vm();
    assert_eq!(m.tick(&code, 1), AmbientTick::Yield);
    assert_eq!(
        m.effects,
        vec![
            AmbientEffect::SystemFlagSet(0x1234),
            AmbientEffect::SystemFlagClear(0x1234),
        ]
    );
    // Both flag ops ran in one tick and the wait is what stopped it.
    assert_eq!(m.pc, 6);
    // Nothing else moved.
    assert_eq!(m.heading, 0x000);
    assert_eq!((m.x, m.z), (0, 0));
    assert_eq!(m.actor_flags, 0);
}

/// `0x0F` lands on the tile centre `tile * 128 + 64`, re-anchors `+0x8C` /
/// `+0x8D`, and adds a further half tile per high bit.
#[test]
fn teleport_lands_on_the_tile_centre() {
    let code = [0x0Fu8, 0x05, 0x03];
    let mut m = vm();
    m.tick(&code, 1);
    assert_eq!((m.x, m.z), (5 * 128 + 64, 3 * 128 + 64));
    assert_eq!(m.home_tile, (5, 3));
    assert_eq!(
        m.effects,
        vec![AmbientEffect::Teleport {
            x: 704,
            z: 448,
            sample_ground: true
        }]
    );

    let code = [0x0Fu8, 0x85, 0x83];
    let mut m = vm();
    m.tick(&code, 1);
    assert_eq!((m.x, m.z), (5 * 128 + 64 + 64, 3 * 128 + 64 + 64));
    assert_eq!(m.home_tile, (5, 3), "the anchor is the masked tile only");

    // The Y-override flag suppresses the ground resample.
    let code = [0x0Fu8, 0x01, 0x01];
    let mut m = vm();
    m.actor_flags = ACTOR_FLAG_Y_OVERRIDE;
    m.tick(&code, 1);
    assert!(matches!(
        m.effects[0],
        AmbientEffect::Teleport {
            sample_ground: false,
            ..
        }
    ));
}

/// The five reachable bit targets, and the sense of the `0x30` nibble:
/// non-zero picks the **low** halfword of a two-halfword word, in both the
/// actor-flag and the scratchpad case.
#[test]
fn bit_selector_picks_the_five_targets() {
    assert_eq!(BitTarget::from_selector(0x10), BitTarget::ActorFlagsLo);
    assert_eq!(BitTarget::from_selector(0x00), BitTarget::ActorFlagsHi);
    assert_eq!(BitTarget::from_selector(0x40), BitTarget::ClipControl);
    assert_eq!(BitTarget::from_selector(0x7F), BitTarget::ClipControl);
    assert_eq!(BitTarget::from_selector(0x90), BitTarget::GlobalsLo);
    assert_eq!(BitTarget::from_selector(0x80), BitTarget::GlobalsHi);
    assert_eq!(BitTarget::from_selector(0xC0), BitTarget::Fault);
}

/// `0x10` / `0x11` move exactly one bit of exactly one halfword.
#[test]
fn bit_set_and_clear_touch_one_bit_of_the_selected_half() {
    // `0x10 0x13`: actor flags, low half, bit 3.
    let mut m = vm();
    m.actor_flags = 0xDEAD_0000;
    m.tick(&[0x10u8, 0x13], 1);
    assert_eq!(m.actor_flags, 0xDEAD_0008);
    m.pc = 0;
    m.tick(&[0x11u8, 0x13], 1);
    assert_eq!(m.actor_flags, 0xDEAD_0000);

    // `0x10 0x03`: the same bit index in the HIGH half.
    let mut m = vm();
    m.tick(&[0x10u8, 0x03], 1);
    assert_eq!(m.actor_flags, 0x0008_0000);

    // `0x10 0x94`: globals, low half, bit 4; then the high half.
    let mut m = vm();
    m.globals = 0x0000_0001;
    m.tick(&[0x10u8, 0x94], 1);
    assert_eq!(m.globals, 0x0000_0011);
    m.pc = 0;
    m.tick(&[0x10u8, 0x84], 1);
    assert_eq!(m.globals, 0x0010_0011);

    // `0x40` - the clip-control word.
    let mut m = vm();
    m.tick(&[0x10u8, 0x45], 1);
    assert_eq!(m.clip_control, 0x0020);

    // `0xC0` - retail's null-pointer arm: nothing is written.
    let mut m = vm();
    m.actor_flags = 0x1234_5678;
    m.globals = 0x8765_4321;
    m.tick(&[0x10u8, 0xC1], 1);
    assert_eq!(m.actor_flags, 0x1234_5678);
    assert_eq!(m.globals, 0x8765_4321);
    assert_eq!(m.effects, vec![AmbientEffect::BitTargetFault]);
}

/// `0x12` waits for the bit to **change**, whichever way it starts, and
/// consumes the tick on every frame including the one it retires on
/// (`addiu s8, s8, 1` sits in the delay slot at `0x80039634`).
#[test]
fn bit_wait_waits_for_a_change_in_either_direction() {
    // Bit clear at install -> wait for a rising edge.
    let code = [0x12u8, 0x12, 0x05, 0x01];
    let mut m = vm();
    assert_eq!(m.tick(&code, 1), AmbientTick::Yield);
    assert_eq!(m.pc, 0, "still waiting");
    assert_eq!(m.cursor & 0xFF, 2, "seeded for a rising edge");
    m.tick(&code, 1);
    assert_eq!(m.pc, 0);
    m.actor_flags |= 1 << 2;
    assert_eq!(m.tick(&code, 1), AmbientTick::Yield);
    assert_eq!(m.pc, 2, "retired, but the tick was still consumed");
    assert_eq!(m.cursor & 0xFF, 0);

    // Bit already set at install -> wait for a falling edge.
    let mut m = vm();
    m.actor_flags = 1 << 2;
    m.tick(&code, 1);
    assert_eq!(m.cursor & 0xFF, 1, "seeded for a falling edge");
    assert_eq!(m.pc, 0);
    m.actor_flags = 0;
    m.tick(&code, 1);
    assert_eq!(m.pc, 2);
}

/// `0x02` writes the requested-move pair only while the default-move record
/// still holds the `0x8C` sentinel; `0x0A` / `0x0B` are gated the same way
/// and additionally move the translucency bit.
#[test]
fn move_pair_ops_are_gated_on_the_unset_record() {
    let mut m = vm();
    m.tick(&[0x02u8, 0x21, 0x00, 0x05, 0x01], 1);
    assert_eq!(m.move_pair, Some(0x21));
    assert_eq!(m.requested_move, Some(0x21));

    let mut m = vm();
    m.default_move = [0x07, 0x09];
    m.tick(&[0x02u8, 0x21, 0x00, 0x05, 0x01], 1);
    assert_eq!(m.move_pair, None, "a record outranks the op");

    let mut m = vm();
    m.tick(&[0x0Au8, 0x21, 0x00, 0x05, 0x01], 1);
    assert_eq!(m.actor_flags, ACTOR_FLAG_TRANSLUCENT);
    assert_eq!(m.move_pair, Some(0x21));
    m.pc = 0;
    m.tick(&[0x0Bu8, 0x22, 0x00, 0x05, 0x01], 1);
    assert_eq!(m.actor_flags, 0);
    assert_eq!(m.move_pair, Some(0x22));

    let mut m = vm();
    m.default_move = [0x07, 0x09];
    m.tick(&[0x0Au8, 0x21, 0x00, 0x05, 0x01], 1);
    assert_eq!(m.actor_flags, 0, "the gate covers the flag bit too");
}

/// `0x0E` splits the id space **unsigned** at `0xF0` and derives the
/// translucency bit from which side it landed on.
#[test]
fn model_swap_splits_at_0xf0() {
    let mut m = vm();
    m.actor_flags = ACTOR_FLAG_TRANSLUCENT;
    m.tick(&[0x0Eu8, 0x12, 0x00], 1);
    assert_eq!(
        m.effects,
        vec![AmbientEffect::ModelSwap {
            bank: ModelBank::Scene,
            offset: 0x12
        }]
    );
    assert_eq!(m.actor_flags, 0);

    let mut m = vm();
    m.tick(&[0x0Eu8, 0xF3, 0x00], 1);
    assert_eq!(
        m.effects,
        vec![AmbientEffect::ModelSwap {
            bank: ModelBank::Special,
            offset: 3
        }]
    );
    assert_eq!(m.actor_flags, ACTOR_FLAG_TRANSLUCENT);
}

/// `0x13` assembles the libgpu `RECT` from four little-endian halfwords and
/// sign-extends the destination corner.
#[test]
fn move_image_assembles_the_rect() {
    let code = [
        0x13u8, 0x40, 0x00, 0x10, 0x00, 0x20, 0x00, 0x08, 0x00, 0x00, 0x01, 0xF0, 0xFF,
    ];
    let mut m = vm();
    m.tick(&code, 1);
    assert_eq!(
        m.effects,
        vec![AmbientEffect::MoveImage {
            rect: [0x40, 0x10, 0x20, 0x08],
            dx: 0x100,
            dy: -16,
        }]
    );
    assert_eq!(m.pc, 13);
}

/// `0x14` / `0x15` / `0x16` write outright at duration zero and install a
/// scheduler slot otherwise.
#[test]
fn scalar_tweens_write_or_ramp() {
    let mut m = vm();
    m.tick(&[0x14u8, 0x00, 0x08, 0x00, 0x00], 1);
    assert_eq!(m.scale, Some(0x800));
    assert_eq!(m.ramps.active(), 0);

    let mut m = vm();
    m.tick(&[0x15u8, 0x00, 0x02, 0x00, 0x00], 1);
    assert_eq!(m.pitch, 0x200);
    let mut m = vm();
    m.tick(&[0x16u8, 0x00, 0xFE, 0x00, 0x00], 1);
    assert_eq!(m.roll, -0x200);

    // Ramped: from the spawn default 0x1000 down to 0x800 over 8 units.
    let code = [0x14u8, 0x00, 0x08, 0x08, 0x00, 0x05, 0x20];
    let mut m = vm();
    m.tick(&code, 1);
    assert!(m.ramps.is_driving(RAMP_DEST_SCALE));
    for _ in 0..8 {
        m.tick(&code, 1);
    }
    assert_eq!(m.scale, Some(0x800), "the endpoint is stored verbatim");
    assert_eq!(m.ramps.active(), 0);
}

/// `0x0C`'s four branch shapes (`0x800391BC` / `0x800391F4` / `0x80039220`).
#[test]
fn tint_fade_branch_shapes() {
    // Duration 0: both written outright.
    let mut m = vm();
    m.tick(&[0x0Cu8, 0x11, 0x22, 0x33, 0x05, 0x00, 0x00, 0x00], 1);
    assert_eq!(m.tint, 0x0033_2211);
    assert_eq!(m.blend, 5);
    assert_eq!(m.ramps.active(), 0);

    // Duration non-zero, mode word zero: colour snaps, mode ramps up.
    let mut m = vm();
    m.tick(&[0x0Cu8, 0x11, 0x22, 0x33, 0x05, 0x00, 0x10, 0x00], 1);
    assert_eq!(m.tint, 0x0033_2211, "fade-in snaps the colour");
    assert_eq!(m.ramps.active(), 1);

    // Duration non-zero, mode live, operand zero: colour untouched.
    let mut m = vm();
    m.blend = 4;
    m.tint = 0x0080_8080;
    m.tick(&[0x0Cu8, 0x11, 0x22, 0x33, 0x00, 0x00, 0x10, 0x00], 1);
    assert_eq!(m.tint, 0x0080_8080, "fade-out leaves the colour standing");
    assert_eq!(m.ramps.active(), 1);

    // Both live: two slots.
    let mut m = vm();
    m.blend = 4;
    m.tick(&[0x0Cu8, 0x11, 0x22, 0x33, 0x05, 0x00, 0x10, 0x00], 1);
    assert_eq!(m.ramps.active(), 2);
}

/// `0x06` walks exactly one 128-unit tile and then retires, whatever pace it
/// was authored at - `(4 << bits)` ticks of `0x80 >> (bits + 2)` units.
#[test]
fn home_step_walks_one_tile_then_retires() {
    for bits in 0u8..4 {
        // A box four tiles wide in every direction around the anchor: the
        // low seven bits are signed tile deltas, so `0x7C` is -4.
        let hi = |v: u8, b: u8| v | (((bits >> b) & 1) << 7);
        let code = [
            0x06u8,
            hi(0x7C, 3),
            hi(0x7C, 2),
            hi(0x03, 1),
            hi(0x03, 0),
            0x05,
            0x01,
        ];
        let mut m = vm();
        m.home_tile = (0x10, 0x10);
        m.x = 0x10 * 128 + 64;
        m.z = 0x10 * 128 + 64;
        let (sx, sz) = (m.x, m.z);
        let mut ticks = 0;
        while m.pc == 0 && ticks < 200 {
            assert_eq!(m.tick(&code, 1), AmbientTick::Yield);
            ticks += 1;
        }
        assert_eq!(m.pc, 5, "bits={bits}: the op retires");
        assert_eq!(ticks, 4 << bits, "bits={bits}: tick count");
        let moved = (m.x - sx).abs() + (m.z - sz).abs();
        assert_eq!(moved, 128, "bits={bits}: one full tile of travel");
    }
}

/// A draw that would leave the home box retires the op on its first tick
/// without moving the actor (`0x80038A18`).
#[test]
fn home_step_rejects_a_draw_that_leaves_the_box() {
    // A zero-width box: every cardinal leaves it.
    let code = [0x06u8, 0x00, 0x00, 0x00, 0x00, 0x05, 0x01];
    let mut m = vm();
    m.home_tile = (0x10, 0x10);
    m.x = 0x10 * 128 + 64;
    m.z = 0x10 * 128 + 64;
    let (sx, sz) = (m.x, m.z);
    assert_eq!(m.tick(&code, 1), AmbientTick::Yield);
    assert_eq!(m.pc, 5, "retired on the rejected draw");
    assert_eq!((m.x, m.z), (sx, sz));
}

/// The point of the table having one executing home: a stream that mixes
/// consuming and non-consuming ops advances exactly as far as the first
/// consuming op each frame.
#[test]
fn mixed_stream_advances_to_the_first_consuming_op() {
    // `[17 05 06][07 01 00][10 13][05 02][08 01 00][01]`
    //   default-move, flag set, bit set  -> none consume
    //   wait 2                           -> consumes
    let code = [
        0x17u8, 0x05, 0x06, 0x07, 0x01, 0x00, 0x10, 0x13, 0x05, 0x02, 0x08, 0x01, 0x00, 0x01,
    ];
    let mut m = vm();
    assert_eq!(m.tick_with(&code, 1, &NeverBlocks), AmbientTick::Yield);
    assert_eq!(m.default_move, [0x05, 0x06]);
    assert_eq!(m.effects, vec![AmbientEffect::SystemFlagSet(1)]);
    assert_eq!(m.actor_flags, 0x0008);
    assert_eq!(m.pc, 8, "parked on the wait");
    // The wait's own second tick only expires the countdown: the arm always
    // consumes, so nothing past it runs this frame.
    m.tick_with(&code, 1, &NeverBlocks);
    assert_eq!(m.pc, 10);
    assert!(m.effects.is_empty());
    // The third tick runs the clear, the restart, and the whole non-consuming
    // run again before parking on the wait a second time.
    m.tick_with(&code, 1, &NeverBlocks);
    assert_eq!(
        m.effects,
        vec![
            AmbientEffect::SystemFlagClear(1),
            AmbientEffect::SystemFlagSet(1),
        ]
    );
    assert_eq!(m.pc, 8);
    assert_eq!(m.default_move, [0x05, 0x06]);
}

/// Effects do not accumulate across ticks - the host drains a fresh list
/// each frame.
#[test]
fn effects_reset_per_tick() {
    let code = [0x07u8, 0x01, 0x00, 0x05, 0x02, 0x01];
    let mut m = vm();
    m.tick(&code, 1);
    assert_eq!(m.effects.len(), 1);
    m.tick(&code, 1);
    assert!(m.effects.is_empty(), "the wait tick raised nothing");
}
