//! The WGSL prelude every PSX-shaded pipeline is composed with: overworld curvature, dither, grade,
//! palette law, modulate, depth cue and the dynamic-light model.

/// The kingdom overworld's **screen-Y curvature**, prepended to every shaded
/// 3D shader alongside [`PSX_DITHER_WGSL`]. Retail's overworld prim leaves
/// (the world-map dispatch rows `12..19` at `0x801F8968`, e.g.
/// `0x801F7770..0x801F77E4`) add `T[(SZ >> 5) + 1]` to every vertex's `SY`
/// after the `RTPT`, `T` being the table `FUN_800271A8` builds into
/// `_DAT_8007BB04` (`legaia_render_kernels::overworld_curvature`). The table is
/// a closed form of its index, so the shader evaluates it instead of
/// sampling it: entry `i >= 2` is the drop `960 * ramp[3(i - 2) / 2] / 2000`
/// saturated at `SY = 1023`, `ramp[k] = (0x2AB980 + 20 k (k + 1)) >> 18`
/// (entries `0` and `1` are `0`). `SZ` saturates at `0xFFFF`, so no index
/// past `2048` is reachable and the 32-bit accumulator never wraps.
/// `legaia_render_kernels::overworld_curvature::curvature_closed_form` is the
/// CPU twin, pinned entry for entry against the built table; the browser
/// play page's GLSL (`site/js/webgl-shaders.js`) carries the same law.
///
/// `SZ` is `clip.w` times the frame's scale
/// (`legaia_engine_field::camera_frame::frame_curve_scale`: `1` under
/// the overworld walk camera, whose matrix carries the 6x world scale, `6`
/// under a `1x` scripted shot), and the bend moves `SY` in PSX stage rows,
/// `240` to the NDC span of `2`. A scale of `0` is the identity.
pub(crate) const OVERWORLD_CURVE_WGSL: &str = r#"
fn overworld_curvature_entry(i: i32) -> i32 {
    if (i < 2) {
        return 0;
    }
    let k = (3 * (i - 2)) / 2;
    let y = (0x2AB980 + 20 * k * (k + 1)) >> 18u;
    return min((960 * y) / 2000 + 120, 1023) - 120;
}

fn overworld_curve_clip(clip: vec4<f32>, sz_scale: f32) -> vec4<f32> {
    if (sz_scale <= 0.0 || clip.w <= 0.0) {
        return clip;
    }
    let sz = clamp(i32(round(clip.w * sz_scale)), 0, 0xFFFF);
    let t = f32(overworld_curvature_entry((sz >> 5u) + 1));
    return vec4<f32>(clip.x, clip.y - t * (2.0 / 240.0) * clip.w, clip.z, clip.w);
}
"#;

/// PSX 24-bit -> 15-bit ordered-dither helper, prepended to every shaded
/// 3D shader via [`compose_psx_shader`]. Mirrors [`psx_dither`] on the CPU
/// (kept in lockstep - see that module's tests). Gated on `enable` so the
/// default (non-PSX) render path is bit-unchanged.
pub(crate) const PSX_DITHER_WGSL: &str = r#"
// PSX GPU ordered dithering: a signed 4x4 offset is added to each 8-bit
// colour component before truncation to the 5-bit-per-channel BGR555
// framebuffer. `enable < 0.5` passes the colour through unchanged.
// Reference: nocash PSX-SPX "GPU - Dithering/Color-Depth".
fn psx_dither(rgb: vec3<f32>, frag: vec2<f32>, dither_on: f32) -> vec3<f32> {
    if (dither_on < 0.5) {
        return rgb;
    }
    var dm = array<f32, 16>(
        -4.0,  0.0, -3.0,  1.0,
         2.0, -2.0,  3.0, -1.0,
        -3.0,  1.0, -4.0,  0.0,
         3.0, -1.0,  2.0, -2.0,
    );
    let xi = u32(frag.x) & 3u;
    let yi = u32(frag.y) & 3u;
    let d = dm[yi * 4u + xi];
    var outc = vec3<f32>(0.0, 0.0, 0.0);
    for (var i = 0u; i < 3u; i = i + 1u) {
        let c8 = clamp(rgb[i] * 255.0 + d, 0.0, 255.0);
        let c5 = floor(c8 / 8.0);            // truncate to 5 bits
        let e8 = c5 * 8.0 + floor(c5 / 4.0); // expand 5->8: (c5<<3)|(c5>>2)
        outc[i] = e8 / 255.0;
    }
    return outc;
}

// Snap a clip-space x/y to the nearest integer pixel of a viewport sized
// (vp_w, vp_h). Returns the snapped clip position with z/w preserved. This
// is the GTE-style "vertex jitter" that gives PSX rendering its
// characteristic shimmer on slow-moving geometry. Shared by the VRAM-mesh and
// colour-mesh vertex stages: retail snaps every vertex, textured or not.
fn psx_snap_clip(clip: vec4<f32>, vp_w: f32, vp_h: f32) -> vec4<f32> {
    // A vertex on or behind the camera plane has no screen pixel to snap
    // to: dividing by its `w` would fold it through the eye (or produce
    // NaN at `w = 0`) and move the clipper's crossing point. Left as is,
    // as the browser play page leaves it.
    if vp_w <= 0.0 || vp_h <= 0.0 || clip.w <= 0.0 {
        return clip;
    }
    // NDC after perspective divide.
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    // Pixel coords (NDC -> [0, vp]).
    let px = (ndc_x * 0.5 + 0.5) * vp_w;
    let py = (ndc_y * 0.5 + 0.5) * vp_h;
    // Snap to nearest integer pixel.
    let snapped_x = floor(px + 0.5);
    let snapped_y = floor(py + 0.5);
    // Back to NDC.
    let nx = (snapped_x / vp_w) * 2.0 - 1.0;
    let ny = (snapped_y / vp_h) * 2.0 - 1.0;
    // Re-multiply by w to rebuild clip space.
    return vec4<f32>(nx * clip.w, ny * clip.w, clip.z, clip.w);
}

// Full-scene colour grade. `grade.rgb` is a per-channel multiply tint and
// `grade.a` the strength: the shaded pixel is cross-faded toward
// `rgb * grade.rgb`. strength 0 (the identity default) passes the colour
// through unchanged.
//
// A *multiply*, not a luminance collapse, to match the retail mechanism:
// the prologue's amber is baked into the drawn colour (dim ambient + gold
// far-colour depth cue folded into per-vertex gouraud / texture-modulation
// words), while bulk backdrop textures draw at NEUTRAL 0x808080 modulation
// and keep their pre-baked warm texel chroma. A multiply preserves that
// texel chroma exactly the way retail does; a luminance collapse would
// flatten it. Drives the opening prologue (`opdeene`): green/blue crush
// toward amber while the narration text - drawn by a separate shader -
// stays white.
fn apply_grade(rgb: vec3<f32>, grade: vec4<f32>) -> vec3<f32> {
    if (grade.a <= 0.0) {
        return rgb;
    }
    return mix(rgb, rgb * grade.rgb, grade.a);
}

// Prologue palette-collapse law - the retail gold grade's ASSET half. Two
// pairs of mode-3 CLUT-cell cyclers (`FUN_80019D50`) re-upload the CLUT
// strip every frame: one pair greys the disc entry (saturation -1024, so
// L = max(r, g, b)), the other captured that grey and tints it at hue
// +0x38 / saturation +0x80. Composed, in 5-bit BGR555 space with the STP
// bit preserved:
//     L = max(r, g, b);  (L, (L * 247) >> 8, L >> 1)
// (see docs/subsystems/cutscene.md "full-scene sepia grade"). Because a
// CLUT rewrite is per palette entry and a 4/8bpp texel IS a palette entry,
// applying the law to the decoded texel word is exactly equivalent.
fn palette_law_word(w: u32) -> u32 {
    let r = w & 0x1Fu;
    let g = (w >> 5u) & 0x1Fu;
    let b = (w >> 10u) & 0x1Fu;
    let l = max(r, max(g, b));
    let g2 = (l * 247u) >> 8u;
    return (w & 0x8000u) | ((l >> 1u) << 10u) | (g2 << 5u) | l;
}

// Whether a prim's texels go through `palette_law_word`: retail's cyclers
// capture and re-upload exactly two VRAM rects, `(0, 473, 320, 2)` and
// `(0, 476, 320, 34)`, so only a 4/8bpp prim whose CLUT base sits there is
// graded - a 15-bit page has no CLUT, and rows 475, 510 and 511 and every
// column from 320 keep their disc colours.
fn palette_law_reaches(cba: u32, tsb: u32) -> bool {
    let depth = (tsb >> 7u) & 0x3u;
    let cx = (cba & 0x3Fu) * 16u;
    let cy = (cba >> 6u) & 0x1FFu;
    let row = (cy >= 473u && cy <= 474u) || (cy >= 476u && cy <= 509u);
    return depth < 2u && cx < 320u && row;
}

// The packet-colour half of the same grade: the prologue scripts' two
// `4C E6` ops, `FUN_801D8280` -> `FUN_801D5E20`, rewrite every baked colour
// word of every resident TMD through the SCUS HSV pair - first saturation
// `-0x100` (grey at `W = min(max(rgb), 0xF8)`, the `0xF8` cap of
// `FUN_8001A6C8`), then hue `+0x38` / saturation `+0x90` / value `-0x1E`.
// Composed, a word depends on its `max` alone:
//     V = max(W - 30, 0);  (V, V * 246 >> 8, V * 112 >> 8)
// (capture-pinned: every resident colour word of the retail `opdeene`
// state lies on it, `0x80` authored words at `(98, 94, 42)`). The ground
// kernel's runtime-emitted neutral `0x80,0x80,0x80` words are not TMD words
// and stay neutral. `prim` is in 0..255 colour-byte units.
fn prologue_sepia_word(m: f32) -> vec3<f32> {
    let w = min(floor(m + 0.5), 248.0);
    let v = max(w - 30.0, 0.0);
    return vec3<f32>(v, floor(v * 246.0 / 256.0), floor(v * 112.0 / 256.0));
}

fn palette_collapse_prim(prim: vec3<f32>) -> vec3<f32> {
    if (prim.r == 128.0 && prim.g == 128.0 && prim.b == 128.0) {
        return prim;
    }
    return prologue_sepia_word(max(prim.r, max(prim.g, prim.b)));
}

// PSX GPU texture blending - THE field lighting model.
//
// A textured primitive's texel is modulated by the packet colour:
//     out = texel * colour / 128
// so colour 0x80 leaves the texel exactly as-is, 0x00 blacks it out, and 0xFF
// brightens it by 255/128 ~= 2x. The colour is the TMD prim's baked colour
// word - retail's two TMD renderers issue no GTE light op at all (their only
// colour op is DPCS, the depth cue below), so all field shading is baked here
// and this multiply is the whole of it. Dropping it flattens the scene to the
// raw texel and loses BOTH tails of the contrast: across a town's env packs
// ~81% of colour components sit below 0x80 (darkening) and ~12% above
// (brightening).
//
// `colour` is the raw 0..255 byte value. Saturates at 1.0, like the GPU.
// Reference: nocash PSX-SPX "GPU - Texture Color / Blending".
fn psx_modulate(texel: vec3<f32>, colour: vec3<f32>) -> vec3<f32> {
    return clamp(texel * colour / 128.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The foreground a semi-transparent textured prim hands the GPU's blend
// equation, truncated to its 5-bit channel: the modulate
// `texel5 * colour / 128` is floored to a 5-bit value before `B + F` reads
// it. `rgb` is `texel5 / 31 * colour / 128`, so `rgb * 31` is the GPU's
// pre-truncation value. A dim additive layer loses up to one 5-bit step per
// channel to the floor, so a large faint layer (Ultra Charge's
// camera-locked discs) adds nothing where a full-precision sum would wash
// the frame. Ordered dithering, where enabled, takes the same mean: its
// offsets `-4..3` average out to the floor's loss.
// Reference: nocash PSX-SPX "GPU - Texture Color / Blending".
fn psx_blend_foreground(rgb: vec3<f32>) -> vec3<f32> {
    return floor(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 31.0 + 0.001) / 31.0;
}

// GTE depth cue (DPCS, cop2 0x780010) - the one colour op the retail TMD
// renderers run. Blends the shaded colour toward a far term by IR0:
//     out = c + (fc - c) * ir0
// `far_rgb` is the far term in 0..1, `ir0` the blend factor in 0..1 (hardware
// 0..0x1000). ir0 = 0 is the identity - the unfogged field case, and what a
// town0c retail capture shows (baked colours leave the GTE's RGB FIFO
// byte-unchanged).
//
// Retail runs DPCS on the PACKET COLOUR before the GPU's texture blend
// (`texel * colour / 128`), so on a textured prim the far term arrives at the
// pixel as `texel * far / 128` - callers pass `psx_modulate(texel, far_bytes)`
// as `far_rgb`. An untextured prim is filled with the packet colour directly,
// so its far term is the far colour itself.
fn psx_depth_cue(rgb: vec3<f32>, far_rgb: vec3<f32>, ir0: f32) -> vec3<f32> {
    if (ir0 <= 0.0) {
        return rgb;
    }
    return clamp(mix(rgb, far_rgb, ir0), vec3<f32>(0.0), vec3<f32>(1.0));
}

// View-depth-driven IR0 - the prologue's per-render-node depth-cue pull.
//
// Retail stages the DPCS far colour + IR0 PER RENDER NODE (`+0x74` / `+0x78`,
// written by the cutscene host across the opening's narration beats), so far
// scenery (sky planes, distant spires) is pulled hard toward the gold far
// colour while near ground keeps the modulation tint almost unblended. The
// engine reproduces that depth dependence with a linear view-depth ramp:
//     ir0(z) = clamp((z - near) * inv_range, 0, 1) * max_ir0
// `ramp = (near_z, inv_range, max_ir0, enable)`; `frag_w` is the fragment
// `@builtin(position).w` = 1/clip_w, so `view_z = 1/frag_w` is the view-space
// depth the vertex projected at. `enable <= 0` falls back to the constant
// `cue_a` (the fog-op path; 0 = the unfogged identity default).
fn cue_ramp_ir0(cue_a: f32, ramp: vec4<f32>, frag_w: f32) -> f32 {
    if (ramp.w <= 0.0) {
        return cue_a;
    }
    let view_z = 1.0 / max(frag_w, 1e-8);
    return clamp((view_z - ramp.x) * ramp.y, 0.0, 1.0) * ramp.z;
}

// ---- Opt-in dynamic-lighting enhancement (NON-RETAIL) ----
//
// Retail field rendering has no light source at all (see `psx_modulate`
// above - the shading is baked into the TMD colour words). This helper is a
// deliberate opt-in enhancement layered *over* the baked shading, gated on
// `light_dir.w`: when it is zero (the default) the input colour is returned
// bit-unchanged, keeping the faithful path pixel-identical.
//
// Model (the enhanced-lighting mood, `legaia_render_kernels::scene_lighting`):
//   out = baked * min(min(ambient + (diffuse*|N.L| + pool_w*pool) * key, MAX)
//                     + point_gain, TOTAL_MAX)
//   emissive prims: out = baked * emissive_gain
//   * |N.L| - a soft directional term off the smoothed per-vertex normals
//     (the TMD-derived normals retail authors but never feeds to the GTE on
//     the field path). abs() rather than max(,0) because the corpus' prim
//     winding is mixed - the accumulated normals have no canonical side -
//     and it keeps a mostly-vertical light Y-flip invariant.
//   * pool - a soft screen-space "pool of light" centred a touch above the
//     middle of the frame, falling off toward the corners: the reference
//     look's gentle vignette-of-light over the ground.
//   * the whole gain is clamped to DYN_MAX_GAIN so nothing ever exceeds
//     ~1.3x the baked brightness (no blow-out), and the result saturates.
//
// `point_gain` is the per-scene point-light layer's additive gain (candle
// / wall-light sources with shadows - `scene_point_gain`, compiled only
// into the scene pipelines); the global term caps at DYN_MAX_GAIN and the
// combined gain at DYN_TOTAL_MAX_GAIN.
//
// Tunables (kept in lockstep with the CPU mirror `crate::dyn_light`):
const DYN_DIFFUSE: f32 = 0.55;   // weight of the orientation (|N.L|) term
const DYN_MAX_GAIN: f32 = 1.3;   // gain ceiling relative to baked colour
const DYN_TOTAL_MAX_GAIN: f32 = 1.9; // ceiling with the point-light layer added
const DYN_LAMBERT_FALLBACK: f32 = 0.6; // orientation term when no normal exists
const DYN_POOL_CENTER: vec2<f32> = vec2<f32>(0.5, 0.45); // pool centre, 0..1
const DYN_POOL_INNER: f32 = 0.15; // pool full-strength radius (screen frac)
const DYN_POOL_OUTER: f32 = 0.75; // pool fade-out radius (screen frac)
// Lit windows (`scene_lighting::LIT_WINDOWS` / `shade_window`): a prim
// tagged TSB bit 12 samples a curated window art; its glass texels (blue
// clearly above red and dominant, or near-black) blend toward a warm lamp
// colour by the mood's window glow.
const WIN_GLASS_MIN_BLUE: f32 = 0.19;
const WIN_GLASS_BLACK_MAX: f32 = 0.1;
const WIN_RGB: vec3<f32> = vec3<f32>(1.0, 0.72, 0.4);
const WIN_FLOOR: f32 = 0.6;

// 4x4 Bayer threshold in [0, 1) for the camera-occlusion fade's
// screen-door discard (see `occl_keep` in the scene-lights layer). CPU
// mirror: `crate::occlusion_fade::bayer_threshold` (kept in lockstep).
// A fragment is discarded when this threshold >= its keep probability, so
// keep = 1.0 (the identity) never discards: the largest threshold is
// 15.5/16.
fn occl_bayer(frag: vec2<f32>) -> f32 {
    var bm = array<f32, 16>(
         0.0,  8.0,  2.0, 10.0,
        12.0,  4.0, 14.0,  6.0,
         3.0, 11.0,  1.0,  9.0,
        15.0,  7.0, 13.0,  5.0,
    );
    let xi = u32(frag.x) & 3u;
    let yi = u32(frag.y) & 3u;
    return (bm[yi * 4u + xi] + 0.5) / 16.0;
}

// The depth cue's far term under the enhancement: the GTE DPCS pulls a
// distant fragment toward a far colour, and a bright far colour (the outdoor
// battle grid's near-white) would otherwise ignore the mood entirely and keep
// a night stage's floor lit like noon. The far term takes the mood's flat
// gain (ambient + half the key, capped) - the light a surface with no
// orientation gets. Identity while the enhancement is off. CPU mirror:
// `scene_lighting::far_gain`.
fn dyn_far(far_rgb: vec3<f32>, light_dir: vec4<f32>, light_color: vec4<f32>) -> vec3<f32> {
    if (light_dir.w < 0.5) {
        return far_rgb;
    }
    let amb = mood_ambient();
    let g = min(amb.rgb + 0.5 * DYN_DIFFUSE * light_color.xyz, vec3<f32>(DYN_MAX_GAIN));
    return clamp(far_rgb * g, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Lit windows (NON-RETAIL, part of enhanced lighting): a window prim's
// glass texel turns toward lamp light by the mood's window glow
// (`mood_window`). `texel` is the raw decoded texel, `lit` the shaded
// colour. Identity while the enhancement is off, the glow is zero, the prim
// is not a window, or the texel is not glass. CPU mirror:
// `scene_lighting::shade_window`.
fn dyn_window(lit: vec3<f32>, texel: vec3<f32>, light_dir: vec4<f32>, window: bool) -> vec3<f32> {
    let glow = mood_window();
    if (light_dir.w < 0.5 || !window || glow <= 0.0) {
        return lit;
    }
    let blue = texel.b - texel.r >= WIN_GLASS_MIN_BLUE && texel.b >= texel.g;
    let black = max(texel.r, max(texel.g, texel.b)) <= WIN_GLASS_BLACK_MAX;
    if (!blue && !black) {
        return lit;
    }
    let pane = WIN_RGB * (WIN_FLOOR + (1.0 - WIN_FLOOR) * texel.b);
    return clamp(mix(lit, pane, clamp(glow, 0.0, 1.0)), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn dyn_light(
    rgb: vec3<f32>,
    vert_normal: vec3<f32>,
    geo_normal: vec3<f32>,
    frag_px: vec2<f32>,
    viewport: vec2<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    point_gain: vec3<f32>,
    emissive: bool,
) -> vec3<f32> {
    if (light_dir.w < 0.5) {
        return rgb;
    }
    // The mood's ambient floor (rgb) + emissive gain (w): the scene
    // pipelines read the per-frame lights block, the single-mesh pipelines
    // a daylight constant (see `mood_ambient` in the scene-lights layer).
    let amb = mood_ambient();
    // Emissive prims (lamps, glow sheets, the Genesis Tree - TSB / blend
    // bit 13, tagged by `scene_lighting::tag_emissive_*`) ignore the mood:
    // they draw at the emissive gain, plus whatever light falls on them
    // (their own light included).
    if (emissive) {
        let eg = min(vec3<f32>(amb.w) + point_gain, vec3<f32>(DYN_TOTAL_MAX_GAIN));
        return clamp(rgb * eg, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    // Orientation term. Prefer the smoothed per-vertex normal (continuous
    // shading across connected surfaces); fall back to the screen-space
    // geometric normal for zero entries (singleton normal bins, and the
    // untextured colour-mesh path which carries no normals at all).
    var lambert = DYN_LAMBERT_FALLBACK;
    var n = vert_normal;
    if (dot(n, n) < 1e-8) {
        n = geo_normal;
    }
    let n_len = length(n);
    if (n_len > 1e-6) {
        lambert = abs(dot(n / n_len, normalize(light_dir.xyz)));
    }
    // Soft screen-space light pool, weighted by the mood (`light_color.w`).
    var pool = 0.0;
    if (viewport.x > 0.0 && viewport.y > 0.0) {
        let d = distance(frag_px / viewport, DYN_POOL_CENTER);
        pool = 1.0 - smoothstep(DYN_POOL_INNER, DYN_POOL_OUTER, d);
    }
    let base = amb.rgb + (DYN_DIFFUSE * lambert + light_color.w * pool) * light_color.xyz;
    let gain = min(min(base, vec3<f32>(DYN_MAX_GAIN)) + point_gain, vec3<f32>(DYN_TOTAL_MAX_GAIN));
    return clamp(rgb * gain, vec3<f32>(0.0), vec3<f32>(1.0));
}
"#;
