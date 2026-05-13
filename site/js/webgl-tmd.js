/* webgl-tmd.js - WebGL2 textured TMD renderer for the WASM viewer.
 *
 * Mirrors the engine-render VRAM-mesh pipeline: a 1024x512 R16UI VRAM
 * texture, per-vertex (position, uv, cba_tsb) attributes, and a fragment
 * shader that does 4bpp/8bpp/15bpp + CLUT lookup against the VRAM texture.
 *
 * Public API:
 *   const r = new TmdRenderer(canvas);
 *   r.uploadVram(vramBytes);  // 1MB Uint8Array
 *   r.uploadMesh(positions, uvs, cbaTsb, indices);
 *   r.render(yaw, pitch, center, radius);
 *   r.dispose();
 */

const VRAM_W = 1024;
const VRAM_H = 512;

const VS_SRC = `#version 300 es
precision highp float;
precision highp int;

uniform mat4 u_mvp;
uniform mat4 u_model;   /* per-draw model matrix (identity for single-mesh mode) */

in vec3 a_position;
in vec2 a_uv_byte;       /* 0..255 each, sent as Uint8x2 normalised=false */
in uvec2 a_cba_tsb;

out vec3 v_world;
out vec2 v_uv;          /* interpolated linearly across the triangle */
flat out uvec2 v_cba_tsb;

void main() {
  vec4 world_pos = u_model * vec4(a_position, 1.0);
  v_world = world_pos.xyz;
  v_uv = a_uv_byte;
  v_cba_tsb = a_cba_tsb;
  gl_Position = u_mvp * world_pos;
}
`;

const FS_SRC = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;
uniform vec3 u_light;
/* When non-zero, render transparent samples as opaque (with a tinted
 * fallback so they're visible). Used by the assembled top-view map where
 * CLUT collisions are expected and discarded fragments leave holes. */
uniform int u_no_discard;

in vec3 v_world;
in vec2 v_uv;
flat in uvec2 v_cba_tsb;

out vec4 o_color;

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
  o_color = vec4(color.rgb * shade, 1.0);
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
    this.locPos     = gl.getAttribLocation(this.program, 'a_position');
    this.locUv      = gl.getAttribLocation(this.program, 'a_uv_byte');
    this.locCbaTsb  = gl.getAttribLocation(this.program, 'a_cba_tsb');

    this.vao    = gl.createVertexArray();
    this.posBuf = gl.createBuffer();
    this.uvBuf  = gl.createBuffer();
    this.ctBuf  = gl.createBuffer();
    this.idxBuf = gl.createBuffer();
    this.tex    = gl.createTexture();
    /* Per-mesh GL state for the assembled (multi-mesh world) path. Indexed
     * by a caller-supplied meshId (typically the kingdom pack slot). Each
     * entry holds its own VAO + buffers so we can switch meshes cheaply
     * inside a single render frame. */
    this.sceneMeshes = new Map();

    /* Allocate the VRAM texture once (R16UI 1024x512). */
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R16UI, VRAM_W, VRAM_H);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

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
      };
      this.sceneMeshes.set(meshId, m);
    }
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
        const model = placementModel(p.x, p.z, p.rotY || 0);
        gl.uniformMatrix4fv(this.locModel, false, model);
        gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
      }
    }
    gl.bindVertexArray(null);
  }

  /* ---------- Top-down wireframe overlay path ------------------------ */

  /* Upload the slot-4 wireframe line list returned by
   * `LegaiaViewer::slot4_wireframe_lines`. Decodes the packed buffer
   * (4-byte line_count + 12 bytes per line) into a per-vertex float
   * stream the line shader can draw. Per-body palette is computed once
   * and held until the next upload.
   *
   * Format (little-endian, matches Rust packing in
   * crates/web-viewer/src/lib.rs::slot4_wireframe_lines):
   *
   *   u32 line_count
   *   per line:
   *     u8  body_index
   *     u8  group_index_low
   *     u8  group_index_high
   *     u8  pad
   *     i16 x0, i16 z0, i16 x1, i16 z1   (X-Z in world units; Y=0)
   */
  uploadWireframeLines(packed) {
    if (!this.wireProgram) this.initWireframePipeline();
    const gl = this.gl;
    const view = new DataView(packed.buffer, packed.byteOffset, packed.byteLength);
    const lineCount = view.getUint32(0, true);
    /* 2 vertices per line; per vertex: 3 floats (x,y,z) + 3 floats (r,g,b) = 24 bytes. */
    const verts = new Float32Array(lineCount * 2 * 6);
    let p = 4;
    let v = 0;
    for (let i = 0; i < lineCount; i++) {
      const bodyIdx = view.getUint8(p); p += 4; /* +pad bytes */
      const x0 = view.getInt16(p, true); p += 2;
      const z0 = view.getInt16(p, true); p += 2;
      const x1 = view.getInt16(p, true); p += 2;
      const z1 = view.getInt16(p, true); p += 2;
      const [r, g, b] = wireframeBodyColor(bodyIdx);
      /* vert 0 */
      verts[v++] = x0; verts[v++] = 0; verts[v++] = z0;
      verts[v++] = r;  verts[v++] = g; verts[v++] = b;
      /* vert 1 */
      verts[v++] = x1; verts[v++] = 0; verts[v++] = z1;
      verts[v++] = r;  verts[v++] = g; verts[v++] = b;
    }
    gl.bindVertexArray(this.wireVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.wireVbo);
    gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.wireLocPos);
    gl.vertexAttribPointer(this.wireLocPos, 3, gl.FLOAT, false, 24, 0);
    gl.enableVertexAttribArray(this.wireLocColor);
    gl.vertexAttribPointer(this.wireLocColor, 3, gl.FLOAT, false, 24, 12);
    gl.bindVertexArray(null);
    this.wireVertexCount = lineCount * 2;
  }

  /* Upload a topology-free point cloud (`LegaiaViewer::slot4_wireframe_points`).
   * Same packing scheme as `uploadWireframeLines` but 8 bytes per point
   * (one vertex per record instead of two). Use this for the `points`
   * style - the most honest visualization since the records are
   * byte-verified against live RAM and we don't depend on a topology
   * interpretation. */
  uploadWireframePoints(packed) {
    if (!this.wireProgram) this.initWireframePipeline();
    const gl = this.gl;
    const view = new DataView(packed.buffer, packed.byteOffset, packed.byteLength);
    const ptCount = view.getUint32(0, true);
    /* per vertex: 3 floats (x,y,z) + 3 floats (r,g,b) = 24 bytes. */
    const verts = new Float32Array(ptCount * 6);
    let p = 4;
    let v = 0;
    for (let i = 0; i < ptCount; i++) {
      const bodyIdx = view.getUint8(p); p += 4; /* +pad bytes */
      const x = view.getInt16(p, true); p += 2;
      const z = view.getInt16(p, true); p += 2;
      const [r, g, b] = wireframeBodyColor(bodyIdx);
      verts[v++] = x; verts[v++] = 0; verts[v++] = z;
      verts[v++] = r;  verts[v++] = g; verts[v++] = b;
    }
    gl.bindVertexArray(this.wireVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.wireVbo);
    gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(this.wireLocPos);
    gl.vertexAttribPointer(this.wireLocPos, 3, gl.FLOAT, false, 24, 0);
    gl.enableVertexAttribArray(this.wireLocColor);
    gl.vertexAttribPointer(this.wireLocColor, 3, gl.FLOAT, false, 24, 12);
    gl.bindVertexArray(null);
    this.wireVertexCount = ptCount;
    this.wireDrawMode = 'points';
  }

  /* Draw the previously-uploaded wireframe overlay on top of the
   * current frame using the same top-down camera math as
   * renderAssembled. No-ops when nothing is uploaded.
   *
   * Renders with depth-test OFF (the wireframe is a UI overlay, not 3D
   * geometry) and standard alpha blending so the assembled scene remains
   * visible through it. The draw primitive (LINES or POINTS) is
   * selected by whichever upload path was last invoked. */
  renderWireframe(worldExtent, cam) {
    if (!this.wireProgram || !this.wireVertexCount) return;
    const gl = this.gl;
    const w = this.canvas.width;
    const h = this.canvas.height;
    gl.viewport(0, 0, w, h);
    gl.disable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.useProgram(this.wireProgram);
    const vp = buildTopDownVp(w, h, worldExtent, cam);
    gl.uniformMatrix4fv(this.wireLocMvp, false, vp);
    /* Points get a slightly higher alpha than lines since each one
     * covers fewer pixels (gl_PointSize = 3 below). */
    const isPoints = this.wireDrawMode === 'points';
    gl.uniform1f(this.wireLocAlpha, isPoints ? 0.85 : 0.55);
    gl.bindVertexArray(this.wireVao);
    gl.drawArrays(isPoints ? gl.POINTS : gl.LINES, 0, this.wireVertexCount);
    gl.bindVertexArray(null);
    /* Restore depth-test for the next frame's assembled-scene pass. */
    gl.disable(gl.BLEND);
    gl.enable(gl.DEPTH_TEST);
  }

  initWireframePipeline() {
    const gl = this.gl;
    const VS = `#version 300 es
      precision highp float;
      uniform mat4 u_mvp;
      in vec3 a_position;
      in vec3 a_color;
      out vec3 v_color;
      void main() {
        gl_Position = u_mvp * vec4(a_position, 1.0);
        gl_PointSize = 3.0;
        v_color = a_color;
      }
    `;
    const FS = `#version 300 es
      precision highp float;
      uniform float u_alpha;
      in vec3 v_color;
      out vec4 o_color;
      void main() {
        o_color = vec4(v_color, u_alpha);
      }
    `;
    this.wireProgram = compileProgram(gl, VS, FS);
    this.wireLocMvp   = gl.getUniformLocation(this.wireProgram, 'u_mvp');
    this.wireLocAlpha = gl.getUniformLocation(this.wireProgram, 'u_alpha');
    this.wireLocPos   = gl.getAttribLocation(this.wireProgram, 'a_position');
    this.wireLocColor = gl.getAttribLocation(this.wireProgram, 'a_color');
    this.wireVao = gl.createVertexArray();
    this.wireVbo = gl.createBuffer();
    this.wireVertexCount = 0;
  }

  clearWireframe() {
    this.wireVertexCount = 0;
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
    if (this.wireProgram) {
      gl.deleteProgram(this.wireProgram);
      gl.deleteVertexArray(this.wireVao);
      gl.deleteBuffer(this.wireVbo);
      this.wireProgram = null;
    }
  }
}

/* Deterministic per-body color palette for the slot-4 wireframe.
 * Body indices are stable across the three kingdoms (Drake bodies
 * 12/13 = coastline + boundary; Sebucus/Karisto reorder slightly).
 * The two contour-heavy bodies (12 in Drake, 4 in Karisto, 8 in
 * Sebucus) get a soft cyan so they stand out as the coastline. */
function wireframeBodyColor(body) {
  /* Carefully picked saturated hues with similar luminance so no
   * single body visually dominates. */
  const palette = [
    [0.95, 0.55, 0.35],  /* 0  - amber       */
    [0.65, 0.85, 0.45],  /* 1  - lime        */
    [0.55, 0.80, 0.95],  /* 2  - sky         */
    [0.95, 0.55, 0.95],  /* 3  - magenta     */
    [0.95, 0.80, 0.45],  /* 4  - gold        */
    [0.65, 0.45, 0.95],  /* 5  - violet      */
    [0.45, 0.95, 0.65],  /* 6  - mint        */
    [0.95, 0.45, 0.55],  /* 7  - rose        */
    [0.45, 0.65, 0.95],  /* 8  - azure       */
    [0.85, 0.85, 0.45],  /* 9  - olive       */
    [0.45, 0.95, 0.95],  /* 10 - aqua        */
    [0.95, 0.65, 0.45],  /* 11 - apricot     */
    [0.55, 0.95, 0.95],  /* 12 - coastline   */
    [0.95, 0.95, 0.65],  /* 13 - boundary    */
    [0.65, 0.95, 0.55],  /* 14 - chartreuse  */
    [0.95, 0.55, 0.65],  /* 15 - blush       */
  ];
  return palette[body % palette.length];
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
function placementModel(x, z, rotY) {
  const c = Math.cos(rotY), s = Math.sin(rotY);
  const sc = MESH_SCALE;
  /* World matrix = T_world * Ry(rotY) * S_yflip_scaled. Column-major. */
  return new Float32Array([
     sc * c,    0, -sc * s, 0,
     0,       -sc,  0,      0,    /* flip Y so PSX +Y down -> world +Y up */
     sc * s,    0,  sc * c, 0,
     x,         0,  z,      1,
  ]);
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
