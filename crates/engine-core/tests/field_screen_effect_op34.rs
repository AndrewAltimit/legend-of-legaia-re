//! Field-VM op `0x34` sub-0 - the screen-effect colour tween, end to end
//! through the world's own field-VM host.
//!
//! The arm at `0x801DFCD4..0x801DFEF8` is a walk-out / walk-in pair over one
//! live pool actor, and the one representation of the effect is the
//! per-frame `FUN_80024EE4(kind, blend, packed)` push each tween emits.
//!
//! The envelope here is pinned against a live PCSX-Redux capture of a field
//! scene entry (`captures/w1a-0921/tint_beat2`): the spawner received the
//! template `[2, 57, _, 0,0,0, _, 255,255,255, 0, 0xFFFF, 0]` with `a1 = 0`,
//! and the tween then pushed `(kind 0, blend 2)` once per step with the grey
//! lane rising `0x00, 0x0D, 0x1A, 0x28, 0x35, 0x43` on clocks
//! `0, 3, 6, 9, 12, 15`. Frame counts and lane values, not sample bytes.
//!
//! REF: FUN_801DE2B0 (the spawner), FUN_801DDC20 (the tick),
//! REF: FUN_80024EE4 (the push)

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::world::World;

/// `34 05 FF FF FF 41 00` - the instruction whose template the capture read.
/// `op0 = 5` selects blend 2 (bit 0) and push kind 0 (bit 2); the operand
/// duration is `0x41 = 65`.
const WHITE_IN: [u8; 7] = [0x34, 0x05, 0xFF, 0xFF, 0xFF, 0x41, 0x00];

fn run(w: &mut World, insn: &[u8]) {
    w.field_bytecode = insn.to_vec();
    w.field_pc = 0;
    w.step_field().expect("the script stepped");
}

fn tweens(w: &World) -> Vec<usize> {
    w.actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active && a.handler == ActorHandler::ColourTween)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn the_shipped_instruction_reproduces_the_captured_template() {
    let mut w = World::new();
    run(&mut w, &WHITE_IN);

    let seated = tweens(&w);
    assert_eq!(seated.len(), 1, "one walk-in tween, no walk-out to pair");
    let t = w.actors[seated[0]].colour_tween.expect("a tween");
    // The capture's template, field for field.
    assert_eq!(t.push_blend, 2, "template[0]");
    assert_eq!(t.duration, 57, "template[1] - 65 shortened by an eighth");
    assert_eq!(t.from, (0, 0, 0), "template[3..=5]");
    assert_eq!(t.to, (255, 255, 255), "template[7..=9]");
    assert_eq!(t.delay, 0, "template[10]");
    assert_eq!(t.hold, -1, "template[11]");
    assert_eq!(t.push_kind, 0, "the spawner's a1");
}

#[test]
fn the_pushed_grey_lane_follows_the_captured_beats() {
    let mut w = World::new();
    run(&mut w, &WHITE_IN);

    // The capture stepped the tween every third vsync with `DAT_1F800393`
    // at 3, so clock runs 0, 3, 6, ... and the lane is `255 * clock / 57`.
    let expected = [0x00u32, 0x0D, 0x1A, 0x28, 0x35, 0x43];
    for (beat, want) in expected.iter().enumerate() {
        // The push is this frame's, produced by the step, and the colour is
        // computed off the clock the step came IN with - which is what the
        // capture logs beside each push.
        w.tick_handler_actors(3);
        let pushes = w.screen_tint_pushes();
        assert_eq!(pushes.len(), 1, "beat {beat}");
        let p = pushes[0];
        assert_eq!((p.kind, p.blend), (0, 2), "beat {beat}");
        assert_eq!(
            p.packed,
            want | (want << 8) | (want << 16),
            "beat {beat} grey lane"
        );
    }
}

#[test]
fn a_second_instruction_walks_the_first_one_out_to_black() {
    let mut w = World::new();
    run(&mut w, &WHITE_IN);
    let first = tweens(&w)[0];

    // A second op retires the live tween and spawns a walk-out from the
    // PREVIOUS target down to black with a one-frame hold, then the new
    // walk-in. Retail reads the old target out of the globals before it
    // overwrites them, which is what makes the walk-out start white.
    run(&mut w, &[0x34, 0x05, 0x40, 0x40, 0x40, 0x10, 0x00]);
    let live = tweens(&w);
    assert_eq!(live.len(), 3, "the retired slot is collected at tick time");
    assert!(w.actors[first].physics.status_flags & 0x8 != 0);

    let walk_out = live
        .iter()
        .filter(|&&s| s != first)
        .map(|&s| w.actors[s].colour_tween.unwrap())
        .find(|t| t.to == (0, 0, 0))
        .expect("a walk-out tween");
    assert_eq!(walk_out.from, (255, 255, 255));
    assert_eq!(walk_out.hold, 1, "one frame, then it retires itself");
    assert_eq!(walk_out.duration, 0x10);

    let walk_in = live
        .iter()
        .filter(|&&s| s != first)
        .map(|&s| w.actors[s].colour_tween.unwrap())
        .find(|t| t.to == (0x40, 0x40, 0x40))
        .expect("a walk-in tween");
    // No eighth off: the shrink is white-only.
    assert_eq!(walk_in.duration, 0x10);
    assert_eq!(walk_in.from, (0, 0, 0));
}

#[test]
fn an_all_zero_target_clears_the_effect_and_spawns_nothing() {
    let mut w = World::new();
    run(&mut w, &WHITE_IN);
    assert_eq!(tweens(&w).len(), 1);

    // `sw zero,-0x49d4(v0)` then straight to the exit: the arm drops the
    // effect pointer and never reaches the spawner.
    run(&mut w, &[0x34, 0x05, 0x00, 0x00, 0x00, 0x20, 0x00]);
    assert!(w.presentation.effect_tween_slot.is_none());
    // Only the walk-out remains; there is no walk-in behind it.
    let live = tweens(&w);
    let walk_ins = live
        .iter()
        .filter(|&&s| w.actors[s].physics.status_flags & 0x8 == 0)
        .filter_map(|&s| w.actors[s].colour_tween)
        .filter(|t| t.hold == -1 && t.from == (0, 0, 0))
        .count();
    assert_eq!(walk_ins, 0, "no walk-in behind the clear");
}

#[test]
fn the_sub_op_byte_selects_the_blend_and_the_push_kind() {
    // blend = (op0 & 1) ? 2 : 1; kind = 8 on bit 1, else 0 on bit 2, else 2.
    for (op0, blend, kind) in [
        (0x00u8, 1i16, 2i16),
        (0x01, 2, 2),
        (0x04, 1, 0),
        (0x05, 2, 0),
        (0x02, 1, 8),
        (0x07, 2, 8),
    ] {
        let mut w = World::new();
        run(&mut w, &[0x34, op0, 0x10, 0x20, 0x30, 0x08, 0x00]);
        let t = w.actors[tweens(&w)[0]].colour_tween.expect("a tween");
        assert_eq!((t.push_blend, t.push_kind), (blend, kind), "op0 {op0:#04x}");
    }
}
