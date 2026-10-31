/* Screen-space ambient occlusion on the play page, with the contact shadows
 * folded into it - the browser twin of the native renderer's SSAO
 * (crates/engine-render/src/renderer/ssao.rs). An ENHANCEMENT, not retail:
 * the field path draws baked colour words with no light source, so there is
 * no ambient term to occlude. On by default on both hosts, off under
 * Retail-faithful (the persisted `ssao` option, `Home`).
 *
 * The scene pass (webgl-tmd-assembled-scene.js `renderAssembled`) owns the
 * ordering, as native does inside its main pass:
 *
 * 1. pre-pass - the opaque draws again, through the scene program itself
 *    with `u_gbuf` set (same vertex stage, same discards; webgl-shaders.js
 *    `gbuf_word`), into this pass's RGBA32F G-buffer: `rgb` the view-space
 *    face normal, `a` the linear view depth (clip w);
 * 2. AO - the Alchemy estimator over a golden-angle spiral (full resolution
 *    up to a 540-row drawing buffer, half above), times the contact-shadow
 *    march toward enhanced lighting's key light;
 * 3. blur - one separable depth- and normal-aware pass pair;
 * 4. apply - a full-screen multiply of the frame by the AO, right after the
 *    opaque draws, so the semi-transparent prims, the glow, the fog, the
 *    bloom and every screen layer draw undarkened.
 *
 * Every number arrives from the engine (`play_ssao_frame`:
 * `legaia_render_kernels::ssao`), and every helper below transcribes the
 * native WGSL function of the same name - the `ssao_parity` test evaluates
 * both against the kernel. GL's window y runs up where wgpu's runs down,
 * which `ssao_view_pos` and `view_to_px` absorb. */
(function () {
  'use strict';

  /* Drawing buffers at or below this many rows run AO at full resolution
   * (`FULL_RES_MAX_HEIGHT`); above it at half on each axis. */
  const FULL_RES_MAX_HEIGHT = 540;

  const VS = `#version 300 es
void main() {
  float x = float((gl_VertexID << 1) & 2);
  float y = float(gl_VertexID & 2);
  gl_Position = vec4(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}`;

  /* The frame block and the per-sample law, shared by the three passes. */
  const SSAO_COMMON_GLSL = `
uniform vec4 u_view_k;
uniform vec4 u_viewport;
uniform vec4 u_p0;
uniform vec4 u_p1;
uniform vec4 u_p2;
uniform vec4 u_p3;
uniform vec4 u_light;
uniform vec4 u_pass_info;
uniform highp sampler2D u_gbuf;
uniform highp sampler2D u_ao;
out vec4 o_color;

const float TAU = 6.2831853;
const float GOLDEN_ANGLE = 2.3999632;

vec3 ssao_view_pos(vec2 px, float w) {
  vec2 ndc = (px - u_viewport.xy) / u_viewport.zw * 2.0 - 1.0;
  return vec3(ndc.x * w * u_view_k.x, ndc.y * w * u_view_k.y, w * u_view_k.z);
}

float spin(vec2 px) {
  return fract(52.982918 * fract(0.06711056 * px.x + 0.00583715 * px.y));
}

vec2 sample_offset(int i, int n, float s) {
  float t = (float(i) + 0.5) / float(max(n, 1));
  float r = sqrt(t);
  float a = float(i) * GOLDEN_ANGLE + s * TAU;
  return vec2(r * cos(a), r * sin(a));
}

float radius_px(float view_z) {
  if (view_z <= 0.0 || u_view_k.y <= 0.0) return 0.0;
  float px = u_p0.x * 0.5 * u_viewport.w * u_view_k.z / (u_view_k.y * view_z);
  return min(px, u_p1.y * u_viewport.w);
}

float occlusion_term(vec3 v, vec3 n, float view_z) {
  float vv = dot(v, v);
  float r2 = u_p0.x * u_p0.x;
  float falloff = max(1.0 - vv / r2, 0.0);
  float num = max(dot(v, n) - u_p0.y * view_z, 0.0);
  return u_p0.x * num / (vv + 0.01 * r2) * falloff;
}

float ao_from_sum(float sum, int n) {
  float a = clamp(1.0 - u_p0.z * sum / float(max(n, 1)), 0.0, 1.0);
  return max(pow(a, u_p0.w), u_p1.x);
}

float blur_weight(float t, float z0, float z, vec3 n0, vec3 n) {
  if (z <= 0.0) return 0.0;
  float g = exp(-(t * t) / (2.0 * u_p2.w * u_p2.w));
  float dz = abs(z - z0) / (max(z0, 1e-3) * u_p2.y);
  float dw = exp(-(dz * dz));
  float nw = pow(max(dot(n0, n), 0.0), u_p2.z);
  return g * dw * nw;
}

vec2 view_to_px(vec3 q) {
  float w = q.z / u_view_k.z;
  vec2 ndc = vec2(q.x / (w * u_view_k.x), q.y / (w * u_view_k.y));
  return u_viewport.xy + (ndc * 0.5 + 0.5) * u_viewport.zw;
}

float contact_hit(float ray_z, float scene_z) {
  float dz = ray_z - scene_z;
  if (dz > u_p3.w * ray_z && dz < u_p3.y) return 1.0;
  return 0.0;
}

float contact_fade(float t) {
  return clamp(1.0 - t / max(u_p3.x, 1e-3), 0.0, 1.0);
}

float contact_from_hit(float hit, float ndl, float strength) {
  float facing = clamp(ndl * 4.0, 0.0, 1.0);
  return 1.0 - strength * clamp(hit, 0.0, 1.0) * facing;
}

ivec2 gbuf_dims() { return textureSize(u_gbuf, 0); }
ivec2 ao_dims() { return textureSize(u_ao, 0); }

ivec2 texel_pixel(ivec2 t) {
  int d = int(u_pass_info.z);
  return clamp(t * d + ivec2(d / 2), ivec2(0), gbuf_dims() - ivec2(1));
}
`;

  const FS_AO = `#version 300 es
precision highp float;
precision highp int;
${SSAO_COMMON_GLSL}
float contact_shadow(vec3 p, vec3 n, float s) {
  float strength = u_light.w;
  vec3 l = u_light.xyz;
  float ndl = dot(n, l);
  if (strength <= 0.0 || ndl <= 0.0) return 1.0;
  int steps = int(u_p3.z);
  vec3 origin = p + n * (u_p3.w * p.z);
  ivec2 hi = gbuf_dims() - ivec2(1);
  float hit = 0.0;
  for (int i = 0; i < steps; i++) {
    float t = (float(i) + s) / float(max(steps, 1)) * u_p3.x;
    vec3 q = origin + l * t;
    if (q.z <= 0.0) break;
    ivec2 px = ivec2(floor(view_to_px(q)));
    if (any(lessThan(px, ivec2(0))) || any(greaterThan(px, hi))) break;
    vec4 gq = texelFetch(u_gbuf, px, 0);
    if (gq.w <= 0.0) continue;
    hit = max(hit, contact_hit(q.z, gq.w * u_view_k.z) * contact_fade(t));
  }
  return contact_from_hit(hit, ndl, strength);
}
void main() {
  ivec2 c = texel_pixel(ivec2(gl_FragCoord.xy));
  vec4 g = texelFetch(u_gbuf, c, 0);
  if (g.w <= 0.0) { o_color = vec4(1.0); return; }
  vec2 pc = vec2(c) + vec2(0.5);
  vec3 p = ssao_view_pos(pc, g.w);
  float r = radius_px(p.z);
  if (r < u_p1.z) { o_color = vec4(1.0); return; }
  vec3 n = g.xyz;
  int ns = int(u_p1.w);
  float s = spin(floor(gl_FragCoord.xy));
  ivec2 hi = gbuf_dims() - ivec2(1);
  float sum = 0.0;
  for (int i = 0; i < ns; i++) {
    ivec2 q = clamp(ivec2(round(pc + sample_offset(i, ns, s) * r)), ivec2(0), hi);
    vec4 gq = texelFetch(u_gbuf, q, 0);
    if (gq.w <= 0.0) continue;
    vec3 v = ssao_view_pos(vec2(q) + vec2(0.5), gq.w) - p;
    sum = sum + occlusion_term(v, n, p.z);
  }
  o_color = vec4(max(ao_from_sum(sum, ns) * contact_shadow(p, n, s), u_p1.x));
}`;

  const FS_BLUR = `#version 300 es
precision highp float;
precision highp int;
${SSAO_COMMON_GLSL}
void main() {
  ivec2 t = ivec2(gl_FragCoord.xy);
  vec4 g0 = texelFetch(u_gbuf, texel_pixel(t), 0);
  float a0 = texelFetch(u_ao, t, 0).r;
  if (g0.w <= 0.0) { o_color = vec4(a0); return; }
  ivec2 axis = ivec2(u_pass_info.xy);
  ivec2 hi = ao_dims() - ivec2(1);
  int taps = int(u_p2.x);
  float sum = a0;
  float wsum = 1.0;
  for (int k = -taps; k <= taps; k++) {
    if (k == 0) continue;
    ivec2 tt = clamp(t + axis * k, ivec2(0), hi);
    vec4 g = texelFetch(u_gbuf, texel_pixel(tt), 0);
    float w = blur_weight(float(k), g0.w, g.w, g0.xyz, g.xyz);
    sum = sum + texelFetch(u_ao, tt, 0).r * w;
    wsum = wsum + w;
  }
  o_color = vec4(sum / wsum);
}`;

  const FS_APPLY = `#version 300 es
precision highp float;
precision highp int;
${SSAO_COMMON_GLSL}
float ao_at(ivec2 px, vec4 g) {
  float d = u_pass_info.z;
  if (d < 1.5) return texelFetch(u_ao, px, 0).r;
  vec2 base = (vec2(px) + vec2(0.5)) / d - vec2(0.5);
  ivec2 i0 = ivec2(floor(base));
  vec2 fr = base - floor(base);
  ivec2 hi = ao_dims() - ivec2(1);
  float sum = 0.0;
  float wsum = 0.0;
  for (int j = 0; j < 4; j++) {
    ivec2 o = ivec2(j & 1, j >> 1);
    ivec2 t = clamp(i0 + o, ivec2(0), hi);
    float bx = o.x == 1 ? fr.x : 1.0 - fr.x;
    float by = o.y == 1 ? fr.y : 1.0 - fr.y;
    vec4 gt = texelFetch(u_gbuf, texel_pixel(t), 0);
    float w = bx * by * blur_weight(0.0, g.w, gt.w, g.xyz, gt.xyz) + 1e-5 * bx * by;
    sum = sum + texelFetch(u_ao, t, 0).r * w;
    wsum = wsum + w;
  }
  return sum / max(wsum, 1e-8);
}
void main() {
  ivec2 px = ivec2(gl_FragCoord.xy);
  vec4 g = texelFetch(u_gbuf, px, 0);
  float a = 1.0;
  if (g.w > 0.0) a = ao_at(px, g);
  o_color = vec4(a, a, a, 1.0);
}`;

  const UNIFORMS = ['u_view_k', 'u_viewport', 'u_p0', 'u_p1', 'u_p2', 'u_p3',
    'u_light', 'u_pass_info', 'u_gbuf', 'u_ao'];
  /* Texture units the passes bind, clear of the scene program's own (VRAM
   * on 0, the lamp shadow array beside it), so handing the frame back to
   * the scene's blend pass needs no rebinding. */
  const UNIT_GBUF = 6;
  const UNIT_AO = 7;

  function compile(gl, type, src) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(s);
      gl.deleteShader(s);
      throw new Error('ssao shader: ' + log);
    }
    return s;
  }

  function link(gl, vs, fsSrc) {
    const fs = compile(gl, gl.FRAGMENT_SHADER, fsSrc);
    const p = gl.createProgram();
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    gl.deleteShader(fs);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      throw new Error('ssao program: ' + gl.getProgramInfoLog(p));
    }
    const loc = {};
    for (const u of UNIFORMS) loc[u] = gl.getUniformLocation(p, u);
    return { p, loc };
  }

  class SsaoPass {
    /* Throws where the G-buffer cannot be rendered (no
     * EXT_color_buffer_float): the caller switches the pass off. */
    constructor(gl) {
      this.gl = gl;
      if (!gl.getExtension('EXT_color_buffer_float')) {
        throw new Error('ssao: float render targets unsupported');
      }
      const vs = compile(gl, gl.VERTEX_SHADER, VS);
      this.ao = link(gl, vs, FS_AO);
      this.blur = link(gl, vs, FS_BLUR);
      this.apply = link(gl, vs, FS_APPLY);
      gl.deleteShader(vs);
      this.vao = gl.createVertexArray();
      this.targets = null;
    }

    _tex(w, h, internal, format, type) {
      const gl = this.gl;
      const tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(gl.TEXTURE_2D, 0, internal, w, h, 0, format, type, null);
      /* Read with texelFetch only; NEAREST keeps the unfilterable float
       * target complete. */
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return tex;
    }

    _fbo(tex, depth) {
      const gl = this.gl;
      const fbo = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      if (depth) {
        gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, depth);
      }
      const ok = gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE;
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      if (!ok) throw new Error('ssao: incomplete framebuffer');
      return fbo;
    }

    _free() {
      const gl = this.gl;
      const t = this.targets;
      if (!t) return;
      for (const f of [t.gbufFbo, t.aFbo, t.bFbo]) gl.deleteFramebuffer(f);
      for (const x of [t.gbuf, t.a, t.b]) gl.deleteTexture(x);
      gl.deleteRenderbuffer(t.depth);
      this.targets = null;
    }

    _ensure(w, h) {
      if (this.targets && this.targets.w === w && this.targets.h === h) return;
      this._free();
      const gl = this.gl;
      const d = h <= FULL_RES_MAX_HEIGHT ? 1 : 2;
      const aw = Math.ceil(w / d), ah = Math.ceil(h / d);
      const depth = gl.createRenderbuffer();
      gl.bindRenderbuffer(gl.RENDERBUFFER, depth);
      gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT24, w, h);
      gl.bindRenderbuffer(gl.RENDERBUFFER, null);
      const gbuf = this._tex(w, h, gl.RGBA32F, gl.RGBA, gl.FLOAT);
      const a = this._tex(aw, ah, gl.R8, gl.RED, gl.UNSIGNED_BYTE);
      const b = this._tex(aw, ah, gl.R8, gl.RED, gl.UNSIGNED_BYTE);
      this.targets = {
        w, h, d, aw, ah, depth, gbuf, a, b,
        gbufFbo: this._fbo(gbuf, depth),
        aFbo: this._fbo(a, null),
        bFbo: this._fbo(b, null),
      };
    }

    /* Bind and clear the G-buffer for the scene's pre-pass draws: depth to
     * the far plane, every texel empty (`w = 0`, which the passes read as
     * "no surface" and leave unoccluded). Returns false when the drawing
     * buffer is empty. */
    beginPrepass(w, h) {
      const gl = this.gl;
      if (!(w > 0 && h > 0)) return false;
      this._ensure(w, h);
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.targets.gbufFbo);
      gl.viewport(0, 0, w, h);
      gl.clearColor(0, 0, 0, 0);
      gl.clearDepth(1.0);
      gl.depthMask(true);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      return true;
    }

    _run(prog, frame, passInfo, aoTex, fbo, w, h) {
      const gl = this.gl;
      const L = prog.loc;
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.viewport(0, 0, w, h);
      gl.useProgram(prog.p);
      gl.uniform4f(L.u_view_k, frame[0], frame[1], frame[2], 0);
      gl.uniform4f(L.u_viewport, 0, 0, this.targets.w, this.targets.h);
      gl.uniform4fv(L.u_p0, frame.subarray(4, 8));
      gl.uniform4fv(L.u_p1, frame.subarray(8, 12));
      gl.uniform4fv(L.u_p2, frame.subarray(12, 16));
      gl.uniform4fv(L.u_p3, frame.subarray(16, 20));
      gl.uniform4fv(L.u_light, frame.subarray(20, 24));
      gl.uniform4fv(L.u_pass_info, passInfo);
      gl.activeTexture(gl.TEXTURE0 + UNIT_GBUF);
      gl.bindTexture(gl.TEXTURE_2D, this.targets.gbuf);
      gl.uniform1i(L.u_gbuf, UNIT_GBUF);
      gl.activeTexture(gl.TEXTURE0 + UNIT_AO);
      gl.bindTexture(gl.TEXTURE_2D, aoTex);
      gl.uniform1i(L.u_ao, UNIT_AO);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }

    /* AO + blur off the filled G-buffer, then multiply the frame in
     * `outFbo` (the scene's own framebuffer: null for the canvas) by it.
     * `frame` is the engine's packet (`play_ssao_frame`); `debug` draws the
     * AO term alone (the native LEGAIA_SSAO_VIEW). Leaves depth test on,
     * depth writes on, blending off, texture unit 0 active and `outFbo`
     * bound, which is the state the scene pass resumes in. */
    resolve(frame, outFbo, debug) {
      const gl = this.gl;
      const t = this.targets;
      const d = t.d;
      gl.disable(gl.DEPTH_TEST);
      gl.depthMask(false);
      gl.disable(gl.BLEND);
      gl.disable(gl.CULL_FACE);
      gl.bindVertexArray(this.vao);
      this._run(this.ao, frame, [0, 0, d, 0], t.b, t.aFbo, t.aw, t.ah);
      this._run(this.blur, frame, [1, 0, d, 0], t.a, t.bFbo, t.aw, t.ah);
      this._run(this.blur, frame, [0, 1, d, 0], t.b, t.aFbo, t.aw, t.ah);
      if (!debug) {
        /* framebuffer * ao, alpha kept. */
        gl.enable(gl.BLEND);
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFuncSeparate(gl.DST_COLOR, gl.ZERO, gl.ZERO, gl.ONE);
      }
      this._run(this.apply, frame, [0, 0, d, debug ? 1 : 0], t.a, outFbo, t.w, t.h);
      gl.disable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ZERO);
      gl.bindVertexArray(null);
      gl.activeTexture(gl.TEXTURE0);
      gl.enable(gl.DEPTH_TEST);
      gl.depthMask(true);
    }

    dispose() {
      const gl = this.gl;
      this._free();
      for (const x of [this.ao, this.blur, this.apply]) gl.deleteProgram(x.p);
      gl.deleteVertexArray(this.vao);
    }
  }

  window.LegaiaSsaoPass = SsaoPass;
})();
