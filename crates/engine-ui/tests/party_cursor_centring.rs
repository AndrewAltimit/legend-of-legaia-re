//! The field party cursor and the battle party panels centre a roster the
//! same way: `FUN_801F1278` seeds a solo member into the middle cell and a
//! pair into the outer two, and the battle panels place one panel for a solo
//! member and two for a pair. The field kernel and the battle HUD model live
//! in different crates, so the pin sits here, above both.

use legaia_engine_battle_vm::battle_party_panel::panel_anchors;
use legaia_engine_field::field_subsystem_enter::seed_member_cells;

#[test]
fn the_centring_matches_the_field_party_cursor() {
    let cells_one = seed_member_cells(&[5]);
    let cells_two = seed_member_cells(&[5, 6]);
    assert_eq!(cells_one[1], 5, "solo takes the middle cell");
    assert_eq!(
        (cells_two[0], cells_two[2]),
        (5, 6),
        "a pair takes the outers"
    );
    assert_eq!(
        panel_anchors(1).unwrap().1,
        None,
        "solo positions one panel"
    );
    assert!(
        panel_anchors(2).unwrap().1.is_some(),
        "a pair positions two"
    );
}
