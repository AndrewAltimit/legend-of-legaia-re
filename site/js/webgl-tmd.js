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
 *   4. webgl-tmd-frame-state.js     - TmdRenderer methods: fog / grade /
 *      depth-cue / lighting / occlusion / ocean state the frame draws with
 *   5. webgl-tmd-assembled-scene.js - TmdRenderer methods: scene meshes,
 *      the ground pass and renderAssembled
 *
 * Files 4 and 5 add methods to the class this file declares
 * (LegaiaUtil.installMethods, site-util.js). A page that loads this file
 * loads both, directly after it.
 *
 * Public API:
 *   const r = new TmdRenderer(canvas);
 *   r.uploadVram(vramBytes);                                   // 1MB Uint8Array
 *   r.uploadMesh(positions, uvs, cbaTsb, indices);
 *   r.render(yaw, pitch, center, radius);
 *   r.uploadSceneMesh(meshId, positions, uvs, cbaTsb, indices, flatRgba?);
 *   r.getMeshAabb(meshId);   // null until uploadSceneMesh has run
 *   r.setDistanceHaze({ enable, eyeZ, scale });               // optional
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
 *     hazeDeco - world-overview decoration stamp: under setDistanceHaze it
 *              takes one IR0 from its origin and hazes toward 0xD0.
 *     nclip  - per-draw NCLIP word (setNclipCull's encoding) overriding the
 *              frame's for this draw only - the battle bodies' single-sided
 *              rule (BattleActorDrawPlan::nclip_mode) over the both-sided
 *              battle pass.
 *     cue    - per-draw GTE depth cue { far: [r,g,b], nearZ, farZ, maxIr0 },
 *              overriding the frame-global `setDepthCue` for this draw only.
 *              The engine's `DrawCue` seam: the battle ground grid's per-stage
 *              far colour and the target-select cursor tint both ride it.
 *
 * Distance haze (setDistanceHaze({ enable, origin, eyeZ, scale })), the world
 * overview's stand-in for the overworld walk view's depth cue:
 *   enable  - draw it (renderAssembled only; every other page leaves it off).
 *   origin  - [x, y, z] virtual walk focus; the camera centre when omitted.
 *   eyeZ    - the walk eye's TRZ; scale - SZ per world unit of XZ distance.
 */

/* The overworld decoration cells' depth cue - the page twin of
 * legaia_engine_field::overworld_ground_cue::decoration_draw_cue. Retail's
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
    this.locFogEnable   = gl.getUniformLocation(this.program, 'u_fog_enable');
    this.locFogOrigin   = gl.getUniformLocation(this.program, 'u_fog_origin');
    this.locHazeEyeZ    = gl.getUniformLocation(this.program, 'u_haze_eye_z');
    this.locHazeScale   = gl.getUniformLocation(this.program, 'u_haze_scale');
    this.locHazeIr0     = gl.getUniformLocation(this.program, 'u_haze_ir0');
    this.locHazeFar     = gl.getUniformLocation(this.program, 'u_haze_far');
    this.locUseFlatColors = gl.getUniformLocation(this.program, 'u_use_flat_colors');
    this.locGhost   = gl.getUniformLocation(this.program, 'u_ghost');
    this.locGrade   = gl.getUniformLocation(this.program, 'u_grade');
    this.locCue     = gl.getUniformLocation(this.program, 'u_cue');
    this.locCueFar  = gl.getUniformLocation(this.program, 'u_cue_far');
    this.locPalette = gl.getUniformLocation(this.program, 'u_palette');
    this.locPairFront = gl.getUniformLocation(this.program, 'u_pair_front');
    this.locNclipCull = gl.getUniformLocation(this.program, 'u_nclip_cull');
    this.locCurve = gl.getUniformLocation(this.program, 'u_curve');
    this.locCurveBias = gl.getUniformLocation(this.program, 'u_curve_bias');
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
    /* Whole-draw far bucket (a placement's `farBucket`, the battle grid). */
    this.locFarSlice = gl.getUniformLocation(this.program, 'u_far_slice');
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
    this.overworldCurveBias = 0;
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

    /* Distance haze (setDistanceHaze); off on every page by default. */
    this.hazeParams = { enable: 0, origin: null, eyeZ: 0, scale: 0 };

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
    this.locOceanHaze       = gl.getUniformLocation(this.oceanProgram, 'u_haze');
    this.locOceanHazeOrigin = gl.getUniformLocation(this.oceanProgram, 'u_haze_origin');
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
