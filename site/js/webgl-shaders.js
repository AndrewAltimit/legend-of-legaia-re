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
uniform float u_shade;            /* flat-ground Lambert shade of the main program, so the
                                   * backdrop matches the heightfield's water cells exactly
                                   * and the sea reads as one continuous layer */

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
/* Per-vertex flat/gouraud colour for the field-character hybrid render:
 * rgb in 0..1 (normalised from u8), a = 1.0 textured / 0.0 untextured.
 * Only consulted when u_use_flat_colors != 0; defaults to (1,1,1,1) when the
 * attribute isn't bound, so scene / world-map draws are unaffected. */
in vec4 a_flat_rgba;

out vec3 v_world;
out vec2 v_uv;          /* interpolated linearly across the triangle */
flat out uvec2 v_cba_tsb;
out float v_fog_t;     /* 0..1, fraction of u_fog_far_ref */
out vec4 v_flat_rgba;

void main() {
  vec4 world_pos = u_model * vec4(a_position, 1.0);
  v_world = world_pos.xyz;
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
}
`;

const FS_SRC = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;
uniform usampler2D u_fog_lut;  /* 512x1 R16UI, BGR555 entries; indexed by Z >> 5 */
uniform vec3 u_light;
/* +1.0 normally; -1.0 when the view-projection mirrors a screen axis (the
 * top-down world map is horizontally flipped to match retail). The shading
 * normal is derived from screen-space derivatives of v_world, whose cross
 * product flips sign with the VP's handedness, so this restores the correct
 * normal orientation without depending on triangle winding. */
uniform float u_normal_sign;
/* When non-zero, render transparent samples as opaque (with a tinted
 * fallback so they're visible). Used by the assembled top-view map where
 * CLUT collisions are expected and discarded fragments leave holes. */
uniform int u_no_discard;
/* When non-zero, blend the per-vertex distance-cue fog LUT into the
 * diffuse term. Mirrors the overlay leaves' dpcs/dpct post-process. */
uniform int u_fog_enable;
/* When non-zero, the field-character hybrid path is active: vertices whose
 * a_flat_rgba.a < 0.5 are untextured (flat/gouraud) prims and take their
 * colour from v_flat_rgba.rgb instead of sampling VRAM. Default 0 → all other
 * draws (scene, world map, monsters, battle/baka characters) are unchanged. */
uniform int u_use_flat_colors;
/* Per-kingdom baseline fog tint (BGR555 -> RGB linear in 0..1). Used as
 * the fog color when u_fog_lut hasn't been bound to a captured LUT yet
 * so the LUT-less path still produces a visually-meaningful gradient. */
uniform vec3 u_fog_color;

in vec3 v_world;
in vec2 v_uv;
flat in uvec2 v_cba_tsb;
in float v_fog_t;
in vec4 v_flat_rgba;

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

void main() {
  uint cba = v_cba_tsb.x;
  uint tsb = v_cba_tsb.y;
  uint u_pix = uint(v_uv.x) & 255u;
  uint v_pix = uint(v_uv.y) & 255u;

  uint tpage_x = (tsb & 15u) * 64u;
  uint tpage_y = ((tsb >> 4u) & 1u) * 256u;
  uint depth   = (tsb >> 7u) & 3u;

  /* Field-character hybrid path: an untextured (flat/gouraud) prim carries no
   * UVs, so it would sample empty VRAM and discard. Take its TMD vertex colour
   * instead, shade it, and return. Gated by u_use_flat_colors so no other draw
   * is affected. */
  if (u_use_flat_colors != 0 && v_flat_rgba.a < 0.5) {
    vec3 dxf = dFdx(v_world);
    vec3 dyf = dFdy(v_world);
    vec3 nf = normalize(cross(dxf, dyf)) * u_normal_sign;
    float lf = max(dot(nf, normalize(-u_light)), 0.0);
    o_color = vec4(v_flat_rgba.rgb * (0.45 + 0.55 * lf), 1.0);
    return;
  }

  vec4 color;
  if (depth == 0u) {
    /* 4bpp: 4 nibbles per VRAM word */
    int vx = int(tpage_x + (u_pix >> 2u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint nibble = u_pix & 3u;
    uint pal_idx = (word >> (nibble * 4u)) & 15u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    uint pal = texelFetch(u_vram, ivec2(cx, cy), 0).r;
    color = bgr555_to_rgba(pal);
  } else if (depth == 1u) {
    /* 8bpp: 2 bytes per VRAM word */
    int vx = int(tpage_x + (u_pix >> 1u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint byte_sel = u_pix & 1u;
    uint pal_idx = (word >> (byte_sel * 8u)) & 255u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    uint pal = texelFetch(u_vram, ivec2(cx, cy), 0).r;
    color = bgr555_to_rgba(pal);
  } else {
    /* 15bpp direct */
    int vx = int(tpage_x + u_pix);
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    color = bgr555_to_rgba(word);
  }

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

  vec3 dx = dFdx(v_world);
  vec3 dy = dFdy(v_world);
  vec3 n = normalize(cross(dx, dy)) * u_normal_sign;
  float lambert = max(dot(n, normalize(-u_light)), 0.0);
  float shade = 0.45 + 0.55 * lambert;
  vec3 lit = color.rgb * shade;

  if (u_fog_enable != 0) {
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
    lit = mix(lit, u_fog_color, factor);
  }

  o_color = vec4(lit, 1.0);
}
`;
