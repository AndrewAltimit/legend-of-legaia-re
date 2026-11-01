use super::*;

fn parse(text: &str) -> MapDef {
    toml::from_str(text).expect("map parses")
}

const MOAT: &str = r##########"
name = "Moat"
floor_heights = [0, 56, 112]
base = { scene = { kind = "scene", name = "rugi" } }

[tiles]
rows = [
  "########",
  "#..LL..#",
  "#..LL..#",
  "#^^ll^^#",
  "########",
]
[tiles.legend]
"#" = { walls = "all" }
"." = {}
"^" = { height = 2 }
"L" = { walls = "all" }
"l" = { height = 1, walls = "all" }

[[ground_motion]]
kind = "swell"
chars = "Ll"
amplitude = 10
period = 120
phases = 4
wavelength = 4.0
angle = 90.0
"##########;

#[test]
fn the_wave_is_a_parabola_either_side_of_rest() {
    // Rest at the start, the quarter and the end of each half.
    assert_eq!(parabolic_wave(0, 120, 10), 0);
    assert_eq!(parabolic_wave(30, 120, 10), 10);
    assert_eq!(parabolic_wave(60, 120, 10), 0);
    assert_eq!(parabolic_wave(90, 120, 10), -10);
    assert_eq!(parabolic_wave(120, 120, 10), 0);
    // A parabola, not a triangle: half way up the first quarter is 3/4 of
    // the swing.
    assert_eq!(parabolic_wave(15, 120, 16), 12);
    // Negative time wraps like the frame counter would.
    assert_eq!(parabolic_wave(-30, 120, 10), -10);
    // Never past the amplitude, on any frame of an odd period.
    for t in 0..300 {
        assert!(parabolic_wave(t, 77, 9).abs() <= 9);
    }
}

#[test]
fn the_wave_meets_rest_with_equal_slope() {
    // Either side of the zero crossing the steps match: no kink where the
    // arc above hands over to the arc below.
    let up = parabolic_wave(59, 120, 60) - parabolic_wave(60, 120, 60);
    let down = parabolic_wave(60, 120, 60) - parabolic_wave(61, 120, 60);
    assert_eq!(up, down);
}

#[test]
fn each_tier_a_motion_stands_on_gets_one_free_rung_per_step() {
    let m = parse(MOAT);
    let plan = m.ground_motion_plan();
    // `L` stands on tier 0 and `l` on tier 1; tiers 0..=2 are the map's.
    assert_eq!(plan.short, 0);
    assert_eq!(plan.rungs.len(), 8);
    let rungs: Vec<u8> = plan.rungs.iter().map(|r| r.rung).collect();
    assert_eq!(rungs, vec![15, 14, 13, 12, 11, 10, 9, 8]);
    assert!(plan.rungs[..4].iter().all(|r| r.base_tier == 0));
    assert!(plan.rungs[4..].iter().all(|r| r.base_tier == 1));
    // The ladder holds each rung at its tier's rest height.
    let ladder = m.floor_ladder().unwrap();
    assert_eq!(ladder[..3], [0, 56, 112]);
    assert_eq!(ladder[15], 0);
    assert_eq!(ladder[8], 56);
}

#[test]
fn a_swell_steps_its_phase_along_its_direction() {
    let m = parse(MOAT);
    let plan = m.ground_motion_plan();
    // Travelling `+X` with a four-tile wavelength in four steps: each
    // column is one step on from the last, and rows do not matter.
    let at = |c, r, ch, tier| plan.rung_at(&m, c, r, ch, tier).unwrap();
    assert_eq!(at(3, 1, 'L', 0), 12);
    assert_eq!(at(4, 1, 'L', 0), 15);
    assert_eq!(at(4, 2, 'L', 0), 15);
    assert_eq!(at(3, 3, 'l', 1), 8);
    // A tile no motion names rides nothing.
    assert_eq!(plan.rung_at(&m, 1, 1, '.', 0), None);
}

#[test]
fn neighbouring_steps_lag_by_a_share_of_the_period() {
    let m = parse(MOAT);
    let plan = m.ground_motion_plan();
    let def = &m.ground_motion[0];
    let ladder = m.floor_ladder().unwrap();
    // Step 1 is a quarter period behind step 0: it peaks 30 frames later.
    let s0 = plan.rungs[0];
    let s1 = plan.rungs[1];
    assert_eq!(s0.offset(def, 30), 10);
    assert_eq!(s1.offset(def, 60), 10);
    // Heights are the rest plus the swing, up positive.
    assert_eq!(plan.rungs[4].height(&m, &ladder, 30), 66);
}

#[test]
fn a_breathe_takes_one_rung_and_every_tile_rides_it() {
    let mut m = parse(MOAT);
    m.ground_motion[0].kind = GroundMotionKind::Breathe;
    let plan = m.ground_motion_plan();
    assert_eq!(plan.rungs.len(), 2);
    assert_eq!(
        plan.rung_at(&m, 3, 1, 'L', 0),
        plan.rung_at(&m, 4, 2, 'L', 0)
    );
}

#[test]
fn a_ripple_centres_on_its_tiles() {
    let mut m = parse(MOAT);
    m.ground_motion[0].kind = GroundMotionKind::Ripple;
    let plan = m.ground_motion_plan();
    // The `L` / `l` block spans columns 3..=4, rows 1..=3.
    assert_eq!(plan.centres[0], [4.0, 2.5]);
}

#[test]
fn a_ladder_too_short_drops_whole_tiers_not_steps() {
    let mut m = parse(MOAT);
    // Fourteen tiers of the map's own leave two free rungs: not enough for
    // either tier's four steps.
    m.floor_heights = vec![0; 14];
    let plan = m.ground_motion_plan();
    assert!(plan.rungs.is_empty());
    assert_eq!(plan.short, 6);
}

#[test]
fn a_terrain_map_or_one_without_its_own_ladder_moves_nothing() {
    let mut m = parse(MOAT);
    m.floor_heights.clear();
    assert!(m.ground_motion_plan().rungs.is_empty());
    assert_eq!(m.floor_ladder(), None);
}

#[test]
fn a_map_with_no_motion_keeps_its_ladder() {
    let mut m = parse(MOAT);
    m.ground_motion.clear();
    let mut want = [0i16; FLOOR_TIERS];
    want[..3].copy_from_slice(&[0, 56, 112]);
    assert_eq!(m.floor_ladder(), Some(want));
}

#[test]
fn a_step_on_a_boundary_is_stable() {
    // `cos(90 deg)` is a hair off zero; a corner exactly on a step boundary
    // must not flip with it.
    let d = GroundMotionDef {
        kind: GroundMotionKind::Swell,
        angle: 90.0,
        phases: 4,
        wavelength: 4.0,
        ..Default::default()
    };
    for row in 0..120 {
        assert_eq!(d.step_at(4, row, [0.0; 2]), 0);
        assert_eq!(d.step_at(5, row, [0.0; 2]), 1);
    }
}

fn issues(map: &str) -> Vec<crate::validate::Issue> {
    let mut files = crate::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"g\"\nname = \"G\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert("maps/moat.toml".into(), map.as_bytes().to_vec());
    crate::validate::validate_files(&files, None)
        .into_iter()
        .filter(|i| i.field.starts_with("ground_motion"))
        .collect()
}

#[test]
fn the_validator_passes_a_good_motion_and_names_each_bad_knob() {
    assert!(issues(MOAT).is_empty(), "{:?}", issues(MOAT));
    let bad = MOAT
        .replace("chars = \"Ll\"", "chars = \"Lz\"")
        .replace("amplitude = 10", "amplitude = 90")
        .replace("period = 120", "period = 1")
        .replace("phases = 4", "phases = 9")
        .replace("wavelength = 4.0", "wavelength = 0.0");
    let found = issues(&bad);
    for field in ["chars", "amplitude", "period", "phases", "wavelength"] {
        assert!(
            found
                .iter()
                .any(|i| i.field == format!("ground_motion[0].{field}")
                    && i.severity == crate::validate::Severity::Error),
            "{field}: {found:?}"
        );
    }
}

#[test]
fn the_validator_refuses_a_full_ladder_and_a_borrowed_one() {
    let full = MOAT.replace(
        "floor_heights = [0, 56, 112]",
        "floor_heights = [0, 56, 112, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]",
    );
    assert!(
        issues(&full)
            .iter()
            .any(|i| i.message.contains("ladder rungs"))
    );
    let borrowed = MOAT.replace("floor_heights = [0, 56, 112]\n", "");
    assert!(
        issues(&borrowed)
            .iter()
            .any(|i| i.message.contains("floor_heights"))
    );
}

#[test]
fn the_validator_warns_of_a_walkable_floor_swinging_far() {
    let walk = MOAT
        .replace("chars = \"Ll\"", "chars = \".\"")
        .replace("amplitude = 10", "amplitude = 30");
    let found = issues(&walk);
    assert!(
        found
            .iter()
            .any(|i| i.severity == crate::validate::Severity::Warning
                && i.field == "ground_motion[0].amplitude"),
        "{found:?}"
    );
}
