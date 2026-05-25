/* webgl-tmd.js - WebGL2 textured TMD renderer for the WASM viewer.
 *
 * Mirrors the engine-render VRAM-mesh pipeline: a 1024x512 R16UI VRAM
 * texture, per-vertex (position, uv, cba_tsb) attributes, and a fragment
 * shader that does 4bpp/8bpp/15bpp + CLUT lookup against the VRAM texture.
 *
 * Required script load order (classic globals):
 *   1. webgl-shaders.js  - VS/FS shader sources + VRAM/FOG constants
 *   2. webgl-math.js     - matrix + placement helpers + compileProgram
 *   3. webgl-tmd.js      - this file (TmdRenderer class)
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
 *   { meshId, x, z, y?, rotY?, scale?, anchor? }
 *     scale  - per-placement world-scale (defaults to MESH_SCALE).
 *     y      - optional world height for the anchor (walk-frame landmarks
 *              pass the floor-LUT height so they sit on the heightfield;
 *              omitted -> anchor on the y=0 plane). Ignored when anchor is
 *              'centroid'.
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

    /* Walk-view continent ground: one big heightfield mesh (per-cell
     * terrain-atlas UVs + [clut, tpage]) that textures against the same
     * VRAM as the landmark meshes. Built once per kingdom by
     * `uploadGround`, drawn by `renderAssembled` before the placement
     * loop so landmarks sit on top. `null` until uploaded. The mesh is
     * already in world coords (col*128, -lut[nibble], row*128), so it
     * draws with a fixed Y-flip model (scale 1, no offset) - the same
     * (1,-1,1) flip the placement models apply. */
    this.ground = null;
    this.groundEnable = true;

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

    /* Ocean plane: 4bpp indexed texture sampled with a 16-entry CLUT
     * that gets swapped every animation frame. The plane lives at
     * y = 0 and spans a single unit square in world units; per-frame
     * we multiply through u_mvp to extend it across the kingdom's
     * world extent + margin. */
    this.oceanProgram   = compileProgram(gl, OCEAN_VS_SRC, OCEAN_FS_SRC);
    this.locOceanMvp        = gl.getUniformLocation(this.oceanProgram, 'u_mvp');
    this.locOceanUvScale    = gl.getUniformLocation(this.oceanProgram, 'u_uv_scale');
    this.locOceanColor      = gl.getUniformLocation(this.oceanProgram, 'u_color');
    this.locOceanTex        = gl.getUniformLocation(this.oceanProgram, 'u_ocean_tex');
    this.locOceanClut       = gl.getUniformLocation(this.oceanProgram, 'u_ocean_clut');
    this.locOceanTextured   = gl.getUniformLocation(this.oceanProgram, 'u_ocean_textured');
    this.locOceanSampleSize = gl.getUniformLocation(this.oceanProgram, 'u_ocean_sample_size');
    this.locOceanPos        = gl.getAttribLocation(this.oceanProgram, 'a_position');
    this.locOceanUv         = gl.getAttribLocation(this.oceanProgram, 'a_uv_world');
    this.oceanVao           = gl.createVertexArray();
    this.oceanPosBuf        = gl.createBuffer();
    this.oceanUvBuf         = gl.createBuffer();
    this.oceanIdxBuf        = gl.createBuffer();
    gl.bindVertexArray(this.oceanVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.oceanPosBuf);
    /* Unit quad in XZ at y=0, centred at origin (extents -0.5..+0.5). */
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
      -0.5, 0, -0.5,
       0.5, 0, -0.5,
       0.5, 0,  0.5,
      -0.5, 0,  0.5,
    ]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locOceanPos);
    gl.vertexAttribPointer(this.locOceanPos, 3, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.oceanUvBuf);
    /* UV matches XZ position so it's easy to compute world-space tiling. */
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
      -0.5, -0.5,
       0.5, -0.5,
       0.5,  0.5,
      -0.5,  0.5,
    ]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.locOceanUv);
    gl.vertexAttribPointer(this.locOceanUv, 2, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.oceanIdxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER,
      new Uint16Array([0, 1, 2, 0, 2, 3]), gl.STATIC_DRAW);
    gl.bindVertexArray(null);

    /* Ocean texture: 128×256 R8UI (4bpp 256-pixel-wide tile packed 2/byte). */
    this.oceanTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.oceanTex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R8UI, 128, 256);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);

    /* Ocean CLUT: 16×1 R16UI (16 BGR555 entries, rewritten per frame). */
    this.oceanClutTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R16UI, 16, 1);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    /* Per-kingdom ocean override. The viewer pushes
     * `setOceanColor({ r, g, b }, enable)` once per kingdom switch so the
     * plane swaps to the captured tint without a per-frame uniform set.
     * When `setOceanAssets()` has uploaded a real texture + animation
     * frames, the textured pipeline takes over and the fallback colour
     * only matters where the CLUT samples to entry 0 (transparent). */
    this.oceanParams = {
      enable: 0,
      color: { r: 0.12, g: 0.14, b: 0.39 },  /* #1F2466, the retail fallback */
      planeY: 0.0,
      extentScale: 2.5,    /* world-extent multiplier so the quad reaches past clip */
      tileWorldSize: 256,  /* world units per ocean-texture wrap */
      /* Logical-pixel region of the texture page that holds ocean data.
       * The retail TIM uploads a 256×256 page but only the top-left
       * 96×96 holds the wave-ramp ocean tile; the rest is shared with
       * other tiles in 4bpp mode and reads as CLUT-entry-0 padding
       * inside the kingdom bundle. Tunable via setOceanAssets. */
      sampleWidth: 96,
      sampleHeight: 96,
      /* Set when setOceanAssets() has uploaded real disc data. */
      textured: false,
      animationFrames: null,   /* Uint16Array, 13×16 entries flat */
      frameCount: 0,
      currentFrame: 0,
      /* How many wall-clock seconds between animation steps. 60 FPS
       * retail / 13 frames = full cycle in ~0.22s (4.6 Hz) - we slow
       * that down to roughly the visible rate in the user's screenshot. */
      frameDurationSec: 1 / 8,
      lastFrameAdvanceTs: 0,
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
   * `legaia_web_viewer::ocean::find_ocean_assets`. `animationFrames` is
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
   * CLUT texture. Called from renderAssembled when wall-clock time
   * crosses `frameDurationSec`. */
  _uploadOceanFrame(idx) {
    const p = this.oceanParams;
    if (!p.animationFrames || idx >= p.frameCount) return;
    const slice = p.animationFrames.subarray(idx * 16, (idx + 1) * 16);
    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texSubImage2D(
      gl.TEXTURE_2D, 0, 0, 0, 16, 1,
      gl.RED_INTEGER, gl.UNSIGNED_SHORT,
      slice,
    );
    p.currentFrame = idx;
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

  /* Upload the walk-view continent ground heightfield. Attribute layout
   * matches `uploadSceneMesh` (positions f32x3, uvs u8x2, cbaTsb u16x2,
   * indices u32). Idempotent: re-upload overwrites. Pass empty arrays to
   * clear the ground (e.g. a kingdom with no resolvable walk `.MAP`). */
  uploadGround(positions, uvs, cbaTsb, indices) {
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

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, g.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

    g.indexCount = indices.length;
    gl.bindVertexArray(null);
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
    gl.clearColor(0.04, 0.05, 0.08, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    const vp = buildTopDownVp(w, h, worldExtent, cam);

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
      const sx = ex * p.extentScale;
      const sz = ez * p.extentScale;
      /* Advance animation frame based on wall-clock. */
      if (p.textured && p.frameCount > 0) {
        const now = performance.now() / 1000;
        if (p.lastFrameAdvanceTs === 0) p.lastFrameAdvanceTs = now;
        if (now - p.lastFrameAdvanceTs >= p.frameDurationSec) {
          const steps = Math.floor((now - p.lastFrameAdvanceTs) / p.frameDurationSec);
          const next = (p.currentFrame + steps) % p.frameCount;
          this._uploadOceanFrame(next);
          p.lastFrameAdvanceTs += steps * p.frameDurationSec;
        }
      }
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
      gl.uniform1i(this.locOceanTextured, p.textured ? 1 : 0);
      gl.uniform2f(this.locOceanSampleSize, p.sampleWidth, p.sampleHeight);
      const c = p.color;
      gl.uniform4f(this.locOceanColor, c.r, c.g, c.b, 1.0);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.oceanTex);
      gl.uniform1i(this.locOceanTex, 0);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, this.oceanClutTex);
      gl.uniform1i(this.locOceanClut, 1);
      gl.bindVertexArray(this.oceanVao);
      gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
      gl.bindVertexArray(null);
    }

    /* Nothing to draw with the textured program (no ground, no placed
     * meshes) - the ocean pass above already ran, so bail. */
    const haveGround = this.groundEnable && this.ground && this.ground.indexCount > 0;
    const havePlacements = this.sceneMeshes.size > 0 && placements.length > 0;
    if (!haveGround && !havePlacements) return;

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
      gl.bindVertexArray(this.ground.vao);
      gl.drawElements(gl.TRIANGLES, this.ground.indexCount, gl.UNSIGNED_INT, 0);
      gl.bindVertexArray(null);
    }

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
        let model;
        if (useCentroid) {
          model = placementModelCentered(p.x, p.z, p.rotY || 0, scale, m.aabb);
        } else if (p.y != null) {
          /* Walk-frame landmarks carry a world Y (floor-LUT height) so they
           * sit on the continent heightfield instead of the y=0 plane. */
          model = placementModelScaledY(p.x, p.y, p.z, p.rotY || 0, scale);
        } else {
          model = placementModelScaled(p.x, p.z, p.rotY || 0, scale);
        }
        gl.uniformMatrix4fv(this.locModel, false, model);
        gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
      }
    }
    gl.bindVertexArray(null);
  }


  dispose() {
    const gl = this.gl;
    this.clearScene();
    if (this.ground) {
      gl.deleteVertexArray(this.ground.vao);
      gl.deleteBuffer(this.ground.posBuf);
      gl.deleteBuffer(this.ground.uvBuf);
      gl.deleteBuffer(this.ground.ctBuf);
      gl.deleteBuffer(this.ground.idxBuf);
      this.ground = null;
    }
    gl.deleteProgram(this.program);
    gl.deleteVertexArray(this.vao);
    gl.deleteBuffer(this.posBuf);
    gl.deleteBuffer(this.uvBuf);
    gl.deleteBuffer(this.ctBuf);
    gl.deleteBuffer(this.idxBuf);
    gl.deleteTexture(this.tex);
    gl.deleteProgram(this.oceanProgram);
    gl.deleteVertexArray(this.oceanVao);
    gl.deleteBuffer(this.oceanPosBuf);
    gl.deleteBuffer(this.oceanUvBuf);
    gl.deleteBuffer(this.oceanIdxBuf);
    gl.deleteTexture(this.oceanTex);
    gl.deleteTexture(this.oceanClutTex);
  }
}


window.TmdRenderer = TmdRenderer;
