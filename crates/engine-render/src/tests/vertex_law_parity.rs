//! Vertex-stage parity: the native VRAM-mesh `vs_main` against the play
//! page's field-mesh vertex shader (`VS_SRC` in `site/js/webgl-shaders.js`),
//! both evaluated through `shader_eval`.
//!
//! One grid of vertices goes through both: the overworld screen-Y curvature
//! (`overworld_curve_clip` / `overworldCurve`) at the walk camera's scale and
//! a scripted shot's, the continent's flat bucket depth and ground depth cue
//! (`overworld_flat_depth` + `overworld_ground_cue` / their GLSL twins), the
//! PSX vertex snap, and retail's per-primitive near reject with and without
//! the GPU span limit. The clip position's `x`, `y` and `w`, the packet
//! colour the cue leaves and the reject decision must agree.
//!
//! Out of the comparison: the clip `z`. The hosts write depth differently by
//! design (reversed-Z on wgpu, a log-of-`w` depth on the page, the field far
//! bucket moved in the page's fragment stage), so the bucket rules that only
//! move `z` are not paired here. The page's curvature bias
//! (`u_curve_bias`) is held at 0: native folds the same constant into its
//! clip matrix instead.

use super::page_glsl::{es300_to_450, js_consts, parse_glsl, parse_wgsl, site_js, template};
use super::shader_eval::{Machine, Val};
use crate::shaders::{COLOR_MESH_SHADER_SRC, VRAM_MESH_SHADER_SRC, compose_scene_lit_shader};

const STAGE_W: f32 = 320.0;
const STAGE_H: f32 = 240.0;

fn page_vertex_glsl() -> String {
    let consts = js_consts(&site_js("webgl-shaders.js"));
    es300_to_450(&template(&consts, "VS_SRC"))
}

fn no_fetch() -> Box<super::shader_eval::ImageFetch> {
    Box::new(|name, _, _| panic!("vertex stage fetched from {name}"))
}

/// A column-major 4x4 as the evaluator's matrix value.
fn mat(m: &[f32; 16]) -> Val {
    Val::Arr((0..4).map(|c| Val::f(&m[c * 4..c * 4 + 4])).collect())
}

/// One frame's vertex-stage state, in the engine's terms.
#[derive(Clone, Copy, Debug)]
struct Frame {
    mvp: [f32; 16],
    /// The overworld's `clip.w`-to-`SZ` factor (0 = off the overworld).
    curve: f32,
    snap: bool,
    /// `(enable, sz_per_w, ot_shift, near_otz)` of the near reject.
    prim_near: [f32; 4],
}

/// One vertex of one primitive.
#[derive(Clone, Copy, Debug)]
struct Vertex {
    pos: [f32; 3],
    colour: [u8; 3],
    /// The continent cell `[x0, z0, x1, z1]` / `[y00, y10, y01, y11]`, zero
    /// off the continent.
    fa: [f32; 4],
    fb: [f32; 4],
    /// The primitive's corners (`c0.w` = corner count, 0 = no owner).
    corners: [[f32; 4]; 4],
}

/// `(clip x, y, w, rejected)`, and the packet colour in bytes.
type VOut = ([f32; 3], bool, [f32; 3]);

fn rejected(clip: &[f32]) -> bool {
    clip == [2.0, 2.0, 2.0, 1.0]
}

fn run_native(m: &mut Machine<'_>, f: &Frame, v: &Vertex) -> VOut {
    m.set_uniform("mvp", mat(&f.mvp));
    m.set_uniform("flags", Val::f(&[0.0, 1.0, 0.0, f.curve]));
    m.set_uniform(
        "psx_params",
        Val::f(&[STAGE_W, STAGE_H, if f.snap { 1.0 } else { 0.0 }, 0.0]),
    );
    m.set_uniform("prim_near", Val::f(&f.prim_near));
    let main = m.function("vs_main");
    let c = v.corners;
    let args: Vec<Val> = main
        .arguments
        .iter()
        .map(|a| match a.name.as_deref().unwrap_or_default() {
            "position" => Val::f(&v.pos),
            "uv_in" => Val::u(&[3, 7, 0, 0]),
            "cba_tsb_in" => Val::u(&[0, 0x80]),
            "normal_in" => Val::f(&[0.0; 3]),
            "color_in" => Val::u(&[
                u32::from(v.colour[0]),
                u32::from(v.colour[1]),
                u32::from(v.colour[2]),
                255,
            ]),
            "flat_a" => Val::f(&v.fa),
            "flat_b" => Val::f(&v.fb),
            "prim_c0" => Val::f(&c[0]),
            "prim_c1" => Val::f(&c[1][..3]),
            "prim_c2" => Val::f(&c[2][..3]),
            "prim_c3" => Val::f(&c[3][..3]),
            // The colour mesh's inputs: the packet colour normalised, and its
            // blend word (no flags).
            "color" => Val::f(&[
                f32::from(v.colour[0]) / 255.0,
                f32::from(v.colour[1]) / 255.0,
                f32::from(v.colour[2]) / 255.0,
                1.0,
            ]),
            "blend" => Val::u(&[0]),
            other => panic!("unstaged vertex input {other}"),
        })
        .collect();
    let out = m.call(main, args).expect("no discard").expect("VsOut");
    let Val::Struct(fields) = out else {
        panic!("VsOut")
    };
    let names = m.struct_member_names(main.result.as_ref().unwrap().ty);
    let field = |n: &str| fields[names.iter().position(|x| x == n).unwrap()].clone();
    let clip = field("clip_pos");
    let clip = clip.floats();
    // The VRAM mesh carries the packet colour in bytes, the colour mesh
    // normalised.
    let col = match names.iter().any(|x| x == "prim_color") {
        true => field("prim_color").floats().to_vec(),
        false => field("color").floats().iter().map(|c| c * 255.0).collect(),
    };
    (
        [clip[0], clip[1], clip[3]],
        rejected(clip),
        [col[0], col[1], col[2]],
    )
}

fn run_page(m: &mut Machine<'_>, f: &Frame, v: &Vertex) -> VOut {
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    m.set_uniform("u_mvp", mat(&f.mvp));
    m.set_uniform("u_model", mat(&IDENTITY));
    m.set_uniform("u_curve", Val::f(&[f.curve]));
    m.set_uniform("u_curve_bias", Val::f(&[0.0]));
    m.set_uniform(
        "u_psx",
        Val::f(&[STAGE_W, STAGE_H, if f.snap { 1.0 } else { 0.0 }, 0.0]),
    );
    m.set_uniform("u_prim_near", Val::f(&f.prim_near));
    m.set_uniform("u_fog_enable", Val::I(vec![0]));
    m.set_global("a_position", Val::f(&v.pos));
    m.set_global("a_uv_byte", Val::f(&[3.0, 7.0]));
    m.set_global("a_cba_tsb", Val::u(&[0, 0x80]));
    m.set_global(
        "a_flat_rgba",
        Val::f(&[
            f32::from(v.colour[0]) / 255.0,
            f32::from(v.colour[1]) / 255.0,
            f32::from(v.colour[2]) / 255.0,
            1.0,
        ]),
    );
    m.set_global("a_ground_ref_xz", Val::f(&v.fa));
    m.set_global("a_ground_ref_y", Val::f(&v.fb));
    m.set_global("a_normal", Val::f(&[0.0; 3]));
    let c = v.corners;
    m.set_global("a_prim_c0", Val::f(&c[0]));
    m.set_global("a_prim_c1", Val::f(&c[1][..3]));
    m.set_global("a_prim_c2", Val::f(&c[2][..3]));
    m.set_global("a_prim_c3", Val::f(&c[3][..3]));
    let main = m.function("main");
    m.call(main, Vec::new()).expect("no discard");
    let clip = m.global("gl_Position").floats().to_vec();
    let w = m.global("v_affine_w").floats()[0];
    let col = m.global("v_flat_rgba_pw").floats().to_vec();
    (
        [clip[0], clip[1], clip[3]],
        rejected(&clip),
        [col[0] / w * 255.0, col[1] / w * 255.0, col[2] / w * 255.0],
    )
}

/// A field-camera-shaped projection: the retail stage's 320x240 frame, H
/// near the field default, a view a few thousand units off, Y flipped as
/// both hosts' field frames are.
fn camera(yaw: f32, dist: f32) -> [f32; 16] {
    let (s, c) = yaw.sin_cos();
    let h = 352.0;
    let sx = 2.0 * h / STAGE_W;
    let sy = 2.0 * h / STAGE_H;
    // view = rotate about Y, then push `dist` down +Z; clip w = view z.
    [
        sx * c,
        0.0,
        0.0,
        -s,
        0.0,
        -sy,
        0.0,
        0.0,
        sx * s,
        0.0,
        0.0,
        c,
        0.0,
        40.0,
        1.0,
        dist,
    ]
}

fn frames() -> Vec<Frame> {
    let mut out = Vec::new();
    for mvp in [camera(0.0, 2400.0), camera(0.7, 6000.0)] {
        for curve in [0.0, 1.0, 6.0] {
            for snap in [false, true] {
                for prim_near in [[0.0; 4], [1.0, 1.0, 2.0, 120.0], [352.0, 1.0, 2.0, 120.0]] {
                    out.push(Frame {
                        mvp,
                        curve,
                        snap,
                        prim_near,
                    });
                }
            }
        }
    }
    out
}

fn vertices() -> Vec<Vertex> {
    let mut out = Vec::new();
    let mut s: u32 = 0x9E37_79B9;
    let mut rnd = |lo: f32, hi: f32| {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        lo + (hi - lo) * ((s >> 8) as f32 / (1u32 << 24) as f32)
    };
    for k in 0..48 {
        let pos = [
            rnd(-3000.0, 3000.0),
            rnd(-400.0, 100.0),
            rnd(-3000.0, 3000.0),
        ];
        let colour = [
            (k * 37 % 256) as u8,
            (k * 91 % 256) as u8,
            (k * 13 % 256) as u8,
        ];
        let (fa, fb) = if k % 3 == 0 {
            let x0 = pos[0] - 64.0;
            let z0 = pos[2] - 64.0;
            (
                [x0, z0, x0 + 128.0, z0 + 128.0],
                [pos[1], pos[1] - 10.0, pos[1] + 5.0, pos[1]],
            )
        } else {
            ([0.0; 4], [0.0; 4])
        };
        let n = [0.0, 3.0, 4.0][k % 3];
        let jitter = rnd(20.0, 900.0);
        let corners = [
            [pos[0], pos[1], pos[2], n],
            [pos[0] + jitter, pos[1], pos[2], 1.0],
            [pos[0], pos[1], pos[2] + jitter, 1.0],
            [pos[0] + jitter, pos[1], pos[2] + jitter, 1.0],
        ];
        out.push(Vertex {
            pos,
            colour,
            fa,
            fb,
            corners,
        });
    }
    out
}

fn compare(native_src: &str, verts: &[Vertex]) {
    let native = parse_wgsl(native_src);
    let page = parse_glsl(&page_vertex_glsl(), naga::ShaderStage::Vertex);
    let mut mn = Machine::new(&native, no_fetch());
    let mut mp = Machine::new(&page, no_fetch());
    let (mut n, mut diffs) = (0, Vec::new());
    for f in frames() {
        for v in verts {
            let a = run_native(&mut mn, &f, v);
            let b = run_page(&mut mp, &f, v);
            n += 1;
            let close = |x: &[f32; 3], y: &[f32; 3], tol: f32| {
                x.iter()
                    .zip(y)
                    .all(|(p, q)| (p - q).abs() <= tol * p.abs().max(1.0))
            };
            if a.1 != b.1 || (!a.1 && (!close(&a.0, &b.0, 1e-5) || !close(&a.2, &b.2, 1e-3))) {
                diffs.push(format!("{f:?}\n  {v:?}\n  native {a:?}\n  page   {b:?}"));
            }
        }
    }
    for d in diffs.iter().take(8) {
        eprintln!("{d}");
    }
    assert!(
        diffs.is_empty(),
        "{} of {n} vertices project differently on the two hosts",
        diffs.len()
    );
}

/// Textured field vertices: the VRAM-mesh pipeline.
#[test]
fn field_vertices_project_identically_on_both_hosts() {
    compare(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC), &vertices());
}

/// Untextured field vertices: the colour-mesh pipeline against the same
/// page program (it draws both halves of a hybrid mesh), off the
/// continent - the colour mesh carries no ground cell.
#[test]
fn untextured_vertices_project_identically_on_both_hosts() {
    let verts: Vec<Vertex> = vertices()
        .into_iter()
        .map(|v| Vertex {
            fa: [0.0; 4],
            fb: [0.0; 4],
            ..v
        })
        .collect();
    compare(&compose_scene_lit_shader(COLOR_MESH_SHADER_SRC), &verts);
}
