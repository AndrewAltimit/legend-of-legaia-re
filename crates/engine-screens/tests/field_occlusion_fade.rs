//! The camera-occlusion fade's world half (`field_occlusion`), the arming
//! decision both play hosts read: armed only in field free roam, and never
//! without a player actor to aim at.
//!
//! See `docs/tooling/host-drift.md` - "One decision, two inputs".

use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_screens::field_occlusion;

/// The fade's world half: field mode, no scripted shot.
#[test]
fn occlusion_fade_arms_in_field_free_roam_only() {
    let field = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    assert!(field_occlusion::fade_armed(&field, false));
    assert!(
        !field_occlusion::fade_armed(&field, true),
        "cutscene camera"
    );
    let battle = World {
        mode: SceneMode::Battle,
        ..World::default()
    };
    assert!(!field_occlusion::fade_armed(&battle, false));
    let map = World {
        mode: SceneMode::WorldMap,
        ..World::default()
    };
    assert!(!field_occlusion::fade_armed(&map, false));
}

/// With no player actor there is no body centre - the same guard that stops
/// the fade arming at all, so neither host can stage a focus at the origin.
#[test]
fn occlusion_focus_needs_a_player_actor() {
    let w = World::default();
    assert!(field_occlusion::player_body_centre(&w).is_none());
}
