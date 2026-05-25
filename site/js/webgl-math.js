/* webgl-math.js — pure-function math + placement helpers for the
 * WASM viewer's TMD pipeline. Split out of webgl-tmd.js for file
 * modularity; consumed by webgl-tmd.js's TmdRenderer class.
 *
 * Loads as a classic global script — exposes: IDENTITY4, MESH_SCALE,
 * compileProgram, mulMat4, perspective, translate, rotateY, rotateX,
 * scaleMat, buildMvp, placementModelScaled, placementModelCentered,
 * computeAabb, buildTopDownVp, ortho, lookAt. Must be loaded before
 * webgl-tmd.js.
 */

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

/* Legacy per-placement model: scale by `sc`, rotate Y, place at (x, 0, z).
 * The mesh's TMD-local origin is the placement pivot; meshes with a
 * non-zero AABB centroid will render offset from the placement coord. */
function placementModelScaled(x, z, rotY, scale) {
  const c = Math.cos(rotY), s = Math.sin(rotY);
  const sc = scale;
  return new Float32Array([
     sc * c,    0,  sc * s, 0,
     0,       -sc,  0,      0,    /* flip Y so PSX +Y down -> world +Y up */
    -sc * s,    0,  sc * c, 0,
     x,         0,  z,      1,
  ]);
}

/* Centroid-anchored per-placement model. Pre-translates the mesh so its
 * AABB centroid sits at the placement coord before rotation + Y-flip
 * scale + world translation. The math expands `T_world * Sflip_scaled *
 * Ry * T(-centroid)`. Used for `kind: 'unplaced'` placements where the
 * TMD-local origin doesn't carry meaningful world-position information. */
function placementModelCentered(x, z, rotY, scale, aabb) {
  const c = Math.cos(rotY), s = Math.sin(rotY);
  const sc = scale;
  const cx = aabb.cx, cy = aabb.cy, cz = aabb.cz;
  return new Float32Array([
     sc * c,                            0,      sc * s,                          0,
     0,                                -sc,     0,                                0,
    -sc * s,                            0,      sc * c,                          0,
     sc * (-c * cx + s * cz) + x,       sc * cy, sc * (-s * cx - c * cz) + z,    1,
  ]);
}

/* Compute AABB + bounding-sphere radius over a Float32Array of vec3s.
 * Empty input collapses to a zero-extent box at origin. */
function computeAabb(positions) {
  if (!positions || positions.length < 3) {
    return { cx: 0, cy: 0, cz: 0, sx: 0, sy: 0, sz: 0, radius: 0 };
  }
  let minX = positions[0], maxX = positions[0];
  let minY = positions[1], maxY = positions[1];
  let minZ = positions[2], maxZ = positions[2];
  for (let i = 3; i < positions.length; i += 3) {
    const x = positions[i], y = positions[i + 1], z = positions[i + 2];
    if (x < minX) minX = x; else if (x > maxX) maxX = x;
    if (y < minY) minY = y; else if (y > maxY) maxY = y;
    if (z < minZ) minZ = z; else if (z > maxZ) maxZ = z;
  }
  const sx = maxX - minX, sy = maxY - minY, sz = maxZ - minZ;
  return {
    cx: (minX + maxX) * 0.5,
    cy: (minY + maxY) * 0.5,
    cz: (minZ + maxZ) * 0.5,
    sx, sy, sz,
    radius: Math.hypot(sx, sy, sz) * 0.5,
  };
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
  /* Rotate the top-down map 90 deg clockwise on screen. With the camera
   * looking straight down, up = -X makes screen-up = world -X and
   * screen-right = world -Z (vs the un-rotated up = -Z, which gave
   * screen-up = -Z / screen-right = +X). The pan controls in
   * attachTopDownControls map drag deltas back through this same basis. */
  const up = [-1, 0, 0];
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

