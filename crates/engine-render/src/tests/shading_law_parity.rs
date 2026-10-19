//! Shading-law parity: the native renderer's WGSL against the browser play
//! page's GLSL, evaluated, not compared as text.
//!
//! The two hosts write the field mesh shading law twice - the VRAM-mesh and
//! colour-mesh pipelines in WGSL here, one fragment shader in GLSL in
//! `site/js/webgl-shaders.js` - and no compiler relates them. This test
//! parses both into naga IR (the page's template resolved against its own
//! JS constants and re-declared as GLSL 4.50 bindings, with no law line
//! touched), runs them on the CPU through `shader_eval`, and pushes one grid
//! of inputs through each: every CLUT word of a grid page through a CLUT row
//! the prologue's palette law reaches and one it does not, packet colours
//! across the `0x80` neutral, both prim families, the opaque pass and each
//! ABR mode's blend pass (composited through each host's blend state), the
//! colour grade, the palette collapse, the view-depth cue ramp and the
//! 15-bit dither over a 4x4 pixel block. Outputs must agree to float
//! rounding; under the dither a value exactly on a 5-bit boundary may land
//! one step apart (the native law divides the colour byte by 128, the page
//! multiplies the normalised colour by 255 / 128), and those are counted
//! and bounded.
//!
//! Out of the grid: the constant depth cue (`MeshUniforms.depth_cue.a`
//! with the ramp off - no production caller stages it, and the page has no
//! such arm), and the opt-in enhancements (dynamic light, occlusion fade,
//! point lights), which evaluate as their identities here.

use crate::shaders::{COLOR_MESH_SHADER_SRC, VRAM_MESH_SHADER_SRC, compose_psx_shader};
use std::collections::HashMap;

fn page_shader_js() -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/js/webgl-shaders.js");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The JS constants a shader template interpolates: numbers, number arrays
/// and nested template strings, keyed by name.
#[derive(Debug, Clone)]
enum JsConst {
    Num(f64),
    Arr(Vec<f64>),
    Template(String),
}

fn js_consts(js: &str) -> HashMap<String, JsConst> {
    let mut out = HashMap::new();
    let mut rest = js;
    while let Some(i) = rest.find("\nconst ") {
        rest = &rest[i + 7..];
        let Some(eq) = rest.find(" = ") else { break };
        let name = rest[..eq].trim().to_string();
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let val = &rest[eq + 3..];
        if let Some(body) = val.strip_prefix('`') {
            let end = body.find('`').expect("template end");
            out.insert(name, JsConst::Template(body[..end].to_string()));
        } else if let Some(body) = val.strip_prefix('[') {
            let end = body.find(']').expect("array end");
            let nums = body[..end]
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("{name}: {s}")))
                .collect();
            out.insert(name, JsConst::Arr(nums));
        } else {
            let end = val.find(';').unwrap_or(val.len());
            if let Ok(v) = val[..end].trim().parse::<f64>() {
                out.insert(name, JsConst::Num(v));
            }
        }
    }
    out
}

/// `glslFloat` from webgl-shaders.js: integers print with `.0`.
fn glsl_float(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// Resolve every `${...}` in a template against the file's constants. Only
/// the interpolation forms the shaders use are understood; anything else
/// fails, so a new form needs this resolver taught it.
fn resolve_template(t: &str, consts: &HashMap<String, JsConst>) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('}').expect("interpolation end") + i;
        let expr = rest[i + 2..end].trim();
        let num = |n: &str| match consts.get(n) {
            Some(JsConst::Num(v)) => *v,
            other => panic!("`{n}` is not a numeric constant: {other:?}"),
        };
        let arr = |n: &str| match consts.get(n) {
            Some(JsConst::Arr(v)) => v.clone(),
            other => panic!("`{n}` is not an array constant: {other:?}"),
        };
        let text = if let Some(n) = expr.strip_suffix(".map(glslFloat).join(', ')") {
            arr(n)
                .into_iter()
                .map(glsl_float)
                .collect::<Vec<_>>()
                .join(", ")
        } else if let Some(inner) = expr
            .strip_prefix("glslFloat(")
            .and_then(|s| s.strip_suffix(')'))
        {
            if let Some((n, idx)) = inner.split_once('[') {
                let idx: usize = idx.trim_end_matches(']').parse().unwrap();
                glsl_float(arr(n)[idx])
            } else {
                glsl_float(num(inner))
            }
        } else {
            match consts.get(expr) {
                Some(JsConst::Template(s)) => resolve_template(s, consts),
                other => panic!("unsupported interpolation `{expr}`: {other:?}"),
            }
        };
        out.push_str(&text);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Rewrite a GLSL ES 3.00 fragment shader into the Vulkan-flavoured GLSL 4.50
/// naga's front end reads, without touching a line of the shading law: the
/// version line, precision statements, loose uniforms gathered into one block,
/// opaque uniforms and stage I/O given explicit bindings.
fn es300_to_450(src: &str) -> String {
    let mut block = Vec::new();
    let mut body = Vec::new();
    let (mut loc_in, mut binding) = (0u32, 0u32);
    // naga reads no combined image-samplers: each one becomes a texture and
    // a sampler, recombined at its call sites.
    let mut combined: Vec<(String, &'static str)> = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("#version") {
            continue;
        }
        if t.starts_with("precision ") {
            continue;
        }
        if let Some(decl) = t.strip_prefix("uniform ") {
            let decl = decl.trim_start_matches("highp ");
            if decl.contains("sampler") {
                let mut it = decl.trim_end_matches(';').split_whitespace();
                let (ty, name) = (it.next().unwrap(), it.next().unwrap());
                let (tex, samp, comb) = match ty {
                    "usampler2D" => ("utexture2D", "sampler", "usampler2D"),
                    "sampler2D" => ("texture2D", "sampler", "sampler2D"),
                    "sampler2DArrayShadow" => {
                        ("texture2DArray", "samplerShadow", "sampler2DArrayShadow")
                    }
                    other => panic!("unmapped opaque uniform type {other}"),
                };
                body.push(format!(
                    "layout(set = 1, binding = {binding}) uniform {tex} {name};"
                ));
                body.push(format!(
                    "layout(set = 1, binding = {}) uniform {samp} {name}_s;",
                    binding + 1
                ));
                binding += 2;
                combined.push((name.to_string(), comb));
            } else {
                block.push(format!("    {decl}"));
            }
            continue;
        }
        let is_in = t.starts_with("in ") || t.starts_with("flat in ");
        if is_in {
            body.push(format!("layout(location = {loc_in}) {t}"));
            loc_in += 1;
            continue;
        }
        if t.starts_with("out vec4 ") {
            body.push(format!("layout(location = 0) {t}"));
            continue;
        }
        let mut l = line.to_string();
        for (name, comb) in &combined {
            l = l.replace(&format!("({name},"), &format!("({comb}({name}, {name}_s),"));
        }
        body.push(l);
    }
    format!(
        "#version 450 core\nlayout(set = 0, binding = 0, std140) uniform PageUniforms {{\n{}\n}};\n{}\n",
        block.join("\n"),
        body.join("\n")
    )
}

fn page_fragment_glsl() -> String {
    let js = page_shader_js();
    let consts = js_consts(&js);
    let fs = match consts.get("FS_SRC") {
        Some(JsConst::Template(s)) => s.clone(),
        _ => panic!("FS_SRC template not found"),
    };
    es300_to_450(&resolve_template(&fs, &consts))
}

fn parse_glsl(src: &str) -> naga::Module {
    let mut fe = naga::front::glsl::Frontend::default();
    let opts = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
    fe.parse(&opts, src)
        .unwrap_or_else(|e| panic!("page GLSL does not parse:\n{}", e.emit_to_string(src)))
}

fn parse_wgsl(src: &str) -> naga::Module {
    naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)))
}

#[test]
fn both_shader_texts_parse() {
    let _ = parse_wgsl(&compose_psx_shader(VRAM_MESH_SHADER_SRC));
    let _ = parse_wgsl(&compose_psx_shader(COLOR_MESH_SHADER_SRC));
    let _ = parse_glsl(&page_fragment_glsl());
}

// ---- the shared scene state, staged into each host's uniforms ----

const STAGE_W: f32 = 320.0;
const STAGE_H: f32 = 240.0;

/// One scene's shading state, in the engine's terms; each host's staging
/// below turns it into that shader's uniforms.
#[derive(Clone, Copy, Debug)]
struct SceneState {
    /// Colour grade `(tint rgb, strength)`; strength 0 = identity.
    grade: [f32; 4],
    /// Prologue palette collapse `(screen tint rgb, enable)`.
    palette: [f32; 4],
    /// View-depth cue ramp `(near_z, far_z, max_ir0)`, or none.
    ramp: Option<[f32; 3]>,
    /// DPCS far colour, display 0..1.
    far: [f32; 3],
    dither: bool,
}

/// The textured CLUT rows the grid draws through: 480 is inside the rect
/// the prologue's CLUT cyclers re-upload, 300 outside it.
const CLUT_ROWS: [u32; 2] = [480, 300];

/// The 8bpp texture page the grid samples (`tsb` page 0, depth bits = 1):
/// row 0 holds the indices `0..=255` in order, so texel `u` is CLUT entry
/// `u`.
const TSB_8BPP: u32 = 1 << 7;
/// The engine's packed ABE (semi-transparency) bit on `tsb` / `blend`.
const ABE: u32 = 0x8000;

/// The CLUT words the grid puts behind each index: the transparent zero,
/// black with STP, the pure primaries, greys, STP-set colours and a spread
/// of mixed words.
fn clut_words() -> Vec<u16> {
    let mut w: Vec<u16> = vec![0x0000, 0x8000, 0x001F, 0x03E0, 0x7C00, 0x7FFF, 0xFFFF];
    for g in [1u16, 8, 15, 16, 24, 31] {
        w.push(g | (g << 5) | (g << 10));
    }
    let mut s: u32 = 0x1234_5678;
    while w.len() < 256 {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        w.push((s >> 16) as u16);
    }
    w
}

fn vram() -> Vec<u16> {
    let mut v = vec![0u16; 1024 * 512];
    for (x, w) in v.iter_mut().take(128).enumerate() {
        let lo = (2 * x) as u16;
        *w = lo | ((lo + 1) << 8);
    }
    let words = clut_words();
    for row in CLUT_ROWS {
        for (i, &w) in words.iter().enumerate() {
            v[row as usize * 1024 + i] = w;
        }
    }
    v
}

fn fetcher(v: std::rc::Rc<Vec<u16>>) -> Box<shader_eval::ImageFetch> {
    Box::new(move |name, x, y| {
        assert!(name == "t_vram" || name == "u_vram", "fetch from {name}");
        let w = v[(y.clamp(0, 511) as usize) * 1024 + x.clamp(0, 1023) as usize];
        [u32::from(w), 0, 0, 1]
    })
}

use super::shader_eval::{self, Machine, Val};

/// One fragment of one prim.
#[derive(Clone, Copy, Debug)]
struct Fragment {
    /// Textured: `Some((cba, tsb, u))`; untextured: `None` (`blend` bits in
    /// `flat_bits`).
    tex: Option<(u32, u32, u32)>,
    flat_bits: u32,
    /// Packet colour bytes.
    colour: [u8; 3],
    /// Top-down stage pixel.
    px: (u32, u32),
    view_z: f32,
    /// `None` for the opaque pass; `Some(abr)` for the blend pass of a
    /// semi-transparent prim drawn with ABR mode `abr`.
    blend: Option<u8>,
}

/// The fragment's output, `None` when it is discarded. Clamped to the
/// framebuffer's 0..1, which is what either host's target stores.
type Out = Option<[f32; 3]>;

fn clamp_out(v: &Val) -> [f32; 3] {
    let f = v.floats();
    [
        f[0].clamp(0.0, 1.0),
        f[1].clamp(0.0, 1.0),
        f[2].clamp(0.0, 1.0),
    ]
}

/// The native renderer: the VRAM-mesh pipeline for a textured fragment, the
/// colour-mesh pipeline for an untextured one, each with the uniforms
/// `Renderer` stages for `scene`.
fn run_native(m: &mut Machine<'_>, scene: &SceneState, frag: &Fragment) -> Out {
    let (ramp, cue_a) = match scene.ramp {
        Some([near, far, max]) => ([near, 1.0 / (far - near), max, 1.0], 0.0),
        None => ([0.0; 4], 0.0),
    };
    m.set_uniform(
        "depth_cue",
        Val::f(&[scene.far[0], scene.far[1], scene.far[2], cue_a]),
    );
    m.set_uniform(
        "psx_params",
        Val::f(&[STAGE_W, STAGE_H, 0.0, if scene.dither { 1.0 } else { 0.0 }]),
    );
    m.set_uniform("grade", Val::f(&scene.grade));
    m.set_uniform("palette", Val::f(&scene.palette));
    m.set_uniform("cue_ramp", Val::f(&ramp));
    // Cull off, the semi-blend split on (the faithful two-pass draw).
    m.set_uniform("flags", Val::f(&[0.0, 1.0, 0.0, 0.0]));
    // The renderer's per-ABR blend pipelines pick the entry by the mode's
    // foreground pre-scale (`psx_blend::src_shader_scale`).
    let entry = match frag.blend {
        None => "fs_main",
        Some(abr) if crate::psx_blend::src_shader_scale(abr) == 1.0 => "fs_blend",
        Some(_) => "fs_blend_quarter",
    };
    let f = m.function(entry);
    let ty = f.arguments[0].ty;
    let names = m.struct_member_names(ty);
    let zero = match m.zero(ty) {
        Val::Struct(z) => z,
        _ => unreachable!(),
    };
    let clip = Val::f(&[
        frag.px.0 as f32 + 0.5,
        frag.px.1 as f32 + 0.5,
        0.5,
        1.0 / frag.view_z,
    ]);
    let rgb = frag.colour.map(f32::from);
    let mut vals: HashMap<&str, Val> = HashMap::new();
    vals.insert("clip_pos", clip);
    match frag.tex {
        Some((cba, tsb, u)) => {
            vals.insert("uv_affine", Val::f(&[u as f32 + 0.25, 0.25]));
            vals.insert("cba_tsb", Val::u(&[cba, tsb]));
            vals.insert("prim_color", Val::f(&rgb));
        }
        None => {
            vals.insert(
                "color",
                Val::f(&[rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0, 1.0]),
            );
            vals.insert("blend", Val::u(&[frag.flat_bits]));
        }
    }
    let arg = shader_eval::by_name(&names, &vals, &zero);
    m.call(f, vec![arg, Val::B(vec![true])])
        .ok()
        .map(|v| clamp_out(&v.expect("fragment colour")))
}

/// The browser play page: its one fragment shader, both prim families, with
/// the uniforms `TmdRenderer` stages for `scene`.
fn run_page(m: &mut Machine<'_>, scene: &SceneState, frag: &Fragment) -> Out {
    let cue = match scene.ramp {
        Some([near, far, max]) => [near, far, max, 1.0],
        None => [0.0; 4],
    };
    m.set_uniform(
        "u_psx",
        Val::f(&[STAGE_W, STAGE_H, 0.0, if scene.dither { 1.0 } else { 0.0 }]),
    );
    m.set_uniform("u_grade", Val::f(&scene.grade));
    m.set_uniform("u_palette", Val::f(&scene.palette));
    m.set_uniform("u_cue", Val::f(&cue));
    m.set_uniform("u_cue_far", Val::f(&scene.far));
    m.set_uniform("u_semi_pass", Val::I(vec![i32::from(frag.blend.is_some())]));
    m.set_uniform("u_use_flat_colors", Val::I(vec![1]));
    m.set_uniform("u_pair_front", Val::I(vec![1]));
    let rgb = frag.colour.map(|c| f32::from(c) / 255.0);
    let (uv, cba_tsb, flag) = match frag.tex {
        Some((cba, tsb, u)) => ([u as f32 + 0.25, 0.25], [cba, tsb], 1.0),
        None => ([0.0, 0.0], [0, frag.flat_bits], 0.0),
    };
    m.set_global("v_uv_pw", Val::f(&uv));
    m.set_global("v_flat_rgba_pw", Val::f(&[rgb[0], rgb[1], rgb[2], flag]));
    m.set_global("v_affine_w", Val::f(&[1.0]));
    m.set_global("v_cba_tsb", Val::u(&cba_tsb));
    m.set_global("v_view_z", Val::f(&[frag.view_z]));
    m.set_global("v_depth_w", Val::f(&[frag.view_z]));
    // gl_FragCoord counts rows from the bottom.
    m.set_global(
        "gl_FragCoord",
        Val::f(&[
            frag.px.0 as f32 + 0.5,
            STAGE_H - (frag.px.1 as f32 + 0.5),
            0.5,
            1.0 / frag.view_z,
        ]),
    );
    m.set_global("gl_FrontFacing", Val::B(vec![true]));
    let main = m.function("main");
    match m.call(main, Vec::new()) {
        Err(()) => None,
        Ok(_) => Some(clamp_out(m.global("o_color"))),
    }
}

/// The scene states the grid crosses: identity, the grade, the prologue
/// palette collapse, the view-depth cue ramp, and the grade under the ramp,
/// each with the 15-bit dither off and on.
fn scenes() -> Vec<SceneState> {
    let base = SceneState {
        grade: [1.0, 1.0, 1.0, 0.0],
        palette: [1.0, 1.0, 1.0, 0.0],
        ramp: None,
        far: [0.0; 3],
        dither: false,
    };
    let grade = [1.15, 0.9, 0.55, 0.7];
    let ramp = Some([200.0, 2200.0, 0.85]);
    let far = [0.62, 0.48, 0.21];
    let mut out = Vec::new();
    for dither in [false, true] {
        let b = SceneState { dither, ..base };
        out.push(b);
        out.push(SceneState { grade, ..b });
        out.push(SceneState {
            palette: [0.95, 0.86, 0.66, 1.0],
            ..b
        });
        out.push(SceneState { ramp, far, ..b });
        out.push(SceneState {
            grade,
            ramp,
            far,
            ..b
        });
    }
    out
}

/// Packet colours: black, the `0x80` neutral and its neighbours, the dark
/// and bright tails, and off-neutral words.
const COLOURS: [[u8; 3]; 9] = [
    [0, 0, 0],
    [0x80, 0x80, 0x80],
    [0x7F, 0x80, 0x81],
    [0x40, 0x40, 0x40],
    [0xFF, 0xFF, 0xFF],
    [0xC0, 0x60, 0x20],
    [0x10, 0xF0, 0x90],
    [0xF8, 0xF8, 0xF8],
    [98, 94, 42],
];

/// The draws a prim takes: an opaque prim draws once; a semi-transparent
/// (ABE) prim draws in the opaque pass (its non-blending texels) and again
/// in the blend pass. Neither host hands a blend pass an opaque prim - the
/// native renderer's blend pass draws only the ABE index tail - so that
/// pairing is not in the grid.
const PASSES: [(u32, Option<u8>); 6] = [
    (0, None),
    (ABE, None),
    (ABE, Some(0)),
    (ABE, Some(1)),
    (ABE, Some(2)),
    (ABE, Some(3)),
];

/// View depths: inside the ramp's near plane, mid-ramp, past its far end.
const DEPTHS: [f32; 3] = [100.0, 1200.0, 5000.0];

struct Diff {
    what: String,
    native: Out,
    page: Out,
}

/// The backbuffer values a blend-pass fragment is composited over.
const BACKGROUNDS: [f32; 4] = [0.0, 0.3, 0.7, 1.0];

/// One channel through a native blend pipeline: the `wgpu::BlendState` the
/// renderer builds for ABR mode `abr` (`psx_blend::blend_state`), with the
/// blend constant its passes set (`MODE0_BLEND_CONSTANT`).
fn native_composite(abr: u8, f: f32, b: f32) -> f32 {
    use wgpu::{BlendFactor as Bf, BlendOperation as Bop};
    let c = crate::psx_blend::blend_state(abr).color;
    let k = crate::psx_blend::MODE0_BLEND_CONSTANT as f32;
    let factor = |x: Bf| match x {
        Bf::One => 1.0,
        Bf::Zero => 0.0,
        Bf::Constant => k,
        other => panic!("blend factor not modelled: {other:?}"),
    };
    let (s, d) = (f * factor(c.src_factor), b * factor(c.dst_factor));
    let r = match c.operation {
        Bop::Add => s + d,
        Bop::Subtract => s - d,
        Bop::ReverseSubtract => d - s,
        other => panic!("blend op not modelled: {other:?}"),
    };
    r.clamp(0.0, 1.0)
}

/// The play page's `_setSemiBlend(mode)` (webgl-tmd-assembled-scene.js),
/// whitespace collapsed. [`page_composite`] is this body's arithmetic; the
/// text is pinned so an edit to the page's blend table fails here rather
/// than going unevaluated.
const PAGE_SEMI_BLEND_BODY: &str = "const gl = this.gl; \
     gl.blendEquation(mode === 2 ? gl.FUNC_REVERSE_SUBTRACT : gl.FUNC_ADD); \
     if (mode === 0) { gl.blendColor(0, 0, 0, 0.5); \
     gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA); } \
     else if (mode === 3) { gl.blendColor(0, 0, 0, 0.25); \
     gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE); } \
     else { gl.blendFunc(gl.ONE, gl.ONE); }";

fn page_composite(abr: u8, f: f32, b: f32) -> f32 {
    let r = match abr {
        0 => 0.5 * f + (1.0 - 0.5) * b,
        3 => 0.25 * f + b,
        2 => b - f,
        _ => f + b,
    };
    r.clamp(0.0, 1.0)
}

#[test]
fn the_page_blend_table_is_the_one_evaluated() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../site/js/webgl-tmd-assembled-scene.js");
    let js = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    let start = js.find("_setSemiBlend(mode) {").expect("_setSemiBlend");
    let open = start + js[start..].find('{').unwrap();
    let mut depth = 0;
    let mut end = open;
    for (i, c) in js[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = js[open + 1..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let want = PAGE_SEMI_BLEND_BODY
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        body, want,
        "the page's ABR blend table changed: re-derive page_composite"
    );
}

/// How two outputs relate.
enum Verdict {
    Same,
    /// Every channel within one 5-bit step under the dither: the two texts
    /// reach the dither's truncation with values a float rounding apart
    /// (the native shader modulates by the colour byte over 128, the page by
    /// the normalised colour times 255 / 128), and a value sitting exactly
    /// on a 5-bit boundary truncates to adjacent steps.
    DitherStep,
    Different,
}

fn verdict(native: Out, page: Out, dither: bool) -> Verdict {
    match (native, page) {
        (None, None) => Verdict::Same,
        (Some(a), Some(b)) => {
            let d: Vec<f32> = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).collect();
            if d.iter().all(|x| *x <= 1e-5) {
                Verdict::Same
            } else if dither && d.iter().all(|x| *x <= 9.0 / 255.0 + 1e-5) {
                Verdict::DitherStep
            } else {
                Verdict::Different
            }
        }
        _ => Verdict::Different,
    }
}

struct Tally {
    n: usize,
    dither_steps: usize,
    diffs: Vec<Diff>,
}

fn compare(frags: &[Fragment]) -> Tally {
    compare_against(frags, &page_fragment_glsl())
}

fn compare_against(frags: &[Fragment], page_glsl: &str) -> Tally {
    let v = std::rc::Rc::new(vram());
    let native_tex = parse_wgsl(&compose_psx_shader(VRAM_MESH_SHADER_SRC));
    let native_flat = parse_wgsl(&compose_psx_shader(COLOR_MESH_SHADER_SRC));
    let page = parse_glsl(page_glsl);
    let mut mt = Machine::new(&native_tex, fetcher(v.clone()));
    let mut mf = Machine::new(&native_flat, fetcher(v.clone()));
    let mut mp = Machine::new(&page, fetcher(v));
    let mut t = Tally {
        n: 0,
        dither_steps: 0,
        diffs: Vec::new(),
    };
    for scene in scenes() {
        for frag in frags {
            let native = if frag.tex.is_some() {
                run_native(&mut mt, &scene, frag)
            } else {
                run_native(&mut mf, &scene, frag)
            };
            let page = run_page(&mut mp, &scene, frag);
            // A blend-pass fragment reaches the framebuffer through its ABR
            // equation, so compare the composites over a set of backbuffer
            // values; an opaque fragment is the stored colour itself.
            let pairs: Vec<(Out, Out)> = match frag.blend {
                None => vec![(native, page)],
                Some(abr) => BACKGROUNDS
                    .iter()
                    .map(|&b| {
                        let comp =
                            |o: Out, f: fn(u8, f32, f32) -> f32| o.map(|c| c.map(|x| f(abr, x, b)));
                        (comp(native, native_composite), comp(page, page_composite))
                    })
                    .collect(),
            };
            for (native, page) in pairs {
                t.n += 1;
                match verdict(native, page, scene.dither) {
                    Verdict::Same => {}
                    Verdict::DitherStep => t.dither_steps += 1,
                    Verdict::Different => t.diffs.push(Diff {
                        what: format!("{scene:?} {frag:?}"),
                        native,
                        page,
                    }),
                }
            }
        }
    }
    t
}

fn report(t: &Tally) {
    for d in t.diffs.iter().take(12) {
        eprintln!("{}\n  native {:?}\n  page   {:?}", d.what, d.native, d.page);
    }
    eprintln!(
        "{} outputs compared, {} differ, {} one dither step apart",
        t.n,
        t.diffs.len(),
        t.dither_steps
    );
    assert!(
        t.diffs.is_empty(),
        "{} of {} outputs shade differently on the two hosts",
        t.diffs.len(),
        t.n
    );
    // The rounding flips are rare by construction (a value exactly on a
    // 5-bit boundary); a law change that moved whole bands one step would
    // swamp this bound.
    assert!(
        t.dither_steps * 1000 <= t.n,
        "{} of {} outputs sit one dither step apart - more than float rounding explains",
        t.dither_steps,
        t.n
    );
}

/// Textured prims: every CLUT word of the grid through both CLUT rows (so
/// the palette law both reaches and skips), with and without the ABE bit,
/// in the opaque and the blend pass, at every packet colour, depth and
/// scene state - and at the sixteen dither phases of a 4x4 pixel block.
#[test]
fn textured_fragments_shade_identically_on_both_hosts() {
    let mut frags = Vec::new();
    for row in CLUT_ROWS {
        for (abe, blend) in PASSES {
            {
                for (k, colour) in COLOURS.iter().enumerate() {
                    for (d, view_z) in DEPTHS.iter().enumerate() {
                        for u in (0..256u32).step_by(19) {
                            let px = ((u + k as u32) % 4, (u / 4 + d as u32) % 4);
                            frags.push(Fragment {
                                tex: Some((row << 6, TSB_8BPP | abe, u)),
                                flat_bits: 0,
                                colour: *colour,
                                px: (16 + px.0, 32 + px.1),
                                view_z: *view_z,
                                blend,
                            });
                        }
                    }
                }
            }
        }
    }
    report(&compare(&frags));
}

/// Untextured prims: the packet colour is the shading, with and without the
/// ABE bit, in both passes, at every colour, depth and scene state, over a
/// 4x4 block of dither phases.
#[test]
fn untextured_fragments_shade_identically_on_both_hosts() {
    let mut frags = Vec::new();
    for (abe, blend) in PASSES {
        {
            for colour in COLOURS {
                for view_z in DEPTHS {
                    for p in 0..16u32 {
                        frags.push(Fragment {
                            tex: None,
                            flat_bits: abe,
                            colour,
                            px: (40 + p % 4, 60 + p / 4),
                            view_z,
                            blend,
                        });
                    }
                }
            }
        }
    }
    report(&compare(&frags));
}

/// The comparison is not vacuous: a one-byte change to the page's modulate
/// law (`/ 128` read as `/ 127`) and a dither matrix with one entry negated
/// are each caught.
#[test]
fn a_changed_page_law_is_caught() {
    let frags: Vec<Fragment> = COLOURS
        .iter()
        .take(3)
        .flat_map(|&colour| {
            (0..256u32).step_by(37).flat_map(move |u| {
                (0..16u32).map(move |p| Fragment {
                    tex: Some((CLUT_ROWS[1] << 6, TSB_8BPP, u)),
                    flat_bits: 0,
                    colour,
                    px: (p % 4, p / 4),
                    view_z: 1200.0,
                    blend: None,
                })
            })
        })
        .collect();
    let law = page_fragment_glsl();
    let modulate = "(255.0 / 128.0)";
    assert!(law.contains(modulate));
    let off_by_one = law.replacen(modulate, "(255.0 / 127.0)", 1);
    assert!(!compare_against(&frags, &off_by_one).diffs.is_empty());
    let dm = "float dm[16] = float[16](-4.0, 0.0,";
    assert!(law.contains(dm), "dither matrix spelling");
    let moved = law.replacen(dm, "float dm[16] = float[16](4.0, 0.0,", 1);
    let t = compare_against(&frags, &moved);
    assert!(!t.diffs.is_empty() || t.dither_steps > 0);
}
