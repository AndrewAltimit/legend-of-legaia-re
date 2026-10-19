/* webgl-tmd-frame-state.js - the TmdRenderer state a frame draws with.
 *
 * Part of the TmdRenderer class (webgl-tmd.js): the distance haze, the colour
 * grade and depth cue, the nclip / near-plane / PSX-mode toggles, enhanced
 * lighting (staging, shadow maps, glow), the camera-occlusion fade and the
 * ocean plane. Everything here is either a setter the page calls or the
 * private pass that applies what the setters stored.
 *
 * Classic script. Requires webgl-tmd.js first (the class, the SHADOW_* and
 * MOOD_WORDS constants) and site-util.js (LegaiaUtil.installMethods). */

window.LegaiaUtil.installMethods(TmdRenderer, class {
  /* World-overview distance haze: retail's overworld walk-view depth cue
   * stood in from a virtual walk camera (see the u_haze_* uniforms in
   * webgl-shaders.js). `params` keys override the stored ones; pass only
   * the fields that changed. `origin: null` follows the camera centre. */
  setDistanceHaze(params) {
    if (!params) return;
    const h = this.hazeParams;
    if (params.enable !== undefined) h.enable = params.enable ? 1 : 0;
    if (params.eyeZ !== undefined) h.eyeZ = +params.eyeZ;
    if (params.scale !== undefined) h.scale = +params.scale;
    if (params.origin !== undefined) h.origin = params.origin ? params.origin.slice(0, 3) : null;
  }

  /* One draw's distance-haze mode: 0 off, 1 the continent ground (per
   * vertex, far colour 0x100), 2 a decoration (one IR0 from its origin at
   * world (x, z), far colour 0xD0). The far word is over 128 with DPCS's
   * 0xFF saturation folded in. */
  _setHazeDraw(mode, x, z) {
    const gl = this.gl;
    const h = this.hazeParams;
    if (!h.enable) mode = 0;
    gl.uniform1i(this.locFogEnable, mode);
    if (mode === 1) {
      gl.uniform1f(this.locHazeFar, 255 / 128);
    } else if (mode === 2) {
      const o = this._hazeOrigin;
      const d = Math.hypot(x - o[0], z - o[2]);
      const trz = h.eyeZ + h.scale * d;
      /* FUN_801F69D8: IR0 = min(max(TRZ - 0x5000, 0) >> 3, 0x1000). */
      const ir0 = Math.min(Math.max(trz - 0x5000, 0) / 8, 0x1000) / 0x1000;
      gl.uniform1f(this.locHazeIr0, ir0);
      gl.uniform1f(this.locHazeFar, 0xD0 / 128);
    }
  }

  /* Prologue colour grade: `rgb` = [r,g,b] multiply tint in 0..1 (or null
   * to clear), `strength` 0..1. Mirrors the native renderer's
   * `set_color_grade` (World::scene_color_grade); identity when cleared. */
  setColorGrade(rgb, strength) {
    this.gradeParams = (rgb && strength > 0)
      ? { rgb: rgb.slice(0, 3), strength: +strength }
      : { rgb: null, strength: 0 };
  }

  /* Prologue depth-cue ramp: `far` = [r,g,b] DPCS far colour (or null to
   * clear), plus the view-depth window + max IR0. Mirrors the native
   * renderer's `set_depth_cue_ramp` / `clear_depth_cue_ramp`. */
  setDepthCue(far, nearZ, farZ, maxIr0) {
    this.cueParams = (far && maxIr0 > 0)
      ? { far: far.slice(0, 3), nearZ: +nearZ, farZ: +farZ, maxIr0: +maxIr0 }
      : { far: null, nearZ: 0, farZ: 0, maxIr0: 0 };
  }

  /* Prologue PALETTE-COLLAPSE grade: `mul` = the op-`4C 12` global screen
   * tint in 0..1 (or null for none), `on` = whether the collapse law runs.
   * The twin of the native renderer's `set_palette_grade`, and the page's
   * half of the native window's two-call staging match: with a prologue
   * grade live, `setColorGrade` carries the GOLD coefficients for the packet
   * collapse and this carries the tint; without one it stays off and the
   * tint rides `setColorGrade` as an ordinary multiply. Identity when off,
   * which is the default on every page. */
  setPaletteGrade(mul, on) {
    this.paletteParams = on
      ? { mul: (mul || [1, 1, 1]).slice(0, 3), on: true }
      : { mul: null, on: false };
  }

  /* Retail GTE NCLIP winding rejection for the assembled scene pass: `mode`
   * is the native renderer's `set_backface_cull` word (0 = both sides,
   * 2 = reject the retail back faces). The play page stages it from the
   * shared `camera_view::nclip_cull_mode` kernel; every other page leaves
   * it at 0. */
  /* Stage one renderAssembled placement's NCLIP word (`p.nclip`, the frame's
   * `nclipCull` when absent); `cur` is the word staged now, returned updated
   * so an unchanged placement costs no uniform write. */
  _setPlacementNclip(want, cur) {
    const mode = (want === undefined || want === null) ? this.nclipCull : (want | 0);
    if (mode !== cur && this.locNclipCull) {
      this.gl.uniform1i(this.locNclipCull, mode);
    }
    return mode;
  }

  setNclipCull(mode) {
    this.nclipCull = (mode | 0);
  }

  /* The kingdom overworld's per-vertex screen-Y curvature for the assembled
   * scene pass: `scale` is the frame's clip.w-to-SZ factor from the shared
   * `overworld_curvature::frame_curve_scale` kernel (the native renderer's
   * `set_overworld_curvature` word). 0 - every other page - is flat.
   * `bias` is the table's build offset in screen rows (the native
   * `set_overworld_curve_bias` word), added at any scale; 0 when omitted. */
  setOverworldCurve(scale, bias) {
    this.overworldCurve = scale > 0 ? +scale : 0;
    this.overworldCurveBias = Number.isFinite(bias) ? +bias : 0;
  }

  /* Retail's per-primitive near reject for the assembled scene pass: the
   * [enable, sz_per_w, ot_shift, near_otz] word from the shared
   * `camera_view::prim_near_cut` kernel (the native renderer's
   * `set_prim_near_reject`). null / all-zero never rejects. */
  setPrimNear(params) {
    this.primNear = (params && params.length === 4)
      ? [+params[0], +params[1], +params[2], +params[3]] : [0, 0, 0, 0];
  }

  /* (Re)build and bind a scene mesh's primitive-corner stream into its VAO
   * (which must be bound). Without a hook, or when the hook declines, the
   * attributes stay disabled and read the generic default - count 0, never
   * rejected. */
  _bindPrimRefs(m, positions, indices) {
    const gl = this.gl;
    const locs = this.locPrimC || [];
    let refs = null;
    if (this.primRefsFn && positions && indices && locs[0] >= 0) {
      try { refs = this.primRefsFn(positions, indices); } catch (_) { refs = null; }
      if (refs && refs.length !== (positions.length / 3) * 13) refs = null;
    }
    m.hasPrimRefs = !!refs;
    if (!refs) {
      for (const l of locs) if (l >= 0) gl.disableVertexAttribArray(l);
      return;
    }
    if (!m.primRefBuf) m.primRefBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, m.primRefBuf);
    gl.bufferData(gl.ARRAY_BUFFER, refs instanceof Float32Array ? refs : new Float32Array(refs),
      gl.DYNAMIC_DRAW);
    const sizes = [4, 3, 3, 3];
    const offs = [0, 16, 28, 40];
    for (let k = 0; k < 4; k++) {
      if (locs[k] < 0) continue;
      gl.enableVertexAttribArray(locs[k]);
      gl.vertexAttribPointer(locs[k], sizes[k], gl.FLOAT, false, 52, offs[k]);
    }
  }

  /* PSX rasterisation (the native renderer's `set_psx_mode`, which the
   * native window turns on with LEGAIA_PSX_RENDER): vertex positions snap to
   * the framebuffer's integer pixel grid and every shaded fragment takes the
   * PSX 4x4 ordered dither down to 15-bit colour. Off by default; off is the
   * untouched faithful path (the shader gates on a uniform that stays 0). */
  setPsxMode(on) {
    this.psxMode = !!on;
  }

  /* Dynamic lighting (the native renderer's `set_dynamic_lighting`, the
   * native window's `I` / `--dynamic-lighting`): a soft warm directional
   * light off smoothed normals plus a screen-centred light pool, capped at
   * 1.3x over the baked shading. Off by default and the identity when off.
   * The native layer's derived per-scene point lights (and their shadow
   * maps) are not part of the page's toggle. */
  /* Opt the perspective renderAssembled frames into the log-of-w depth write
   * (webgl-shaders.js LOG_DEPTH_GLSL). Off by default, so every other page
   * keeps the rasterised depth. */
  setLogDepth(on) {
    this.logDepth = !!on;
  }

  setDynamicLighting(on) {
    this.dynLighting = !!on;
  }

  /* The point lights' shadow sub-layer (the native window's `Y` /
   * `--no-dyn-shadows`). On by default, inert while dynamic lighting is off
   * or a frame picks no lights. */
  setDynShadows(on) {
    this.dynShadows = !!on;
  }

  /* Stage the two toggles on the bound main program for a `w` x `h` frame.
   * The framebuffer size goes up regardless: it is the light pool's
   * viewport too, and the native `psx_params.xy` carries it the same way. */
  _applyRenderToggles(w, h) {
    const gl = this.gl;
    if (this.locPsx) {
      const on = this.psxMode ? 1 : 0;
      gl.uniform4f(this.locPsx, w, h, on, on);
    }
    if (this.locDynDir) {
      /* The mood comes from the engine with this frame's lighting packet;
       * without one (every page but the play page) the enable stays 0. */
      const f = this.dynLighting ? this.lightFrame : null;
      if (f) {
        const m = f.mood;
        gl.uniform4f(this.locDynDir, m[0], m[1], m[2], 1);
        gl.uniform4f(this.locDynColor, m[4], m[5], m[6], m[7]);
        gl.uniform4f(this.locDynAmbient, m[8], m[9], m[10], m[11]);
        if (this.locDynWindow) gl.uniform1f(this.locDynWindow, m[12] || 0);
        gl.uniform1i(this.locLightCount, f.count);
        if (f.count > 0) {
          gl.uniform4fv(this.locLightPr, f.pr);
          gl.uniform4fv(this.locLightCol, f.col);
        }
        const sh = this.shadowCount > 0 && this.shadowDim > 0;
        if (this.locShadow) {
          gl.uniform4f(this.locShadow, sh ? 1 : 0, sh ? 1 / this.shadowDim : 0,
            SHADOW_COMPARE_BIAS, 0);
        }
        if (sh && this.locLightVp) gl.uniformMatrix4fv(this.locLightVp, false, this.lightVps);
      } else {
        gl.uniform4f(this.locDynDir, 0, 0, 0, 0);
        gl.uniform1i(this.locLightCount, 0);
        if (this.locShadow) gl.uniform4f(this.locShadow, 0, 0, 0, 0);
      }
    }
  }

  /* Enhanced lighting: ask the page's provider (the engine's
   * `play_lighting_frame`) for this frame's mood, lights and glow quads
   * against the camera basis of `vp`, and convert the engine's retail
   * Y-down frame into this page's (x, -y, z). The basis is read off the VP
   * rows (right ~ row 0, up ~ row 1 of the view), which is all a
   * camera-facing quad needs - sign is irrelevant to a symmetric sprite. */
  _stageLighting(vp) {
    this.lightFrame = null;
    if (!this.dynLighting || !this.lightingProvider) return;
    const norm = (x, y, z) => {
      const l = Math.hypot(x, y, z) || 1;
      return [x / l, y / l, z / l];
    };
    const r = norm(vp[0], vp[4], vp[8]);
    const u = norm(vp[1], vp[5], vp[9]);
    /* Page frame -> retail frame: negate Y. */
    let pkt;
    try { pkt = this.lightingProvider([r[0], -r[1], r[2]], [u[0], -u[1], u[2]]); }
    catch (_) { pkt = null; }
    if (!pkt || pkt.length < MOOD_WORDS + 2) return;
    const mood = pkt.slice(0, MOOD_WORDS);
    const n = pkt[MOOD_WORDS] | 0;
    const nv = pkt[MOOD_WORDS + 1] | 0;
    const pr = new Float32Array(32);
    const col = new Float32Array(32);
    let o = MOOD_WORDS + 2;
    for (let i = 0; i < n && i < 8; i++, o += 7) {
      pr.set([pkt[o], -pkt[o + 1], pkt[o + 2], pkt[o + 3]], i * 4);
      col.set([pkt[o + 4], pkt[o + 5], pkt[o + 6], 0], i * 4);
    }
    o = MOOD_WORDS + 2 + n * 7;
    const glow = nv > 0 ? Float32Array.from(pkt.slice(o, o + nv * 9)) : null;
    this.lightFrame = { mood, count: Math.min(n, 8), pr, col, glow, glowCount: nv };
  }

  /* Render one shadow-map layer per staged point light over this frame's
   * geometry (the ground + every placement, full index range) - the twin of
   * engine-render's stage_scene_lights_and_shadows. Each light looks
   * straight down a wide cone (retail field space is Y-down; this page's
   * frame is (x, -y, z), so down is -Y here). The projection is GL-style:
   * its window depth equals the native 0..1 depth for the same near / far,
   * so SHADOW_COMPARE_BIAS and the polygon offset carry over unchanged.
   * Restores the caller's framebuffer and viewport (the VR path renders
   * into its own). */
  _renderLightShadows(placements) {
    this.shadowCount = 0;
    const f = this.lightFrame;
    if (!this.dynLighting || !this.dynShadows || !f || f.count === 0 || !this.shadowProgram) return;
    const gl = this.gl;
    const n = f.count;
    if (this.shadowDim !== SHADOW_MAP_DIM) {
      gl.activeTexture(gl.TEXTURE0 + SHADOW_TEX_UNIT);
      gl.bindTexture(gl.TEXTURE_2D_ARRAY, this.shadowTex);
      gl.texImage3D(gl.TEXTURE_2D_ARRAY, 0, gl.DEPTH_COMPONENT24, SHADOW_MAP_DIM,
        SHADOW_MAP_DIM, 8, 0, gl.DEPTH_COMPONENT, gl.UNSIGNED_INT, null);
      gl.activeTexture(gl.TEXTURE0);
      this.shadowDim = SHADOW_MAP_DIM;
    }
    const g = 1 / Math.tan(SHADOW_FOV / 2);
    for (let i = 0; i < n; i++) {
      const ex = f.pr[i * 4], ey = f.pr[i * 4 + 1], ez = f.pr[i * 4 + 2];
      const r = f.pr[i * 4 + 3];
      const near = Math.max(r * SHADOW_NEAR_FRAC, SHADOW_NEAR_MIN);
      const far = Math.max(r, near * 2);
      const a = (far + near) / (near - far);
      const b = (2 * far * near) / (near - far);
      /* Column-major. View: x' = X - ex, y' = Z - ez, z' = Y - ey (the
       * native look_at_rh(eye, eye + down, +Z) in this frame); then a GL
       * perspective, so clip.w = ey - Y - positive below the light. */
      const m = this.lightVps.subarray(i * 16, i * 16 + 16);
      m.fill(0);
      m[0] = g; m[12] = -g * ex;
      m[9] = g; m[13] = -g * ez;
      m[6] = a; m[14] = -a * ey + b;
      m[7] = -1; m[15] = ey;
    }
    const prevFb = gl.getParameter(gl.FRAMEBUFFER_BINDING);
    const prevVp = gl.getParameter(gl.VIEWPORT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.shadowFbo);
    gl.drawBuffers([gl.NONE]);
    gl.readBuffer(gl.NONE);
    gl.viewport(0, 0, SHADOW_MAP_DIM, SHADOW_MAP_DIM);
    gl.useProgram(this.shadowProgram);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LESS);
    gl.depthMask(true);
    gl.disable(gl.BLEND);
    gl.enable(gl.POLYGON_OFFSET_FILL);
    gl.polygonOffset(2, 2);
    const flipY = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
    const ground = this.groundEnable && this.ground && this.ground.indexCount > 0 ? this.ground : null;
    for (let li = 0; li < n; li++) {
      gl.framebufferTextureLayer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, this.shadowTex, 0, li);
      gl.clear(gl.DEPTH_BUFFER_BIT);
      const lvp = this.lightVps.subarray(li * 16, li * 16 + 16);
      if (ground) {
        gl.uniformMatrix4fv(this.locShadowMvp, false, mulMat4(lvp, flipY));
        gl.bindVertexArray(ground.vao);
        gl.drawElements(gl.TRIANGLES, ground.indexCount, gl.UNSIGNED_INT, 0);
      }
      for (const p of placements) {
        const mesh = this.sceneMeshes.get(p.meshId);
        if (!mesh || mesh.indexCount === 0) continue;
        gl.uniformMatrix4fv(this.locShadowMvp, false, mulMat4(lvp, this._placementModel(p, mesh)));
        gl.bindVertexArray(mesh.vao);
        gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_INT, 0);
      }
    }
    gl.bindVertexArray(null);
    gl.framebufferTextureLayer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, null, 0, 0);
    gl.disable(gl.POLYGON_OFFSET_FILL);
    gl.depthFunc(gl.LEQUAL);
    gl.bindFramebuffer(gl.FRAMEBUFFER, prevFb);
    gl.viewport(prevVp[0], prevVp[1], prevVp[2], prevVp[3]);
    this.shadowCount = n;
  }

  /* Draw the staged glow quads: additive, depth-tested against the scene,
   * no depth writes. `vp` is the frame's view-projection; the quads are in
   * the retail frame, so the page's Y flip goes on the left of nothing -
   * it is folded in as vp * diag(1, -1, 1). */
  _drawGlow(vp) {
    const f = this.lightFrame;
    if (!this.dynLighting || !f || !f.glow || f.glowCount === 0) return;
    const gl = this.gl;
    const flipY = new Float32Array([1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
    gl.useProgram(this.glowProgram);
    gl.uniformMatrix4fv(this.locGlowMvp, false, mulMat4(vp, flipY));
    /* Same depth space as the scene the halos are tested against. */
    if (this.locGlowLogDepth) gl.uniform1i(this.locGlowLogDepth, this.lastLogDepth ? 1 : 0);
    gl.bindVertexArray(this.glowVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.glowBuf);
    gl.bufferData(gl.ARRAY_BUFFER, f.glow, gl.DYNAMIC_DRAW);
    gl.enable(gl.BLEND);
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.ONE, gl.ONE);
    gl.depthMask(false);
    gl.drawArrays(gl.TRIANGLES, 0, f.glowCount);
    gl.depthMask(true);
    gl.disable(gl.BLEND);
    gl.bindVertexArray(null);
    gl.useProgram(this.program);
  }

  /* Bind (computing on first use or after a position update) the smoothed
   * normal stream of one mesh record into its VAO - only while dynamic
   * lighting is on, so the default path never pays for it. `rec` is a scene
   * mesh, the ground, or the single-mesh record; it carries `vao`,
   * `cpuPositions`, `cpuIndices` and `normalsDirty`. A record with no CPU
   * copy (the ground: native leaves its normals at the zero sentinel too)
   * keeps the attribute unbound and lights off the facet normal. */
  _ensureNormals(rec) {
    if (!this.dynLighting || this.locNormal < 0 || !rec) return;
    if (!rec.normalsDirty && rec.normBuf) return;
    const gl = this.gl;
    const pos = rec.cpuPositions;
    const idx = rec.cpuIndices;
    if (!pos || !idx || pos.length === 0) return;
    const normals = computeSmoothNormals(pos, idx);
    if (!rec.normBuf) rec.normBuf = gl.createBuffer();
    gl.bindVertexArray(rec.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, rec.normBuf);
    gl.bufferData(gl.ARRAY_BUFFER, normals, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(this.locNormal);
    gl.vertexAttribPointer(this.locNormal, 3, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
    rec.normalsDirty = false;
  }

  /* Set the context-global `a_flat_rgba` constant that a draw with no bound
   * colour stream reads.
   *
   * It must be the PSX **neutral modulation word** 0x80 (128/255), not white:
   * the fragment shader's textured path is `texel * colour * 255/128`, so a
   * white constant would brighten every un-coloured draw by ~2x. Alpha 1.0
   * marks the vertex textured, which is the only sane reading when no stream
   * says otherwise. Generic vertex-attribute values are context state rather
   * than VAO state, so one call covers every VAO that leaves the attribute
   * disabled. */
  _setNeutralPacketColor() {
    if (this.locFlatRgba < 0) return;
    const n = 128 / 255;
    this.gl.vertexAttrib4f(this.locFlatRgba, n, n, n, 1.0);
  }

  /* Stage the grade + cue uniforms on the currently-bound main program. */
  _applyGradeCue() {
    const gl = this.gl;
    if (!this.locGrade) return;
    const g = this.gradeParams;
    if (g.rgb) gl.uniform4f(this.locGrade, g.rgb[0], g.rgb[1], g.rgb[2], g.strength);
    else gl.uniform4f(this.locGrade, 1, 1, 1, 0);
    if (this.locPalette) {
      const p = this.paletteParams;
      if (p && p.on) gl.uniform4f(this.locPalette, p.mul[0], p.mul[1], p.mul[2], 1);
      else gl.uniform4f(this.locPalette, 1, 1, 1, 0);
    }
    this._setCue(this.cueParams);
  }

  /* Push one depth-cue record (the frame-global `cueParams`, or a
   * placement's own `cue`) into the bound program. Split out of
   * `_applyGradeCue` so `renderAssembled` can switch the cue PER DRAW: the
   * native renderer's cue is a per-`SceneDraw` field, and the battle ground
   * grid's stage fog and the target-select cursor tint are both draws that
   * carry their own while everything around them stays uncued. */
  _setCue(c) {
    const gl = this.gl;
    if (!this.locCue) return;
    if (c && c.far && c.maxIr0 > 0) {
      gl.uniform4f(this.locCue, c.nearZ, c.farZ, c.maxIr0, 1);
      gl.uniform3f(this.locCueFar, c.far[0], c.far[1], c.far[2]);
    } else {
      gl.uniform4f(this.locCue, 0, 0, 0, 0);
      gl.uniform3f(this.locCueFar, 0, 0, 0);
    }
  }

  /* Stage one draw's object-effect clip (`[m0, m1, m2, lo, hi, 1]` from the
   * engine's `play_effect_clip`, or none). `on` is whether one is staged
   * now; returns the new state, so a frame with no clip writes nothing. */
  _setEffectClip(c, on) {
    const gl = this.gl;
    if (!this.locEclipM) return false;
    if (c && c.length >= 6) {
      gl.uniform4f(this.locEclipM, c[0], c[1], c[2], 1);
      gl.uniform2f(this.locEclipB, c[3], c[4]);
      return true;
    }
    if (on) gl.uniform4f(this.locEclipM, 0, 0, 0, 0);
    return false;
  }

  /* Camera-occlusion fade (see-through walls): stage the player's world
   * position (draw frame - the Y-flipped coords the placements use; the
   * play page passes `[x, -y + 90, z]`, its body-centre point) plus the
   * page's eased fade strength (0..1, the visibility-gate ramp) for the
   * next renderAssembled call, which projects it through the frame's own
   * view-projection and screen-doors scene fragments that sit between the
   * camera and this point (GLSL `occl_keep`/`occl_bayer`; the native twin
   * is engine-render's occlusion_fade module). Call per frame; stale foci
   * would fade the wrong screen region. `feetPos` (same frame) is the
   * floor point under the character: only fragments above its projection
   * fade (the feet-line rule - see OCCL_LIFT_FEATHER_FRAC); omitted, the
   * rule is off. */
  setOcclusionFocus(worldPos, strength, feetPos) {
    if (!worldPos || !(strength > 0)) {
      this.occlFocus = null;
      return;
    }
    this.occlFocus = {
      pos: [worldPos[0], worldPos[1], worldPos[2]],
      feet: feetPos ? [feetPos[0], feetPos[1], feetPos[2]] : null,
      strength: Math.min(strength, 1.0),
    };
  }

  /* Drop the occlusion-fade focus - every fragment keeps (the default). */
  clearOcclusionFocus() {
    this.occlFocus = null;
  }

  /* Stage the occlusion-fade uniforms on the bound main program for a
   * frame rendered with view-projection `vp` into a `w`x`h` framebuffer.
   * Projects the staged world focus to gl_FragCoord pixels (origin
   * bottom-left) + view depth; a missing focus (or one behind the camera
   * plane) stages the all-zero disable so the shader is the identity. */
  _applyOcclusionFade(vp, w, h) {
    const gl = this.gl;
    if (!this.locOcclFocus) return;
    const f = this.occlFocus;
    if (f) {
      /* clip = vp * (x, y, z, 1) - column-major mat4. */
      const p = f.pos;
      const cx = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
      const cy = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
      const cw = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
      if (cw > 1e-3) {
        const px = (cx / cw * 0.5 + 0.5) * w;
        const py = (cy / cw * 0.5 + 0.5) * h;
        gl.uniform4f(this.locOcclFocus, px, py, cw, f.strength);
        /* The circle is authored in world units and projected here, at the
         * focus depth, so it keeps hugging the character as the camera
         * pushes in instead of staying a fixed slice of the viewport. */
        const radius = occlRadiusPx(cw, occlProjScaleY(vp), h);
        gl.uniform4f(this.locOcclParams,
          radius, OCCL_MIN_KEEP,
          OCCL_DEPTH_MARGIN, radius * OCCL_FEATHER_FRAC_OF_RADIUS);
        /* Feet-line rule: project the floor point under the character and
         * stage the feet -> centre lift axis (zero = rule off). */
        let lift = [0, 0, 0, 0];
        const q = f.feet;
        if (q) {
          const fx = vp[0] * q[0] + vp[4] * q[1] + vp[8] * q[2] + vp[12];
          const fy = vp[1] * q[0] + vp[5] * q[1] + vp[9] * q[2] + vp[13];
          const fw = vp[3] * q[0] + vp[7] * q[1] + vp[11] * q[2] + vp[15];
          if (fw > 1e-3) {
            const feet = [(fx / fw * 0.5 + 0.5) * w, (fy / fw * 0.5 + 0.5) * h];
            const axis = occlLiftAxis(feet, [px, py]);
            lift = [feet[0], feet[1], axis[0], axis[1]];
          }
        }
        if (this.locOcclLift) gl.uniform4f(this.locOcclLift, lift[0], lift[1], lift[2], lift[3]);
        return;
      }
    }
    gl.uniform4f(this.locOcclFocus, 0, 0, 0, 0);
    if (this.locOcclLift) gl.uniform4f(this.locOcclLift, 0, 0, 0, 0);
  }

  /* Set the per-kingdom ocean tint + enable flag. `color` is `{ r, g, b }`
   * in 0..1 floats (typically the `ocean_color_normalized` field from
   * world-overview.json). `enable` toggles whether the ocean pass runs
   * at all - the viewer wires this to a "show ocean" checkbox so the
   * 2D-style overview without the backdrop is still reachable. Pass
   * `planeY` to lift/lower the plane (default 0). The colour is only
   * the visible output when `setOceanAssets` hasn't uploaded a real
   * texture; once it has, the textured pipeline takes over. */
  setOceanColor(color, enable, planeY) {
    if (color && typeof color === 'object') {
      this.oceanParams.color = {
        r: +color.r, g: +color.g, b: +color.b,
      };
    }
    if (enable !== undefined) {
      this.oceanParams.enable = enable ? 1 : 0;
    }
    if (planeY !== undefined) {
      this.oceanParams.planeY = +planeY;
    }
  }

  /* Upload the disc-side ocean tile assets. `texture` is the raw 4bpp
   * VRAM data (32 768 bytes, 128×256 packed) extracted by
   * `legaia_scene_formats::ocean::find_ocean_assets`. `animationFrames` is
   * the 416-byte flat buffer (13 frames × 16 BGR555 entries, LE) from
   * the same source. `tileWorldSize` is how many world units one
   * texture wrap should cover (default 256, matches the retail tile
   * pitch). After this call the ocean pass switches into textured mode.
   *
   * `sampleWidth` / `sampleHeight` (optional, default 96/96) restrict
   * sampling to a top-left sub-rectangle of the texture page. Retail
   * uses only the top-left 96x96 logical-pixel region for the ocean
   * wave ramp; the rest of the page is shared with other tile prims
   * and reads as zeros for our purposes.
   *
   * Pass `null` arguments to clear back to the solid-colour fallback. */
  setOceanAssets(texture, animationFrames, tileWorldSize, sampleWidth, sampleHeight) {
    const gl = this.gl;
    if (!texture || !animationFrames) {
      this.oceanParams.textured = false;
      this.oceanParams.animationFrames = null;
      this.oceanParams.frameCount = 0;
      this.oceanParams.currentFrame = 0;
      return;
    }
    if (texture.byteLength !== 128 * 256) {
      console.warn('[webgl-tmd] unexpected ocean texture size:', texture.byteLength);
    }
    /* Upload the 4bpp byte stream into the R8UI atlas. */
    gl.bindTexture(gl.TEXTURE_2D, this.oceanTex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0, 0, 0, 128, 256,
      gl.RED_INTEGER, gl.UNSIGNED_BYTE,
      texture,
    );
    /* Stash the animation table as a Uint16Array view so we can splat
     * one frame at a time into the small CLUT texture each tick. */
    const u16 = new Uint16Array(
      animationFrames.buffer,
      animationFrames.byteOffset,
      animationFrames.byteLength / 2,
    );
    const frameCount = Math.floor(u16.length / 16);
    this.oceanParams.animationFrames = u16;
    this.oceanParams.frameCount = frameCount;
    this.oceanParams.currentFrame = 0;
    this.oceanParams.textured = frameCount > 0 && texture.byteLength === 128 * 256;
    if (tileWorldSize !== undefined && tileWorldSize > 0) {
      this.oceanParams.tileWorldSize = +tileWorldSize;
    }
    if (sampleWidth !== undefined && sampleWidth > 0) {
      this.oceanParams.sampleWidth = +sampleWidth;
    }
    if (sampleHeight !== undefined && sampleHeight > 0) {
      this.oceanParams.sampleHeight = +sampleHeight;
    }
    /* Push frame 0 so the first draw has valid CLUT data. */
    this._uploadOceanFrame(0);
  }

  /* Internal: upload animation frame `idx` (16 BGR555 entries) to the
   * backdrop plane's CLUT texture AND into the main VRAM texture at
   * `(0, 506)` - the retail per-frame ocean DMA target (CBA 0x7E80).
   * The continent heightfield's water cells sample that VRAM CLUT row
   * directly, so writing the frame there animates every water prim in
   * the scene in lockstep with the backdrop plane - one layer, exactly
   * the retail mechanism (mirrors the live engine's
   * `advance_ocean_animation` / `write_clut_row(0, 506, frame)`).
   * Called from renderAssembled when wall-clock time crosses
   * `frameDurationSec`. */
  _uploadOceanFrame(idx) {
    const p = this.oceanParams;
    if (!p.animationFrames || idx >= p.frameCount) return;
    const slice = p.animationFrames.subarray(idx * 16, (idx + 1) * 16);
    const gl = this.gl;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0, 0, 0, 16, 1,
      gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      slice,
    );
    p.currentFrame = idx;
    /* The slot-5 walker owns the VRAM row when it runs (setOceanWalkerDriven):
     * the fallback frames carry a transparent entry 0 where retail's walked
     * row holds 0x8000, so they must not land on it. */
    if (p.walkerDriven) return;
    /* Legacy fallback: overwrite the first 16 entries of the ocean
     * CLUT row inside VRAM so terrain-embedded water shimmers too. */
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0, 0, 506, 16, 1,
      gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      slice,
    );
  }

  /* Hand the ocean head to the kingdom's slot-5 CLUT walker
   * (`viewer.kingdom_clut_tick`, the engine's `ClutWalkAnim`): the page
   * re-uploads the walked VRAM and feeds the backdrop plane the same row
   * through `setOceanClutFromVram`, and the wall-clock fallback cycle stops
   * writing. `false` restores the fallback (a kingdom with no walker). */
  setOceanWalkerDriven(on) {
    this.oceanParams.walkerDriven = !!on;
  }

  /* Copy the ocean head `(0, 506)` - 16 BGR555 entries - out of a full VRAM
   * image (the bytes `uploadVram` takes) into the backdrop plane's CLUT, so
   * the open sea past the continent grid shimmers in lockstep with the
   * heightfield's own water cells. */
  setOceanClutFromVram(bytes) {
    if (!bytes || bytes.byteLength !== VRAM_W * VRAM_H * 2) return;
    const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 2);
    const row = u16.slice(506 * VRAM_W, 506 * VRAM_W + 16);
    const gl = this.gl;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0, 0, 0, 16, 1,
      gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      row,
    );
  }

});
