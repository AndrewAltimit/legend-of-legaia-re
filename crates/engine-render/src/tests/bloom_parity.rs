//! Bloom parity: the native bloom passes' fragment stages
//! (`renderer::bloom::BLOOM_SHADER_SRC`: `fs_extract`, `fs_blur`,
//! `fs_composite`) against the play page's GLSL transcription
//! (`FS_EXTRACT`, `FS_BLUR`, `FS_COMPOSITE` in `site/js/webgl-bloom.js`),
//! both evaluated through `shader_eval`.
//!
//! The sampled texture is an analytic mock (each host samples the same
//! function of its UV), so filtering is not what is compared - the shader
//! arithmetic is: the soft-knee weight across the threshold, the four
//! extract taps, the Gaussian taps and weights, and the composite's glow
//! term. The native composite adds the scene texel itself where the page
//! leaves that sum to an additive blend; the scene is staged black here, so
//! both write the glow term alone.

use super::page_glsl::{es300_to_450, js_consts, parse_glsl, parse_wgsl, site_js, template};
use super::shader_eval::{Machine, Val};
use legaia_render_kernels::bloom::BloomParams;

/// The analytic colour both hosts sample: three phase-shifted waves over
/// `0..1`, so every brightness band of the knee is visited.
fn tex(uv: [f32; 2]) -> [f32; 4] {
    let [u, v] = uv;
    [
        0.5 + 0.5 * (u * 7.0 + v * 3.0).sin(),
        0.5 + 0.5 * (u * 5.0 - v * 4.0 + 1.0).sin(),
        0.5 + 0.5 * (u * 9.0 + v * 2.0).cos(),
        1.0,
    ]
}

#[derive(Clone, Copy, Debug)]
enum Stage {
    Extract,
    BlurH,
    BlurV,
    Composite,
}

impl Stage {
    const ALL: [Stage; 4] = [Stage::Extract, Stage::BlurH, Stage::BlurV, Stage::Composite];

    fn native_entry(self) -> &'static str {
        match self {
            Stage::Extract => "fs_extract",
            Stage::BlurH | Stage::BlurV => "fs_blur",
            Stage::Composite => "fs_composite",
        }
    }

    fn page_template(self) -> &'static str {
        match self {
            Stage::Extract => "FS_EXTRACT",
            Stage::BlurH | Stage::BlurV => "FS_BLUR",
            Stage::Composite => "FS_COMPOSITE",
        }
    }
}

const SRC_W: f32 = 960.0;
const SRC_H: f32 = 720.0;
const Q_W: f32 = 240.0;
const Q_H: f32 = 180.0;

fn page_fragment(stage: Stage) -> String {
    let consts = js_consts(&site_js("webgl-bloom.js"));
    es300_to_450(&template(&consts, stage.page_template()))
}

type Out = [f32; 4];

fn run_native(m: &mut Machine<'_>, stage: Stage, p: BloomParams, uv: [f32; 2]) -> Out {
    let (texel, step) = match stage {
        Stage::Extract => ([1.0 / SRC_W, 1.0 / SRC_H, p.threshold, p.knee], [0.0; 4]),
        Stage::BlurH => ([1.0 / Q_W, 1.0 / Q_H, 0.0, 0.0], [1.0 / Q_W, 0.0, 0.0, 0.0]),
        Stage::BlurV => ([1.0 / Q_W, 1.0 / Q_H, 0.0, 0.0], [0.0, 1.0 / Q_H, 0.0, 0.0]),
        Stage::Composite => ([0.0; 4], [0.0, 0.0, p.intensity, 0.0]),
    };
    m.set_uniform("texel", Val::f(&texel));
    m.set_uniform("step", Val::f(&step));
    m.fetch_f = Some(Box::new(|name, _, _| {
        assert_eq!(name, "t_scene");
        [0.0; 4]
    }));
    let main = m.function(stage.native_entry());
    let ty = main.arguments[0].ty;
    let names = m.struct_member_names(ty);
    let arg = Val::Struct(
        names
            .iter()
            .map(|n| match n.as_str() {
                "pos" => Val::f(&[uv[0] * SRC_W, uv[1] * SRC_H, 0.0, 1.0]),
                "uv" => Val::f(&uv),
                other => panic!("unstaged bloom input {other}"),
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

fn run_page(m: &mut Machine<'_>, stage: Stage, p: BloomParams, uv: [f32; 2]) -> Out {
    match stage {
        Stage::Extract => m.set_uniform(
            "u_texel",
            Val::f(&[1.0 / SRC_W, 1.0 / SRC_H, p.threshold, p.knee]),
        ),
        Stage::BlurH => m.set_uniform("u_step", Val::f(&[1.0 / Q_W, 0.0])),
        Stage::BlurV => m.set_uniform("u_step", Val::f(&[0.0, 1.0 / Q_H])),
        Stage::Composite => m.set_uniform("u_intensity", Val::f(&[p.intensity])),
    }
    m.set_global("v_uv", Val::f(&uv));
    let main = m.function("main");
    m.call(main, Vec::new()).expect("no discard");
    let o = m.global("o_color").floats();
    // The page adds this over the frame with a saturating blend; against a
    // black frame that is the native composite's `min(.., 1)`.
    match stage {
        Stage::Composite => [o[0].min(1.0), o[1].min(1.0), o[2].min(1.0), o[3]],
        _ => [o[0], o[1], o[2], o[3]],
    }
}

fn uvs() -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for i in 0..12 {
        for j in 0..9 {
            out.push([(i as f32 + 0.37) / 12.0, (j as f32 + 0.61) / 9.0]);
        }
    }
    out
}

fn params() -> Vec<BloomParams> {
    [0.0, 0.25, 0.65, 1.0]
        .into_iter()
        .map(BloomParams::for_glow)
        .collect()
}

/// `(fragments compared, differing descriptions, extract fragments that glow)`.
fn compare(page: impl Fn(Stage) -> String) -> (usize, Vec<String>, usize) {
    let native = parse_wgsl(crate::renderer::BLOOM_SHADER_SRC);
    let no_u = || -> Box<super::shader_eval::ImageFetch> {
        Box::new(|name, _, _| panic!("uint fetch from {name}"))
    };
    let mut mn = Machine::new(&native, no_u());
    mn.sample = Some(Box::new(|name, uv, _, _| {
        assert_eq!(name, "t_src");
        tex(uv)
    }));
    let (mut n, mut diffs, mut glowing) = (0, Vec::new(), 0);
    for stage in Stage::ALL {
        let src = page(stage);
        let module = parse_glsl(&src, naga::ShaderStage::Fragment);
        let mut mp = Machine::new(&module, no_u());
        mp.sample = Some(Box::new(|name, uv, _, _| {
            assert_eq!(name, "u_src");
            tex(uv)
        }));
        for p in params() {
            for uv in uvs() {
                let a = run_native(&mut mn, stage, p, uv);
                let b = run_page(&mut mp, stage, p, uv);
                n += 1;
                if matches!(stage, Stage::Extract) && a[..3].iter().any(|&x| x > 1e-3) {
                    glowing += 1;
                }
                if a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 1e-5) {
                    diffs.push(format!(
                        "{stage:?} {p:?} uv {uv:?}\n  native {a:?}\n  page   {b:?}"
                    ));
                }
            }
        }
    }
    (n, diffs, glowing)
}

#[test]
fn the_bloom_passes_shade_identically_on_both_hosts() {
    let (n, diffs, glowing) = compare(page_fragment);
    for d in diffs.iter().take(8) {
        eprintln!("{d}");
    }
    assert!(
        diffs.is_empty(),
        "{} of {n} bloom fragments differ between the two hosts",
        diffs.len()
    );
    // Not vacuous: the extract stage lets a real share of the grid glow,
    // and holds the rest back.
    let extract = n / Stage::ALL.len();
    assert!(
        glowing * 5 >= extract && glowing < extract,
        "{glowing} of {extract} extract fragments glow"
    );
}

/// A one-term change to the page's knee and to its blur weights is caught.
#[test]
fn a_changed_page_bloom_law_is_caught() {
    for (from, to) in [
        ("(4.0 * knee + 1e-5)", "(3.0 * knee + 1e-5)"),
        ("* 0.3162162162", "* 0.3262162162"),
    ] {
        let page = |stage: Stage| {
            let src = page_fragment(stage);
            src.replacen(from, to, 1)
        };
        assert!(
            Stage::ALL.iter().any(|&s| page_fragment(s).contains(from)),
            "{from}"
        );
        let (_, diffs, _) = compare(page);
        assert!(!diffs.is_empty(), "`{from}` -> `{to}` went unnoticed");
    }
}
