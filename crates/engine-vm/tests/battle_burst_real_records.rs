//! Image-gated oracle for the two `FUN_801F30C4` stager records.
//!
//! Parses both arms' move-VM records straight out of the mapped `0898` image
//! and asserts the *structural* claims `battle_burst`'s module docs make. The
//! records are disc data: nothing here reproduces their bytes, and every
//! assertion is about extent, terminator, opcode sequence, or which single word
//! the two arms disagree on.
//!
//! Skips silently when `extracted/overlays/overlay_battle_action_0898.bin` is
//! missing - same convention as the rest of the disc-gated suite. Produce it
//! with `asset overlay extract` (see `docs/tooling/static-overlay-pipeline.md`).

use std::path::PathBuf;

use legaia_engine_vm::battle_burst::{BurstMode, BurstRecord, RECORD_HEADER_WORDS};

/// Verified static base of PROT 0898 (`crates/asset/data/static-overlays.toml`).
const BASE_VA: u32 = 0x801C_E818;

/// Move-VM opcodes the records are expected to be built from. Named rather
/// than listed as raw values in the assertions below.
const OP_RENDER_BANK_SET: u16 = 0x39;
const OP_FLAG_52: u16 = 0x15;
const OP_MODE2_CHILD: u16 = 0x23;
const OP_CONTROL_WORD: u16 = 0x0C;
const OP_WAIT_SET: u16 = 0x09;
const OP_SPRITE_ADD: u16 = 0x24;
const OP_HALT: u16 = 0x08;
const OP_BATTLE_EXT: u16 = 0x17;

fn overlay_image() -> Option<Vec<u8>> {
    for prefix in ["extracted/overlays", "../../extracted/overlays"] {
        let p = PathBuf::from(prefix).join("overlay_battle_action_0898.bin");
        if p.exists() {
            return std::fs::read(&p).ok();
        }
    }
    None
}

fn arms(image: &[u8]) -> (BurstRecord, BurstRecord) {
    (
        BurstRecord::parse(image, BASE_VA, BurstMode::Wide).expect("wide record parses"),
        BurstRecord::parse(image, BASE_VA, BurstMode::Narrow).expect("narrow record parses"),
    )
}

#[test]
fn both_arms_parse_as_transform_node_stager_records() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    let (wide, narrow) = arms(&img);
    for (label, rec) in [("wide", &wide), ("narrow", &narrow)] {
        assert_eq!(rec.model_sel, -1, "{label}: transform/pivot node");
        assert_eq!(rec.flags, 0, "{label}");
        assert_eq!(
            rec.program.last().copied(),
            Some(OP_HALT),
            "{label}: terminates at HALT"
        );
    }
}

#[test]
fn both_arms_run_the_same_program_shape() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    let (wide, narrow) = arms(&img);

    assert_eq!(
        wide.opcode_sequence(),
        narrow.opcode_sequence(),
        "the two arms differ in constants, not in instructions"
    );
    assert_eq!(wide.byte_len(), narrow.byte_len());

    // The shape: set up a render-mode-2 child, then walk it through a sprite
    // strip one frame per wait, then halt.
    let ops = wide.opcode_sequence();
    assert_eq!(
        &ops[..4],
        &[
            OP_RENDER_BANK_SET,
            OP_FLAG_52,
            OP_MODE2_CHILD,
            OP_CONTROL_WORD
        ],
        "prologue"
    );
    assert_eq!(ops.last().copied(), Some(OP_HALT));

    // The tail is strictly alternating WAIT_SET / sprite-add pairs, closed by a
    // final WAIT_SET before the HALT. That alternation is what makes it a
    // frame-per-step strip rather than a single jump.
    let tail = &ops[4..ops.len() - 1];
    assert!(tail.len() >= 3, "tail is {} ops", tail.len());
    for (i, op) in tail.iter().enumerate() {
        let want = if i % 2 == 0 {
            OP_WAIT_SET
        } else {
            OP_SPRITE_ADD
        };
        assert_eq!(*op, want, "tail op {i}");
    }
    assert_eq!(
        tail.last().copied(),
        Some(OP_WAIT_SET),
        "the strip ends on a wait, not on a step"
    );
}

#[test]
fn the_two_arms_differ_in_exactly_one_operand() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    let (wide, narrow) = arms(&img);
    assert_eq!(wide.program.len(), narrow.program.len());

    let diffs: Vec<usize> = wide
        .program
        .iter()
        .zip(narrow.program.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        diffs.len(),
        1,
        "the records are one halfword apart, not {} - if this grew, the arms \
         are no longer the same effect at two settings",
        diffs.len()
    );

    // And that halfword is an operand of the render-mode-2 child spawn, not a
    // header field or an opcode. Op 0x23's operand 8 lands in the child's
    // +0xB2; what that field means under render mode 2 is not pinned here (the
    // ported actor tick names +0xB0/+0xB2 for the mode-5 SFX emitter arm, which
    // is a different mode), so this asserts the position, not a meaning.
    let idx = diffs[0];
    let spawn_pc = wide
        .program
        .iter()
        .position(|&w| w == OP_MODE2_CHILD)
        .expect("the record spawns a mode-2 child");
    assert_eq!(
        idx - spawn_pc,
        8,
        "the differing word is operand 8 of the 0x23 at program word {spawn_pc}"
    );
}

#[test]
fn each_trigger_record_fires_the_arm_it_precedes() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    for (mode, want_operand) in [(BurstMode::Wide, 0u16), (BurstMode::Narrow, 1)] {
        let va = mode.trigger_addr();
        let trig = BurstRecord::parse_at(&img, BASE_VA, va)
            .unwrap_or_else(|| panic!("{mode:?}: trigger at {va:#010X} parses"));

        assert_eq!(trig.model_sel, -1, "{mode:?}");
        assert_eq!(
            trig.opcode_sequence(),
            vec![OP_WAIT_SET, OP_BATTLE_EXT, OP_WAIT_SET, OP_HALT],
            "{mode:?}: wait / battle-escape / wait / halt"
        );
        assert_eq!(trig.byte_len(), 18, "{mode:?}: nine words with the header");

        // The escape's single operand is this arm's mode - which is what makes
        // the trigger/record adjacency meaningful rather than coincidental.
        let ext_pc = trig
            .program
            .iter()
            .position(|&w| w == OP_BATTLE_EXT)
            .expect("escape present");
        assert_eq!(
            trig.program[ext_pc + 1],
            want_operand,
            "{mode:?}: op 0x17 operand"
        );

        // And the arm's own stager record starts one alignment word past the
        // trigger's end.
        assert_eq!(
            va + trig.byte_len() as u32 + 2,
            mode.record_addr(),
            "{mode:?}: record follows its trigger"
        );
    }
}

#[test]
fn the_documented_shape_pins_the_base() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    // Walking to HALT is not by itself a base check: start a few words late and
    // the walk still finds the record's own terminator, so it returns a
    // *suffix* and reports success. What pins the base is the pair of header
    // and prologue claims - `model_sel == -1` and a program that opens on the
    // render-bank set. Assert that no near-miss base satisfies both, so the
    // shape assertions in this file are load-bearing rather than decorative.
    //
    // The probe walks forward only (a larger offset into the image); 64 words
    // stays inside the wide record, whose 65 words contain exactly one
    // `0xFFFF`.
    let truth = BurstRecord::parse(&img, BASE_VA, BurstMode::Wide).expect("the real base parses");
    assert_eq!(truth.model_sel, -1);
    assert_eq!(truth.program[0], OP_RENDER_BANK_SET);

    for k in 1..=64u32 {
        let bogus = BASE_VA.wrapping_sub(k * 2);
        if let Some(rec) = BurstRecord::parse(&img, bogus, BurstMode::Wide) {
            assert!(
                rec.model_sel != -1 || rec.program.first() != Some(&OP_RENDER_BANK_SET),
                "base off by {k} words still matches the documented shape"
            );
            assert_ne!(rec, truth, "base off by {k} words reproduced the record");
        }
    }
}

#[test]
fn the_header_is_two_words_and_the_program_is_what_follows() {
    let Some(img) = overlay_image() else {
        eprintln!("[skip] extracted/overlays/overlay_battle_action_0898.bin missing");
        return;
    };
    let (wide, _) = arms(&img);
    assert_eq!(
        wide.byte_len(),
        (RECORD_HEADER_WORDS + wide.program.len()) * 2
    );
    // The seater reads the record's first word; the VM starts at the third.
    // If the header size were wrong the first opcode would be `flags`, and the
    // program would not begin with a recognised setup op.
    assert_eq!(wide.program[0], OP_RENDER_BANK_SET);
}
