//! Synthetic programs only: hand-authored halfword streams, no disc bytes.

use super::*;

fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The constant widths, spot-checked against the `li a2, N` each arm of
/// `FUN_80023070` sets (`move-vm.md`'s opcode table).
#[test]
fn constant_widths_match_the_dispatcher_arms() {
    for (op, want) in [
        (0x00u16, 4usize),
        (0x09, 2),
        (0x0C, 6),
        (0x13, 16),
        (0x14, 5),
        (0x19, 1),
        (0x1E, 8),
        (0x20, 3),
        (0x21, 7),
        (0x22, 1),
        (0x23, 13),
        (0x2C, 5),
        (0x30, 1),
        (0x34, 9),
        (0x40, 7),
        (0x42, 15),
        (0x45, 8),
        (0x46, 4),
    ] {
        assert_eq!(
            insn_width(|i| if i == 0 { op } else { 0 }),
            Some(want),
            "op {op:#04X}"
        );
    }
    // No constant-width opcode is width 0 except the special six.
    for op in 0..MOVE_OP_COUNT {
        let special = matches!(op, 0x08 | 0x0A | 0x0B | 0x2F | 0x3C | 0x3D);
        assert_eq!(MOVE_OP_HALFWORDS[op as usize] == 0, special, "op {op:#04X}");
    }
}

#[test]
fn operand_dependent_widths() {
    // KEYFRAME_LOAD with two lanes: 3 + 3*2.
    let w = [0x0A, 1, 2, 0, 0, 0, 0, 0, 0];
    assert_eq!(insn_width(|i| w[i]), Some(9));
    // KEYFRAME_SEAT with one part: 2 + 6.
    let w = [0x3C, 1, 0, 0, 0, 0, 0, 0];
    assert_eq!(insn_width(|i| w[i]), Some(8));
    // KEYFRAME_RETARGET with two parts: 3 + 12.
    assert_eq!(insn_width(|i| [0x3D, 0, 2][i.min(2)]), Some(15));
    // A negative count reads as zero lanes.
    assert_eq!(insn_width(|i| [0x0A, 0, 0xFFFF][i.min(2)]), Some(3));
    // OVERLAY_EXT takes the sub-op's width; out of range advances 1.
    assert_eq!(insn_width(|i| [0x2F, 0x0E][i.min(1)]), Some(11));
    assert_eq!(insn_width(|i| [0x2F, 0x3D][i.min(1)]), Some(1));
    // 0x0B has no width of its own.
    assert_eq!(insn_width(|_| 0x0B), None);
    assert_eq!(insn_width(|_| 0x47), Some(0));
}

/// The record shape every slot-B transform part opens with: a velocity, the
/// control word, a draw setup, a wait, the halt.
#[test]
fn walks_a_record_to_its_halt() {
    let mut w = vec![0xFFFF, 0, 0x00, 0, 8, 0xFFC0, 0x0C, 0xFFA9, 0, 0, 0, 0];
    w.extend_from_slice(&[
        0x23, 0, 0x40, 0x40, 0x40, 0x800, 0x200, 0xB, 0x7AC0, 0, 0, 0xFF, 0x3F,
    ]);
    w.extend_from_slice(&[0x09, 0x1F, 0x08, 0x1234]);
    let b = bytes(&w);
    let r = disassemble_record(&b, 0, WalkOptions::default()).unwrap();
    assert_eq!(r.model_sel, -1);
    let ops: Vec<u16> = r.program.iter().map(|i| i.op).collect();
    assert_eq!(ops, [0x00, 0x0C, 0x23, 0x09, 0x08]);
    assert_eq!(r.end(), 2 * 28);
    assert_eq!(describe(&r.program[0]), "+0x3C=0 +0x3E=64 +0x40=-512");
    assert_eq!(r.program[3].flow, Flow::Wait { units: 248 });
    assert!(describe(&r.program[2]).contains("clut +0xB2=0x7AC0"));
    assert_eq!(describe(&r.program[1]), "+0x74=0xE9000000 +0x78=0");
}

#[test]
fn a_forward_branch_carries_the_walk_past_a_halt() {
    // 0x2F 0x0A: branch when the predicate is set, base 3 + delta 1 lands
    // past the HALT below it.
    let w = [0x2F, 0x0A, 1, 0x08, 0x09, 2, 0x08];
    let b = bytes(&w);
    let p = disassemble(&b, 0, WalkOptions::default());
    assert_eq!(p[0].flow, Flow::Branch { target: Some(8) });
    let ops: Vec<u16> = p.iter().map(|i| i.op).collect();
    assert_eq!(ops, [0x2F, 0x08, 0x09, 0x08]);
    // A backward (spin-wait) displacement lands on itself.
    let w = [0x2F, 0x13, 5, 0xFFFC, 0x08];
    let p = disassemble(&bytes(&w), 0, WalkOptions::default());
    assert_eq!(p[0].flow, Flow::Branch { target: Some(0) });
    assert_eq!(p.len(), 2);
}

#[test]
fn carry_advances_by_the_previous_width() {
    let w = [0x0D, 4, 0x0B, 7, 0x08];
    let p = disassemble(&bytes(&w), 0, WalkOptions::default());
    assert_eq!(p[1].op, 0x0B);
    assert_eq!(p[1].width, 2);
    assert_eq!(p[2].op, 0x08);
    // At the program's start it has no width to carry and the walk stops.
    let p = disassemble(&bytes(&[0x0B, 0x08]), 0, WalkOptions::default());
    assert_eq!(p.len(), 1);
}

#[test]
fn loops_are_reported_not_followed() {
    let w = [0x18, 0x4000, 0x09, 1, 0x19, 0x08];
    let p = disassemble(&bytes(&w), 0, WalkOptions::default());
    assert_eq!(
        p[0].flow,
        Flow::LoopOpen {
            reg: b'A',
            count: 0x4000
        }
    );
    assert_eq!(p[2].flow, Flow::LoopBack { reg: b'A' });
    assert_eq!(p[2].width, 1);
    assert_eq!(describe(&p[0]), "count 0 forever");
}

#[test]
fn out_of_range_ends_and_through_halt_continues() {
    let w = [0x08, 0x09, 3, 0x50];
    let p = disassemble(&bytes(&w), 0, WalkOptions::default());
    assert_eq!(p.len(), 1);
    let p = disassemble(
        &bytes(&w),
        0,
        WalkOptions {
            through_halt: true,
            ..WalkOptions::default()
        },
    );
    let flows: Vec<Flow> = p.iter().map(|i| i.flow).collect();
    assert_eq!(flows[2], Flow::End);
    assert_eq!(p.len(), 3);
}

#[test]
fn every_mnemonic_is_distinct() {
    let mut names: Vec<&str> = MOVE_OP_NAMES
        .iter()
        .chain(MOVE_EXT_NAMES.iter())
        .copied()
        .collect();
    names.sort_unstable();
    let n = names.len();
    names.dedup();
    assert_eq!(names.len(), n);
}

#[test]
fn formats_with_a_load_base() {
    let b = bytes(&[0x09, 0x1F]);
    let i = decode_at(&b, 0, None);
    let line = format_insn(
        &i,
        Addressing {
            base: Some(0x801F_69D8),
        },
    );
    assert!(line.starts_with("801F69D8  0009 001F"), "{line}");
    assert!(line.contains("WAIT"));
}
