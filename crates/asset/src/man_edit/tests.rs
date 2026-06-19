//! Unit tests for the variable-length MAN editor, on synthetic MANs (no disc).

use super::*;
use crate::man_section::{self, RECORDS_BEGIN_OFFSET};

/// Build a synthetic MAN with `n2` partition-2 records (each the *full* record
/// bytes: `[name_len][name*2][c0][c1][c2] + script`), no P0/P1 records, and six
/// zero-length terminator sections after the records.
fn build_man(p2_records: &[Vec<u8>]) -> Vec<u8> {
    let n2 = p2_records.len();
    let data_region = RECORDS_BEGIN_OFFSET + 3 * n2;
    let mut records_blob = Vec::new();
    let mut offsets = Vec::new();
    for rec in p2_records {
        offsets.push(records_blob.len() as u32);
        records_blob.extend_from_slice(rec);
    }
    let u24_at_28 = records_blob.len() as u32; // section 0 right after records

    let mut man = vec![0u8; data_region];
    // partition counts: N0=0, N1=0, N2=n2 at 0x22/0x24/0x26.
    man[0x26] = (n2 & 0xFF) as u8;
    man[0x27] = ((n2 >> 8) & 0xFF) as u8;
    // u24_at_28
    man[0x28] = (u24_at_28 & 0xFF) as u8;
    man[0x29] = ((u24_at_28 >> 8) & 0xFF) as u8;
    man[0x2A] = ((u24_at_28 >> 16) & 0xFF) as u8;
    // partition-2 record offset table (N0=N1=0, so it starts at 0x2B).
    let mut cur = RECORDS_BEGIN_OFFSET;
    for off in &offsets {
        man[cur] = (off & 0xFF) as u8;
        man[cur + 1] = ((off >> 8) & 0xFF) as u8;
        man[cur + 2] = ((off >> 16) & 0xFF) as u8;
        cur += 3;
    }
    man.extend_from_slice(&records_blob);
    // six zero-length sections (3 bytes each).
    man.extend_from_slice(&[0u8; 18]);
    man
}

/// A `0x3F` scene-change op: `3f [idx LE][name_len][name][ex][ez][dir]`.
fn scene_change_op(index: i16, name: &[u8], ex: u8, ez: u8, dir: u8) -> Vec<u8> {
    let mut v = vec![0x3F];
    v.extend_from_slice(&index.to_le_bytes());
    v.push(name.len() as u8);
    v.extend_from_slice(name);
    v.extend_from_slice(&[ex, ez, dir]);
    v
}

/// A minimal partition-2 record prefix: `[name_len=1]["XY"][c0=0][c1=0][c2=0]`,
/// giving pc0 = 1 + 2 + 1 + 1 + 1 = 6.
fn p2_prefix() -> Vec<u8> {
    vec![0x01, b'X', b'Y', 0x00, 0x00, 0x00]
}

#[test]
fn grow_name_relocates_section_and_validates() {
    let mut rec = p2_prefix();
    rec.extend_from_slice(&scene_change_op(0x05, b"ab", 0x10, 0x20, 0x30));
    rec.push(0x21); // trailing Nop
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    // op sits at data_region + pc0(6).
    let op_pc = mf.data_region_offset + 6;
    assert_eq!(man[op_pc], 0x3F);
    let old_sec0 = mf.sections[0].offset;

    let edit = DestEdit {
        op_pc,
        index: 0x05,
        name: b"abcd".to_vec(), // +2 bytes
        entry_x: 0x10,
        entry_z: 0x20,
        dir: 0x30,
    };
    let out = apply_dest_edits(&man, &[edit]).unwrap();
    assert_eq!(out.len(), man.len() + 2);
    let mf2 = man_section::parse(&out).unwrap();
    // op_pc is before the edit, so it doesn't move; section 0 shifts +2.
    assert_eq!(mf2.sections[0].offset, old_sec0 + 2);
    // the single P2 record's offset is unchanged (record starts before the edit).
    assert_eq!(mf2.partitions[2][0], mf.partitions[2][0]);
    // validate: op now names "abcd".
    assert!(validate(&out, &[(op_pc, b"abcd")]));
    let insn = field_disasm::decode(&out, op_pc).unwrap();
    assert_eq!(
        field_disasm::scene_change_name(&out, &insn).as_deref(),
        Some("abcd")
    );
}

#[test]
fn shrink_name_relocates_section() {
    let mut rec = p2_prefix();
    rec.extend_from_slice(&scene_change_op(0x05, b"rikuroa", 0x10, 0x20, 0x30));
    rec.push(0x21);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let op_pc = mf.data_region_offset + 6;
    let old_sec0 = mf.sections[0].offset;

    let edit = DestEdit {
        op_pc,
        index: 0x05,
        name: b"jou".to_vec(), // 7 -> 3 = -4 bytes
        entry_x: 0x10,
        entry_z: 0x20,
        dir: 0x30,
    };
    let out = apply_dest_edits(&man, &[edit]).unwrap();
    assert_eq!(out.len(), man.len() - 4);
    let mf2 = man_section::parse(&out).unwrap();
    assert_eq!(mf2.sections[0].offset, old_sec0 - 4);
    assert!(validate(&out, &[(op_pc, b"jou")]));
}

#[test]
fn later_record_offset_is_bumped() {
    // Two P2 records; edit the first, the second's table offset must move.
    let mut rec0 = p2_prefix();
    rec0.extend_from_slice(&scene_change_op(0x01, b"ab", 0, 0, 0));
    rec0.push(0x21);
    let mut rec1 = p2_prefix();
    rec1.extend_from_slice(&scene_change_op(0x02, b"cd", 0, 0, 0));
    rec1.push(0x21);
    let man = build_man(&[rec0, rec1]);
    let mf = man_section::parse(&man).unwrap();
    let op0 = mf.data_region_offset + 6;
    let rec1_off = mf.partitions[2][1];

    let edit = DestEdit {
        op_pc: op0,
        index: 0x01,
        name: b"abcdef".to_vec(), // +4
        entry_x: 0,
        entry_z: 0,
        dir: 0,
    };
    let out = apply_dest_edits(&man, &[edit]).unwrap();
    let mf2 = man_section::parse(&out).unwrap();
    // record 0 unchanged offset; record 1 bumped by +4.
    assert_eq!(mf2.partitions[2][0], mf.partitions[2][0]);
    assert_eq!(mf2.partitions[2][1], rec1_off + 4);
    // both ops still decode at their (mapped) positions.
    assert!(validate(&out, &[(op0, b"abcdef")]));
    let op1_new = mf2.data_region_offset + mf2.partitions[2][1] as usize + 6;
    let insn = field_disasm::decode(&out, op1_new).unwrap();
    assert_eq!(
        field_disasm::scene_change_name(&out, &insn).as_deref(),
        Some("cd")
    );
}

#[test]
fn spanning_forward_jump_delta_is_fixed() {
    // Record: [prefix] JmpRel(forward, over the op) 0x3F[op] 0x21 <target>.
    // The JmpRel target is the 0x21 after the op; growing the name must grow
    // the jump delta by the same amount.
    let prefix = p2_prefix();
    let op = scene_change_op(0x07, b"ab", 0, 0, 0);
    // JmpRel: 0x26 [u16 delta]. base = pc(after this insn's header) ; target =
    // base + delta. Place jump right at pc0, target = the 0x21 after the op.
    // Layout from pc0: [26 dd dd][op...][21]. jump size 3, op size = 1+6+2=9.
    // base = pc0 + 1 (header). target offset (abs) = pc0 + 3 + 9 = pc0+12.
    // delta = target - base = (pc0+12) - (pc0+1) = 11.
    let mut script = vec![0x26, 11, 0x00];
    script.extend_from_slice(&op);
    script.push(0x21);
    let mut rec = prefix.clone();
    rec.extend_from_slice(&script);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let pc0 = mf.data_region_offset + 6;
    let op_pc = pc0 + 3; // after the JmpRel
    assert_eq!(man[op_pc], 0x3F);

    let edit = DestEdit {
        op_pc,
        index: 0x07,
        name: b"abcd".to_vec(), // +2
        entry_x: 0,
        entry_z: 0,
        dir: 0,
    };
    let out = apply_dest_edits(&man, &[edit]).unwrap();
    // The JmpRel (still at pc0) must now have delta 11 + 2 = 13 (its target, the
    // 0x21 after the op, moved +2 while its base stayed put).
    let jmp = field_disasm::decode(&out, pc0).unwrap();
    match jmp.info {
        InsnInfo::JmpRel { delta, target } => {
            assert_eq!(delta, 13, "spanning forward jump delta grows with the edit");
            // target still points at the 0x21 trailing the (grown) op.
            assert_eq!(out[target], 0x21);
        }
        other => panic!("expected JmpRel, got {other:?}"),
    }
}

#[test]
fn non_spanning_jump_delta_is_unchanged() {
    // A backward self-loop AFTER the op (26 ff ff = JmpRel -1) must keep its
    // delta - both endpoints sit after the edit, so they shift together.
    let prefix = p2_prefix();
    let op = scene_change_op(0x07, b"ab", 0, 0, 0);
    let mut rec = prefix;
    rec.extend_from_slice(&op);
    rec.extend_from_slice(&[0x26, 0xFF, 0xFF]); // self-loop after the op
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let op_pc = mf.data_region_offset + 6;
    let loop_pc = op_pc + op_len(&man, op_pc);

    let edit = DestEdit {
        op_pc,
        index: 0x07,
        name: b"abcdef".to_vec(), // +4
        entry_x: 0,
        entry_z: 0,
        dir: 0,
    };
    let out = apply_dest_edits(&man, &[edit]).unwrap();
    let new_loop_pc = loop_pc + 4; // shifted by the grown name
    let jmp = field_disasm::decode(&out, new_loop_pc).unwrap();
    match jmp.info {
        InsnInfo::JmpRel { delta, .. } => assert_eq!(delta, 0xFFFF, "self-loop delta unchanged"),
        other => panic!("expected JmpRel, got {other:?}"),
    }
}

#[test]
fn rejects_non_scene_change_op() {
    let mut rec = p2_prefix();
    rec.push(0x21); // a Nop where we'll point the edit
    rec.extend_from_slice(&[0x21, 0x21]);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let op_pc = mf.data_region_offset + 6;
    let edit = DestEdit {
        op_pc,
        index: 0,
        name: b"x".to_vec(),
        entry_x: 0,
        entry_z: 0,
        dir: 0,
    };
    assert_eq!(
        apply_dest_edits(&man, &[edit]),
        Err(ManEditError::NotSceneChange { op_pc })
    );
}

fn op_len(man: &[u8], op_pc: usize) -> usize {
    field_disasm::decode(man, op_pc).unwrap().size
}
