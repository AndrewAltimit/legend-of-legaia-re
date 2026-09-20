//! Unit tests for the variable-length MAN editor, on synthetic MANs (no disc).

use super::*;
use crate::field_disasm::CameraKind;
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
    // Names are >= 3 bytes: `clean_scene_name` rejects shorter runs, which
    // on the disc are only text desyncs (no CDNAME label is that short).
    let mut rec0 = p2_prefix();
    rec0.extend_from_slice(&scene_change_op(0x01, b"abc", 0, 0, 0));
    rec0.push(0x21);
    let mut rec1 = p2_prefix();
    rec1.extend_from_slice(&scene_change_op(0x02, b"cde", 0, 0, 0));
    rec1.push(0x21);
    let man = build_man(&[rec0, rec1]);
    let mf = man_section::parse(&man).unwrap();
    let op0 = mf.data_region_offset + 6;
    let rec1_off = mf.partitions[2][1];

    let edit = DestEdit {
        op_pc: op0,
        index: 0x01,
        name: b"abcdefg".to_vec(), // +4
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
    assert!(validate(&out, &[(op0, b"abcdefg")]));
    let op1_new = mf2.data_region_offset + mf2.partitions[2][1] as usize + 6;
    let insn = field_disasm::decode(&out, op1_new).unwrap();
    assert_eq!(
        field_disasm::scene_change_name(&out, &insn).as_deref(),
        Some("cde")
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

/// A `0x45 0xC0` camera-apply in an edited record is **not** a control-flow
/// field, so a resize must (a) go through rather than refusing, and (b) leave
/// the apply-trigger halfword byte-identical.
///
/// The old reading treated that halfword as an absolute jump target: the
/// record scan rejected the whole edit (`ManEditError::AbsoluteRef`) and
/// `control_targets` listed it, so any path that did relocate one would have
/// rewritten a camera parameter as a shifted PC. Retail's APPLY arm
/// (`0x801DF254..0x801DF288`) hands that halfword to
/// `FUN_801DE084(0x801C6EA8, s16, mode)` and exits `addiu s8,s8,0x4` - a
/// plain fall-through, never a jump.
#[test]
fn camera_apply_trigger_survives_a_resize_untouched() {
    let prefix = p2_prefix();
    let op = scene_change_op(0x07, b"ab", 0, 0, 0);
    // `45 C0 34 12` - APPLY with trigger 0x1234, ahead of the edited op so
    // both its own offset and the op's shift are in play.
    let mut rec = prefix;
    rec.extend_from_slice(&[0x45, 0xC0, 0x34, 0x12]);
    rec.extend_from_slice(&op);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let apply_pc = mf.data_region_offset + 6;
    let op_pc = apply_pc + 4;
    assert_eq!(man[op_pc], 0x3F);

    let edit = DestEdit {
        op_pc,
        index: 0x07,
        name: b"abcdef".to_vec(), // +4
        entry_x: 0,
        entry_z: 0,
        dir: 0,
    };
    let out = apply_dest_edits(&man, &[edit]).expect("a camera-apply no longer blocks the edit");
    match field_disasm::decode(&out, apply_pc).unwrap().info {
        InsnInfo::Camera {
            op0,
            kind: CameraKind::Apply { apply_trigger },
        } => {
            assert_eq!(op0, 0xC0);
            assert_eq!(
                apply_trigger, 0x1234,
                "the apply trigger is a camera parameter and must not be relocated"
            );
        }
        other => panic!("expected a camera APPLY, got {other:?}"),
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

/// Build a synthetic MAN with `p1_records` (each full record bytes:
/// `[u8 N][N*2 locals][4-byte header] + script`) and `p2_records` (full P2
/// record bytes as in [`build_man`]). The blob lays the **P2 records first**,
/// then the P1 records, so the P1 table pass's over-walk (last P1 record
/// bounded by man-end) never crosses the P2 bodies - the synthetic mirror of
/// a retail P2-only door the P1 pass cannot see.
fn build_man_p1_p2(p1_records: &[Vec<u8>], p2_records: &[Vec<u8>]) -> Vec<u8> {
    let n1 = p1_records.len();
    let n2 = p2_records.len();
    let data_region = RECORDS_BEGIN_OFFSET + 3 * (n1 + n2);

    let mut records_blob = Vec::new();
    let mut p2_offsets = Vec::new();
    for rec in p2_records {
        p2_offsets.push(records_blob.len() as u32);
        records_blob.extend_from_slice(rec);
    }
    let mut p1_offsets = Vec::new();
    for rec in p1_records {
        p1_offsets.push(records_blob.len() as u32);
        records_blob.extend_from_slice(rec);
    }
    let u24_at_28 = records_blob.len() as u32; // section 0 right after records

    let mut man = vec![0u8; data_region];
    // partition counts: N0=0, N1 at 0x24, N2 at 0x26.
    man[0x24] = (n1 & 0xFF) as u8;
    man[0x25] = ((n1 >> 8) & 0xFF) as u8;
    man[0x26] = (n2 & 0xFF) as u8;
    man[0x27] = ((n2 >> 8) & 0xFF) as u8;
    man[0x28] = (u24_at_28 & 0xFF) as u8;
    man[0x29] = ((u24_at_28 >> 8) & 0xFF) as u8;
    man[0x2A] = ((u24_at_28 >> 16) & 0xFF) as u8;
    // record-offset table, [P0..P1..P2] order.
    let mut cur = RECORDS_BEGIN_OFFSET;
    for off in p1_offsets.iter().chain(&p2_offsets) {
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

/// A minimal P1 record: `[N=0][4-byte header] + script` (pc0 = 5).
fn p1_record(script: &[u8]) -> Vec<u8> {
    let mut rec = vec![0x00, 0xAA, 0xBB, 0xCC, 0xDD];
    rec.extend_from_slice(script);
    rec
}

#[test]
fn scene_destinations_fold_in_p2_only_door() {
    // P1 controller: a 0x3F to "dolk" + Nop.
    let mut p1_script = scene_change_op(60, b"dolk", 0x10, 0x20, 0x30);
    p1_script.push(0x21);
    // P2 record: the return door to "jouina" - carried ONLY here.
    let mut p2 = p2_prefix();
    p2.extend_from_slice(&scene_change_op(2, b"jouina", 0x08, 0x09, 0x00));
    p2.push(0x21);
    let man = build_man_p1_p2(&[p1_record(&p1_script)], &[p2]);
    let mf = man_section::parse(&man).unwrap();

    // The P1 table pass alone misses the P2-only door...
    let p1_only = partition1_destinations(&mf, &man);
    assert_eq!(
        p1_only,
        vec![SceneDestination {
            scene_name: "dolk".into(),
            index: 60,
            entry_x: 0x10,
            entry_z: 0x20,
        }]
    );

    // ...and the merged scan sees it, as a superset of the P1 pass.
    let merged = scene_destinations(&mf, &man);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0], p1_only[0], "P1 results keep first-seen order");
    assert_eq!(
        merged[1],
        SceneDestination {
            scene_name: "jouina".into(),
            index: 2,
            entry_x: 0x08,
            entry_z: 0x09,
        }
    );
}

#[test]
fn scene_destinations_dedupe_across_partitions() {
    // The same (name, index) destination carried by BOTH a P1 script and a
    // P2 record folds to a single entry.
    let mut p1_script = scene_change_op(7, b"vell", 0x01, 0x02, 0x03);
    p1_script.push(0x21);
    let mut p2 = p2_prefix();
    p2.extend_from_slice(&scene_change_op(7, b"vell", 0x01, 0x02, 0x03));
    p2.push(0x21);
    let man = build_man_p1_p2(&[p1_record(&p1_script)], &[p2]);
    let mf = man_section::parse(&man).unwrap();

    let merged = scene_destinations(&mf, &man);
    assert_eq!(merged.len(), 1, "cross-partition duplicate is folded");
    assert_eq!(merged[0].scene_name, "vell");
}

#[test]
fn scene_destinations_p2_pass_rejects_text_desync_name() {
    // A 0x3F in a P2 record whose "name" is not a clean CDNAME label (the
    // literal-'?' desync hazard) is dropped by the clean-name gate.
    let mut p2 = p2_prefix();
    p2.extend_from_slice(&scene_change_op(0, b"Hi! ", 0, 0, 0));
    p2.push(0x21);
    let man = build_man_p1_p2(&[], &[p2]);
    let mf = man_section::parse(&man).unwrap();
    assert!(scene_destinations(&mf, &man).is_empty());
}

/// A field-VM dialog op: `0x49 0x00 0x00 | 0x1F <text> 0x00 | <trailing>`. Its
/// width is `6 + text.len` (header 3 + `0x1F` + text + terminator + 1 trailing),
/// which `field_disasm` recovers by walking the mes bytes to the `0x00`, so a
/// grown `text` keeps the fall-through decode in sync.
fn mes_run(text: &[u8]) -> Vec<u8> {
    let mut v = vec![0x49, 0x00, 0x00, 0x1F];
    v.extend_from_slice(text);
    v.push(0x00); // segment terminator
    v.push(0x00); // trailing byte the op width consumes
    v
}

/// A `0x26 JMP_REL` with a raw delta (target = `pc + 1 + delta`).
fn jmp_rel(delta: u16) -> Vec<u8> {
    let mut v = vec![0x26];
    v.extend_from_slice(&delta.to_le_bytes());
    v
}

#[test]
fn text_edit_grows_dialog_and_relocates_a_straddling_jump() {
    // Record script (pc0 = 6): a forward JMP over the dialog to the trailing
    // halt, then the dialog op, then the halt. Growing the dialog text shifts
    // the halt but not the jump - the classic straddling relative jump.
    let mut rec = p2_prefix(); // pc0 = 6
    // Lay out to learn the halt offset, then set the jump delta to reach it.
    let jmp = jmp_rel(0); // patched below
    let mes = mes_run(b"hi");
    let halt_rel = 6 + jmp.len() + mes.len(); // record-relative offset of 0x21
    // JMP target = jmp_pc + 1 + delta; jmp_pc(rel) = 6. delta = halt_rel - 7.
    let jmp = jmp_rel((halt_rel - 7) as u16);
    rec.extend_from_slice(&jmp);
    rec.extend_from_slice(&mes);
    rec.push(0x21); // halt (jump target)

    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let d = mf.data_region_offset;
    let old_sec0 = mf.sections[0].offset;

    // Locate the 0x1F segment's text and length by scanning the record.
    let seg_1f = man[d..].iter().position(|&b| b == 0x1F).unwrap() + d;
    let text_off = seg_1f + 1;
    let term = man[text_off..].iter().position(|&b| b == 0x00).unwrap() + text_off;
    let old_len = term - text_off;
    assert_eq!(&man[text_off..term], b"hi");

    // The jump decodes to the halt before the edit.
    let jmp_insn = field_disasm::decode(&man, d + 6).unwrap();
    let orig_target = match jmp_insn.info {
        field_disasm::InsnInfo::JmpRel { target, .. } => target,
        _ => panic!("expected JmpRel"),
    };

    let edit = TextEdit {
        offset: text_off,
        old_len,
        new_bytes: b"hello there".to_vec(), // +9 bytes
    };
    let out = apply_text_edits(&man, &[edit]).unwrap();
    assert_eq!(out.len(), man.len() + (11 - old_len));

    // Structure relocated: section 0 shifted by the growth; the record start
    // (before the edit) did not move.
    let mf2 = man_section::parse(&out).unwrap();
    assert_eq!(mf2.sections[0].offset, old_sec0 + (11 - old_len));
    assert_eq!(mf2.partitions[2][0], mf.partitions[2][0]);

    // The grown text is present, still `0x1F`-framed and `0x00`-terminated.
    let seg2 = out[d..].iter().position(|&b| b == 0x1F).unwrap() + d;
    let term2 = out[seg2 + 1..].iter().position(|&b| b == 0x00).unwrap() + seg2 + 1;
    assert_eq!(&out[seg2 + 1..term2], b"hello there");

    // The straddling jump's delta was recomputed: it still targets the halt,
    // now shifted by the growth.
    let jmp2 = field_disasm::decode(&out, d + 6).unwrap();
    let new_target = match jmp2.info {
        field_disasm::InsnInfo::JmpRel { target, .. } => target,
        _ => panic!("expected JmpRel"),
    };
    assert_eq!(new_target, orig_target + (11 - old_len));

    // Round-trip backstop: same program, relocated.
    assert!(text_edits_preserve_scripts(&man, &out));
}

#[test]
fn text_edit_shrinks_dialog_too() {
    let mut rec = p2_prefix();
    rec.extend_from_slice(&mes_run(b"a longer line"));
    rec.push(0x21);
    let man = build_man(&[rec]);
    let d = man_section::parse(&man).unwrap().data_region_offset;
    let seg = man[d..].iter().position(|&b| b == 0x1F).unwrap() + d;
    let text_off = seg + 1;
    let term = man[text_off..].iter().position(|&b| b == 0x00).unwrap() + text_off;
    let old_len = term - text_off;

    let out = apply_text_edits(
        &man,
        &[TextEdit {
            offset: text_off,
            old_len,
            new_bytes: b"hi".to_vec(),
        }],
    )
    .unwrap();
    assert_eq!(out.len(), man.len() - (old_len - 2));
    assert!(text_edits_preserve_scripts(&man, &out));
}

#[test]
fn text_edit_refuses_section_region_offset() {
    let mut rec = p2_prefix();
    rec.extend_from_slice(&mes_run(b"hi"));
    rec.push(0x21);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    // Section 0 lives past the record region; editing there is out of scope.
    let bad = mf.sections[0].offset + 1;
    let err = apply_text_edits(
        &man,
        &[TextEdit {
            offset: bad,
            old_len: 0,
            new_bytes: vec![0x41],
        }],
    )
    .unwrap_err();
    assert!(matches!(err, ManEditError::RecordNotFound { .. }));
}

#[test]
fn text_edit_after_a_wait_loop_leaves_the_backward_jump_alone() {
    // Record script (pc0 = 6): the per-frame park loop `[21] [26 FE FF]`
    // (a backward jump of -2 onto the halt), then the dialog op, then a halt.
    // Growing the dialog must not touch the loop: both its endpoints sit
    // before the edit. Reading the delta as unsigned put the loop's target
    // past the buffer, where every later splice shifted it while the base
    // stayed - rewriting the loop into a forward jump that no round-trip
    // check could see (an unresolvable target on both sides compares equal).
    let mut rec = p2_prefix(); // pc0 = 6
    rec.push(0x21); // rel 6: halt the loop parks on
    rec.extend_from_slice(&jmp_rel(0xFFFE)); // rel 7..10: jump back to rel 6
    rec.extend_from_slice(&mes_run(b"hi"));
    rec.push(0x21);
    let man = build_man(&[rec]);
    let mf = man_section::parse(&man).unwrap();
    let d = mf.data_region_offset;

    let loop_jmp = field_disasm::decode(&man, d + 7).unwrap();
    match loop_jmp.info {
        field_disasm::InsnInfo::JmpRel { delta, target } => {
            assert_eq!(delta, 0xFFFE);
            assert_eq!(target, d + 6, "the loop parks on its own halt");
        }
        _ => panic!("expected JmpRel"),
    }

    let seg_1f = man[d..].iter().position(|&b| b == 0x1F).unwrap() + d;
    let text_off = seg_1f + 1;
    let term = man[text_off..].iter().position(|&b| b == 0x00).unwrap() + text_off;
    let edit = TextEdit {
        offset: text_off,
        old_len: term - text_off,
        new_bytes: b"hello there, stranger".to_vec(),
    };
    let out = apply_text_edits(&man, &[edit]).unwrap();

    // The loop's bytes are untouched and it still decodes onto its halt.
    assert_eq!(&out[d + 7..d + 10], &[0x26, 0xFE, 0xFF]);
    let loop_jmp2 = field_disasm::decode(&out, d + 7).unwrap();
    match loop_jmp2.info {
        field_disasm::InsnInfo::JmpRel { delta, target } => {
            assert_eq!(delta, 0xFFFE);
            assert_eq!(target, d + 6);
        }
        _ => panic!("expected JmpRel"),
    }
    assert!(text_edits_preserve_scripts(&man, &out));
}

/// Build a synthetic MAN with records in all three partitions (each the
/// *full* record bytes), the offset table in `[P0..P1..P2]` order, section 0
/// right after the records and six zero-length terminator sections.
fn build_man_parts(p0: &[Vec<u8>], p1: &[Vec<u8>], p2: &[Vec<u8>]) -> Vec<u8> {
    let (n0, n1, n2) = (p0.len(), p1.len(), p2.len());
    let data_region = RECORDS_BEGIN_OFFSET + 3 * (n0 + n1 + n2);
    let mut blob = Vec::new();
    let mut offsets = Vec::new();
    for rec in p0.iter().chain(p1).chain(p2) {
        offsets.push(blob.len() as u32);
        blob.extend_from_slice(rec);
    }
    let u24_at_28 = blob.len() as u32;
    let mut man = vec![0u8; data_region];
    for (at, n) in [(0x22, n0), (0x24, n1), (0x26, n2)] {
        man[at] = (n & 0xFF) as u8;
        man[at + 1] = ((n >> 8) & 0xFF) as u8;
    }
    man[0x28] = (u24_at_28 & 0xFF) as u8;
    man[0x29] = ((u24_at_28 >> 8) & 0xFF) as u8;
    man[0x2A] = ((u24_at_28 >> 16) & 0xFF) as u8;
    let mut cur = RECORDS_BEGIN_OFFSET;
    for off in &offsets {
        man[cur] = (off & 0xFF) as u8;
        man[cur + 1] = ((off >> 8) & 0xFF) as u8;
        man[cur + 2] = ((off >> 16) & 0xFF) as u8;
        cur += 3;
    }
    man.extend_from_slice(&blob);
    man.extend_from_slice(&[0u8; 18]);
    man
}

/// A bare text segment `1F <text> 00`.
fn text_seg(text: &[u8]) -> Vec<u8> {
    let mut v = vec![0x1F];
    v.extend_from_slice(text);
    v.push(0x00);
    v
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .expect("needle present")
}

#[test]
fn partition0_record_walk_starts_after_the_attr_byte() {
    // Partition-0 header `[n=1]["AB"][attr]`: pc0 = 4. Script: two nops, a
    // sign text, then the park loop jumping back to the text lead.
    let mut rec = vec![0x01, b'A', b'B', 0x00, 0x25, 0x21];
    rec.extend(text_seg(b"Hi")); // script offsets 2..6
    rec.extend(jmp_rel(0xFFFB)); // base 7 -> target 2 (the `1F`)
    let man = build_man_parts(&[rec], &[], &[]);
    let off = find(&man, b"\x1FHi\0") + 1;
    // With the partition-1 formula (pc0 = 7) the walk would start inside
    // the text; the record's own header puts it on the first opcode.
    assert_eq!(text_site(&man, off), TextSite::Segment);
    let out = apply_text_edits(
        &man,
        &[TextEdit {
            offset: off,
            old_len: 2,
            new_bytes: b"Hello".to_vec(),
        }],
    )
    .expect("grow");
    assert!(text_edits_preserve_scripts(&man, &out));
    // The backward jump now spans three more bytes: -5 becomes -8.
    let jmp = find(&out, b"\x1FHello\0") + 7;
    assert_eq!(&out[jmp..jmp + 3], &[0x26, 0xF8, 0xFF]);
}

#[test]
fn picker_jump_entries_relocate_across_a_grown_label() {
    // prompt, 2-option picker, two labels, then the two branch handlers.
    let mut script = text_seg(b"Q?"); // 0..3
    script.push(0x27); // open at 4; entries at 5..6 and 7..8
    script.extend_from_slice(&(13i16).to_le_bytes()); // 5 + 13 = 18
    script.extend_from_slice(&(12i16).to_le_bytes()); // 7 + 12 = 19
    script.extend(text_seg(b"Yes")); // 9..13
    script.extend(text_seg(b"No")); // 14..17
    script.push(0x21); // handler 0 at 18
    script.extend_from_slice(&[0x2B, 0x00]); // handler 1 at 19
    script.push(0x21);
    let man = build_man_parts(&[], &[p1_record(&script)], &[]);
    let label = find(&man, b"\x1FYes\0") + 1;
    assert_eq!(text_site(&man, label), TextSite::Segment);
    let out = apply_text_edits(
        &man,
        &[TextEdit {
            offset: label,
            old_len: 3,
            new_bytes: b"Sim!!".to_vec(),
        }],
    )
    .expect("grow");
    assert!(text_edits_preserve_scripts(&man, &out));
    let open = find(&out, b"\x1FQ?\0") + 4;
    let p = field_disasm::decode(&out, open).unwrap();
    let InsnInfo::Picker { targets, .. } = p.info else {
        panic!("not a picker");
    };
    // Both handlers moved two bytes; the entries did not, so every delta grew.
    assert_eq!(&out[targets[0]..targets[0] + 1], &[0x21]);
    assert_eq!(&out[targets[1]..targets[1] + 2], &[0x2B, 0x00]);
    assert_eq!(targets[0], open + 1 + 15);
    assert_eq!(targets[1], open + 3 + 14);
}

#[test]
fn text_site_separates_dialog_from_operand_runs_and_unwalked_text() {
    // `CC 1F 50 4B 00`: a cross-context MENU_CTRL whose operand bytes read
    // `1F "PK" 00`; then a real segment; then an undecodable byte that ends
    // the clean walk; then a segment the walk never reaches.
    let mut script = vec![0xCC, 0x1F, 0x50, 0x4B, 0x00];
    script.extend(text_seg(b"Hi"));
    script.push(0x00);
    script.extend(text_seg(b"Yo"));
    let man = build_man_parts(&[], &[p1_record(&script)], &[]);
    let pk = find(&man, b"\x1FPK\0") + 1;
    let hi = find(&man, b"\x1FHi\0") + 1;
    let yo = find(&man, b"\x1FYo\0") + 1;
    assert_eq!(text_site(&man, pk), TextSite::Operand);
    assert_eq!(text_site(&man, hi), TextSite::Segment);
    assert_eq!(text_site(&man, yo), TextSite::Unreached);
    let edit = |offset: usize| TextEdit {
        offset,
        old_len: 2,
        new_bytes: b"Bem!".to_vec(),
    };
    assert_eq!(
        apply_text_edits(&man, &[edit(pk)]),
        Err(ManEditError::NotTextSegment { offset: pk })
    );
    assert_eq!(
        apply_text_edits(&man, &[edit(yo)]),
        Err(ManEditError::UnwalkedText { offset: yo })
    );
    assert!(apply_text_edits(&man, &[edit(hi)]).is_ok());
}
