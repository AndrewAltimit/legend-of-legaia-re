/* Volumetric ground fog on the play page - the browser twin of the native
 * renderer's `fog_volume` pass (crates/engine-render/src/renderer/
 * fog_volume.rs). An ENHANCEMENT, not retail: the bank is simulated by the
 * engine (crates/render-kernels/src/fog_volume.rs, stepped inside
 * World::tick) and handed over each frame through five wasm exports; this
 * file only draws it.
 *
 * The bank is a stack of horizontal alpha-blended sheets over the walk
 * ground, depth-tested against the scene already in the framebuffer and
 * writing no depth, drawn after the 3D scene and before the screen-prim
 * layer and the 2D HUD canvas. The GLSL below is a transcription of the
 * native WGSL - keep the two in step. The recipe's numbers (noise scales,
 * sheet gain, profile exponent) come from the frame header, not from here.
 *
 * The scene's ground coverage field (fog_volume::FogGround) arrives as an
 * RGBA8 texture - coverage, spill, flow - so the bank lies on the drawn
 * floor and spills off its rims; without one (battle, an older bundle) the
 * pass binds a single full-coverage texel and draws the bank as before. A
 * negative mesh-vertex weight marks the distant haze layer far below a rim.
 *
 * Header layout: legaia_render_kernels::fog_volume::header. */
(function () {
  'use strict';

  /* Indices into the header (fog_volume::header). */
  const H = {
    SIM_ORIGIN_X: 0, SIM_ORIGIN_Z: 1, SIM_CELL: 2, SIM_DIM: 3,
    MESH_ORIGIN_X: 4, MESH_ORIGIN_Z: 5, MESH_CELL: 6, MESH_DIM: 7,
    COLOR_R: 8, COLOR_G: 9, COLOR_B: 10, OPACITY: 11,
    HEIGHT: 12, LAYERS: 13, DRIFT_X: 14, DRIFT_Z: 15,
    GROUND_GEN: 16, SPACE: 17, SHADER_CONSTANTS: 18, SOFT_DISTANCE: 22, LEN: 23,
    /* The ground-field tail (absent from a header shorter than FULL_LEN). */
    SPILL_PHASE: 23, GROUND_TEX_ORIGIN_X: 24, GROUND_TEX_ORIGIN_Z: 25,
    GROUND_TEX_EXTENT_X: 26, GROUND_TEX_EXTENT_Z: 27, GROUND_TEX_W: 28,
    GROUND_TEX_H: 29, GROUND_TEX_GEN: 30, FULL_LEN: 31,
    /* The spill / haze constants (fog_volume::FOG_SPILL_CONSTANTS,
     * FOG_HAZE_CONSTANTS), absent from a header shorter than SPILL_LEN. */
    SPILL_CONSTANTS: 31, HAZE_CONSTANTS: 35, SPILL_LEN: 39,
  };
  /* fog_volume::FOG_GROUND_NONE_TEXEL: full coverage, no spill. */
  const GROUND_NONE = new Uint8Array([255, 0, 0, 255]);

  /* The spill's outward direction at ground-texture UV `guv`: the gradient
   * of the flow channel (distance past the rim), +X where it is flat. Shared
   * by both stages (the native `spill_dir`). */
  const SPILL_DIR_GLSL = `
vec2 spill_dir(vec2 guv) {
  vec2 e = u_ground.zw * 64.0;
  float fx = textureLod(u_gtex, guv + vec2(e.x, 0.0), 0.0).b
    - textureLod(u_gtex, guv - vec2(e.x, 0.0), 0.0).b;
  float fz = textureLod(u_gtex, guv + vec2(0.0, e.y), 0.0).b
    - textureLod(u_gtex, guv - vec2(0.0, e.y), 0.0).b;
  float glen = length(vec2(fx, fz));
  return glen > 1.0e-4 ? vec2(fx, fz) / glen : vec2(1.0, 0.0);
}
`;

  const VS = `#version 300 es
precision highp float;
layout(location = 0) in vec4 a_pos;
uniform mat4 u_m;
uniform vec4 u_params;
uniform vec4 u_haze;
uniform vec4 u_ground;
uniform vec4 u_spill;
uniform sampler2D u_gtex;
out vec2 v_xz;
out float v_layer;
out float v_clip_w;
out vec3 v_world;
out float v_floor_w;
out float v_haze;
out vec3 v_view;
${SPILL_DIR_GLSL}
/* The camera's position in the bank's space: the one point the matrix sends
 * to clip x = y = w = 0 (every view ray passes through it). */
vec3 eye_of(mat4 m) {
  vec4 r0 = vec4(m[0].x, m[1].x, m[2].x, m[3].x);
  vec4 r1 = vec4(m[0].y, m[1].y, m[2].y, m[3].y);
  vec4 r3 = vec4(m[0].w, m[1].w, m[2].w, m[3].w);
  vec3 c01 = cross(r0.xyz, r1.xyz);
  vec3 c13 = cross(r1.xyz, r3.xyz);
  vec3 c30 = cross(r3.xyz, r0.xyz);
  float det = dot(r0.xyz, c13);
  if (abs(det) < 1.0e-12) return vec3(0.0);
  return -(r0.w * c13 + r1.w * c30 + r3.w * c01) / det;
}
void main() {
  float s = (float(gl_InstanceID) + 0.5) / u_params.y;
  /* Sheets packed toward the floor (quadratic spacing); retail Y-down, so
   * they stack upward from just above it. A negative weight marks the
   * distant haze layer, whose stack is deeper. */
  float t = s * s;
  float haze = a_pos.w < 0.0 ? 1.0 : 0.0;
  float y = a_pos.y - 2.0 - t * u_params.x * mix(1.0, u_haze.x, haze);
  /* On the spill's near-vertical curtain a vertical stack slides the sheets
   * along themselves; spread them outward instead, so the fall is a slab
   * with depth rather than one bright plane seen edge-on. */
  vec2 xz = a_pos.xz;
  vec2 guv = (a_pos.xz - u_ground.xy) * u_ground.zw;
  vec4 gv = textureLod(u_gtex, guv, 0.0);
  if (gv.g > 0.0 && haze == 0.0) {
    xz += spill_dir(guv) * (s * u_spill.w * min(gv.g * 4.0, 1.0));
  }
  gl_Position = u_m * vec4(xz.x, y, xz.y, 1.0);
  v_xz = a_pos.xz;
  v_layer = t;
  v_clip_w = gl_Position.w;
  v_world = vec3(xz.x, y, xz.y);
  v_view = v_world - eye_of(u_m);
  v_floor_w = abs(a_pos.w);
  v_haze = haze;
}`;

  /* The play page's log-of-w depth (webgl-shaders.js LOG_DEPTH_GLSL): when
   * the frame's mesh program wrote it, the sheets must test in the same
   * encoding and the soft intersection must decode it. A cached
   * webgl-shaders.js without it falls back to the same formula. */
  const LOG_GLSL = (typeof LOG_DEPTH_GLSL === 'string') ? LOG_DEPTH_GLSL : `
float logDepthOfW(float w) {
  return clamp(log2(max(w, 1.0)) / 24.0, 0.0, 1.0);
}
`;
  const LOG_RANGE = (typeof LOG_DEPTH_RANGE === 'number') ? LOG_DEPTH_RANGE : 24.0;

  const FS = `#version 300 es
precision highp float;
precision highp int;
uniform vec2 u_log_depth;
${LOG_GLSL}
uniform vec4 u_sim;
uniform vec4 u_mesh;
uniform vec4 u_color;
uniform vec4 u_params;
uniform vec4 u_consts;
uniform vec4 u_depth;
uniform vec4 u_ground;
uniform float u_spill_phase;
uniform vec4 u_spill;
uniform vec4 u_haze;
uniform sampler2D u_dens;
uniform sampler2D u_gtex;
uniform highp sampler2D u_scene_depth;
in vec2 v_xz;
in float v_layer;
in float v_clip_w;
in vec3 v_world;
in float v_floor_w;
in float v_haze;
in vec3 v_view;
out vec4 o_color;
${SPILL_DIR_GLSL}

float fog_hash(ivec2 i) {
  uint x = uint(i.x & 0xffff);
  uint y = uint(i.y & 0xffff);
  uint h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
  h = (h ^ (h >> 13u)) * 0x85ebca6bu;
  h = h ^ (h >> 16u);
  return float(h & 0xffffu) / 65535.0;
}

float fog_noise(vec2 p) {
  vec2 fl = floor(p);
  ivec2 i = ivec2(fl);
  vec2 f = p - fl;
  vec2 s = f * f * (vec2(3.0) - 2.0 * f);
  float a = fog_hash(i);
  float b = fog_hash(i + ivec2(1, 0));
  float c = fog_hash(i + ivec2(0, 1));
  float d = fog_hash(i + ivec2(1, 1));
  return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

void main() {
  float t = v_layer;
  vec2 q = v_xz * u_consts.x;
  vec2 dq = u_params.zw * u_consts.x;
  vec2 w = vec2(
    fog_noise(q * 0.45 + vec2(1.7, 9.2) - dq * 0.3),
    fog_noise(q * 0.45 + vec2(8.3, 2.8) + dq * 0.25)) - vec2(0.5);
  vec2 p = q + w * 1.8;
  float f = 0.55 * fog_noise(p - dq)
    + 0.30 * fog_noise(p * 2.07 - dq * 1.7 + vec2(t * 3.1, 0.0))
    + 0.15 * fog_noise(p * 4.31 - dq * 2.6 + vec2(0.0, t * 5.7));
  float nb = fog_noise((v_xz - u_params.zw * 0.4) * u_consts.y + vec2(3.1, 8.9));
  float crest = smoothstep(0.30, 0.72, f * 0.65 + nb * 0.55);
  float top = 0.35 + 0.65 * crest;
  float h = clamp(1.0 - t / top, 0.0, 1.0);
  float profile = pow(h, u_consts.w);
  vec2 uv = (v_xz - u_sim.xy) / (u_sim.z * u_sim.w);
  float d = 1.0;
  if (all(greaterThanEqual(uv, vec2(0.0))) && all(lessThanEqual(uv, vec2(1.0)))) {
    d = textureLod(u_dens, uv, 0.0).r;
  }
  float half_extent = 0.5 * u_mesh.z * u_mesh.w;
  vec2 centre = u_mesh.xy + vec2(half_extent);
  float r = length(v_xz - centre) / half_extent;
  float edge = 1.0 - smoothstep(0.55, 0.95, r);
  /* Soft intersection against the scene depth (window depth -> GL NDC ->
   * view w through ndc = A + B / w). u_depth.w = 0 when the page could not
   * copy its depth buffer: no fade. */
  /* u_log_depth.x: the scene was written as log2(w) / u_log_depth.y. */
  gl_FragDepth = u_log_depth.x > 0.5 ? logDepthOfW(v_clip_w) : gl_FragCoord.z;
  float soft = 1.0;
  if (u_depth.w > 0.5) {
    float raw = texelFetch(u_scene_depth, ivec2(gl_FragCoord.xy), 0).r;
    float ws = 1.0e9;
    if (u_log_depth.x > 0.5) {
      ws = raw >= 1.0 ? 1.0e9 : exp2(raw * u_log_depth.y);
    } else {
      float den = (raw * 2.0 - 1.0) - u_depth.x;
      if (abs(den) > 1.0e-7) {
        float cand = u_depth.y / den;
        if (cand > 0.0) ws = cand;
      }
    }
    soft = clamp((ws - v_clip_w) / u_depth.z, 0.0, 1.0);
  }
  vec3 nrm = cross(dFdx(v_world), dFdy(v_world));
  float flat_ = abs(nrm.y) / max(length(nrm), 1.0e-6);
  float level = smoothstep(0.55, 0.85, flat_);
  /* Ground field: full bank on the floor, a soft fade over its edge, and
   * past a rim the spill - slow tendrils pouring down the falling sheet,
   * parting and thinning with the fall. */
  vec2 guv = (v_xz - u_ground.xy) * u_ground.zw;
  vec4 g = textureLod(u_gtex, guv, 0.0);
  float ground_a = g.r;
  if (g.g > 0.0) {
    vec2 dir = spill_dir(guv);
    /* World units along the rim, and of drop below it (the spill_drop
     * curve over the flow coordinate). */
    float along = dot(v_xz, vec2(-dir.y, dir.x));
    float fall01 = g.b * sqrt(g.b);
    float fall = fall01 * u_spill.y;
    float ph = u_spill_phase;
    /* Pour mask: long stretches of rim pour, others barely do; it evolves
     * in place rather than scrolling. */
    float m = fog_noise(vec2(along * u_spill.z + 4.7, ph * 0.05 + 1.3));
    float pour = 0.12 + 0.88 * smoothstep(0.32, 0.62, m);
    /* Tendrils: narrow along the rim, long down the fall, swaying slowly;
     * the second octave flows at its own rate so the pattern changes as it
     * falls. */
    float sway = (fog_noise(vec2(fall * 0.6 - ph * 0.4, along * u_spill.z * 2.0 + 9.1)) - 0.5) * 1.8;
    /* Each sheet of the slab a little apart, so the layers do not repeat
     * one pattern. */
    float ta = along * u_spill.x + sway + sqrt(t) * 0.9;
    float tn = 0.65 * fog_noise(vec2(ta, fall - ph))
      + 0.35 * fog_noise(vec2(ta * 2.1 + 3.3, fall * 1.7 - ph * 1.35 + 5.2));
    /* High-frequency erosion: only matters near the threshold, so it rags
     * each tendril's edges and leaves its core. */
    float er = fog_noise(vec2(along * u_spill.x * 4.3 + 1.9, fall * 5.0 - ph * 2.3)) - 0.5;
    float th = 0.34 + 0.24 * fall01;
    float tendril = smoothstep(th, th + 0.3, tn + er * 0.3);
    /* Puffs: the tendrils fall as clumps with thin gaps between them, not
     * as unbroken lines. */
    float puff = smoothstep(0.3, 0.7, fog_noise(vec2(along * u_spill.z * 3.7 + 2.0, fall * 1.3 - ph * 0.8 + 6.1)));
    float spill = 1.4 * g.g * pour * tendril * (0.45 + 0.55 * puff) * (0.35 + 0.65 * f);
    ground_a = max(ground_a * pour * (0.4 + 0.6 * tendril), spill);
    /* The spill sheet falls steeply on purpose: exempt it from the
     * cliff-face fade, but thin a sheet seen edge-on, where each layer of
     * the slab would draw as one bright line. */
    float facing = abs(dot(nrm, v_view)) / max(length(nrm) * length(v_view), 1.0e-6);
    level = mix(level, smoothstep(0.1, 0.45, facing), clamp(g.g * 4.0, 0.0, 1.0) * (1.0 - g.r));
  }
  float a = u_color.a * d * crest * profile * edge * soft * level * ground_a * v_floor_w * u_consts.z / u_params.y;
  /* The distant haze: broad, soft banks drifting slower than the bank,
   * thickest mid-stack, faint. */
  float haze_a = 0.0;
  if (v_haze > 0.0) {
    vec2 hq = (v_xz - u_params.zw * u_haze.w) * u_haze.z;
    float hn = 0.6 * fog_noise(hq + vec2(t * 0.9, 2.3))
      + 0.4 * fog_noise(hq * 2.3 + vec2(7.7, t * 1.7));
    float bank = smoothstep(0.25, 0.8, hn);
    float hp = 4.0 * t * (1.0 - t);
    haze_a = u_color.a * u_haze.y * bank * hp * edge * soft * v_floor_w * v_haze * u_consts.z / u_params.y;
  }
  vec3 rgb = u_color.rgb * mix(0.75 + 0.5 * f, 0.9, v_haze);
  o_color = vec4(rgb, clamp(a * (1.0 - v_haze) + haze_a, 0.0, 1.0));
}`;

  function compile(gl, type, src) {
    const sh = gl.createShader(type);
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(sh);
      gl.deleteShader(sh);
      throw new Error('fog volume shader: ' + log);
    }
    return sh;
  }

  /* Column-major 4x4 multiply: out = a * b. */
  function mul4(a, b) {
    const o = new Float32Array(16);
    for (let c = 0; c < 4; c++) {
      for (let r = 0; r < 4; r++) {
        let s = 0;
        for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
        o[c * 4 + r] = s;
      }
    }
    return o;
  }

  class FogVolumePass {
    constructor(gl) {
      if (typeof WebGL2RenderingContext === 'undefined' || !(gl instanceof WebGL2RenderingContext)) {
        throw new Error('fog volume needs WebGL2');
      }
      this.gl = gl;
      const prog = gl.createProgram();
      const vs = compile(gl, gl.VERTEX_SHADER, VS);
      const fs = compile(gl, gl.FRAGMENT_SHADER, FS);
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.linkProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        throw new Error('fog volume link: ' + gl.getProgramInfoLog(prog));
      }
      this.prog = prog;
      this.loc = {
        m: gl.getUniformLocation(prog, 'u_m'),
        sim: gl.getUniformLocation(prog, 'u_sim'),
        mesh: gl.getUniformLocation(prog, 'u_mesh'),
        color: gl.getUniformLocation(prog, 'u_color'),
        params: gl.getUniformLocation(prog, 'u_params'),
        consts: gl.getUniformLocation(prog, 'u_consts'),
        depth: gl.getUniformLocation(prog, 'u_depth'),
        logDepth: gl.getUniformLocation(prog, 'u_log_depth'),
        dens: gl.getUniformLocation(prog, 'u_dens'),
        ground: gl.getUniformLocation(prog, 'u_ground'),
        spillPhase: gl.getUniformLocation(prog, 'u_spill_phase'),
        spill: gl.getUniformLocation(prog, 'u_spill'),
        haze: gl.getUniformLocation(prog, 'u_haze'),
        gtex: gl.getUniformLocation(prog, 'u_gtex'),
        sceneDepth: gl.getUniformLocation(prog, 'u_scene_depth'),
      };
      this.vao = gl.createVertexArray();
      this.vbuf = gl.createBuffer();
      this.ibuf = gl.createBuffer();
      this.indexCount = 0;
      this.groundGen = null;
      this.tex = gl.createTexture();
      this.texDim = 0;
      /* The ground coverage field; `groundTexGen` is the header generation
       * last uploaded (`-1` = the full-coverage texel, `null` = nothing). */
      this.gtex = gl.createTexture();
      this.groundTexGen = null;
      /* Scene-depth copy for the soft intersection: the default framebuffer's
       * depth cannot be sampled, so it is blitted into a depth texture each
       * frame. `null` until first use, `false` once the copy proved
       * impossible on this driver (the pass then draws without the fade). */
      this.depthCopy = null;
    }

    /* Copy the default framebuffer's depth into a sampleable texture. The
     * blit requires matching depth formats, which the context does not
     * report, so both plausible ones are tried once. Returns the texture or
     * null. */
    _copySceneDepth(w, h) {
      const gl = this.gl;
      if (this.depthCopy === false) return null;
      const tryFormat = (internal, format, type, attach) => {
        const tex = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.texImage2D(gl.TEXTURE_2D, 0, internal, w, h, 0, format, type, null);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        const fbo = gl.createFramebuffer();
        gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, attach, gl.TEXTURE_2D, tex, 0);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        return { tex, fbo, w, h, internal, format, type, attach };
      };
      const blit = (c) => {
        while (gl.getError() !== gl.NO_ERROR) { /* drain */ }
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, c.fbo);
        gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, gl.DEPTH_BUFFER_BIT, gl.NEAREST);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        return gl.getError() === gl.NO_ERROR;
      };
      const free = (c) => { gl.deleteFramebuffer(c.fbo); gl.deleteTexture(c.tex); };
      let c = this.depthCopy;
      if (c && (c.w !== w || c.h !== h)) {
        const n = tryFormat(c.internal, c.format, c.type, c.attach);
        free(c);
        c = this.depthCopy = n;
      }
      if (c) return blit(c) ? c.tex : null;
      /* DEPTH24_STENCIL8 first: it is what the browsers back a WebGL2
       * default depth buffer with, even for a context created without
       * `stencil` (ANGLE reports the packed format to the blit check). Trying
       * DEPTH_COMPONENT24 first failed on every context the page creates and
       * logged a "glBlitFramebuffer: depth/stencil format" warning to the
       * console each time a pass was built. The depth-only format stays as the
       * fallback for a driver that really allocates one. */
      const candidates = [
        [gl.DEPTH24_STENCIL8, gl.DEPTH_STENCIL, gl.UNSIGNED_INT_24_8, gl.DEPTH_STENCIL_ATTACHMENT],
        [gl.DEPTH_COMPONENT24, gl.DEPTH_COMPONENT, gl.UNSIGNED_INT, gl.DEPTH_ATTACHMENT],
      ];
      for (const [internal, format, type, attach] of candidates) {
        const cand = tryFormat(internal, format, type, attach);
        if (blit(cand)) { this.depthCopy = cand; return cand.tex; }
        free(cand);
      }
      this.depthCopy = false;
      return null;
    }

    /* Draw one frame's bank. `rt` is the LegaiaRuntime, `vp` the matrix the
     * page drew the scene with (its Y-up draw frame), `battleScale` the
     * battle stage's world scale (a battle bank lives in raw stage units),
     * `logDepth` whether the scene this frame was drawn with log-of-w depth.
     * Returns false when the engine has no bank this frame. */
    draw(rt, vp, battleScale, logDepth) {
      const h = rt.play_fog_volume_header();
      if (!h || h.length < H.LEN || !vp) return false;
      const gl = this.gl;
      if (this.indexCount === 0) {
        const idx = rt.play_fog_volume_indices();
        gl.bindVertexArray(this.vao);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibuf);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, idx, gl.STATIC_DRAW);
        gl.bindVertexArray(null);
        this.indexCount = idx.length;
      }
      if (this.groundGen !== h[H.GROUND_GEN]) {
        const pos = rt.play_fog_volume_mesh();
        gl.bindVertexArray(this.vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vbuf);
        gl.bufferData(gl.ARRAY_BUFFER, pos, gl.DYNAMIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 4, gl.FLOAT, false, 16, 0);
        gl.bindVertexArray(null);
        this.groundGen = h[H.GROUND_GEN];
      }
      const dim = h[H.SIM_DIM] | 0;
      const dens = rt.play_fog_volume_density();
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.tex);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      if (this.texDim !== dim) {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, dim, dim, 0, gl.RED, gl.UNSIGNED_BYTE, dens);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        this.texDim = dim;
      } else {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, dim, dim, gl.RED, gl.UNSIGNED_BYTE, dens);
      }

      /* The ground coverage field: re-uploaded only when its generation
       * changes; a frame without one binds the full-coverage texel. */
      const hasGround = h.length >= H.FULL_LEN && h[H.GROUND_TEX_W] > 0
        && h[H.GROUND_TEX_EXTENT_X] > 0 && h[H.GROUND_TEX_EXTENT_Z] > 0;
      const wantGen = hasGround ? h[H.GROUND_TEX_GEN] : -1;
      if (this.groundTexGen !== wantGen) {
        let gw = 1, gh = 1, texels = GROUND_NONE;
        if (hasGround) {
          const t = rt.play_fog_volume_ground ? rt.play_fog_volume_ground() : null;
          const w = h[H.GROUND_TEX_W] | 0, hh = h[H.GROUND_TEX_H] | 0;
          if (t && t.length === w * hh * 4) { gw = w; gh = hh; texels = t; }
        }
        gl.activeTexture(gl.TEXTURE2);
        gl.bindTexture(gl.TEXTURE_2D, this.gtex);
        gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gw, gh, 0, gl.RGBA, gl.UNSIGNED_BYTE, texels);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.activeTexture(gl.TEXTURE0);
        this.groundTexGen = wantGen;
        this.groundRect = (hasGround && gw > 1)
          ? [h[H.GROUND_TEX_ORIGIN_X], h[H.GROUND_TEX_ORIGIN_Z],
            1 / h[H.GROUND_TEX_EXTENT_X], 1 / h[H.GROUND_TEX_EXTENT_Z]]
          : [0, 0, 0, 0];
      }

      /* The bank's space -> the page's Y-up draw frame: the field world is
       * retail Y-down (one Y negation, the per-model flip every field
       * placement carries); the battle stage is raw units under the stage's
       * world scale with the same flip. */
      const s = h[H.SPACE] >= 0.5 ? (battleScale || 4.0) : 1.0;
      const model = new Float32Array([
        s, 0, 0, 0,
        0, -s, 0, 0,
        0, 0, s, 0,
        0, 0, 0, 1,
      ]);
      const m = mul4(vp, model);
      /* The camera's depth mapping ndc = A + B / w off the matrix's third
       * and fourth rows (column-major: row r is m[r], m[4 + r], ...). */
      const zr = [m[2], m[6], m[10], m[14]];
      const wr = [m[3], m[7], m[11], m[15]];
      const ww = wr[0] * wr[0] + wr[1] * wr[1] + wr[2] * wr[2];
      const A = ww > 1e-12 ? (zr[0] * wr[0] + zr[1] * wr[1] + zr[2] * wr[2]) / ww : 0;
      const B = ww > 1e-12 ? zr[3] - A * wr[3] : 0;
      const cw = gl.drawingBufferWidth, ch = gl.drawingBufferHeight;
      const depthTex = this._copySceneDepth(cw, ch);

      gl.useProgram(this.prog);
      gl.uniformMatrix4fv(this.loc.m, false, m);
      gl.uniform4f(this.loc.sim, h[H.SIM_ORIGIN_X], h[H.SIM_ORIGIN_Z], h[H.SIM_CELL], h[H.SIM_DIM]);
      gl.uniform4f(this.loc.mesh, h[H.MESH_ORIGIN_X], h[H.MESH_ORIGIN_Z], h[H.MESH_CELL], h[H.MESH_DIM]);
      gl.uniform4f(this.loc.color, h[H.COLOR_R], h[H.COLOR_G], h[H.COLOR_B], h[H.OPACITY]);
      gl.uniform4f(this.loc.params, h[H.HEIGHT], h[H.LAYERS], h[H.DRIFT_X], h[H.DRIFT_Z]);
      const c = H.SHADER_CONSTANTS;
      gl.uniform4f(this.loc.consts, h[c], h[c + 1], h[c + 2], h[c + 3]);
      gl.uniform2f(this.loc.logDepth, logDepth ? 1 : 0, LOG_RANGE);
      gl.uniform4f(this.loc.depth, A, B, Math.max(h[H.SOFT_DISTANCE] || 0, 1e-3), depthTex ? 1 : 0);
      gl.uniform1i(this.loc.dens, 0);
      const gr = this.groundRect || [0, 0, 0, 0];
      gl.uniform4f(this.loc.ground, gr[0], gr[1], gr[2], gr[3]);
      gl.uniform1f(this.loc.spillPhase, h.length >= H.FULL_LEN ? h[H.SPILL_PHASE] : 0);
      /* A header without the spill / haze tail (a stale bundle) draws a
       * still spill and no haze rather than reading past its end. */
      const sc = H.SPILL_CONSTANTS, hc = H.HAZE_CONSTANTS;
      const tail = h.length >= H.SPILL_LEN;
      gl.uniform4f(this.loc.spill, tail ? h[sc] : 0, tail ? h[sc + 1] : 0,
        tail ? h[sc + 2] : 0, tail ? h[sc + 3] : 0);
      gl.uniform4f(this.loc.haze, tail ? h[hc] : 1, tail ? h[hc + 1] : 0,
        tail ? h[hc + 2] : 0, tail ? h[hc + 3] : 0);
      gl.activeTexture(gl.TEXTURE2);
      gl.bindTexture(gl.TEXTURE_2D, this.gtex);
      gl.uniform1i(this.loc.gtex, 2);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, depthTex || null);
      gl.uniform1i(this.loc.sceneDepth, 1);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.tex);

      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.depthMask(false);
      gl.disable(gl.CULL_FACE);
      gl.enable(gl.BLEND);
      gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ZERO, gl.ONE);
      gl.blendEquation(gl.FUNC_ADD);
      gl.bindVertexArray(this.vao);
      gl.drawElementsInstanced(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0, h[H.LAYERS] | 0);
      gl.bindVertexArray(null);
      gl.depthMask(true);
      gl.disable(gl.BLEND);
      return true;
    }

    dispose() {
      const gl = this.gl;
      gl.deleteProgram(this.prog);
      gl.deleteBuffer(this.vbuf);
      gl.deleteBuffer(this.ibuf);
      gl.deleteVertexArray(this.vao);
      gl.deleteTexture(this.tex);
      gl.deleteTexture(this.gtex);
      if (this.depthCopy) {
        gl.deleteFramebuffer(this.depthCopy.fbo);
        gl.deleteTexture(this.depthCopy.tex);
      }
    }
  }

  window.LegaiaFogVolumePass = FogVolumePass;
})();
