/* Play the port in the browser.
 *
 * The WASM side (`LegaiaRuntime`) is the *engine*: a real `SceneHost` running
 * the field VM, the free-movement controller against the scene's walkability
 * grid, floor-height sampling, NPC motion, the interaction probe, the dialogue
 * runner. This module is the *shell*: keyboard -> PSX pad word, one engine tick
 * per animation frame, and a draw of whatever the engine reports.
 *
 * Rendering reuses the shared `TmdRenderer` scene path (the same plumbing the
 * world-overview continents and the viewer's full-map button run on):
 *
 *   - static map   - terrain tiles + placed objects + the ground heightfield,
 *                    uploaded once per scene;
 *   - player       - one scene mesh whose positions are re-uploaded each frame
 *                    from the engine's live pose (idle / walk locomotion clip);
 *   - NPCs         - one scene mesh each, posed from the scene's ANM bundle and
 *                    drawn at the world's live NPC position / heading.
 *
 * Requires webgl-math.js + webgl-shaders.js + webgl-tmd.js to be loaded first.
 *
 * The page's script is split by job. This file holds the screen-space
 * primitive pass (ScreenPrimPass and its shaders) and declares PlayView - its
 * construction, scene entry, the run loop, audio and teardown. The rest of
 * the class body is installed onto it by the files the page loads directly
 * after (LegaiaUtil.installMethods, site-util.js):
 *
 *   play-view-scene-build.js the scene upload on entry: static map, ground,
 *                            player, NPCs, tile board
 *   play-view-input.js       keyboard / pad input, the pause menu, name
 *                            entry and shop sessions, the fishing toggle
 *   play-view-overlay.js     the 2D overlay canvas (menus, HUD, text wash)
 *   play-view-field-sync.js  per-frame field state the engine reports:
 *                            placement windows, floor waves, object moves
 *   play-view-frame.js       _frame, the staged frame state, screen
 *                            primitives, the fog volume, field effects
 *   play-view-battle.js      the battle branch of the frame
 *
 * Loaded before this file: play-view-constants.js (mesh-id spaces, VR scale,
 * pad aliases, occlusion geometry) and play-atlas-blitter.js (AtlasBlitter).
 */
(function () {
  'use strict';

  const {
    adoptPadBindings, VR_FP_FALLBACK_MESH_HEIGHT, VR_UNITS_PER_METER,
    PLAYER_MESH_ID, VAHN_HEIGHT_M, VR_FP_EYE_HEIGHT_M, A2R,
  } = window.LegaiaPlayShared;

  /* ------------------------------------------------------------------ */
  /* Screen-space PSX primitive pass (2D `POLY_FT4` / `POLY_GT4` quads).   */
  /* ------------------------------------------------------------------ */

  /* The 2D half of the PSX GPU, which this page did not have.
   *
   * The engine hands over a vertex/index/run triple built by the SHARED
   * `legaia_render_kernels::screen_prim` builder - the same code the native window
   * feeds to wgpu. Positions arrive already in NDC and the triangles already
   * in ordering-table order (farthest OT bucket first, LIFO within a bucket),
   * so there is nothing here to project and nothing here to sort: this class
   * uploads three arrays and issues one draw per run.
   *
   * The fragment path is the 3D VRAM-mesh decode with the texture-window
   * remap dropped (screen sprites never use GP0(E2)): 4/8/15-bpp texture
   * pages out of the same 1024x512 R16UI VRAM texture, CLUT lookup through
   * CBA, BGR555 -> RGBA, `0x0000` discarded. `flags` bit 0 picks textured vs
   * flat; `a_color` is a `/128` modulation factor on the textured path and a
   * straight `/255` colour on the flat one.
   *
   * The per-run blend state is NOT a table in this file - it is
   * `TmdRenderer._setSemiBlend`, the same four ABR equations the 3D blend
   * pass binds. A second copy here is precisely how the two halves of one
   * GPU model drift apart.
   *
   * See docs/tooling/host-drift.md for what this pass can and cannot draw. */
  const VS_SCREEN_PRIM = `#version 300 es
precision highp float;
precision highp int;

in vec2 a_pos;          /* already NDC - built by the shared Rust builder */
in vec2 a_uv;           /* texel coordinates inside the texture page */
in uvec2 a_cba_tsb;     /* GP0 CLUT base + texpage words */
in vec4 a_color;        /* textured: /128 factor. flat: /255 colour. */
in uint a_flags;        /* bit 0 = textured, bit 1 = depth-tested */
in float a_depth;       /* scene depth, read only with flags bit 1 */

out vec2 v_uv;
flat out uvec2 v_cba_tsb;
out vec4 v_color;
flat out uint v_flags;
out float v_log_depth;

/* Log depth (LOG_DEPTH_GLSL in webgl-shaders.js): x = on, yz = the (a, b)
 * of ndc = a + b / w the engine projected these corners' depths through
 * (play_fx_depth_affine), so a corner's depth goes back to its exact w and
 * onto the scale the mesh program writes. x = 0 keeps the rasterised depth. */
uniform vec3 u_log_depth;
${LOG_DEPTH_GLSL}

void main() {
  v_uv = a_uv;
  v_cba_tsb = a_cba_tsb;
  v_color = a_color;
  v_flags = a_flags;
  /* A depth-tested quad (the overworld markers) carries the shared
   * view-projection's clip z / w - the value the 3D pass's gl_Position
   * produces for the same world point - so it hides behind nearer terrain.
   * Every other quad sits on the near plane (-1) and passes against any
   * scene depth, which is how it composited before the channel existed. */
  float z = (a_flags & 2u) != 0u ? a_depth : -1.0;
  gl_Position = vec4(a_pos, z, 1.0);
  v_log_depth = 0.0;
  if (u_log_depth.x > 0.5 && (a_flags & 2u) != 0u) {
    float den = a_depth - u_log_depth.y;
    v_log_depth = abs(den) > 0.0 ? logDepthOfW(u_log_depth.z / den) : 1.0;
    /* The far plane itself (the overworld sky band) is the clear value
     * exactly, so it lands only where the scene drew nothing - including
     * in front of nothing in the far-bucket slice just short of it. */
    if (a_depth >= 1.0) v_log_depth = 1.0;
  }
}
`;

  const FS_SCREEN_PRIM = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;

in vec2 v_uv;
flat in uvec2 v_cba_tsb;
in vec4 v_color;
flat in uint v_flags;
in float v_log_depth;

out vec4 o_color;

uniform vec3 u_log_depth;
/* Which half of a semi-transparent run this draw is (PSX semi-transparency
 * is per texel: inside an ABE prim only the STP-set texels blend, an
 * STP-clear texel draws opaque; an untextured ABE prim blends every pixel).
 * 0 = an opaque run, everything draws; 1 = the semi run's opaque half, only
 * the textured prims' STP-clear texels; 2 = its blend half, the rest. */
uniform int u_stp_pass;

vec4 bgr555_to_rgba(uint c) {
  float r = float(c & 31u) / 31.0;
  float g = float((c >> 5u) & 31u) / 31.0;
  float b = float((c >> 10u) & 31u) / 31.0;
  uint stp = (c >> 15u) & 1u;
  float a = (c == 0u && stp == 0u) ? 0.0 : 1.0;
  return vec4(r, g, b, a);
}

uint fetch_vram_word(vec2 uv, uint cba, uint tsb) {
  uint u_pix = uint(max(uv.x, 0.0)) & 255u;
  uint v_pix = uint(max(uv.y, 0.0)) & 255u;
  uint tpage_x = (tsb & 15u) * 64u;
  uint tpage_y = ((tsb >> 4u) & 1u) * 256u;
  uint depth   = (tsb >> 7u) & 3u;   /* 0=4bpp, 1=8bpp, 2=15bpp */
  if (depth == 0u) {
    int vx = int(tpage_x + (u_pix >> 2u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint nibble = u_pix & 3u;
    uint pal_idx = (word >> (nibble * 4u)) & 15u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    return texelFetch(u_vram, ivec2(cx, cy), 0).r;
  } else if (depth == 1u) {
    int vx = int(tpage_x + (u_pix >> 1u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint byte_sel = u_pix & 1u;
    uint pal_idx = (word >> (byte_sel * 8u)) & 255u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    return texelFetch(u_vram, ivec2(cx, cy), 0).r;
  }
  int vx = int(tpage_x + u_pix);
  int vy = int(tpage_y + v_pix);
  return texelFetch(u_vram, ivec2(vx, vy), 0).r;
}

/* The PSX texture blend at the framebuffer's 5-bit depth - the twin of
 * engine-render's psx_texture_blend (legaia_render_kernels::screen_prim::
 * psx_texture_blend): (t5 * c8) >> 7, saturated at 31, so a dim packet
 * colour drops a faint texel to zero. Exact at the neutral 0x80. It is what
 * keeps the additive overworld fog sheets sparse. */
vec3 psxTextureBlend(vec3 texel, vec3 factor) {
  uvec3 t5 = uvec3(round(texel * 31.0));
  uvec3 c8 = uvec3(round(clamp(factor, vec3(0.0), vec3(255.0 / 128.0)) * 128.0));
  return vec3(min((t5 * c8) >> 7u, uvec3(31u))) / 31.0;
}

void main() {
  gl_FragDepth = (u_log_depth.x > 0.5 && (v_flags & 2u) != 0u)
    ? v_log_depth : gl_FragCoord.z;
  if ((v_flags & 1u) != 0u) {
    uint word = fetch_vram_word(v_uv, v_cba_tsb.x, v_cba_tsb.y);
    if (word == 0u) discard;
    bool stp = (word & 0x8000u) != 0u;
    if ((u_stp_pass == 1 && stp) || (u_stp_pass == 2 && !stp)) discard;
    vec4 texel = bgr555_to_rgba(word);
    o_color = vec4(psxTextureBlend(texel.rgb, v_color.rgb), v_color.a * texel.a);
  } else {
    if (u_stp_pass == 1) discard;
    o_color = v_color;
  }
}
`;

  /* The text-layer composite (`ScreenPrimPass.drawLayer`): one oversized
   * triangle over the viewport, sampling the 2D canvas texel for texel. */
  const VS_LAYER = `#version 300 es
out vec2 v_uv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  v_uv = p;
  gl_Position = vec4(p * 2.0 - 1.0, -1.0, 1.0);
}
`;
  const FS_LAYER = `#version 300 es
precision mediump float;
uniform sampler2D u_layer;
in vec2 v_uv;
out vec4 o_color;
void main() { o_color = texture(u_layer, v_uv); }
`;

  /* Byte layout of `legaia_render_kernels::screen_prim::ScreenVertex`. Pinned on
   * the Rust side by `the_vertex_field_offsets_match_the_layout_hosts_read_as_bytes`
   * so a field reorder fails a test rather than a shader. */
  const SCREEN_VERTEX_STRIDE = 48;
  const SCREEN_VERTEX_OFF_POS = 0;
  const SCREEN_VERTEX_OFF_UV = 8;
  const SCREEN_VERTEX_OFF_CBA_TSB = 16;
  const SCREEN_VERTEX_OFF_COLOR = 24;
  const SCREEN_VERTEX_OFF_FLAGS = 40;
  const SCREEN_VERTEX_OFF_DEPTH = 44;

  class ScreenPrimPass {
    /* `renderer` is the live TmdRenderer: this pass borrows its GL context,
     * its uploaded VRAM texture and its ABR blend table rather than keeping
     * a second copy of any of the three. */
    constructor(renderer) {
      const gl = renderer.gl;
      this.renderer = renderer;
      this.gl = gl;
      this.program = compileScreenPrimProgram(gl);
      this.locVram = gl.getUniformLocation(this.program, 'u_vram');
      this.locLogDepth = gl.getUniformLocation(this.program, 'u_log_depth');
      this.locStpPass = gl.getUniformLocation(this.program, 'u_stp_pass');
      this.vao = gl.createVertexArray();
      this.vbo = gl.createBuffer();
      this.ibo = gl.createBuffer();
      gl.bindVertexArray(this.vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      const s = SCREEN_VERTEX_STRIDE;
      /* A driver is free to optimise an attribute away and hand back -1;
       * `enableVertexAttribArray(-1)` is an INVALID_VALUE, so bind by name and
       * skip the missing ones rather than taking the pass down. */
      const bindF = (name, size, off) => {
        const l = gl.getAttribLocation(this.program, name);
        if (l < 0) return;
        gl.enableVertexAttribArray(l);
        gl.vertexAttribPointer(l, size, gl.FLOAT, false, s, off);
      };
      const bindI = (name, size, off) => {
        const l = gl.getAttribLocation(this.program, name);
        if (l < 0) return;
        gl.enableVertexAttribArray(l);
        gl.vertexAttribIPointer(l, size, gl.UNSIGNED_INT, s, off);
      };
      bindF('a_pos', 2, SCREEN_VERTEX_OFF_POS);
      bindF('a_uv', 2, SCREEN_VERTEX_OFF_UV);
      bindI('a_cba_tsb', 2, SCREEN_VERTEX_OFF_CBA_TSB);
      bindF('a_color', 4, SCREEN_VERTEX_OFF_COLOR);
      bindI('a_flags', 1, SCREEN_VERTEX_OFF_FLAGS);
      bindF('a_depth', 1, SCREEN_VERTEX_OFF_DEPTH);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibo);
      gl.bindVertexArray(null);
    }

    /* Upload one frame's screen primitives. `vertexBytes` is a Uint8Array in
     * the ScreenVertex layout, `indices` a Uint32Array, `runs` a Uint32Array
     * of `[class_code, index_start, index_count]` triples where class_code 0
     * is opaque and `1 + abr` is semi-transparent. The runs are drawn by
     * `drawRuns`, in two halves with the page's text layer between them. */
    upload(vertexBytes, indices, runs, depthAffine) {
      const gl = this.gl;
      gl.bindVertexArray(this.vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      gl.bufferData(gl.ARRAY_BUFFER, vertexBytes, gl.STREAM_DRAW);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibo);
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STREAM_DRAW);
      gl.bindVertexArray(null);
      this.runs = runs;
      this.depthAffine = depthAffine;
    }

    /* Draw runs `[from, to)` of the uploaded frame over whatever is already
     * in the framebuffer. */
    drawRuns(from, to) {
      const gl = this.gl;
      const runs = this.runs;
      const depthAffine = this.depthAffine;
      if (!runs || to <= from) return;
      gl.bindVertexArray(this.vao);
      gl.useProgram(this.program);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.renderer.tex);
      gl.uniform1i(this.locVram, 0);
      /* The log-depth scale the mesh program wrote this frame: a depth-tested
       * corner goes back to its w through the engine's own (a, b). */
      if (this.locLogDepth) {
        const on = this.renderer.lastLogDepth && depthAffine && depthAffine.length === 2;
        gl.uniform3f(this.locLogDepth, on ? 1 : 0,
          on ? depthAffine[0] : 0, on ? depthAffine[1] : 0);
      }
      /* Retail screen-space packets carry no depth: they composite in
       * ordering-table order over the finished scene, on the near plane, so
       * the test always passes for them. The test is armed (no write) for
       * the one kind that does carry a depth - the overworld markers, which
       * must hide behind the terrain the frame already drew. */
      const depthWasOn = gl.isEnabled(gl.DEPTH_TEST);
      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.depthMask(false);
      gl.disable(gl.CULL_FACE);
      let blendOn = false;
      for (let i = from * 3; i + 2 < runs.length && i < to * 3; i += 3) {
        const code = runs[i], start = runs[i + 1], count = runs[i + 2];
        if (!count) continue;
        if (code === 0) {
          if (blendOn) { gl.disable(gl.BLEND); blendOn = false; }
          gl.uniform1i(this.locStpPass, 0);
        } else {
          /* A semi run draws twice: its STP-clear texels solid, then the
           * blend for the rest (the native renderer's draw_runs). */
          if (blendOn) { gl.disable(gl.BLEND); blendOn = false; }
          gl.uniform1i(this.locStpPass, 1);
          gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, start * 4);
          gl.enable(gl.BLEND); blendOn = true;
          /* One ABR table on this page, and it lives in webgl-tmd.js. */
          this.renderer._setSemiBlend((code - 1) & 3);
          gl.uniform1i(this.locStpPass, 2);
        }
        gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, start * 4);
      }
      if (blendOn) {
        gl.disable(gl.BLEND);
        gl.blendEquation(gl.FUNC_ADD);
      }
      gl.depthMask(true);
      if (!depthWasOn) gl.disable(gl.DEPTH_TEST);
      gl.bindVertexArray(null);
    }

    /* Composite a 2D canvas (the page's text / HUD layer) into the GL frame,
     * pixel for pixel over the whole viewport. Used only on a frame whose
     * `over` list is not empty: those primitives draw AFTER the text (a warp
     * fade, the pause wipe, the cinematic bars cover it, as on the native
     * window), which a DOM canvas stacked above the GL view cannot be under.
     * The canvas holds premultiplied colour, so the blend is `ONE,
     * ONE_MINUS_SRC_ALPHA`. */
    drawLayer(canvas) {
      const gl = this.gl;
      if (!this.layerProgram) {
        this.layerProgram = compileProgramPair(gl, VS_LAYER, FS_LAYER, 'layer');
        this.locLayer = gl.getUniformLocation(this.layerProgram, 'u_layer');
        this.layerTex = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, this.layerTex);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      }
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.layerTex);
      /* The unpack flags are context state every other upload on the page
       * relies on being off: set for this one call, restored after it. */
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, canvas);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.bindVertexArray(null);
      gl.useProgram(this.layerProgram);
      gl.uniform1i(this.locLayer, 0);
      const depthWasOn = gl.isEnabled(gl.DEPTH_TEST);
      gl.disable(gl.DEPTH_TEST);
      gl.depthMask(false);
      gl.disable(gl.CULL_FACE);
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.disable(gl.BLEND);
      gl.depthMask(true);
      if (depthWasOn) gl.enable(gl.DEPTH_TEST);
    }

    dispose() {
      const gl = this.gl;
      gl.deleteProgram(this.program);
      if (this.layerProgram) gl.deleteProgram(this.layerProgram);
      if (this.layerTex) gl.deleteTexture(this.layerTex);
      gl.deleteVertexArray(this.vao);
      gl.deleteBuffer(this.vbo);
      gl.deleteBuffer(this.ibo);
    }
  }

  function compileProgramPair(gl, vsSrc, fsSrc, tag) {
    const mk = (type, src, kind) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
        throw new Error(tag + ' ' + kind + ': ' + gl.getShaderInfoLog(s));
      }
      return s;
    };
    const v = mk(gl.VERTEX_SHADER, vsSrc, 'VS');
    const f = mk(gl.FRAGMENT_SHADER, fsSrc, 'FS');
    const p = gl.createProgram();
    gl.attachShader(p, v);
    gl.attachShader(p, f);
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      throw new Error(tag + ' link: ' + gl.getProgramInfoLog(p));
    }
    gl.deleteShader(v);
    gl.deleteShader(f);
    return p;
  }

  function compileScreenPrimProgram(gl) {
    return compileProgramPair(gl, VS_SCREEN_PRIM, FS_SCREEN_PRIM, 'screen-prim');
  }

  class PlayView {
    /* `runtime` is the WASM LegaiaRuntime (disc already loaded); `canvas` an
     * unused <canvas> in the DOM. `opts.onState` fires once per frame with the
     * engine's state JSON (already parsed) for the HUD. */
    constructor(runtime, canvas, opts) {
      this._fieldFx = { fxMeshes: new Map(), packMeshes: new Map() };
      this._dynActors = new Set();
      if (typeof window.TmdRenderer === 'undefined') {
        throw new Error('TmdRenderer global missing (webgl-tmd.js not loaded?)');
      }
      this.rt = runtime;
      /* One binding table, read from the engine rather than typed here. */
      adoptPadBindings(runtime);
      this.canvas = canvas;
      this.renderer = new window.TmdRenderer(canvas);
      /* Every scene mesh carries its primitives' corners for retail's
       * per-primitive near reject, built by the engine's kernel (read
       * through this.rt so a recovered runtime keeps serving it). */
      this.renderer.primRefsFn = (p, i) => (this.rt && typeof this.rt.prim_corner_refs === 'function'
        ? this.rt.prim_corner_refs(p, i) : null);
      this.opts = opts || {};
      this.raf = 0;
      this.paused = false;
      this.stepOnce = false;
      this.pad = 0;
      this.held = new Set();
      /* Keys that went down since the last engine tick. A tap shorter than one
       * frame (the browser delivers keydown+keyup between two animation frames)
       * would otherwise be sampled as "never pressed" - and the engine's
       * just-pressed edge, which is what talking to an NPC rides on, would never
       * fire. Latching the edge here means a tap always lands on exactly one
       * tick, however fast it was. */
      this.pulse = new Set();
      this.scene = null;
      this.staticDraws = [];
      this._terrainByIdx = null;
      this._terrainFacingLive = null;
      this.player = null;    /* { basePositions } */
      this.npcs = [];        /* [{ meshId, base, objectIds, frames, partCount, frameCount, out }] */
      this.tileMeshSlots = [];
      this.tileActorSlots = new Set();
      /* Animated environment props (a placement whose object bind names a
       * clip: the Rim Elm windmill, swinging house doors). Each gets its own
       * mesh instance so its clip advances independently; re-posed per frame
       * from the engine's live prop-bank cursor. See `_frame`. */
      this.animProps = [];   /* [{ meshId, i, slot, anim, lastFrame }] */
      /* Battle 3D scene, `{ gen, scale, backdrop, ground, actors }` while the
       * engine is in a battle whose render state built (see `_battleFrame`);
       * null otherwise. `_camBeforeBattle` restores the field framing after. */
      this._battle = null;
      this._camBeforeBattle = null;
      /* Screen-space PSX primitive pass (the 2D half of the GPU), built on
       * the first frame that carries a primitive. `_screenPrimBroken` latches
       * a driver that could not link it, so a failure costs the effect and
       * not the session. */
      this._screenPrims = null;
      this._screenPrimBroken = false;
      /* Camera-occlusion fade (see-through walls) - the browser twin of the
       * native play-window's default-on enhancement: field geometry between
       * the camera and the player screen-doors open around the character
       * (per-fragment, in the shared FS - not a per-body cull, which
       * blinked neighbouring bodies out). The page's checkbox drives setOcclusionFade;
       * the focus itself is staged per field frame in `_frame` and never
       * during battle or VR first-person. */
      this.occlusionFade = true;
      /* Retail pause menu (Start): the state + navigation live in the engine
       * (`LegaiaRuntime::play_menu_*`), which serves the byte-pinned window
       * chrome + font glyphs as `{ dst, src, color }` quads. This page owns only
       * the two atlas textures + a 2D overlay canvas the quads blit onto; the
       * field freezes while the menu is up (retail's `SceneMode::Menu`). */
      this.menuOverlay = (opts && opts.menuOverlay) || null;   /* <canvas> over the GL view */
      this._menuCtx = null;
      this._menuFont = null;      /* AtlasBlitter for the dialog-font atlas */
      this._menuChrome = undefined;  /* AtlasBlitter | null (null once resolved to "no chrome") */
      this._overlayActive = false;
      /* Whether this session CAN draw the retail reading box on the overlay
       * canvas: the engine exports the draw builder and the font atlas is
       * uploaded. The page's DOM `.play-dialog` fallback keys off this.
       *
       * A *capability*, deliberately, not a per-frame "did we draw it". The
       * flag used to be the latter, and it produced a visible defect: the HUD
       * DOM update runs earlier in `_frame` than `_drawOverlay`, so on the
       * frame a box opened the page read last frame's value (false), unhid the
       * DOM box - which CSS anchors at `bottom: 5%` - and only hid it again a
       * frame later, once the canvas had drawn the real box at the TOP
       * (`dialog_reading_box_layout`: `(0x26, 0x10, ...)`, FUN_801D84D0). One
       * frame of an empty-looking window at the bottom, on every box open and
       * every page break. A capability cannot race a draw: it settles once,
       * before the first conversation, and never flickers. */
      this.dialogCanvasCapable = false;
      /* Last engine state handed to the HUD - lets the menu gate on Field
       * mode / no-dialog before opening. */
      this._hudState = null;
      /* Follow camera. `halfWidth` is the ortho-equivalent half-window the shared
       * orbit projection consumes (smaller = closer); 520 frames a ~130-unit
       * character at roughly the on-screen height retail's follow camera gives
       * them. */
      this.cam = {
        centerX: 0, centerY: 0, centerZ: 0,
        halfWidth: 520, halfHeight: 520,
        yaw: 0, pitch: 0.62,
      };
      /* Debug orbit vantage, the browser twin of the native window's `F3`
       * (`PlayWindowApp::field_debug_camera`): off by default, so the page's
       * default projection is the ENGINE camera - the retail follow view, the
       * op-0x45 cutscene shot, the overworld walk view or the world map's
       * top-view debug camera, whichever `camera_view::resolve_field_camera`
       * says owns the frame. On, the page frames the scene with the orbit
       * above instead, which is the wide vantage for eyeballing scene
       * completeness. `cam.vp` is what selects between them: the shared
       * projection sets it, the debug orbit clears it. */
      this.debugCamera = false;
      this.fps = 0;
      this._fpsAccum = 0;
      this._fpsFrames = 0;
      this._fpsLast = performance.now();
      /* Fixed-timestep sim clock. The engine's field / motion VMs are authored
       * for a fixed 60 Hz tick (retail's vsync-locked field loop; the native
       * window drives it off a wall-clock accumulator, `Window::drain_ticks`,
       * `TICK_DT = 1/60`). requestAnimationFrame fires at the DISPLAY refresh -
       * 120/144 Hz on a high-refresh monitor - so ticking once per rAF ran the
       * whole world (NPC walkers included) at 2-2.4x speed, which is the "NPCs
       * zip around" symptom. Accumulate real elapsed time and run only as many
       * 1/60 s ticks as have actually elapsed (capped so a stall can't spiral). */
      this._simAccum = 0;
      this._simLast = performance.now();
      /* Last frame's draw list + world extent, kept on the instance so the VR
       * loop can re-issue the same draw once per eye without re-ticking the
       * engine. */
      this._draws = [];
      this._ext = [16384, 16384];
      this._attachInput();

      /* Measured standing height of the player mesh (world units), refreshed
       * per scene in `_rebuild`; drives the first-person world scale. */
      this.playerHeight = VR_FP_FALLBACK_MESH_HEIGHT;
      /* While the VR first-person mode is live: `_vrFp` filters the player
       * mesh out of the eye draws (you are inside it) and disables the
       * occluder cull (there is no third-person lens to occlude); `_vrDrive`
       * carries this frame's VR-stick pad word + gaze azimuth into the engine
       * tick. */
      this._vrFp = false;
      this._vrDrive = null;
      this._vrPrecise = false;

      /* VR: walk the live scene in a headset. The engine keeps ticking (the XR
       * frame loop drives it), so NPCs move and the keyboard still steers the
       * character. Two viewing modes (toggle button next to Enter VR):
       *   - Spectator: the headset is a free-flying camera in the running
       *     world, spawned where the follow camera sits.
       *   - First-person: the rig is anchored at the player's position at eye
       *     height ("what Vahn sees"); the left stick drives the REAL player
       *     through the engine's collision / walkability grid.
       * The button is always visible; without an immersive-vr device it reads
       * "VR unavailable" and click / hover explain why. */
      this.vr = window.LegaiaVr ? window.LegaiaVr.attach({
        mount: (this.opts.vrMount || document.querySelector('.play-btn-row')
          || canvas.parentElement),
        unitsPerMeter: VR_UNITS_PER_METER,
        renderer: () => this.renderer,
        cam: () => this.cam,
        extent: () => this._ext,
        /* The follow camera owns cam.center* every frame - don't fight it. */
        syncCamCenter: false,
        update: () => this._frame(true),
        draw: () => {
          const draws = this._vrFp
            ? this._draws.filter(d => d.meshId !== PLAYER_MESH_ID)
            : this._draws;
          this.renderer.renderAssembled(draws, this._ext, this.cam);
        },
        modes: [
          /* Spawn where the third-person camera sits (behind the character,
           * looking at them), feet on the character's floor. */
          {
            id: 'spectator', label: 'Spectator',
            unitsPerMeter: VR_UNITS_PER_METER,
            start: () => {
              const eye = this._eye();
              const pt = this.rt.player_transform();
              return {
                x: eye[0], y: -pt[1], z: eye[2],
                yaw: Math.PI - this.cam.yaw,
              };
            },
          },
          /* First-person: floor origin pinned to the player's feet, world
           * scaled so the measured mesh height reads as a 1.7 m adult. The
           * spawn faces the player's current heading (engine heading 0 =
           * travelling +Z = world dir (sin, cos); the rig's yaw 0 faces -Z
           * through the mirrored world transform, hence the half-turn). */
          {
            id: 'first-person', label: 'First-person',
            unitsPerMeter: () => this._measurePlayerHeight() / VAHN_HEIGHT_M,
            eyeHeightHint: VR_FP_EYE_HEIGHT_M,
            start: () => {
              const pt = this.rt.player_transform();
              return {
                x: pt[0], y: -pt[1], z: pt[2],
                yaw: Math.PI + pt[3] * A2R,
              };
            },
            anchor: () => {
              const pt = this.rt.player_transform();
              return { x: pt[0], y: -pt[1], z: pt[2] };
            },
            drive: (d) => this._vrDriveInput(d),
          },
        ],
        onMode: (id) => this._setVrMode(id),
        onEnter: () => this.stop(),
        onExit: () => { this._setVrMode(null); this.start(); },
      }) : null;
    }

    /* Standing height of the LIVE posed player mesh (world Y extent), the
     * first-person scale anchor. Measured lazily at VR placement time - at
     * `_rebuild` the just-uploaded geometry is still the unposed object-local
     * vertex pile (parts relative to their own joints, ~half the standing
     * height); once the engine has posed a frame the extent is the real
     * standing figure (~130 units). Cached on the instance for the harness. */
    _measurePlayerHeight() {
      let h = 0;
      try {
        const pos = this.rt.player_mesh_positions();
        let yMin = Infinity, yMax = -Infinity;
        for (let i = 1; i < pos.length; i += 3) {
          if (pos[i] < yMin) yMin = pos[i];
          if (pos[i] > yMax) yMax = pos[i];
        }
        h = yMax - yMin;
      } catch (e) { /* fall through to the anchor constant */ }
      /* Guard well above the ~64-unit unposed pile: a not-yet-posed mesh (or
       * a failed accessor) falls back to the 130-unit standing anchor. */
      this.playerHeight = (Number.isFinite(h) && h > 90)
        ? h : VR_FP_FALLBACK_MESH_HEIGHT;
      return this.playerHeight;
    }

    /* The VR mode toggled (or the session ended, id = null). Arm / disarm the
     * first-person state and hand the engine's input path back to the
     * keyboard when leaving first-person. */
    _setVrMode(id) {
      this._vrFp = (id === 'first-person');
      if (!this._vrFp) {
        this._vrDrive = null;
        if (this._vrPrecise) {
          this._vrPrecise = false;
          /* Lift the session-only override: the player's own persisted
           * setting rules again. (Writing `false` through the persisting
           * setter used to save "off" over whatever they had chosen.) */
          if (typeof this.rt.set_precise_movement_override === 'function') {
            this.rt.set_precise_movement_override(undefined);
            this.rt.set_left_stick(0, 0);
          }
        }
      }
    }

    /* One VR first-person input sample (called once per XR frame by the VR
     * module's drive hook). Routes the left stick into the ENGINE's
     * free-movement controller - the same collision-checked path the keyboard
     * uses - by (a) pointing the engine's camera azimuth along the gaze so
     * "stick forward" walks where the user looks, and (b) feeding the stick
     * as the analog axes of the engine's precise-locomotion decode (falling
     * back to 8-way d-pad bits on a stale cached WASM without that API).
     * Trigger / A = Cross (talk / confirm), B = Circle (cancel). */
    _vrDriveInput(d) {
      /* Azimuth a makes screen-up walk along world (sin a, cos a). */
      const azRad = Math.atan2(d.forward[0], d.forward[1]);
      const azimuth = ((Math.round(azRad / (Math.PI * 2) * 4096) % 4096) + 4096) % 4096;
      let pad = 0;
      if (d.buttons.trigger || d.buttons.a) pad |= 0x4000;  /* Cross */
      if (d.buttons.b) pad |= 0x2000;                       /* Circle */
      const hasPrecise = typeof this.rt.set_precise_movement_override === 'function'
        && typeof this.rt.set_left_stick === 'function';
      if (hasPrecise) {
        if (!this._vrPrecise) {
          /* Session-only: never persisted over the player's option. */
          this.rt.set_precise_movement_override(true);
          this._vrPrecise = true;
        }
        const clamp = (v) => Math.max(-127, Math.min(127, Math.round(v * 127)));
        /* PSX stick: +Y is DOWN; our z is forward(+). */
        this.rt.set_left_stick(clamp(d.x), clamp(-d.z));
      } else if (Math.hypot(d.x, d.z) > 0.3) {
        const ang = Math.atan2(d.x, d.z);   /* 0 = forward, + = right */
        const oct = ((Math.round(ang / (Math.PI / 4)) % 8) + 8) % 8;
        const DIR = [0x0010, 0x0030, 0x0020, 0x0060,
          0x0040, 0x00C0, 0x0080, 0x0090];
        pad |= DIR[oct];
      }
      this._vrDrive = { pad, azimuth };
    }

    /* ---------- scene ---------- */

    /* Boot a CDNAME scene through the engine and (re)build everything drawn.
     * Throws the engine's error message when the label doesn't resolve. */
    enter(label) {
      /* A picked scene is that scene's free-roam, never the next leg of an
       * opening the picker interrupted: abandon a live opening chain first
       * (the chain's own hand-offs - the intro-skip, the post-movie leg -
       * bypass this method). `enter_field` itself resets the engine camera
       * so the interrupted shot cannot frame the new scene. Guarded for a
       * cached WASM without the export. */
      if (typeof this.rt.play_abandon_opening_chain === 'function') {
        try { this.rt.play_abandon_opening_chain(); } catch (_) {}
      }
      const state = JSON.parse(this.rt.enter_field(label));
      return this._adopt(state, label);
    }

    /* Land the save a Load / import parked, through the engine's one resume
     * entry (`play_resume_save`: the saved scene, else the running scene,
     * else the opening town - never a New Game; the native window's
     * `BootSession::resume_save`). Rebuilds only when the landing entered a
     * scene. Returns the landing JSON (`landing`, `scene`, `entered`,
     * `state`). */
    resumeSave() {
      const r = JSON.parse(this.rt.play_resume_save());
      if (r.entered) this._adopt(r.state, r.scene);
      else if (r.scene) this.scene = r.scene;
      return r;
    }

    /* Start a New Game through the engine's one entry (`play_new_game`: the
     * seeded slate, then `opdeene`, else `town01`; the native
     * `BootSession::start_new_game`). Same return shape as `resumeSave`. */
    newGame() {
      const r = JSON.parse(this.rt.play_new_game());
      this._adopt(r.state, r.scene);
      return r;
    }

    /* Rebuild the page-side scene after the engine entered one. */
    _adopt(state, label) {
      this._rebuild();
      this.scene = (state && state.scene) || label;
      /* Demo tile board (`?tileboard=1`): the browser's twin of the native
       * window's `LEGAIA_TILE_BOARD_DEMO=1`, which no browser can set. No
       * retail scene installs a board, so without a trigger the per-cell
       * draw pass is unreachable on this host. */
      if (/[?&]tileboard=1\b/.test(location.search)
          && typeof this.rt.play_install_demo_tile_board === 'function') {
        try { this.rt.play_install_demo_tile_board(); } catch (e) {}
      }
      if (this.vr) {
        this.vr.setReady(true);
        /* A live session survives a scene swap (same canvas / GL context) - just
         * re-place the viewer in the new map. */
        this.vr.respawn();
      }
      return state;
    }

    /* Swap in a freshly-instantiated engine (after a WASM trap poisoned the
     * previous one) and re-enter `label`, then resume the loop. Reuses the
     * existing GL renderer + canvas + input listeners - only the engine handle
     * changes, so every callback that reads `this.rt` picks up the new one.
     * The page's `recoverRuntime` builds the fresh runtime from cached disc
     * bytes and drives this (Bug-3 recovery). */
    recover(newRuntime, label) {
      this.rt = newRuntime;
      this.stop();               /* the trapped loop is dead; clear its raf id */
      this._vrDrive = null;
      this._audioUp = false;     /* fresh engine has no audio output yet */
      const st = this.enter(label);
      this.start();
      return st;
    }

    /* ---------- loop ---------- */

    start() {
      if (this.raf) return;
      if (this.vr && this.vr.isActive()) return;   /* the XR loop is driving */
      const tick = () => {
        this.raf = requestAnimationFrame(tick);
        this._frame();
      };
      this.raf = requestAnimationFrame(tick);
    }

    stop() {
      if (this.raf) cancelAnimationFrame(this.raf);
      this.raf = 0;
    }

    setPaused(on) { this.paused = !!on; }
    step() { this.stepOnce = true; }

    /* Bring up the WebAudio BGM backend from inside a user gesture (browser
     * autoplay policy). Idempotent: initialises the output once, then always
     * resumes the AudioContext (browsers open it suspended even in a gesture).
     * The scene's music plays itself - every engine tick routes the field VM's
     * op-0x35 BGM events into the output - so there is nothing to start here.
     * Tolerant of a stale cached WASM without the audio methods. */
    _enableAudio() {
      const rt = this.rt;
      if (!rt || typeof rt.audio_init !== 'function') return;
      try {
        if (typeof rt.audio_ready === 'function' ? !rt.audio_ready() : !this._audioUp) {
          rt.audio_init();
          this._audioUp = true;
        }
        if (typeof rt.audio_resume === 'function') rt.audio_resume();
        /* `audio_init` parks the engine's default gain on the GainNode, which
         * would silently override wherever the page's volume slider is sitting
         * (and does, after a trap-recovery rebuild). Re-assert the control. */
        if (typeof window.__playApplyVolume === 'function') window.__playApplyVolume();
        /* Same for the dev-menu opt-in: a rebuilt runtime starts with it off,
         * so re-assert whatever the page checkbox says. */
        if (typeof window.__playApplyDevMenu === 'function') window.__playApplyDevMenu();
      } catch (e) { console.warn('play audio enable', e); }
    }

    /* Serve one lazy CD-XA staging request per frame: the engine names a
     * file's read span (`play_xa_stage_requests_json`), the page slices those
     * raw 2352-byte sectors out of the disc bytes it holds and hands them
     * back (`play_xa_install_span`), which decodes that one channel and
     * replays the clip request. No disc bytes on the page means no request
     * can be served; the engine keeps listing it, silently. */
    _serveXaStage(rt) {
      if (typeof rt.play_xa_stage_requests_json !== 'function') return;
      const bytes = window.__playDiscBytes;
      if (!bytes) return;
      let reqs = [];
      try { reqs = JSON.parse(rt.play_xa_stage_requests_json()) || []; } catch (e) { return; }
      const req = reqs[0];
      if (!req) return;
      const RAW = 2352;
      const start = req.lba * RAW, end = Math.min(bytes.length, start + req.sectors * RAW);
      if (start >= end) return;
      try { rt.play_xa_install_span(req.path, bytes.subarray(start, end), req.channel); }
      catch (e) { console.warn('xa stage', req.path, e); }
    }

    /* A WASM trap (or any throw from an engine call) during the frame poisons
     * the engine instance. Stop the dead loop and hand the message to the page,
     * whose `onError` rebuilds a fresh runtime from cached disc bytes and
     * resumes - no page reload (Bug-3 recovery). Unifies every in-frame engine
     * call (tick, scene rebuild, per-frame pose reads, draw) onto one path so
     * none can escape uncaught and freeze the loop. */
    _onEngineTrap(where, e) {
      console.warn(where, e);
      this.stop();
      if (this.opts.onError) this.opts.onError((e && e.message) || String(e));
    }

    /* This frame's world-space lens, in the page's Y-UP draw frame.
     *
     * The engine answers it analytically for every retail-model camera
     * (`camera_view::frame_eye` - the inverse of the same view composition
     * `play_camera_vp` uploads), in raw retail Y-down world, so the Y flips
     * on the way out. Only the page's own debug orbit falls through to the
     * orbit formula below. */
    _eye() {
      if (!this.debugCamera && typeof this.rt.play_camera_eye === 'function') {
        try {
          const e = this.rt.play_camera_eye();
          if (e && e.length === 3) return [e[0], -e[1], e[2]];
        } catch (_) { /* fall through to the orbit eye */ }
      }
      const FOV_Y = 0.9;
      const dist = Math.max(this.cam.halfHeight / Math.tan(FOV_Y / 2), 1);
      const sy = Math.sin(this.cam.yaw), cy = Math.cos(this.cam.yaw);
      const sp = Math.sin(this.cam.pitch), cp = Math.cos(this.cam.pitch);
      return [
        this.cam.centerX + dist * sp * sy,
        this.cam.centerY + dist * cp,
        this.cam.centerZ - dist * sp * cy,
      ];
    }

    /* Toggle the camera-occlusion fade (the page's "See-through walls"
     * checkbox; default on, matching the native play-window). Off drops
     * the staged focus immediately so the next frame is fully solid. */
    setOcclusionFade(on) {
      this.occlusionFade = !!on;
      if (!this.occlusionFade) {
        if (this.renderer) this.renderer.clearOcclusionFocus();
      }
    }

    /* The two opt-in render toggles the native window carries
     * (LEGAIA_PSX_RENDER and the `I` key / --dynamic-lighting): PSX
     * rasterisation (vertex snap + 15-bit dither) and the dynamic light.
     * Both default off and both are the identity when off; the page's
     * checkboxes drive them and re-apply on every view rebuild; the
     * renderer picks them up in `_stageFrameState`. */
    setPsxRender(on) {
      this.psxRender = !!on;
    }

    setDynamicLighting(on) {
      this.dynLighting = !!on;
    }

    /* The point lights' shadow sub-layer (the native window's `Y`); on
     * unless the "Lamp shadows" box is cleared. */
    setDynShadows(on) {
      this.dynShadows = !!on;
    }

    /* Bloom (the native window's F10): drawn by `_drawBloom` over the
     * finished 3D frame. Off unless the player ticked it. */
    setBloom(on) {
      this.bloom = !!on;
    }

    /* Stage this frame's camera.
     *
     * Default: the ENGINE's. `play_camera_vp` resolves which camera owns the
     * frame and returns the retail view-projection for it, which goes on
     * `cam.vp` - the explicit override `buildWorldOrbitVp` checks first, the
     * same channel the battle frame already used for `battle_cam_script`.
     * The orbit fields are kept in step underneath it so the debug vantage,
     * the VR spawn and the FX billboards read a sane camera basis if the
     * engine ever hands back nothing.
     *
     * Debug orbit (`F3`): clear `cam.vp` and let the page's own vantage frame
     * the scene, mirroring the native window's `field_debug_camera`. */
    _stageEngineCamera(pt) {
      const t = pt || this.rt.player_transform();
      this.cam.centerX = t[0];
      /* Retail Y is down-positive, and the draw frame flips it; target a little
       * above the floor so the camera looks at the character, not their feet. */
      this.cam.centerY = -t[1] + 60;
      this.cam.centerZ = t[2];
      this.cam.roll = 0;
      if (this.debugCamera || typeof this.rt.play_camera_vp !== 'function') {
        this.cam.vp = null;
        return;
      }
      let vp = null;
      try {
        const c = this.renderer.canvas;
        const m = this.rt.play_camera_vp(c.width, Math.max(c.height, 1));
        if (m && m.length === 16) vp = Float32Array.from(m);
      } catch (e) { vp = null; }
      this.cam.vp = vp;
    }

    dispose() {
      this.stop();
      if (this.vr) { this.vr.destroy(); this.vr = null; }
      window.removeEventListener('keydown', this._onDown);
      window.removeEventListener('keyup', this._onUp);
      window.removeEventListener('keydown', this._onHotkey);
      window.removeEventListener('blur', this._onBlur);
      if (this._screenPrims) { this._screenPrims.dispose(); this._screenPrims = null; }
      if (this._fogPass) { this._fogPass.dispose(); this._fogPass = null; }
      if (this.renderer) { this.renderer.dispose(); this.renderer = null; }
    }
  }

  window.PlayView = PlayView;
  /* Drawn by PlayView._drawScreenPrims (play-view-frame.js). */
  window.LegaiaScreenPrimPass = ScreenPrimPass;
})();
