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
 * A context without MSAA has nothing to avoid, and this does nothing on one,
 * until a cast needs a frame that outlives its draw or the frame is read out as
 * the 15-bit framebuffer (`begin`'s `scanout`, below).
 *
 * That readout is the copy's: the native window presents and captures PSX mode's
 * frame through `fs_scanout15` unless SSAO, bloom or the volumetric fog lays a
 * smooth term over it (`Renderer::scanout_15bit`), and `present(scanout15)` does
 * the same here.
 *
 * That frame is the one thing this target has that the canvas does not. A retail
 * battle frame's farthest bucket holds an opaque black rect, and an arm that arms
 * the wash has it hold a subtractive quad instead, so the frame's additive layers
 * sum over the frames before. Retail's earlier frame is the same display page two
 * game frames back, twice the running battle frame's span in vsyncs (eight at the
 * step a heavy arm runs at, ten under a streaming clip's overrun), so a ring of
 * pictures stands behind the target: while a cast module is resident every frame
 * takes the oldest slot, and a persisting frame loads the one the engine's depth
 * names (`play_cast_frame_persist_depth`, the native window's
 * `Renderer::set_frame_persist_depth`). The ring fills before the wash arms, so an
 * arm that arms it a pass late still builds on the frame before the arm
 * (`Renderer::set_frame_ring_keep`). The ring's size and the depth's clamp are the
 * shared kernel's (`render_kernels::frame_persist`), which the native renderer's
 * ring reads too; docs/subsystems/renderer-port.md says why.
 *
 * Classic script. */
(function () {
  'use strict';

  const VS = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID & 1) * 4 - 1), float((gl_VertexID & 2) * 2 - 1));
  gl_Position = vec4(p, 0.0, 1.0);
}`;
  /* The copy reads the frame out as the 15-bit framebuffer PSX mode models when
   * `u_scanout15` is set: each channel's 5-bit value `v >> 3` expanded
   * `(c5 << 3) | (c5 >> 2)`, the native renderer's `fs_scanout15` (twin of
   * engine-render's `psx_blend::scanout15`). PSX mode leaves every channel
   * inside its 5-bit bin, so a blended pixel's low bits are the background's,
   * not the display's; read raw, it stood up to seven levels off the window's. */
  const FS = `#version 300 es
precision highp float;
uniform sampler2D u_frame;
uniform int u_scanout15;
out vec4 o;
void main() {
  vec3 c = texelFetch(u_frame, ivec2(gl_FragCoord.xy), 0).rgb;
  if (u_scanout15 != 0) {
    vec3 c5 = floor(clamp(c, 0.0, 1.0) * 255.0 / 8.0 + 0.001);
    c = (c5 * 8.0 + floor(c5 / 4.0)) / 255.0;
  }
  o = vec4(c, 1.0);
}`;

  /* The ring's size and default depth when the bundle predates
   * `play_frame_ring_slots` / `play_cast_frame_persist_depth`: the kernel's
   * `DEFAULT_PERSIST_VSYNCS`, a fixed eight. */
  const FRAME_PERSIST_FALLBACK = 8;

  class PsxTarget {
    /* `ringSlots`: how many earlier frames the ring keeps (the kernel's
     * `FRAME_RING_SLOTS`, handed over by the engine). */
    constructor(gl, ringSlots) {
      this.gl = gl;
      this.ringSlots = ringSlots > 0 ? ringSlots | 0 : FRAME_PERSIST_FALLBACK;
      /* How many frames back a persisting frame reads (`setPersistDepth`). */
      this.persistDepth = Math.min(FRAME_PERSIST_FALLBACK, this.ringSlots);
      this.needed = !!gl.getContextAttributes().antialias;
      this.live = false;
      this.fbo = null;
      this.bindReal = null;
      this.broken = false;
      /* The ring of earlier frames, built by the first cast that may need it. */
      this.ring = [];
      this.ringNext = 0;
      this.ringFbo = null;
      /* The ring holds the frames before this one, kept while `keepLast` holds. */
      this.lastValid = false;
      this.ringKept = false;
    }

    /* Point `null` at this framebuffer and start the frame on an empty picture,
     * which is what the canvas's own buffer hands a frame after a composite.
     * `keepLast`: a cast module is resident, so this frame or a later one may ask
     * for the picture the last frame left (`loadEarlierFrame`). */
    begin(keepLast, scanout) {
      if (this.broken || (!this.needed && !keepLast && !scanout && !this.fbo)) return;
      const gl = this.gl;
      const hadFrame = !!this.fbo && this.w === gl.drawingBufferWidth && this.h === gl.drawingBufferHeight;
      this._ensure(gl.drawingBufferWidth, gl.drawingBufferHeight);
      this.live = true;
      this.bindReal(gl.FRAMEBUFFER, this.fbo);
      gl.disable(gl.SCISSOR_TEST);
      gl.colorMask(true, true, true, true);
      gl.depthMask(true);
      this.lastValid = !!keepLast && hadFrame && this._keepLast();
      if (!keepLast) this.ringKept = false;
      gl.clearBufferfv(gl.COLOR, 0, [0, 0, 0, 1]);
      gl.clearBufferfi(gl.DEPTH_STENCIL, 0, 1, 0);
    }

    /* How many presented frames back a persisting frame reads, already clamped
     * to the ring by the engine (`play_cast_frame_persist_depth`). */
    setPersistDepth(frames) {
      if (frames > 0) this.persistDepth = Math.min(frames | 0, this.ringSlots);
    }

    /* The cleared frame takes the picture of an earlier one instead, and `true`
     * says so; `false` leaves the caller to clear. The ring's newest slot,
     * `ringNext - 1`, is the last frame, so the one `persistDepth` back sits
     * `persistDepth` slots behind `ringNext` (`frame_persist::ring_read_slot`). */
    loadEarlierFrame() {
      if (!this.live || !this.lastValid) return false;
      const n = this.ring.length;
      const back = Math.min(this.persistDepth, n);
      this._copy(this.ring[(this.ringNext + n - back) % n], this.tex);
      return true;
    }

    /* Copy the frame to the canvas, with the GL state the frame left put back;
     * `scanout15` reads it out as the 15-bit framebuffer (the FS above). */
    present(scanout15) {
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
      gl.uniform1i(this.locScanout, scanout15 ? 1 : 0);
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

    /* A colour texture the size of the frame, with the frame's format. */
    _colorTexture(w, h) {
      const gl = this.gl;
      const unit = gl.getParameter(gl.ACTIVE_TEXTURE);
      gl.activeTexture(gl.TEXTURE0);
      const was = gl.getParameter(gl.TEXTURE_BINDING_2D);
      const tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, tex);
      /* RGB, as the canvas is (`alpha: false`): a read gives alpha 1, a blend sees 1. */
      gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGB8, w, h);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.bindTexture(gl.TEXTURE_2D, was);
      gl.activeTexture(unit);
      return tex;
    }

    /* One frame-sized picture onto another, with both framebuffer bindings left on
     * this target's framebuffer. */
    _copy(from, to) {
      const gl = this.gl;
      if (!this.ringFbo) this.ringFbo = [gl.createFramebuffer(), gl.createFramebuffer()];
      const [read, draw] = this.ringFbo;
      this.bindReal(gl.READ_FRAMEBUFFER, read);
      gl.framebufferTexture2D(gl.READ_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, from, 0);
      this.bindReal(gl.DRAW_FRAMEBUFFER, draw);
      gl.framebufferTexture2D(gl.DRAW_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, to, 0);
      const mask = gl.getParameter(gl.COLOR_WRITEMASK);
      const scissor = gl.isEnabled(gl.SCISSOR_TEST);
      if (scissor) gl.disable(gl.SCISSOR_TEST);
      gl.colorMask(true, true, true, true);
      gl.blitFramebuffer(0, 0, this.w, this.h, 0, 0, this.w, this.h, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      gl.colorMask(mask[0], mask[1], mask[2], mask[3]);
      if (scissor) gl.enable(gl.SCISSOR_TEST);
      this.bindReal(gl.FRAMEBUFFER, this.fbo);
    }

    /* The last frame, still on the target before this one clears it, takes the
     * oldest slot of the ring; a ring just started holds it in every slot, the
     * one earlier frame it has. `true` when it did. */
    _keepLast() {
      this._ensureRing();
      if (this.ringKept) {
        this._copy(this.tex, this.ring[this.ringNext]);
        this.ringNext = (this.ringNext + 1) % this.ring.length;
      } else {
        for (const slot of this.ring) this._copy(this.tex, slot);
        this.ringNext = 0;
        this.ringKept = true;
      }
      return true;
    }

    _ensureRing() {
      if (this.ring.length) return;
      for (let i = 0; i < this.ringSlots; i++) this.ring.push(this._colorTexture(this.w, this.h));
    }

    _dropRing() {
      const gl = this.gl;
      for (const t of this.ring) gl.deleteTexture(t);
      this.ring = [];
      this.lastValid = false;
      this.ringKept = false;
    }

    _ensure(w, h) {
      if (this.fbo && this.w === w && this.h === h) return;
      this._dropRing();
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
        this.locScanout = gl.getUniformLocation(this.program, 'u_scanout15');
        this.vao = gl.createVertexArray();
      }
      if (this.fbo) {
        gl.deleteFramebuffer(this.fbo);
        gl.deleteTexture(this.tex);
        gl.deleteRenderbuffer(this.depth);
      }
      this.w = w;
      this.h = h;
      this.tex = this._colorTexture(w, h);
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
        this.broken = true;
      }
      this.bindReal(gl.FRAMEBUFFER, null);
    }

    dispose() {
      if (!this.bindReal) return;
      const gl = this.gl;
      this.live = false;
      this._dropRing();
      if (this.ringFbo) for (const f of this.ringFbo) gl.deleteFramebuffer(f);
      this.ringFbo = null;
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
