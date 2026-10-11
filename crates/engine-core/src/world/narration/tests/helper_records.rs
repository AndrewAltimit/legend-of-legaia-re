//! Spawned helper records: install, the one-shot gate, the bounded context
//! table, index rebase and the opening-chain abandon.

use crate::world::World;

/// Build a minimal MAN whose partition 2 carries the given record
/// bodies (each already in the named-record shape from [`p2_record`]).
/// `n0` / `n1` fill the partition-0/1 counts so the op-`0x44` global
/// re-base (`global - N0 - N1`) is exercised.
fn man_with_p2_records(
    records: &[Vec<u8>],
    n0: i16,
    n1: i16,
) -> (legaia_scene_formats::man_section::ManFile, Vec<u8>) {
    use legaia_scene_formats::man_section::{ManFile, ManHeader, SectionRef};
    let data_region_offset = 0x40usize;
    let mut man = vec![0u8; data_region_offset];
    let mut offsets = Vec::new();
    for body in records {
        offsets.push((man.len() - data_region_offset) as u32);
        man.extend_from_slice(body);
    }
    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [n0, n1, records.len() as i16],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![], offsets],
        data_region_offset,
        sections: std::array::from_fn(|_| SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

/// A partition-2 named-record body: 1-char name, empty C0, the given C1
/// story-flag OR-gate, empty C2, then `script`.
fn p2_record(c1: &[u16], script: &[u8]) -> Vec<u8> {
    let mut r = vec![1u8, 0x41, 0x00]; // name_len=1 + 2 SJIS name bytes
    r.push(0); // C0 empty
    r.push(c1.len() as u8);
    for f in c1 {
        r.extend_from_slice(&f.to_le_bytes());
    }
    r.push(0); // C2 empty
    r.extend_from_slice(script);
    r
}

#[test]
fn helper_record_installs_and_executes_without_modal_attributes() {
    // A mid-play spawned record (GFLAG_SET 26 then run-off-end) installs
    // as a concurrent helper context: it executes its script by the next
    // frame slice and never touches the modal timeline slot.
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 0, 0);
    let mut w = World::new();
    assert!(w.install_helper_record(&mf, &man, 0));
    assert_eq!(w.field_vm.helper_contexts.len(), 1);
    assert!(
        !w.cutscene_timeline_active(),
        "a helper spawn never installs the modal cutscene timeline"
    );
    w.step_helper_contexts();
    assert_ne!(
        w.flags.story_flags & crate::world::PROLOGUE_HANDOFF_FLAG,
        0,
        "the helper record's GFLAG_SET executed"
    );
    assert!(
        w.field_vm.helper_contexts.is_empty(),
        "a completed helper context is dropped from the table"
    );
}

#[test]
fn helper_record_honors_c1_one_shot_gate() {
    // C1 blocks the spawn when ANY listed flag is set (the one-shot
    // latch) - the same FUN_8003BDE0 gate walk as the modal install.
    let (mf, man) = man_with_p2_records(&[p2_record(&[0x0193], &[0x21])], 0, 0);
    let mut w = World::new();
    assert!(w.install_helper_record(&mf, &man, 0), "clear flag: spawns");
    w.field_vm.helper_contexts.clear();
    w.system_flag_set(0x0193);
    assert!(
        !w.install_helper_record(&mf, &man, 0),
        "latched C1 flag blocks the spawn"
    );
    assert!(w.field_vm.helper_contexts.is_empty());
}

#[test]
fn helper_spawn_while_timeline_active_is_not_dropped() {
    // A second spawned record while a modal timeline (or another helper)
    // executes must not be dropped: both helper contexts coexist with the
    // active timeline in the bounded context table.
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let long_wait = &[0x4A, 0xFF, 0x7F]; // WAIT_FRAMES 0x7FFF: stays live
    let (mf, man) = man_with_p2_records(
        &[p2_record(&[], long_wait), p2_record(&[], long_wait)],
        0,
        0,
    );
    let mut w = World::new();
    w.cutscene.timeline = Some(CutsceneTimeline::new(long_wait.to_vec(), 0));
    assert!(w.cutscene_timeline_active());
    assert!(w.install_helper_record(&mf, &man, 0));
    assert!(w.install_helper_record(&mf, &man, 1));
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        2,
        "concurrent spawns coexist with the modal timeline"
    );
    w.step_helper_contexts();
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        2,
        "waiting helper contexts stay installed across a frame"
    );
    assert!(w.cutscene_timeline_active(), "the modal slot is untouched");
}

#[test]
fn helper_context_table_is_bounded() {
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x4A, 0xFF, 0x7F])], 0, 0);
    let mut w = World::new();
    for _ in 0..crate::world::SPAWNED_CONTEXT_SLOTS {
        assert!(w.install_helper_record(&mf, &man, 0));
    }
    assert!(
        !w.install_helper_record(&mf, &man, 0),
        "a full context table refuses further spawns"
    );
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        crate::world::SPAWNED_CONTEXT_SLOTS
    );
}

#[test]
fn spawned_helper_record_rebases_global_index() {
    // op-0x44 carries a GLOBAL record index; the install re-bases it into
    // partition 2 (`global - N0 - N1`, retail FUN_8003BDE0).
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 3, 4);
    let mut w = World::new();
    assert!(
        !w.install_spawned_helper_record(&mf, &man, 2),
        "a global index below N0+N1 cannot re-base"
    );
    assert!(w.install_spawned_helper_record(&mf, &man, 7));
    assert_eq!(w.field_vm.helper_contexts.len(), 1);
}

/// A picker entry mid-opening tears the chain down - flag, timeline,
/// narration - and reports that something was live; a second call is a
/// quiet no-op.
#[test]
fn abandon_opening_chain_tears_down_and_reports() {
    let mut w = World::default();
    w.cutscene.opening_chain_active = true;
    w.cutscene.timeline =
        Some(legaia_engine_field::cutscene_timeline::CutsceneTimeline::new(vec![0], 0));
    assert!(w.cutscene_timeline_active());
    assert!(w.abandon_opening_chain());
    assert!(!w.cutscene.opening_chain_active);
    assert!(!w.cutscene_timeline_active());
    assert!(w.cutscene.narration.is_none() && w.cutscene.card.is_none());
    assert!(!w.cutscene.entering_town01_opening);
    assert!(!w.abandon_opening_chain(), "nothing live the second time");
}
