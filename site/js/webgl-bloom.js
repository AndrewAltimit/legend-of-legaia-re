/* Bloom on the play page - the browser twin of the native renderer's bloom
 * pass (crates/engine-render/src/renderer/bloom.rs). An ENHANCEMENT, not
 * retail, and off by default: the 3D frame's bright pixels (lamp halos,
 * emissive props, lit windows, sunlit sky) bleed a soft glow into their
 * surroundings.
 *
 * Drawn over the finished 3D frame - after the volumetric fog, before the
 * screen-prim layer and the 2D HUD canvas, so the HUD never blooms. The
 * default framebuffer cannot be sampled, so its colour is blitted into a
 * texture first; four quarter-resolution passes then extract the bright
 * share, blur it twice (horizontal + vertical), and add it back with an
 * additive blend - the sum the native composite writes.
 *
 * The response (threshold, knee, intensity) is the engine's
 * (`legaia_render_kernels::bloom::BloomParams`, `play_bloom_params`), keyed
 * on enhanced lighting's mood. The GLSL below is a transcription of the
 * native WGSL - the `bloom_parity` test holds the two together. */
(function () {
  'use strict';

  /* Bloom targets are the drawing buffer divided by this on each axis
   * (`BLOOM_DOWNSCALE`), and the blur runs this many H + V rounds
   * (`BLOOM_BLUR_ROUNDS`). */
  const DOWNSCALE = 4;
  const BLUR_ROUNDS = 2;

  const VS = `#version 300 es
out vec2 v_uv;
void main() {
  float x = float((gl_VertexID << 1) & 2);
  float y = float(gl_VertexID & 2);
  v_uv = vec2(x, y);
  gl_Position = vec4(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}`;

  const FS_EXTRACT = `#version 300 es
precision highp float;
uniform sampler2D u_src;
uniform vec4 u_texel;
in vec2 v_uv;
out vec4 o_color;
float bloom_weight(vec3 c, float threshold, float knee) {
  float br = max(c.r, max(c.g, c.b));
  float soft = clamp(br - threshold + knee, 0.0, 2.0 * knee);
  soft = soft * soft / (4.0 * knee + 1e-5);
  return max(soft, br - threshold) / max(br, 1e-5);
}
vec3 bloom_tap(vec2 uv) {
  vec3 c = textureLod(u_src, uv, 0.0).rgb;
  return c * bloom_weight(c, u_texel.z, u_texel.w);
}
void main() {
  vec2 o = u_texel.xy;
  vec3 c = bloom_tap(v_uv + vec2(-o.x, -o.y))
    + bloom_tap(v_uv + vec2(o.x, -o.y))
    + bloom_tap(v_uv + vec2(-o.x, o.y))
    + bloom_tap(v_uv + vec2(o.x, o.y));
  o_color = vec4(c * 0.25, 1.0);
}`;

  const FS_BLUR = `#version 300 es
precision highp float;
uniform sampler2D u_src;
uniform vec2 u_step;
in vec2 v_uv;
out vec4 o_color;
vec3 blur_at(vec2 uv) {
  return textureLod(u_src, uv, 0.0).rgb;
}
void main() {
  vec2 s = u_step;
  vec3 c = blur_at(v_uv) * 0.2270270270;
  c = c + (blur_at(v_uv + s * 1.3846153846) + blur_at(v_uv - s * 1.3846153846)) * 0.3162162162;
  c = c + (blur_at(v_uv + s * 3.2307692308) + blur_at(v_uv - s * 3.2307692308)) * 0.0702702703;
  o_color = vec4(c, 1.0);
}`;

  const FS_COMPOSITE = `#version 300 es
precision highp float;
uniform sampler2D u_src;
uniform float u_intensity;
in vec2 v_uv;
out vec4 o_color;
void main() {
  o_color = vec4(textureLod(u_src, v_uv, 0.0).rgb * u_intensity, 1.0);
}`;

  function compile(gl, type, src) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(s);
      gl.deleteShader(s);
      throw new Error('bloom shader: ' + log);
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
      throw new Error('bloom program: ' + gl.getProgramInfoLog(p));
    }
    return p;
  }

  class BloomPass {
    constructor(gl) {
      this.gl = gl;
      const vs = compile(gl, gl.VERTEX_SHADER, VS);
      this.extract = link(gl, vs, FS_EXTRACT);
      this.blur = link(gl, vs, FS_BLUR);
      this.composite = link(gl, vs, FS_COMPOSITE);
      gl.deleteShader(vs);
      this.loc = {
        exSrc: gl.getUniformLocation(this.extract, 'u_src'),
        exTexel: gl.getUniformLocation(this.extract, 'u_texel'),
        blSrc: gl.getUniformLocation(this.blur, 'u_src'),
        blStep: gl.getUniformLocation(this.blur, 'u_step'),
        coSrc: gl.getUniformLocation(this.composite, 'u_src'),
        coIntensity: gl.getUniformLocation(this.composite, 'u_intensity'),
      };
      this.vao = gl.createVertexArray();
      this.targets = null;
      /* Whether the scene copy is RGB8 (`true`) or RGBA8 (`false`); `null`
       * until a blit settles it. A blit out of the multisampled default
       * framebuffer needs the copy's format to match the framebuffer's,
       * which the context does not report: an `alpha: false` canvas is RGB8
       * on some drivers and RGBA8 on others, so both are tried once. */
      this.sceneRgb = null;
      const attrs = gl.getContextAttributes();
      this.preferRgb = !(attrs && attrs.alpha);
    }

    _target(w, h, rgb) {
      const gl = this.gl;
      const tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, tex);
      if (rgb) {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB8, w, h, 0, gl.RGB, gl.UNSIGNED_BYTE, null);
      } else {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
      }
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      const fbo = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return { tex, fbo, w, h };
    }

    _free() {
      const gl = this.gl;
      if (!this.targets) return;
      for (const t of [this.targets.scene, this.targets.a, this.targets.b]) {
        gl.deleteFramebuffer(t.fbo);
        gl.deleteTexture(t.tex);
      }
      this.targets = null;
    }

    _ensure(w, h) {
      if (this.targets && this.targets.w === w && this.targets.h === h) return;
      this._free();
      const qw = Math.max(1, Math.ceil(w / DOWNSCALE));
      const qh = Math.max(1, Math.ceil(h / DOWNSCALE));
      this.targets = {
        w, h, qw, qh,
        scene: this._target(w, h, this.sceneRgb === null ? this.preferRgb : this.sceneRgb),
        a: this._target(qw, qh),
        b: this._target(qw, qh),
      };
    }

    _blitScene(w, h) {
      const gl = this.gl;
      while (gl.getError() !== gl.NO_ERROR) { /* drain */ }
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, this.targets.scene.fbo);
      gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return gl.getError() === gl.NO_ERROR;
    }

    /* Copy the default framebuffer into the scene target, settling its
     * format on the first frame. */
    _copyScene(w, h) {
      if (this._blitScene(w, h)) {
        if (this.sceneRgb === null) this.sceneRgb = this.preferRgb;
        return true;
      }
      if (this.sceneRgb !== null) return false;
      /* The format the context's alpha flag suggests failed: rebuild the
       * copy in the other one and try once more. */
      const gl = this.gl;
      this.sceneRgb = !this.preferRgb;
      gl.deleteFramebuffer(this.targets.scene.fbo);
      gl.deleteTexture(this.targets.scene.tex);
      this.targets.scene = this._target(w, h, this.sceneRgb);
      return this._blitScene(w, h);
    }

    _pass(prog, src, dst) {
      const gl = this.gl;
      gl.bindFramebuffer(gl.FRAMEBUFFER, dst ? dst.fbo : null);
      gl.useProgram(prog);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, src.tex);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }

    /* Bloom the frame in the default framebuffer. `params` is
     * `[threshold, knee, intensity]` (`play_bloom_params`). */
    draw(params) {
      const gl = this.gl;
      const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      if (!(w > 0 && h > 0)) return false;
      this._ensure(w, h);
      const t = this.targets;
      const viewport = gl.getParameter(gl.VIEWPORT);
      const scissor = gl.isEnabled(gl.SCISSOR_TEST);
      gl.disable(gl.SCISSOR_TEST);
      gl.disable(gl.DEPTH_TEST);
      gl.depthMask(false);
      gl.disable(gl.CULL_FACE);
      gl.disable(gl.BLEND);

      /* The finished frame into a sampleable texture (resolving the
       * multisampled default framebuffer on the way). */
      if (!this._copyScene(w, h)) {
        gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        gl.enable(gl.DEPTH_TEST);
        gl.depthMask(true);
        if (scissor) gl.enable(gl.SCISSOR_TEST);
        throw new Error('bloom: the frame cannot be copied on this driver');
      }

      gl.bindVertexArray(this.vao);
      gl.viewport(0, 0, t.qw, t.qh);
      gl.useProgram(this.extract);
      gl.uniform1i(this.loc.exSrc, 0);
      gl.uniform4f(this.loc.exTexel, 1 / w, 1 / h, params[0], params[1]);
      this._pass(this.extract, t.scene, t.a);
      gl.useProgram(this.blur);
      gl.uniform1i(this.loc.blSrc, 0);
      for (let i = 0; i < BLUR_ROUNDS; i++) {
        gl.useProgram(this.blur);
        gl.uniform2f(this.loc.blStep, 1 / t.qw, 0);
        this._pass(this.blur, t.a, t.b);
        gl.uniform2f(this.loc.blStep, 0, 1 / t.qh);
        this._pass(this.blur, t.b, t.a);
      }

      /* Add the glow back over the frame. */
      gl.viewport(0, 0, w, h);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ONE);
      gl.blendEquation(gl.FUNC_ADD);
      gl.useProgram(this.composite);
      gl.uniform1i(this.loc.coSrc, 0);
      gl.uniform1f(this.loc.coIntensity, params[2]);
      this._pass(this.composite, t.a, null);

      gl.bindVertexArray(null);
      gl.disable(gl.BLEND);
      gl.enable(gl.DEPTH_TEST);
      gl.depthMask(true);
      gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
      if (scissor) gl.enable(gl.SCISSOR_TEST);
      return true;
    }

    dispose() {
      const gl = this.gl;
      this._free();
      gl.deleteProgram(this.extract);
      gl.deleteProgram(this.blur);
      gl.deleteProgram(this.composite);
      gl.deleteVertexArray(this.vao);
    }
  }

  window.LegaiaBloomPass = BloomPass;
})();
