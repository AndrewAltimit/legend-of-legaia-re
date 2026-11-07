//! The executing VM and the side-effect-free disassembler
//! (`legaia_overlay_images::move_disasm`) agree on every instruction width.

use super::*;
use legaia_overlay_images::move_disasm;

/// One `step` over `[op, 0, 0, ...]` advances the PC by the width the
/// disassembler decodes for the same words, for every main opcode whose width
/// the instruction fixes (all but `0x0B`, which carries the previous one).
#[test]
fn step_and_disassembler_agree_on_every_main_opcode() {
    for op in 0..move_disasm::MOVE_OP_COUNT {
        if op == move_disasm::OP_CARRY {
            continue;
        }
        let mut bc = vec![op];
        bc.resize(64, 0);
        let want = move_disasm::insn_width(|i| bc[i]).expect("fixed width");
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let _ = step(&mut host, &mut state, &bc);
        assert_eq!(
            state.pc as usize, want,
            "op {op:#04X}: the VM advanced {}, the disassembler says {want}",
            state.pc
        );
    }
}

/// Same for every `0x2F` sub-opcode, with the engine's own width table
/// (`canonical_size`) as the third reading.
#[test]
fn step_and_disassembler_agree_on_every_ext_sub_opcode() {
    use crate::move_vm_overlay_ext::canonical_size;
    for sub in 0..move_disasm::MOVE_EXT_COUNT {
        let bc = ext_program(sub);
        let want = move_disasm::insn_width(|i| bc[i]).expect("fixed width");
        assert_eq!(
            canonical_size(sub).map(usize::from),
            Some(want),
            "sub {sub:#04X}"
        );
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        let _ = step(&mut host, &mut state, &bc);
        assert_eq!(state.pc as usize, want, "sub {sub:#04X}");
    }
}
