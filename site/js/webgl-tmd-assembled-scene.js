/* webgl-tmd-assembled-scene.js - the TmdRenderer assembled-scene path.
 *
 * Part of the TmdRenderer class (webgl-tmd.js): scene meshes kept resident
 * under a caller's meshId, the ground heightfield pass, and renderAssembled,
 * which draws a placement list over both. The single-mesh inspector path
 * (uploadMesh / render) stays in webgl-tmd.js.
 *
 * Classic script. Requires webgl-tmd.js first (the class, buildSemiTail,
 * overworldDecorationCue) and site-util.js (LegaiaUtil.installMethods). */

window.LegaiaUtil.installMethods(TmdRenderer, class {
  /* ---------- Assembled / scene-mesh path ----------------------------- */

  /* Upload one TMD's geometry under a caller-supplied meshId and keep it
   * resident on the GPU. The world-overview page uses the kingdom pack slot
   * as the meshId, so repeated placements that share a slot share GPU buffers.
   *
   * `flatRgba` (optional): Uint8Array, 4 bytes per vertex [r, g, b, flag]
   * (flag 255 = textured / sample VRAM and modulate by the RGB, 0 = untextured
   * / fill with the RGB) - the prim's packet colour. Pass it for every mesh
   * built off a TMD; omit / pass null only for geometry with no packet colour
   * (a generated heightfield), which then reads the neutral 0x80 constant and
   * draws at the raw texel.
   *
   * Idempotent: re-upload under the same meshId overwrites. */
  uploadSceneMesh(meshId, positions, uvs, cbaTsb, indices, flatRgba) {
    const gl = this.gl;
    let m = this.sceneMeshes.get(meshId);
    if (!m) {
      m = {
        vao: gl.createVertexArray(),
        posBuf: gl.createBuffer(),
        uvBuf:  gl.createBuffer(),
        ctBuf:  gl.createBuffer(),
        flatBuf: null,
        idxBuf: gl.createBuffer(),
        indexCount: 0,
        hasFlat: false,
        semiRanges: null,
        aabb: null,
      };
      this.sceneMeshes.set(meshId, m);
    }
    m.aabb = computeAabb(positions);
    m.morphReach = 0;
    m.vertexCount = positions.length / 3;
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

    m.hasFlat = !!(flatRgba && flatRgba.length && this.locFlatRgba >= 0);
    if (m.hasFlat) {
      if (!m.flatBuf) m.flatBuf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, m.flatBuf);
      gl.bufferData(gl.ARRAY_BUFFER, flatRgba, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(this.locFlatRgba);
      gl.vertexAttribPointer(this.locFlatRgba, 4, gl.UNSIGNED_BYTE, true, 0, 0);
    } else if (this.locFlatRgba >= 0) {
      /* Attribute enables are VAO state: leave it disabled in this VAO so
       * draws read the context-global neutral-modulation constant. */
      gl.disableVertexAttribArray(this.locFlatRgba);
    }

    /* Semi-transparent (ABE) prims: append the per-ABR-mode blend tail. The
     * opaque pass keeps drawing 0..indices.length; the tail ranges are only
     * touched by renderAssembled's blend pass. */
    const tail = buildSemiTail(indices, cbaTsb);
    m.semiRanges = tail ? tail.ranges : null;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, m.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, tail ? tail.indices : indices, gl.STATIC_DRAW);

    m.indexCount = indices.length;
    /* Kept so a CBA/TSB re-upload can rebuild the semi tail. */
    m.baseIndices = indices;
    this._bindPrimRefs(m, positions, indices);
    gl.bindVertexArray(null);
    /* The dynamic light's normal source (computed lazily, see
     * _ensureNormals); the semi tail repeats triangles, so the base list. */
    m.cpuPositions = positions;
    m.cpuIndices = indices;
    m.normalsDirty = true;
  }

  /* Re-upload just the per-vertex CBA/TSB words of an already-registered
   * scene mesh (same vertex count) and rebuild its per-ABR semi tail from
   * them. A battle body's whole-mesh blend rides this: the engine hands the
   * TSB stream with the colour word's ABE + ABR ORed in (retail
   * FUN_80043390 ORs them into every packet) when the body's blend changes.
   * No-op for an unknown meshId. */
  updateSceneMeshCbaTsb(meshId, cbaTsb) {
    const gl = this.gl;
    const m = this.sceneMeshes.get(meshId);
    if (!m || !m.baseIndices || !cbaTsb || cbaTsb.length !== m.vertexCount * 2) return;
    gl.bindVertexArray(m.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, m.ctBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, cbaTsb);
    const tail = buildSemiTail(m.baseIndices, cbaTsb);
    m.semiRanges = tail ? tail.ranges : null;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, m.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, tail ? tail.indices : m.baseIndices, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
  }

  hasSceneMesh(meshId) {
    return this.sceneMeshes.has(meshId);
  }

  /* Re-upload just the positions of an already-registered scene mesh, keeping
   * its UVs / CBA-TSB / indices / flat colours. This is the animated-actor path
   * on the play page: a character's vertices are object-local, so every frame
   * its posed positions change while the rest of the vertex stream doesn't.
   * The buffer must keep its vertex count (a pose moves vertices, it never adds
   * them). No-op for an unknown meshId. */
  updateSceneMeshPositions(meshId, positions) {
    const gl = this.gl;
    const m = this.sceneMeshes.get(meshId);
    if (!m || !positions || positions.length === 0) return;
    gl.bindBuffer(gl.ARRAY_BUFFER, m.posBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, positions);
    m.aabb = computeAabb(positions);
    /* The primitive corners move with the pose. */
    if (m.hasPrimRefs) {
      gl.bindVertexArray(m.vao);
      this._bindPrimRefs(m, positions, m.baseIndices);
      gl.bindVertexArray(null);
    }
    /* A posed mesh re-derives its normals, as native's posed VRAM mesh
     * build does - lazily, and only while dynamic lighting is on. */
    m.cpuPositions = positions;
    m.normalsDirty = true;
  }

  /* Re-upload just the per-vertex packet colours of an already-registered
   * scene mesh (same vertex count). The battle actors' Rot limb dimming
   * (retail FUN_80048A08's per-object colour rule) rides this: the engine
   * hands a re-coloured stream when the dimmed set changes. No-op for an
   * unknown meshId or a mesh uploaded without a colour stream. */
  updateSceneMeshFlat(meshId, flatRgba) {
    const gl = this.gl;
    const m = this.sceneMeshes.get(meshId);
    if (!m || !m.hasFlat || !m.flatBuf || !flatRgba || flatRgba.length === 0) return;
    gl.bindBuffer(gl.ARRAY_BUFFER, m.flatBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, flatRgba);
  }

  /* Draw scene mesh `meshId` once over the finished frame, sampling its own
   * VRAM texture `tex` (a mesh of another scene than the one on screen): the
   * workbench's placement ghost. `mvp` is the caller's view-projection, in the
   * same log-depth space as the frame; `alpha` < 1 blends it over the picture
   * with the depth buffer read but not written, `tint` (`[r, g, b]`) replaces
   * its colour by the texel's luminance. */
  drawMeshOver(meshId, mvp, model, tex, alpha, tint) {
    const m = this.sceneMeshes.get(meshId);
    if (!m || !m.indexCount) return;
    const gl = this.gl;
    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locMvp, false, mvp);
    gl.uniformMatrix4fv(this.locModel, false, model);
    if (this.locLogDepthOn) gl.uniform1i(this.locLogDepthOn, this.lastLogDepth ? 1 : 0);
    /* The scene-pass words on the shared program: identity for this draw;
     * renderAssembled stages them again on the next frame. */
    if (this.locCurve) gl.uniform1f(this.locCurve, 0);
    if (this.locPrimNear) gl.uniform4f(this.locPrimNear, 0, 0, 0, 0);
    if (this.locGroundDrop) gl.uniform1f(this.locGroundDrop, 0);
    if (this.locGteSat) gl.uniform4f(this.locGteSat, 0, 0, 0, 0);
    if (this.locNclipCull) gl.uniform1i(this.locNclipCull, 0);
    if (this.locGrade) gl.uniform4f(this.locGrade, 1, 1, 1, 0);
    if (this.locPalette) gl.uniform4f(this.locPalette, 1, 1, 1, 0);
    this._setCue(null);
    gl.uniform1i(this.locFogEnable, 0);
    gl.uniform1i(this.locNoDisc, 0);
    gl.uniform1i(this.locSemiPass, -1);
    gl.uniform1i(this.locUseFlatColors, m.hasFlat ? 1 : 0);
    if (tint) gl.uniform4f(this.locGhost, tint[0], tint[1], tint[2], 1);
    else gl.uniform4f(this.locGhost, 0, 0, 0, 0);
    gl.uniform4f(this.locOcclFocus, 0, 0, 0, 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(this.locVram, 0);
    gl.disable(gl.CULL_FACE);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    const over = alpha < 1;
    if (over) {
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendColor(0, 0, 0, alpha);
      gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
      gl.depthMask(false);
    }
    gl.bindVertexArray(m.vao);
    gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
    gl.bindVertexArray(null);
    if (over) {
      gl.depthMask(true);
      gl.disable(gl.BLEND);
    }
    gl.uniform4f(this.locGhost, 0, 0, 0, 0);
    /* The frame's own VRAM back on its unit. */
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
  }

  /* A VRAM image as a texture of its own, for `drawMeshOver`. */
  makeVramTexture(bytes) {
    const gl = this.gl;
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16UI, VRAM_W, VRAM_H, 0, gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 2));
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    return tex;
  }

  clearScene() {
    this.freeSceneMeshRecords(this.sceneMeshes.values());
    this.sceneMeshes.clear();
  }

  /* Empty the scene mesh table without freeing anything: the meshes come
   * back as a meshId -> record map. A scene rebuilt from mostly the same
   * meshes (play-view-scene-build.js) hands the records it keeps back
   * (`adoptSceneMesh`) and frees the rest (`freeSceneMeshRecords`). */
  detachSceneMeshes() {
    const old = this.sceneMeshes;
    this.sceneMeshes = new Map();
    return old;
  }

  /* The record `uploadSceneMesh` made for `meshId` (null when none). */
  sceneMeshRecord(meshId) {
    return this.sceneMeshes.get(meshId) || null;
  }

  /* Register a record `detachSceneMeshes` gave out under `meshId`. */
  adoptSceneMesh(meshId, rec) {
    this.sceneMeshes.set(meshId, rec);
  }

  /* Free the GL objects of detached records nothing adopted. */
  freeSceneMeshRecords(recs) {
    const gl = this.gl;
    for (const m of recs) {
      gl.deleteVertexArray(m.vao);
      gl.deleteBuffer(m.posBuf);
      gl.deleteBuffer(m.uvBuf);
      gl.deleteBuffer(m.ctBuf);
      if (m.flatBuf) gl.deleteBuffer(m.flatBuf);
      if (m.primRefBuf) gl.deleteBuffer(m.primRefBuf);
      gl.deleteBuffer(m.idxBuf);
      if (m.normBuf) gl.deleteBuffer(m.normBuf);
    }
  }

  /* Upload the walk-view continent ground heightfield. Attribute layout
   * matches `uploadSceneMesh` (positions f32x3, uvs u8x2, cbaTsb u16x2,
   * indices u32). Idempotent: re-upload overwrites. Pass empty arrays to
   * clear the ground (e.g. a kingdom with no resolvable walk `.MAP`).
   *
   * `flatRefs` (optional, f32 x8 per vertex: the cell's [x0, z0, x1, z1,
   * y00, y10, y01, y11], from `field_ground_flat_refs`) lets the overworld
   * draw each cell at its ordering-table bucket's depth, which is what keeps
   * the fog sheets retail's side of the ridges; without it the ground keeps
   * per-pixel depth. */
  uploadGround(positions, uvs, cbaTsb, indices, flatRefs) {
    const gl = this.gl;
    if (!positions || positions.length === 0 || !indices || indices.length === 0) {
      this.ground = null;
      return;
    }
    let g = this.ground;
    if (!g) {
      g = {
        vao: gl.createVertexArray(),
        posBuf: gl.createBuffer(),
        uvBuf:  gl.createBuffer(),
        ctBuf:  gl.createBuffer(),
        refBuf: gl.createBuffer(),
        idxBuf: gl.createBuffer(),
        indexCount: 0,
        aabb: null,
      };
      this.ground = g;
    }
    g.aabb = computeAabb(positions);
    gl.bindVertexArray(g.vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, g.posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locPos);
    gl.vertexAttribPointer(this.locPos, 3, gl.FLOAT, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, g.uvBuf);
    gl.bufferData(gl.ARRAY_BUFFER, uvs, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locUv);
    gl.vertexAttribPointer(this.locUv, 2, gl.UNSIGNED_BYTE, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, g.ctBuf);
    gl.bufferData(gl.ARRAY_BUFFER, cbaTsb, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locCbaTsb);
    gl.vertexAttribIPointer(this.locCbaTsb, 2, gl.UNSIGNED_SHORT, 0, 0);

    const haveRefs = !!(flatRefs && flatRefs.length === (positions.length / 3) * 8
      && this.locGroundRefXz >= 0 && this.locGroundRefY >= 0);
    g.hasRefs = haveRefs;
    if (haveRefs) {
      gl.bindBuffer(gl.ARRAY_BUFFER, g.refBuf);
      gl.bufferData(gl.ARRAY_BUFFER, flatRefs, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(this.locGroundRefXz);
      gl.vertexAttribPointer(this.locGroundRefXz, 4, gl.FLOAT, false, 32, 0);
      gl.enableVertexAttribArray(this.locGroundRefY);
      gl.vertexAttribPointer(this.locGroundRefY, 4, gl.FLOAT, false, 32, 16);
    } else {
      if (this.locGroundRefXz >= 0) gl.disableVertexAttribArray(this.locGroundRefXz);
      if (this.locGroundRefY >= 0) gl.disableVertexAttribArray(this.locGroundRefY);
    }

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, g.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

    g.indexCount = indices.length;
    gl.bindVertexArray(null);
  }

  /* Replace the ground's index list without re-uploading its vertices - the
   * visible-tile crop (`field_ground_indices_cropped`) re-issues it whenever
   * the camera's cell rectangle moves. An empty list draws no ground. */
  setGroundIndices(indices) {
    const g = this.ground;
    if (!g) return;
    const gl = this.gl;
    gl.bindVertexArray(g.vao);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, g.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.DYNAMIC_DRAW);
    gl.bindVertexArray(null);
    g.indexCount = indices.length;
  }

  /* Rewrite the ground's vertex positions (and its flat bucket-depth refs,
   * which carry the cell corner heights) in place - same vertex count, same
   * order as the `uploadGround` stream. The live floor-height ladder moves
   * the ground per frame on the scenes whose script animates it. */
  updateGroundPositions(positions, flatRefs) {
    const g = this.ground;
    if (!g || !positions || positions.length === 0) return;
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, g.posBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, positions);
    if (g.hasRefs && flatRefs && flatRefs.length === (positions.length / 3) * 8) {
      gl.bindBuffer(gl.ARRAY_BUFFER, g.refBuf);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, flatRefs);
    }
    g.aabb = computeAabb(positions);
  }

  /* Return the ground heightfield AABB (null until uploadGround has run). */
  getGroundAabb() {
    return this.ground ? this.ground.aabb : null;
  }

  /* Toggle the ground pass (wired to the "show terrain" checkbox). */
  setGroundEnable(on) {
    this.groundEnable = !!on;
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
    gl.clearColor(this.clearColor[0], this.clearColor[1], this.clearColor[2], this.clearColor[3]);
    /* A frame that builds on an earlier one (a cast's armed wash, under PSX
     * rasterisation: webgl-psx-target.js) has its picture loaded by the hook
     * and clears the depth alone. */
    gl.clear(this.loadFrameHook && this.loadFrameHook()
      ? gl.DEPTH_BUFFER_BIT
      : gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    /* Orbit-3D vs legacy ortho top-down: a cam carrying a `yaw` field
     * opts into the perspective orbit path (the world-overview page and
     * viewer.html's full-map mode both do); cams without it keep the
     * ortho projection. */
    const vp = (cam && cam.yaw != null)
      ? buildWorldOrbitVp(w, h, worldExtent, cam)
      : buildTopDownVp(w, h, worldExtent, cam);
    /* Log-depth write (webgl-shaders.js LOG_DEPTH_GLSL): opt-in by the play
     * page, and only under a perspective camera - an ortho frame's w is
     * constant. The screen-prim pass reads the flag back. */
    this.lastLogDepth = !!(this.logDepth && cam && cam.yaw != null);
    /* Enhanced lighting (identity unless the play page opted in). */
    this._stageLighting(vp);
    /* The one point of the frame where both this frame's camera and its lights
     * are known: the page settles what it held back until it could tell
     * whether anything will see it. One-shot, like `preSceneHook`. */
    if (this.settleHook) {
      const settle = this.settleHook;
      this.settleHook = null;
      placements = settle(placements, vp);
    }
    this._renderLightShadows(placements);

    /* Advance the water CLUT animation on wall-clock time, regardless
     * of whether the backdrop plane is enabled - the frame write also
     * lands in the VRAM CLUT row at (0, 506), which the continent
     * heightfield's water cells sample, so terrain water must shimmer
     * even when the backdrop pass is toggled off. */
    {
      const p = this.oceanParams;
      if (p.textured && p.frameCount > 0 && !p.walkerDriven) {
        const now = performance.now() / 1000;
        if (p.lastFrameAdvanceTs === 0) p.lastFrameAdvanceTs = now;
        if (now - p.lastFrameAdvanceTs >= p.frameDurationSec) {
          const steps = Math.floor((now - p.lastFrameAdvanceTs) / p.frameDurationSec);
          const next = (p.currentFrame + steps) % p.frameCount;
          this._uploadOceanFrame(next);
          p.lastFrameAdvanceTs += steps * p.frameDurationSec;
        }
      }
    }

    /* Ocean plane: drawn first so bulk-terrain meshes occlude it
     * through depth-test. Skipped when `setOceanColor(..., false)` is
     * the current state. When `setOceanAssets` has uploaded disc-side
     * data the textured pipeline takes over; otherwise we paint a
     * solid fallback colour. */
    if (this.oceanParams.enable) {
      const p = this.oceanParams;
      const ex = (worldExtent && worldExtent[0]) || 16320;
      const ez = (worldExtent && worldExtent[1]) || 16320;
      const cx = (cam && cam.centerX != null) ? cam.centerX : ex * 0.5;
      const cz = (cam && cam.centerZ != null) ? cam.centerZ : ez * 0.5;
      /* Under the perspective orbit camera a tilted view can see much
       * further toward the horizon than the ortho frame ever showed, so
       * widen the backdrop quad to keep the sea unbroken to the far
       * plane. */
      const horizonScale = (cam && cam.yaw != null)
        ? Math.max(p.extentScale, 8.0)
        : p.extentScale;
      const sx = ex * horizonScale;
      const sz = ez * horizonScale;
      /* model = T(cx, planeY, cz) * S(sx, 1, sz) */
      const model = new Float32Array([
        sx, 0,  0,  0,
        0,  1,  0,  0,
        0,  0,  sz, 0,
        cx, p.planeY, cz, 1,
      ]);
      const mvp = mulMat4(vp, model);
      gl.useProgram(this.oceanProgram);
      gl.uniformMatrix4fv(this.locOceanMvp, false, mvp);
      /* UV scale: world-units-per-quad × wraps-per-quad. The vertex
       * shader multiplies the unit-quad UV by this; the fragment
       * shader does fract() to tile. */
      const wrapsX = sx / p.tileWorldSize;
      const wrapsZ = sz / p.tileWorldSize;
      gl.uniform2f(this.locOceanUvScale, wrapsX, wrapsZ);
      /* World-anchor the pattern: offset UVs by the quad centre so the
       * waves stay put as the camera pans (the quad follows the cam). */
      gl.uniform2f(this.locOceanUvOffset,
        cx / p.tileWorldSize, cz / p.tileWorldSize);
      gl.uniform1i(this.locOceanTextured, p.textured ? 1 : 0);
      gl.uniform2f(this.locOceanSampleSize, p.sampleWidth, p.sampleHeight);
      /* Match the main program's ground water cells. Those are a generated
       * heightfield with no packet colour word, so they draw at the neutral
       * 0x80 modulation = 1.0; the backdrop plane takes the same factor and
       * the sea reads as one layer. (This tracked the old synthetic Lambert
       * shade before the textured path became retail's texel * colour / 128.) */
      gl.uniform1f(this.locOceanShade, 1.0);
      /* The ground's distance haze, so the backdrop stays one layer with
       * the water cells (origin: the camera centre unless pinned). */
      {
        const hp = this.hazeParams;
        const o = hp.origin || [cam ? cam.centerX : 0, 0, cam ? cam.centerZ : 0];
        gl.uniform4f(this.locOceanHaze, hp.enable ? 1 : 0, hp.eyeZ, hp.scale, p.tileWorldSize);
        gl.uniform2f(this.locOceanHazeOrigin, o[0], o[2]);
      }
      const c = p.color;
      gl.uniform4f(this.locOceanColor, c.r, c.g, c.b, 1.0);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.oceanTex);
      gl.uniform1i(this.locOceanTex, 0);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
      gl.uniform1i(this.locOceanClut, 1);
      gl.bindVertexArray(this.oceanVao);
      /* No depth write: the plane is a backdrop fill, not geometry. Retail
       * has no sea plane - the open sea inside the kingdom is the ground
       * heightfield's own water cells, and its rivers / lakes sit only
       * GROUND_SINK-adjusted 0.6 units above y = 0. A depth-writing plane
       * z-fights those cells at any oblique angle (the depth step at range
       * exceeds the gap), so the sea showed through the rivers and coast as
       * the camera tilted. Everything drawn after simply covers it. */
      gl.depthMask(false);
      gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
      gl.depthMask(true);
      gl.bindVertexArray(null);
    }

    /* The page's screen-primitive backdrops (`preSceneHook`, one-shot):
     * the primitives retail links into the farthest ordering-table bucket
     * - the cast colour wash, the overworld sky band - drawn before every
     * 3D draw, so a blended scene primitive (which writes no depth) lands
     * over them as it does over retail's first-drawn packets. */
    if (this.preSceneHook) {
      const hook = this.preSceneHook;
      this.preSceneHook = null;
      try { hook(); } catch (e) { /* presentation-only */ }
      gl.viewport(0, 0, w, h);
      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.depthMask(true);
      gl.disable(gl.CULL_FACE);
      gl.disable(gl.BLEND);
    }

    /* Nothing to draw with the textured program (no ground, no placed
     * meshes) - the ocean pass above already ran, so bail. */
    const haveGround = this.groundEnable && this.ground && this.ground.indexCount > 0;
    const havePlacements = this.sceneMeshes.size > 0 && placements.length > 0;
    if (!haveGround && !havePlacements) return;

    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locMvp, false, vp);
    if (this.locLogDepthOn) gl.uniform1i(this.locLogDepthOn, this.lastLogDepth ? 1 : 0);
    /* Assembled VPs add the retail screen-X mirror on top of the per-model
     * Y flip (two reflections), which inverts gl_FrontFacing - a pair's
     * visible copy is the back-facing one here (see u_pair_front). */
    gl.uniform1i(this.locPairFront, 0);
    /* Retail NCLIP winding rejection (0 unless the play page staged a
     * cutscene-camera frame this tick). */
    if (this.locNclipCull) gl.uniform1i(this.locNclipCull, this.nclipCull);
    /* Overworld curvature (0 = flat unless the play page staged a scale). */
    if (this.locCurve) gl.uniform1f(this.locCurve, this.overworldCurve);
    if (this.locCurveBias) gl.uniform1f(this.locCurveBias, this.overworldCurveBias || 0);
    /* Retail's per-primitive near reject (off unless the play page staged it). */
    if (this.locPrimNear) gl.uniform4fv(this.locPrimNear, this.primNear);
    /* The field ground's depth rule (0 = retail's buckets unless the play
     * page staged a drop). */
    if (this.locGroundDrop) gl.uniform1f(this.locGroundDrop, this.groundDrop || 0);
    /* The GTE's register limits (off unless the play page staged them). */
    if (this.locGteSat) gl.uniform4fv(this.locGteSat, this.gteSat);
    /* Prologue grade + depth cue (identity / off unless the play page
     * staged them this frame). */
    this._applyGradeCue();
    /* Camera-occlusion fade (identity unless the play page staged a focus
     * this frame; uniforms persist through the opaque + blend passes so
     * semi-transparent wall patches open up too). */
    this._applyOcclusionFade(vp, w, h);
    /* PSX rasterisation + dynamic light (identity unless the play page
     * opted in); uniforms persist through the opaque + blend passes. */
    this._applyRenderToggles(w, h);
    /* Cutout discard ON (retail semantics): texel 0 with STP 0 is fully
     * transparent, which is what makes the crossed-quad billboard trees
     * read as foliage instead of solid star-shaped slabs. The old
     * `u_no_discard = 1` silhouette fallback dated from the misaligned
     * overview-frame era when placements sampled the wrong TIMs' CLUT
     * rows; the walk-frame path uploads the kingdom's real VRAM image, so
     * CLUTs now resolve exactly like retail. */
    gl.uniform1i(this.locNoDisc, 0);
    /* Opaque pass: defer semi-transparent (ABE) fragments; the blend pass
     * after the placement loop re-draws them per ABR mode. */
    gl.uniform1i(this.locSemiPass, 0);
    /* Untextured-fill branch: off for the ground pass; re-enabled per placed
     * mesh below when it carries an untextured vertex-colour half (the
     * viewer full-map path). The context-global constant keeps disabled
     * a_flat_rgba attributes reading as "textured, neutral modulation". */
    gl.uniform1i(this.locUseFlatColors, 0);
    this._setNeutralPacketColor();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);
    /* Distance haze (setDistanceHaze): the virtual walk focus is the
     * camera centre unless the page pinned one. Off (mode 0) for every
     * draw but the ground and the hazeDeco stamps. */
    {
      const hp = this.hazeParams;
      this._hazeOrigin = hp.origin || [cam ? cam.centerX : 0, 0, cam ? cam.centerZ : 0];
      gl.uniform3f(this.locFogOrigin, this._hazeOrigin[0], this._hazeOrigin[1], this._hazeOrigin[2]);
      gl.uniform1f(this.locHazeEyeZ, hp.eyeZ);
      gl.uniform1f(this.locHazeScale, hp.scale);
      gl.uniform1i(this.locFogEnable, 0);
    }

    /* Group draws by meshId so we bind each VAO once per frame. A placement
     * wholly off screen goes no further: most of a town is, and its draws
     * would cost three passes for no fragment. Nothing here changes a pixel,
     * so this is not a port's cull of retail geometry. The mesh keeps its seat
     * in the map even when none of its placements is on screen, or the meshes
     * that follow would draw in another order, and depth ties and blend runs
     * read that order. A posed mesh that stays off screen keeps its normals
     * dirty for the frame it comes back.
     *
     * PSX mode draws them all: the GTE's register limits move a far corner
     * from off screen to on (gteSaturateClip), so no bound on the vertices
     * bounds what its vertex stage writes. The overworld's curvature moves Y,
     * so a curved frame judges X alone. */
    const byMesh = new Map();
    const cullOff = !this.psxMode;
    const xOnly = this.overworldCurve > 0 || this.overworldCurveBias !== 0;
    const cullMvp = this._cullMvp || (this._cullMvp = new Float32Array(16));
    for (const p of placements) {
      const m = this.sceneMeshes.get(p.meshId);
      if (!m) continue;
      let list = byMesh.get(p.meshId);
      if (!list) { list = []; byMesh.set(p.meshId, list); }
      const b = m.aabb;
      if (cullOff && b) {
        mulMat4Into(cullMvp, vp, this._placementModel(p, m));
        if (boxOffScreen(cullMvp, b.cx, b.cy, b.cz, b.sx * 0.5, b.sy * 0.5, b.sz * 0.5, xOnly)) continue;
      }
      list.push(p);
    }
    let flatColorsOn = false;
    /* Which cue record is currently staged, so a frame with no per-draw cues
     * costs no extra uniform writes. `undefined` = the frame-global one. */
    let cueOn;
    /* Whether a decoration's distance-haze IR0 is staged (`_setHazeDraw`). */
    let hazeOn = false;
    /* Whether an object-effect clip is staged (`_setEffectClip`). */
    let eclipOn = this._setEffectClip(null, true);
    /* The NCLIP word staged for the current placement (`_setPlacementNclip`);
     * the frame's `nclipCull` until a placement carries its own. */
    let nclipOn = this.nclipCull;
    /* Whether the whole-draw far bucket (`u_far_slice`) is staged. */
    let farOn = false;
    /* The opaque draws - the ground, then every placement - as one
     * closure, so the SSAO pre-pass can issue them a second time into its
     * G-buffer under the same uniforms (the trackers above carry across
     * both runs). */
    const drawOpaque = () => {
      /* Continent ground heightfield: one draw, fixed Y-flip model (the
       * mesh is already in world coords, PSX +Y down). Drawn after the
       * ocean (so land occludes water through depth-test) and before the
       * landmark placements (so they sit on top). Per-cell UVs/CBA/tpage
       * sample the kingdom slot-0 terrain atlas already in u_vram. */
      if (this.groundEnable && this.ground && this.ground.indexCount > 0) {
        /* model = diag(1, -1, 1): flip PSX +Y(down) to world +Y(up), no
         * translation - matches placementModelScaled(0, 0, 0, 1). */
        const flipY = new Float32Array([
          1, 0, 0, 0,
          0, -1, 0, 0,
          0, 0, 1, 0,
          0, 0, 0, 1,
        ]);
        gl.uniformMatrix4fv(this.locModel, false, flipY);
        /* The ground is environment: the occlusion fade may dissolve it. */
        gl.uniform1i(this.locOcclAllow, 1);
        this._setHazeDraw(1);
        gl.bindVertexArray(this.ground.vao);
        gl.drawElements(gl.TRIANGLES, this.ground.indexCount, gl.UNSIGNED_INT, 0);
        gl.bindVertexArray(null);
        this._setHazeDraw(0);
      }

      /* Set on the first draw, then only on a change. */
      let occlAllow = -1;
      let meanDepth = -1;
      for (const [meshId, list] of byMesh) {
        const m = this.sceneMeshes.get(meshId);
        if (m.indexCount === 0 || list.length === 0) continue;
        /* Hybrid env meshes carry an untextured vertex-colour half; flip the
         * FS flat branch on only for them (state-change once per mesh). */
        const wantFlat = !!m.hasFlat;
        if (wantFlat !== flatColorsOn) {
          gl.uniform1i(this.locUseFlatColors, wantFlat ? 1 : 0);
          flatColorsOn = wantFlat;
        }
        this._ensureNormals(m);
        gl.bindVertexArray(m.vao);
        for (const p of list) {
          const model = this._placementModel(p, m);
          const wantCue = p.cue
            || (p.decoCue && overworldDecorationCue(vp, model, this.overworldCurve))
            || this.cueParams;
          if (wantCue !== cueOn) { this._setCue(wantCue); cueOn = wantCue; }
          gl.uniformMatrix4fv(this.locModel, false, model);
          if (p.hazeDeco || hazeOn) {
            this._setHazeDraw(p.hazeDeco ? 2 : 0, model[12], model[14]);
            hazeOn = !!p.hazeDeco;
          }
          /* Actor draws (the player, NPCs - `noOccl` on the placement) must
           * never dissolve; environment placements may. */
          const wantOccl = p.noOccl ? 0 : 1;
          if (wantOccl !== occlAllow) { gl.uniform1i(this.locOcclAllow, wantOccl); occlAllow = wantOccl; }
          nclipOn = this._setPlacementNclip(p.nclip, nclipOn);
          const wantMean = Number(p.meanDepth) || 0;
          if (this.locPrimMeanDepth && wantMean !== meanDepth) {
            gl.uniform1f(this.locPrimMeanDepth, wantMean);
            meanDepth = wantMean;
          }
          eclipOn = this._setEffectClip(p.effectClip, eclipOn);
          /* A far-bucket placement (the battle ground grid) writes its depth
           * into the far slice, behind everything else on the frame. */
          const wantFar = !!p.farBucket;
          if (wantFar !== farOn && this.locFarSlice) {
            gl.uniform1i(this.locFarSlice, wantFar ? 1 : 0);
            farOn = wantFar;
          }
          gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
        }
      }
      if (farOn) gl.uniform1i(this.locFarSlice, 0);
      /* Hand the frame-global cue back before the blend pass / next frame. */
      if (cueOn !== undefined && cueOn !== this.cueParams) {
        this._setCue(this.cueParams);
        cueOn = this.cueParams;
      }
    };
    drawOpaque();
    /* Screen-space ambient occlusion + contact shadows (an enhancement,
     * webgl-ssao.js): the opaque draws again into the G-buffer with the
     * program's pre-pass switch on, then the AO multiplies the frame -
     * before the blend pass, so the semi-transparent prims, the glow and
     * every later layer stay undarkened. `ssaoFrame(vp)` is the page's
     * hook into the engine's packet (`play_ssao_frame`); empty = off. */
    this._drawSsao(vp, w, h, drawOpaque);

    /* Blend pass: re-draw the deferred semi-transparent (ABE) prims over the
     * finished opaque scene, one fixed-function blend state per ABR mode.
     * Depth-tested against the opaque scene but not depth-written, so blend
     * prims never occlude. Mirrors the retail GPU: the ordering table draws
     * blend prims against the already-rendered background. */
    let blendOn = false;
    let strictOn = false;
    let blendOccl = -1;
    let blendMean = -1;
    for (const [meshId, list] of byMesh) {
      const m = this.sceneMeshes.get(meshId);
      if (!m || !m.semiRanges || m.indexCount === 0 || list.length === 0) continue;
      if (!blendOn) {
        gl.enable(gl.BLEND);
        gl.depthMask(false);
        gl.uniform1i(this.locSemiPass, 1);
        blendOn = true;
      }
      const wantFlat = !!m.hasFlat;
      if (wantFlat !== flatColorsOn) {
        gl.uniform1i(this.locUseFlatColors, wantFlat ? 1 : 0);
        flatColorsOn = wantFlat;
      }
      gl.bindVertexArray(m.vao);
      for (const p of list) {
        const model = this._placementModel(p, m);
        const wantCue = p.cue
          || (p.decoCue && overworldDecorationCue(vp, model, this.overworldCurve))
          || this.cueParams;
        if (wantCue !== cueOn) { this._setCue(wantCue); cueOn = wantCue; }
        /* Strictly-nearer depth test for placements that ask for it (the
         * arts after-image ghosts): at LEQUAL a ghost pose coincident with
         * the live body passes on every fragment and washes the whole mesh
         * additive; LESS rejects the equal-depth overlap so the tint only
         * builds where the delayed pose has separated - the retail look of
         * a trail drawn in a deeper OT bucket than the body. */
        const wantStrict = !!p.strictDepth;
        if (wantStrict !== strictOn) {
          gl.depthFunc(wantStrict ? gl.LESS : gl.LEQUAL);
          strictOn = wantStrict;
        }
        gl.uniformMatrix4fv(this.locModel, false, model);
        if (p.hazeDeco || hazeOn) {
          this._setHazeDraw(p.hazeDeco ? 2 : 0, model[12], model[14]);
          hazeOn = !!p.hazeDeco;
        }
        const wantOccl = p.noOccl ? 0 : 1;
        if (wantOccl !== blendOccl) { gl.uniform1i(this.locOcclAllow, wantOccl); blendOccl = wantOccl; }
        nclipOn = this._setPlacementNclip(p.nclip, nclipOn);
        const wantMean = Number(p.meanDepth) || 0;
        if (this.locPrimMeanDepth && wantMean !== blendMean) {
          gl.uniform1f(this.locPrimMeanDepth, wantMean);
          blendMean = wantMean;
        }
        eclipOn = this._setEffectClip(p.effectClip, eclipOn);
        for (const r of m.semiRanges) {
          this._setSemiBlend(r.mode);
          gl.drawElements(gl.TRIANGLES, r.count, gl.UNSIGNED_INT, r.start * 4);
        }
      }
    }
    if (cueOn !== undefined && cueOn !== this.cueParams) {
      this._setCue(this.cueParams);
      cueOn = this.cueParams;
    }
    if (blendOn) {
      gl.disable(gl.BLEND);
      gl.depthMask(true);
      gl.blendEquation(gl.FUNC_ADD);
      gl.uniform1i(this.locSemiPass, 0);
    }
    if (strictOn) gl.depthFunc(gl.LEQUAL);
    if (eclipOn) this._setEffectClip(null, true);
    if (hazeOn) this._setHazeDraw(0);
    this._setPlacementNclip(undefined, nclipOn);
    if (this.locPrimMeanDepth) gl.uniform1f(this.locPrimMeanDepth, 0);
    gl.bindVertexArray(null);
    /* Enhanced lighting's halos + light shafts over the finished scene. */
    this._drawGlow(vp);
    /* The frame's view-projection, for overlay passes drawn after this
     * one into the same depth buffer (the world overview's Mist). */
    this.lastVp = vp;
  }

  /* The SSAO pre-pass and resolve inside renderAssembled. Inert unless the
   * page installed `ssaoFrame` and it returns a packet; a driver that
   * cannot build the pass turns it off for the session. */
  _drawSsao(vp, w, h, drawOpaque) {
    if (!this.ssaoFrame || this._ssaoBroken || !this.locGbuf) return;
    if (typeof window.LegaiaSsaoPass !== 'function') return;
    let frame = null;
    try { frame = this.ssaoFrame(vp); } catch (e) { frame = null; }
    if (!frame || frame.length < 24) return;
    if (!(frame instanceof Float32Array)) frame = Float32Array.from(frame);
    const gl = this.gl;
    /* A WebXR session draws each eye into its own framebuffer and viewport;
     * the pass stays out of VR (an eye's G-buffer would need the eye's
     * viewport). */
    const outFbo = gl.getParameter(gl.FRAMEBUFFER_BINDING);
    if (outFbo !== null) return;
    try {
      if (!this._ssaoPass) this._ssaoPass = new window.LegaiaSsaoPass(gl);
      const pass = this._ssaoPass;
      if (pass.beginPrepass(w, h)) {
        /* (Re)creating the pass's targets binds them on the active unit: a
         * pre-pass drawn over one samples a colour target through `u_vram`,
         * and every draw of that frame fails with a sampler-type mismatch. */
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, this.tex);
        gl.uniform1i(this.locGbuf, 1);
        gl.uniform4f(this.locGbufK, frame[0], frame[1], frame[2], 0);
        gl.uniform4f(this.locGbufView, 0, 0, w, h);
        drawOpaque();
        gl.uniform1i(this.locGbuf, 0);
        gl.bindVertexArray(null);
        pass.resolve(frame, outFbo, !!this.ssaoDebug);
      }
    } catch (e) {
      this._ssaoBroken = true;
      try { console.warn('ssao pass disabled:', e); } catch (_) { /* no console */ }
    }
    /* Hand the frame back to the scene pass exactly as it left it. */
    gl.bindFramebuffer(gl.FRAMEBUFFER, outFbo);
    gl.viewport(0, 0, w, h);
    gl.clearColor(this.clearColor[0], this.clearColor[1], this.clearColor[2], this.clearColor[3]);
    gl.useProgram(this.program);
    gl.uniform1i(this.locGbuf, 0);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.depthMask(true);
    gl.disable(gl.BLEND);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
  }

  /* Whether a sphere in the draw frame can touch no pixel and no shadow this
   * frame: off screen by the sides `renderAssembled` culls on, and out of reach
   * of every light's frustum. False under PSX mode, whose vertex stage can bring
   * a far corner back on screen. */
  sphereUnseen(vp, x, y, z, r) {
    if (this.psxMode) return false;
    const wc = vp[3] * x + vp[7] * y + vp[11] * z + vp[15];
    const slack = Math.abs(wc) * 1e-3 + 1e-3;
    const xOnly = this.overworldCurve > 0 || this.overworldCurveBias !== 0;
    let off = false;
    for (let axis = 0; axis < (xOnly ? 1 : 2) && !off; axis++) {
      const pc = vp[axis] * x + vp[axis + 4] * y + vp[axis + 8] * z + vp[axis + 12];
      for (let sign = 1; sign >= -1; sign -= 2) {
        const reach = r * Math.hypot(vp[3] + sign * vp[axis], vp[7] + sign * vp[axis + 4],
          vp[11] + sign * vp[axis + 8]);
        if (wc + sign * pc + reach < -slack) off = true;
      }
    }
    if (!off) return false;
    const f = this.lightFrame;
    if (!this.dynLighting || !this.dynShadows || !f || !this.shadowProgram) return true;
    const g = 1 / Math.tan(SHADOW_FOV / 2);
    for (let li = 0; li < f.count; li++) {
      const lr = f.pr[li * 4 + 3];
      const lfar = Math.max(lr, Math.max(lr * SHADOW_NEAR_FRAC, SHADOW_NEAR_MIN) * 2);
      const side = lfar / g + r;
      if (Math.abs(x - f.pr[li * 4]) <= side && Math.abs(z - f.pr[li * 4 + 2]) <= side
          && y - r <= f.pr[li * 4 + 1] && y + r >= f.pr[li * 4 + 1] - lfar) return false;
    }
    return true;
  }

  /* Model matrix for one renderAssembled placement (shared by the opaque
   * and blend passes so both draw at identical transforms). */
  _placementModel(p, m) {
    /* Engine-composed override: the battle FX layer hands whole model
     * matrices through (billboards ride an identity, FX parts ride the
     * native T*Ry*Rx*Rz*flip composition with the retail 4x world scale
     * already folded in), so no JS-side transform model exists to drift. */
    if (p.model) return p.model;
    const scale = (p.scale != null) ? p.scale : MESH_SCALE;
    const rotY = p.rotY || 0;
    const aabb = m.aabb;
    /* A frame asks for a placement's model once per pass and per light, and a
     * static placement's never moves: the last answer stands while its inputs
     * do. */
    const c = p._modelMemo;
    /* Only a centroid anchor reads the box, and a posed mesh's box is new with
     * every pose. */
    const memoBox = p.anchor === 'centroid' ? aabb : null;
    if (c && c.x === p.x && c.y === p.y && c.z === p.z && c.rotY === rotY && c.scale === scale
        && c.anchor === p.anchor && c.aabb === memoBox) {
      return c.model;
    }
    let model;
    if ((p.anchor === 'centroid') && aabb) {
      model = placementModelCentered(p.x, p.z, rotY, scale, aabb);
    } else if (p.y != null) {
      /* Walk-frame landmarks carry a world Y (floor-LUT height) so they
       * sit on the continent heightfield instead of the y=0 plane. */
      model = placementModelScaledY(p.x, p.y, p.z, rotY, scale);
    } else {
      model = placementModelScaled(p.x, p.z, rotY, scale);
    }
    p._modelMemo = { x: p.x, y: p.y, z: p.z, rotY, scale, anchor: p.anchor, aabb: memoBox, model };
    return model;
  }

  /* GL blend state for one PSX ABR semi-transparency mode (B = backbuffer,
   * F = fragment): 0 = 0.5B + 0.5F, 1 = B + F, 2 = B - F, 3 = B + 0.25F.
   * Constant-alpha factors carry the 0.5 / 0.25 weights, so no shader
   * pre-scale is needed. Same table as engine-render's psx_blend. */
  _setSemiBlend(mode) {
    const gl = this.gl;
    gl.blendEquation(mode === 2 ? gl.FUNC_REVERSE_SUBTRACT : gl.FUNC_ADD);
    if (mode === 0) {
      gl.blendColor(0, 0, 0, 0.5);
      gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
    } else if (mode === 3) {
      gl.blendColor(0, 0, 0, 0.25);
      gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE);
    } else {
      gl.blendFunc(gl.ONE, gl.ONE);
    }
  }


});
