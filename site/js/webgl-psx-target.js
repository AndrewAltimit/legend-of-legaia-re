/* webgl-psx-target.js - PSX rasterisation's own single-sample framebuffer.
 *
 * The play page's context is created with MSAA and a context cannot change that
 * afterwards, and WebGL has no switch that turns multisampling off on the canvas's
 * framebuffer. Retail's rasteriser has no partial coverage, and the native
 * renderer's targets are all single-sample, so a PSX frame drawn on the canvas
 * comes out with blended edge pixels neither has. PSX rasterisation therefore
 * draws here, and the finished frame is copied to the canvas once.
 *
 * The passes keep binding `null` for "the screen". While a frame is in here, that
 * resolves to this framebuffer and reads back as `null`, which also covers every read of the screen in the
 * frame (the battle capture, the photo grab, the fog's depth copy, bloom's scene
 * copy): each reads what the frame drew. `present` leaves the canvas bound and
 * `null` meaning the canvas again, so a read after the frame reads the canvas.
 *
 * It is the copy that needs a draw: a blit into a multisampled framebuffer is an
 * error. Every sample of a pixel covered by one full-screen triangle holds that
 * pixel's colour, so the resolve hands back the same bytes.
 *
 * A context without MSAA has nothing to avoid, and this does nothing on one.
 *
 * Classic script. */
(function () {
  'use strict';

  const VS = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID & 1) * 4 - 1), float((gl_VertexID & 2) * 2 - 1));
  gl_Position = vec4(p, 0.0, 1.0);
}`;
  const FS = `#version 300 es
precision highp float;
uniform sampler2D u_frame;
out vec4 o;
void main() { o = vec4(texelFetch(u_frame, ivec2(gl_FragCoord.xy), 0).rgb, 1.0); }`;

  class PsxTarget {
    constructor(gl) {
      this.gl = gl;
      this.needed = !!gl.getContextAttributes().antialias;
      this.live = false;
      this.fbo = null;
      this.bindReal = null;
    }

    /* Point `null` at this framebuffer and start the frame on an empty picture,
     * which is what the canvas's own buffer hands a frame after a composite. */
    begin() {
      if (!this.needed) return;
      const gl = this.gl;
      this._ensure(gl.drawingBufferWidth, gl.drawingBufferHeight);
      this.live = true;
      this.bindReal(gl.FRAMEBUFFER, this.fbo);
      gl.disable(gl.SCISSOR_TEST);
      gl.colorMask(true, true, true, true);
      gl.depthMask(true);
      gl.clearBufferfv(gl.COLOR, 0, [0, 0, 0, 1]);
      gl.clearBufferfi(gl.DEPTH_STENCIL, 0, 1, 0);
    }

    /* Copy the frame to the canvas, with the GL state the frame left put back. */
    present() {
      if (!this.live) return;
      this.live = false;
      const gl = this.gl;
      const on = [gl.BLEND, gl.DEPTH_TEST, gl.CULL_FACE, gl.SCISSOR_TEST, gl.STENCIL_TEST,
        gl.SAMPLE_ALPHA_TO_COVERAGE, gl.SAMPLE_COVERAGE, gl.RASTERIZER_DISCARD]
        .filter((cap) => gl.isEnabled(cap));
      const program = gl.getParameter(gl.CURRENT_PROGRAM);
      const vao = gl.getParameter(gl.VERTEX_ARRAY_BINDING);
      const unit = gl.getParameter(gl.ACTIVE_TEXTURE);
      gl.activeTexture(gl.TEXTURE0);
      const tex = gl.getParameter(gl.TEXTURE_BINDING_2D);
      const sampler = gl.getParameter(gl.SAMPLER_BINDING);
      const viewport = gl.getParameter(gl.VIEWPORT);
      const mask = gl.getParameter(gl.COLOR_WRITEMASK);
      this.bindReal(gl.FRAMEBUFFER, null);
      for (const cap of on) gl.disable(cap);
      gl.colorMask(true, true, true, true);
      gl.viewport(0, 0, this.w, this.h);
      gl.useProgram(this.program);
      gl.bindVertexArray(this.vao);
      gl.bindSampler(0, null);
      gl.bindTexture(gl.TEXTURE_2D, this.tex);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.bindSampler(0, sampler);
      gl.activeTexture(unit);
      gl.bindVertexArray(vao);
      gl.useProgram(program);
      gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
      gl.colorMask(mask[0], mask[1], mask[2], mask[3]);
      for (const cap of on) gl.enable(cap);
    }

    _ensure(w, h) {
      if (this.fbo && this.w === w && this.h === h) return;
      const gl = this.gl;
      if (!this.bindReal) {
        const real = gl.bindFramebuffer.bind(gl);
        this.bindReal = real;
        /* An own property over the prototype's method: only this context, and only
         * once a PSX frame has been asked for. */
        gl.bindFramebuffer = (target, fb) => real(target, fb === null && this.live ? this.fbo : fb);
        /* And it reads back as `null`: a pass that asks whether it is on the screen
         * (ambient occlusion, which stands down in a VR layer) must get a yes. */
        const realGet = gl.getParameter.bind(gl);
        gl.getParameter = (p) => {
          const v = realGet(p);
          return this.live && v === this.fbo ? null : v;
        };
        this.program = gl.createProgram();
        for (const [type, src] of [[gl.VERTEX_SHADER, VS], [gl.FRAGMENT_SHADER, FS]]) {
          const sh = gl.createShader(type);
          gl.shaderSource(sh, src);
          gl.compileShader(sh);
          gl.attachShader(this.program, sh);
          gl.deleteShader(sh);
        }
        gl.linkProgram(this.program);
        this.vao = gl.createVertexArray();
      }
      if (this.fbo) {
        gl.deleteFramebuffer(this.fbo);
        gl.deleteTexture(this.tex);
        gl.deleteRenderbuffer(this.depth);
      }
      this.w = w;
      this.h = h;
      const unit = gl.getParameter(gl.ACTIVE_TEXTURE);
      gl.activeTexture(gl.TEXTURE0);
      const was = gl.getParameter(gl.TEXTURE_BINDING_2D);
      this.tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, this.tex);
      /* RGB, as the canvas is (`alpha: false`): a read gives alpha 1, a blend sees 1. */
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGB8, w, h);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.bindTexture(gl.TEXTURE_2D, was);
      gl.activeTexture(unit);
      this.depth = gl.createRenderbuffer();
      gl.bindRenderbuffer(gl.RENDERBUFFER, this.depth);
      /* The format browsers back the canvas's depth with: the fog pass copies it. */
      gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH24_STENCIL8, w, h);
      gl.bindRenderbuffer(gl.RENDERBUFFER, null);
      this.fbo = gl.createFramebuffer();
      this.bindReal(gl.FRAMEBUFFER, this.fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.tex, 0);
      gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_STENCIL_ATTACHMENT, gl.RENDERBUFFER, this.depth);
      if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
        console.warn('psx target: incomplete framebuffer; drawing on the canvas');
        this.needed = false;
      }
      this.bindReal(gl.FRAMEBUFFER, null);
    }

    dispose() {
      if (!this.bindReal) return;
      const gl = this.gl;
      this.live = false;
      delete gl.bindFramebuffer;
      delete gl.getParameter;
      gl.deleteFramebuffer(this.fbo);
      gl.deleteTexture(this.tex);
      gl.deleteRenderbuffer(this.depth);
      gl.deleteProgram(this.program);
      gl.deleteVertexArray(this.vao);
    }
  }

  window.LegaiaPsxTarget = PsxTarget;
})();
