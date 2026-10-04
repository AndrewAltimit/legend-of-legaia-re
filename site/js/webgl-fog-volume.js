/* Volumetric ground fog on the play page - the browser twin of the native
 * renderer's `fog_volume` pass (crates/engine-render/src/renderer/
 * fog_volume.rs). An ENHANCEMENT, not retail: the bank is simulated by the
 * engine (crates/engine-core/src/fog_volume.rs, stepped inside World::tick)
 * and handed over each frame through four wasm exports; this file only
 * draws it.
 *
 * The bank is a stack of horizontal alpha-blended sheets over the walk
 * ground, depth-tested against the scene already in the framebuffer and
 * writing no depth, drawn after the 3D scene and before the screen-prim
 * layer and the 2D HUD canvas. The GLSL below is a transcription of the
 * native WGSL - keep the two in step. The recipe's numbers (noise scales,
 * sheet gain, profile exponent) come from the frame header, not from here.
 *
 * Header layout: legaia_engine_core::fog_volume::header. */
(function () {
  'use strict';

  /* Indices into the header (fog_volume::header). */
  const H = {
    SIM_ORIGIN_X: 0, SIM_ORIGIN_Z: 1, SIM_CELL: 2, SIM_DIM: 3,
    MESH_ORIGIN_X: 4, MESH_ORIGIN_Z: 5, MESH_CELL: 6, MESH_DIM: 7,
    COLOR_R: 8, COLOR_G: 9, COLOR_B: 10, OPACITY: 11,
    HEIGHT: 12, LAYERS: 13, DRIFT_X: 14, DRIFT_Z: 15,
    GROUND_GEN: 16, SPACE: 17, SHADER_CONSTANTS: 18, LEN: 22,
  };

  const VS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
uniform mat4 u_m;
uniform vec4 u_params;
out vec2 v_xz;
out float v_layer;
void main() {
  float t = (float(gl_InstanceID) + 0.5) / u_params.y;
  /* Retail Y-down: the sheets stack upward from just above the floor. */
  float y = a_pos.y - 4.0 - t * u_params.x;
  gl_Position = u_m * vec4(a_pos.x, y, a_pos.z, 1.0);
  v_xz = a_pos.xz;
  v_layer = t;
}`;

  const FS = `#version 300 es
precision highp float;
precision highp int;
uniform vec4 u_sim;
uniform vec4 u_mesh;
uniform vec4 u_color;
uniform vec4 u_params;
uniform vec4 u_consts;
uniform sampler2D u_dens;
in vec2 v_xz;
in float v_layer;
out vec4 o_color;

float fog_hash(ivec2 i) {
  uint x = uint(i.x & 0xffff);
  uint y = uint(i.y & 0xffff);
  uint h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
  h = (h ^ (h >> 13u)) * 0x85ebca6bu;
  h = h ^ (h >> 16u);
  return float(h & 0xffffu) / 65535.0;
}

float fog_noise(vec2 p) {
  vec2 fl = floor(p);
  ivec2 i = ivec2(fl);
  vec2 f = p - fl;
  vec2 s = f * f * (vec2(3.0) - 2.0 * f);
  float a = fog_hash(i);
  float b = fog_hash(i + ivec2(1, 0));
  float c = fog_hash(i + ivec2(0, 1));
  float d = fog_hash(i + ivec2(1, 1));
  return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

void main() {
  vec2 drift = u_params.zw;
  float t = v_layer;
  float fine = u_consts.x;
  float bank = u_consts.y;
  float n1 = fog_noise((v_xz - drift) * fine + vec2(t * 7.31, t * 3.17));
  float n2 = fog_noise((v_xz - drift * 0.6) * (fine * 2.3) + vec2(11.7, 5.3));
  float nb = fog_noise((v_xz - drift * 0.35) * bank + vec2(3.1, 8.9));
  float top = 0.55 + 0.65 * nb;
  float profile = pow(clamp(1.0 - t / top, 0.0, 1.0), u_consts.w);
  float shape = clamp(0.35 + 0.9 * nb, 0.0, 1.0) * (0.35 + 0.65 * (0.65 * n1 + 0.35 * n2));
  vec2 uv = (v_xz - u_sim.xy) / (u_sim.z * u_sim.w);
  float d = 1.0;
  if (all(greaterThanEqual(uv, vec2(0.0))) && all(lessThanEqual(uv, vec2(1.0)))) {
    d = textureLod(u_dens, uv, 0.0).r;
  }
  float half_extent = 0.5 * u_mesh.z * u_mesh.w;
  vec2 centre = u_mesh.xy + vec2(half_extent);
  float r = length(v_xz - centre) / half_extent;
  float edge = 1.0 - smoothstep(0.55, 0.95, r);
  float a = u_color.a * d * profile * shape * edge * u_consts.z / u_params.y;
  vec3 rgb = u_color.rgb * (0.9 + 0.2 * n1);
  o_color = vec4(rgb, clamp(a, 0.0, 1.0));
}`;

  function compile(gl, type, src) {
    const sh = gl.createShader(type);
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(sh);
      gl.deleteShader(sh);
      throw new Error('fog volume shader: ' + log);
    }
    return sh;
  }

  /* Column-major 4x4 multiply: out = a * b. */
  function mul4(a, b) {
    const o = new Float32Array(16);
    for (let c = 0; c < 4; c++) {
      for (let r = 0; r < 4; r++) {
        let s = 0;
        for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
        o[c * 4 + r] = s;
      }
    }
    return o;
  }

  class FogVolumePass {
    constructor(gl) {
      if (typeof WebGL2RenderingContext === 'undefined' || !(gl instanceof WebGL2RenderingContext)) {
        throw new Error('fog volume needs WebGL2');
      }
      this.gl = gl;
      const prog = gl.createProgram();
      const vs = compile(gl, gl.VERTEX_SHADER, VS);
      const fs = compile(gl, gl.FRAGMENT_SHADER, FS);
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.linkProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        throw new Error('fog volume link: ' + gl.getProgramInfoLog(prog));
      }
      this.prog = prog;
      this.loc = {
        m: gl.getUniformLocation(prog, 'u_m'),
        sim: gl.getUniformLocation(prog, 'u_sim'),
        mesh: gl.getUniformLocation(prog, 'u_mesh'),
        color: gl.getUniformLocation(prog, 'u_color'),
        params: gl.getUniformLocation(prog, 'u_params'),
        consts: gl.getUniformLocation(prog, 'u_consts'),
        dens: gl.getUniformLocation(prog, 'u_dens'),
      };
      this.vao = gl.createVertexArray();
      this.vbuf = gl.createBuffer();
      this.ibuf = gl.createBuffer();
      this.indexCount = 0;
      this.groundGen = null;
      this.tex = gl.createTexture();
      this.texDim = 0;
    }

    /* Draw one frame's bank. `rt` is the LegaiaRuntime, `vp` the matrix the
     * page drew the scene with (its Y-up draw frame), `battleScale` the
     * battle stage's world scale (a battle bank lives in raw stage units).
     * Returns false when the engine has no bank this frame. */
    draw(rt, vp, battleScale) {
      const h = rt.play_fog_volume_header();
      if (!h || h.length < H.LEN || !vp) return false;
      const gl = this.gl;
      if (this.indexCount === 0) {
        const idx = rt.play_fog_volume_indices();
        gl.bindVertexArray(this.vao);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibuf);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, idx, gl.STATIC_DRAW);
        gl.bindVertexArray(null);
        this.indexCount = idx.length;
      }
      if (this.groundGen !== h[H.GROUND_GEN]) {
        const pos = rt.play_fog_volume_mesh();
        gl.bindVertexArray(this.vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vbuf);
        gl.bufferData(gl.ARRAY_BUFFER, pos, gl.DYNAMIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 12, 0);
        gl.bindVertexArray(null);
        this.groundGen = h[H.GROUND_GEN];
      }
      const dim = h[H.SIM_DIM] | 0;
      const dens = rt.play_fog_volume_density();
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, this.tex);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
      if (this.texDim !== dim) {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, dim, dim, 0, gl.RED, gl.UNSIGNED_BYTE, dens);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        this.texDim = dim;
      } else {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, dim, dim, gl.RED, gl.UNSIGNED_BYTE, dens);
      }

      /* The bank's space -> the page's Y-up draw frame: the field world is
       * retail Y-down (one Y negation, the per-model flip every field
       * placement carries); the battle stage is raw units under the stage's
       * world scale with the same flip. */
      const s = h[H.SPACE] >= 0.5 ? (battleScale || 4.0) : 1.0;
      const model = new Float32Array([
        s, 0, 0, 0,
        0, -s, 0, 0,
        0, 0, s, 0,
        0, 0, 0, 1,
      ]);
      const m = mul4(vp, model);

      gl.useProgram(this.prog);
      gl.uniformMatrix4fv(this.loc.m, false, m);
      gl.uniform4f(this.loc.sim, h[H.SIM_ORIGIN_X], h[H.SIM_ORIGIN_Z], h[H.SIM_CELL], h[H.SIM_DIM]);
      gl.uniform4f(this.loc.mesh, h[H.MESH_ORIGIN_X], h[H.MESH_ORIGIN_Z], h[H.MESH_CELL], h[H.MESH_DIM]);
      gl.uniform4f(this.loc.color, h[H.COLOR_R], h[H.COLOR_G], h[H.COLOR_B], h[H.OPACITY]);
      gl.uniform4f(this.loc.params, h[H.HEIGHT], h[H.LAYERS], h[H.DRIFT_X], h[H.DRIFT_Z]);
      const c = H.SHADER_CONSTANTS;
      gl.uniform4f(this.loc.consts, h[c], h[c + 1], h[c + 2], h[c + 3]);
      gl.uniform1i(this.loc.dens, 0);

      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.depthMask(false);
      gl.disable(gl.CULL_FACE);
      gl.enable(gl.BLEND);
      gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ZERO, gl.ONE);
      gl.blendEquation(gl.FUNC_ADD);
      gl.bindVertexArray(this.vao);
      gl.drawElementsInstanced(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0, h[H.LAYERS] | 0);
      gl.bindVertexArray(null);
      gl.depthMask(true);
      gl.disable(gl.BLEND);
      return true;
    }

    dispose() {
      const gl = this.gl;
      gl.deleteProgram(this.prog);
      gl.deleteBuffer(this.vbuf);
      gl.deleteBuffer(this.ibuf);
      gl.deleteVertexArray(this.vao);
      gl.deleteTexture(this.tex);
    }
  }

  window.LegaiaFogVolumePass = FogVolumePass;
})();
