//! Scene-level gates: the hidden-NPC restore and the prologue colour grade and
//! depth cue.

use crate::world::World;
use legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;

#[test]
fn restore_hidden_field_npcs_unparks_only_the_hide_box() {
    let mut w = World::default();
    // Two townsfolk parked off-map by the opening cutscene, one NPC left at
    // a real tile (e.g. a mid-scene walker), plus stale headings for all.
    let hide = FIELD_OFFMAP_HIDE_XZ;
    w.npcs.positions.insert(1, (hide, hide));
    w.npcs.positions.insert(2, (hide, hide));
    w.npcs.positions.insert(3, (2880, 5440));
    w.npcs.headings.insert(1, 0x800);
    w.npcs.headings.insert(2, 0x000);
    w.npcs.headings.insert(3, 0x400);

    w.restore_hidden_field_npcs();

    // The hide-box NPCs lose their overrides (render falls back to the MAN
    // spawn); the on-tile NPC and its heading are untouched.
    assert!(!w.npcs.positions.contains_key(&1));
    assert!(!w.npcs.positions.contains_key(&2));
    assert!(!w.npcs.headings.contains_key(&1));
    assert!(!w.npcs.headings.contains_key(&2));
    assert_eq!(w.npcs.positions.get(&3), Some(&(2880, 5440)));
    assert_eq!(w.npcs.headings.get(&3), Some(&0x400));
}

#[test]
fn restore_hidden_field_npcs_noop_when_none_parked() {
    let mut w = World::default();
    w.npcs.positions.insert(5, (1000, 2000));
    w.restore_hidden_field_npcs();
    assert_eq!(w.npcs.positions.get(&5), Some(&(1000, 2000)));
}

#[test]
fn scene_color_grade_only_on_the_prologue_cutscene() {
    let mut w = World::new();
    // No scene / arbitrary field scene -> ungraded natural colour.
    assert!(w.scene_color_grade().is_none());
    w.set_active_scene_label("town01");
    assert!(
        w.scene_color_grade().is_none(),
        "the Rim Elm hand-off renders in full colour"
    );
    // The opdeene prologue cutscene renders through the warm sepia grade.
    w.set_active_scene_label(legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE);
    assert_eq!(
        w.scene_color_grade(),
        Some(legaia_engine_system::fade::ColorGrade::PROLOGUE_SEPIA)
    );
}

#[test]
fn scene_depth_cue_tracks_the_prologue_grade_gate() {
    let mut w = World::new();
    // Interactive scenes stage NO depth-cue ramp: the renderer's ramp-off
    // path is the pre-ramp identity, so town01 pixels are unchanged.
    assert!(w.scene_depth_cue().is_none());
    w.set_active_scene_label("town01");
    assert!(
        w.scene_depth_cue().is_none(),
        "interactive field renders without the far-colour pull"
    );
    // All three prologue legs pull toward the gold far colour.
    for scene in ["opdeene", "opstati", "opurud"] {
        w.set_active_scene_label(scene);
        assert_eq!(
            w.scene_depth_cue(),
            Some(legaia_engine_system::fade::DepthCueRamp::PROLOGUE_GOLD),
            "{scene} stages the prologue depth-cue ramp"
        );
    }
}
