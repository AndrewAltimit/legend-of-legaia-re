//! Per-actor pitch / roll reaches BOTH hosts' NPC draw, from one world field.
//!
//! The scripted-motion VM's ops `0x15` / `0x16` tween an actor's X and Z Euler
//! angles (`actor+0x24` / `actor+0x28`), and retail's per-actor render
//! dispatcher composes all three angles together: `FUN_8001ADA4` hands
//! `actor+0x24` whole to the three-angle composer (`addiu a0,s0,0x24` /
//! `jal 0x80026988` at `0x8001af04`), which reads X at `+0`, Y at `+2` and Z
//! at `+4`. Both hosts used to draw a field NPC with a single `Ry(heading)`,
//! so neither showed a tilt.
//!
//! What this pins:
//!
//! - the tween reaches `World` and both hosts' accessor
//!   ([`legaia_engine_core::world::World::field_npc_tilt`], surfaced to the
//!   page as `play_npc_tilts`) on the one retail scene that authors it -
//!   `juui1`, whose 45 `0x16` sites are the whole disc-wide census for the
//!   class;
//! - op `0x15` stays silent there, which is what the census says (zero
//!   authored sites disc-wide) - so a non-zero pitch here would mean the
//!   decode had drifted, not that the scene tilts;
//! - a scene that authors neither (`town01`) reports an all-zero array, so
//!   each host's cheap yaw-only model build stays the common path;
//! - the two hosts' model builds agree on the composition. The native NPC
//!   pass calls `legaia_engine_render::battle_intro::placement_rotation`,
//!   which is a re-export of the `legaia_engine_ui` builder called here; the
//!   page composes `placementModelEuler` (`site/js/webgl-math.js`), whose
//!   linear part is transcribed below. A divergence between the two is
//!   exactly the failure this file exists to catch, and no screenshot of one
//!   host can see it.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc
//! data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;

/// PSX 12-bit angle -> radians, the page's `A2R`.
const A2R: f32 = std::f32::consts::TAU / 4096.0;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// The linear part of `site/js/webgl-math.js`'s `placementModelEuler`,
/// transcribed. Row-major `R = Rx * Ry * Rz`, the same nine entries the page
/// writes (its model additionally folds `T` and the `diag(sc, -sc, sc)` flip,
/// which are not part of the rotation the two hosts have to agree on).
fn page_euler_rows(rot_x: f32, rot_y: f32, rot_z: f32) -> [[f32; 3]; 3] {
    let (cx, sx) = (rot_x.cos(), rot_x.sin());
    let (cy, sy) = (rot_y.cos(), rot_y.sin());
    let (cz, sz) = (rot_z.cos(), rot_z.sin());
    [
        [cy * cz, -cy * sz, sy],
        [sx * sy * cz + cx * sz, -sx * sy * sz + cx * cz, -sx * cy],
        [-cx * sy * cz + sx * sz, cx * sy * sz + sx * cz, cx * cy],
    ]
}

/// Advance until some catalogued actor reports a tilt, up to `frames`.
fn tick_until_tilt(rt: &mut LegaiaRuntime, frames: u32) -> Option<(u32, Vec<f32>)> {
    for f in 0..frames {
        let _ = rt.tick_frame();
        let t = rt.play_npc_tilts();
        if t.iter().any(|v| *v != 0.0) {
            return Some((f, t));
        }
    }
    None
}

#[test]
fn juui1_actor_tilt_reaches_the_page_and_stays_roll_only() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("juui1").expect("enter juui1");

    let n = rt.play_npc_transforms().len() / 4;
    assert!(n > 0, "juui1 catalogs actors");
    assert_eq!(
        rt.play_npc_tilts().len(),
        n * 2,
        "one (pitch, roll) pair per catalog entry, parallel to the transforms"
    );

    let (frame, tilts) =
        tick_until_tilt(&mut rt, 600).expect("juui1 authors op 0x16, so some actor must tilt");
    eprintln!("[ok] juui1: first tilt at sim frame {frame}, pairs = {tilts:?}");

    let tilted: Vec<(usize, f32, f32)> = tilts
        .chunks(2)
        .enumerate()
        .filter(|(_, p)| p[0] != 0.0 || p[1] != 0.0)
        .map(|(i, p)| (i, p[0], p[1]))
        .collect();
    assert!(!tilted.is_empty());
    for (i, pitch, _) in &tilted {
        assert_eq!(
            *pitch, 0.0,
            "actor {i}: op 0x15 is authored at zero sites disc-wide, so pitch must stay 0"
        );
    }
    assert!(
        tilted.iter().any(|(_, _, roll)| *roll != 0.0),
        "the tilt must be the Z angle - op 0x16's destination"
    );
}

#[test]
fn an_untilted_scene_reports_an_all_zero_array() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..600 {
        let _ = rt.tick_frame();
    }
    let t = rt.play_npc_tilts();
    assert!(!t.is_empty(), "town01 catalogs actors");
    assert!(
        t.iter().all(|v| *v == 0.0),
        "town01 authors neither 0x15 nor 0x16, so every host keeps its yaw-only build: {t:?}"
    );
    eprintln!("[ok] town01: {} pairs, all zero", t.len() / 2);
}

#[test]
fn both_hosts_compose_the_same_rotation_for_juui1s_live_tilt() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("juui1").expect("enter juui1");
    let (_, tilts) = tick_until_tilt(&mut rt, 600).expect("juui1 tilts");
    let nt = rt.play_npc_transforms();

    let mut checked = 0usize;
    for (i, p) in tilts.chunks(2).enumerate() {
        if p[0] == 0.0 && p[1] == 0.0 {
            continue;
        }
        let (pitch, roll) = (p[0], p[1]);
        let yaw_units = nt[i * 4 + 3] + 2048.0;

        // Native: the shared kernel, in 12-bit units.
        let u = |v: f32| (v as i32).rem_euclid(4096) as u16;
        let native =
            legaia_engine_ui::battle_intro::placement_rotation(u(pitch), u(yaw_units), u(roll));
        // Page: `placementModelEuler`, in radians.
        let page = page_euler_rows(pitch * A2R, yaw_units * A2R, roll * A2R);

        for (r, row) in page.iter().enumerate() {
            for (c, want) in row.iter().enumerate() {
                // `placement_rotation` is a `Mat4`; glam is column-major, so
                // entry (row r, col c) is `col(c)[r]`.
                let got = native.col(c)[r];
                assert!(
                    (got - want).abs() < 1e-4,
                    "actor {i} rotation[{r}][{c}]: native {got}, page {want}"
                );
            }
        }

        // And the tilt is not a no-op: a yaw-only matrix would differ.
        let yaw_only = legaia_engine_ui::battle_intro::placement_rotation(0, u(yaw_units), 0);
        let differs = (0..3).any(|c| (native.col(c) - yaw_only.col(c)).length() > 1e-4);
        assert!(
            differs,
            "actor {i}: composing the tilt must change the draw"
        );
        checked += 1;
    }
    assert!(checked > 0, "at least one tilted actor to compare");
    eprintln!("[ok] {checked} tilted actors: native and page compositions agree");
}
