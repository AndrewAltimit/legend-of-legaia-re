use super::*;

fn score_rows() -> [ScoreRow; COURSE_COUNT] {
    [[0i32; MAX_ROUNDS_PER_COURSE]; COURSE_COUNT]
}

fn unlocked(courses: [bool; COURSE_COUNT]) -> ContestFlags {
    ContestFlags {
        course_unlock: courses,
        ..ContestFlags::default()
    }
}

/// The Beginner course keeps its gear; the restore still refills.
#[test]
fn beginner_course_does_not_strip_gear() {
    let mut c = DomeContest::enter(&unlocked([true, false, false]), [8, 8, 13], score_rows());
    assert_eq!(c.course(), 0);
    let r = c.take_start_restore().expect("the first entry restores");
    assert!(!r.strip_gear);
}

/// Every course above Beginner strips - the `bnez DAT_801D1A90` arm.
#[test]
fn higher_courses_strip_gear() {
    let mut c = DomeContest::enter(&unlocked([true, true, false]), [8, 8, 13], score_rows());
    assert!(c.course() > 0);
    assert!(c.take_start_restore().expect("first entry").strip_gear);
}

/// Retail's re-entry arm jumps past the `jal`, so the restore is a
/// one-shot: a later leg never refills.
#[test]
fn restore_is_one_shot() {
    let mut c = DomeContest::enter(&unlocked([true, false, false]), [8, 8, 13], score_rows());
    assert!(c.take_start_restore().is_some());
    assert!(c.take_start_restore().is_none());
}

/// HP / MP / SP all come back full, and only the four gear bytes go -
/// the Seru lock `+0x199` and the three accessories `+0x19B..+0x19D`
/// survive a stripped entry.
#[test]
fn refills_all_three_pools_and_strips_only_gear() {
    let mut rec = legaia_save::CharacterRecord::zeroed();
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = 400;
    hms.hp_cur = 12;
    hms.mp_max = 90;
    hms.mp_cur = 0;
    hms.sp_max = 60;
    hms.sp_cur = 3;
    rec.set_hp_mp_sp(hms);
    rec.set_equipment(legaia_save::EquipmentSlots {
        slots: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
    });

    apply_contest_start_restore(&mut rec, ContestStartRestore { strip_gear: true });

    let out = rec.hp_mp_sp();
    assert_eq!((out.hp_cur, out.mp_cur, out.sp_cur), (400, 90, 60));
    assert_eq!(
        rec.equipment().slots,
        [0, 0, 0, 0x44, 0, 0x66, 0x77, 0x88],
        "only armour / head / weapon / leg gear are zeroed"
    );
}

/// The un-stripped arm leaves every equipment byte alone.
#[test]
fn beginner_refill_leaves_equipment_untouched() {
    let mut rec = legaia_save::CharacterRecord::zeroed();
    let slots = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    rec.set_equipment(legaia_save::EquipmentSlots { slots });
    let mut hms = rec.hp_mp_sp();
    hms.hp_max = 250;
    rec.set_hp_mp_sp(hms);

    apply_contest_start_restore(&mut rec, ContestStartRestore { strip_gear: false });

    assert_eq!(rec.equipment().slots, slots);
    assert_eq!(rec.hp_mp_sp().hp_cur, 250);
}
