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

/// A field world with a modal timeline installed: `stepped` says whether it
/// has taken a slice yet.
fn field_with_timeline(stepped: bool) -> World {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    // A long wait: the timeline stays live.
    let mut tl = CutsceneTimeline::new(vec![0x4A, 0xFF, 0x7F], 0);
    tl.stepped = stepped;
    w.cutscene.timeline = Some(tl);
    w
}

const ON: field_occlusion::FadeHostTerms = field_occlusion::FadeHostTerms {
    enabled: true,
    debug_camera: false,
    screen_owned: false,
};

/// A script context that holds the player owns the frame, staged shot or
/// not: the fade stands down for the whole span, not only while an op-`0x45`
/// set is staged (the gaps between beats and an APPLY's ease back are the
/// script's framing too).
#[test]
fn a_record_holding_the_player_stands_the_fade_down() {
    use field_occlusion::FadeArming;
    let held = field_with_timeline(true);
    assert!(held.script_holds_camera());
    assert!(held.camera.state.params.is_empty(), "no shot staged");
    assert!(!field_occlusion::fade_armed(&held, false));
    assert_eq!(
        field_occlusion::host_fade_arming(&held, false, ON),
        FadeArming::ScriptHeld
    );
    // The host's cutscene arm alone is a script hold too.
    let free = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    assert_eq!(
        field_occlusion::host_fade_arming(&free, true, ON),
        FadeArming::ScriptHeld
    );
    assert_eq!(
        field_occlusion::host_fade_arming(&free, false, ON),
        FadeArming::Armed
    );
    // A host term that is not a shot still resets.
    let off = field_occlusion::FadeHostTerms {
        enabled: false,
        ..ON
    };
    assert_eq!(
        field_occlusion::host_fade_arming(&held, false, off),
        FadeArming::Off
    );
}

/// A timeline that has not taken a slice holds nothing yet, as retail's
/// camera ease reads the engaged bit ahead of the spawn: a stand-and-press
/// walk-on beat that re-spawns and ends inside one slice on every frame of
/// its band keeps the fade working while the player walks it.
#[test]
fn an_unstepped_timeline_keeps_the_fade() {
    let w = field_with_timeline(false);
    assert!(w.cutscene_timeline_active());
    assert!(!w.script_holds_camera());
    assert!(field_occlusion::fade_armed(&w, false));
}

/// A shot that opens mid-fade eases the screen-door out over the fade's own
/// release (no pop), and one that opens at rest never stages anything.
#[test]
fn a_shot_opening_mid_fade_eases_out() {
    use legaia_engine_scene::field_occlusion::FadeRamp;
    let mut ramp = FadeRamp::default();
    for _ in 0..40 {
        ramp.step(true);
    }
    assert_eq!(ramp.strength(), 1.0);
    // The hosts step the ramp on `false` while `ScriptHeld`.
    let mut prev = ramp.strength();
    let mut frames = 0;
    while let Some(s) = ramp.step(false) {
        assert!(
            s < prev && prev - s <= 0.25 + 1e-6,
            "eases, no pop: {prev} -> {s}"
        );
        prev = s;
        frames += 1;
    }
    assert!(frames > 4, "the release takes several frames: {frames}");
    assert_eq!(ramp.strength(), 0.0);
    let mut rest = FadeRamp::default();
    assert!(rest.step(false).is_none(), "a fade at rest stages nothing");
}
