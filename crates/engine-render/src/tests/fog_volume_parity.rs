//! Fog-volume parity: the native fog sheets' fragment stage
//! (`renderer::fog_volume::FOG_SHADER_SRC`) against the play page's GLSL
//! transcription (`FS` in `site/js/webgl-fog-volume.js`), both evaluated
//! through `shader_eval`.
//!
//! One grid of sheet fragments goes through both: the wisp noise and crest
//! profile across layers, the disturbance grid inside and outside its rect,
//! the mesh rim fade, the soft intersection against a scene surface in front
//! of, at and behind the sheet (native reverse-Z, the page's GL depth and its
//! log-of-`w` depth, each built from one view depth), the cliff-face fade off
//! a flat and a steep surface, the ground field's coverage and spill, and
//! the distant haze layer. Colour and alpha must agree to float rounding.
//!
//! The textures are analytic mocks (each host samples the same function of
//! its UV), so filtering is not what is compared - the shader arithmetic is.
//! The vertex stage is not paired here.

use super::page_glsl::{
    JsConst, es300_to_450, js_consts, parse_glsl, parse_wgsl, site_js, template,
};
use super::shader_eval::{Machine, Val};

const H: f32 = 240.0;

fn page_fog_fragment_glsl() -> String {
    let mut consts = js_consts(&site_js("webgl-shaders.js"));
    consts.extend(js_consts(&site_js("webgl-fog-volume.js")));
    // The page binds the fog pass's log-depth helper to the mesh program's
    // (`LOG_GLSL = LOG_DEPTH_GLSL` when webgl-shaders.js is loaded, which on
    // the play page it always is).
    let log = consts
        .get("LOG_DEPTH_GLSL")
        .cloned()
        .expect("LOG_DEPTH_GLSL");
    consts.insert("LOG_GLSL".into(), log);
    assert!(matches!(consts.get("FS"), Some(JsConst::Template(_))));
    es300_to_450(&template(&consts, "FS"))
}

/// The analytic textures both hosts sample.
fn tex(name: &str, uv: [f32; 2]) -> [f32; 4] {
    let [u, v] = uv;
    match name {
        // Disturbance density, 0..1.
        "dens" | "u_dens" => [0.5 + 0.5 * (u * 7.0 + v * 3.0).sin(), 0.0, 0.0, 1.0],
        // Ground field: coverage, spill, flow.
        "gtex" | "u_gtex" => [
            (1.2 - u).clamp(0.0, 1.0),
            ((v - 0.6) * 1.5).max(0.0),
            (u * 0.8 + v * 0.3).clamp(0.0, 1.0),
            1.0,
        ],
        other => panic!("fog sampled {other}"),
    }
}

/// One frame's fog state (the `FogVolumeDraw` fields the fragment stage
/// reads), shared by both hosts.
#[derive(Clone, Copy, Debug)]
struct FogFrame {
    sim: [f32; 4],
    mesh: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
    consts: [f32; 4],
    soft: f32,
    ground: [f32; 4],
    spill_phase: f32,
    spill: [f32; 4],
    haze: [f32; 4],
}

/// One sheet fragment, and the view depth of the scene surface behind it.
#[derive(Clone, Copy, Debug)]
struct FogFrag {
    px: (u32, u32),
    xz: [f32; 2],
    layer: f32,
    clip_w: f32,
    world: [f32; 3],
    floor_w: f32,
    haze: f32,
    view: [f32; 3],
    /// The scene surface's view depth at this pixel (`None` = cleared).
    scene_w: Option<f32>,
    /// `d world / d x`, `d world / d y` (top-down rows).
    dworld: ([f32; 3], [f32; 3]),
}

/// The page's depth encoding the frame was drawn with.
#[derive(Clone, Copy, Debug)]
enum PageDepth {
    Gl,
    Log,
}

const NEAR: f32 = 8.0;
const LOG_RANGE: f32 = 24.0;

type Out = [f32; 4];

fn run_native(m: &mut Machine<'_>, f: &FogFrame, g: &FogFrag) -> Out {
    // Reverse-Z with the near plane at NEAR: ndc = NEAR / w, so A = 0,
    // B = NEAR (`depth_mapping` of the reversed matrix).
    let (a, b) = (0.0, NEAR);
    m.set_uniform("sim", Val::f(&f.sim));
    m.set_uniform("mesh", Val::f(&f.mesh));
    m.set_uniform("color", Val::f(&f.color));
    m.set_uniform("params", Val::f(&f.params));
    m.set_uniform("consts", Val::f(&f.consts));
    m.set_uniform("depth", Val::f(&[a, b, f.soft, f.spill_phase]));
    m.set_uniform("ground", Val::f(&f.ground));
    m.set_uniform("spill", Val::f(&f.spill));
    m.set_uniform("haze", Val::f(&f.haze));
    let ds = g.scene_w.map_or(0.0, |w| a + b / w);
    m.fetch_f = Some(Box::new(move |name, _, _| {
        assert_eq!(name, "scene_depth");
        [ds, 0.0, 0.0, 0.0]
    }));
    m.gradients
        .insert("world".into(), (g.dworld.0.to_vec(), g.dworld.1.to_vec()));
    let main = m.function("fs_main");
    let ty = main.arguments[0].ty;
    let names = m.struct_member_names(ty);
    let arg = Val::Struct(
        names
            .iter()
            .map(|n| match n.as_str() {
                "pos" => Val::f(&[
                    g.px.0 as f32 + 0.5,
                    g.px.1 as f32 + 0.5,
                    0.5,
                    1.0 / g.clip_w,
                ]),
                "xz" => Val::f(&g.xz),
                "layer" => Val::f(&[g.layer]),
                "clip_w" => Val::f(&[g.clip_w]),
                "world" => Val::f(&g.world),
                "floor_w" => Val::f(&[g.floor_w]),
                "haze" => Val::f(&[g.haze]),
                "view" => Val::f(&g.view),
                other => panic!("unstaged fog input {other}"),
            })
            .collect(),
    );
    let out = m
        .call(main, vec![arg])
        .expect("no discard")
        .expect("colour");
    let o = out.floats();
    [o[0], o[1], o[2], o[3]]
}

fn run_page(m: &mut Machine<'_>, f: &FogFrame, g: &FogFrag, depth: PageDepth) -> Out {
    // GL: window depth = (ndc + 1) / 2 with ndc = A + B / w of a GL
    // perspective at the same near plane (infinite far): A = 1, B = -2 NEAR.
    let (a, b) = (1.0, -2.0 * NEAR);
    m.set_uniform("u_sim", Val::f(&f.sim));
    m.set_uniform("u_mesh", Val::f(&f.mesh));
    m.set_uniform("u_color", Val::f(&f.color));
    m.set_uniform("u_params", Val::f(&f.params));
    m.set_uniform("u_consts", Val::f(&f.consts));
    m.set_uniform("u_depth", Val::f(&[a, b, f.soft, 1.0]));
    m.set_uniform("u_ground", Val::f(&f.ground));
    m.set_uniform("u_spill_phase", Val::f(&[f.spill_phase]));
    m.set_uniform("u_spill", Val::f(&f.spill));
    m.set_uniform("u_haze", Val::f(&f.haze));
    let log_on = matches!(depth, PageDepth::Log);
    m.set_uniform(
        "u_log_depth",
        Val::f(&[if log_on { 1.0 } else { 0.0 }, LOG_RANGE]),
    );
    let raw = match (g.scene_w, depth) {
        (None, _) => 1.0,
        (Some(w), PageDepth::Gl) => ((a + b / w) + 1.0) * 0.5,
        (Some(w), PageDepth::Log) => (w.max(1.0).log2() / LOG_RANGE).clamp(0.0, 1.0),
    };
    m.fetch_f = Some(Box::new(move |name, _, _| {
        assert_eq!(name, "u_scene_depth");
        [raw, 0.0, 0.0, 1.0]
    }));
    m.gradients.insert(
        "v_world".into(),
        (g.dworld.0.to_vec(), g.dworld.1.map(|x| -x).to_vec()),
    );
    m.set_global("v_xz", Val::f(&g.xz));
    m.set_global("v_layer", Val::f(&[g.layer]));
    m.set_global("v_clip_w", Val::f(&[g.clip_w]));
    m.set_global("v_world", Val::f(&g.world));
    m.set_global("v_floor_w", Val::f(&[g.floor_w]));
    m.set_global("v_haze", Val::f(&[g.haze]));
    m.set_global("v_view", Val::f(&g.view));
    m.set_global(
        "gl_FragCoord",
        Val::f(&[
            g.px.0 as f32 + 0.5,
            H - (g.px.1 as f32 + 0.5),
            0.5,
            1.0 / g.clip_w,
        ]),
    );
    let main = m.function("main");
    m.call(main, Vec::new()).expect("no discard");
    let o = m.global("o_color").floats();
    [o[0], o[1], o[2], o[3]]
}

fn frames() -> Vec<FogFrame> {
    let base = FogFrame {
        sim: [-512.0, -512.0, 16.0, 64.0],
        mesh: [-1024.0, -1024.0, 64.0, 32.0],
        color: [0.82, 0.86, 0.9, 0.6],
        params: [96.0, 12.0, 37.5, -21.0],
        consts: [0.0042, 0.0011, 1.3, 1.6],
        soft: 180.0,
        ground: [-1024.0, -1024.0, 1.0 / 2048.0, 1.0 / 2048.0],
        spill_phase: 3.7,
        spill: [0.031, 220.0, 0.0065, 48.0],
        haze: [2.5, 0.35, 0.0012, 0.6],
    };
    vec![
        base,
        FogFrame {
            params: [140.0, 20.0, -310.0, 92.0],
            spill_phase: 41.0,
            ..base
        },
    ]
}

fn fragments() -> Vec<FogFrag> {
    let mut out = Vec::new();
    let mut s: u32 = 0x2545_F491;
    let mut rnd = |lo: f32, hi: f32| {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        lo + (hi - lo) * ((s >> 8) as f32 / (1u32 << 24) as f32)
    };
    let steep = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.3]);
    let flat = ([1.0, 0.0, 0.0], [0.0, 0.05, 1.0]);
    for k in 0..160 {
        let xz = [rnd(-1100.0, 1100.0), rnd(-1100.0, 1100.0)];
        let clip_w = rnd(300.0, 4000.0);
        let scene_w = match k % 4 {
            0 => None,
            1 => Some(clip_w + rnd(-50.0, 0.0)),
            2 => Some(clip_w + rnd(0.0, 400.0)),
            _ => Some(clip_w + 5000.0),
        };
        out.push(FogFrag {
            px: ((k * 7 % 320) as u32, (k * 13 % 240) as u32),
            xz,
            layer: rnd(0.0, 1.0),
            clip_w,
            world: [xz[0], rnd(-200.0, 0.0), xz[1]],
            floor_w: rnd(0.2, 1.0),
            haze: if k % 5 == 0 { 1.0 } else { 0.0 },
            view: [rnd(-500.0, 500.0), rnd(-800.0, -100.0), rnd(-500.0, 500.0)],
            scene_w,
            dworld: if k % 3 == 0 { steep } else { flat },
        });
    }
    out
}

/// `(fragments compared, differing descriptions, fragments carrying fog)`.
fn compare_against(page_glsl: &str) -> (usize, Vec<String>, usize) {
    let native = parse_wgsl(crate::renderer::FOG_SHADER_SRC);
    let page = parse_glsl(page_glsl, naga::ShaderStage::Fragment);
    let no_u = || -> Box<super::shader_eval::ImageFetch> {
        Box::new(|name, _, _| panic!("uint fetch from {name}"))
    };
    let mut mn = Machine::new(&native, no_u());
    let mut mp = Machine::new(&page, no_u());
    mn.sample = Some(Box::new(|name, uv, _, _| tex(name, uv)));
    mp.sample = Some(Box::new(|name, uv, _, _| tex(name, uv)));
    let frags = fragments();
    let (mut n, mut diffs, mut lit) = (0, Vec::new(), 0);
    for f in frames() {
        for g in &frags {
            let a = run_native(&mut mn, &f, g);
            if a[3] > 1e-4 {
                lit += 1;
            }
            for depth in [PageDepth::Gl, PageDepth::Log] {
                let b = run_page(&mut mp, &f, g, depth);
                n += 1;
                if a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 1e-4) {
                    diffs.push(format!(
                        "{f:?}\n  {g:?} {depth:?}\n  native {a:?}\n  page   {b:?}"
                    ));
                }
            }
        }
    }
    (n, diffs, lit)
}

#[test]
fn the_fog_sheets_shade_identically_on_both_hosts() {
    let (n, diffs, lit) = compare_against(&page_fog_fragment_glsl());
    for d in diffs.iter().take(8) {
        eprintln!("{d}");
    }
    assert!(
        diffs.is_empty(),
        "{} of {n} fog fragments shade differently on the two hosts",
        diffs.len()
    );
    // Not vacuous: a real share of the grid draws fog at all.
    // (`n` counts each fragment twice, once per page depth encoding.)
    assert!(
        lit * 10 >= n / 2,
        "only {lit} of {} fragments carry fog",
        n / 2
    );
}

/// A one-term change to the page's crest band and to its soft-intersection
/// decode are each caught.
#[test]
fn a_changed_page_fog_law_is_caught() {
    let law = page_fog_fragment_glsl();
    for (from, to) in [
        ("smoothstep(0.30, 0.72,", "smoothstep(0.31, 0.72,"),
        (
            "(raw * 2.0 - 1.0) - u_depth.x",
            "(raw * 2.0 - 0.9) - u_depth.x",
        ),
    ] {
        assert!(law.contains(from), "{from}");
        let (_, diffs, _) = compare_against(&law.replacen(from, to, 1));
        assert!(!diffs.is_empty(), "`{from}` -> `{to}` went unnoticed");
    }
}
