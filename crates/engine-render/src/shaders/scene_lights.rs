//! The scene point-light WGSL chunk, real and stub variants.

/// Per-scene point-light layer - the REAL variant, compiled only into the
/// multi-actor **scene** pipelines (VRAM + colour mesh), whose layouts bind
/// the lights uniform + shadow-map array at group 2. Derivation of the
/// lights lives in [`crate::scene_lights`]; the analytic (non-shadow) part
/// of `scene_point_gain` is mirrored there by `point_gain` /
/// `point_attenuation`, and the count is staged to zero whenever dynamic
/// lighting (or its shadow sub-toggle) is off, so this whole layer costs
/// one uniform read on the faithful path.
///
/// Shadowing: each light renders the scene's opaque geometry into one
/// layer of a depth array from a downward cone ([`crate::scene_lights::
/// light_view_proj`]); fragments compare through a 3x3 PCF kernel
/// (`textureSampleCompareLevel`, hardware-filtered comparisons) for soft
/// edges. Fragments outside a light's cone sample as unshadowed - they
/// still attenuate by distance.
pub(crate) const SCENE_LIGHTS_REAL_WGSL: &str = r#"
const SCENE_LIGHT_MAX: u32 = 8u;

struct ScenePointLightU {
    // xyz = world position, w = influence radius (attenuation reaches 0).
    pos_radius: vec4<f32>,
    // rgb = gain colour, w = shadows on (1) / off (0: the light still
    // shades, unshadowed - the `Y` toggle).
    color: vec4<f32>,
    // The light's shadow view-projection (downward cone, 0..1 depth).
    viewproj: mat4x4<f32>,
};

struct SceneLightsU {
    // x = active light count (0 = layer off), y = shadow-map texel size
    // (1 / dimension), z = depth-compare bias, w = the mood's window glow
    // (`LightingMood::window_word`, read by `dyn_window`).
    params: vec4<f32>,
    // Camera-occlusion fade (opt-in see-through-walls enhancement,
    // NON-RETAIL - see `crate::occlusion_fade`): .xy = the player's
    // projected framebuffer pixel, .z = the player's view-space depth,
    // .w = fade strength 0..1 (the host's eased visibility-gate output;
    // 0.0, the default, is the identity - every fragment keeps).
    occl_focus: vec4<f32>,
    // (radius_px, min_keep, depth_margin, feather_px) - the staged
    // `crate::occlusion_fade` constants (radius/feather scaled to the
    // viewport height). Only read while `occl_focus.w` is set.
    occl_params: vec4<f32>,
    // Feet-line rule: .xy = the player's projected feet (framebuffer
    // pixels), .zw = the lift axis (`crate::occlusion_fade::lift_axis`) -
    // `dot(frag - feet, axis)` is 0 on the feet line and 1 where the fade
    // reaches full strength. A zero axis switches the rule off.
    occl_lift: vec4<f32>,
    // The enhanced-lighting mood's ambient floor (rgb) + emissive gain (w)
    // (`scene_lighting::LightingMood::uniforms`'s third word). Read only
    // while the enhancement is on (`light_dir.w`).
    ambient: vec4<f32>,
    // Retail's per-primitive near reject (`prim_near_reject` in
    // engine-ui): (enable, sz_per_w, ot_shift, near_otz). Read by the
    // vertex stage; all-zero (the default) never rejects.
    prim_near: vec4<f32>,
    lights: array<ScenePointLightU, 8u>,
};

@group(2) @binding(0) var<uniform> sl: SceneLightsU;

fn prim_near_params() -> vec4<f32> {
    return sl.prim_near;
}

// The mood's (ambient rgb, emissive gain) for `dyn_light`.
fn mood_ambient() -> vec4<f32> {
    return sl.ambient;
}

// The mood's window glow for `dyn_window`.
fn mood_window() -> f32 {
    return sl.params.w;
}
@group(2) @binding(1) var t_shadow: texture_depth_2d_array;
@group(2) @binding(2) var s_shadow: sampler_comparison;

// 3x3 PCF visibility of `world_pos` from light `idx`: 0 = fully shadowed,
// 1 = fully lit. Out-of-cone / behind-the-light fragments return 1.0
// (distance attenuation still bounds them).
fn scene_light_shadow(idx: u32, world_pos: vec3<f32>) -> f32 {
    if (sl.lights[idx].color.w < 0.5) {
        return 1.0;
    }
    let clip = sl.lights[idx].viewproj * vec4<f32>(world_pos, 1.0);
    if (clip.w <= 0.0) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if (ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0
        || ndc.z <= 0.0 || ndc.z >= 1.0) {
        return 1.0;
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let depth_ref = ndc.z - sl.params.z;
    let ts = sl.params.y;
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let o = vec2<f32>(f32(dx), f32(dy)) * ts;
            sum = sum + textureSampleCompareLevel(
                t_shadow, s_shadow, uv + o, i32(idx), depth_ref);
        }
    }
    return sum / 9.0;
}

// Camera-occlusion fade keep probability for one fragment - 1.0 = keep
// unconditionally (the identity while the enable is off). A fragment
// fades only when it is nearer the camera than the player by more than
// the depth margin, within the screen-space fade circle around the
// player's projected centre, AND above the player's feet on screen; the
// keep ramps from 1.0 at the rim to
// `min_keep` at the centre. The caller discards when
// `occl_bayer(frag) >= keep` (screen-door transparency - no blend state,
// depth writes intact). CPU mirror: `crate::occlusion_fade::
// keep_probability` (kept in lockstep by that module's tests).
// `frag_w` is `@builtin(position).w` = 1/clip_w, so `1/frag_w` is the
// fragment's view-space depth (same recovery as `cue_ramp_ir0`).
fn occl_keep(frag_px: vec2<f32>, frag_w: f32) -> f32 {
    // `.w` is the host's eased fade strength (0..1): 0 = off (identity),
    // fractional values blend the geometric keep toward 1.0 so the
    // screen-door dissolves in/out with the visibility gate.
    let s = sl.occl_focus.w;
    if (s < 0.004) {
        return 1.0;
    }
    let view_z = 1.0 / max(frag_w, 1e-8);
    if (view_z >= sl.occl_focus.z - sl.occl_params.z) {
        return 1.0;
    }
    let d = distance(frag_px, sl.occl_focus.xy);
    let r = sl.occl_params.x;
    if (d >= r) {
        return 1.0;
    }
    // Feet-line rule: a fragment at or below the player's feet on screen
    // cannot be hiding them (the floor in front of the character, the foot
    // of the wall that hides them) - see `crate::occlusion_fade::lift_factor`.
    var lift = 1.0;
    if (any(sl.occl_lift.zw != vec2<f32>(0.0))) {
        lift = clamp(dot(frag_px - sl.occl_lift.xy, sl.occl_lift.zw), 0.0, 1.0);
    }
    if (lift <= 0.0) {
        return 1.0;
    }
    let t = smoothstep(r - sl.occl_params.w, r, d);
    return mix(1.0, mix(sl.occl_params.y, 1.0, t), s * lift);
}

// Summed point-light gain for one fragment (the `point_gain` argument of
// `dyn_light`). Attenuation `(1 - (d/r)^2)^2`, |N.L| orientation with the
// mixed-winding abs() the global term uses, PCF shadow per light.
fn scene_point_gain(
    world_pos: vec3<f32>,
    vert_normal: vec3<f32>,
    geo_normal: vec3<f32>,
) -> vec3<f32> {
    let n_lights = u32(sl.params.x);
    if (n_lights == 0u) {
        return vec3<f32>(0.0);
    }
    var n = vert_normal;
    if (dot(n, n) < 1e-8) {
        n = geo_normal;
    }
    let n_len = length(n);
    var gain = vec3<f32>(0.0);
    for (var i = 0u; i < SCENE_LIGHT_MAX; i = i + 1u) {
        if (i >= n_lights) {
            break;
        }
        let l = sl.lights[i];
        let to_l = l.pos_radius.xyz - world_pos;
        let dist = length(to_l);
        let radius = l.pos_radius.w;
        if (radius <= 0.0 || dist >= radius) {
            continue;
        }
        var att = clamp(1.0 - (dist * dist) / (radius * radius), 0.0, 1.0);
        att = att * att;
        // Half-Lambert wrap: a lamp grazing a wall still lifts it.
        var lam = DYN_LAMBERT_FALLBACK;
        if (n_len > 1e-6 && dist > 1e-3) {
            lam = abs(dot(n / n_len, to_l / dist)) * 0.5 + 0.5;
        }
        gain = gain + l.color.rgb * (att * lam * scene_light_shadow(i, world_pos));
    }
    return gain;
}
"#;

/// Stub twin of [`SCENE_LIGHTS_REAL_WGSL`] for the single-mesh pipelines,
/// whose layouts carry no lights group: same signature, always zero, so
/// one shader body serves both variants.
pub(crate) const SCENE_LIGHTS_STUB_WGSL: &str = r#"
fn scene_point_gain(
    world_pos: vec3<f32>,
    vert_normal: vec3<f32>,
    geo_normal: vec3<f32>,
) -> vec3<f32> {
    return vec3<f32>(0.0);
}

// Mood stub: the single-mesh pipelines carry no lights block, so they
// light under the daylight mood (`scene_lighting::LightingMood::DAY`).
fn mood_ambient() -> vec4<f32> {
    return vec4<f32>(0.7, 0.7, 0.7, 1.15);
}

// Window-glow stub: daylight - no window glows.
fn mood_window() -> f32 {
    return 0.0;
}

// Camera-occlusion fade stub: the single-mesh pipelines carry no scene
// focus (group 2 is absent from their layouts), so every fragment keeps.
fn occl_keep(frag_px: vec2<f32>, frag_w: f32) -> f32 {
    return 1.0;
}

// Per-primitive near reject stub: the single-mesh pipelines carry no scene
// block, so no primitive is ever rejected.
fn prim_near_params() -> vec4<f32> {
    return vec4<f32>(0.0);
}
"#;
