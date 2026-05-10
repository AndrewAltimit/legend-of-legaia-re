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

in vec3 a_position;
in vec2 a_uv_byte;       /* 0..255 each, sent as Uint8x2 normalised=false */
in uvec2 a_cba_tsb;

out vec3 v_world;
out vec2 v_uv;          /* interpolated linearly across the triangle */
flat out uvec2 v_cba_tsb;

void main() {
  v_world = a_position;
  v_uv = a_uv_byte;
  v_cba_tsb = a_cba_tsb;
  gl_Position = u_mvp * vec4(a_position, 1.0);
}
`;

const FS_SRC = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;
uniform vec3 u_light;

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
   * paint solid quads. Matches engine-render's WGSL fragment shader. */
  if (color.a <= 0.0) {
    discard;
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
    this.locVram    = gl.getUniformLocation(this.program, 'u_vram');
    this.locLight   = gl.getUniformLocation(this.program, 'u_light');
    this.locPos     = gl.getAttribLocation(this.program, 'a_position');
    this.locUv      = gl.getAttribLocation(this.program, 'a_uv_byte');
    this.locCbaTsb  = gl.getAttribLocation(this.program, 'a_cba_tsb');

    this.vao    = gl.createVertexArray();
    this.posBuf = gl.createBuffer();
    this.uvBuf  = gl.createBuffer();
    this.ctBuf  = gl.createBuffer();
    this.idxBuf = gl.createBuffer();
    this.tex    = gl.createTexture();

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
    gl.uniform3f(this.locLight, 0.5, -0.7, 0.4);  /* matches WGSL light_dir.xyz */
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);

    gl.bindVertexArray(this.vao);
    gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
    gl.bindVertexArray(null);
  }

  dispose() {
    const gl = this.gl;
    gl.deleteProgram(this.program);
    gl.deleteVertexArray(this.vao);
    gl.deleteBuffer(this.posBuf);
    gl.deleteBuffer(this.uvBuf);
    gl.deleteBuffer(this.ctBuf);
    gl.deleteBuffer(this.idxBuf);
    gl.deleteTexture(this.tex);
  }
}

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

window.TmdRenderer = TmdRenderer;
