//! Op `0x2F` sub-op widths: every sub-op advances by its canonical width.

use super::*;

/// Every recognised sub-opcode advances the PC by its own width.
///
/// The widths are the `li s2, N` exit slots of `FUN_801D362C`, mirrored in
/// [`crate::move_vm_overlay_ext::canonical_size`]. Ten arms are conditional
/// branches whose taken side adds a signed displacement read from the last
/// operand word; with a zeroed operand stream both sides collapse onto the
/// fall-through width, so one table covers all 61.
///
/// The thing this guards against is a size-1 return leaking back in: that
/// leaves the PC on the sub-opcode word, which the **outer** opcode space
/// re-decodes as an instruction of its own.
#[test]
fn op2f_every_sub_opcode_advances_by_its_canonical_width() {
    use crate::move_vm_overlay_ext::{MAX_SUB_OPCODE, canonical_size};

    for sub_op in 0..MAX_SUB_OPCODE {
        let expected = canonical_size(sub_op).expect("in-range sub-op has a width");
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = ext_program(sub_op);
        let r = step(&mut host, &mut state, &bc);
        assert_eq!(
            r,
            StepResult::Advance,
            "sub-op 0x{sub_op:02X} did not advance"
        );
        assert_eq!(
            state.pc, expected as i16,
            "sub-op 0x{sub_op:02X}: advanced {} halfwords, disassembly says {expected}",
            state.pc
        );
        assert!(
            state.pc >= 2,
            "sub-op 0x{sub_op:02X} advanced onto its own sub-opcode word"
        );
    }
}

/// Out-of-range sub-opcodes are the one size-1 path: `li s2, 0x1` at
/// `0x801D365C`, in the delay slot of the `sltiu v1, 0x3D` bounds check.
#[test]
fn op2f_out_of_range_sub_opcode_advances_one_halfword() {
    for sub_op in [crate::move_vm_overlay_ext::MAX_SUB_OPCODE, 0x40, 0xFFFF] {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let bc = ext_program(sub_op);
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 1, "sub-op 0x{sub_op:04X} should resync by 1");
    }
}

/// Sub-op 0x00 is the widest arm and the one most visibly wrong under a
/// default-arm reading: a bare `j 0x801d4a3c` with `li s2, 0x10`.
#[test]
fn op2f_subop_00_skips_sixteen_halfwords() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    step(&mut host, &mut state, &ext_program(0x00));
    assert_eq!(state.pc, 16);
}

/// 0x18 is the odd member of the 0x17..0x1A world-struct family: it seeds
/// two words from `world_y`, not five from the bytecode, and is 5 wide
/// where its siblings are 8.
#[test]
fn op2f_subop_18_is_five_wide_while_19_and_1a_are_eight() {
    for (sub_op, width) in [(0x17u16, 8i16), (0x18, 5), (0x19, 8), (0x1A, 8)] {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        step(&mut host, &mut state, &ext_program(sub_op));
        assert_eq!(state.pc, width, "sub-op 0x{sub_op:02X}");
    }
}

/// 0x19 adds its five operands to the object-effect row
/// (`0x801D3C5C..0x801D3CDC`: load, `addu`, store per halfword) - it does
/// not overwrite it.
#[test]
fn op2f_subop_19_adds_to_the_object_effect_row() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    let mut prog = ext_program(0x19);
    prog[2..8].copy_from_slice(&[1, 0x10, 0x20, 0x30, 0xFFFF, 5]);
    step(&mut host, &mut state, &prog);
    assert_eq!(
        host.ext_world_struct_adds,
        vec![(1, [0x10, 0x20, 0x30, -1, 5])]
    );
    assert!(host.ext_world_struct_writes.is_empty());
}

/// 0x1A seats a yaw row: `+0 = 0`, `+2 = (op[3] + 0x400) & 0xFFF`,
/// `+4 = 0x400`, and offsets both clip words by
/// `(op[5] * sin[yaw] + op[4] * cos[yaw]) >> 12` (`0x801D3CE0..0x801D3D80`).
#[test]
fn op2f_subop_1a_seats_a_yaw_row_with_rotated_clip_offsets() {
    let mut host = TestHost::default();
    // yaw = (0x0C00 + 0x400) & 0xFFF = 0; sin 0x800 (0.5), cos 0x1000 (1.0).
    host.rotation_table.insert(0, (0x800, 0x1000));
    let mut state = ActorState::new();
    let mut prog = ext_program(0x1A);
    prog[2..8].copy_from_slice(&[0, 0x0C00, 100, 40, 7, 9]);
    step(&mut host, &mut state, &prog);
    // v = (40 * 0x800 + 100 * 0x1000) >> 12 = 20 + 100 = 120.
    assert_eq!(
        host.ext_world_struct_writes,
        vec![(0, [0, 0, 0x400, 127, 129])]
    );
}

/// 0x28's clamp branch selects whether z is clamped, not the width -
/// `li s2, 0x5` sits in the delay slot of the branch at 0x801D43A8 and the
/// fall-through exits with it unchanged.
#[test]
fn op2f_subop_28_is_five_wide_on_both_clamp_paths() {
    for (raw_z, scale) in [(0x10u16, 0x1000u16), (0x7FFF, 0x7FFF)] {
        let mut host = TestHost::default();
        host.move_slot_save_u16(1, 4, raw_z);
        let mut state = ActorState::new();
        let mut bc = ext_program(0x28);
        bc[2] = 1; // slot
        bc[3] = 0x1000; // y scale
        bc[4] = scale; // z scale
        step(&mut host, &mut state, &bc);
        assert_eq!(state.pc, 5);
    }
}

/// 0x3B is 4 wide whether or not the party lookup resolves - `li s2, 0x4`
/// is in the delay slot of the `beq` that guards the write.
#[test]
fn op2f_subop_3b_is_four_wide_on_both_lookup_paths() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    step(&mut host, &mut state, &ext_program(0x3B));
    assert_eq!(state.pc, 4, "lookup miss");

    let mut host2 = TestHost::default();
    host2.party_member = Some([7, 8, 9]);
    let mut state2 = ActorState::new();
    step(&mut host2, &mut state2, &ext_program(0x3B));
    assert_eq!(state2.pc, 4, "lookup hit");
}
