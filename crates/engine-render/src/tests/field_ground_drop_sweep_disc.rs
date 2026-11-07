//! Disc-gated: the field ground off retail's vantage keeps every walkable
//! cell in front of the ground behind it, at every camera yaw.
//!
//! `town01`'s plateau cell `(32, 23)` is a far-bucket cell with one corner
//! 32 units lower than the other three. Under the free-vantage drop
//! (`Renderer::set_field_ground_drop`) the flat far-bucket cells left the
//! far slice for the dropped floor while this sloped one stayed in it, so
//! from a camera swung toward the sea the beach 192 units below - its flat
//! cell `(35, 17)` and the rock pile on it - painted over the plateau: a
//! pale quad with a dark hole in the ground beside the player.
//!
//! This sweeps the follow camera's yaw around the player standing at
//! `(30, 22)` on that plateau, at the low tilt the report was taken from and
//! at a steeper one, runs every ground vertex near the spot through the
//! native vertex stage (`vs_main`, through `shader_eval`) with the drop on,
//! rasterises the cells on a coarse pixel grid with the depth the stage
//! wrote, and asserts that wherever a gently sloped ground cell is the
//! nearest surface along the pixel's ray, the depth test keeps ground at
//! that depth: no cell lying farther along the ray wins the pixel. The page
//! twin must keep no ground vertex in its fragment-stage far slice under
//! the drop (`v_far_bucket`).
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset or `extracted/` is
//! missing.

use std::path::{Path, PathBuf};

use super::page_glsl::{parse_glsl, parse_wgsl};
use super::shader_eval::Machine;
use super::vertex_law_parity::{Frame, Vertex, no_fetch, page_vertex_glsl, run_native, run_page};
use crate::shaders::{VRAM_MESH_SHADER_SRC, compose_scene_lit_shader};
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_engine_scene::field_ground;
use legaia_engine_scene::scene::{ProtIndex, Scene};

/// The report's spot: the player on the plateau at tile `(30, 22)`, the
/// follow camera's pose there under the default `Far` preset and the
/// user's tilt toward the horizon (pitch `0.19`), yaw swept.
const FOCUS: [f32; 3] = [3904.0, -256.0, 2880.0];
const TR_EYE: [f32; 3] = [-14.5, 43.275_177, 2_361.264_2];
const H: f32 = 512.0;
/// The tilted-down knob the report shows, and a steeper follow pitch.
const PITCHES: [f32; 2] = [0.190_291_35, 0.45];
const YAW_STEPS: usize = 24;
/// The cells near the spot that go through the vertex stage.
const COLS: std::ops::RangeInclusive<i32> = 18..=42;
const ROWS: std::ops::RangeInclusive<i32> = 10..=34;
/// A coarse pixel grid over the frame.
const GRID: (usize, usize) = (96, 72);

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let mut roots: Vec<PathBuf> = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    roots.extend(["extracted", "../extracted", "../../extracted"].map(PathBuf::from));
    roots
        .into_iter()
        .find(|p| Path::new(p).join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists())
}

/// The field cameras' single Y negation (`FIELD_WORLD_FLIP`): the ground's
/// retail Y-down positions ride it into the Y-up render frame.
fn field_mvp(view: &FieldCameraView) -> [f32; 16] {
    let mut m = view.vp(4.0 / 3.0);
    for v in &mut m[4..8] {
        *v = -*v;
    }
    m
}

/// One ground vertex through a vertex stage: screen `(x, y)` in NDC, its
/// true view depth `w`, and the depth the stage wrote (as a view `w`).
#[derive(Clone, Copy)]
struct Staged {
    ndc: [f32; 2],
    w: f32,
    depth_w: f32,
}

#[test]
fn town01_plateau_ground_holds_against_the_beach_at_every_yaw() {
    let Some(root) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open extracted");
    let scene = Scene::load(&index, "town01").expect("town01");
    let hf = scene
        .walk_heightfield(&index)
        .expect("heightfield")
        .expect("town01 has a ground grid");
    let positions = field_ground::render_positions(&hf);
    let refs = field_ground::flat_refs(&hf, &positions);

    // The cells near the spot, four vertices each (the builder's layout).
    let cells: Vec<usize> = positions
        .chunks(4)
        .enumerate()
        .filter(|(_, c)| {
            let (col, row) = ((c[0][0] / 128.0) as i32, (c[0][2] / 128.0) as i32);
            COLS.contains(&col) && ROWS.contains(&row)
        })
        .map(|(q, _)| q)
        .collect();
    assert!(cells.len() > 200, "{} cells near the spot", cells.len());
    // The report's two cells are among them, and the plateau one is the
    // sloped far-bucket cell.
    let cell_at = |col: i32, row: i32| {
        cells.iter().copied().find(|&q| {
            positions[q * 4][0] == (col * 128) as f32 && positions[q * 4][2] == (row * 128) as f32
        })
    };
    let plateau = cell_at(32, 23).expect("cell (32, 23)");
    assert!(field_ground::is_far_bucket_ref(&refs[plateau * 4]));
    cell_at(35, 17).expect("the beach cell (35, 17)");

    let native = parse_wgsl(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC));
    let page = parse_glsl(&page_vertex_glsl(), naga::ShaderStage::Vertex);
    let mut mn = Machine::new(&native, no_fetch());
    let mut mp = Machine::new(&page, no_fetch());
    let drop = field_ground::FREE_VANTAGE_FLOOR_DROP;

    let (mut checked, mut lost) = (0usize, Vec::new());
    for pitch in PITCHES {
        for step in 0..YAW_STEPS {
            let yaw = step as f32 / YAW_STEPS as f32 * std::f32::consts::TAU;
            let view = FieldCameraView {
                focus: FOCUS,
                pitch,
                yaw,
                roll: 0.0,
                h: H,
                tr_eye: TR_EYE,
            };
            let frame = Frame {
                mvp: field_mvp(&view),
                curve: 0.0,
                snap: false,
                prim_near: [0.0; 4],
                ground_drop: drop,
            };
            let mut staged = vec![None; positions.len()];
            for &q in &cells {
                for i in q * 4..q * 4 + 4 {
                    let r = refs[i];
                    let v = Vertex {
                        pos: positions[i],
                        colour: [128; 3],
                        fa: [r[0], r[1], r[2], r[3]],
                        fb: [r[4], r[5], r[6], r[7]],
                        corners: [[0.0; 4]; 4],
                    };
                    let (clip, _, _, depth_w) = run_native(&mut mn, &frame, &v);
                    run_page(&mut mp, &frame, &v);
                    assert_eq!(
                        mp.global("v_far_bucket").floats()[0],
                        0.0,
                        "page: a ground vertex kept the far slice under the drop"
                    );
                    if clip[2] > 50.0 {
                        staged[i] = Some(Staged {
                            ndc: [clip[0] / clip[2], clip[1] / clip[2]],
                            w: clip[2],
                            depth_w,
                        });
                    }
                }
            }
            // Per pixel: the nearest cell along the ray (true depth) and the
            // cell the depth test keeps (the stage's depth).
            let mut nearest = vec![(f32::MAX, usize::MAX); GRID.0 * GRID.1];
            let mut kept = vec![(f32::MAX, usize::MAX, f32::MAX); GRID.0 * GRID.1];
            for &q in &cells {
                for tri in hf.indices[q * 6..q * 6 + 6].chunks(3) {
                    let Some(t) = tri
                        .iter()
                        .map(|&i| staged[i as usize])
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    rasterise(&t, |px, l| {
                        let inv = |f: &dyn Fn(&Staged) -> f32| {
                            1.0 / (l[0] / f(&t[0]) + l[1] / f(&t[1]) + l[2] / f(&t[2]))
                        };
                        let (w, d) = (inv(&|s| s.w), inv(&|s| s.depth_w));
                        if w < nearest[px].0 {
                            nearest[px] = (w, q);
                        }
                        if d < kept[px].0 {
                            kept[px] = (d, q, w);
                        }
                    });
                }
            }
            for px in 0..nearest.len() {
                let (w, q) = nearest[px];
                if q == usize::MAX {
                    continue;
                }
                let ys = &refs[q * 4][4..8];
                let rise = ys.iter().copied().fold(f32::MIN, f32::max)
                    - ys.iter().copied().fold(f32::MAX, f32::min);
                if rise > 64.0 {
                    continue;
                }
                checked += 1;
                let (_, k, w_k) = kept[px];
                // Another cell won the pixel: it has to lie on the ray
                // within the drop band of the nearest one, not behind it.
                if k != q && w_k > w * 1.05 + 16.0 {
                    let cell = |q: usize| {
                        (
                            (positions[q * 4][0] / 128.0) as i32,
                            (positions[q * 4][2] / 128.0) as i32,
                        )
                    };
                    lost.push((pitch, step, cell(q), cell(k), w, w_k));
                }
            }
        }
    }
    assert!(checked > 10_000, "{checked} ground pixels checked");
    assert!(
        lost.is_empty(),
        "{} ground pixels lost to ground behind them, e.g. {:?}",
        lost.len(),
        &lost[..lost.len().min(8)]
    );
}

/// Visit every grid pixel centre inside the screen triangle `t`, with its
/// screen-space barycentric weights.
fn rasterise(t: &[Staged], mut f: impl FnMut(usize, [f32; 3])) {
    let (gw, gh) = GRID;
    let to_px = |s: &Staged| {
        [
            (s.ndc[0] * 0.5 + 0.5) * gw as f32,
            (0.5 - s.ndc[1] * 0.5) * gh as f32,
        ]
    };
    let [a, b, c] = [to_px(&t[0]), to_px(&t[1]), to_px(&t[2])];
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if area.abs() < 1e-6 {
        return;
    }
    let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
    let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(gw);
    let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
    let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(gh);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let l1 = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / area;
            let l2 = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / area;
            let l0 = 1.0 - l1 - l2;
            if l0 >= 0.0 && l1 >= 0.0 && l2 >= 0.0 {
                f(y * gw + x, [l0, l1, l2]);
            }
        }
    }
}
