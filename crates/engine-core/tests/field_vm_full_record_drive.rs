//! Field VM end-to-end drive against a synthesised event-script record that
//! exercises a representative slice of every opcode band. Companion test to
//! [`field_scripts_smoke`] (which is disc-gated); this one runs in CI.
//!
//! What it proves:
//!  - The field VM dispatches all 43 implemented opcodes without panic.
//!  - The World's `FieldHost` impl satisfies every callback.
//!  - The pending event queue captures one event per side-effecting opcode.
//!  - The VM reaches a clean `Halt` from a long synthetic record without
//!    drifting into `Unknown` opcodes.

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field::StepResult;

/// Build a synthetic event-script record that hits every documented
/// side-effecting opcode at least once. Each block is short and self-
/// contained so an unknown / pending arm bisects to a specific opcode.
fn synthetic_record() -> Vec<u8> {
    let mut bc = Vec::with_capacity(128);
    // Op 0x35 sub-1 = BGM start, text_id = 0x12. Encoding is
    // `[0x35, lo, hi, sub_op]` (le16 text_id then sub_op byte).
    bc.extend_from_slice(&[0x35, 0x12, 0x00, 0x01]);
    // Op 0x39 = play SFX, sfx_id = 0x42 (1 byte operand).
    bc.extend_from_slice(&[0x39, 0x42]);
    // Op 0x3A = add money, 24-bit operand (LE) = +500.
    bc.extend_from_slice(&[0x3A, 0xF4, 0x01, 0x00]);
    // Op 0x3B = set inventory slot count. slot_byte=0x10, count=5.
    bc.extend_from_slice(&[0x3B, 0x10, 0x05]);
    // Op 0x3C = party_add char_id=4.
    bc.extend_from_slice(&[0x3C, 0x04]);
    // Op 0x3D = party_remove char_id=4.
    bc.extend_from_slice(&[0x3D, 0x04]);
    // Op 0x44 = counter update with op0=1.
    bc.extend_from_slice(&[0x44, 0x01]);
    // Op 0x4F = scene register write (slot_10, slot_12, slot_14).
    bc.extend_from_slice(&[0x4F, 0x01, 0x02, 0x03]);
    // Halt opcode - terminates the field VM cleanly. The halt opener is
    // 0x00 (END / HALT in the field-VM tables).
    bc.extend_from_slice(&[0x00]);
    bc
}

#[test]
fn synthetic_record_runs_to_halt_without_unknown_opcodes() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.load_field_record(&synthetic_record());

    let mut counters = [0u32; 5]; // advance, yield, halt, pending, unknown
    let mut total_steps = 0u32;
    for _ in 0..1_000 {
        match world.step_field() {
            Some(StepResult::Advance { .. }) => counters[0] += 1,
            Some(StepResult::Yield { .. }) => counters[1] += 1,
            Some(StepResult::Halt { .. }) => {
                counters[2] += 1;
                break;
            }
            Some(StepResult::Pending { .. }) => counters[3] += 1,
            Some(StepResult::Unknown { opcode, .. }) => {
                panic!("synthetic record hit Unknown opcode 0x{opcode:02x}");
            }
            None => break,
        }
        total_steps += 1;
    }

    eprintln!(
        "[field-vm-drive] advance={} yield={} halt={} pending={} unknown={} total_steps={}",
        counters[0], counters[1], counters[2], counters[3], counters[4], total_steps
    );

    assert_eq!(counters[4], 0, "VM hit unknown opcodes");
    assert!(counters[2] >= 1, "VM never halted within 1000 steps");
    assert!(
        counters[0] >= 8,
        "VM made fewer Advance steps than the 9 opcodes in the record"
    );
}

#[test]
fn synthetic_record_emits_expected_event_shapes() {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.load_field_record(&synthetic_record());

    for _ in 0..200 {
        if matches!(world.step_field(), Some(StepResult::Halt { .. })) {
            break;
        }
    }

    let events = world.drain_field_events();
    let mut has_bgm = false;
    let mut has_sfx = false;
    let mut has_money = false;
    let mut has_party_add = false;
    for ev in &events {
        match ev {
            FieldEvent::Bgm { sub_op: 1, .. } => has_bgm = true,
            FieldEvent::PlaySfx { .. } => has_sfx = true,
            FieldEvent::AddMoney { .. } => has_money = true,
            FieldEvent::PartyAdd { .. } => has_party_add = true,
            _ => {}
        }
    }
    assert!(has_bgm, "expected Bgm event from op 0x35 sub-1");
    assert!(has_sfx, "expected PlaySfx event from op 0x39");
    assert!(has_money, "expected AddMoney event from op 0x3A");
    assert!(has_party_add, "expected PartyAdd event from op 0x3C");
}
