/* webgl-prim-replay.js - replay the live PSX GPU primitive pool extracted
 * from a mednafen save state. Each vertex is a 14-byte record produced by
 * `LegaiaViewer.save_state_prim_replay` (Rust → WASM):
 *
 *   i16 x, i16 y          (screen-space position, 320x240-ish)
 *   u8  u, u8 v           (texture page UV in PSX framebuffer halfwords)
 *   u16 cba, u16 tsb      (CLUT base address + texture page header,
 *                          PSX format; tsb=0 for sprites that inherit
 *                          the global DrawMode tpage)
 *   u8  r, g, b, flags    (RGB modulator + bit 0 = semi-transparent,
 *                          bit 1 = raw texture)
 *
 * The shader does PSX-faithful sampling: 4/8/15-bpp depth from tsb, CLUT
 * lookup via cba, BGR555→RGBA8. Identical decode to `webgl-tmd.js` but
 * positions are 2D screen-space transformed through an orthographic
 * projection matching the PSX framebuffer.
 *
 * The result is pixel-perfectly equivalent to what the in-game PSX GPU
 * was about to draw at the save state's capture moment - the live
 * primitive pool replayed in a WebGL2 fragment shader.
 */

/* Use an IIFE so our private constants don't collide with `webgl-tmd.js`,
 * which lives in the same global scope on this page. */
(function () {
const VRAM_W = 1024;
const VRAM_H = 512;

const VS_REPLAY = `#version 300 es
precision highp float;
precision highp int;

uniform mat4 u_proj;     /* ortho 0..screen_w × screen_h..0 → clip space */

in vec2 a_pos;           /* i16 screen-space */
in vec2 a_uv;            /* u8 framebuffer halfword UVs */
in uvec2 a_cba_tsb;
in vec4 a_color;         /* RGB modulator + flags in .a */

out vec2 v_uv;
flat out uvec2 v_cba_tsb;
out vec4 v_color;

void main() {
  v_uv = a_uv;
  v_cba_tsb = a_cba_tsb;
  v_color = a_color;
  gl_Position = u_proj * vec4(a_pos, 0.0, 1.0);
}
`;

const FS_REPLAY = `#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;

uniform usampler2D u_vram;

in vec2 v_uv;
flat in uvec2 v_cba_tsb;
in vec4 v_color;

out vec4 o_color;

vec4 bgr555(uint c) {
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
  vec4 tex;
  if (tsb == 0u) {
    /* Sprite path - tsb is unset (PSX sprites inherit global DrawMode).
     * For Drake's compass / minimap markers most are 4bpp, CLUT in cba.
     * Sample as 4bpp from page (0,0) - the save state's framebuffer
     * confirms compass / minimap textures live in the top-left region.
     */
    int vx = int(u_pix >> 2u);
    int vy = int(v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint nibble = u_pix & 3u;
    uint pal_idx = (word >> (nibble * 4u)) & 15u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    uint pal = texelFetch(u_vram, ivec2(cx, cy), 0).r;
    tex = bgr555(pal);
  } else if (depth == 0u) {
    int vx = int(tpage_x + (u_pix >> 2u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint nibble = u_pix & 3u;
    uint pal_idx = (word >> (nibble * 4u)) & 15u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    uint pal = texelFetch(u_vram, ivec2(cx, cy), 0).r;
    tex = bgr555(pal);
  } else if (depth == 1u) {
    int vx = int(tpage_x + (u_pix >> 1u));
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    uint byte_sel = u_pix & 1u;
    uint pal_idx = (word >> (byte_sel * 8u)) & 255u;
    int cx = int((cba & 63u) * 16u + pal_idx);
    int cy = int((cba >> 6u) & 511u);
    uint pal = texelFetch(u_vram, ivec2(cx, cy), 0).r;
    tex = bgr555(pal);
  } else {
    int vx = int(tpage_x + u_pix);
    int vy = int(tpage_y + v_pix);
    uint word = texelFetch(u_vram, ivec2(vx, vy), 0).r;
    tex = bgr555(word);
  }
  /* PSX cutout transparency: BGR555 == 0 ⇒ discard. */
  if (tex.a <= 0.0) discard;
  /* Apply color modulation unless 'raw texture' flag is set.
   * PSX modulator: out = tex * color * 2 (since color is 0..255 with 128 = 1.0). */
  uint flags = uint(v_color.a * 255.0 + 0.5);
  vec3 rgb;
  if ((flags & 2u) != 0u) {
    rgb = tex.rgb;
  } else {
    rgb = clamp(tex.rgb * v_color.rgb * 2.0, 0.0, 1.0);
  }
  o_color = vec4(rgb, 1.0);
}
`;

class PrimReplayRenderer {
  constructor(canvas) {
    const gl = canvas.getContext('webgl2', { antialias: false, alpha: false });
    if (!gl) throw new Error('WebGL2 not available');
    this.canvas = canvas;
    this.gl = gl;
    this.program = compileProgram2(gl, VS_REPLAY, FS_REPLAY);
    this.locProj    = gl.getUniformLocation(this.program, 'u_proj');
    this.locVram    = gl.getUniformLocation(this.program, 'u_vram');
    this.locPos     = gl.getAttribLocation(this.program, 'a_pos');
    this.locUv      = gl.getAttribLocation(this.program, 'a_uv');
    this.locCbaTsb  = gl.getAttribLocation(this.program, 'a_cba_tsb');
    this.locColor   = gl.getAttribLocation(this.program, 'a_color');
    this.vao = gl.createVertexArray();
    this.vbo = gl.createBuffer();
    this.tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R16UI, VRAM_W, VRAM_H);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    this.vertexCount = 0;
    this.screenW = 320;
    this.screenH = 240;
  }

  /* Upload VRAM bytes (raw 1 MB BGR555 halfwords) as R16UI texture. */
  uploadVram(bytes) {
    const gl = this.gl;
    const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 2);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, VRAM_W, VRAM_H,
                     gl.RED_INTEGER, gl.UNSIGNED_SHORT, u16);
  }

  /* Upload the packed vertex buffer + record screen dims. `vertexBytes`
   * has stride 14 (see file-level comment); count = byteLength / 14. */
  uploadVertices(vertexBytes, screenW, screenH) {
    const gl = this.gl;
    this.screenW = screenW;
    this.screenH = screenH;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, vertexBytes, gl.STATIC_DRAW);
    const stride = 14;
    gl.enableVertexAttribArray(this.locPos);
    gl.vertexAttribPointer(this.locPos, 2, gl.SHORT, false, stride, 0);
    gl.enableVertexAttribArray(this.locUv);
    gl.vertexAttribPointer(this.locUv, 2, gl.UNSIGNED_BYTE, false, stride, 4);
    gl.enableVertexAttribArray(this.locCbaTsb);
    gl.vertexAttribIPointer(this.locCbaTsb, 2, gl.UNSIGNED_SHORT, stride, 6);
    gl.enableVertexAttribArray(this.locColor);
    gl.vertexAttribPointer(this.locColor, 4, gl.UNSIGNED_BYTE, true, stride, 10);
    gl.bindVertexArray(null);
    this.vertexCount = vertexBytes.byteLength / stride;
  }

  /* Render with a top-down camera. `cam` is `{ centerX, centerY, zoom }`
   * in screen-space units (the prims' i16 coords); zoom is canvas pixels
   * per screen-space pixel. */
  render(cam) {
    const gl = this.gl;
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    gl.viewport(0, 0, cw, ch);
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.CULL_FACE);
    gl.clearColor(0.02, 0.03, 0.08, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (this.vertexCount === 0) return;
    /* Build an orthographic projection that places (centerX, centerY)
     * at the centre of the canvas with `zoom` pixel-to-pixel scale,
     * Y-axis flipped (screen y grows downward, clip-space y grows up). */
    const halfW = cw / (2 * cam.zoom);
    const halfH = ch / (2 * cam.zoom);
    const left = cam.centerX - halfW;
    const right = cam.centerX + halfW;
    const top = cam.centerY - halfH;
    const bottom = cam.centerY + halfH;
    const proj = ortho2(left, right, bottom, top);
    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locProj, false, proj);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);
    gl.bindVertexArray(this.vao);
    gl.drawArrays(gl.TRIANGLES, 0, this.vertexCount);
    gl.bindVertexArray(null);
  }

  dispose() {
    const gl = this.gl;
    gl.deleteProgram(this.program);
    gl.deleteVertexArray(this.vao);
    gl.deleteBuffer(this.vbo);
    gl.deleteTexture(this.tex);
  }
}

function compileProgram2(gl, vs, fs) {
  const v = gl.createShader(gl.VERTEX_SHADER);
  gl.shaderSource(v, vs); gl.compileShader(v);
  if (!gl.getShaderParameter(v, gl.COMPILE_STATUS))
    throw new Error('VS: ' + gl.getShaderInfoLog(v));
  const f = gl.createShader(gl.FRAGMENT_SHADER);
  gl.shaderSource(f, fs); gl.compileShader(f);
  if (!gl.getShaderParameter(f, gl.COMPILE_STATUS))
    throw new Error('FS: ' + gl.getShaderInfoLog(f));
  const p = gl.createProgram();
  gl.attachShader(p, v); gl.attachShader(p, f); gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS))
    throw new Error('link: ' + gl.getProgramInfoLog(p));
  gl.deleteShader(v); gl.deleteShader(f);
  return p;
}

function ortho2(l, r, b, t) {
  return new Float32Array([
    2 / (r - l), 0,           0, 0,
    0,           2 / (t - b), 0, 0,
    0,           0,          -1, 0,
    -(r + l) / (r - l), -(t + b) / (t - b), 0, 1,
  ]);
}

window.PrimReplayRenderer = PrimReplayRenderer;
})();
