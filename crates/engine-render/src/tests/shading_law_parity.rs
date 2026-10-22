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
//! The native side is the scene pipelines (the real point-light and
//! occlusion layer); with that block zeroed they are the single-mesh
//! pipelines exactly. A second grid raises the enhancement layer at its
//! enabled settings (both hosts default it on): the key light, screen pool,
//! ambient floor, emissive gain, window glow and two point lights (shadows
//! off - each host samples maps rendered under its own clip conventions), the
//! occlusion fade's screen-door decision, and the object-effect clip through
//! the native f16 packing. Each host stages it in its own frame: the page's
//! world is `(x, -y, z)` and its fade pixels count rows from the bottom.
//!
//! Out of the grid: the constant depth cue (`MeshUniforms.depth_cue.a`
//! with the ramp off - no production caller stages it, and the page has no
//! such arm).

use crate::shaders::{
    COLOR_MESH_SHADER_SRC, VRAM_MESH_SHADER_SRC, compose_psx_shader, compose_scene_lit_shader,
};
use std::collections::HashMap;

use super::page_glsl::{es300_to_450, js_consts, parse_wgsl, site_js, template};

/// The page's one field-mesh fragment shader (`FS_SRC`), materialised.
pub(super) fn page_fragment_glsl() -> String {
    let consts = js_consts(&site_js("webgl-shaders.js"));
    es300_to_450(&template(&consts, "FS_SRC"))
}

fn parse_glsl(src: &str) -> naga::Module {
    super::page_glsl::parse_glsl(src, naga::ShaderStage::Fragment)
}

#[test]
fn both_shader_texts_parse() {
    let _ = parse_wgsl(&compose_psx_shader(VRAM_MESH_SHADER_SRC));
    let _ = parse_wgsl(&compose_psx_shader(COLOR_MESH_SHADER_SRC));
    let _ = parse_wgsl(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC));
    let _ = parse_wgsl(&compose_scene_lit_shader(COLOR_MESH_SHADER_SRC));
    let _ = parse_glsl(&page_fragment_glsl());
}

/// The blend pass's foreground is the GPU's truncated 5-bit channel on both
/// hosts: `floor(texel5 * colour / 128)`. A dim texel under a dim colour
/// adds nothing, and an exact step keeps its value.
#[test]
fn the_blend_foreground_truncates_to_five_bits_on_both_hosts() {
    let native = parse_wgsl(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC));
    let page = parse_glsl(&page_fragment_glsl());
    let no_fetch = || -> Box<shader_eval::ImageFetch> { Box::new(|_, _, _| [0, 0, 0, 1]) };
    let mut machines = [
        Machine::new(&native, no_fetch()),
        Machine::new(&page, no_fetch()),
    ];
    // `(texel5, colour byte)` -> the GPU's 5-bit foreground.
    let cases: [(u32, u32, u32); 5] = [
        (1, 0x40, 0),
        (10, 0x30, 3),
        (10, 0x60, 7),
        (16, 0x80, 16),
        (31, 0xFF, 31),
    ];
    for m in &mut machines {
        let f = m.function("psx_blend_foreground");
        for &(t5, c, want) in &cases {
            let x = t5 as f32 / 31.0 * c as f32 / 128.0;
            let out = m
                .call(f, vec![Val::f(&[x, x, x])])
                .ok()
                .flatten()
                .expect("foreground");
            let got = (out.floats()[0] * 31.0).round() as u32;
            assert_eq!(got, want, "texel5 {t5} colour {c:#x}");
        }
    }
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
    /// The enhancement layer this frame stages, or none (the faithful path).
    enh: Option<&'static Enhance>,
}

/// One frame's enhancement layer, in the engine's (retail Y-down) frame -
/// what `scene_lighting::LightingMood::uniforms`, the scene-light pick, the
/// occlusion fade and the object-effect clip hand each host.
#[derive(Debug)]
struct Enhance {
    /// Key light direction (unit), key colour x strength, pool weight.
    key_dir: [f32; 3],
    key_rgb: [f32; 3],
    pool: f32,
    /// Ambient floor and emissive gain.
    ambient: [f32; 3],
    emissive: f32,
    /// The mood's window glow.
    window: f32,
    /// Point lights: world position, radius, gain colour (shadows off).
    lights: &'static [([f32; 3], f32, [f32; 3])],
    occl: Option<Occl>,
    /// The object-effect clip `lo <= m . p <= hi`, mesh space.
    clip: Option<([f32; 3], f32, f32)>,
}

/// The occlusion fade's staging, in top-down framebuffer pixels.
#[derive(Debug, Clone, Copy)]
struct Occl {
    focus: [f32; 2],
    focus_z: f32,
    strength: f32,
    radius: f32,
    feet: Option<[f32; 2]>,
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
    geo: Geometry,
}

/// Where a fragment sits, for the enhancement layer: its world position
/// (retail frame), its mesh-space position with the surface's screen
/// gradients (top-down rows), and its smoothed vertex normal (zero = none).
#[derive(Clone, Copy, Debug)]
struct Geometry {
    world: [f32; 3],
    pos: [f32; 3],
    dpos: ([f32; 3], [f32; 3]),
    normal: [f32; 3],
}

const NO_GEOMETRY: Geometry = Geometry {
    world: [0.0; 3],
    pos: [0.0; 3],
    dpos: ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    normal: [0.0; 3],
};

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
    stage_native_enhancements(m, scene.enh);
    let g = &frag.geo;
    m.gradients
        .insert("world_pos".into(), (g.dpos.0.to_vec(), g.dpos.1.to_vec()));
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
    vals.insert("world_pos", Val::f(&g.pos));
    vals.insert("world_w", Val::f(&g.world));
    vals.insert("normal", Val::f(&g.normal));
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

/// The enhancement layer as the native renderer stages it: the mood on
/// `MeshUniforms.light_dir` / `light_color` and the scene-lights block, the
/// lights with their shadows off, the occlusion fade on `flags.z` and the
/// scene-lights block, the clip packed into `tex_window` lanes 1..3.
fn stage_native_enhancements(m: &mut Machine<'_>, enh: Option<&Enhance>) {
    let zero4 = Val::f(&[0.0; 4]);
    let light = |p: [f32; 3], r: f32, c: [f32; 3]| {
        Val::Struct(vec![
            Val::f(&[p[0], p[1], p[2], r]),
            Val::f(&[c[0], c[1], c[2], 0.0]),
            Val::Arr(vec![Val::f(&[0.0; 4]); 4]),
        ])
    };
    let mut lights = vec![light([0.0; 3], 0.0, [0.0; 3]); 8];
    let Some(e) = enh else {
        m.set_uniform("flags", Val::f(&[0.0, 1.0, 0.0, 0.0]));
        m.set_uniform("light_dir", zero4.clone());
        m.set_uniform("light_color", zero4.clone());
        m.set_uniform("ambient", zero4.clone());
        m.set_uniform("params", zero4.clone());
        m.set_uniform("occl_focus", zero4.clone());
        m.set_uniform("occl_params", zero4.clone());
        m.set_uniform("occl_lift", zero4);
        m.set_uniform("lights", Val::Arr(lights));
        m.set_uniform("tex_window", Val::u(&[0; 4]));
        return;
    };
    m.set_uniform(
        "light_dir",
        Val::f(&[e.key_dir[0], e.key_dir[1], e.key_dir[2], 1.0]),
    );
    m.set_uniform(
        "light_color",
        Val::f(&[e.key_rgb[0], e.key_rgb[1], e.key_rgb[2], e.pool]),
    );
    m.set_uniform(
        "ambient",
        Val::f(&[e.ambient[0], e.ambient[1], e.ambient[2], e.emissive]),
    );
    m.set_uniform(
        "params",
        Val::f(&[e.lights.len() as f32, 0.0, 0.0, e.window]),
    );
    for (i, (p, r, c)) in e.lights.iter().enumerate() {
        lights[i] = light(*p, *r, *c);
    }
    m.set_uniform("lights", Val::Arr(lights));
    match e.occl {
        Some(o) => {
            m.set_uniform("flags", Val::f(&[0.0, 1.0, 1.0, 0.0]));
            m.set_uniform(
                "occl_focus",
                Val::f(&[o.focus[0], o.focus[1], o.focus_z, o.strength]),
            );
            m.set_uniform("occl_params", Val::f(&occl_params(o)));
            let lift = match o.feet {
                Some(f) => {
                    let a = crate::occlusion_fade::lift_axis(f, o.focus);
                    [f[0], f[1], a[0], a[1]]
                }
                None => [0.0; 4],
            };
            m.set_uniform("occl_lift", Val::f(&lift));
        }
        None => {
            m.set_uniform("flags", Val::f(&[0.0, 1.0, 0.0, 0.0]));
            m.set_uniform("occl_focus", zero4.clone());
            m.set_uniform("occl_params", zero4.clone());
            m.set_uniform("occl_lift", zero4);
        }
    }
    let clip = e
        .clip
        .map(|(mm, lo, hi)| crate::renderer::DrawClip { m: mm, lo, hi });
    let w = crate::renderer::pack_draw_clip(clip);
    m.set_uniform("tex_window", Val::u(&[0, w[0], w[1], w[2]]));
}

/// `(radius_px, min_keep, depth_margin, feather_px)` - the staged
/// `occlusion_fade` constants both hosts send.
fn occl_params(o: Occl) -> [f32; 4] {
    use crate::occlusion_fade as of;
    [
        o.radius,
        of::OCCL_MIN_KEEP,
        of::OCCL_DEPTH_MARGIN,
        of::feather_px(o.radius),
    ]
}

/// The same layer as the play page stages it (`webgl-tmd-frame-state.js`):
/// positions in its (x, -y, z) frame, the fade's pixels in `gl_FragCoord`
/// space (rows from the bottom, so the lift axis's y negated), the clip as
/// plain floats.
fn stage_page_enhancements(m: &mut Machine<'_>, enh: Option<&Enhance>) {
    let zero4 = Val::f(&[0.0; 4]);
    let mut pr = vec![Val::f(&[0.0; 4]); 8];
    let mut col = vec![Val::f(&[0.0; 4]); 8];
    m.set_uniform("u_shadow", zero4.clone());
    let Some(e) = enh else {
        m.set_uniform("u_dyn_dir", zero4.clone());
        m.set_uniform("u_dyn_color", zero4.clone());
        m.set_uniform("u_dyn_ambient", zero4.clone());
        m.set_uniform("u_dyn_window", Val::f(&[0.0]));
        m.set_uniform("u_light_count", Val::I(vec![0]));
        m.set_uniform("u_light_pr", Val::Arr(pr));
        m.set_uniform("u_light_col", Val::Arr(col));
        m.set_uniform("u_occl_allow", Val::I(vec![0]));
        m.set_uniform("u_occl_focus", zero4.clone());
        m.set_uniform("u_occl_params", zero4.clone());
        m.set_uniform("u_occl_lift", zero4.clone());
        m.set_uniform("u_eclip_m", zero4);
        m.set_uniform("u_eclip_b", Val::f(&[0.0; 2]));
        return;
    };
    m.set_uniform(
        "u_dyn_dir",
        Val::f(&[e.key_dir[0], e.key_dir[1], e.key_dir[2], 1.0]),
    );
    m.set_uniform(
        "u_dyn_color",
        Val::f(&[e.key_rgb[0], e.key_rgb[1], e.key_rgb[2], e.pool]),
    );
    m.set_uniform(
        "u_dyn_ambient",
        Val::f(&[e.ambient[0], e.ambient[1], e.ambient[2], e.emissive]),
    );
    m.set_uniform("u_dyn_window", Val::f(&[e.window]));
    m.set_uniform("u_light_count", Val::I(vec![e.lights.len() as i32]));
    for (i, (p, r, c)) in e.lights.iter().enumerate() {
        pr[i] = Val::f(&[p[0], -p[1], p[2], *r]);
        col[i] = Val::f(&[c[0], c[1], c[2], 0.0]);
    }
    m.set_uniform("u_light_pr", Val::Arr(pr));
    m.set_uniform("u_light_col", Val::Arr(col));
    match e.occl {
        Some(o) => {
            m.set_uniform("u_occl_allow", Val::I(vec![1]));
            m.set_uniform(
                "u_occl_focus",
                Val::f(&[o.focus[0], STAGE_H - o.focus[1], o.focus_z, o.strength]),
            );
            m.set_uniform("u_occl_params", Val::f(&occl_params(o)));
            let lift = match o.feet {
                Some(f) => {
                    let fg = [f[0], STAGE_H - f[1]];
                    let cg = [o.focus[0], STAGE_H - o.focus[1]];
                    // `occlLiftAxis` (webgl-shaders.js) is
                    // `occlusion_fade::lift_axis` over the GL-space points.
                    let a = crate::occlusion_fade::lift_axis(fg, cg);
                    [fg[0], fg[1], a[0], a[1]]
                }
                None => [0.0; 4],
            };
            m.set_uniform("u_occl_lift", Val::f(&lift));
        }
        None => {
            m.set_uniform("u_occl_allow", Val::I(vec![0]));
            m.set_uniform("u_occl_focus", zero4.clone());
            m.set_uniform("u_occl_params", zero4.clone());
            m.set_uniform("u_occl_lift", zero4.clone());
        }
    }
    match e.clip {
        Some((mm, lo, hi)) => {
            m.set_uniform("u_eclip_m", Val::f(&[mm[0], mm[1], mm[2], 1.0]));
            m.set_uniform("u_eclip_b", Val::f(&[lo, hi]));
        }
        None => {
            m.set_uniform("u_eclip_m", zero4);
            m.set_uniform("u_eclip_b", Val::f(&[0.0; 2]));
        }
    }
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
    stage_page_enhancements(m, scene.enh);
    let g = &frag.geo;
    // The page frame is (x, -y, z), and GL counts rows from the bottom, so
    // its y screen gradient is the native one negated.
    let flip = |v: [f32; 3]| [v[0], -v[1], v[2]];
    m.gradients.insert(
        "v_obj_pos".into(),
        (g.dpos.0.to_vec(), g.dpos.1.map(|x| -x).to_vec()),
    );
    m.set_global("v_obj_pos", Val::f(&g.pos));
    m.set_global("v_normal", Val::f(&g.normal));
    m.set_global("v_world", Val::f(&flip(g.world)));
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
        enh: None,
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

/// The CLUT-index stride of the textured grid: every 19th of the page's
/// 256 texels in a debug build, every 7th under an optimised test profile
/// (`release-test`), where the interpreter runs fast enough for the full
/// density.
const TEXEL_STEP: usize = if cfg!(debug_assertions) { 19 } else { 7 };

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
    compare_against(frags, &page_fragment_glsl(), &scenes())
}

fn compare_against(frags: &[Fragment], page_glsl: &str, scenes: &[SceneState]) -> Tally {
    let v = std::rc::Rc::new(vram());
    // The scene pipelines (the real point-light / occlusion layer): with the
    // enhancement block zeroed they are the single-mesh pipelines exactly.
    let native_tex = parse_wgsl(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC));
    let native_flat = parse_wgsl(&compose_scene_lit_shader(COLOR_MESH_SHADER_SRC));
    let page = parse_glsl(page_glsl);
    let mut mt = Machine::new(&native_tex, fetcher(v.clone()));
    let mut mf = Machine::new(&native_flat, fetcher(v.clone()));
    let mut mp = Machine::new(&page, fetcher(v));
    let mut t = Tally {
        n: 0,
        dither_steps: 0,
        diffs: Vec::new(),
    };
    for scene in scenes {
        for frag in frags {
            let native = if frag.tex.is_some() {
                run_native(&mut mt, scene, frag)
            } else {
                run_native(&mut mf, scene, frag)
            };
            let page = run_page(&mut mp, scene, frag);
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
                        for u in (0..256u32).step_by(TEXEL_STEP) {
                            let px = ((u + k as u32) % 4, (u / 4 + d as u32) % 4);
                            frags.push(Fragment {
                                tex: Some((row << 6, TSB_8BPP | abe, u)),
                                flat_bits: 0,
                                colour: *colour,
                                px: (16 + px.0, 32 + px.1),
                                view_z: *view_z,
                                blend,
                                geo: NO_GEOMETRY,
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
                            geo: NO_GEOMETRY,
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
                    geo: NO_GEOMETRY,
                })
            })
        })
        .collect();
    let law = page_fragment_glsl();
    let modulate = "(255.0 / 128.0)";
    assert!(law.contains(modulate));
    let off_by_one = law.replacen(modulate, "(255.0 / 127.0)", 1);
    assert!(
        !compare_against(&frags, &off_by_one, &scenes())
            .diffs
            .is_empty()
    );
    let dm = "float dm[16] = float[16](-4.0, 0.0,";
    assert!(law.contains(dm), "dither matrix spelling");
    let moved = law.replacen(dm, "float dm[16] = float[16](4.0, 0.0,", 1);
    let t = compare_against(&frags, &moved, &scenes());
    assert!(!t.diffs.is_empty() || t.dither_steps > 0);
}

// ---- the enhancement layer at its enabled settings ----

/// Two point lights the enhancement grid shades under, in the retail frame.
const LIGHTS: [([f32; 3], f32, [f32; 3]); 2] = [
    ([100.0, -200.0, 300.0], 400.0, [0.8, 0.6, 0.3]),
    ([-150.0, -50.0, -100.0], 250.0, [0.2, 0.3, 0.9]),
];

/// The default-on mood: key light, screen pool, ambient floor, emissive
/// gain, window glow and two lamps.
const ENH_LIGHT: Enhance = Enhance {
    key_dir: [0.299_342_5, -0.798_246_7, 0.498_904_2],
    key_rgb: [0.9, 0.85, 0.7],
    pool: 0.35,
    ambient: [0.55, 0.55, 0.6],
    emissive: 1.15,
    window: 0.6,
    lights: &LIGHTS,
    occl: None,
    clip: None,
};

static ENH_LIGHT_S: Enhance = ENH_LIGHT;

/// The mood with the occlusion fade raised around a player at the stage
/// centre, at full strength and mid-ease, feet below the centre.
static ENH_FADE_FULL: Enhance = Enhance {
    occl: Some(Occl {
        focus: [160.0, 120.0],
        focus_z: 800.0,
        strength: 1.0,
        radius: 60.0,
        feet: Some([160.0, 165.0]),
    }),
    ..ENH_LIGHT
};
static ENH_FADE_HALF: Enhance = Enhance {
    occl: Some(Occl {
        focus: [160.0, 120.0],
        focus_z: 800.0,
        strength: 0.5,
        radius: 60.0,
        feet: None,
    }),
    ..ENH_LIGHT
};

/// The object-effect clip, along mesh Y and along a tilted axis whose terms
/// do not survive the native f16 packing exactly.
static ENH_CLIP_Y: Enhance = Enhance {
    clip: Some(([0.0, 1.0, 0.0], -50.0, 40.0)),
    ..ENH_LIGHT
};
static ENH_CLIP_TILT: Enhance = Enhance {
    clip: Some(([0.3, 0.9, 0.1], -33.3, 21.7)),
    ..ENH_LIGHT
};

fn enh_scene(enh: &'static Enhance, dither: bool) -> SceneState {
    SceneState {
        grade: [1.0, 1.0, 1.0, 0.0],
        palette: [1.0, 1.0, 1.0, 0.0],
        ramp: None,
        far: [0.0; 3],
        dither,
        enh: Some(enh),
    }
}

/// The engine's prim flag bits on `tsb` / `blend`: emissive (13) and window
/// (12).
const EMISSIVE: u32 = 0x2000;
const WINDOW: u32 = 0x1000;

const NORMALS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.580_381_9, -0.580_381_9, 0.483_651_6],
    [1.0, 0.0, 0.0],
];

const WORLDS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [120.0, -150.0, 280.0],
    [-100.0, -60.0, -80.0],
    [500.0, 0.0, 500.0],
];

/// Lit fragments of both families: every normal (none, up, tilted, side) at
/// world points inside, between and outside the two lamps, plain, emissive
/// and window prims, across the screen pool.
fn lit_fragments() -> Vec<Fragment> {
    let mut out = Vec::new();
    for (k, colour) in [[0x80, 0x80, 0x80], [0xC0, 0x60, 0x20], [0x10, 0x20, 0x60]]
        .into_iter()
        .enumerate()
    {
        for normal in NORMALS {
            for world in WORLDS {
                for flags in [0, EMISSIVE, WINDOW] {
                    let geo = Geometry {
                        world,
                        pos: world,
                        dpos: ([0.8, 0.1, 0.0], [0.0, 0.3, 0.9]),
                        normal,
                    };
                    for (i, u) in [5u32, 40, 120, 200].into_iter().enumerate() {
                        let px = [(20, 20), (160, 108), (90, 200), (300, 30)][(i + k) % 4];
                        out.push(Fragment {
                            tex: Some((CLUT_ROWS[1] << 6, TSB_8BPP | flags, u)),
                            flat_bits: 0,
                            colour,
                            px,
                            view_z: 900.0,
                            blend: None,
                            geo,
                        });
                    }
                    out.push(Fragment {
                        tex: None,
                        flat_bits: flags,
                        colour,
                        px: (160, 108),
                        view_z: 900.0,
                        blend: None,
                        geo,
                    });
                }
            }
        }
    }
    out
}

/// Dynamic light, the window glow and the point lights at their enabled
/// settings: the same fragments through both hosts.
#[test]
fn the_enhanced_lighting_shades_identically_on_both_hosts() {
    let frags = lit_fragments();
    let scenes = [
        enh_scene(&ENH_LIGHT_S, false),
        enh_scene(&ENH_LIGHT_S, true),
    ];
    report(&compare_against(&frags, &page_fragment_glsl(), &scenes));
}

/// The occlusion fade: fragments inside, on the feather of and outside the
/// fade circle, above and below the feet line, nearer and farther than the
/// player, at full and half strength - the screen-door decision must agree
/// pixel for pixel, which pins the Bayer matrix's row order too.
#[test]
fn the_occlusion_fade_discards_the_same_fragments_on_both_hosts() {
    let mut frags = Vec::new();
    for y in [60u32, 90, 105, 118, 119, 120, 121, 135, 160, 166, 175, 200] {
        for x in [100u32, 125, 140, 150, 158, 159, 160, 161, 175, 195, 230] {
            for view_z in [300.0, 790.0, 1200.0] {
                frags.push(Fragment {
                    tex: Some((CLUT_ROWS[1] << 6, TSB_8BPP, 40)),
                    flat_bits: 0,
                    colour: [0x80, 0x80, 0x80],
                    px: (x, y),
                    view_z,
                    blend: None,
                    geo: NO_GEOMETRY,
                });
            }
        }
    }
    let scenes = [
        enh_scene(&ENH_FADE_FULL, false),
        enh_scene(&ENH_FADE_HALF, false),
    ];
    report(&compare_against(&frags, &page_fragment_glsl(), &scenes));
}

/// The object-effect clip on both families: mesh points either side of and
/// on each bound, along a clean axis and a tilted one.
#[test]
fn the_effect_clip_keeps_the_same_fragments_on_both_hosts() {
    let mut frags = Vec::new();
    for y in [
        -80.0, -50.5, -50.0, -49.5, -33.0, 0.0, 21.0, 22.5, 39.5, 40.0, 40.5, 80.0,
    ] {
        for x in [-40.0, 0.0, 35.0] {
            let geo = Geometry {
                pos: [x, y, 10.0],
                ..NO_GEOMETRY
            };
            frags.push(Fragment {
                tex: Some((CLUT_ROWS[1] << 6, TSB_8BPP, 40)),
                flat_bits: 0,
                colour: [0x80, 0x80, 0x80],
                px: (40, 40),
                view_z: 900.0,
                blend: None,
                geo,
            });
            frags.push(Fragment {
                tex: None,
                flat_bits: 0,
                colour: [0x80, 0x80, 0x80],
                px: (40, 40),
                view_z: 900.0,
                blend: None,
                geo,
            });
        }
    }
    let scenes = [
        enh_scene(&ENH_CLIP_Y, false),
        enh_scene(&ENH_CLIP_TILT, false),
    ];
    report(&compare_against(&frags, &page_fragment_glsl(), &scenes));
}
