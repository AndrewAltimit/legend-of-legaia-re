/* webgl-shaders.js - WebGL2 shader sources + render constants for
 * the WASM viewer's TMD pipeline. Split out of webgl-tmd.js for
 * file modularity; consumed by webgl-tmd.js's TmdRenderer class.
 *
 * Loads as a classic global script - exposes: VRAM_W, VRAM_H,
 * FOG_LUT_SIZE, OCEAN_VS_SRC, OCEAN_FS_SRC, VS_SRC, FS_SRC.
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

out vec2 v_uv;          /* interpolated linearly across the triangle */
flat out uvec2 v_cba_tsb;
out float v_fog_t;     /* 0..1, fraction of u_fog_far_ref */
out vec4 v_flat_rgba;
out float v_view_z;    /* perspective view depth (clip w) for the depth cue */

void main() {
  vec4 world_pos = u_model * vec4(a_position, 1.0);
  v_uv = a_uv_byte;
  v_cba_tsb = a_cba_tsb;
  v_flat_rgba = a_flat_rgba;
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
  gl_Position = u_mvp * world_pos;
  v_view_z = gl_Position.w;
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
/* Per-draw occlusion-fade permission: 1 on environment draws (terrain /
 * placements / ground), 0 (the GL default) on actor draws - the player and
 * NPCs must never dissolve - and on every page that never stages a focus. */
uniform int u_occl_allow;

in vec2 v_uv;
flat in uvec2 v_cba_tsb;
in float v_fog_t;
in vec4 v_flat_rgba;
in float v_view_z;

out vec4 o_color;

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
 * A fragment fades only when it is BOTH nearer the camera than the player
 * by more than the depth margin AND inside the screen-space fade circle;
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
  float t = smoothstep(r - u_occl_params.w, r, d);
  return mix(1.0, mix(u_occl_params.y, 1.0, t), s);
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
  uint cba = v_cba_tsb.x;
  uint tsb = v_cba_tsb.y;
  /* Double-sided pair copies: draw only the copy facing the camera under
   * this view's parity (see u_pair_front). The CLUT decode below masks the
   * flag bit out ((cba >> 6) & 511 covers CBA bits 6..14 only). */
  if ((cba & 0x8000u) != 0u && gl_FrontFacing != (u_pair_front != 0)) discard;
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
     * cue runs on the packet colour and there is no texel multiply). */
    vec3 flat_lit = apply_distance_fog(v_flat_rgba.rgb);
    flat_lit = grade_near(flat_lit);
    o_color = vec4(mix(flat_lit, u_cue_far, cue_ir0(v_view_z)), 1.0);
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
  vec3 lit = clamp(color.rgb * v_flat_rgba.rgb * (255.0 / 128.0),
                   vec3(0.0), vec3(1.0));

  lit = apply_distance_fog(lit);

  /* Prologue grade + depth-cue ramp (identity when unset). Retail order:
   * the grade tints the NEAR term; the far term is texel * far colour
   * (texture detail survives the crush - DPCS runs on the packet colour
   * before the GPU texel multiply). */
  {
    float ir0 = cue_ir0(v_view_z);
    /* The far term is the far colour MODULATED by the texel, on the same
     * / 128 scale as the near term (native: psx_modulate(texel,
     * depth_cue.rgb * 255.0)) - u_cue_far is a display 0..1 colour, so it
     * needs the identical 255/128 that the packet word gets. */
    vec3 far = clamp(color.rgb * u_cue_far * (255.0 / 128.0), vec3(0.0), vec3(1.0));
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

  o_color = vec4(lit, 1.0);
}
`;
