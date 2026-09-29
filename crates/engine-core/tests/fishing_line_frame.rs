//! The fishing **line** across a session: the rod actor the cast lock spawns
//! (`FUN_801D1C5C`), the packet the lure tick builds from the fish's projected
//! point and the rod tip, clipped by `FUN_801D56E4` - the frame every host
//! draws through `PondSession::line_frame`.
//!
//! The session half is disc-free: synthetic species / spawn / gesture tables
//! and a synthetic rod shaped like the venue's (a 42-vertex object whose
//! vertex 37 is the on-axis tip, one bend record on it). The last test lifts
//! the real rods and bend out of the `other1` bank and is disc-gated.

use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate, FishingSpecies, SPAWN_BANDS};
use legaia_engine_core::fishing::{
    CAST_POWER_MAX, FishingRecord, PondInput, PondPhase, PondSession, PondVenue, ROD_PAD_RIGHT,
};
use legaia_engine_core::fishing_actors::{
    LINE_CLIP_RECT, LINE_FISH_RGB, LINE_ROD_RGB, ROD_BEND_VDF_ENTRY, ROD_MODEL_BASE,
    ROD_TIP_VERTEX, RodMesh, RodSwing, VENUE_ANCHOR, clip_segment_2d,
};

const REEL_A: u32 = 0x40;
const HOLD: i32 = 6;

fn species(index: usize) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 40_000,
        pull_factor: 90,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 4096,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 100,
    }
}

fn templates() -> Vec<CadenceTemplate> {
    vec![CadenceTemplate {
        history_window: HOLD * 2,
        steps: vec![
            CadenceStep {
                duration: HOLD,
                button: 1,
            },
            CadenceStep {
                duration: HOLD,
                button: 0,
            },
        ],
    }]
}

fn rod_mesh() -> RodMesh {
    let mut rest = vec![0u8; 42 * 8];
    let o = ROD_TIP_VERTEX * 8;
    rest[o + 2..o + 4].copy_from_slice(&(-138i16).to_le_bytes());
    let mut bend = Vec::new();
    for w in [1u32, 0, ROD_TIP_VERTEX as u32, 1] {
        bend.extend_from_slice(&w.to_le_bytes());
    }
    for c in [0i16, 6, 45, 0] {
        bend.extend_from_slice(&c.to_le_bytes());
    }
    RodMesh {
        rods: [Some(rest.clone()), Some(rest.clone()), Some(rest)],
        bend: Some(bend),
    }
}

fn pond() -> PondSession {
    let mut p = PondSession::new(
        (0..10).map(species).collect(),
        vec![[3u32; SPAWN_BANDS]; 8],
        templates(),
        0,
        1,
        2,
        100,
        FishingRecord::default(),
        0,
        0x1234_5678,
    );
    let (anchor_x, anchor_z) = VENUE_ANCHOR;
    p.attach_venue(PondVenue {
        map: vec![0u8; 0x12000],
        region_block: None,
        anchor_x,
        anchor_z,
        facing: 0,
        rod_mesh: Some(rod_mesh()),
    });
    p
}

fn tick(p: &mut PondSession, input: PondInput) {
    p.tick(input, 1, 0x80);
}

fn cast(p: &mut PondSession) {
    let press = PondInput {
        cast_edge: true,
        ..Default::default()
    };
    tick(p, press);
    while p.phase() != PondPhase::Power {
        tick(p, PondInput::default());
    }
    while p.cast_power() < CAST_POWER_MAX {
        tick(p, PondInput::default());
    }
    tick(p, press);
    while p.phase() != PondPhase::Waiting {
        tick(p, PondInput::default());
    }
}

/// Play the reel gesture until a fish hooks, recasting when the empty line is
/// reeled in, drawing the line every frame the way a host does.
fn hook(p: &mut PondSession, fish: (i16, i16)) {
    for _ in 0..64 {
        cast(p);
        let mut f = 0;
        while p.phase() == PondPhase::Waiting && f < 4000 {
            let held = (f / HOLD) % 2 == 0;
            tick(
                p,
                PondInput {
                    reel_mask: if held { REEL_A } else { 0 },
                    cast_edge: false,
                    edge_bonus: i32::from(f % HOLD == 0),
                },
            );
            if p.phase() == PondPhase::Waiting {
                assert!(
                    p.line_frame(|_| Some(fish)).is_some(),
                    "no line while waiting"
                );
            }
            f += 1;
        }
        if p.phase() == PondPhase::Hooked {
            return;
        }
    }
    panic!("no strike");
}

#[test]
fn no_line_is_out_before_the_cast() {
    let mut p = pond();
    assert!(p.line_frame(|_| Some((100, 150))).is_none());
    assert!(p.rod_actor().is_none());
}

#[test]
fn the_line_runs_from_the_projected_fish_to_the_rod_tip_while_a_fish_is_hooked() {
    let mut p = pond();
    let fish = (100, 150);
    hook(&mut p, fish);
    let mut frames = 0;
    while p.phase() == PondPhase::Hooked && frames < 600 {
        tick(
            &mut p,
            PondInput {
                reel_mask: REEL_A,
                ..Default::default()
            },
        );
        if p.phase() != PondPhase::Hooked {
            break;
        }
        let lure = p.lure_actor().expect("a hooked fish rides the lure");
        // The host projects the lure's world point with its height zeroed.
        let line = p
            .line_frame(|w| {
                assert_eq!(w, [lure.x() as i32, 0, lure.z as i32]);
                Some(fish)
            })
            .expect("the line is out every hooked frame");
        let tip = p.rod_actor().and_then(|r| r.tip).expect("a projected tip");
        let (mut a, mut b) = (fish, tip.sxy);
        clip_segment_2d(&mut a, &mut b, LINE_CLIP_RECT);
        assert_eq!((line.fish, line.rod), (a, b));
        assert_eq!((line.fish_rgb, line.rod_rgb), (LINE_FISH_RGB, LINE_ROD_RGB));
        // A second draw in the same frame reuses the latched line.
        assert_eq!(p.line_frame(|_| panic!("projected twice")), Some(line));
        // The rod yaws toward the fish off the fish point it last saw.
        assert_eq!(
            p.rod_actor().unwrap().yaw,
            (tip.sxy.0 as i32 - fish.0 as i32) * 3
        );
        frames += 1;
    }
    assert!(frames > 0, "the fight resolved on its first frame");
    // A fish on keeps the rod bent at the hooked cap net of the actor's bleed.
    assert!(p.rod_actor().is_none_or(|r| r.bend > 0x1000));
}

#[test]
fn the_line_goes_and_the_rod_recovers_once_the_fight_resolves() {
    let mut p = pond();
    hook(&mut p, (100, 150));
    for _ in 0..20_000 {
        if matches!(p.phase(), PondPhase::Landed | PondPhase::Snapped) {
            break;
        }
        let t = p.tension();
        tick(
            &mut p,
            PondInput {
                reel_mask: if t < 0x800 { REEL_A } else { 0 },
                ..Default::default()
            },
        );
        let _ = p.line_frame(|_| Some((100, 150)));
    }
    assert!(matches!(p.phase(), PondPhase::Landed | PondPhase::Snapped));
    assert!(p.line_frame(|_| Some((100, 150))).is_none());
    assert_eq!(p.rod_actor().map(|r| r.swing), Some(RodSwing::Recover));
    for _ in 0..64 {
        tick(&mut p, PondInput::default());
    }
    assert!(p.rod_actor().is_none(), "the recover swing retires the rod");
}

#[test]
fn a_held_dpad_side_rolls_the_rod() {
    let mut p = pond();
    cast(&mut p);
    let right = PondInput::from_engine_pad(legaia_engine_core::input::PadButton::Right.mask(), 0);
    assert_ne!(right.reel_mask & ROD_PAD_RIGHT, 0);
    for _ in 0..32 {
        tick(&mut p, right);
        if p.phase() != PondPhase::Waiting {
            break;
        }
    }
    assert_eq!(p.rod_actor().unwrap().lean, 0x100);
}

#[test]
fn the_venue_rods_are_scene_models_0x19_to_0x1b_with_a_tip_the_bend_moves() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let scene = legaia_engine_core::scene::Scene::load(&host.index, "other1").expect("other1");
    let mesh = RodMesh::from_scene(&scene).expect("the venue carries its rods");
    assert_eq!(ROD_MODEL_BASE, 0x19);
    assert_eq!(ROD_BEND_VDF_ENTRY, 0);
    for r in 0..3 {
        let rest = mesh.rods[r].as_deref().expect("every rod resolves");
        assert!(rest.len() / 8 > ROD_TIP_VERTEX, "rod {r} is too short");
        let tip = mesh.tip(r, 0).unwrap();
        // The tip sits on the rod's own axis, at its far end.
        assert_eq!((tip[0], tip[2]), (0, 0), "rod {r} tip {tip:?}");
        let far = (0..rest.len() / 8)
            .map(|i| i16::from_le_bytes([rest[i * 8 + 2], rest[i * 8 + 3]]))
            .min()
            .unwrap();
        assert_eq!(
            tip[1], far,
            "rod {r}: vertex 37 is the farthest along the rod"
        );
        // The bend moves it.
        assert_ne!(mesh.tip(r, 0x1000).unwrap(), tip, "rod {r} does not bend");
    }
}
