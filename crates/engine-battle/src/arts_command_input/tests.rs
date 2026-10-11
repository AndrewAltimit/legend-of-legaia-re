use super::*;

/// The seat loop's table `[0x0D, 0x10, 0x11, 0x0C]` plus Noa's swap and
/// the empty-arm `+ 2`: Vahn / Gala read `Arms | RaSeru`, Noa
/// `RaSeru | Arms` - the `arts_bar_*` captures - and an empty arm its
/// direction word.
#[test]
fn chip_icons_follow_the_seat_table_the_noa_swap_and_empty_arms() {
    use chip_icon::*;
    let full = [0, 0, 1, 1, 0];
    assert_eq!(retail_chip_icons(1, &full), [ARMS, RASERU, LOW, HIGH]);
    assert_eq!(retail_chip_icons(3, &full), [ARMS, RASERU, LOW, HIGH]);
    assert_eq!(retail_chip_icons(2, &full), [RASERU, ARMS, LOW, HIGH]);
    // The empty check keys on the unswapped seat: index 2 for Left.
    assert_eq!(
        retail_chip_icons(1, &[0, 0, 0, 1]),
        [LEFT, RASERU, LOW, HIGH]
    );
    assert_eq!(
        retail_chip_icons(2, &[0, 0, 1, 0]),
        [RASERU, RIGHT, LOW, HIGH]
    );
    assert_eq!(retail_chip_icons(1, &[]), PLAIN_CHIP_ICONS);
}

#[test]
fn rot_blocks_each_limb_s_directions() {
    assert!(rot_blocks(0x08, Command::Left));
    assert!(!rot_blocks(0x08, Command::Right));
    assert!(rot_blocks(0x10, Command::Right));
    assert!(rot_blocks(0x20, Command::Up) && rot_blocks(0x20, Command::Down));
    assert!(!rot_blocks(0x20, Command::Left));
    let all = 0x38;
    for c in [Command::Left, Command::Right, Command::Up, Command::Down] {
        assert!(rot_blocks(all, c));
        assert!(!rot_blocks(0x1007, c), "non-Rot bits gate nothing");
    }
}
use legaia_art::queue::ActionConstant;

fn alive(present: bool) -> SlotState {
    SlotState::alive(present, true)
}
fn party3() -> [SlotState; 3] {
    [alive(true), alive(true), alive(true)]
}
fn one_monster() -> [SlotState; 5] {
    [
        alive(true),
        SlotState::default(),
        SlotState::default(),
        SlotState::default(),
        SlotState::default(),
    ]
}
fn press(b: &str) -> ArtsCommandPad {
    ArtsCommandPad {
        up: b == "U",
        down: b == "D",
        left: b == "L",
        right: b == "R",
        cross: b == "c",
        circle: b == "o",
        triangle: b == "t",
    }
}
fn rec(action: u8, cmds: &[Command], power: &[u8]) -> (ActionConstant, ArtRecord) {
    (
        ActionConstant::from_byte(action).unwrap(),
        ArtRecord {
            action: ActionConstant::from_byte(action).unwrap(),
            commands: cmds.to_vec(),
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: power.iter().map(|&b| PowerByte::from_byte(b)).collect(),
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::None,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        },
    )
}

#[test]
fn press_appends_and_debits_per_command_cost() {
    // Off-class arm: Left costs 42, the others 30. Pool 120 leaves 48
    // after the two presses, so the entry is still live and the test
    // measures the debit rather than the auto-end.
    let mut s = ArtsCommandInputSession::new(0, 0, 120, [42, 30, 30, 30], 0);
    s.input(press("U"), party3(), one_monster());
    s.input(press("L"), party3(), one_monster());
    assert_eq!(s.buffer, vec![4, 1]);
    assert_eq!(s.spent, vec![30, 42], "each press debits its own cost");
    assert_eq!(s.pool, 48);
    assert!(matches!(s.phase, ArtsInputPhase::Entering));
}

#[test]
fn entry_auto_ends_when_nothing_is_affordable() {
    // Pool 90 at flat cost 30: the third press exhausts the pool and
    // the entry ends by itself (retail 0x50 -> 0x5A, no confirm).
    let mut s = ArtsCommandInputSession::new(0, 0, 90, [30; 4], 0);
    s.input(press("U"), party3(), one_monster());
    s.input(press("D"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Entering));
    s.input(press("U"), party3(), one_monster());
    assert_eq!(s.buffer.len(), 3);
    assert_eq!(s.pool, 0);
    assert!(matches!(s.phase, ArtsInputPhase::Review), "auto-ended");
}

#[test]
fn unaffordable_press_is_refused_and_pool_untouched() {
    // Pool 116, arm (Left) 42: two Lefts leave 32 - too little for a
    // third arm (42) but enough for a plain swing (30), so the entry
    // stays live and the unaffordable press is simply dropped.
    let mut s = ArtsCommandInputSession::new(0, 0, 116, [42, 30, 30, 30], 0);
    s.input(press("L"), party3(), one_monster());
    s.input(press("L"), party3(), one_monster());
    assert_eq!(s.pool, 32);
    assert!(matches!(s.phase, ArtsInputPhase::Entering));
    s.input(press("L"), party3(), one_monster());
    assert_eq!(s.buffer.len(), 2, "third arm press refused");
    assert_eq!(s.pool, 32, "a refused press does not debit");
    assert!(
        matches!(s.phase, ArtsInputPhase::Entering),
        "entry stays live"
    );
    // An affordable swing still lands, and exhausts the pool.
    s.input(press("U"), party3(), one_monster());
    assert_eq!(s.buffer.len(), 3);
    assert_eq!(s.pool, 2);
    assert!(matches!(s.phase, ArtsInputPhase::Review), "auto-ended");
}

#[test]
fn circle_on_the_review_restarts_the_entry() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
    s.input(press("U"), party3(), one_monster());
    s.input(press("D"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Review));
    s.input(press("o"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Entering));
    assert!(s.buffer.is_empty());
    assert_eq!(s.pool, 60, "the cancel re-seeds the pool");
}

/// The review's press goes straight to the target: there is no per-member
/// Begin | Reselect in the session - that screen is the party's, raised by
/// the World after the last member commits.
#[test]
fn the_review_press_resolves_through_the_target_picker() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
    s.input(press("U"), party3(), one_monster());
    s.input(press("U"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Review));
    s.input(press("c"), party3(), one_monster()); // review -> target
    // One monster: the picker may resolve immediately or need one
    // confirm.
    if s.resolved().is_none() {
        s.input(press("c"), party3(), one_monster());
    }
    assert_eq!(
        s.resolved(),
        Some(ArtsInputResolution::Confirmed {
            target_row: CursorRow::Enemy,
            target_slot: 0,
        })
    );
}

/// The open-time preseed replays on a bare confirm, costs nothing from
/// the entry pool, and survives nothing but an empty entry: the first
/// press wipes it, and so does the review's cancel.
#[test]
fn a_bare_confirm_replays_the_preseed_without_a_charge() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0).with_preseed(vec![4, 3]);
    s.input(press("c"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Review));
    assert!(s.replay);
    assert_eq!(s.committed_string(), &[4, 3]);
    assert_eq!(s.pool, 60, "no press was charged");
    assert!(s.spent.is_empty());

    // The review's cancel wipes the window: back to a plain entry.
    s.input(press("o"), party3(), one_monster());
    assert!(matches!(s.phase, ArtsInputPhase::Entering));
    assert!(!s.replay && s.preseed.is_empty());
    s.input(press("c"), party3(), one_monster());
    assert!(
        matches!(s.phase, ArtsInputPhase::Entering),
        "nothing left to replay"
    );

    // The first press wipes it too.
    let mut t = ArtsCommandInputSession::new(0, 0, 90, [30; 4], 0).with_preseed(vec![4, 3]);
    t.input(press("L"), party3(), one_monster());
    assert!(t.preseed.is_empty());
    t.input(press("c"), party3(), one_monster());
    assert!(matches!(t.phase, ArtsInputPhase::Review));
    assert!(!t.replay);
    assert_eq!(t.committed_string(), &[1]);
}

/// The gauge build draws the preseed as pennants and cuts it at the
/// first command one full pool cannot pay (case `0x2C`,
/// `0x801D4E90` / `0x801D4DC4`) - Gala's off-class capture: a `42`-AP
/// arm, a `197` pool, and a saved `Right Right Left Left Left Up` that
/// opens as five pennants.
#[test]
fn the_gauge_build_draws_the_affordable_preseed_prefix() {
    let s = ArtsCommandInputSession::new(0, 0, 197, [42, 30, 30, 30], 0)
        .with_preseed(vec![2, 2, 1, 1, 1, 4]);
    assert_eq!(s.preseed, [2, 2, 1, 1, 1], "the unaffordable Up is cut");
    assert_eq!(s.bar_commands(), [2, 2, 1, 1, 1]);
    assert_eq!(s.bar_spent(), [30, 30, 42, 42, 42]);
    assert_eq!(s.pool, 197, "drawing the preseed charges nothing");

    // A bare confirm replays the cut string.
    let mut r = s.clone();
    r.input(press("c"), party3(), one_monster());
    assert_eq!(r.committed_string(), &[2, 2, 1, 1, 1]);
    assert_eq!(r.bar_commands(), [2, 2, 1, 1, 1], "the review keeps them");

    // The first press closes the preseeded pennants.
    let mut t = s;
    t.input(press("U"), party3(), one_monster());
    assert_eq!(t.bar_commands(), [4]);
    assert_eq!(t.bar_spent(), [30]);
}

#[test]
fn circle_on_empty_buffer_aborts() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
    s.input(press("o"), party3(), one_monster());
    assert_eq!(s.resolved(), Some(ArtsInputResolution::Aborted));
}

/// Retail's cancel mask on a **non-empty** buffer is `FUN_801D388C`
/// case `0x26`: the sixteen queue bytes are wiped, the pool is re-seeded
/// from AGL and the committed count is zeroed, with `ctx+0x06` never
/// written - so the entry restarts in place instead of ending.
///
/// The distinction from [`circle_on_empty_buffer_aborts`] is the whole
/// point: the same button leaves the entry only when there is nothing
/// to clear, which is why one press cannot be read as the other.
#[test]
fn circle_on_a_typed_buffer_resets_the_entry_instead_of_leaving_it() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
    s.input(press("U"), party3(), one_monster());
    assert_eq!(s.buffer.len(), 1, "the direction was accepted");
    assert_eq!(s.pool, 30, "and debited its cost");

    s.input(press("o"), party3(), one_monster());

    assert_eq!(s.resolved(), None, "the entry is still open, not aborted");
    assert!(matches!(s.phase, ArtsInputPhase::Entering), "still in 0x50");
    assert!(s.buffer.is_empty(), "the queue is wiped");
    assert!(s.spent.is_empty());
    assert_eq!(s.pool, s.pool_max, "the pool is re-seeded in full");

    // And a second Circle - now on the empty buffer it just made - is
    // the leave press, so the reset is not a one-way trap.
    s.input(press("o"), party3(), one_monster());
    assert_eq!(s.resolved(), Some(ArtsInputResolution::Aborted));
}

#[test]
fn the_retail_pager_steps_at_most_three_pages() {
    assert_eq!(arts_list_pager(None, 0), None, "no art: inert");
    assert_eq!(arts_list_pager(Some(0), 0), Some(0));
    assert_eq!(arts_list_pager(None, 3), Some(0));
    assert_eq!(arts_list_pager(Some(0), 5), None, "one page closes");
    assert_eq!(arts_list_pager(Some(0), 6), Some(1));
    assert_eq!(arts_list_pager(Some(1), 10), None);
    assert_eq!(arts_list_pager(Some(1), 11), Some(2));
    assert_eq!(arts_list_pager(Some(2), 16), None, "the third page closes");
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 9).with_list_rows(7);
    assert_eq!(s.list_pages, 2);
    s.input(press("t"), party3(), one_monster());
    s.input(press("t"), party3(), one_monster());
    assert_eq!(s.list_page, Some(1));
    s.input(press("t"), party3(), one_monster());
    assert_eq!(s.list_page, None);
}

#[test]
fn triangle_cycles_the_arts_list_and_is_inert_without_pages() {
    let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 2);
    assert_eq!(s.list_page, None);
    s.input(press("t"), party3(), one_monster());
    assert_eq!(s.list_page, Some(0));
    s.input(press("t"), party3(), one_monster());
    assert_eq!(s.list_page, Some(1));
    s.input(press("t"), party3(), one_monster());
    assert_eq!(s.list_page, None, "last page closes");
    // No pages: the toggle is inert (retail's no-learned-art case).
    let mut none = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
    none.input(press("t"), party3(), one_monster());
    assert_eq!(none.list_page, None);
}

#[test]
fn resolver_matches_arts_and_leaves_swings_synthetic() {
    use Command::{Down, Up};
    // Art 0x1B = [Up, Up] with two damage bytes; art 0x1C = [Down].
    let records = vec![
        rec(0x1B, &[Up, Up], &[0x1A, 0x1A]),
        rec(0x1C, &[Down], &[0x1B]),
    ];
    // Left (unmatched swing) + Up Up (art) + Down (art).
    let entry = resolve_entered_commands(&records, &[1, 4, 4, 3]);
    assert_eq!(entry.matched.len(), 2);
    assert_eq!(entry.matched[0].as_byte(), 0x1B);
    assert_eq!(entry.matched[1].as_byte(), 0x1C);
    // 1 synthetic + 2 (art 0x1B) + 1 (art 0x1C) strikes.
    assert_eq!(entry.power.len(), 4);
    assert_eq!(entry.action.map(|a| a.as_byte()), Some(0x1B));
}

#[test]
fn resolver_with_no_catalog_is_all_synthetic() {
    let entry = resolve_entered_commands(&[], &[4, 3, 1]);
    assert!(entry.matched.is_empty());
    assert_eq!(entry.power.len(), 3);
    assert_eq!(entry.action, None);
    // Empty buffer floors at one hit.
    assert_eq!(resolve_entered_commands(&[], &[]).power.len(), 1);
}
