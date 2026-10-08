/* webgl-shaders.js - WebGL2 shader sources + render constants for
 * the WASM viewer's TMD pipeline. Split out of webgl-tmd.js for
 * file modularity; consumed by webgl-tmd.js's TmdRenderer class.
 *
 * Loads as a classic global script - exposes: VRAM_W, VRAM_H,
 * FOG_LUT_SIZE, OCEAN_VS_SRC, OCEAN_FS_SRC, VS_SRC, FS_SRC, the DYN_*
 * dynamic-lighting constants and glslFloat.
 * Must be loaded before webgl-tmd.js.
 */

const VRAM_W = 1024;
const VRAM_H = 512;

/* Ocean tile pipeline: 4bpp indexed texture (sampled from a 256×256
 * pixel atlas) + a 16-entry CLUT that gets rewritten every animation
 * frame. This is a runtime port of the retail disc-side asset (located
 * at PROT 0085/0244/0391, slot 0 TIM_LIST, ocean TIM with image at
 * VRAM `(768, 256)` 64×256 4bpp and CLUT at `(0, 506)` 256×1). The
 * 13-frame animation table at a known signature inside slot 0 drives
 * the rolling-wave effect by cycling the first 16 CLUT entries each
 * frame.
 *
 * See `crates/web-viewer/src/ocean.rs` and
 * `docs/subsystems/world-map.md` § "Ocean / coastline source" for the
 * full RE provenance.
 *
 * The plane lives at y=0 and tiles UV across the world extent. The
 * shader does 4bpp index decode + CLUT lookup matching the PSX GPU
 * (low-nibble pixel first; CLUT entry 0 transparent; BGR555 -> linear
 * RGB). When the disc-side assets aren't loaded (no disc supplied yet)
 * we fall back to a solid royal-blue colour. */
const OCEAN_VS_SRC = `#version 300 es
precision highp float;
uniform mat4 u_mvp;
uniform vec2 u_uv_scale;   /* quad extent in texture wraps */
uniform vec2 u_uv_offset;  /* quad centre in texture wraps (world-anchors the pattern) */
in vec3 a_position;
in vec2 a_uv_world;        /* unit-quad XZ in [-0.5, 0.5] */
out vec2 v_uv;
void main() {
  /* a_uv_world matches the quad's XZ; multiply by u_uv_scale to tile
   * across the kingdom extent, then add the quad-centre offset so the
   * pattern is anchored in WORLD space - the quad itself recentres on
   * the camera target every frame, and without the offset the waves
   * would slide with the camera instead of staying put like the
   * terrain-embedded water cells. The fragment shader takes fract()
   * so UVs wrap. */
  v_uv = a_uv_world * u_uv_scale + u_uv_offset;
  gl_Position = u_mvp * vec4(a_position, 1.0);
}
`;
const OCEAN_FS_SRC = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_ocean_tex;   /* R8UI, 128×256: each texel = one byte of 4bpp data (2 pixels) */
uniform usampler2D u_ocean_clut;  /* R16UI, 16×1: 16 BGR555 entries (animated per frame) */
uniform int u_ocean_textured;     /* 0 = solid u_color fallback, 1 = textured pipeline */
uniform vec2 u_ocean_sample_size; /* (w, h) - the logical-pixel region of the texture page that holds ocean data */
uniform vec4 u_color;             /* fallback solid colour (also used where CLUT entry 0 maps to transparent) */
uniform float u_shade;            /* packet-colour modulation of the main program's ground
                                   * water cells, so the backdrop plane matches them exactly
                                   * and the sea reads as one continuous layer. 1.0 = the
                                   * neutral 0x80 word the generated heightfield carries. */

in vec2 v_uv;
out vec4 o_color;

vec3 bgr555_to_rgb(uint c) {
  return vec3(
    float((c >> 0u)  & 0x1Fu) / 31.0,
    float((c >> 5u)  & 0x1Fu) / 31.0,
    float((c >> 10u) & 0x1Fu) / 31.0
  );
}

void main() {
  if (u_ocean_textured == 0) {
    o_color = u_color;
    return;
  }
  /* Wrap UVs into [0, 1) and sample only the top-left region of the
   * texture page that actually contains ocean data. The retail TIM
   * uploads a 256×256 page but only the top-left 96×96 holds the
   * blue-ramp ocean tile; the rest is reserved for other tiles that
   * share the page in 4bpp mode. Sampling the whole page would
   * surface CLUT-entry-0 (transparent) padding in the unused regions.
   *
   * 4bpp packing: each VRAM byte holds 2 pixels, low nibble first.
   * One byte column = 2 logical pixels. */
  vec2 uv = fract(v_uv);
  int px = int(uv.x * u_ocean_sample_size.x);
  int py = int(uv.y * u_ocean_sample_size.y);
  int byte_x = px >> 1;
  int low_nib = px & 1;
  uint b = texelFetch(u_ocean_tex, ivec2(byte_x, py), 0).r;
  uint nibble = (low_nib == 0) ? (b & 0xFu) : ((b >> 4) & 0xFu);
  uint entry = texelFetch(u_ocean_clut, ivec2(int(nibble), 0), 0).r;
  /* PSX CLUT entry 0 = fully transparent. The retail world-map
   * renderer never samples this in the ocean region; if we hit it the
   * texture is mis-sized so we fall back to the kingdom tint colour. */
  if (entry == 0u) {
    o_color = u_color;
    return;
  }
  o_color = vec4(bgr555_to_rgb(entry) * u_shade, 1.0);
}
`;

/* Matches the retail fog-LUT shape: 2048 u16 entries indexed by
 * Z >> 5 (where Z is the 16-bit GTE-output Z, range 0..65535). The
 * shader samples this via `int(v_fog_t * (FOG_LUT_SIZE - 1))`. */
const FOG_LUT_SIZE = 2048;

/* Camera-occlusion fade (see-through walls) tunables - the GLSL twin of
 * the native renderer's `crates/engine-render/src/occlusion_fade.rs`
 * constants (keep the two in lockstep; the pre-commit host-drift gate
 * pairs them by name): fragments between the camera and the player
 * dissolve to a 4x4-Bayer screen-door inside a circle around the player's
 * projected centre. MIN_KEEP is the pixel-keep floor at the centre;
 * DEPTH_MARGIN is the view-depth clearance (world units) that shields the
 * player mesh, the floor at its feet and bystander NPCs.
 *
 * The radius is authored in WORLD units and projected per frame by
 * occlRadiusPx below, not held as a fraction of the framebuffer. A screen
 * fraction is zoom-invariant by construction, so it cannot serve two
 * framings: tuned at follow distance it collapses to a peephole around the
 * character's head as the camera pushes in, which is the reported defect.
 * 250 world units is about two character heights - an opening roughly four
 * characters wide. One character height is what the previous 0.12-of-height
 * tuning worked out to at the distance it was made at, and it played too
 * tight: the wall opened around the character but not around what they were
 * walking toward. */
const OCCL_RADIUS_WORLD = 250.0;
const OCCL_FEATHER_FRAC_OF_RADIUS = 0.42;
/* Clamps on the projected radius, as fractions of framebuffer height.
 * Guards against degenerate cameras (1/z diverges as the lens approaches
 * the focus, and vanishes far away), NOT tuning knobs - the upper one is
 * deliberately loose because the tightest play-page zoom already projects
 * to ~0.57 of the height, and a clamp near there would silently cap the
 * close-up hole and bring back the zoom dependence this model removes. */
const OCCL_RADIUS_MIN_FRAC = 0.04;
const OCCL_RADIUS_MAX_FRAC = 0.9;
const OCCL_MIN_KEEP = 0.25;
/* Guards only environment geometry AT the focus depth (floor tier,
 * coplanar decals) - the player / NPC draws are exempted per draw via
 * u_occl_allow, so a wall hugging the character still opens up. Larger
 * values protected such walls as if they were the player ("only the
 * nearest of several stacked occluders fades"). */
const OCCL_DEPTH_MARGIN = 16.0;

/* Fade-circle radius in framebuffer pixels: OCCL_RADIUS_WORLD projected at
 * the focus's view depth, clamped to the guard band. A world length L
 * perpendicular to the view axis at depth z spans `L * projScaleY / z` in
 * NDC, whose -1..1 covers `h` pixels - hence the halved height. `viewZ` is
 * the focus's clip w; `projScaleY` comes from occlProjScaleY (webgl-math).
 * Degenerate inputs fall back to the floor rather than to a NaN uniform.
 * Rust twin: occlusion_fade::radius_px. */
function occlRadiusPx(viewZ, projScaleY, h) {
  const lo = OCCL_RADIUS_MIN_FRAC * h;
  const hi = OCCL_RADIUS_MAX_FRAC * h;
  if (!(viewZ > 1e-3) || !(projScaleY > 0)) return lo;
  const r = OCCL_RADIUS_WORLD * projScaleY * h / (2 * viewZ);
  return Math.min(Math.max(r, lo), hi);
}

/* Feet-line rule: the fade reaches only fragments above the player's
 * projected feet. A ray from the lens through a fragment below that line
 * meets the player's depth under their feet, so it cannot be hiding them -
 * it is the floor in front of the character, or the foot of the wall that
 * hides them, and with nothing modelled under a floor tile what showed
 * through was a black band from the feet down. The keep ramps in over this
 * fraction of the feet -> body-centre screen span (0.5 = about knee height).
 * Rust twin: occlusion_fade::OCCL_LIFT_FEATHER_FRAC. */
const OCCL_LIFT_FEATHER_FRAC = 0.5;

/* The lift axis: feet -> centre screen vector, scaled so dot(frag - feet,
 * axis) reads 0 on the feet line and 1 at OCCL_LIFT_FEATHER_FRAC of the way
 * up. A degenerate span (camera straight above) returns [0, 0], which the
 * shader reads as "rule off". Rust twin: occlusion_fade::lift_axis. */
function occlLiftAxis(feetPx, centrePx) {
  const ux = centrePx[0] - feetPx[0];
  const uy = centrePx[1] - feetPx[1];
  const len2 = ux * ux + uy * uy;
  if (!(len2 >= 0.25) || !isFinite(len2)) return [0, 0];
  const k = 1 / (len2 * OCCL_LIFT_FEATHER_FRAC);
  return [ux * k, uy * k];
}

/* Enhanced-lighting enhancement (NON-RETAIL) - the page twin of the native
 * renderer's `dyn_light` + `scene_point_gain`. The shading-law constants
 * below are interpolated into FS_SRC and paired by name with their native
 * twins (crates/engine-ui/src/scene_lighting.rs) in
 * scripts/ci/check-ui-host-drift.py. The MOOD (ambient, key light, pool
 * weight, emissive gain) and the picked point lights are not constants on
 * this side at all: the engine hands them over per frame
 * (`play_lighting_frame`), the same values the native window stages. */
const DYN_DIFFUSE = 0.55;
const DYN_MAX_GAIN = 1.3;
const DYN_TOTAL_MAX_GAIN = 1.9;
const DYN_LAMBERT_FALLBACK = 0.6;
const DYN_POOL_CENTER = [0.5, 0.45];
const DYN_POOL_INNER = 0.15;
const DYN_POOL_OUTER = 0.75;
/* Lit windows (scene_lighting::LIT_WINDOWS / shade_window): a prim tagged
 * TSB bit 12 samples a curated window art; its glass texels (blue clearly
 * above red and dominant, or near-black) blend toward a warm lamp colour by
 * the mood's window glow (u_dyn_window, from the engine's lighting packet). */
const DYN_WIN_GLASS_MIN_BLUE = 0.19;
const DYN_WIN_GLASS_BLACK_MAX = 0.1;
const DYN_WIN_RGB = [1.0, 0.72, 0.4];
const DYN_WIN_FLOOR = 0.6;

/* The PSX GPU's signed 4x4 ordered-dither offsets, row-major (row = pixel
 * y & 3) - paired with engine-render's psx_dither::DITHER_MATRIX. */
const PSX_DITHER_MATRIX = [
  -4, 0, -3, 1,
  2, -2, 3, -1,
  -3, 1, -4, 0,
  3, -1, 2, -2,
];

/* A JS number as a GLSL float literal (`1` would be an int constant). */
function glslFloat(x) {
  return Number.isInteger(x) ? x.toFixed(1) : String(x);
}

/* Play-page depth precision. The engine's projection puts the near plane a
 * few units from the eye, so a scene point's normalised depth a + b / w
 * sits within ~1e-3 of 1. WebGL maps that onto [0.5, 1] of a 24-bit
 * fixed-point buffer, and the f32 depth row itself carries a few units of
 * rounding at overworld distances (the walk matrix carries the 6x world
 * scale) - together coarser than the quarter bucket that keeps an overworld
 * fog sheet behind the continent cells of its bucket, so sheets won or lost
 * per cell (polygon-shaped cut-outs in the mountains) and washed over the
 * player. The native renderer keeps the margin with reversed-Z on a float
 * buffer, which WebGL2 cannot select. So the play page's mesh program and
 * its depth-tested screen primitives write log2(w) / LOG_DEPTH_RANGE
 * instead: monotonic in w, so every comparison keeps its order, with a
 * relative step near 1e-6. A continent cell writes its bucket's
 * representative w and a screen corner the w its depth was projected from,
 * so the bucket rule compares exact values. */
const LOG_DEPTH_RANGE = 24.0;
const LOG_DEPTH_GLSL = `
float logDepthOfW(float w) {
  return clamp(log2(max(w, 1.0)) / ${glslFloat(LOG_DEPTH_RANGE)}, 0.0, 1.0);
}
`;

const VS_SRC = `#version 300 es
precision highp float;
precision highp int;

uniform mat4 u_mvp;
uniform mat4 u_model;   /* per-draw model matrix (identity for single-mesh mode) */

/* Fog (mirrors the overlay leaves at 0x801F7644..0x801F8690 - per-vertex
 * distance-cue tint added between GTE projection and OT packet write.
 * Disabled per-draw via u_fog_enable=0; see uploadFogLut.) */
uniform vec3 u_fog_origin;   /* world-space camera/eye origin (XZ floor plane) */
uniform float u_fog_far_ref; /* retail gp-0x2E0; far-plane reference Z */
uniform float u_fog_z_shift; /* retail gp+0x90; exponent for Z_far = Z >> shift */
uniform int u_fog_enable;    /* 0 = no fog; mirrors gp-0x2D1 & 0x10 gate */

/* The kingdom overworld's per-vertex screen-Y curvature - the GLSL twin of
 * engine-render's OVERWORLD_CURVE_WGSL. Retail's overworld prim leaves add
 * T[(SZ >> 5) + 1] to every vertex's SY after the RTPT (FUN_800271A8's
 * table at _DAT_8007BB04), evaluated here in closed form: entry i >= 2 is
 * 960 * ((0x2AB980 + 20 k (k + 1)) >> 18) / 2000 at k = 3 (i - 2) / 2,
 * saturated at SY = 1023 (legaia_engine_core::overworld_curvature::
 * curvature_closed_form, pinned against the table). u_curve is the frame's
 * clip.w-to-SZ factor (play_render_curve_scale -> setOverworldCurve);
 * 0.0 - every page but the play page on an overworld - is the identity. */
uniform float u_curve;

/* Retail's per-primitive near reject - the GLSL twin of engine-render's
 * PRIM_NEAR_WGSL (legaia_engine_ui::prim_near_reject). Every TMD prim
 * handler behind FUN_80043390 drops a primitive whose OTZ (AVSZ3 / AVSZ4 of
 * the corners' saturated SZ, ZSF = 0x555 / 0x400 >> the OT shift) is below
 * the scratch floor 0x1F80037E; there is no near-plane clip on that path.
 * u_prim_near = (enable, sz_per_w, ot_shift, near_otz) from the shared
 * camera_view::prim_near_cut (play_render_prim_near -> setPrimNear); all
 * zeros - the GL default, and every page but the play page - never rejects.
 * a_prim_c0.w is the primitive's corner count (0 = no single owner, never
 * rejected); unbound, the attributes read the generic default (0,0,0,1). */
uniform vec4 u_prim_near;
in vec4 a_prim_c0;
in vec3 a_prim_c1;
in vec3 a_prim_c2;
in vec3 a_prim_c3;

int primSz(mat4 m, vec3 c, float szPerW) {
  float w = m[0].w * c.x + m[1].w * c.y + m[2].w * c.z + m[3].w;
  return clamp(int(floor(w * szPerW)), 0, 0xFFFF);
}

vec2 primSxy(mat4 m, vec3 c) {
  vec4 cl = m * vec4(c, 1.0);
  float d = max(max(cl.w * u_prim_near.y, 0.0), u_prim_near.x * 0.5);
  vec2 half_ = vec2(160.0, 120.0);
  vec2 off = cl.xy * half_ * u_prim_near.y / d;
  return clamp(off + half_, vec2(-1024.0), vec2(1023.0)) - half_;
}

bool spanTooBig(vec2 a, vec2 b, vec2 c) {
  vec2 lo = min(min(a, b), c);
  vec2 hi = max(max(a, b), c);
  return hi.x - lo.x > 1023.0 || hi.y - lo.y > 511.0;
}

bool primNearRejected(mat4 m) {
  if (u_prim_near.x < 0.5 || a_prim_c0.w < 2.5) return false;
  int shift = int(u_prim_near.z);
  int sum = primSz(m, a_prim_c0.xyz, u_prim_near.y) + primSz(m, a_prim_c1, u_prim_near.y)
    + primSz(m, a_prim_c2, u_prim_near.y);
  int zsf = 0x555 >> shift;
  if (a_prim_c0.w > 3.5) {
    sum += primSz(m, a_prim_c3, u_prim_near.y);
    zsf = 0x400 >> shift;
  }
  if (((zsf * sum) >> 12) < int(u_prim_near.w)) return true;
  /* The GPU polygon-size limit (prim_near_reject::gpu_span_rejected), armed
   * when the enable lane carries the projection's H. A quad is split
   * [0,1,2] / [1,3,2]; it drops only when both halves would. */
  if (u_prim_near.x < 1.5) return false;
  vec2 s0 = primSxy(m, a_prim_c0.xyz);
  vec2 s1 = primSxy(m, a_prim_c1);
  vec2 s2 = primSxy(m, a_prim_c2);
  if (!spanTooBig(s0, s1, s2)) return false;
  if (a_prim_c0.w > 3.5) return spanTooBig(s1, primSxy(m, a_prim_c3), s2);
  return true;
}

/* PSX rasterisation (opt-in, NON-default - the GLSL twin of the native
 * renderer's psx_params, Renderer::set_psx_mode / LEGAIA_PSX_RENDER):
 * x, y = framebuffer width and height in pixels (staged on every draw, since
 * the dynamic light's screen pool reads them too), z = vertex snap on,
 * w = 15-bit dither on. All zeros - the GL default - is off, so every draw
 * that never stages it renders exactly as before. Shared by both stages. */
uniform vec4 u_psx;

/* Snap a clip-space position to the nearest integer pixel of a vp_w x vp_h
 * framebuffer - the GTE's integer screen coordinates, i.e. the "vertex
 * jitter". Twin of engine-render's psx_snap_clip; z and w are preserved. */
vec4 psxSnapClip(vec4 clip, float vp_w, float vp_h) {
  if (vp_w <= 0.0 || vp_h <= 0.0 || clip.w <= 0.0) return clip;
  float px = (clip.x / clip.w * 0.5 + 0.5) * vp_w;
  float py = (clip.y / clip.w * 0.5 + 0.5) * vp_h;
  float nx = (floor(px + 0.5) / vp_w) * 2.0 - 1.0;
  float ny = (floor(py + 0.5) / vp_h) * 2.0 - 1.0;
  return vec4(nx * clip.w, ny * clip.w, clip.z, clip.w);
}

int overworldCurveEntry(int i) {
  if (i < 2) return 0;
  int k = (3 * (i - 2)) / 2;
  int y = (0x2AB980 + 20 * k * (k + 1)) >> 18;
  return min((960 * y) / 2000 + 120, 1023) - 120;
}

vec4 overworldCurve(vec4 clip) {
  if (u_curve <= 0.0 || clip.w <= 0.0) return clip;
  int sz = clamp(int(floor(clip.w * u_curve + 0.5)), 0, 0xFFFF);
  float t = float(overworldCurveEntry((sz >> 5) + 1));
  return vec4(clip.x, clip.y - t * (2.0 / 240.0) * clip.w, clip.z, clip.w);
}

/* z = a * w + b of m's depth row over its w row, read the way
 * legaia_engine_core::overworld_draw_order::depth_affine reads it: the column
 * with the largest w weight gives a, the translation column b. */
vec2 depthAffine(mat4 m) {
  vec4 col = m[2];
  if (abs(m[0].w) > abs(col.w) && abs(m[0].w) >= abs(m[1].w)) col = m[0];
  else if (abs(m[1].w) > abs(col.w)) col = m[1];
  float a = abs(col.w) > 1.0e-7 ? col.z / col.w : 0.0;
  return vec2(a, m[3].z - a * m[3].w);
}

/* The flat bucket's representative w for a continent cell (the w whose
 * normalised depth overworldFlatDepth writes), or -1 where it keeps the
 * per-pixel depth. Same gate and same key as overworldFlatDepth. */
float overworldFlatW(mat4 m, vec4 fa, vec4 fb) {
  if (u_curve <= 0.0 || fa.z <= fa.x) return -1.0;
  float w0 = (m * vec4(fa.x, fb.x, fa.y, 1.0)).w;
  float w1 = (m * vec4(fa.z, fb.y, fa.y, 1.0)).w;
  float w2 = (m * vec4(fa.x, fb.z, fa.w, 1.0)).w;
  float w3 = (m * vec4(fa.z, fb.w, fa.w, 1.0)).w;
  int sz = clamp(int(floor(max(max(w0, w1), max(w2, w3)) * u_curve + 0.5)), 0, 0xFFFF);
  return (float((sz >> 5) + 14) * 32.0 + 32.0) / u_curve;
}

/* The field ground pass's far bucket - the GLSL twin of engine-render's
 * field_far_bucket_depth (legaia_engine_core::field_ground::flat_refs).
 * Retail links a field ground cell without the object-grid sort bit 0x8000
 * into the ordering table's fixed far bucket, so every other primitive
 * paints over it; a sloped such cell carries a swapped x pair (fa.x > fa.z).
 * Off the overworld only (u_curve 0); the FS moves the marked depth. */
bool fieldFarBucket(vec4 fa) {
  return u_curve <= 0.0 && fa.x > fa.z;
}

vec4 overworldFlatDepth(vec4 clip, mat4 m, vec4 fa, vec4 fb) {
  if (u_curve <= 0.0 || fa.z <= fa.x || clip.w <= 0.0) return clip;
  float w0 = (m * vec4(fa.x, fb.x, fa.y, 1.0)).w;
  float w1 = (m * vec4(fa.z, fb.y, fa.y, 1.0)).w;
  float w2 = (m * vec4(fa.x, fb.z, fa.w, 1.0)).w;
  float w3 = (m * vec4(fa.z, fb.w, fa.w, 1.0)).w;
  int sz = clamp(int(floor(max(max(w0, w1), max(w2, w3)) * u_curve + 0.5)), 0, 0xFFFF);
  int bucket = (sz >> 5) + 14;
  float repW = (float(bucket) * 32.0 + 32.0) / u_curve;
  vec2 ab = depthAffine(m);
  return vec4(clip.x, clip.y, (ab.x + ab.y / repW) * clip.w, clip.w);
}

/* The continent ground's depth cue - the GLSL twin of engine-render's
 * overworld_ground_cue (legaia_engine_core::overworld_ground_cue). Retail's
 * ground emitter (FUN_801F89B8) runs each cell's packet colour through DPCS
 * with IR0 = max(SZ1 - 0x5000, 0) >> 3, SZ1 the depth of the cell's corner
 * (x1, z0), toward the far colour 0x1000 its caller sets with the literal
 * SetFarColor(0x100, 0x100, 0x100). Same gate as overworldFlatDepth; one
 * value per cell, since every vertex carries the same corners. rgb in 0..1. */
vec3 overworldGroundCue(vec3 rgb, mat4 m, vec4 fa, vec4 fb) {
  if (u_curve <= 0.0 || fa.z <= fa.x) return rgb;
  float w1 = (m * vec4(fa.z, fb.y, fa.y, 1.0)).w;
  int sz1 = clamp(int(floor(w1 * u_curve + 0.5)), 0, 0xFFFF);
  int ir0 = max(sz1 - 0x5000, 0) >> 3;
  ivec3 base = ivec3(floor(rgb * 255.0 + 0.5)) << 16;
  ivec3 ir = clamp(((ivec3(0x1000) << 12) - base) >> 12, ivec3(-0x8000), ivec3(0x7FFF));
  ivec3 mac = (base + ir * ir0) >> 12;
  return vec3(clamp(mac >> 4, ivec3(0), ivec3(255))) / 255.0;
}

in vec3 a_position;
in vec2 a_uv_byte;       /* 0..255 each, sent as Uint8x2 normalised=false */
in uvec2 a_cba_tsb;
/* Per-vertex PSX **packet colour**: rgb in 0..1 (normalised from u8, so the
 * neutral modulation word 0x80 arrives as 0.502), a = 1.0 textured /
 * 0.0 untextured.
 *
 * BOTH halves consult the rgb, for two different jobs: an untextured prim is
 * FILLED with it, a textured prim MODULATES its texel by it
 * (texel * colour / 128 - the PSX GPU's texture blend, which is the whole
 * of retail's field lighting). The alpha only says which.
 *
 * A mesh that binds no stream reads the context-global attribute constant,
 * which the renderer sets to the neutral 0x80 triple, so an un-coloured draw
 * is texel * 1.0. u_use_flat_colors gates only the untextured branch. */
in vec4 a_flat_rgba;
/* The continent ground's flat bucket-depth reference - the GLSL twin of
 * engine-render's overworld_flat_depth (legaia_engine_core::
 * overworld_draw_order). Each ground vertex carries its cell's
 * [x0, z0, x1, z1] / [y00, y10, y01, y11]; retail links the cell
 * (FUN_801F89B8) at (max corner SZ >> 5) + 14 of the ordering table the fog
 * sheets link into, so on the overworld (u_curve > 0) the whole cell draws
 * at that bucket's depth. Every other mesh leaves both unbound and reads the
 * generic-attribute default (0, 0, 0, 1): x1 <= x0, per-pixel depth. */
in vec4 a_ground_ref_xz;
in vec4 a_ground_ref_y;
/* Smoothed per-vertex normal (object space) for the opt-in dynamic light -
 * the twin of the native VRAM mesh's normal stream (legaia_tmd::mesh::
 * compute_smooth_normals; JS twin computeSmoothNormals in webgl-tmd.js).
 * Bound only while dynamic lighting is on; unbound it reads the generic
 * default (0, 0, 0), which the light treats as "use the facet normal". */
in vec3 a_normal;

/* Affine (screen-linear) UV and gouraud colour, as the PSX rasteriser and
 * native's @interpolate(linear) draw them. GLSL ES 3.00 has no
 * noperspective, so each is written premultiplied by clip w alongside w
 * itself; perspective-correct interpolation of (a * w) / w is the
 * screen-linear interpolation of a. The fragment shader divides. */
out vec2 v_uv_pw;
out vec4 v_flat_rgba_pw;
out float v_affine_w;
flat out uvec2 v_cba_tsb;
out float v_fog_t;     /* 0..1, fraction of u_fog_far_ref */
out float v_view_z;    /* perspective view depth (clip w) for the depth cue */
out float v_depth_w;   /* the w the log-depth write keys on */
/* 1 on a sloped far-bucket field ground cell (fieldFarBucket), else 0. */
flat out float v_far_bucket;
out vec3 v_normal;     /* object-space smoothed normal (dynamic light only) */
out vec3 v_obj_pos;    /* object-space position: the facet-normal fallback */
out vec3 v_world;      /* page-frame world position (enhanced lighting's point lights) */

void main() {
  vec4 world_pos = u_model * vec4(a_position, 1.0);
  v_world = world_pos.xyz;
  v_cba_tsb = a_cba_tsb;
  vec4 flat_rgba = vec4(overworldGroundCue(a_flat_rgba.rgb, u_mvp * u_model,
                                        a_ground_ref_xz, a_ground_ref_y),
                     a_flat_rgba.a);
  /* Mirror the per-vertex Z_far the overlay leaves compute. The retail
   * pipeline pulls Z from the GTE's screen-space pipeline after rtpt;
   * here we approximate using XZ-plane distance to the camera origin
   * since the world-overview camera is a top-down ortho looking straight
   * down. The far-ref + shift come straight from gp-0x2E0 / gp+0x90. */
  if (u_fog_enable != 0 && u_fog_far_ref > 0.0) {
    float dx = world_pos.x - u_fog_origin.x;
    float dz = world_pos.z - u_fog_origin.z;
    float dist = sqrt(dx * dx + dz * dz);
    /* Retail does Z_far = Z >> shift. The same right-shift here in float
     * space is exp2(-shift); applied to dist before normalisation against
     * u_fog_far_ref. */
    float shifted = dist * exp2(-u_fog_z_shift);
    v_fog_t = clamp(shifted / u_fog_far_ref, 0.0, 1.0);
  } else {
    v_fog_t = 0.0;
  }
  gl_Position = overworldFlatDepth(overworldCurve(u_mvp * world_pos), u_mvp * u_model,
                                   a_ground_ref_xz, a_ground_ref_y);
  /* After the overworld bend, as native snaps after its curve: retail bends
   * SY before the packet is written. Identity while u_psx.z is 0. */
  if (u_psx.z >= 0.5) gl_Position = psxSnapClip(gl_Position, u_psx.x, u_psx.y);
  /* A rejected primitive parks every corner on one point outside the clip
   * volume: no area, nothing rasterised. */
  if (primNearRejected(u_mvp * u_model)) gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
  v_view_z = gl_Position.w;
  v_affine_w = gl_Position.w;
  v_uv_pw = a_uv_byte * gl_Position.w;
  v_flat_rgba_pw = flat_rgba * gl_Position.w;
  /* The log-depth write's w (LOG_DEPTH_GLSL): the flat bucket's
   * representative w on a continent cell, the vertex's own clip w elsewhere.
   * Perspective interpolation of w is exact. */
  float flatW = overworldFlatW(u_mvp * u_model, a_ground_ref_xz, a_ground_ref_y);
  v_depth_w = flatW > 0.0 ? flatW : gl_Position.w;
  v_far_bucket = fieldFarBucket(a_ground_ref_xz) ? 1.0 : 0.0;
  v_normal = a_normal;
  v_obj_pos = a_position;
}
`;

const FS_SRC = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;
uniform usampler2D u_fog_lut;  /* 512x1 R16UI, BGR555 entries; indexed by Z >> 5 */
/* When non-zero, render transparent samples as opaque (with a tinted
 * fallback so they're visible). Used by the assembled top-view map where
 * CLUT collisions are expected and discarded fragments leave holes. */
uniform int u_no_discard;
/* When non-zero, blend the per-vertex distance-cue fog LUT into the
 * diffuse term. Mirrors the overlay leaves' dpcs/dpct post-process. */
uniform int u_fog_enable;
/* When non-zero, the hybrid path is active: vertices whose a_flat_rgba.a < 0.5
 * are untextured (flat/gouraud) prims and are FILLED from v_flat_rgba.rgb
 * instead of sampling VRAM. Default 0 → a mesh with no untextured half never
 * takes that branch. It does NOT gate the textured path's packet-colour
 * modulation, which is unconditional (retail's law) and neutral by default. */
uniform int u_use_flat_colors;
/* PSX semi-transparency (ABE) pass selector, mirroring the retail GPU:
 *   -1 legacy       - draw everything opaque (single-mesh inspector paths
 *                     that don't run a blend pass; the pre-blend behaviour).
 *    0 opaque pass  - draw opaque fragments only; DEFER the blending ones
 *                     (discard) for the blend pass to re-draw.
 *    1 blend pass   - draw ONLY the deferred fragments (caller has GL
 *                     blending configured per ABR mode).
 * A fragment blends when its prim's ABE bit (TSB bit 15, packed by the mesh
 * builders) is set AND - for textured prims - the sampled texel's own STP
 * bit is set: STP=0 texels draw opaque even inside a semi-transparent prim.
 * Untextured (flat-colour) prims have no STP; the whole prim blends. */
uniform int u_semi_pass;
/* Per-kingdom baseline fog tint (BGR555 -> RGB linear in 0..1). Used as
 * the fog color when u_fog_lut hasn't been bound to a captured LUT yet
 * so the LUT-less path still produces a visually-meaningful gradient. */
uniform vec3 u_fog_color;
/* After-image ("ghost") pass: rgb = tint, a = intensity. When a > 0 the
 * fragment collapses to a tinted-luminance silhouette of the lit texel -
 * the caller draws it additively over the opaque pose (the retail arts
 * trail is a delayed mesh copy drawn as a PSX ABE additive prim). a == 0
 * (the GL uniform default) leaves every existing draw untouched. */
uniform vec4 u_ghost;
/* Prologue colour grade: rgb = multiply tint, a = strength (0 = identity -
 * the GL default, so every existing draw is untouched). Mirrors the engine's
 * set_color_grade (World::scene_color_grade, the opdeene/opstati/opurud
 * sepia). Applied to the NEAR term only, per the retail DPCS order. */
uniform vec4 u_grade;
/* Prologue depth-cue ramp (World::scene_depth_cue -> the native renderer's
 * set_depth_cue_ramp): x = near_z, y = far_z, z = max_ir0, w = enable
 * (0 = off, the GL default). Retail's GTE DPCS runs on the packet colour
 * BEFORE the texel multiply, so a textured prim's far term is
 * texel * far_colour and an untextured prim pulls to the far colour
 * directly - mirrored here with ir0(z) = clamp((z-near)/(far-near),0,1)
 * * max_ir0 over the perspective view depth. REF: FUN_8002735C */
uniform vec4 u_cue;
uniform vec3 u_cue_far;   /* DPCS far colour, linear 0..1 */
/* Prologue PALETTE-COLLAPSE grade: rgb = the op-4C 12 global screen tint,
 * w = enable (0 = off, the GL default, so every existing draw is untouched).
 * The twin of the native renderer's set_palette_grade
 * (crates/engine-render/src/shaders.rs palette_law_word /
 * palette_collapse_prim) - the gold grade's true altitude: retail rewrites
 * the scene's uploaded CLUT entries and TMD packet words at load, so the law
 * applies per decoded texel and per packet colour rather than as a pixel
 * multiply. While it is on, the packet words take the 4C E6 sepia rewrite
 * (prologue_sepia_word; u_grade plays no part), the screen tint is the whole-pixel
 * term, and the view-depth cue ramp is inert - every render node holds
 * IR0 = 0 across the retail prologue. */
uniform vec4 u_palette;
/* Double-sided prim pairs (CBA bit 15, set by the Rust mesh post-pass
 * legaia_tmd::mesh::mark_double_sided_pairs): two coincident copies of one
 * surface with opposite winding. Retail's NCLIP rasterises only the
 * camera-facing copy; with culling off both copies draw and z-fight. Flagged
 * fragments keep exactly one copy per view: the front-facing one when
 * u_pair_front != 0, the back-facing one otherwise. The value encodes the
 * view chain's reflection parity - buildMvp's single Y-flip projections keep
 * front (1); the assembled views add the retail screen-X mirror on top (two
 * reflections), which inverts gl_FrontFacing, so they keep back (0). */
uniform int u_pair_front;
/* Retail GTE **NCLIP** winding rejection, as a fragment test. 0 (the GL
 * default) draws both sides - the engine's rasterizer pipelines do the same,
 * because winding parity differs per render frame. 2 discards the fragments
 * whose gl_FrontFacing matches the parity that carries retail's BACK faces on
 * this page's assembled view chain - the same u_pair_front = 0 parity the
 * double-sided-pair rule above encodes, and the same predicate the native
 * renderer's set_backface_cull(2) applies. Staged for the whole field pass
 * (camera_view::nclip_cull_mode): retail culls every field mesh's back faces,
 * which hides a sky dome's outer shell and the opdeene prologue shot's near
 * cave wall. */
uniform int u_nclip_cull;
/* Camera-occlusion fade (see-through walls enhancement, NON-RETAIL - the
 * GLSL twin of the native scene shaders' occl_keep/occl_bayer, see
 * crates/engine-render/src/occlusion_fade.rs). xy = the player's projected
 * framebuffer pixel (gl_FragCoord space, origin bottom-left), z = the
 * player's view-space depth, w = fade strength 0..1 (the page's eased
 * visibility-gate output). All-zero (the GL default) is the identity - no
 * fragment ever fades. */
uniform vec4 u_occl_focus;
/* (radius_px, min_keep, depth_margin, feather_px); only read while
 * u_occl_focus.w is set. */
uniform vec4 u_occl_params;
/* Feet-line rule: xy = the player's projected feet (gl_FragCoord space),
 * zw = occlLiftAxis. A zero axis (the GL default) switches the rule off. */
uniform vec4 u_occl_lift;
/* Per-draw occlusion-fade permission: 1 on environment draws (terrain /
 * placements / ground), 0 (the GL default) on actor draws - the player and
 * NPCs must never dissolve - and on every page that never stages a focus. */
uniform int u_occl_allow;
/* PSX rasterisation word, shared with the vertex stage (see there):
 * xy = framebuffer size, w = 15-bit dither on. */
uniform vec4 u_psx;
/* Enhanced lighting (NON-RETAIL - the GLSL twin of the native renderer's
 * MeshUniforms.light_dir / light_color + the scene-lights block's ambient
 * word, all three the engine's LightingMood::uniforms): u_dyn_dir = (key
 * direction, enable), u_dyn_color = (key rgb, pool weight), u_dyn_ambient =
 * (ambient rgb, emissive gain). enable 0 - the GL default - is the identity.
 * The point lights are the engine's nearest-to-player pick, in this page's
 * world frame (x, -y, z): xyz = position, w = radius; colour rgb carries the
 * mood's lamp strength. */
uniform vec4 u_dyn_dir;
uniform vec4 u_dyn_color;
uniform vec4 u_dyn_ambient;
/* The mood's window glow (LightingMood::window_word.x); 0 - the GL default
 * - leaves every window as painted. */
uniform float u_dyn_window;
uniform int u_light_count;
uniform vec4 u_light_pr[8];
uniform vec4 u_light_col[8];
/* The point lights' shadow maps (the GLSL twin of the native scene-lights
 * group's t_shadow / s_shadow): one depth layer per picked light, rendered
 * by TmdRenderer._renderLightShadows from a downward cone, compared through
 * the hardware filter. u_light_vp[i] maps this page's world frame into
 * light i's clip space; u_shadow = (on, texel size, compare bias, -). x = 0
 * (the GL default) skips every lookup - unshadowed lights. */
uniform highp sampler2DArrayShadow u_shadow_maps;
uniform mat4 u_light_vp[8];
uniform vec4 u_shadow;

in vec2 v_uv_pw;
in vec4 v_flat_rgba_pw;
in float v_affine_w;
flat in uvec2 v_cba_tsb;
in float v_fog_t;
/* The screen-linear UV and colour, recovered from their w-premultiplied
 * varyings at the top of main() (see the vertex shader). */
vec2 v_uv;
vec4 v_flat_rgba;
in float v_view_z;
in float v_depth_w;
flat in float v_far_bucket;
in vec3 v_normal;
in vec3 v_obj_pos;
in vec3 v_world;

/* Object-effect clip of an actor whose +0x42 is raised (field-VM 4C C2):
 * retail's FUN_8002735C clips each primitive against the slab its
 * object-effect row stages (FUN_8001C204 -> 0x1F800380, FUN_80027F00).
 * u_eclip_m = (m.xyz, enable), u_eclip_b = (lo, hi): keep lo <= m . p <= hi
 * for mesh-space p. Same slab the native mesh shaders discard outside
 * (EFFECT_CLIP_WGSL), handed per draw by the engine (play_effect_clip). */
uniform vec4 u_eclip_m;
uniform vec2 u_eclip_b;

out vec4 o_color;

/* Log-depth write on (LOG_DEPTH_GLSL): 1 on the play page's perspective
 * frames, 0 (the GL default) on every other surface. */
uniform int u_log_depth_on;
${LOG_DEPTH_GLSL}

/* The fragment's pixel in the native renderer's frame (origin top-left):
 * gl_FragCoord counts from the bottom, wgpu's position builtin from the
 * top, and both the dither matrix row and the light pool's centre are
 * authored top-down. */
vec2 frag_top_px() {
  return vec2(gl_FragCoord.x, u_psx.y - gl_FragCoord.y);
}

/* PSX 24-bit -> 15-bit ordered dither (opt-in; identity while u_psx.w is 0).
 * Twin of engine-render's psx_dither: the signed 4x4 PSX matrix is added to
 * each 8-bit component before the truncation to 5 bits, then expanded back
 * as (c5 << 3) | (c5 >> 2). Retail dithers shading arithmetic only, so the
 * textured blend pass (a raw texel) never calls this. */
vec3 psx_dither(vec3 rgb) {
  if (u_psx.w < 0.5) return rgb;
  float dm[16] = float[16](${PSX_DITHER_MATRIX.map(glslFloat).join(', ')});
  vec2 f = frag_top_px();
  int xi = int(f.x) & 3;
  int yi = int(f.y) & 3;
  float d = dm[yi * 4 + xi];
  vec3 c8 = clamp(rgb * 255.0 + d, 0.0, 255.0);
  vec3 c5 = floor(c8 / 8.0);
  return (c5 * 8.0 + floor(c5 / 4.0)) / 255.0;
}

/* Enhanced-lighting tunables, from the JS constants above the shader. */
const float DYN_DIFFUSE = ${glslFloat(DYN_DIFFUSE)};
const float DYN_MAX_GAIN = ${glslFloat(DYN_MAX_GAIN)};
const float DYN_TOTAL_MAX_GAIN = ${glslFloat(DYN_TOTAL_MAX_GAIN)};
const float DYN_LAMBERT_FALLBACK = ${glslFloat(DYN_LAMBERT_FALLBACK)};
const vec2 DYN_POOL_CENTER = vec2(${glslFloat(DYN_POOL_CENTER[0])}, ${glslFloat(DYN_POOL_CENTER[1])});
const float DYN_POOL_INNER = ${glslFloat(DYN_POOL_INNER)};
const float DYN_POOL_OUTER = ${glslFloat(DYN_POOL_OUTER)};
const float DYN_WIN_GLASS_MIN_BLUE = ${glslFloat(DYN_WIN_GLASS_MIN_BLUE)};
const float DYN_WIN_GLASS_BLACK_MAX = ${glslFloat(DYN_WIN_GLASS_BLACK_MAX)};
const vec3 DYN_WIN_RGB = vec3(${DYN_WIN_RGB.map(glslFloat).join(', ')});
const float DYN_WIN_FLOOR = ${glslFloat(DYN_WIN_FLOOR)};

/* Twin of engine-render's scene_light_shadow: 3x3 PCF visibility of the
 * fragment from light i (0 = fully shadowed, 1 = lit). Out-of-cone and
 * behind-the-light fragments return 1.0 - distance attenuation still bounds
 * them. GL window depth for a GL-style projection equals the native 0..1
 * depth for the same near / far, so the compare bias carries over as is.
 * textureGrad with zero gradients: the lookup sits in non-uniform control
 * flow, and the depth layers carry one mip. */
float light_shadow(int i, vec3 wp) {
  if (u_shadow.x < 0.5) return 1.0;
  vec4 clip = u_light_vp[i] * vec4(wp, 1.0);
  if (clip.w <= 0.0) return 1.0;
  vec3 ndc = clip.xyz / clip.w;
  if (abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z <= -1.0 || ndc.z >= 1.0) return 1.0;
  vec2 uv = ndc.xy * 0.5 + 0.5;
  float ref = ndc.z * 0.5 + 0.5 - u_shadow.z;
  float sum = 0.0;
  for (int dy = -1; dy <= 1; dy++) {
    for (int dx = -1; dx <= 1; dx++) {
      vec2 o = vec2(float(dx), float(dy)) * u_shadow.y;
      sum += textureGrad(u_shadow_maps, vec4(uv + o, float(i), ref), vec2(0.0), vec2(0.0));
    }
  }
  return sum / 9.0;
}

/* Twin of engine-render's scene_point_gain: attenuation (1 - (d/r)^2)^2,
 * the half-Lambert wrap off the same normal the native shader hands it (the
 * smoothed vertex normal, else the facet normal) and the per-light shadow. */
vec3 point_gain(vec3 n) {
  vec3 g = vec3(0.0);
  float n_len = length(n);
  for (int i = 0; i < 8; i++) {
    if (i >= u_light_count) break;
    vec3 to_l = u_light_pr[i].xyz - v_world;
    float d = length(to_l);
    float r = u_light_pr[i].w;
    if (r <= 0.0 || d >= r) continue;
    float att = clamp(1.0 - (d * d) / (r * r), 0.0, 1.0);
    att = att * att;
    float lam = DYN_LAMBERT_FALLBACK;
    if (n_len > 1e-6 && d > 1e-3) lam = abs(dot(n / n_len, to_l / d)) * 0.5 + 0.5;
    g += u_light_col[i].rgb * (att * lam * light_shadow(i, v_world));
  }
  return g;
}

/* Twin of engine-render's dyn_far: the depth cue's far term under the
 * enhancement takes the mood's flat gain (ambient + half the key, capped),
 * so a bright far colour cannot keep a night frame lit. Identity off. */
vec3 dyn_far(vec3 far_rgb) {
  if (u_dyn_dir.w < 0.5) return far_rgb;
  vec3 g = min(u_dyn_ambient.rgb + 0.5 * DYN_DIFFUSE * u_dyn_color.rgb, vec3(DYN_MAX_GAIN));
  return clamp(far_rgb * g, vec3(0.0), vec3(1.0));
}

/* Twin of engine-render's dyn_light: the mood's ambient floor + a soft
 * |N.L| key light + a screen-centred pool, capped at DYN_MAX_GAIN over the
 * baked colour, plus the point lights up to DYN_TOTAL_MAX_GAIN. An emissive
 * prim (TSB / blend bit 13) skips the mood and draws at the emissive gain
 * plus the light falling on it. vn = the smoothed vertex normal (zero =
 * none), gn = the facet normal. Identity while u_dyn_dir.w is 0. */
vec3 dyn_light(vec3 rgb, vec3 vn, vec3 gn, bool emissive) {
  if (u_dyn_dir.w < 0.5) return rgb;
  vec3 n = dot(vn, vn) < 1e-8 ? gn : vn;
  vec3 pg = point_gain(n);
  if (emissive) {
    return clamp(rgb * min(vec3(u_dyn_ambient.a) + pg, vec3(DYN_TOTAL_MAX_GAIN)),
                 vec3(0.0), vec3(1.0));
  }
  float lambert = DYN_LAMBERT_FALLBACK;
  float n_len = length(n);
  if (n_len > 1e-6) lambert = abs(dot(n / n_len, normalize(u_dyn_dir.xyz)));
  float pool = 0.0;
  if (u_psx.x > 0.0 && u_psx.y > 0.0) {
    float d = distance(frag_top_px() / u_psx.xy, DYN_POOL_CENTER);
    pool = 1.0 - smoothstep(DYN_POOL_INNER, DYN_POOL_OUTER, d);
  }
  vec3 base = u_dyn_ambient.rgb + (DYN_DIFFUSE * lambert + u_dyn_color.w * pool) * u_dyn_color.rgb;
  return clamp(rgb * min(min(base, vec3(DYN_MAX_GAIN)) + pg, vec3(DYN_TOTAL_MAX_GAIN)),
               vec3(0.0), vec3(1.0));
}

/* Twin of engine-render's dyn_window: a window prim's (TSB bit 12) glass
 * texel - saturated blue, or near-black - turns toward lamp light by
 * the mood's window glow. texel = the raw decoded texel. Identity while the
 * enhancement is off or the glow is 0. */
vec3 dyn_window(vec3 lit, vec3 texel, bool window) {
  if (u_dyn_dir.w < 0.5 || !window || u_dyn_window <= 0.0) return lit;
  bool blue = texel.b - texel.r >= DYN_WIN_GLASS_MIN_BLUE && texel.b >= texel.g;
  bool black = max(texel.r, max(texel.g, texel.b)) <= DYN_WIN_GLASS_BLACK_MAX;
  if (!blue && !black) return lit;
  vec3 pane = DYN_WIN_RGB * (DYN_WIN_FLOOR + (1.0 - DYN_WIN_FLOOR) * texel.b);
  return clamp(mix(lit, pane, clamp(u_dyn_window, 0.0, 1.0)), vec3(0.0), vec3(1.0));
}

/* Decode BGR555 R/G/B in 0..1 linear. Used for VRAM texture samples. */
vec3 bgr555_to_rgb(uint c) {
  return vec3(
    float(c & 31u) / 31.0,
    float((c >> 5u) & 31u) / 31.0,
    float((c >> 10u) & 31u) / 31.0
  );
}

vec4 bgr555_to_rgba(uint c) {
  float r = float(c & 31u) / 31.0;
  float g = float((c >> 5u) & 31u) / 31.0;
  float b = float((c >> 10u) & 31u) / 31.0;
  uint stp = (c >> 15u) & 1u;
  float a = (c == 0u && stp == 0u) ? 0.0 : 1.0;
  return vec4(r, g, b, a);
}

/* Depth-cue interpolation factor for the current fragment's view depth. */
float cue_ir0(float z) {
  if (u_cue.w < 0.5) return 0.0;
  float d = max(u_cue.y - u_cue.x, 1.0);
  return clamp((z - u_cue.x) / d, 0.0, 1.0) * u_cue.z;
}

/* Prologue grade multiply on the near term (identity at strength 0). */
vec3 grade_near(vec3 c) {
  return mix(c, c * u_grade.rgb, u_grade.a);
}

/* The prologue grade's ASSET half, on the raw BGR555 texel word: retail
 * rewrote every uploaded CLUT entry from (r, g, b) to
 *     L = max(r, g, b);  (L, max(L - 1, 0), L >> 1)
 * in 5-bit space with the STP bit preserved. A 4/8bpp texel IS a palette
 * entry, so applying the law to the decoded word is exactly equivalent - and
 * it leaves 0 at 0, so the transparency test below is unaffected. Byte-equal
 * to the native shader's palette_law_word. */
uint palette_law_word(uint w) {
  uint r = w & 31u;
  uint g = (w >> 5u) & 31u;
  uint b = (w >> 10u) & 31u;
  uint l = max(r, max(g, b));
  uint g2 = max(l, 1u) - 1u;
  return (w & 0x8000u) | ((l >> 1u) << 10u) | (g2 << 5u) | l;
}

/* The packet-colour half: the prologue scripts' two 4C E6 ops
 * (FUN_801D8280 -> FUN_801D5E20) rewrite every baked colour word of every
 * resident TMD through the SCUS HSV pair, and composed the result depends on
 * the word's max alone: V = max(min(max, 0xF8) - 30, 0) ->
 * (V, V*246 >> 8, V*112 >> 8). Twin of the native prologue_sepia_word /
 * palette_collapse_prim. The ground kernel's runtime-emitted neutral
 * 0x80,0x80,0x80 words are not TMD words and stay neutral. prim is
 * normalised 0..1 here (the native twin works in 0..255 colour-byte units),
 * so neutral is 128/255. */
vec3 prologue_sepia_word(float m01) {
  float w = min(floor(m01 * 255.0 + 0.5), 248.0);
  float v = max(w - 30.0, 0.0);
  return vec3(v, floor(v * 246.0 / 256.0), floor(v * 112.0 / 256.0)) / 255.0;
}

vec3 palette_collapse_prim(vec3 prim) {
  const float NEUTRAL = 128.0 / 255.0;
  if (abs(prim.r - NEUTRAL) < (0.5 / 255.0)
      && abs(prim.g - NEUTRAL) < (0.5 / 255.0)
      && abs(prim.b - NEUTRAL) < (0.5 / 255.0)) {
    return prim;
  }
  return prologue_sepia_word(max(prim.r, max(prim.g, prim.b)));
}

/* 4x4 Bayer threshold in [0, 1) for the occlusion fade's screen-door
 * discard. keep = 1.0 never discards (largest threshold is 15.5/16). */
float occl_bayer(vec2 frag) {
  float bm[16] = float[16](
     0.0,  8.0,  2.0, 10.0,
    12.0,  4.0, 14.0,  6.0,
     3.0, 11.0,  1.0,  9.0,
    15.0,  7.0, 13.0,  5.0);
  int xi = int(frag.x) & 3;
  int yi = int(frag.y) & 3;
  return (bm[yi * 4 + xi] + 0.5) / 16.0;
}

/* Keep probability for the occlusion fade - 1.0 = keep unconditionally.
 * A fragment fades only when it is nearer the camera than the player by
 * more than the depth margin, inside the screen-space fade circle, AND
 * above the player's feet on screen;
 * the keep feathers from 1.0 at the rim to min_keep at the centre.
 * frag_w is gl_FragCoord.w = 1/clip_w, so 1/frag_w is the fragment's
 * view depth - the same recovery the native WGSL uses. */
float occl_keep(vec2 frag_px, float frag_w) {
  /* .w = the eased fade strength (0..1): 0 = off (identity), fractions
   * blend the geometric keep toward 1.0 so the screen-door dissolves in
   * and out with the visibility gate - same law as the native WGSL. */
  float s = u_occl_focus.w;
  if (s < 0.004) return 1.0;
  float view_z = 1.0 / max(frag_w, 1e-8);
  if (view_z >= u_occl_focus.z - u_occl_params.z) return 1.0;
  float d = distance(frag_px, u_occl_focus.xy);
  float r = u_occl_params.x;
  if (d >= r) return 1.0;
  /* Feet-line rule: at or below the player's feet on screen nothing can be
   * hiding them - same law as the native WGSL. */
  float lift = 1.0;
  if (u_occl_lift.z != 0.0 || u_occl_lift.w != 0.0) {
    lift = clamp(dot(frag_px - u_occl_lift.xy, u_occl_lift.zw), 0.0, 1.0);
  }
  if (lift <= 0.0) return 1.0;
  float t = smoothstep(r - u_occl_params.w, r, d);
  return mix(1.0, mix(u_occl_params.y, 1.0, t), s * lift);
}

/* World-overview distance haze, applied to BOTH prim families: retail's
 * overlay renderers run the dpcs/dpct cue pass on every prim they emit,
 * untextured F* / G* leaves included (the four untextured slots of the
 * 0x801F8968 row carry the same post-process as the textured four), so a
 * flat-filled roof hazes with distance exactly like the textured wall
 * under it. Identity when u_fog_enable is 0. */
vec3 apply_distance_fog(vec3 lit) {
  if (u_fog_enable == 0) return lit;
  /* The retail LUT at gp-0x2BC stores a per-Z SCALAR (entries climb
   * from 0x0000 at near-Z to ~0x01FF at far-Z) that the overlay
   * leaves add to vertex SXY+offset words; the per-kingdom haze
   * COLOR comes from the GTE FAR_COLOR register, set via ctc2
   * during kingdom init. The retail visual is "diffuse fades toward
   * a kingdom-tinted haze color with distance" - not a color tint
   * baked into the LUT itself.
   *
   * The WebGL approximation mirrors that split: sample the LUT as a
   * scalar fog factor in 0..1, then mix(lit, u_fog_color, factor)
   * with u_fog_color = the kingdom haze tint. When v_fog_t already
   * encodes the distance signal, the LUT shapes the per-tier
   * curve (retail samples discrete tiers at Z >> 5 boundaries). */
  float lut_idx_f = clamp(v_fog_t * 2047.0, 0.0, 2047.0);
  int lut_idx = int(lut_idx_f);
  uint lut_word = texelFetch(u_fog_lut, ivec2(lut_idx, 0), 0).r;
  /* The retail LUT saturates at 0x01FF (= 511); normalise to 0..1.
   * Without a captured LUT (the 1D texture is seeded to all zeros)
   * we fall back to v_fog_t directly so the toggle still produces
   * a distance-based fade. */
  float lut_factor = float(lut_word) / 511.0;
  float factor = (lut_word == 0u && v_fog_t > 0.0)
    ? v_fog_t
    : clamp(lut_factor, 0.0, 1.0);
  return mix(lit, u_fog_color, factor);
}

void main() {
  v_uv = v_uv_pw / v_affine_w;
  v_flat_rgba = v_flat_rgba_pw / v_affine_w;
  if (u_eclip_m.w > 0.5) {
    float ey = dot(u_eclip_m.xyz, v_obj_pos);
    if (ey < u_eclip_b.x || ey > u_eclip_b.y) discard;
  }
  float far_bucket_depth = u_log_depth_on != 0 ? logDepthOfW(v_depth_w) : gl_FragCoord.z;
  /* A sloped far-bucket field ground cell draws under everything, as
   * retail's far bucket does: its depth goes into the thin slice in front of
   * the clear value, keeping its own per-pixel order inside the slice
   * (FIELD_FAR_BUCKET_DEPTH_SCALE in engine-render - same scale, mirrored for
   * this page's forward depth). */
  if (v_far_bucket > 0.5) far_bucket_depth = 1.0 - (1.0 - far_bucket_depth) * 0.001;
  gl_FragDepth = far_bucket_depth;
  /* Facet normal for the dynamic light, taken before any discard so the
   * derivatives sit in uniform control flow. Its sign follows the
   * framebuffer's Y direction, which the light's abs() makes irrelevant. */
  vec3 geo_n = cross(dFdx(v_obj_pos), dFdy(v_obj_pos));
  uint cba = v_cba_tsb.x;
  uint tsb = v_cba_tsb.y;
  /* Double-sided pair copies: draw only the copy facing the camera under
   * this view's parity (see u_pair_front). The CLUT decode below masks the
   * flag bit out ((cba >> 6) & 511 covers CBA bits 6..14 only). */
  if ((cba & 0x8000u) != 0u && gl_FrontFacing != (u_pair_front != 0)) discard;
  /* Retail NCLIP: on this page's assembled chain the camera-facing (retail
   * front) copy is the BACK-facing one, which is exactly what u_pair_front
   * encodes - so the rejected half is gl_FrontFacing. */
  if (u_nclip_cull >= 2 && gl_FrontFacing) discard;
  /* Camera-occlusion fade: screen-door discard of fragments between the
   * camera and the player. Placed before the untextured early-return so
   * both prim families fade; identity while u_occl_focus.w is zero, and
   * actor draws (u_occl_allow == 0) never fade. */
  if (u_occl_allow != 0
      && occl_bayer(gl_FragCoord.xy) >= occl_keep(gl_FragCoord.xy, gl_FragCoord.w)) discard;
  uint u_pix = uint(v_uv.x) & 255u;
  uint v_pix = uint(v_uv.y) & 255u;

  uint tpage_x = (tsb & 15u) * 64u;
  uint tpage_y = ((tsb >> 4u) & 1u) * 256u;
  uint depth   = (tsb >> 7u) & 3u;

  /* Prim semi-transparency enable: the TMD group mode byte's ABE bit, packed
   * by the mesh builders into bit 15 of the per-vertex TSB attribute. */
  bool prim_semi = (tsb & 0x8000u) != 0u;

  /* Untextured (flat/gouraud) prim path: the prim carries no UVs, so it would
   * sample empty VRAM and discard. Take its TMD packet colour instead and
   * return. Gated by u_use_flat_colors so no other draw is affected.
   *
   * THE PACKET COLOUR IS THE SHADING. Retail fills an untextured PSX prim
   * with the colour word directly - no modulation and no light source - and
   * then runs the GTE depth cue on it, which is exactly what the native
   * window's COLOR_MESH_SHADER_SRC does ("An untextured PSX prim is filled
   * with its packet colour directly ... The colour IS the baked shading").
   * This path used to multiply by a synthetic Lambert term
   * (0.45 + 0.55 * dot(n, -light)) off the screen-space geometric normal,
   * which is a viewer aid, not retail: on a battle-stage sky dome the panels
   * sweep through every azimuth, so the term paints repeating vertical
   * lighter bands across the sky and the mountain arc that the native window
   * does not draw. Same TMD, same second-copy transform - the divergence was
   * entirely this multiply. See docs/subsystems/renderer.md. */
  if (u_use_flat_colors != 0 && v_flat_rgba.a < 0.5) {
    /* No per-texel STP for untextured prims - the whole prim defers. */
    if (u_semi_pass == 0 && prim_semi) discard;
    if (u_semi_pass == 1 && !prim_semi) discard;
    /* Untextured prims pull to the DPCS far colour directly (retail: the
     * cue runs on the packet colour and there is no texel multiply).
     *
     * In prologue palette mode the authored colour word takes the 4C E6
     * sepia rewrite first (no neutral exemption here - an untextured prim IS
     * its colour word), the screen tint is the whole-pixel term and the cue
     * ramp is inert. Byte-for-byte the native COLOR_MESH shader's arm. */
    bool flat_palette = u_palette.w > 0.5;
    vec3 flat_base = v_flat_rgba.rgb;
    if (flat_palette) {
      flat_base = prologue_sepia_word(max(flat_base.r, max(flat_base.g, flat_base.b)));
    }
    /* Native's colour-mesh format carries no normals, so its untextured
     * prims light off the facet normal alone - same here. */
    flat_base = dyn_light(flat_base, vec3(0.0), geo_n, (tsb & 0x2000u) != 0u);
    vec3 flat_lit = apply_distance_fog(flat_base);
    if (flat_palette) {
      o_color = vec4(psx_dither(flat_lit * u_palette.rgb), 1.0);
      return;
    }
    flat_lit = grade_near(flat_lit);
    /* An untextured prim's colour is shading arithmetic in both passes, so
     * it dithers in both (native COLOR_MESH fs_main and blend_pass_color). */
    o_color = vec4(psx_dither(mix(flat_lit, dyn_far(u_cue_far), cue_ir0(v_view_z))), 1.0);
    return;
  }

  uint raw;
  if (depth == 0u) {
    /* 4bpp: 4 nibbles per VRAM word */
    int vx = int(tpage_x + (u_pix >> 2u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint nibble = u_pix & 3u;
    uint pal_idx = (word >> (nibble * 4u)) & 15u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    raw = texelFetch(u_vram, ivec2(cx, cy), 0).r;
  } else if (depth == 1u) {
    /* 8bpp: 2 bytes per VRAM word */
    int vx = int(tpage_x + (u_pix >> 1u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint byte_sel = u_pix & 1u;
    uint pal_idx = (word >> (byte_sel * 8u)) & 255u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    raw = texelFetch(u_vram, ivec2(cx, cy), 0).r;
  } else {
    /* 15bpp direct */
    int vx = int(tpage_x + u_pix);
    int vy = int(tpage_y + v_pix);
    raw = texelFetch(u_vram, ivec2(vx, vy), 0).r;
  }
  /* Prologue palette-collapse grade, asset half: retail rewrites the scene's
   * uploaded CLUTs at load, so the law runs on the decoded texel word here
   * (exactly equivalent). Applied BEFORE the transparency test, which the
   * law preserves (0 -> 0, STP kept). */
  bool palette_on = u_palette.w > 0.5;
  if (palette_on) raw = palette_law_word(raw);
  vec4 color = bgr555_to_rgba(raw);
  /* PSX per-texel semi-transparency gate: inside an ABE prim, only texels
   * with the STP bit set blend; STP=0 texels stay opaque. */
  bool texel_blends = prim_semi && ((raw >> 15u) & 1u) == 1u;

  /* PSX transparency: BGR555 == 0 with STP == 0 is "fully transparent".
   * Discard so cutout textures (grates, foliage, dialog windows) don't
   * paint solid quads. Matches engine-render's WGSL fragment shader.
   *
   * Assembled-scene path can opt out: in the kingdom world-map view
   * many TMDs share CLUT rows in VRAM (~40 TMDs, ~50 TIMs), so a prim's
   * effective CLUT can be the wrong TIM's data and produce all-zero
   * samples that would discard the whole landmark. With u_no_discard
   * we fall back to a flat tint derived from the (cba, tsb) bits so
   * the geometry at least registers as a coloured silhouette. */
  if (color.a <= 0.0) {
    if (u_no_discard != 0) {
      /* Deterministic-per-prim grey-blue tint from the texture-page bits. */
      float t = float((tsb ^ cba) & 31u) / 31.0;
      color = vec4(0.25 + 0.35 * t, 0.30 + 0.30 * t, 0.45, 1.0);
    } else {
      discard;
    }
  }

  /* Two-pass semi-transparency: the opaque pass defers blending texels, the
   * blend pass draws only them (u_semi_pass -1 = legacy single pass). */
  if (u_semi_pass == 0 && texel_blends) discard;
  if (u_semi_pass == 1 && !texel_blends) discard;

  /* THE FIELD LIGHTING MODEL. Retail issues no GTE light op at all on either
   * of its TMD render paths: a textured prim is blended by the PSX GPU as
   *     out = texel * colour / 128
   * where colour is the prim's baked packet word (0x80 = the texel
   * unchanged, 0x00 blacks it out, 0xFF brightens by ~2x). The word arrives
   * as v_flat_rgba.rgb, normalised, so the byte-space / 128 is * 255/128
   * here. This is the same arithmetic as the native renderer's psx_modulate
   * (crates/engine-render/src/shaders.rs), and dropping it flattens the scene
   * to the raw texel - across a town's env packs ~81% of colour components
   * sit below 0x80 and ~12% above, so BOTH tails of the contrast go.
   *
   * What used to be here was a synthetic Lambert
   * (0.45 + 0.55 * dot(n, -u_light)) off the screen-space geometric normal of
   * v_world - a bare-geometry viewer aid, not retail, and the last of it on
   * this host. See docs/tooling/host-drift.md. */
  vec3 prim_color = palette_on ? palette_collapse_prim(v_flat_rgba.rgb)
                               : v_flat_rgba.rgb;
  vec3 lit = clamp(color.rgb * prim_color * (255.0 / 128.0),
                   vec3(0.0), vec3(1.0));
  /* Opt-in dynamic light over the baked shading (identity when off), at
   * native's point in the chain: after the modulate, before grade and cue. */
  lit = dyn_window(dyn_light(lit, v_normal, geo_n, (tsb & 0x2000u) != 0u),
                   color.rgb, (tsb & 0x1000u) != 0u);

  lit = apply_distance_fog(lit);

  /* Prologue grade + depth-cue ramp (identity when unset). Retail order:
   * the grade tints the NEAR term; the far term is texel * far colour
   * (texture detail survives the crush - DPCS runs on the packet colour
   * before the GPU texel multiply).
   *
   * Palette mode replaces the pixel multiply with the texel + packet
   * collapse above (grade_near would double-grade), carries the global
   * screen tint whole-pixel, and holds the cue ramp inert. */
  if (palette_on) {
    lit = clamp(lit * u_palette.rgb, vec3(0.0), vec3(1.0));
  } else {
    float ir0 = cue_ir0(v_view_z);
    /* The far term is the far colour MODULATED by the texel, on the same
     * / 128 scale as the near term (native: psx_modulate(texel,
     * depth_cue.rgb * 255.0)) - u_cue_far is a display 0..1 colour, so it
     * needs the identical 255/128 that the packet word gets. */
    vec3 far = dyn_far(clamp(color.rgb * u_cue_far * (255.0 / 128.0), vec3(0.0), vec3(1.0)));
    lit = mix(grade_near(lit), far, ir0);
  }

  /* Ghost (after-image) draw: keep the cutout silhouette (the discard above
   * already ran) but replace the colour with the caller's tint, weighted by
   * the lit texel's luminance so the echo keeps the pose's internal shading
   * structure. Blending is the caller's (additive, depth-read-only). */
  if (u_ghost.a > 0.0) {
    float luma = dot(lit, vec3(0.299, 0.587, 0.114));
    o_color = vec4(u_ghost.rgb * (0.35 + 0.65 * luma) * u_ghost.a, 1.0);
    return;
  }

  /* 15-bit dither on the opaque pass only: the textured blend pass's
   * foreground is a raw texel, which retail never dithers. */
  o_color = vec4(u_semi_pass == 1 ? lit : psx_dither(lit), 1.0);
}
`;

/* Enhanced lighting's glow sprites (halos + soft light shafts) - the GLSL
 * twin of engine-render's GLOW_SHADER_SRC. The quads arrive pre-expanded
 * from the engine (`scene_lighting::glow_vertices`) in the retail Y-down
 * frame, so u_mvp carries this page's Y flip. Drawn additively with the
 * depth test on and depth writes off; the kind rides colour.w (0 = halo,
 * 1 = shaft). */
const GLOW_VS_SRC = `#version 300 es
precision highp float;
uniform mat4 u_mvp;
in vec3 a_position;
in vec2 a_uv;
in vec4 a_color;
out vec2 v_uv;
out vec4 v_color;
out float v_depth_w;
void main() {
  gl_Position = u_mvp * vec4(a_position, 1.0);
  v_uv = a_uv;
  v_color = a_color;
  v_depth_w = gl_Position.w;
}
`;

const GLOW_FS_SRC = `#version 300 es
precision highp float;
in vec2 v_uv;
in vec4 v_color;
in float v_depth_w;
out vec4 o_color;
/* The glow is depth-tested against the scene, so it must compare in the
 * scene's depth space: the play page's mesh program writes log2(w)
 * (LOG_DEPTH_GLSL) on its perspective frames, and a halo that wrote the
 * rasterised gl_FragCoord.z (~0.99 at field distances) against a log
 * buffer (~0.4) failed LEQUAL almost everywhere - the halos vanished. */
uniform int u_log_depth_on;
${LOG_DEPTH_GLSL}
void main() {
  gl_FragDepth = u_log_depth_on != 0 ? logDepthOfW(v_depth_w) : gl_FragCoord.z;
  float f;
  if (v_color.w > 0.5) {
    float x = clamp(1.0 - v_uv.x * v_uv.x, 0.0, 1.0);
    f = x * x * clamp(1.0 - v_uv.y, 0.0, 1.0);
  } else {
    float x = clamp(1.0 - dot(v_uv, v_uv), 0.0, 1.0);
    f = x * x;
  }
  o_color = vec4(v_color.rgb * f, 1.0);
}
`;

/* Depth-only shadow pass for the enhanced-lighting point lights - the GLSL
 * twin of engine-render's SHADOW_MESH_SHADER_SRC. Position only, bound at the
 * main program's a_position location so every mesh VAO draws through it
 * unchanged; u_mvp = light view-projection * the draw's model matrix. The
 * fragment stage writes nothing: the target is the light's own depth layer,
 * never the scene's buffer, and cutout texels shadow as solid (the native
 * pass's accepted approximation too). */
const SHADOW_VS_SRC = `#version 300 es
precision highp float;
uniform mat4 u_mvp;
in vec3 a_position;
void main() {
  gl_Position = u_mvp * vec4(a_position, 1.0);
}
`;

const SHADOW_FS_SRC = `#version 300 es
precision highp float;
void main() {}
`;
