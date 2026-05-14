/* webgl-tmd.js - WebGL2 textured TMD renderer for the WASM viewer.
 *
 * Mirrors the engine-render VRAM-mesh pipeline: a 1024x512 R16UI VRAM
 * texture, per-vertex (position, uv, cba_tsb) attributes, and a fragment
 * shader that does 4bpp/8bpp/15bpp + CLUT lookup against the VRAM texture.
 *
 * Public API:
 *   const r = new TmdRenderer(canvas);
 *   r.uploadVram(vramBytes);                                   // 1MB Uint8Array
 *   r.uploadMesh(positions, uvs, cbaTsb, indices);
 *   r.render(yaw, pitch, center, radius);
 *   r.uploadSceneMesh(meshId, positions, uvs, cbaTsb, indices);
 *   r.getMeshAabb(meshId);   // null until uploadSceneMesh has run
 *   r.uploadFogLut(u16Array, params);                          // optional
 *   r.renderAssembled(placements, worldExtent, cam);
 *   r.dispose();
 *
 * Each `placement` for `renderAssembled` is:
 *   { meshId, x, z, rotY?, scale?, anchor? }
 *     scale  - per-placement world-scale (defaults to MESH_SCALE).
 *     anchor - 'origin' (default) uses the mesh's TMD-local origin as the
 *              placement pivot; 'centroid' first translates the mesh so
 *              its AABB centroid sits at (x, 0, z).
 *
 * Fog parameters (uploadFogLut(lut, { enable, zShift, color, farRef })):
 *   lut     - 512-entry Uint16Array, BGR555 entries indexed by Z >> 5.
 *   enable  - non-zero enables the fog post-process (matches retail's
 *             gp-0x2D1 & 0x10 gate).
 *   zShift  - exponent used to compute Z_far = Z >> zShift (retail gp+0x90).
 *   color   - { r, g, b } in 0..1 floats; mid-distance tint baseline.
 *   farRef  - 0..16383 reference Z for the far plane (retail gp-0x2E0).
 */

const VRAM_W = 1024;
const VRAM_H = 512;
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

out vec3 v_world;
out vec2 v_uv;          /* interpolated linearly across the triangle */
flat out uvec2 v_cba_tsb;
out float v_fog_t;     /* 0..1, fraction of u_fog_far_ref */

void main() {
  vec4 world_pos = u_model * vec4(a_position, 1.0);
  v_world = world_pos.xyz;
  v_uv = a_uv_byte;
  v_cba_tsb = a_cba_tsb;
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
/* When non-zero, render transparent samples as opaque (with a tinted
 * fallback so they're visible). Used by the assembled top-view map where
 * CLUT collisions are expected and discarded fragments leave holes. */
uniform int u_no_discard;
/* When non-zero, blend the per-vertex distance-cue fog LUT into the
 * diffuse term. Mirrors the overlay leaves' dpcs/dpct post-process. */
uniform int u_fog_enable;
/* Per-kingdom baseline fog tint (BGR555 -> RGB linear in 0..1). Used as
 * the fog color when u_fog_lut hasn't been bound to a captured LUT yet
 * so the LUT-less path still produces a visually-meaningful gradient. */
uniform vec3 u_fog_color;

in vec3 v_world;
in vec2 v_uv;
flat in uvec2 v_cba_tsb;
in float v_fog_t;

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
  vec3 n = normalize(cross(dx, dy));
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

class TmdRenderer {
  constructor(canvas) {
    const gl = canvas.getContext('webgl2', { antialias: true, alpha: false });
    if (!gl) throw new Error('WebGL2 not available');
    this.canvas = canvas;
    this.gl = gl;

    this.program = compileProgram(gl, VS_SRC, FS_SRC);
    this.locMvp     = gl.getUniformLocation(this.program, 'u_mvp');
    this.locModel   = gl.getUniformLocation(this.program, 'u_model');
    this.locVram    = gl.getUniformLocation(this.program, 'u_vram');
    this.locLight   = gl.getUniformLocation(this.program, 'u_light');
    this.locNoDisc  = gl.getUniformLocation(this.program, 'u_no_discard');
    this.locFogLut  = gl.getUniformLocation(this.program, 'u_fog_lut');
    this.locFogEnableFs = gl.getUniformLocation(this.program, 'u_fog_enable');
    this.locFogColor    = gl.getUniformLocation(this.program, 'u_fog_color');
    this.locFogOrigin   = gl.getUniformLocation(this.program, 'u_fog_origin');
    this.locFogFarRef   = gl.getUniformLocation(this.program, 'u_fog_far_ref');
    this.locFogZShift   = gl.getUniformLocation(this.program, 'u_fog_z_shift');
    this.locPos     = gl.getAttribLocation(this.program, 'a_position');
    this.locUv      = gl.getAttribLocation(this.program, 'a_uv_byte');
    this.locCbaTsb  = gl.getAttribLocation(this.program, 'a_cba_tsb');

    this.vao    = gl.createVertexArray();
    this.posBuf = gl.createBuffer();
    this.uvBuf  = gl.createBuffer();
    this.ctBuf  = gl.createBuffer();
    this.idxBuf = gl.createBuffer();
    this.tex    = gl.createTexture();
    this.fogTex = gl.createTexture();
    /* Per-mesh GL state for the assembled (multi-mesh world) path. Indexed
     * by a caller-supplied meshId (typically the kingdom pack slot). Each
     * entry holds its own VAO + buffers so we can switch meshes cheaply
     * inside a single render frame. Each entry also carries an AABB so
     * placements can opt into centroid-anchored layout instead of using
     * the TMD-local origin as the placement pivot. */
    this.sceneMeshes = new Map();

    /* Allocate the VRAM texture once (R16UI 1024x512). */
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R16UI, VRAM_W, VRAM_H);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    /* Fog LUT texture: 512x1 R16UI; entries are PSX BGR555 packets. */
    gl.bindTexture(gl.TEXTURE_2D, this.fogTex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R16UI, FOG_LUT_SIZE, 1);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    /* Seed with a neutral grey ramp (BGR555 == 0) so a fog draw before
     * the first uploadFogLut still produces u_fog_color-blended output. */
    {
      const zeros = new Uint16Array(FOG_LUT_SIZE);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      gl.texSubImage2D(
        gl.TEXTURE_2D, 0, 0, 0, FOG_LUT_SIZE, 1,
        gl.RED_INTEGER, gl.UNSIGNED_SHORT, zeros,
      );
    }
    this.fogParams = {
      enable: 0,
      zShift: 5,
      color: { r: 0.18, g: 0.20, b: 0.32 },
      farRef: 16384.0,
      origin: [0, 0, 0],
    };

    this.indexCount = 0;
  }

  uploadVram(bytes) {
    const gl = this.gl;
    /* bytes is a Uint8Array of 1024*512*2 = 1,048,576. View as Uint16Array. */
    if (bytes.byteLength !== VRAM_W * VRAM_H * 2) {
      console.warn('[webgl-tmd] unexpected VRAM size:', bytes.byteLength);
    }
    const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 2);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0,
      0, 0, VRAM_W, VRAM_H,
      gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      u16,
    );
  }

  /* Upload a 512-entry fog LUT + scalar params. `lut` is a Uint16Array of
   * BGR555 entries (matches the bytes the retail Lua probe dumps to
   * fog_probe.lut.bin). `params` keys override the cached defaults; pass
   * only the fields that changed. */
  uploadFogLut(lut, params) {
    const gl = this.gl;
    if (lut && lut.length >= FOG_LUT_SIZE) {
      gl.bindTexture(gl.TEXTURE_2D, this.fogTex);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      gl.texSubImage2D(
        gl.TEXTURE_2D, 0, 0, 0, FOG_LUT_SIZE, 1,
        gl.RED_INTEGER, gl.UNSIGNED_SHORT,
        lut.subarray(0, FOG_LUT_SIZE),
      );
    }
    if (params) {
      if (params.enable !== undefined) this.fogParams.enable = params.enable ? 1 : 0;
      if (params.zShift !== undefined) this.fogParams.zShift = +params.zShift;
      if (params.farRef !== undefined) this.fogParams.farRef = +params.farRef;
      if (params.color) {
        this.fogParams.color = {
          r: +params.color.r,
          g: +params.color.g,
          b: +params.color.b,
        };
      }
      if (params.origin) this.fogParams.origin = params.origin.slice(0, 3);
    }
  }

  /* Convenience setter for the per-frame fog origin (typically the
   * top-down camera target so silhouettes near the player fade last). */
  setFogOrigin(x, y, z) {
    this.fogParams.origin = [x, y, z];
  }

  /* Return the AABB this renderer computed for an uploaded scene mesh.
   * Returns null if the meshId has never been uploaded. */
  getMeshAabb(meshId) {
    const m = this.sceneMeshes.get(meshId);
    return m ? m.aabb : null;
  }

  uploadMesh(positions, uvs, cbaTsb, indices) {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locPos);
    gl.vertexAttribPointer(this.locPos, 3, gl.FLOAT, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.uvBuf);
    gl.bufferData(gl.ARRAY_BUFFER, uvs, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locUv);
    /* UV is u8x2 sent as float (not normalized) - values 0..255. */
    gl.vertexAttribPointer(this.locUv, 2, gl.UNSIGNED_BYTE, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.ctBuf);
    gl.bufferData(gl.ARRAY_BUFFER, cbaTsb, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locCbaTsb);
    gl.vertexAttribIPointer(this.locCbaTsb, 2, gl.UNSIGNED_SHORT, 0, 0);

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

    this.indexCount = indices.length;
    gl.bindVertexArray(null);
  }

  /* center: [cx, cy, cz]; radius: bounding-sphere half-extent;
   * distance: camera distance in unit-radius units (default 2.5);
   * panX/panY: view-space pan in unit-radius units (default 0). */
  render(yaw, pitch, distance, panX, panY, center, radius) {
    const gl = this.gl;
    const w = this.canvas.width;
    const h = this.canvas.height;
    gl.viewport(0, 0, w, h);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    /* Legaia TMDs have inconsistent winding; let the depth buffer sort it out. */
    gl.disable(gl.CULL_FACE);
    gl.clearColor(0.04, 0.05, 0.08, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    if (this.indexCount === 0) return;

    const mvp = buildMvp(yaw, pitch, distance, panX, panY, center, radius, w, h);

    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locMvp, false, mvp);
    gl.uniformMatrix4fv(this.locModel, false, IDENTITY4);
    gl.uniform3f(this.locLight, 0.5, -0.7, 0.4);  /* matches WGSL light_dir.xyz */
    gl.uniform1i(this.locNoDisc, 0);  /* per-mesh inspector: keep cutout discard */
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);

    gl.bindVertexArray(this.vao);
    gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
    gl.bindVertexArray(null);
  }

  /* ---------- Assembled / scene-mesh path ----------------------------- */

  /* Upload one TMD's geometry under a caller-supplied meshId and keep it
   * resident on the GPU. The world-overview page uses the kingdom pack slot
   * as the meshId, so repeated placements that share a slot share GPU buffers.
   *
   * Idempotent: re-upload under the same meshId overwrites. */
  uploadSceneMesh(meshId, positions, uvs, cbaTsb, indices) {
    const gl = this.gl;
    let m = this.sceneMeshes.get(meshId);
    if (!m) {
      m = {
        vao: gl.createVertexArray(),
        posBuf: gl.createBuffer(),
        uvBuf:  gl.createBuffer(),
        ctBuf:  gl.createBuffer(),
        idxBuf: gl.createBuffer(),
        indexCount: 0,
        aabb: null,
      };
      this.sceneMeshes.set(meshId, m);
    }
    m.aabb = computeAabb(positions);
    gl.bindVertexArray(m.vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, m.posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locPos);
    gl.vertexAttribPointer(this.locPos, 3, gl.FLOAT, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, m.uvBuf);
    gl.bufferData(gl.ARRAY_BUFFER, uvs, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locUv);
    gl.vertexAttribPointer(this.locUv, 2, gl.UNSIGNED_BYTE, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, m.ctBuf);
    gl.bufferData(gl.ARRAY_BUFFER, cbaTsb, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locCbaTsb);
    gl.vertexAttribIPointer(this.locCbaTsb, 2, gl.UNSIGNED_SHORT, 0, 0);

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, m.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

    m.indexCount = indices.length;
    gl.bindVertexArray(null);
  }

  hasSceneMesh(meshId) {
    return this.sceneMeshes.has(meshId);
  }

  clearScene() {
    const gl = this.gl;
    for (const m of this.sceneMeshes.values()) {
      gl.deleteVertexArray(m.vao);
      gl.deleteBuffer(m.posBuf);
      gl.deleteBuffer(m.uvBuf);
      gl.deleteBuffer(m.ctBuf);
      gl.deleteBuffer(m.idxBuf);
    }
    this.sceneMeshes.clear();
  }

  /* Render an assembled top-down scene. `placements` is an array of
   * `{ meshId, x, z, rotY? }` records (one draw call per record). `worldExtent`
   * is `[wx, wz]` (the full kingdom world size, e.g. [16320, 16320]). `cam`
   * is `{ centerX, centerZ, halfWidth, halfHeight, pitch }` - `pitch=0` is
   * a true top-down view, larger angles tilt toward the +Z horizon.
   *
   * No-ops when no scene meshes are registered. */
  renderAssembled(placements, worldExtent, cam) {
    const gl = this.gl;
    const w = this.canvas.width;
    const h = this.canvas.height;
    gl.viewport(0, 0, w, h);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.disable(gl.CULL_FACE);
    gl.clearColor(0.04, 0.05, 0.08, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    if (this.sceneMeshes.size === 0 || placements.length === 0) return;

    const vp = buildTopDownVp(w, h, worldExtent, cam);

    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locMvp, false, vp);
    gl.uniform3f(this.locLight, 0.5, -0.7, 0.4);
    gl.uniform1i(this.locNoDisc, 1);  /* assembled scene: paint silhouettes instead of discarding */
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);
    /* Fog uniforms. The cam's centre doubles as the fog origin when no
     * explicit override has been pushed - silhouettes near the camera
     * stay tinted least, matching the runtime's per-vertex pipeline. */
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.fogTex);
    gl.uniform1i(this.locFogLut, 1);
    gl.uniform1i(this.locFogEnableFs, this.fogParams.enable);
    gl.uniform3f(
      this.locFogColor,
      this.fogParams.color.r,
      this.fogParams.color.g,
      this.fogParams.color.b,
    );
    const fogOrigin = this.fogParams.origin && this.fogParams.origin.length === 3
      ? this.fogParams.origin
      : [cam.centerX, 0, cam.centerZ];
    gl.uniform3f(this.locFogOrigin, fogOrigin[0], fogOrigin[1], fogOrigin[2]);
    gl.uniform1f(this.locFogFarRef, this.fogParams.farRef);
    gl.uniform1f(this.locFogZShift, this.fogParams.zShift);

    /* Group draws by meshId so we bind each VAO once per frame. */
    const byMesh = new Map();
    for (const p of placements) {
      if (!this.sceneMeshes.has(p.meshId)) continue;
      let list = byMesh.get(p.meshId);
      if (!list) { list = []; byMesh.set(p.meshId, list); }
      list.push(p);
    }
    for (const [meshId, list] of byMesh) {
      const m = this.sceneMeshes.get(meshId);
      if (m.indexCount === 0) continue;
      gl.bindVertexArray(m.vao);
      for (const p of list) {
        const scale = (p.scale != null) ? p.scale : MESH_SCALE;
        const useCentroid = (p.anchor === 'centroid') && m.aabb;
        const model = useCentroid
          ? placementModelCentered(p.x, p.z, p.rotY || 0, scale, m.aabb)
          : placementModelScaled(p.x, p.z, p.rotY || 0, scale);
        gl.uniformMatrix4fv(this.locModel, false, model);
        gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
      }
    }
    gl.bindVertexArray(null);
  }


  dispose() {
    const gl = this.gl;
    this.clearScene();
    gl.deleteProgram(this.program);
    gl.deleteVertexArray(this.vao);
    gl.deleteBuffer(this.posBuf);
    gl.deleteBuffer(this.uvBuf);
    gl.deleteBuffer(this.ctBuf);
    gl.deleteBuffer(this.idxBuf);
    gl.deleteTexture(this.tex);
  }
}


const IDENTITY4 = new Float32Array([
  1, 0, 0, 0,
  0, 1, 0, 0,
  0, 0, 1, 0,
  0, 0, 0, 1,
]);

function compileProgram(gl, vsSrc, fsSrc) {
  const vs = gl.createShader(gl.VERTEX_SHADER);
  gl.shaderSource(vs, vsSrc);
  gl.compileShader(vs);
  if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(vs);
    throw new Error('vertex shader: ' + log);
  }
  const fs = gl.createShader(gl.FRAGMENT_SHADER);
  gl.shaderSource(fs, fsSrc);
  gl.compileShader(fs);
  if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(fs);
    throw new Error('fragment shader: ' + log);
  }
  const prog = gl.createProgram();
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    throw new Error('link: ' + gl.getProgramInfoLog(prog));
  }
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  return prog;
}

/* Multiply two 4x4 column-major matrices: out = a * b. */
function mulMat4(a, b) {
  const out = new Float32Array(16);
  for (let r = 0; r < 4; r++) {
    for (let c = 0; c < 4; c++) {
      let s = 0;
      for (let k = 0; k < 4; k++) {
        s += a[k * 4 + r] * b[c * 4 + k];
      }
      out[c * 4 + r] = s;
    }
  }
  return out;
}

/* Standard right-handed perspective. Camera at origin, looking down -Z,
 * +Y up. Visible points have negative z_view; w_clip = -z_view. */
function perspective(fov, aspect, near, far) {
  const f = 1 / Math.tan(fov / 2);
  const nf = 1 / (near - far);
  return new Float32Array([
    f / aspect, 0, 0,                       0,
    0,          f, 0,                       0,
    0,          0, (far + near) * nf,      -1,
    0,          0, 2 * far * near * nf,     0,
  ]);
}

/* Translation matrix as column-major. */
function translate(tx, ty, tz) {
  return new Float32Array([
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 1, 0,
    tx, ty, tz, 1,
  ]);
}

function rotateY(a) {
  const c = Math.cos(a), s = Math.sin(a);
  return new Float32Array([
     c, 0, -s, 0,
     0, 1,  0, 0,
     s, 0,  c, 0,
     0, 0,  0, 1,
  ]);
}

function rotateX(a) {
  const c = Math.cos(a), s = Math.sin(a);
  return new Float32Array([
    1,  0, 0, 0,
    0,  c, s, 0,
    0, -s, c, 0,
    0,  0, 0, 1,
  ]);
}

function scaleMat(s) {
  return new Float32Array([
    s, 0, 0, 0,
    0, s, 0, 0,
    0, 0, s, 0,
    0, 0, 0, 1,
  ]);
}

/* Build a model-view-projection matrix in column-major order (matching glsl
 * mat4 layout). Centers the model at origin, scales to unit radius, rotates
 * Y then X, pushes back to camera space, perspective-projects.
 *
 * PSX TMD coords have +Y pointing down, so we negate Y in the model scale
 * to bring "up" back to +Y for the rest of the pipeline. */
function buildMvp(yaw, pitch, distance, panX, panY, center, radius, viewportW, viewportH) {
  const s = 1.0 / radius;
  /* Model: translate to origin, then non-uniform scale that flips Y. */
  const T = translate(-center[0], -center[1], -center[2]);
  const S = new Float32Array([
    s,  0,  0, 0,
    0, -s,  0, 0,    /* flip Y: PSX +Y down → screen +Y up */
    0,  0,  s, 0,
    0,  0,  0, 1,
  ]);
  const M = mulMat4(S, T);

  /* View: rotate Y, then X, then push camera away by `distance` (in unit-
   * radius units; 2.5 is the default), then apply view-space pan. The pan
   * goes after the rotate so it's relative to camera orientation: the model
   * stays oriented as you orbit, and W/A/S/D pans relative to your view.
   *
   * Distance is allowed to go below 1.0 - the camera enters the bounding
   * sphere and you fly through the model. Floor at a tiny epsilon to avoid
   * matrix singularities. */
  const cameraDist = Math.max(distance, 0.001);
  const Ry = rotateY(yaw);
  const Rx = rotateX(pitch);
  const Tv = translate(panX, panY, -cameraDist);
  const V = mulMat4(Tv, mulMat4(Rx, Ry));

  /* Projection. Tiny near plane so fly-through views don't clip out the
   * model walls when the camera passes through them. */
  const aspect = viewportW / viewportH;
  const P = perspective(1.2, aspect, 0.001, 100.0);

  return mulMat4(P, mulMat4(V, M));
}

/* Per-placement model matrix for the assembled world view.
 *
 * Takes a TMD-local mesh (PSX +Y down, ~tens of units across) and places it at
 * world (x, z) with optional Y-axis rotation. Also flips Y so PSX +Y down
 * renders as +Y up (height above the ground plane), matching `buildMvp`.
 *
 * x / z are world coords (0..16320 per the Man asset). World-map TMDs are
 * tiny "flat" icons (|Z| <= ~64 in PSX units) and would render as
 * specks against the kingdom-scale frustum, so we scale them up by
 * `MESH_SCALE` to make landmarks legible. This is a viewer-side
 * presentation knob, not the retail engine's behaviour - the in-game
 * top-view debug camera is much closer to its meshes. */
const MESH_SCALE = 6.0;

/* Legacy per-placement model: scale by `sc`, rotate Y, place at (x, 0, z).
 * The mesh's TMD-local origin is the placement pivot; meshes with a
 * non-zero AABB centroid will render offset from the placement coord. */
function placementModelScaled(x, z, rotY, scale) {
  const c = Math.cos(rotY), s = Math.sin(rotY);
  const sc = scale;
  return new Float32Array([
     sc * c,    0,  sc * s, 0,
     0,       -sc,  0,      0,    /* flip Y so PSX +Y down -> world +Y up */
    -sc * s,    0,  sc * c, 0,
     x,         0,  z,      1,
  ]);
}

/* Centroid-anchored per-placement model. Pre-translates the mesh so its
 * AABB centroid sits at the placement coord before rotation + Y-flip
 * scale + world translation. The math expands `T_world * Sflip_scaled *
 * Ry * T(-centroid)`. Used for `kind: 'unplaced'` placements where the
 * TMD-local origin doesn't carry meaningful world-position information. */
function placementModelCentered(x, z, rotY, scale, aabb) {
  const c = Math.cos(rotY), s = Math.sin(rotY);
  const sc = scale;
  const cx = aabb.cx, cy = aabb.cy, cz = aabb.cz;
  return new Float32Array([
     sc * c,                            0,      sc * s,                          0,
     0,                                -sc,     0,                                0,
    -sc * s,                            0,      sc * c,                          0,
     sc * (-c * cx + s * cz) + x,       sc * cy, sc * (-s * cx - c * cz) + z,    1,
  ]);
}

/* Compute AABB + bounding-sphere radius over a Float32Array of vec3s.
 * Empty input collapses to a zero-extent box at origin. */
function computeAabb(positions) {
  if (!positions || positions.length < 3) {
    return { cx: 0, cy: 0, cz: 0, sx: 0, sy: 0, sz: 0, radius: 0 };
  }
  let minX = positions[0], maxX = positions[0];
  let minY = positions[1], maxY = positions[1];
  let minZ = positions[2], maxZ = positions[2];
  for (let i = 3; i < positions.length; i += 3) {
    const x = positions[i], y = positions[i + 1], z = positions[i + 2];
    if (x < minX) minX = x; else if (x > maxX) maxX = x;
    if (y < minY) minY = y; else if (y > maxY) maxY = y;
    if (z < minZ) minZ = z; else if (z > maxZ) maxZ = z;
  }
  const sx = maxX - minX, sy = maxY - minY, sz = maxZ - minZ;
  return {
    cx: (minX + maxX) * 0.5,
    cy: (minY + maxY) * 0.5,
    cz: (minZ + maxZ) * 0.5,
    sx, sy, sz,
    radius: Math.hypot(sx, sy, sz) * 0.5,
  };
}

/* Top-down orthographic view-projection.
 *
 * Camera looks straight down (-Y) by default; `cam.pitch` tilts it toward
 * the +Z horizon (radians, 0 = pure top-down). The visible frame is sized
 * around (centerX, centerZ) with the requested half-extents in world units,
 * letterboxed to fit the canvas aspect ratio.
 *
 * Z-up note: world coords on the ground plane are (X, Z). After Y-flip in
 * placementModel, mesh height contributes to Y, so we keep Y as the view's
 * vertical-clip axis when the camera tilts. */
function buildTopDownVp(viewportW, viewportH, worldExtent, cam) {
  const aspect = viewportW / viewportH;
  let hw = cam.halfWidth;
  let hh = cam.halfHeight;
  /* Letterbox to fit. Expand the shorter axis so nothing in the requested
   * window is clipped. */
  if (hw / hh < aspect) hw = hh * aspect; else hh = hw / aspect;
  /* Far plane needs to clear the world diagonal + mesh height with a
   * generous margin so terrain at the world's edge doesn't z-clip. */
  const wx = worldExtent[0], wz = worldExtent[1];
  const farPad = Math.max(wx, wz) * 2 + 4096;

  /* View: place camera above the center, look down (slightly tilted by
   * `pitch`). +Y is up after placementModel's flip. */
  const pitch = cam.pitch || 0;
  const eyeY = Math.max(wx, wz);  /* high enough to clear any mesh height */
  /* World-space target = (centerX, 0, centerZ). Camera position offset by
   * pitch toward -Z so a non-zero pitch tilts the view "forward". */
  const eyeX = cam.centerX;
  const eyeZ = cam.centerZ - eyeY * Math.tan(pitch);
  const target = [cam.centerX, 0, cam.centerZ];
  const up = [0, 0, -1];  /* world -Z is "up" on screen for a top-down map */
  const V = lookAt([eyeX, eyeY, eyeZ], target, up);

  const P = ortho(-hw, hw, -hh, hh, 1.0, farPad);
  return mulMat4(P, V);
}

/* Standard right-handed orthographic projection. Column-major. */
function ortho(l, r, b, t, n, f) {
  return new Float32Array([
    2 / (r - l),         0,                   0,                   0,
    0,                   2 / (t - b),         0,                   0,
    0,                   0,                  -2 / (f - n),         0,
    -(r + l) / (r - l), -(t + b) / (t - b), -(f + n) / (f - n),   1,
  ]);
}

/* Right-handed lookAt. eye / target / up are 3-element arrays. */
function lookAt(eye, target, up) {
  const fx = target[0] - eye[0], fy = target[1] - eye[1], fz = target[2] - eye[2];
  const fl = Math.hypot(fx, fy, fz) || 1;
  const f = [fx / fl, fy / fl, fz / fl];
  /* s = normalize(f x up) */
  let sx = f[1] * up[2] - f[2] * up[1];
  let sy = f[2] * up[0] - f[0] * up[2];
  let sz = f[0] * up[1] - f[1] * up[0];
  const sl = Math.hypot(sx, sy, sz) || 1;
  sx /= sl; sy /= sl; sz /= sl;
  /* u = s x f */
  const ux = sy * f[2] - sz * f[1];
  const uy = sz * f[0] - sx * f[2];
  const uz = sx * f[1] - sy * f[0];
  /* Column-major: rows of rotation are s, u, -f; translation = -R * eye. */
  return new Float32Array([
     sx,  ux, -f[0], 0,
     sy,  uy, -f[1], 0,
     sz,  uz, -f[2], 0,
    -(sx * eye[0] + sy * eye[1] + sz * eye[2]),
    -(ux * eye[0] + uy * eye[1] + uz * eye[2]),
     (f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2]),
     1,
  ]);
}

window.TmdRenderer = TmdRenderer;
