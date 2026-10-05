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
 *   r.uploadSceneMesh(meshId, positions, uvs, cbaTsb, indices, flatRgba?);
 *   r.getMeshAabb(meshId);   // null until uploadSceneMesh has run
 *   r.uploadFogLut(u16Array, params);                          // optional
 *   r.renderAssembled(placements, worldExtent, cam);
 *   r.dispose();
 *
 * Each `placement` for `renderAssembled` is:
 *   { meshId, x, z, y?, rotY?, scale?, anchor?, model?, cue? }
 *     scale  - per-placement world-scale (defaults to MESH_SCALE).
 *     y      - optional world height for the anchor (walk-frame landmarks
 *              pass the floor-LUT height so they sit on the heightfield;
 *              omitted -> anchor on the y=0 plane). Ignored when anchor is
 *              'centroid'.
 *     anchor - 'origin' (default) uses the mesh's TMD-local origin as the
 *              placement pivot; 'centroid' first translates the mesh so
 *              its AABB centroid sits at (x, 0, z).
 *     model  - explicit column-major mat4 (Float32Array(16)) that REPLACES
 *              the whole x/y/z/rotY/scale/anchor construction. The battle FX
 *              layer passes engine-composed matrices through this so the
 *              browser cannot drift from the native `fx_cam * model`.
 *     decoCue - overworld decoration: stage retail's per-object cue
 *              (overworldDecorationCue) from the draw origin's depth.
 *     cue    - per-draw GTE depth cue { far: [r,g,b], nearZ, farZ, maxIr0 },
 *              overriding the frame-global `setDepthCue` for this draw only.
 *              The engine's `DrawCue` seam: the battle ground grid's per-stage
 *              far colour and the target-select cursor tint both ride it.
 *
 * Fog parameters (uploadFogLut(lut, { enable, zShift, color, farRef })):
 *   lut     - 512-entry Uint16Array, BGR555 entries indexed by Z >> 5.
 *   enable  - non-zero enables the fog post-process (matches retail's
 *             gp-0x2D1 & 0x10 gate).
 *   zShift  - exponent used to compute Z_far = Z >> zShift (retail gp+0x90).
 *   color   - { r, g, b } in 0..1 floats; mid-distance tint baseline.
 *   farRef  - 0..16383 reference Z for the far plane (retail gp-0x2E0).
 */

/* The overworld decoration cells' depth cue - the page twin of
 * legaia_engine_core::overworld_ground_cue::decoration_draw_cue. Retail's
 * decoration sweep (FUN_801F69D8, PROT 0901) takes one IR0 per object from
 * the camera depth of its origin, IR0 = min(max(TRZ - 0x5000, 0) >> 3,
 * 0x1000), and hazes every prim of it toward the far colour 0xD0 (the
 * dispatcher FUN_80043390 stages a1 = 0x00D0D0D0 as the far colour). TRZ
 * is the origin's clip w times the frame's clip.w-to-SZ factor (the
 * u_curve the ground cue reads); 0 - every page but the play page on an
 * overworld - is no cue. Returns a per-draw cue record (a flat blend: the
 * ramp saturates at any positive depth) or null. vp and model are
 * column-major. */
function overworldDecorationCue(vp, model, curve) {
  if (!(curve > 0)) return null;
  const w = vp[3] * model[12] + vp[7] * model[13] + vp[11] * model[14] + vp[15] * model[15];
  const trz = Math.round(w * curve);
  const ir0 = Math.min(Math.max(trz - 0x5000, 0) >> 3, 0x1000);
  if (ir0 <= 0) return null;
  const far = 0xD0 / 255;
  return { far: [far, far, far], nearZ: -1, farZ: 0, maxIr0: ir0 / 4096 };
}

/* PSX semi-transparency (ABE) tail for a scene mesh's index list: bucket
 * every semi-transparent triangle (first vertex's TSB bit 15, packed by the
 * Rust mesh builders) into one of four per-ABR-mode runs appended after the
 * original indices. The opaque pass draws the original range (the fragment
 * shader defers the blending texels via u_semi_pass = 0); the blend pass
 * re-draws each tail run with the matching GL blend state. The browser
 * mirror of engine-render's psx_blend::append_semi_tail.
 *
 * Returns null when the mesh has no semi prims, so pure-opaque meshes
 * upload their index list untouched. */
function buildSemiTail(indices, cbaTsb) {
  const buckets = [[], [], [], []];
  for (let i = 0; i + 2 < indices.length; i += 3) {
    const tsb = cbaTsb[indices[i] * 2 + 1];
    if ((tsb & 0x8000) === 0) continue;
    buckets[(tsb >> 5) & 3].push(indices[i], indices[i + 1], indices[i + 2]);
  }
  let tailLen = 0;
  for (const b of buckets) tailLen += b.length;
  if (tailLen === 0) return null;
  const out = new Uint32Array(indices.length + tailLen);
  out.set(indices, 0);
  const ranges = [];
  let at = indices.length;
  for (let mode = 0; mode < 4; mode++) {
    const b = buckets[mode];
    if (b.length === 0) continue;
    ranges.push({ mode, start: at, count: b.length });
    out.set(b, at);
    at += b.length;
  }
  return { indices: out, ranges };
}

/* Enhanced lighting's shadow maps - the native renderer's constants
 * (engine-render helpers.rs SHADOW_MAP_DIM / SHADOW_COMPARE_BIAS and
 * scene_lights.rs SHADOW_FOV / SHADOW_NEAR_FRAC / SHADOW_NEAR_MIN). */
const SHADOW_MAP_DIM = 512;
const SHADOW_COMPARE_BIAS = 0.0015;
const SHADOW_FOV = 2.1;
const SHADOW_NEAR_FRAC = 0.04;
const SHADOW_NEAR_MIN = 24.0;
/* The texture unit the shadow array lives on for the program's life. */
const SHADOW_TEX_UNIT = 4;
/* Words of mood at the head of the engine's lighting packet
 * (`play_lighting_frame`): the three `LightingMood::uniforms` words, then
 * `window_word` (the window glow). The light and glow counts follow. */
const MOOD_WORDS = 16;

class TmdRenderer {
  constructor(canvas) {
    const gl = canvas.getContext('webgl2', { antialias: true, alpha: false });
    /* Still a throw, so every existing caller behaves exactly as before - but
     * routed through the shared notice, which puts the real cause on screen
     * first. Of the twelve construction sites only viewer.html catches this,
     * and its flat-shaded fallback is what made a missing WebGL2 context look
     * like an untextured-model bug in the renderer. See main.js. */
    if (!gl) {
      throw (window.legaiaWebgl2Failure
        ? window.legaiaWebgl2Failure()
        : new Error('WebGL2 not available'));
    }
    this.canvas = canvas;
    this.gl = gl;

    this.program = compileProgram(gl, VS_SRC, FS_SRC);
    this.locMvp     = gl.getUniformLocation(this.program, 'u_mvp');
    this.locModel   = gl.getUniformLocation(this.program, 'u_model');
    this.locVram    = gl.getUniformLocation(this.program, 'u_vram');
    this.locNoDisc  = gl.getUniformLocation(this.program, 'u_no_discard');
    this.locSemiPass = gl.getUniformLocation(this.program, 'u_semi_pass');
    this.locFogLut  = gl.getUniformLocation(this.program, 'u_fog_lut');
    this.locFogEnableFs = gl.getUniformLocation(this.program, 'u_fog_enable');
    this.locFogColor    = gl.getUniformLocation(this.program, 'u_fog_color');
    this.locFogOrigin   = gl.getUniformLocation(this.program, 'u_fog_origin');
    this.locFogFarRef   = gl.getUniformLocation(this.program, 'u_fog_far_ref');
    this.locFogZShift   = gl.getUniformLocation(this.program, 'u_fog_z_shift');
    this.locUseFlatColors = gl.getUniformLocation(this.program, 'u_use_flat_colors');
    this.locGhost   = gl.getUniformLocation(this.program, 'u_ghost');
    this.locGrade   = gl.getUniformLocation(this.program, 'u_grade');
    this.locCue     = gl.getUniformLocation(this.program, 'u_cue');
    this.locCueFar  = gl.getUniformLocation(this.program, 'u_cue_far');
    this.locPalette = gl.getUniformLocation(this.program, 'u_palette');
    this.locPairFront = gl.getUniformLocation(this.program, 'u_pair_front');
    this.locNclipCull = gl.getUniformLocation(this.program, 'u_nclip_cull');
    this.locCurve = gl.getUniformLocation(this.program, 'u_curve');
    this.locPrimNear = gl.getUniformLocation(this.program, 'u_prim_near');
    this.locOcclFocus  = gl.getUniformLocation(this.program, 'u_occl_focus');
    this.locOcclParams = gl.getUniformLocation(this.program, 'u_occl_params');
    this.locOcclAllow  = gl.getUniformLocation(this.program, 'u_occl_allow');
    this.locEclipM     = gl.getUniformLocation(this.program, 'u_eclip_m');
    this.locEclipB     = gl.getUniformLocation(this.program, 'u_eclip_b');
    this.locOcclLift   = gl.getUniformLocation(this.program, 'u_occl_lift');
    this.locPsx      = gl.getUniformLocation(this.program, 'u_psx');
    this.locDynDir   = gl.getUniformLocation(this.program, 'u_dyn_dir');
    this.locLogDepthOn = gl.getUniformLocation(this.program, 'u_log_depth_on');
    /* Opt-in log-depth write (setLogDepth); off on every page but play. */
    this.logDepth = false;
    this.lastLogDepth = false;
    this.locDynColor = gl.getUniformLocation(this.program, 'u_dyn_color');
    this.locDynAmbient = gl.getUniformLocation(this.program, 'u_dyn_ambient');
    this.locDynWindow = gl.getUniformLocation(this.program, 'u_dyn_window');
    this.locLightCount = gl.getUniformLocation(this.program, 'u_light_count');
    this.locLightPr  = gl.getUniformLocation(this.program, 'u_light_pr');
    this.locLightCol = gl.getUniformLocation(this.program, 'u_light_col');
    /* Enhanced lighting's per-frame state, staged by the play page through
     * `lightingProvider(right, up)` (which asks the engine's
     * `play_lighting_frame`): the mood's four shader words, the picked
     * point lights in this page's frame, and the glow-sprite quads. null on
     * every other page - and the enable word stays 0, the identity. */
    this.lightingProvider = null;
    this.lightFrame = null;
    this.glowProgram = compileProgram(gl, GLOW_VS_SRC, GLOW_FS_SRC);
    this.locGlowMvp = gl.getUniformLocation(this.glowProgram, 'u_mvp');
    this.locGlowLogDepth = gl.getUniformLocation(this.glowProgram, 'u_log_depth_on');
    this.glowVao = gl.createVertexArray();
    this.glowBuf = gl.createBuffer();
    gl.bindVertexArray(this.glowVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.glowBuf);
    {
      const stride = 9 * 4;
      const lp = gl.getAttribLocation(this.glowProgram, 'a_position');
      const lu = gl.getAttribLocation(this.glowProgram, 'a_uv');
      const lc = gl.getAttribLocation(this.glowProgram, 'a_color');
      gl.enableVertexAttribArray(lp);
      gl.vertexAttribPointer(lp, 3, gl.FLOAT, false, stride, 0);
      gl.enableVertexAttribArray(lu);
      gl.vertexAttribPointer(lu, 2, gl.FLOAT, false, stride, 12);
      gl.enableVertexAttribArray(lc);
      gl.vertexAttribPointer(lc, 4, gl.FLOAT, false, stride, 20);
    }
    gl.bindVertexArray(null);
    /* Enhanced lighting's point-light shadow maps (the native renderer's
     * scene-light shadow array + depth-only pass). The array texture is
     * created here, at 1x1 until a frame needs real layers, and stays bound
     * on its own unit for the program's life: a sampler2DArrayShadow left on
     * unit 0 would share it with the VRAM TEXTURE_2D, which WebGL rejects at
     * every draw. */
    this.locShadowMaps = gl.getUniformLocation(this.program, 'u_shadow_maps');
    this.locLightVp = gl.getUniformLocation(this.program, 'u_light_vp');
    this.locShadow = gl.getUniformLocation(this.program, 'u_shadow');
    this.dynShadows = true;
    this.shadowDim = 0;
    this.shadowTex = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0 + SHADOW_TEX_UNIT);
    gl.bindTexture(gl.TEXTURE_2D_ARRAY, this.shadowTex);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_COMPARE_MODE, gl.COMPARE_REF_TO_TEXTURE);
    gl.texParameteri(gl.TEXTURE_2D_ARRAY, gl.TEXTURE_COMPARE_FUNC, gl.LEQUAL);
    gl.texImage3D(gl.TEXTURE_2D_ARRAY, 0, gl.DEPTH_COMPONENT24, 1, 1, 1, 0,
      gl.DEPTH_COMPONENT, gl.UNSIGNED_INT, null);
    gl.activeTexture(gl.TEXTURE0);
    gl.useProgram(this.program);
    gl.uniform1i(this.locShadowMaps, SHADOW_TEX_UNIT);
    gl.useProgram(null);
    this.shadowFbo = gl.createFramebuffer();
    {
      /* Bound at the main program's a_position slot so every mesh VAO
       * (built against the main program) feeds the shadow pass as is. */
      const posLoc = gl.getAttribLocation(this.program, 'a_position');
      const vs = gl.createShader(gl.VERTEX_SHADER);
      gl.shaderSource(vs, SHADOW_VS_SRC);
      gl.compileShader(vs);
      const fs = gl.createShader(gl.FRAGMENT_SHADER);
      gl.shaderSource(fs, SHADOW_FS_SRC);
      gl.compileShader(fs);
      const prog = gl.createProgram();
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.bindAttribLocation(prog, posLoc >= 0 ? posLoc : 0, 'a_position');
      gl.linkProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      this.shadowProgram = gl.getProgramParameter(prog, gl.LINK_STATUS) ? prog : null;
      this.locShadowMvp = this.shadowProgram
        ? gl.getUniformLocation(this.shadowProgram, 'u_mvp') : null;
    }
    this.lightVps = new Float32Array(16 * 8);
    this.shadowCount = 0;
    /* The two native-only render toggles, both OFF by default and both the
     * identity when off (see setPsxMode / setDynamicLighting). */
    this.psxMode = false;
    this.dynLighting = false;
    /* What the 3D pass clears to, as linear RGBA. The default is the dark
     * ground every viewer page draws on; the play page overwrites it per
     * frame from the engine (`play_scene_clear_color`), because in a battle
     * the stage dome is a FRONT HALF and the open band above it is read as
     * sky. Hard-coding one clear here made every browser battle draw that
     * band black while the native window showed sky. */
    this.clearColor = [0.04, 0.05, 0.08, 1.0];
    /* Prologue colour grade + depth-cue ramp, staged per frame by the play
     * page (identity / off by default - no other page is affected). */
    this.gradeParams = { rgb: null, strength: 0 };
    this.cueParams = { far: null, nearZ: 0, farZ: 0, maxIr0: 0 };
    /* Prologue palette-collapse grade (the native renderer's
     * `set_palette_grade`): `mul` = the op-`4C 12` screen tint, `on` = the
     * mode flag. Off by default, so every other page draws unchanged. */
    this.paletteParams = { mul: null, on: false };
    /* Retail NCLIP winding rejection mode (the native renderer's
     * `set_backface_cull`), staged per frame by the play page. 0 = both
     * sides, the default every other page keeps. */
    this.nclipCull = 0;
    this.overworldCurve = 0;
    /* Retail's per-primitive near reject for the assembled scene pass:
     * [enable, sz_per_w, ot_shift, near_otz] (setPrimNear), and the hook
     * that builds a mesh's per-vertex primitive-corner stream
     * (positions, indices) -> Float32Array of 13 floats per vertex - the
     * play page points it at the engine's prim_corner_refs. Off / null on
     * every other page. */
    this.primNear = [0, 0, 0, 0];
    this.primRefsFn = null;
    /* Camera-occlusion fade focus: the player's WORLD-space body centre
     * (draw frame, i.e. the Y-flipped coords every placement uses), staged
     * per frame by the play page via setOcclusionFocus / cleared with
     * clearOcclusionFocus. null (the default) disables the fade, so no
     * other consumer of this renderer is affected. renderAssembled
     * projects it through the same view-projection it builds for the
     * scene draws - the page never duplicates camera math. */
    this.occlFocus = null;
    this.locPos     = gl.getAttribLocation(this.program, 'a_position');
    this.locUv      = gl.getAttribLocation(this.program, 'a_uv_byte');
    this.locCbaTsb  = gl.getAttribLocation(this.program, 'a_cba_tsb');
    this.locFlatRgba = gl.getAttribLocation(this.program, 'a_flat_rgba');
    /* The continent ground's flat bucket-depth reference (see
     * overworldFlatDepth in webgl-shaders.js); -1 when the driver dropped
     * the unused attributes. */
    this.locGroundRefXz = gl.getAttribLocation(this.program, 'a_ground_ref_xz');
    this.locGroundRefY  = gl.getAttribLocation(this.program, 'a_ground_ref_y');
    /* Smoothed normals for the dynamic light; -1 if the driver dropped it. */
    this.locNormal = gl.getAttribLocation(this.program, 'a_normal');
    /* Retail's per-primitive near reject: each vertex's primitive corners
     * (see primNearRejected in webgl-shaders.js); -1 where dropped. */
    this.locPrimC = ['a_prim_c0', 'a_prim_c1', 'a_prim_c2', 'a_prim_c3']
      .map(n => gl.getAttribLocation(this.program, n));

    /* Field-character hybrid mode: when set, draws bind the per-vertex
     * a_flat_rgba colours and the FS uses them for untextured prims. Off for
     * every other consumer (scene, world map, monsters, battle characters). */
    this.useFlatColors = false;

    /* Opt-in backface culling for the single-mesh `render()` path. Off by
     * default (Legaia TMDs have inconsistent winding across the corpus, so
     * the viewer relies on the depth buffer); the dance stage turns it on -
     * retail's NCLIP pass culls the hall's inward-facing panels (the crowd
     * billboard right behind its camera) and the interior only reads
     * correctly with the same rule. Winding choice via `cullFrontFace`
     * ('cw' | 'ccw'). */
    this.cullBackfaces = false;
    this.cullFrontFace = 'ccw';

    /* Opt-in two-pass semi-transparency for the single-mesh `render()`
     * path: pass 0 draws the opaque prims, pass 1 re-draws only the ABE
     * prims (TSB bit 15) additively with the depth buffer read-only - the
     * dance hall's smoke columns and spotlight glows are ABE prims that
     * read as opaque grey slabs without it. Off by default (every existing
     * consumer keeps the one-pass draw-everything behaviour). */
    this.semiTwoPass = false;

    /* After-image trail for the single-mesh `render()` path (the arts page's
     * per-character tinted echoes). `{ passes: [{ positions, tint: [r,g,b],
     * alpha }], restore: Float32Array }` or null. Each pass re-draws the mesh
     * additively at a delayed pose; `restore` puts the live pose back in the
     * position buffer afterwards (MeshView only re-uploads on frame change). */
    this.ghostTrail = null;

    this.vao    = gl.createVertexArray();
    this.posBuf = gl.createBuffer();
    this.uvBuf  = gl.createBuffer();
    this.ctBuf  = gl.createBuffer();
    this.flatBuf = gl.createBuffer();
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
    this.locOceanUvOffset   = gl.getUniformLocation(this.oceanProgram, 'u_uv_offset');
    this.locOceanColor      = gl.getUniformLocation(this.oceanProgram, 'u_color');
    this.locOceanTex        = gl.getUniformLocation(this.oceanProgram, 'u_ocean_tex');
    this.locOceanClut       = gl.getUniformLocation(this.oceanProgram, 'u_ocean_clut');
    this.locOceanTextured   = gl.getUniformLocation(this.oceanProgram, 'u_ocean_textured');
    this.locOceanSampleSize = gl.getUniformLocation(this.oceanProgram, 'u_ocean_sample_size');
    this.locOceanShade      = gl.getUniformLocation(this.oceanProgram, 'u_shade');
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
      /* How many wall-clock seconds between animation steps. Matches
       * the live engine's tuned cadence (OCEAN_ANIM_TICKS_PER_FRAME = 6
       * sim ticks at 60 Hz = 0.1 s/frame, full 13-frame cycle ~1.3 s). */
      frameDurationSec: 6 / 60,
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
  setNclipCull(mode) {
    this.nclipCull = (mode | 0);
  }

  /* The kingdom overworld's per-vertex screen-Y curvature for the assembled
   * scene pass: `scale` is the frame's clip.w-to-SZ factor from the shared
   * `overworld_curvature::frame_curve_scale` kernel (the native renderer's
   * `set_overworld_curvature` word). 0 - every other page - is flat. */
  setOverworldCurve(scale) {
    this.overworldCurve = scale > 0 ? +scale : 0;
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

  /* Return the AABB this renderer computed for an uploaded scene mesh.
   * Returns null if the meshId has never been uploaded. */
  getMeshAabb(meshId) {
    const m = this.sceneMeshes.get(meshId);
    return m ? m.aabb : null;
  }

  /* Vertex count of an uploaded scene mesh (0 until uploadSceneMesh has
   * run). The sky-mesh classifier's density guard reads it: a genuine sky
   * shell is a few dozen verts stretched over kilometres, while big REAL
   * geometry (kor5's 459-vert town paving) is dense. */
  getMeshVertexCount(meshId) {
    const m = this.sceneMeshes.get(meshId);
    return m ? (m.vertexCount || 0) : 0;
  }

  /* `flatRgba` (optional): Uint8Array, 4 bytes per vertex
   * [r, g, b, textured_flag] - the prim's PSX packet colour plus which job it
   * does. Textured verts (flag 255) MODULATE their texel by it
   * (`texel * colour / 128`, retail's whole lighting model); untextured verts
   * (flag 0) are FILLED with it. Omit it only for geometry that has no packet
   * colour at all (a generated heightfield): the attribute then falls back to
   * the neutral 0x80 constant and the draw is `texel * 1.0`. */
  uploadMesh(positions, uvs, cbaTsb, indices, flatRgba) {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);

    gl.bindBuffer(gl.ARRAY_BUFFER, this.posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.DYNAMIC_DRAW);
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

    /* Optional per-vertex flat colours (normalised u8 → 0..1). */
    this.useFlatColors = !!(flatRgba && flatRgba.length && this.locFlatRgba >= 0);
    if (this.useFlatColors) {
      gl.bindBuffer(gl.ARRAY_BUFFER, this.flatBuf);
      gl.bufferData(gl.ARRAY_BUFFER, flatRgba, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(this.locFlatRgba);
      gl.vertexAttribPointer(this.locFlatRgba, 4, gl.UNSIGNED_BYTE, true, 0, 0);
    } else if (this.locFlatRgba >= 0) {
      gl.disableVertexAttribArray(this.locFlatRgba);
      this._setNeutralPacketColor();
    }

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

    this.indexCount = indices.length;
    this.posByteLength = positions.byteLength;
    gl.bindVertexArray(null);
    /* The single-mesh path's normal record (see _ensureNormals). */
    if (!this.single) this.single = { vao: this.vao, normBuf: null };
    this.single.cpuPositions = positions;
    this.single.cpuIndices = indices;
    this.single.normalsDirty = true;
  }

  /* Replace just the position buffer in-place (DYNAMIC_DRAW). For animation:
   * UVs / CBA-TSB / indices stay the same per-frame, only positions change.
   * `positions` must match the byte length of the last `uploadMesh` call. */
  updatePositions(positions) {
    const gl = this.gl;
    if (positions.byteLength !== this.posByteLength) return;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.posBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, positions);
    if (this.single) {
      this.single.cpuPositions = positions;
      this.single.normalsDirty = true;
    }
  }

  /* Replace just the index buffer of the last `uploadMesh` (the vertex
   * streams stay). For a scene that draws a per-frame subset of its
   * triangles - the dance hall, cut to what the PSX GPU draws from the
   * frame's eye. */
  updateIndices(indices) {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.idxBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.DYNAMIC_DRAW);
    gl.bindVertexArray(null);
    this.indexCount = indices.length;
    if (this.single) {
      this.single.cpuIndices = indices;
      this.single.normalsDirty = true;
    }
  }

  /* center: [cx, cy, cz]; radius: bounding-sphere half-extent;
   * distance: camera distance in unit-radius units (default 2.5);
   * panX/panY: view-space pan in unit-radius units (default 0);
   * fovY (optional, radians): vertical field of view (default 1.2). */
  render(yaw, pitch, distance, panX, panY, center, radius, fovY) {
    const gl = this.gl;
    const w = this.canvas.width;
    const h = this.canvas.height;
    gl.viewport(0, 0, w, h);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    /* Legaia TMDs have inconsistent winding; by default let the depth buffer
     * sort it out. Consumers that need retail's NCLIP cull opt in. */
    if (this.cullBackfaces) {
      gl.enable(gl.CULL_FACE);
      gl.cullFace(gl.BACK);
      gl.frontFace(this.cullFrontFace === 'cw' ? gl.CW : gl.CCW);
    } else {
      gl.disable(gl.CULL_FACE);
    }
    gl.clearColor(this.clearColor[0], this.clearColor[1], this.clearColor[2], this.clearColor[3]);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    if (this.indexCount === 0) return;

    /* `mvpOverride` (a column-major Float32Array(16)), when a caller sets
     * one, replaces the orbit framing for this draw: a page whose engine
     * hands it a ready view-projection (the Baka duel's arena camera) draws
     * through it instead of re-framing the mesh. */
    const mvp = this.mvpOverride
      || buildMvp(yaw, pitch, distance, panX, panY, center, radius, w, h, fovY);

    gl.useProgram(this.program);
    gl.uniformMatrix4fv(this.locMvp, false, mvp);
    gl.uniformMatrix4fv(this.locModel, false, IDENTITY4);
    if (this.locLogDepthOn) gl.uniform1i(this.locLogDepthOn, 0);
    /* The single-mesh viewer never bends (the curve is an overworld
     * scene-pass term, staged by renderAssembled). */
    if (this.locCurve) gl.uniform1f(this.locCurve, 0);
    /* Nor near-reject: another scene-pass word on the shared program. */
    if (this.locPrimNear) gl.uniform4f(this.locPrimNear, 0, 0, 0, 0);
    /* Nor does it NCLIP-reject: `u_nclip_cull` is a scene-pass word too
     * (renderAssembled stages `nclipCull`), and the uniform persists on the
     * shared program. Left at the field's `2`, the play page's in-world
     * minigames (the Muscle Dome's arena shell, drawn through this path
     * after a field frame) lost every front-facing wall fragment - the
     * dome read as a bare dirt floor. */
    if (this.locNclipCull) gl.uniform1i(this.locNclipCull, 0);
    /* The prologue grade / palette collapse / depth cue are scene-pass
     * state of the same kind (play-app stages them per field frame and only
     * renderAssembled pushes them): stage the identity so a field's grade
     * or cue cannot tint a minigame drawn here. Every other page leaves the
     * params at their identity defaults, so this is a no-op there. */
    if (this.locGrade) gl.uniform4f(this.locGrade, 1, 1, 1, 0);
    if (this.locPalette) gl.uniform4f(this.locPalette, 1, 1, 1, 0);
    this._setCue(null);
    /* buildMvp = single reflection (Y flip): a double-sided pair's visible
     * copy is the front-facing one under this projection. */
    gl.uniform1i(this.locPairFront, 1);
    gl.uniform1i(this.locNoDisc, 0);  /* per-mesh inspector: keep cutout discard */
    /* Single-mesh inspector: legacy single pass (no semi-transparency defer,
     * ABE prims draw opaque) - only renderAssembled runs the blend pass. */
    gl.uniform1i(this.locSemiPass, -1);
    /* Untextured-fill branch: untextured prims are filled from their vertex
     * colour. Set explicitly every frame (the uniform persists on the shared
     * program). The textured path's packet modulation is unconditional. */
    gl.uniform1i(this.locUseFlatColors, this.useFlatColors ? 1 : 0);
    /* Ghost tint OFF for the opaque pose (the trail pass sets it per echo). */
    gl.uniform4f(this.locGhost, 0, 0, 0, 0);
    /* Occlusion fade OFF on the single-mesh inspector path (uniforms
     * persist on the shared program; a play-page focus must not leak). */
    gl.uniform4f(this.locOcclFocus, 0, 0, 0, 0);
    /* PSX rasterisation + dynamic light (identity unless a page opted in). */
    this._applyRenderToggles(w, h);
    this._ensureNormals(this.single);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.locVram, 0);

    gl.bindVertexArray(this.vao);
    if (this.semiTwoPass) {
      /* Pass 0: opaque prims only. */
      gl.uniform1i(this.locSemiPass, 0);
      gl.disable(gl.BLEND);
      gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
      /* Pass 1: ABE prims, additive, depth-tested but not depth-written -
       * the closest single-mode stand-in for the PSX blend modes (the hall's
       * ABE prims are glow/smoke, which retail draws additively). */
      gl.uniform1i(this.locSemiPass, 1);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ONE);
      gl.depthMask(false);
      gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
      gl.depthMask(true);
      gl.disable(gl.BLEND);
      gl.uniform1i(this.locSemiPass, -1);
    } else {
      gl.uniform1i(this.locSemiPass, -1);
      gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
    }

    /* After-image trail: re-draw the mesh at each delayed pose, tinted and
     * additive (PSX ABE mode 1 - the retail arts after-image is a delayed
     * mesh copy drawn as a semi-transparent prim). Depth-tested against the
     * opaque pose but not depth-written, so echoes never occlude it. */
    const trail = this.ghostTrail;
    if (trail && trail.passes && trail.passes.length) {
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.ONE, gl.ONE);
      gl.depthMask(false);
      /* Strictly-nearer depth test: where an echo coincides with the live
       * pose (equal depth) it is rejected, so the character stays readable
       * and the tint only builds where the delayed pose has separated -
       * matching the retail look of a trail *behind* the motion. */
      gl.depthFunc(gl.LESS);
      gl.uniform1i(this.locSemiPass, -1);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.posBuf);
      for (const p of trail.passes) {
        if (!p.positions || p.positions.byteLength !== this.posByteLength) continue;
        gl.bufferSubData(gl.ARRAY_BUFFER, 0, p.positions);
        gl.uniform4f(this.locGhost, p.tint[0], p.tint[1], p.tint[2], p.alpha);
        gl.drawElements(gl.TRIANGLES, this.indexCount, gl.UNSIGNED_INT, 0);
      }
      gl.uniform4f(this.locGhost, 0, 0, 0, 0);
      gl.depthMask(true);
      gl.depthFunc(gl.LEQUAL);
      gl.disable(gl.BLEND);
      /* Put the live pose back - MeshView only re-uploads on frame change. */
      if (trail.restore && trail.restore.byteLength === this.posByteLength) {
        gl.bufferSubData(gl.ARRAY_BUFFER, 0, trail.restore);
      }
    }
    gl.bindVertexArray(null);
  }

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

  clearScene() {
    const gl = this.gl;
    for (const m of this.sceneMeshes.values()) {
      gl.deleteVertexArray(m.vao);
      gl.deleteBuffer(m.posBuf);
      gl.deleteBuffer(m.uvBuf);
      gl.deleteBuffer(m.ctBuf);
      if (m.flatBuf) gl.deleteBuffer(m.flatBuf);
      if (m.primRefBuf) gl.deleteBuffer(m.primRefBuf);
      gl.deleteBuffer(m.idxBuf);
      if (m.normBuf) gl.deleteBuffer(m.normBuf);
    }
    this.sceneMeshes.clear();
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
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

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
    /* Retail's per-primitive near reject (off unless the play page staged it). */
    if (this.locPrimNear) gl.uniform4fv(this.locPrimNear, this.primNear);
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
      /* The ground is environment: the occlusion fade may dissolve it. */
      gl.uniform1i(this.locOcclAllow, 1);
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
    let flatColorsOn = false;
    /* Which cue record is currently staged, so a frame with no per-draw cues
     * costs no extra uniform writes. `undefined` = the frame-global one. */
    let cueOn;
    /* Whether an object-effect clip is staged (`_setEffectClip`). */
    let eclipOn = this._setEffectClip(null, true);
    for (const [meshId, list] of byMesh) {
      const m = this.sceneMeshes.get(meshId);
      if (m.indexCount === 0) continue;
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
        /* Actor draws (the player, NPCs - `noOccl` on the placement) must
         * never dissolve; environment placements may. */
        gl.uniform1i(this.locOcclAllow, p.noOccl ? 0 : 1);
        eclipOn = this._setEffectClip(p.effectClip, eclipOn);
        gl.drawElements(gl.TRIANGLES, m.indexCount, gl.UNSIGNED_INT, 0);
      }
    }
    /* Hand the frame-global cue back before the blend pass / next frame. */
    if (cueOn !== undefined && cueOn !== this.cueParams) {
      this._setCue(this.cueParams);
      cueOn = this.cueParams;
    }

    /* Blend pass: re-draw the deferred semi-transparent (ABE) prims over the
     * finished opaque scene, one fixed-function blend state per ABR mode.
     * Depth-tested against the opaque scene but not depth-written, so blend
     * prims never occlude. Mirrors the retail GPU: the ordering table draws
     * blend prims against the already-rendered background. */
    let blendOn = false;
    let strictOn = false;
    for (const [meshId, list] of byMesh) {
      const m = this.sceneMeshes.get(meshId);
      if (!m || !m.semiRanges || m.indexCount === 0) continue;
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
        gl.uniform1i(this.locOcclAllow, p.noOccl ? 0 : 1);
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
    gl.bindVertexArray(null);
    /* Enhanced lighting's halos + light shafts over the finished scene. */
    this._drawGlow(vp);
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
    if ((p.anchor === 'centroid') && m.aabb) {
      return placementModelCentered(p.x, p.z, p.rotY || 0, scale, m.aabb);
    }
    if (p.y != null) {
      /* Walk-frame landmarks carry a world Y (floor-LUT height) so they
       * sit on the continent heightfield instead of the y=0 plane. */
      return placementModelScaledY(p.x, p.y, p.z, p.rotY || 0, scale);
    }
    return placementModelScaled(p.x, p.z, p.rotY || 0, scale);
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


  dispose() {
    const gl = this.gl;
    this.clearScene();
    if (this.ground) {
      gl.deleteVertexArray(this.ground.vao);
      gl.deleteBuffer(this.ground.posBuf);
      gl.deleteBuffer(this.ground.uvBuf);
      gl.deleteBuffer(this.ground.ctBuf);
      gl.deleteBuffer(this.ground.refBuf);
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
